# BUG-1468 — `position: sticky`: в корневом документе и внутри `overflow` контейнера липкий бокс не следует за прокруткой (`getBoundingClientRect` и снимок)

**Статус:** FIXED 2026-11-18 (P3, срезы 1–5)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout/paint/shell (`crates/engine/paint/src/display_list/walk.rs:1056` — `BeginStickyLayer`; прокрутка вложенного контейнера в `crates/shell`)

## Симптом

Липкий бокс остаётся в потоке: после `scrollTop = 250` у контейнера с `overflow:auto` `getBoundingClientRect().top` липкого элемента не меняется (200 до и после при `top:100px`, ожидается 100), а на снимке `--screenshot` липкий зелёный квадрат в прокрученном контейнере стоит на позиции потока. Разные поверхности ведут себя по-разному: `--dump-display-list` на странице с `position:sticky` не выдаёт `BeginStickyLayer` вовсе (пишет обычный `FillRect`; тест `sticky_top_emits_begin_end_layer` строит список иначе, через `build_display_list`), `getBoundingClientRect` не знает о прокрутке контейнера ([BUG-1166](BUG-1166-OPEN.md)). Для `sticky` пикселей нет проверки ни в одном графическом тесте (`CC-CSS-3`: «нечем детерминированно заскриншотить прокрученное состояние»). 87 id среза: `css-position/sticky/` и `position-sticky-dynamic-ancestor-001` — 57 reftest `FAIL` (52 `thick`, 4 `thin-only`, 1 `identical`: снимки теста и эталона совпали, а в `wptrunner` он красный) и 30 testharness (63 из 76 упавших сабтестов; `position-sticky-top.html` ожидается 250, получено 200), плюс 5 `anchor-scroll-to-sticky-*` в кластере `anchor-scroll`.

## Проба

Проба (`--mcp`, контейнер 250×150 `overflow:auto`, липкий `top:100px`, `scrollTop=250`):

| проверка | у нас | ожидается |
|---|---|---|
| `st.getBoundingClientRect().top` до | 200 | 200 |
| `sc.scrollTop = 250; sc.scrollTop` | 250 | 250 |
| `st.getBoundingClientRect().top` после | **200** | 100 |
| `st.offsetTop` | 200 | 200 |
| `--dump-display-list` для `position:sticky;top:10px` в корне | нет `BeginStickyLayer` | `BeginStickyLayer … top=10` |
| `--screenshot`: контейнер 100 px, липкий `top:0`, высота 40, `scrollTop=80` | `60…99` — вне слоя прокрутки: не сдвинут и не прилип (`--dump-display-list`: `FillRect` стоит после `PopScrollLayer`) | `0…39`: прилип к верху контейнера |

Снимок `position-sticky-top-003.html` (`--viewport 800x600 --ahem`): три красных квадрата у цели, у эталона — три зелёных на их месте.

## Как найдено

WPT-RUN-14 срез 21: `css-position/sticky/position-sticky-top-003.html`, `position-sticky-top.html`, `position-sticky-left.html`.

## Что делать

Развести три поверхности: (1) почему `--dump-display-list` не содержит `BeginStickyLayer` при том, что `walk.rs` его пишет (путь `paint_ordered` против `build_display_list`; сверить со стеком `box_layer.rs:678`); (2) сдвиг `getBoundingClientRect`/`offsetTop` липкого элемента от `scrollTop` предка — общий с BUG-1166; (3) отрисовка `BeginStickyLayer` в CPU-растре `cpu_raster.rs` (`grep Sticky` даёт ноль — команда там не реализована, wgpu-путь `renderer.rs:3421` реализован), без чего `--screenshot` и `LumenRefTestExecutor` не видят прилипания. Правило `crates/engine/paint/CLAUDE.md`: новая команда реализуется в обоих растрах.

## Как проверить

Таблица выше; `css/css-position/sticky/position-sticky-top-003.html`, `position-sticky-top.html`.

## Срез 1 (2026-11-18, P3): липкий бокс внутри `overflow`-контейнера — отрисовка

Поверхность «ordered-билдер не эмитит sticky-слой» решена не через `BeginStickyLayer`, а через запечённое в display list смещение: `box_layer_ops` (ordered-путь, тот, что реально рисуют и `--screenshot`/CPU, и живое окно/wgpu) для `position:sticky`-бокса со scroll-контейнером в цепочке клипов вычисляет склэмпленный offset (`sticky_offset_for`, зеркало `renderer/sticky_geom.rs`) и оборачивает вывод бокса в `PushTransform(translation(dx,dy))` + `PopTransform` — команды, уже реализованные в обоих растрах, поэтому отдельной реализации в `cpu_raster.rs` не понадобилось. Смещение считается от `scroll_x`/`scroll_y` и `clip_rect` ближайшего `PushScrollLayer` из inherited-цепочки (сам бокс этот слой теряет — BUG-159-фильтр), `top/bottom`-инсеты резолвятся против высоты scrollport, `left/right` — против ширины (`%` работает). При `scroll=0`/не-прилипании (`offset=(0,0)`) трансформ не эмитится, чтобы не плодить пустой offscreen-слой и не сдвинуть AA. Проверено живым `--screenshot` (`scrollTop=200`, `top:100px`): зелёный бокс 100×100 стоит на y=102 (прилип), а не уезжает со скроллом. Юнит-тесты `sticky_in_scrolled_container_bakes_offset_transform` / `sticky_at_scroll_zero_emits_no_offset_transform` в `ordered_build_scroll.rs`.

**Осталось:** (2) `getBoundingClientRect`/`offsetTop` липкого элемента — общее с BUG-1166. Пункт (1) закрыт срезом 4 (2026-11-18).

## Срез 2 (2026-11-18, P3): клип липкого бокса scrollport-ом контейнера

Срез 1 оборачивал прилипший sticky-бокс только в `PushTransform(offset)` — клип scrollport-а терялся вместе со scroll-слоем (BUG-159-фильтр), поэтому бокс, прилипший из-под нижней кромки контейнера, рисовался за его пределами. Теперь `sticky_offset_for` возвращает вместе со смещением и `clip_rect` ближайшего `PushScrollLayer` (scrollport контейнера), а `box_layer_ops` оборачивает вывод бокса в `PushClipRect { rect: scrollport }` → `PushTransform(offset)` (клип внешний, сдвиг внутренний — зеркало wgpu `clip_stack` + `sticky_stack`); при `offset=(0,0)` клип, как и трансформ, не эмитится. Юнит-тест `sticky_in_scrolled_container_bakes_offset_transform` расширен проверкой клипа и порядка `clip → transform → PopTransform → PopClip`.

## Срез 3 (2026-11-18, P3): липкий бокс на уровне страницы — ordered-билдер эмитит `BeginStickyLayer`

`sticky_offset_for` теперь различает два случая и возвращает `StickyEmit`: `Baked` (бокс в scroll-контейнере — запечённый `PushClipRect(scrollport)` → `PushTransform(offset)`, как в срезах 1–2) и `Page` (бокс без scroll-контейнера — `BeginStickyLayer { flow_rect, top/bottom/left/right }`/`EndStickyLayer`, как legacy-`walk`; `%`/`em`-инсеты сбрасываются тем же `to_px_opt`, что и `walk`). `box_layer_ops` эмитит `Page`-случай самым внешним слоем (до anchor-scroll-слоёв), зеркально legacy-`walk` (`dispatch`/`finish_epilogue`). wgpu-путь (`renderer.rs`) и femtovg уже потребляют `BeginStickyLayer`, поэтому живое окно теперь прилипает страничный sticky; `cpu_raster` команду игнорирует (`_ => {}`) — см. «Осталось (1)». Юнит-тест `page_level_sticky_emits_begin_end_layer` в `ordered_build_scroll.rs`.

## Срез 4 (2026-11-18, P3): отрисовка `BeginStickyLayer` + прокрутка страницы в CPU-растре

`cpu_raster` теперь знает про прокрутку страницы и `BeginStickyLayer`/`BeginFixedLayer` — до среза `_scroll_x`/`_scroll_y` игнорировались (снимок на прокрученной позиции рисовал scroll 0), а `BeginStickyLayer` падал в `_ => {}` (страничный sticky на `--screenshot`/`LumenRefTestExecutor` просто уезжал со скроллом). Механика — не новый растр-примитив, а уже существующий affine-путь `PushTransform`:

- **Прокрутка страницы** запекается в геометрию списка на входе `rasterize_cpu_pixmap`: если `scroll != 0`, весь список сдвигается `DisplayCommand::translated(-scroll_x, -scroll_y)` — всё рисуется в screen-пространстве, tiny-skia естественно обрезает то, что ушло за полотно. Это заодно чинит `render_tile`/IPC-`Screenshot`, которые всегда передавали scroll и молча получали scroll 0.
- **`BeginStickyLayer`** считает кламп-коррекцию `sticky_correction` (0-базисный зеркальный `sticky_offset_dy/dx` из femtovg; `flow_rect` уже в screen-пространстве, bound — весь вьюпорт) и оборачивает поддерево в `PushTransform(translation(cdx,cdy))`; `(0,0)` = не прилип — слой не эмитится. Когда допрокруточная позиция бокса уходит выше/ниже полотна, `emit_translation_bracket` рисует поддерево в off-screen слой собственного габарита (`render_offcanvas_transform_group`, тот же путь BUG-1479) и композитит целиком.
- **`BeginFixedLayer`** отменяет запечённый scroll трансформом `translation(+scroll_x, +scroll_y)`: fixed-контент уже в координатах вьюпорта, прокрутка его сдвигать не должна.

Юнит-тесты в `cpu_raster/tests.rs`: `page_scroll_shifts_content_up`, `page_level_sticky_sticks_at_top`, `fixed_content_stays_put_under_scroll`. Правки не затрагивают вывод при `scroll=0` (запекание — тождество, коррекция и отмена — нули), поэтому графический гейт и snapshot-тесты не двигаются. Остаток (1) закрыт.

## Срез 5 (2026-11-18, P3): геометрия для скриптов — `getBoundingClientRect`/`offsetTop` липкого бокса

Пункт (2) «Что делать» закрыт: `getBoundingClientRect`/`getClientRects`/`getBoxQuads`/`offsetLeft`/`offsetTop` липкого бокса в scroll-контейнере теперь отражают прилипание. Механика зеркалит BUG-1166 (сдвиг считается **при чтении**, а не запекается в прямоугольники):

- **layout** (`sticky_snap.rs`): `collect_sticky_geoms(root)` собирает статическую геометрию липких боксов — резолвленные инсеты (`top`/`bottom`/`left`/`right`), ближайший scroll-контейнер (`overflow: scroll|auto|hidden`) и его scrollport (padding box; на неклипуемой оси — `BIG`-сентинел, как paint `overflow_clip_rect`). `StickyGeom::correction(flow, scroll_x, scroll_y)` даёт поправку `[cdx, cdy]` — ровно то, что paint-овые `sticky_dy`/`sticky_dx` добавляют сверх `-scroll`. Page-level sticky (без контейнера) в карту не попадает — его сдвиг зависит от прокрутки страницы (BUG-1166).
- **js**: новая нативная `_lumen_get_sticky_shift(nid)` (читает `sticky_geoms` + `layout_rects` + `scroll_states` из `FlushHandles`, считает на чтении) и шим `web_api_shim_mid.js` — `getBoundingClientRect`/`getClientRects`/`getBoxQuads` прибавляют `+st[0]/+st[1]`, `offsetLeft`/`offsetTop` — тоже (border edge относительно offsetParent включает липкий сдвиг, в отличие от прокрутки предков).
- **shell/driver**: `sticky_geoms` публикуется теми же путями, что `client_rects` (`PrecollectedJsData`, `JsLayoutSnapshot`, relayout/page_load/frames/hibernation/scripts/driver) и пересчитывается в `maybe_flush`.

Тесты: `sticky_correction_*`/`collect_sticky_geoms_*` (layout) + `get_bounding_client_rect_sticky_sticks_inside_scrolled_container` (js, нативный + шим). WPT `position-sticky-top.html`: «до прилипания» и «после прилипания» зелёные.

**Осталось (вне этого среза):** (a) **containing-block constraint** — липкий бокс не зажат своим containing block (CSS Position 3 §2.2, sticky constraint rectangle = scrollport ∩ containing block): при `scrollTop=300` в `position-sticky-top.html` `offsetTop` даёт 350 вместо 300 (бокс уезжает за нижний край `container`, y=100..400). Тот же пробел во всех четырёх paint-путях (`sticky_dy/dx` в `box_layer.rs`, `femtovg_backend.rs`, `renderer/sticky_geom.rs`, `cpu_raster.rs`) — заведено отдельно как [BUG-1825](BUG-1825-FIXED.md). (b) Page-level sticky геометрия (`getBoundingClientRect` липкого бокса на странице) — прокрутка страницы, [BUG-1166](BUG-1166-OPEN.md).
