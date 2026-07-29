# BUG-099

**Статус:** OPEN (DEBTOR)
**Компонент:** js/paint/layout
**Файлы:** `crates/engine/layout/src/box_tree.rs`, `crates/engine/paint/src/display_list.rs`, `crates/engine/canvas/src/rasterize.rs`

## Описание

Изначальная заявка: `<canvas>` 2D-контекст не реализован — TEST-57 28.66%,
`getContext("2d")` заглушка.

2D-контекст реализован (Фаза 2, `crates/engine/canvas`), исходная формулировка
устарела. Строка осталась как DEBTOR-якорь ратчета TEST-57 в
`graphic_tests/run.py` (`KNOWN_DEBTORS['57']`).

## Что было починено 2026-07-29

Разложение диффа TEST-57 против Edge-эталона (детерминированный CPU-снапшот
`graphic_tests/snapshots/cpu/57-canvas-2d.png`) показало, что остаток — вовсе не
только «font-parity + canvas-AA», как утверждала строка BUGS.md. Ячейка `c3`
(`<canvas width="180" height="150" style="border:3px solid">` на странице с
`* { box-sizing: border-box }`) расходилась с Edge структурно:

| | Edge | Lumen (до) |
|---|---|---|
| border-box `c3` | 186×156, (565,25)–(750,180) | 180×150, (565,25)–(744,174) |
| `fillRect(20,20,…)` в битмапе | (588,48) | (585,45) |

Два независимых дефекта:

1. **Layout.** Размеры битмапа (`width`/`height`-атрибуты) подставлялись прямо в
   `style.width`/`style.height`, из-за чего `box-sizing: border-box` вычитал из
   них рамку и padding. Но HTML Rendering §15.4.1 **не** включает `canvas` в
   список элементов, чьи dimension-атрибуты маппятся на свойства `width`/
   `height` (в отличие от `img`/`video`/`iframe`): для `<canvas>` это
   *intrinsic*-размер, то есть размер **content-box**. Починка —
   `box_tree.rs`, ветка `is_canvas_element`: при `BoxSizing::BorderBox`
   подставляемое значение увеличивается на рамку + padding по своей оси.
2. **Paint.** `DisplayCommand::DrawImage` для битмапа шёл в `b.rect`, то есть в
   **border-box** — вся отрисовка канваса уезжала под рамку на её ширину.
   Починка — новый хелпер `content_box_rect()` в `display_list.rs` (его же
   переиспользует ветка `BackgroundClip::ContentBox` у `background_clip_rect`);
   обе ветки покраски `BoxKind::Canvas` (в `emit_box_self` и в `walk`) рисуют
   битмап в content-box.

Побочный эффект первого дефекта: заниженная на 6px высота `c3` тянула вниз
высоту первого flex-ряда, и **весь второй ряд страницы стоял на 5px выше**, чем
у Edge — отсюда «раздвоенные» подписи на дифф-картинке. После фикса `c3`
совпадает с Edge пиксель-в-пиксель (bbox рамки и красного прямоугольника
идентичны), остаток по ряду 2 — 1px.

Диф детерминированного CPU-снапшота против Edge-эталона: **2.95% → 0.82%**.

Тот же дефект покраски в border-box найден у `<img>`/`<video>`/`<iframe>` —
заведён отдельно как [BUG-431](BUG-431-OPEN.md), в этот фикс не входит.

Тесты: `canvas_intrinsic_size_is_a_content_box_under_border_box_sizing`,
`canvas_intrinsic_size_unchanged_under_content_box_sizing`,
`canvas_explicit_css_size_is_not_grown_by_the_border` (lumen-layout);
`canvas_intrinsic_size_survives_border_box_sizing`,
`canvas_bitmap_is_painted_into_the_content_box`,
`canvas_bitmap_content_box_accounts_for_padding`,
`canvas_explicit_css_size_keeps_border_box_meaning` (lumen-paint).

## Остаток (почему всё ещё DEBTOR)

* **Canvas-AA.** `crates/engine/canvas/src/rasterize.rs` — скан-лайн заливка с
  бинарным покрытием; антиалиасинга нет вообще ни у `fill_path`, ни у
  `stroke_path`, ни у `build_clip_mask`. Геометрия при этом точна: площадь круга
  `arc(100,80,50)` = 7856 px против аналитических π·50² = 7854, площадь
  треугольника = ровно 9600 px — расходится только кайма. Edge сглаживает.
* **Font-parity** подписей (Inter vs Edge) — класс [BUG-128](BUG-128-OPEN.md),
  правило 3.
* Остаточный сдвиг ряда 2 на 1px — класс PS-1 / [BUG-124](BUG-124-OPEN.md)
  (единая политика pixel-snapping), не канвасный дефект.
