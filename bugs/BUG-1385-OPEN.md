# BUG-1385 — ширина не выводится из заданной высоты по `aspect-ratio` (кроме замещаемого с одной высотой)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 16, `css/css-overflow` + `css/css-sizing`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs:571` — ветка «`width: auto` + высота → ширина» есть только для `is_replaced`; `flex_trampoline.rs:1397`, `grid_trampoline.rs:1105`, `layout_cache.rs:287` — то же)

## Симптом

`--dump-layout`, `body{margin:0}`, `background:green`, viewport 800×600:

| разметка | получено | ожидается |
|---|---|---|
| `<div style="height:100px;aspect-ratio:1/1">` | 800×100 | 100×100 |
| `<div style="width:0"><div style="float:left;height:100px;aspect-ratio:1/1">` | 0×100 | 100×100 |
| `<div style="display:inline-block;height:60px;aspect-ratio:2">` | 800×60 | 120×60 |
| `<div style="display:flex"><div style="height:100px;aspect-ratio:1/2;min-width:0">` | 0×100 | 50×100 |
| `<div style="display:grid"><div style="height:100px;aspect-ratio:1/1">` | 800×100 | 100×100 |
| abspos `left:0;top:0;bottom:0;aspect-ratio:2/1` в `300×50` | 0×50 | 100×50 |
| abspos `left:0;right:0;top:0;bottom:0;aspect-ratio:1/1` в `100×500` | 100×500 | 100×100 |
| `<div style="width:100px;aspect-ratio:auto 3/1">` и `3/1 auto` | 100×**0** | 100×33.3 (`aspect-ratio:3/1` без `auto` — 33.3, верно) |
| `<svg height="40" viewBox="0 0 4 1">` | 4×40 | 160×40 |

Направление «ширина → высота» работает (`width:100px;aspect-ratio:3/1` → 33.3, `box-sizing:border-box` с `padding` — верно); «высота → ширина» не сработало ни в одной из проверенных раскладок. Для замещаемого с `width:auto` есть отдельная ветка (BUG-734, `layout_dispatch.rs:571`), но `<img style="height:20px">` всё равно даёт 60×20, а не 20×20 — это [BUG-1280](BUG-1280-FIXED.md). Форма `auto <ratio>` теряет соотношение у незамещаемого бокса.

## Как найдено

WPT-RUN-14 срез 16: 150 id `css-sizing/aspect-ratio/{block,flex,grid,abspos}-aspect-ratio-*`, `intrinsic-size-*`, `abspos-*` (135 thick) + 20 id с формой `auto <ratio>` + `svg-intrinsic-size-*` (3 thick; по имени — ещё `intrinsic-percent-replaced-*`, но их держит и BUG-1280). Почти все — «заливка 100×100 зелёным без красного», так что любое расхождение размера видно в пикселях.

## Что делать

Общая функция «внутренняя ось по соотношению» для block/flex/grid/abspos/float/inline-block и для `SvgRoot` с `viewBox`; разбор `aspect-ratio: auto <ratio>` не должен терять отношение для боксов без естественных пропорций. Сначала block (самая крупная группа, 28 id), затем flex (41) и grid (28).

## Как проверить

`css/css-sizing/aspect-ratio/block-aspect-ratio-002.html`, `flex-aspect-ratio-005.html`, `grid-aspect-ratio-003.html`, `intrinsic-size-003.html`, `abspos-003.html`, `abspos-004.html`, `css/css-sizing/svg-intrinsic-size-004.html`.

## Срез 1 (2026-10-20, P3)

Исправлено (ветка `p3-bug-1385`, тесты — `box_tree/tests/aspect_ratio_from_height.rs`, 15 штук):

- **block / float / flex- и grid-контейнер**: `width: auto` + определённая высота + соотношение → ширина = высота по соотношению (`layout_dispatch.rs` `block_ratio_width`); автоматический минимум (`min-width: auto` = min-content содержимого, если не scroll container) тоже применяется — и в `lay_out`, и в `max-/min-content` родителя (`intrinsic.rs` `transferred_width`); `invariants.rs` не считает такой бокс «ушедшим из родителя».
- **grid-элемент**: явный `justify-self: stretch` заполняет область (в т.ч. при `align-self: stretch`), `normal` берёт ширину по соотношению (`grid_trampoline.rs`).
- **abspos** (`multicol_abspos.rs`): обе inline-инсеты + `height: auto` → ширина растягивается, высота по соотношению; одна block-инсета/заданная `width` → высота по соотношению, а не растяжение; заданная `height` + `width: auto` → ширина по соотношению.
- **`<svg viewBox>`**: `auto`-ось рядом с заданной берётся по соотношению viewBox (`svg.rs`).
- **`aspect-ratio: auto <ratio>` / `<ratio> auto`** парсится (`parse_auto_aspect_ratio_value`, поле `ComputedStyle::aspect_ratio_auto`): у бокса без естественной пропорции это обычное соотношение, у картинки/canvas с декодированной пропорцией побеждает естественная.

Замер (`lumen --screenshot`, 800×600, база — `main` на ~03:30 20.10, пиксель-дифф с эталоном): `css/css-sizing/aspect-ratio` identical 110 → 146 из 266, регрессий 0; остальные 190 тестов `css/**`, где встречается `aspect-ratio`: identical +16, регрессий 0; `--dump-display-list` по 187 страницам `graphic_tests`/`samples` байт-в-байт; `dump_golden.py` 12/12.

Остаток (120 thick в `css-sizing/aspect-ratio`): `flex-aspect-ratio-*` (23), `grid-aspect-ratio-*` (20), `replaced-element-*` (26, смежно с BUG-1280), `intrinsic-size-*` (17), `block-aspect-ratio-*` (16, часть — `writing-mode: vertical-*`), `abspos-*` (10), `percentage-resolution-*`, `fieldset-element-*`. Причины по одному не отделены.
