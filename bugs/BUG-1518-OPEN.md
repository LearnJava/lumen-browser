# BUG-1518 — `shape-outside: <shape-box>` и `<basic-shape> <shape-box>` не поддержаны: опорный бокс всегда margin-box, скругление рамки (`border-radius`) контура не задаёт

**Статус:** OPEN (ДОРАБОТКА → SHAPE-OUTSIDE-2)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 23, `css/css-text-decor` + `css-gaps` + `css-shapes`)
**Область:** layout (`crates/engine/layout/src/box_tree/shapes_floats.rs::register_shape_outside`, `style::ShapeOutside::Value(String)`)

## Симптом

`ShapeOutside::Value(String)` хранит сырую строку, `<shape-box>` (`margin-box`/`border-box`/`padding-box`/`content-box`) не разбирается. Одиночный `border-box`/`padding-box`/`content-box`/`margin-box` обтекается как прямоугольник поля float; `border-radius` на таком float контур не скругляет; `<basic-shape> <shape-box>` (`content-box circle(…)`, `circle(…) border-box`) вычисляется от margin-box, а не от указанного бокса.

## Проба

Проба (`--mcp`, float `100×100`, текст `Arial 20px/20px`, левые края первых 6 строк):

| `shape-outside` и стиль float | у нас | ожидается |
|---|---|---|
| `margin-box`, `border-radius:50%` | `100,100,100,100,100,100` | круг (`50,…` по краям) |
| `border-box`, `border-radius:50%` | `100,…` | круг |
| `padding-box`, `border-radius:50%`, `border:10px solid` | `120,…` | круг по padding-box |
| `content-box`, `border-radius:50%`, `padding:20px` | `140,…` | круг по content-box |
| `content-box circle(50px at 50px 50px)` + `padding:20px; border:10px` | `160,160,160,160,160,160` (центр круга — центр margin-box) | круг в content-box (центр смещён на 30 px) |
| `circle(50px at 50px 50px) margin-box` + `margin:20px` | `140,…` | круг от margin-box |

## Как найдено

WPT-RUN-14 срез 23: `shape-outside/shape-box/*` (39 id, из них 12 `border-box-border-radius`, 8 `margin-box-border-radius`), `supported-shapes/circle|ellipse|inset|polygon` с `<shape-box>` (29 + прочее), `spec-examples/shape-outside-014…017` (testharness, «Line N is positioned properly»). 111 id: 107 reftest `thick` и 4 testharness (кластер «shape-outside: `<shape-box>`»).

## Что делать

Задача SHAPE-OUTSIDE-2: разбирать `<shape-box>` (по умолчанию `margin-box`), вычислять опорный прямоугольник и его скругления (`border-radius`, CSS Backgrounds 3 §5.5 «shaping box corners»), давать контур для одиночного `<shape-box>`; `<basic-shape>` считать от выбранного бокса.

## Как проверить

`css/css-shapes/shape-outside/shape-box/shape-outside-border-box-border-radius-001.html`, `supported-shapes/circle/shape-outside-circle-026.html`.
