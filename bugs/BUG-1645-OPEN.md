# BUG-1645 — остаток BUG-1292: процент в `translate` у SVG-элементов, сериализация `translate`, `calc()` в `rotate()`/`scale()`

**Статус:** OPEN
**Заведён:** 2026-10-19 (P3, остаток после BUG-1292)
**Область:** layout (`crates/engine/layout/src/box_tree/svg.rs`, `style/parse/transform.rs`, `selector_query.rs`)

## Симптом

BUG-1292 закрыт для HTML-боксов (`transform-percent-001…008` совпадают с reference). Не закрыто:

1. SVG-элементы: `transform: translate(50%, 50%)` на `<rect>`/`<g>` — процент должен считаться от reference box по
   `transform-box` (`fill-box` / `view-box` / `stroke-box`), SVG-ветка (`box_tree/svg.rs`) его не знает. WPT
   `css/css-transforms/transform-percent-009.html` (`getComputedStyle().transform` = `matrix(1, 0, 0, 1, 50, 50)`),
   `transform-percent-010.html` (reftest, `<g style="transform: translate(50%, 50%)">`).
2. `getComputedStyle(el).translate` пуст; ожидается сериализация computed-значения без разрешения процентов:
   `10px 20px`, `30% 40% 50px`, `calc(80% + 70px) -90px` (`translate-getComputedStyle.html`, `transforms-support-calc.html`
   «translate supports calc» → `calc(20% + 30px) calc(100% - 200px)`). Для этого `ComputedStyle::translate` должен
   хранить и Z (сейчас третий компонент принимается и отбрасывается).
3. `rotate(calc(1turn - 400grad))`, `scale(calc(5 + 2), calc(5 - 2))` внутри `transform` отбрасываются
   (`parse_angle_to_radians`/`parse::<f32>` не понимают `calc()`): `transforms-support-calc.html` «transform supports calc».

## Как проверить

`css/css-transforms/transform-percent-009.html`, `transform-percent-010.html`, `translate-getComputedStyle.html`,
`transforms-support-calc.html`.
