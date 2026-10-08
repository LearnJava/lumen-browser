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

## Ещё экземпляры (WPT-RUN-14 срез 16, 2026-10-07, `css/css-sizing`, `css/css-overflow`)

`css-sizing/animation/{width,height,min-width,min-height,max-width,max-height}-{interpolation,composition}.html`, `aspect-ratio-interpolation.html`, `contain-intrinsic-size/animation/contain-intrinsic-size-interpolation.html`, `css-overflow/overflow-no-interpolation.html` — 16 id, 1 470 из 3 567 сабтестов. Разрез по механизму (сабтесты `width`/`height`/`min-*`/`max-*`/`aspect-ratio`/`contain-intrinsic-size`): `CSS Animations` (`@keyframes`) — 37 PASS из 574; `CSS Transitions` — 888 из 1 154; `Web Animations` (`el.animate`) — 477 из 569; `Compositing CSS Animations` — 6 из 183. В упавших `CSS Animations` `getComputedStyle` отдаёт базовое значение (`expected "0px" but got "100px"`). Проба `el.animate({width:['10px','20px']}, {duration:1000, fill:'both'})` + `pause()` + `currentTime=500` → `15px` (верно). Проба `@keyframes` + `animation-delay:-500s` в `--dump-layout` ничего не говорит — там и `opacity` остаётся `1`, т. е. headless-режим не продвигает CSS-анимации; причину для `@keyframes` надо снимать в живом окне или `wptrunner`-ом. Промежуточные значения `%`/`em`/`vw` и `inherit`/`initial`/`unset` в концах падают и у `Web Animations` (4 из 6 для `10px → 100%`).

## Ещё экземпляры (WPT-RUN-14 срез 18, 2026-10-07, `css/css-fonts`, `css/css-masking`)

Тот же механизм (`animation.rs` знает opacity/transform/color/background-color/height), 24 testharness-id, 3 345 сабтестов:
`css-fonts/animations/{font-size-interpolation-001…003,font-size-composition,font-stretch-interpolation,font-style-interpolation,font-weight-composition,font-palette-interpolation,system-fonts}.html`,
`discrete-no-interpolation.html`, `font-stretch-interpolation-math-functions.html` (1 116 из 1 900); `css-masking/animations/{clip-composition,clip-interpolation,mask-border-*}.html`
(2 229 из 2 229 — здесь ещё и свойство не поддержано, `assert_true: 'from' value should be supported`, см. `mask-border` в `CSS-SPECS.md`);
`clip-path/animations/clip-path-transition-crash.html` (TIMEOUT: `document.getAnimations()[0]` — `undefined` после смены `clip-path`, в логе
`Cannot read properties of undefined (reading 'finished')`) и 55 reftest `clip-path/animations/*` (`reftest-wait`, см. WPT-RUN-15).

## Повторное измерение: WPT-RUN-14 срез 24 (2026-10-08)

`filter`/`backdrop-filter`: WAAPI `el.animate({filter:["blur(0px)","blur(20px)"]})` + `pause()` + `currentTime=2000` — `getComputedStyle().filter` даёт `blur(20px)`, ожидается `blur(10px)` (интерполяции нет); 7 id `filter-effects/animation/*` (672 из 954 сабтестов) и 20 reftest `css-filters-animation-*`/`css-backdrop-filters-animation-*` (в снимке видна конечная точка). `color-interpolation-filters` — `'from' value should be supported`: свойства нет в разборе.
