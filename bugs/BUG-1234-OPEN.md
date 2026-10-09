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

## Повторное измерение: WPT-RUN-14 срез 26 (2026-10-09)

Граница прохода между CSS-движком и Web Animations из JS измерена пробой (`run_smoke.py` + testharness, `getComputedStyle` через 250–700 мс после старта; 23 свойства, `transition: <свойство> 20s linear`):

- **CSS-переход идёт только у `opacity`, `color`, `background-color`, `transform`, `height`** — у них `transitionrun` приходит и значение промежуточное. Остальные 18 (`width`, `margin-left`, `padding-left`, `left`, `top`, `border-top-width`, `border-top-color`, `border-top-left-radius`, `font-size`, `line-height`, `letter-spacing`, `outline-width`, `box-shadow`, `flex-grow`, `z-index`, `min-width`, `max-width`, `visibility`) за 250–700 мс прыгают к конечному значению, `transitionrun` не приходит, `getAnimations()` пуст.
- **CSS `@keyframes` — то же**: `margin-left: 0→100px` за 6 с остаётся `0px` на 1,3 / 2,6 / 3,9 с, `opacity`/`background-color`/`transform` идут.
- **WAAPI из JS (`el.animate`) тот же набор свойств интерполирует**: `margin-left` 18.3 px через 1 с из 6 с, `z-index` 5 на 50 %, `vertical-align` 10px, `text-shadow`, `display` — то есть интерполятор в JS-шиме шире, чем в `animation.rs`.

Следствие: 56 id / 2 614 сабтестов `css-animations` (`animation-base-response-*`, `animation-iteration-count-*`, `display-interpolation`, `text-decoration-inset-auto`, `animate-with-color-mix`…) и `css-transitions` (`z-index-interpolation`, `vertical-align-interpolation`, `text-shadow-interpolation`, `all-interpolates-same-as-explicit-property`: 21 из 32) — кластер AT08 в `docs/wpt-vendor-notes/css.md` §css-page + css-animations + … . Эти id не разделены с BUG-1305 (neutral keyframe) и BUG-1293 (старт).
