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

## Частичный фикс #2 (P3, 2026-09-29) — инкрементальные пост-layout коллекторы

Сделал инкрементальными все четыре пост-layout коллектора из предыдущего среза, плюс
`collect_scroll_containers_for_js_state`, которые до этого всегда гоняли по **всему** дереву
на каждый флаш вне зависимости от размера `dirty_roots`. Новые scoped-варианты в
`crates/engine/layout/src/lib.rs`: `collect_layout_rects_scoped`, `collect_client_rects_scoped`,
`collect_computed_styles_scoped` (единственный, кому нужен `GeomCtx` предков — сначала
спускается от реального корня до каждого искомого узла, потом обычная scoped-рекурсия),
`collect_scroll_containers_for_js_state_scoped`. Общий диспетчер — `find_dirty_root_boxes`
(один проход по дереву вместо N вызовов `find_box_by_node`, каждый из которых заново обходит
от корня). `FlushHandles::maybe_flush` (`style_flush.rs`) теперь, когда инкрементальный путь
сработал (`incr_scope = Some(...)`), вызывает scoped-варианты только для узлов из `dirty_roots`
вместо full-tree версий; иначе (`incr_scope = None`, т.е. сработал full-путь) — как раньше.

`try_incremental_flush` дополнительно возвращает `prev_node_ids`/`prev_node_raw_ids` —
множество id узлов, которыми задетые поддеревья владели в **предыдущем** (`basis.layout`)
дереве, до реструктуризации. Кэши (`layout_rects`/`client_rects`/`computed_styles`, ключ —
`NodeId::index()`; `scroll_states` отдельно, ключ — `NodeId::raw()`, т.е. с поколением) сперва
вычищают эти id, потом заполняются заново из свежего поддерева — иначе узел, удалённый мутацией
из DOM, остался бы в кэше с устаревшими данными навсегда.

Найденная и исправленная регрессия при первом проходе: `computed_styles` собирается лениво
(флаг `computed_styles_needed`/`computed_styles_collected` — первое чтение форсит полный сбор).
Флаш, который ПЕРВЫМ включает `computed_styles_needed`, не обязательно тот же флаш, у которого
непустой `dirty_roots` (BUG-935 S44 уже описывает этот сценарий: `.focus()`'s
scroll-into-view читает `_lumen_get_bounding_rect` и форсит реальный флаш, а `getComputedStyle`
взводит `computed_styles_needed` только на СЛЕДУЮЩЕМ флаше, где `dirty_roots` уже пуст —
ничего не поменялось с прошлого флаша). Scoped-сбор с пустым `dirty_roots` оставлял бы
`computed_styles` пустой картой навсегда. Поймано регрессионными тестами
`v8_bug560_sync_focus::get_computed_style_sees_same_tick_focus_call`/`_within` — оба упали на
`""` вместо ожидаемого цвета. Фикс: при первом сборе (`computed_styles_collected == false`)
всегда падать на полный `collect_computed_styles`, даже если инкрементальный путь сработал;
scoped-путь используется только начиная со второго и далее сбора.

**Измерено** (`--dump-layout` на том же локальном репро, N=1500/READS=200, без сети): до этого
среза `33360` мс, после — `20331` мс — заметное (~39%) улучшение, но кост всё ещё растёт с
размером скрипта/DOM (N=3000/READS=500 не укладывается в 60 с headless-таймаут). Оставшаяся
стоимость на N=1500/READS=200: pseudo-styles/custom-properties/text-frags коллекторы
(`collect_pseudo_computed_styles`, `collect_custom_properties`, `collect_text_frag_rects`)
намеренно остались full-document (гейтятся только своими `_needed`-флагами, как до этого среза
— они редкие чтения, не участвовали в замерах §Частичный фикс), а также сама O(depth)-стоимость
`find_dirty_root_boxes`/`collect_computed_styles_scoped`'s spine-walk на каждый флаш. Реальный
процент улучшения на настоящих сайтах (десятки тысяч узлов, cnn/udemy/dailymail) всё ещё не
измерялся — репродукция без сети не воспроизводит их (см. §Локализация).

Изменения этого среза: `crates/engine/layout/src/lib.rs` (`*_scoped` варианты четырёх
коллекторов, `find_dirty_root_boxes`, `find_box_by_node` уже существовал,
`collect_subtree_node_indices`/`collect_subtree_node_raw_ids`), `crates/js/src/v8_runtime/
style_flush.rs` (`try_incremental_flush` возвращает доп. eviction-множества, `maybe_flush`
разветвляется на scoped/full по `incr_scope`, `IncrFlushResult` типовой алиас для
`clippy::type_complexity`). Полный набор тестов `lumen-layout` (4112 passed, 1 ignored) и
`lumen-js` (4599 passed, `--features v8-backend`) зелёные, `cargo clippy -D warnings` на обоих
крейтах чисто, `graphic_tests/dump_golden.py` (12/12 PASS) — эта правка не затрагивает display
list/paint, дифф чисто в JS-видимых кэшах поверх уже построенного дерева layout, так что полный
20-минутный `graphic_tests/run.py` не запускался (см. правило "anything else → scoped-test +
dump_golden" в корневом `CLAUDE.md`).
Статус остаётся **OPEN** — квадратичность смягчена (два независимых среза), но не устранена:
следующий кандидат на профилирование — сам `find_dirty_root_boxes`/spine-walk (O(depth) на
коллектор на флаш) и/или реальный прогон на cnn/udemy/dailymail для измерения итогового эффекта.


## Срез 2026-10-01 (P1, BUG-935 срез 53) — замер без сети и реальный след

Стенд `scripts/perf-fixtures/bug935_forced_reflow_stand.html` (1500 div, `--dump-layout`)
воспроизводит квадратичность без внешнего сайта: чтение геометрии на чистом DOM 0,005–0,06
мс/оп, а `el.style.width=…; el.offsetWidth` — 150–400 мс за цикл. `LUMEN_FRAME_LOG=1`:
`maybe_flush done … path=incremental dirty_roots=1` — путь инкрементальный, но единственный
корень — родитель всех 1500 элементов (`NodeChange::Unattributed` расширяется до родителя),
из ~155–185 мс на флаш раскладка корня 110–140 мс, коллекторы ~40 мс, `restyle_node_index`
и поиск корней ≈ 0. Живой след на lenta.ru (`LUMEN_JS_STALL_SAMPLE_MS`, см. BUG-935
срез 53): 41 принудительный флаш за 75 с, баннерный цикл `_saveBannerSizes` — 46 % занятого
времени движкового потока. Кандидат: атрибуция `style`-мутаций (узкий корень = сам узел).

## Срез BUG-935 S54 (P1, 2026-10-01) — атрибуция `style`/`setAttribute` сужает корень

`DomTouched::attr_gen`/`structural_gen` + `NodeChange::Attr` в `try_incremental_flush`:
`style.width=…; offsetWidth` на стенде 1500 div — 388 → 97 мс/цикл (`dirty_roots` = сам узел).
Остаток — линейные по документу раскладка (~55 мс) и коллекторы (~20 мс); подробности —
[BUG-935 срез 54](BUG-935-OPEN.md). Баг остаётся OPEN (частично).

## Срез BUG-935 S55 (P1, 2026-10-01) — журнал содержимого документа

`ContentDirty::Nodes(journal)` вместо `Untracked`: `build_box` 26 → ~1 мс, `graft_geometry`
23–31 → 0,05 мс на стенде 1500 div, цикл `mutate+read` ~110 → ~30–45 мс. Подробности —
[BUG-935 срез 55](BUG-935-OPEN.md). Попутно найден независимый дефект scoped-коллекторов —
[BUG-1238](BUG-1238-FIXED.md) (следствие «post-collectors» этого бага: сдвинутые соседи получают
устаревший `getBoundingClientRect`). BUG-1211 остаётся OPEN.

## Срез BUG-1238 (P1, 2026-10-02) — коллекторы заменены планом

`collect_*_scoped` по `dirty_roots` удалены: они оставляли устаревшие записи у сдвинутых
соседей и у предков корня ([BUG-1238](BUG-1238-FIXED.md)). Их заменил
`lumen_layout::ScopedCollection` (`scoped_collect.rs`) — обход свежего дерева с отсечением по
`clean_subtrees` и совпавшему rect; карту `computed_styles` чисто вертикальный сдвиг не
затрагивает. Строка `maybe_flush done` теперь печатает время rect-коллекторов.

## Срез 2026-10-07 (P3) — `:has()` и `:nth-child(… of S)` больше не расширяют рестайл вслепую

Замер на текущем `main` (dev-release, `--dump-layout`, `LUMEN_NO_ADBLOCK=1`): локальный репро
N=1500/READS=200 — 3,1 с (было 33 с), N=3000/READS=500 — 9,6 с (было «не завершается за 60 с»);
udemy — 7,8 с, готов; dailymail — отдаёт страницу за 2 с; **cnn — по-прежнему не завершался за 100 с**.
`LUMEN_JS_STALL_SAMPLE_MS` на cnn показал другой механизм, чем в §Локализация: скрипт Zion ставит ~30
`data-zjs-*` на ссылку и читает `innerText` после каждой, а каждый такой принудительный флаш стоил ~3 с
(`maybe_flush done … path=incremental dirty_roots=36 planned=3939`, пересчёт 3743 элементов,
`forced_same` = все). Причины — две, обе в `crates/engine/layout/src/style/restyle.rs`:

1. **`:has()` без учёта имени атрибута.** `has_reach_roots` звался на каждую запись и добавлял корнями
   всех предков, способных совпасть со subject-компаундом с `:has()` (на cnn — `html`, `body`, `header`,
   секции…), даже если запись — `data-zjs-*`, которого не читает ни один аргумент `:has()`. Теперь
   `NodeRestyleIndex::change_can_flip_has` пропускает запись, если имя/токены не названы в аргументах
   `:has()` (`has_deps`): `data-*`/`aria-*`/`style` — по имени, `class`/`id` — по изменившимся токенам и по
   результату атрибутных селекторов `[class*=…]` для старого и нового значения (`attr_selector_may_flip`).
2. **`:nth-child(2 of .zone)` в таблице стилей cnn включал `conservative` для всего листа** — отключалось
   всё сужение (shallow/point-корни, `attr_change_stays_local`, `affected_descendants`), и запись `class` на
   большом `div` пересчитывала его поддерево (5956 узлов, ~2 с на флаш, 9 раз подряд). Теперь `S` из
   одного компаунда (тип/класс/id/атрибут) моделируется: запись, способная перевернуть `S` на узле
   (`attr_change_hits_nth_of`), берёт родителя целиком (позиции соседей с обеих сторон), остальные записи
   сужаются как обычно. Более сложный `S` (псевдокласс, комбинатор) по-прежнему включает `conservative`.

Попутно атрибутные селекторы `class`/`id` в позиции предка (`[class*="open"] p`) больше не считаются
читающими любую запись: `AncestorDeps::attr_sels` сравнивает совпадение на старом и новом значении
(`attr_value_matches` вынесен из `matching.rs`), так что `affected_descendants` и
`attr_change_stays_local` сужают и их.

cnn после среза: заканчивается за ~78 с (был ≥238 с без конца), флашей >1 с — 18 из 157 (было 28; суммарно
63 с против 84 с). Остаток — не квадратичный цикл, а пачки структурных мутаций страницы (скрипты,
`ChildList`, `inert`, `style` на `body`) с несколькими глубокими корнями: каждый пересчитывает 2–4 тыс.
элементов по ~0,5 мс (`CascadeStats.walk_ns`, `forced_same` = все) — это стоимость самого каскада на огромном
листе cnn и ветка BUG-935 («forced» потомки), а не `maybe_flush`. Баг остаётся OPEN (частично).
