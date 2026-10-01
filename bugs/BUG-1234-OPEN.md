# BUG-1234 — анимации не интерполируют/не складывают произвольные CSS-свойства (SVG-геометрия, `color-interpolation`, `path-length`, `stroke-*`)

**Статус:** OPEN
**Тип:** пробел реализации.
**Заведён:** 2026-10-01 (P6, остаток [BUG-1094](BUG-1094-FIXED.md))
**Область:** `crates/engine/layout/src/animation.rs` — `AnimatedStyle`/`KeyframeStyle` содержат только `opacity`/`transform`/`color`/`background-color`/`height`.

## Симптом

После заведения свойств в BUG-1094 `CSS.supports()` и разбор работают, но 9 файлов (`svg/geometry/animations/{cx,cy,r,rx,ry,x,y}-composition.html`, `svg/painting/color-interpolation-animation.html`, `svg/path/animations/path-length-interpolation.tentative.html`, 284 подтеста) и `svg/painting/animations/stroke-{width,dashoffset,dasharray}-composition.html` падают: `getComputedStyle()` во время CSS/Web-анимации возвращает базовое значение.

## Ожидание

Общий механизм интерполяции и `composite: add/accumulate` для `<length-percentage>`, `<number>` и дискретных ключевых слов по списку свойств.
