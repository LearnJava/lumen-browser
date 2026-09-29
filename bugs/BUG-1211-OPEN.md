# BUG-1211 — cnn, udemy, dailymail: движковый поток занят минутами, страница не доходит до готовности

**Статус:** OPEN
**Компонент:** js / `crates/js/src/v8_runtime/style_flush.rs:176` (`FlushHandles::maybe_flush`) — локализовано
**Найден:** P2, повторный прогон top100, 2026-09-28
**Локализовано:** P3, 2026-09-29 (см. §Локализация)

## Симптом

dev-release `8bd5c5dd1`, без блокировщика, чистый `data/`.

| Сайт | 09-23 (`c05801655`) | 09-28 прогон 1 | 09-28 прогон 2 | headless `--dump-layout`, 200 с |
|---|---|---|---|---|
| edition.cnn.com | TIMEOUT, CPU занят 249 с | HUNG, занят 238 с | HUNG | не завершился (6 ответов) |
| www.udemy.com | BROKEN_RENDER, 42 с | HUNG, занят 136 с, 2.3 ГБ | HUNG | не завершился (39 ответов) |
| www.dailymail.co.uk | TIMEOUT (сеть) | HUNG, 1.35 ГБ | HUNG, «Не отвечает» 70 с | — |

`HUNG` в `perf_audit.py` — `Wait error: automation command timed out`: MCP не получает ответ от
движкового потока. На cnn сеть к этому моменту стоит (6 ответов, последний — шрифты), последняя строка
stderr — `[JS] … storeTid: no storage consent, clearing stored TID` после загрузки
`cdn.optimizely.com/…/landingprod.js`; процесс держит ~150 % CPU. Headless воспроизводится и через
VPN-туннель без прокси — это не сеть. У Chrome 153 cnn и dailymail не открылись в замере 09-23
(таймаут), udemy — `load` 6.3 с.

## Что сделать

Снять стек движкового потока в момент зависания (cnn — самый быстрый репро, ~10 с после старта) и
свести к локальной странице. BUG-306 (github) — другой механизм (память), не этот.

## Локализация

Репро: `lumen.exe --dump-layout https://edition.cnn.com/` (dev-release, `LUMEN_NO_ADBLOCK=1`) виснет
~10-15 с после старта, последняя строка stderr — `[JS] … storeTid: no storage consent, clearing
stored TID` (после загрузки `cdn.optimizely.com/…/landingprod.js`), процесс держит ~415 МБ RSS и не
завершается за 200+ с. `lldb -p <pid>` (venv `python3.dll` из `Python314` нужно добавить в `PATH` —
штатный `lldb.exe` иначе падает `Can't open python3.dll`) на зависшем процессе:

- Главный поток (thread #1) блокирован в `std::sync::mpsc::Receiver::recv` внутри
  `V8JsRuntime::run` → `eval_and_report` → `lumen::scripts::collect_scripts_ordered` —
  ждёт ответа от JS/движкового потока синхронно исполняемого скрипта (штатно для
  `--dump-layout`, который гоняет скрипты последовательно).
- JS-поток (thread #11, `v8_thread_main`) в момент снятия стека **не в V8**, а в чистом Rust: он
  вызван из нативного колбэка `install_computed_styles::{closure#0}` (`platform.rs:931`,
  `_lumen_get_computed_style`) → `FlushHandles::maybe_flush` (`style_flush.rs:284`) →
  `lumen_layout::layout_measured_with_counters` → `precompute_counters` (`counters.rs:668`) →
  `layout_measured_hyp_with_counters` → **рекурсия `parse_counter_style_descriptors` глубиной
  29 кадров** (`counters.rs:1612`, через `ShareCache::compute`/`compute_style_shareable`).

Причина зависания — не рекурсия сама по себе (глубина 29 разумна для реального дерева `@counter-
style`), а то, **что этот полный (не инкрементальный) relayout запускается синхронно на JS-потоке
из каждого `getComputedStyle`/`offsetWidth`-подобного чтения** (`FlushHandles::maybe_flush`,
CSSOM-4/BUG-493), если с прошлого флаша что-то помечено грязным (`flush_stale`/`sheet_sync.dirty`/
`computed_styles_needed` и т.п.). `landingprod.js` (Optimizely) в паттерне read-after-mutate дергает
`getComputedStyle` в цикле (типичный A/B-тест, меряющий геометрию множества узлов) — на странице
cnn.com с её огромным DOM (десятки тысяч узлов, докрутка ленты статей) каждый такой вызов стоит
полный `layout_measured_with_counters` по всему документу вместо `x-scope-relayout`-подобной
точечной перекладки. Один вызов уже недёшев (наблюдалось 146–2600 мс на full-relayout, BUG-935
S3/S5) — цикл из сотен/тысяч таких вызовов даёт минуты занятости при 100 % одного ядра, что и
маскируется в `perf_audit.py`/headless как «движковый поток не отвечает»: он отвечает, но каждый
ответ стоит один полный relayout, и синхронный API (`eval`/`wait document_ready`) не может вклиниться
между вызовами скрипта.

Не багфикс сам по себе (нет одной точечной правки без риска регресса `CSSOM-4`/`BUG-493`'s
same-tick-consistency контракта) — заводится как продолжение: `maybe_flush` нужен либо дебаунс/
коалессинг повторных вызовов в пределах одного скрипт-тика (сейчас каждый `getComputedStyle`
форсит новый `layout_measured_with_counters`, даже если между вызовами ничего не менялось —
`flush_stale`/`*_needed`-флаги гейтят *что* собирать, а не частоту самого relayout при их
многократной установке одним и тем же скриптом), либо инкрементальный путь (`RestyleDelta`/
`incremental_precompute_counters`, уже существующий для движкового потока в
`crates/engine/layout/src/counters.rs`) вместо полного `layout_measured_with_counters`. См. `docs/
engine-gaps.md`/`style_flush.rs`'s собственные заметки о том, что этот флаш **намеренно** не идёт
через движковый поток (риск дедлока ADR-016) — так что фикс не может просто «переслать это в
EngineThread», нужна отдельная инкрементальная модель для same-tick flush.

## Сведено к локальной странице

Минимальный репро (без сети, `--dump-layout`), сохранён как
`samples/bug1211-flush-quadratic/repro.html`, воспроизводит тот же паттерн read-after-mutate,
которым `landingprod.js` бьёт по `getComputedStyle` на cnn.com:

```html
<!DOCTYPE html>
<html><head><style>
div { counter-reset: c; }
div::before { counter-increment: c; content: counter(c); }
</style></head>
<body>
<div id="root"></div>
<script>
var root = document.getElementById('root');
for (var i = 0; i < 3000; i++) {
  var d = document.createElement('div');
  d.textContent = 'x';
  root.appendChild(d);
}
for (var i = 0; i < 500; i++) {
  var el = root.children[i % root.children.length];
  el.style.color = (i % 2) ? 'red' : 'blue';
  var cs = window.getComputedStyle(el);
  var v = cs.color;
}
</script>
</body></html>
```

`lumen --dump-layout file:///…/repro1211_local.html` с `N=3000` узлами / `500` итераций не
завершается за 60 с (каждая итерация форсит `maybe_flush` → полный
`layout_measured_with_counters` по всем 3000 узлам — `O(reads × nodes)`, а не `O(reads +
nodes)`). При `N=300`/`50` та же страница укладывается в 1.7 с — подтверждает, что зависание не
бесконечный цикл/рекурсия, а квадратичный рост стоимости same-tick flush от размера DOM,
помноженный на число read-after-mutate вызовов скрипта. Это и есть минимальная репродукция
BUG-1211 — сеть/CDN cnn.com не нужны, только большой DOM + скрипт, чередующий мутацию стиля и
`getComputedStyle` в цикле (ровно то, что делает Optimizely `landingprod.js`).

## Частичный фикс (P3, 2026-09-29) — инкрементальный cascade+layout в `maybe_flush`

`FlushHandles::maybe_flush` теперь сначала пробует `try_incremental_flush` — тот же
`RestyleDelta`/`restyle_root_set_for_node_change`/`layout_mutation_incremental_restyle`
(`crates/engine/layout/src/counters.rs`, `box_tree/entry.rs`), которым уже пользуется
`crates/shell/src/relayout.rs` для JS-мутаций на chrome-стороне. Базис для инкремента —
`FlushHandles::incr_basis` (`IncrFlushBasis`: предыдущее дерево/каскад + viewport/sheet revision/
focus в момент публикации), обновляется после каждого успешного флаша (и full, и incremental).
`DomTouched` (`crates/js/src/v8_runtime/runtime.rs`) получил `touch_gen: HashMap<NodeId, u64>` —
per-node генерацию последнего касания, потому что `touched.nodes`/`epoch` этот флаш **не
дренирует** (дренаж — исключительно право `V8JsRuntime::take_dom_touched`, страничного
rAF-конвейера; если бы same-tick flush тоже дренировал, следующий цикл `try_relayout_raf_
incremental` не увидел бы уже обработанные мутации и форсил бы там полный cascade). Без
`touch_gen` любой диф против уже накопленного (никогда не очищаемого) `touched.nodes` либо
неверно схлопывает повторные мутации одного узла между двумя флашами (см. правку в этом же
коммите — первая версия сравнивала со снапшотом `HashSet`, не с генерацией), либо вообще не
сужает набор.

**Измерено** (`--dump-layout` на локальном репро без сети, тот же паттерн, N=1500/READS=200):
до фикса `[JS] loop took ms: 31054`, после — `33360` (в пределах шума). Инкрементальный путь
реально включается (проверено отладочным `eprintln`, убран из финального коммита) и сокращает
сам cascade+layout до микросекунд на каждый повторный флаш — но **общее время не улучшилось**,
потому что доминирующей стоимостью оказались не cascade/layout, а четыре пост-layout коллектора
`maybe_flush` всегда гоняет по всему дереву заново на **каждый** флаш вне зависимости от размера
дельты: `collect_layout_rects`, `collect_client_rects`, `collect_computed_styles`,
`collect_scroll_containers_for_js_state` (все — `crates/engine/layout/src/lib.rs`). Замер на
N=1500/READS=5: `restyle+layout` — 2-12 мкс на повторный флаш (инкремент работает), но
`post-collectors` — стабильно ~90-120 мс на каждый флаш (полный проход по всем узлам). Это и есть
следующий шаг — сделать эти четыре коллектора инкрементальными (обновлять только записи для
`dirty_roots`/задетых поддеревьев, а не пересобирать всю `HashMap`/`Vec` с нуля), иначе выигрыш от
инкрементального cascade+layout полностью съедается этими проходами и на реальных сайтах
(cnn/udemy/dailymail, десятки тысяч узлов) BUG-1211 не закрывается — цикл `read-after-mutate` всё
ещё будет стоить `O(reads × nodes)` через эти четыре функции.

Изменения этого среза: `crates/js/src/v8_runtime/style_flush.rs` (`FlushHandles::incr_basis`/
`IncrFlushBasis`, `try_incremental_flush`), `crates/js/src/v8_runtime/runtime.rs`
(`DomTouched::touch_gen`), `crates/js/src/v8_runtime/dom_helpers.rs` (`record_dom_touch` пишет
`touch_gen`), `crates/js/src/v8_runtime.rs` (конструктор `FlushHandles` — новые поля). Полный
набор тестов `lumen-js` (4599, `--features v8-backend`) и clippy (`-D warnings`) зелёные.
Статус остаётся **OPEN** — квадратичность по факту не устранена, только частично (cascade/layout
часть), реальный процент улучшения на настоящих сайтах не измерялся (эмулятор не воспроизводит
без сети — см. §Локализация).


