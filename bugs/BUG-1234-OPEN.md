# BUG-1234 — анимации не интерполируют/не складывают произвольные CSS-свойства (SVG-геометрия, `color-interpolation`, `path-length`, `stroke-*`)

**Статус:** OPEN
**Тип:** пробел реализации.
**Заведён:** 2026-10-01 (P6, остаток [BUG-1094](BUG-1094-FIXED.md))
**Область:** `crates/engine/layout/src/animation.rs` — `AnimatedStyle`/`KeyframeStyle` содержат только `opacity`/`transform`/`color`/`background-color`/`height`.

## Симптом

После заведения свойств в BUG-1094 `CSS.supports()` и разбор работают, но 9 файлов (`svg/geometry/animations/{cx,cy,r,rx,ry,x,y}-composition.html`, `svg/painting/color-interpolation-animation.html`, `svg/path/animations/path-length-interpolation.tentative.html`, 284 подтеста) и `svg/painting/animations/stroke-{width,dashoffset,dasharray}-composition.html` падают: `getComputedStyle()` во время CSS/Web-анимации возвращает базовое значение.

## Ожидание

Общий механизм интерполяции и `composite: add/accumulate` для `<length-percentage>`, `<number>` и дискретных ключевых слов по списку свойств.

## Ещё экземпляры (WPT-RUN-14 срез 3, 2026-10-05)

`css/css-ui/animation/outline-width-interpolation.html` (120 из 148 сабтестов), `outline-color-interpolation.html`
(99 из 120), `outline-width-composition.html` (35 из 52): во время перехода/анимации `getComputedStyle` отдаёт
конечное (`20px`, `rgb(0, 128, 0)`) или базовое значение. Свойства в карте `computed_style_to_map` есть, не хватает
только интерполяции — тот же механизм.
