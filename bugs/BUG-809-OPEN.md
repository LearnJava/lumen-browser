# BUG-809 — Layout Instability объявлен, но ни одна запись `layout-shift` не доставляется: шелловский триггер `deliver_layout_shift` не вызывается ниоткуда

**Статус:** OPEN (ДОРАБОТКА → [GAP-LAYOUTSHIFT](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-LAYOUTSHIFT` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 17 — категория `layout-instability`, 35 TIMEOUT из 37 прогнанных, 94.6 %)
**Область:** `crates/shell/src/main.rs:2925` (объявление `deliver_layout_shift` в трейте, помечено `#[allow(dead_code)]`), `crates/shell/src/main.rs:3359` (реализация — зовёт JS-хук), `crates/js/src/dom.rs:11035` (`_lumen_deliver_layout_shift`), `crates/js/src/dom.rs:10907` (`_PERF_SUPPORTED_ENTRY_TYPES`, где `layout-shift` объявлен поддерживаемым)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Страница подписывается на `layout-shift`, двигает блок и ждёт запись —
которая не приходит никогда. Страница при этом жива, исключений нет,
`supportedEntryTypes` уверяет, что тип поддержан:

```js
// layout-instability/simple-block-movement.html, сокращённо
const watcher = new ScoreWatcher;              // не бросает: тип «поддержан»
promise_test(async () => {
  await waitForAnimationFrames(2);
  document.querySelector("#shifter").style = "top: 160px";   // сдвиг 300×200 на 160px
  await watcher.promise;                       // ← висит до таймаута враннера
}, 'Simple block movement.');
```

Ровно это объявление и превращает категорию в TIMEOUT, а не в FAIL:
`ScoreWatcher` (`layout-instability/resources/util.js`) первой строкой
проверяет `PerformanceObserver.supportedEntryTypes.indexOf("layout-shift")`
и бросает `Error("Layout Instability API not supported")`, если типа нет.
Честный ответ «не поддерживаю» дал бы быстрый провал; ложноположительный
даёт зависание. Тот же класс расхождения, что закрытый
[BUG-354](BUG-354-FIXED.md) («геттер обещает пять типов, которых нет»), — но
здесь тип из списка не убрали, потому что доставка *почти* есть.

## Прямое измерение

`tests/wpt/verify_layout_shift_and_peer_gaps.py` (живое окно, http, улики из
stderr браузера; dev-release, Linux, 2026-08-21, коммит `79ea47826`,
`--seconds 8`; 15 тиков `setInterval` — страница жива всё это время):

| проба | получено |
|---|---|
| `cls-feature-detect` | `supported=…,layout-shift,…`, `LayoutShift=undefined`, `LayoutShiftAttribution=undefined`, `observe-ok` |
| `cls-shift` — наблюдаем, ждём 2 кадра, двигаем блок 300×200 на 160px | только `shifted`; **записи нет** |
| `cls-shift-buffered` — сдвиг до создания наблюдателя, `{type, buffered: true}` | только `shifted`; **буфер пуст** |
| `cls-attribution` — читаем `entry.sources[0].node` | только `shifted`; колбэка нет вовсе |

## Причина (локализована чтением кода)

Цепочка доставки построена целиком, но у неё нет входа:

* `_lumen_deliver_layout_shift(value, session_id, had_input)`
  (`crates/js/src/dom.rs:11035`) собирает запись и рассылает наблюдателям —
  рабочий код;
* шелловский `deliver_layout_shift` (`crates/shell/src/main.rs:3359`) зовёт
  этот хук — рабочий код;
* объявление того же метода в трейте (`main.rs:2925`) помечено
  `#[allow(dead_code)]`, и `grep -rn deliver_layout_shift crates/` даёт
  ровно шесть совпадений: объявление, реализация, JS-хук, заглушки в
  `driver`/`winit_session`/`core::ext` — **и ни одного вызова из layout или
  reflow**. Никто не считает сдвиги и никто не зовёт триггер.

Дополнительно отсутствуют `window.LayoutShift` и `LayoutShiftAttribution`
(тип записи не веб-видим), а `sources` в `_lumen_deliver_layout_shift`
захардкожен пустым массивом — то есть даже после включения триггера
`sources.html`/`attribution-*.html` останутся красными, но уже как FAIL.

Всё это было замечено ещё при вендоринге категории
(`WPT-VENDOR-layout-instability`, 2026-08-05: «Rust-триггер помечен
`#[allow(dead_code)]` и нигде не вызывается»), но номера тогда не получило —
заводится сейчас, когда измерена корпусная цена.

## Масштаб

Механизм `layout-shift-never-delivered` в `tests/wpt/timeout_audit.py`
забирает **35 id** остатка снимка WPT-RUN-5 — всю неразобранную часть
категории `layout-instability` (35 из 37 её TIMEOUT; оставшиеся два
объяснены `iframe-no-nested-context` и `helper-404`). Это самая плотная
категория остатка на момент среза 17: 94.6 % её таймаутов — один этот
дефект. Остальные ~29 FAIL категории (по прогону вендоринга) — известные
маски [BUG-384](BUG-384-FIXED.md)/[BUG-525](BUG-525-FIXED.md) и
неимплементированные testdriver-экшены ([BUG-810](BUG-810-FIXED.md)).

Цена шире WPT: CLS — одна из трёх метрик Core Web Vitals, и любая
RUM-библиотека (web-vitals.js и производные) на Lumen сегодня получает
`supportedEntryTypes`, включающий `layout-shift`, подписывается и молча
не получает ничего — то есть считает CLS равным нулю, а не «не измеримо».

## Направление починки (не предписание)

Считать сдвиг там, где вёрстка уже пересчитывается: у релэйаута есть и
старые, и новые прямоугольники, а `deliver_layout_shift` ждёт готовую
дробь. Минимальный полезный шаг — доля площади сдвинувшихся элементов на
нормированное расстояние (Layout Instability §3), с `had_input` из
недавнего пользовательского ввода. Веб-видимые `LayoutShift`/
`LayoutShiftAttribution` и непустой `sources` — отдельный, следующий шаг:
без них тесты перестанут виснуть и станут честно падать.

Альтернатива, если считать сдвиги пока не планируется: убрать
`layout-shift` из `_PERF_SUPPORTED_ENTRY_TYPES` (`dom.rs:10907`). Это не
починка API, но она немедленно превращает 35 зависаний в 35 быстрых
провалов с внятной причиной и снимает ложноположительный feature-detect для
любой RUM-библиотеки — ровно то решение, которое уже принято в BUG-354 для
пяти других типов.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_layout_shift_and_peer_gaps.py
   --variant cls-shift` печатает `cls-entry value=…`.
2. `--variant cls-shift-buffered` печатает `cls-buffered-entries=1` и больше.
3. WPT: `run_report.py --all --root layout-instability --recursive` — 35
   TIMEOUT уходят; часть тестов станет FAIL (пустой `sources`,
   отсутствующий `LayoutShift`), и это ожидаемый промежуточный результат.

**Обновление 2026-09-22 (GAP-LAYOUTSHIFT срез 1, P6):** доставка подключена.
`compute_layout_shift_score` (`crates/shell/src/relayout.rs`) считает
`impact_fraction × distance_fraction` из диффа двух снимков
`collect_layout_rects` — приближение к Layout Instability L1 §3 (сумма
клипованных площадей вместо точного объединения непересекающихся
прямоугольников; см. doc-комментарий на функции для точной формулировки
расхождения). Вызывается из `apply_relayout_result`, единственной общей
точки всех продюсеров relayout-а (`relayout()`, `try_relayout_raf_incremental`,
`poll_engine_commit`, streaming layout) — тот самый пробел, который
изначально не давал триггеру сработать ни разу. База для диффа
(`Lumen::prev_layout_shift_rects`) заводится не только там: без семени на
«первом осевшем кадре» страницы (`reload()`, оба JS-push блока
`apply_loaded_page`, hibernate-восстановление) первый relayout после
загрузки сравнивал бы новые rects с пустым снимком и всегда получал 0 — то
есть страница бы жила, но CLS оставался мёртв ещё раз, только тише. `had_input`
(`hadRecentInput`) — новое поле `Lumen::last_input_epoch_s`, обновляется на
каждом нажатии мыши/клавиши (`on_mouse_input`/`handle_key`), сравнивается с
окном 500мс.

Живой замер (`verify_layout_shift_and_peer_gaps.py`, dev-release, Windows,
2026-09-22): `cls-shift` печатает `cls-entry value=0.0127…` вместо
зависания. `cls-attribution` печатает `cls-source node=none` — честный
быстрый FAIL вместо TIMEOUT: `sources` заполняется по-прежнему пустым
массивом, атрибуция элемента остаётся следующим шагом (симптом раздел уже
это предсказывал).

**Не в этом срезе:** `cls-shift-buffered` всё ещё виснет. Тот пробник
двигает блок **синхронно** в `<script>` страницы, до того как первое
семя baseline'а успевает осесть — диффить не с чем, счёт честно 0 (то же
самое, что реальный браузер не засчитывает сдвиг до первого paint, но
WPT-хелпер `buffered-flag.html` всё равно ждёт запись). `window.LayoutShift`/
`window.LayoutShiftAttribution` конструкторы по-прежнему не веб-видимы.
Статус GAP-LAYOUTSHIFT остаётся `planned`.

**Обновление 2026-09-22 (GAP-LAYOUTSHIFT срез 2, P6):** `entry.sources[]`
теперь непустой. `compute_layout_shift_score` возвращает `LayoutShiftResult
{ score, sources }` — `sources` ранжирует сдвинувшиеся узлы по их
собственной клипованной площади (largest first, capped at 5 — §4.2 «at most
five largest»); это приближение к точному алгоритму спеки (который ранжирует
по *объединённому* вкладу узла в общий регион), но узел в `sources[0].node`
теперь реальный элемент, а не пустой массив. `window.LayoutShift`/
`window.LayoutShiftAttribution` веб-видимы (`window.LayoutShift = LayoutShift`
в `web_api_shim_tail_mc.js`), и `_lumen_deliver_layout_shift` строит запись
через настоящий конструктор вместо голого литерала.

Живой замер (`verify_layout_shift_and_peer_gaps.py`, dev-release, Windows,
2026-09-22): `cls-attribution` печатает `cls-source node=shifter` вместо
`node=none`.

**Не в этом срезе:** `LayoutShiftAttribution.previousRect`/`currentRect`
остаются `null` — движок пока не прокидывает пред-/пост-сдвиговую геометрию
узла через `deliver_layout_shift`, только его идентификатор. `cls-shift-buffered`
всё ещё виснет (см. срез 1). Статус GAP-LAYOUTSHIFT остаётся `planned`.

**Обновление 2026-09-22 (GAP-LAYOUTSHIFT срез 3, P6):** `previousRect`/
`currentRect` теперь реальная геометрия, а не `null`. `LayoutShiftResult::sources`
(`crates/shell/src/relayout.rs`) — раньше просто `Vec<u32>` — стал
`Vec<LayoutShiftSource>` (`node` + `previous_rect`/`current_rect`, оба в
формате `collect_layout_rects` `[x, y, w, h]`, взяты из того же диффа
`prev`/`next`, что уже считал score). `PersistentJs::deliver_layout_shift`
сериализует их JS-литералом `{nid, prev:[...], curr:[...]}`; шим
(`_lumen_deliver_layout_shift`, `web_api_shim_tail.js`) строит из каждой пары
настоящий `DOMRectReadOnly` (`geometry_shim.js`, уже веб-видим) вместо
подставленного `null`.

Живой замер (`verify_layout_shift_and_peer_gaps.py`, расширен печатью
`cls-rects`, dev-release, Windows, 2026-09-22): `cls-attribution` печатает
`cls-rects prev=8,200 curr=168,200` — дельта `top` (168−8=160) совпадает с
реальным сдвигом `style.top = "160px"` пробника, `height` (200) не меняется,
как и должно быть при чисто вертикальном сдвиге.

**Не в этом срезе:** `cls-shift-buffered` всё ещё виснет (см. срез 1) — не
входная геометрия, а таймінг относительно первого осевшего кадра. Rect'ы не
клипуются к вьюпорту на JS-стороне (спека не требует — `DOMRectReadOnly` тут
border box в layout-пространстве, как и `getBoundingClientRect`). Статус
GAP-LAYOUTSHIFT остаётся `planned`.

**Обновление 2026-09-22 (GAP-LAYOUTSHIFT срез 4, P6):** `cls-shift-buffered`
починен. Пайплайн парсинга уже считал полный pre-script layout ради
`getBoundingClientRect()`/`getComputedStyle()` внутри синхронных скриптов
(BUG-443/FONTLOAD-4/CSSOM-7, `parse_time_snapshot` в
`page_pipeline.rs::parse_and_layout`) — его геометрия (`JsLayoutSnapshot::rects`,
тот же `collect_layout_rects`, что и весь остальной CLS-код) просто не
переживала эту функцию. `prescript_layout_rects` теперь клонируется из
`parse_time_snapshot` ДО того, как он уезжает по значению в
`run_scripts_with_dom`, и прокидывается через `ParsedPage`/`LoadedPage`
(`page_pipeline.rs`) до `apply_loaded_page` (`page_load.rs`). Там, на первом
осевшем кадре страницы, `compute_layout_shift_score` один раз диффит этот
pre-script снимок против финальной пост-скриптовой геометрии (`rects`,
которым уже сеется `prev_layout_shift_rects`); при ненулевом счёте
`deliver_layout_shift(score, sources, had_input=false)` зовётся из того же
`route_task_js`-замыкания, что и остальной push геометрии на этой точке —
`had_input=false` безусловно, потому что загрузка страницы по определению
предшествует любому пользовательскому вводу. `_lumen_deliver_layout_shift`
кладёт запись в `_perf_entries` до уведомления наблюдателей, поэтому она
достаётся `buffered: true` подпиской, даже если та появилась позже.

Живой замер (`verify_layout_shift_and_peer_gaps.py`, dev-release, Windows,
2026-09-22): `cls-shift-buffered` печатает `cls-buffered-entries=1` вместо
голого `shifted` — не FAIL, а PASS-форма. `cls-shift`/`cls-attribution`
(срезы 1-3) не регрессировали — тот же вывод, что и раньше.
`run_report.py --all --root layout-instability --recursive`: 71/82 harness OK
(было 53/82 в заметке о вендоринге), 21/113 сабтестов (было 17/82 при
вендоринге, другой знаменатель из-за `--recursive`) — заметное улучшение, но
категория не закрыта: остаток — approximation-gaps самой формулы
`compute_layout_shift_score` (например `translate-change.html`/
`visibility-hidden.html` ожидают счёт `0` для трансформаций/`visibility`,
которые не должны считаться сдвигом по L1 §3, а наша функция считает их по
голым border-box координатам) — отдельная задача, не эта. Статус
GAP-LAYOUTSHIFT остаётся `planned`.

**Обновление 2026-09-22 (GAP-LAYOUTSHIFT срез 5, P6):** два approximation-gap'а
из среза 4 закрыты. Новая `lumen_layout::collect_layout_shift_rects`
(`crates/engine/layout/src/lib.rs`) — geometry-снимок специально для
`compute_layout_shift_score`, отдельный от `collect_layout_rects`, который эту
формулу до сих пор кормил тем же снимком, что идёт в `getBoundingClientRect`:

* не применяет `forward_box_transform` (собственный CSS `transform`/
  `translate` узла) — composited-трансформация двигает paint-вывод, а не
  layout-бокс, который пересчитал бы релэйаут (`translate-change.html`
  ожидает счёт `0` для правки `translate`);
* пропускает узлы с `style.visibility != Visible` — скрытый элемент не
  отрисован, а L1 §5.2.4 засчитывает только отрисованные (`visibility-hidden.html`
  ожидает счёт `0` для правки `top` под `visibility: hidden`). Узел,
  скрытый и в prev-, и в next-снимке, просто отсутствует в обеих картах, и
  `compute_layout_shift_score`'s `prev.get(node)` промах трактует его как
  «вошёл/вышел из дерева» — не сдвиг, тот же путь, что уже обрабатывает
  `display: none`.

Все четыре точки посева `prev_layout_shift_rects`/`prescript_layout_rects`
(`relayout.rs::apply_relayout_result`, `page_load.rs::reload`/
`apply_loaded_page`/hibernate-восстановление, `page_pipeline.rs::
collect_js_layout_snapshot`'s pre-script снимок) переведены на новую функцию;
`rects`, что уходит в JS (`update_layout_rects`), не тронут — там by design
нужна gBCR-геометрия с трансформацией.

Живой замер (`verify_layout_shift_and_peer_gaps.py`, расширен вариантами
`cls-translate`/`cls-visibility-hidden`, dev-release, Windows, 2026-09-22):
обе новые пробы печатают `no-entry=true` вместо `cls-entry value=…` — сдвиг
больше не засчитывается. `cls-shift`/`cls-shift-buffered`/`cls-attribution`
(срезы 1-4) не регрессировали — тот же вывод, что и раньше.

**Не в этом срезе:** формула остаётся приближением (сумма клипованных
площадей вместо объединения непересекающихся регионов, см. doc-комментарий
`compute_layout_shift_score`) — остаток `translate`/`visibility`-класса
исчерпан, но не весь класс approximation-gap'ов спеки (например `opacity`,
`content-visibility`, scroll-driven сдвиги — не измерены в этом срезе).
Статус GAP-LAYOUTSHIFT остаётся `planned`.
