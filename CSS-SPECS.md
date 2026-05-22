# CSS Specifications & Property Roadmap

Canonical reference for CSS compliance work in Lumen. Tracks which W3C modules and properties are implemented, partial, or pending.

**Source of truth for specs:** https://www.w3.org/Style/CSS/specs.en.html  
**Implementation tracking:** P4 developer owns this file. Update on every property merge.

Legend: ✅ implemented · 🟡 parsed/stored, rendering deferred · ⬜ not started · 🚫 out of scope

---

## Quick stats (2026-05-22)

| Status | Properties |
|--------|-----------|
| ✅ Fully implemented | ~135 |
| 🟡 Partial (parsed, not rendered) | ~91 |
| ⬜ Not started | ~15 |
| 🚫 Out of scope | ~20 |

---

## Module Priority

Modules ordered by **impact on real web pages**: what breaks most sites when missing.

### Tier 0 — Foundation (✅ stable)

These modules are fully or nearly-fully implemented. Maintain correctness; no new work needed.

| Module | Spec | Status | Notes |
|--------|------|--------|-------|
| CSS Cascading L3 | [css3-cascade](https://www.w3.org/TR/css3-cascade/) | ✅ | specificity, inheritance, !important |
| CSS Color L3 | [css3-color](https://www.w3.org/TR/css3-color/) | ✅ | named/hex/rgb/rgba/hsl/hsla; currentColor |
| CSS Box Model L3 | [css3-box](https://www.w3.org/TR/css3-box/) | ✅ | all margin/padding/box-sizing |
| CSS Backgrounds & Borders L3 | [css3-background](https://www.w3.org/TR/css3-background/) | ✅ | borders/radius/box-shadow/bg-color/image/size/pos/repeat |
| CSS Fonts L3 | [css3-fonts](https://www.w3.org/TR/css3-fonts/) | ✅ | font-size/weight/style/family/variant; @font-face parsing |
| CSS Flexible Box L1 | [css3-flexbox](https://www.w3.org/TR/css3-flexbox/) | ✅ | all flex properties; align-*/justify-content |
| CSS Transforms L1 | [css-transforms-1](https://www.w3.org/TR/css-transforms-1/) | ✅ | translate/rotate/scale/skew/matrix; transform-origin |
| CSS Text Decoration L3 | [css-text-decor-3](https://www.w3.org/TR/css-text-decor-3/) | ✅ | underline/overline/line-through; style/color/thickness |
| Selectors L3 | [css3-selectors](https://www.w3.org/TR/css3-selectors/) | ✅ | type/class/id/attr; combinators; :nth-*; :not() |
| CSS Logical Properties L1 | [css-logical-1](https://www.w3.org/TR/css-logical-1/) | ✅ | margin/padding/border/inset logical → physical (LTR) |
| CSS Color L4 | [css-color-4](https://www.w3.org/TR/css-color-4/) | 🟡 | oklch ✅; color-mix() ⬜; wide-gamut display ⬜ |

### Tier 1 — Critical gaps (break most web pages when missing)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Custom Properties L1 | [css-variables](https://www.w3.org/TR/css-variables/) | ✅ | var() recursive + @property + env() | **#1** |
| CSS Transitions | [css3-transitions](https://www.w3.org/TR/css3-transitions/) | ✅ | TransitionScheduler wired: sync()+tick() in shell loop | **#2** |
| CSS Animations L1 | [css-animations-1](https://www.w3.org/TR/css-animations-1/) | ✅ | AnimationScheduler::tick() wired in shell RedrawRequested | **#3** |
| CSS Nesting | [css-nesting-1](https://www.w3.org/TR/css-nesting-1/) | ✅ | `&`-explicit + implicit `.foo{}`/`>.foo{}` nesting + nested `@media`/`@supports`/`@layer`/`@container`; 20 tests | **#4** |
| CSS Display L3 (table) | [css-display-3](https://www.w3.org/TR/css-display-3/) | 🟡 | display:table/row/cell layout engine | **#5** |
| CSS Positioning L3 (sticky) | [css3-positioning](https://www.w3.org/TR/css3-positioning/) | 🟡 | position:sticky + scroll listener | **#6** |
| CSS Positioning L3 (z-index) | [css3-positioning](https://www.w3.org/TR/css3-positioning/) | 🟡 | stacking context paint ordering | **#7** |
| CSS 2.1 floats | [CSS2](https://www.w3.org/TR/CSS2/) | 🟡 | float + clear layout algorithm | **#8** |
| CSS Lists L3 | [css3-lists](https://www.w3.org/TR/css3-lists/) | 🟡 | list-style-type marker rendering | **#9** |
| CSS Cascading L4/L5 | [css-cascade-4](https://www.w3.org/TR/css-cascade-4/) | ✅ | @layer cascade ordering: layer_priority in sort key, 6 tests | **#10** |
| Selectors L4 | [selectors4](https://www.w3.org/TR/selectors4/) | 🟡 | :is()/:where()/:has() matching | **#11** |
| Media Queries L3 | [mediaqueries-3](https://www.w3.org/TR/mediaqueries-3/) | 🟡 | resize hook; @media re-evaluation | **#12** |

### Tier 2 — High visual value (visually broken without these)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| Filter Effects L1 | [filter-effects](https://www.w3.org/TR/filter-effects/) | 🟡 | backdrop-filter GPU compositing | **#13** |
| CSS Masking | [css-masking](https://www.w3.org/TR/css-masking/) | 🟡 | mask-image GPU compositing | **#14** |
| Compositing & Blending | [compositing](https://www.w3.org/TR/compositing/) | 🟡 | mix-blend-mode blend pipeline | **#15** |
| CSS Pseudo-Elements L4 | [css-pseudo-4](https://www.w3.org/TR/css-pseudo-4/) | 🟡 | ::first-line/::first-letter split; ::marker; ::selection | **#16** |
| CSS Images L3 | [css3-images](https://www.w3.org/TR/css3-images/) | 🟡 | conic-gradient(); multiple bg layers | **#17** |
| CSS Images L4 | [css4-images](https://www.w3.org/TR/css4-images/) | ⬜ | image-set(), cross-fade() | **#18** |
| CSS Grid L1 | [css-grid-1](https://www.w3.org/TR/css-grid-1/) | 🟡 | grid-template-areas ✅ 2026-05-22; dense auto-flow ⬜ | **#19** |
| CSS Fonts L4 | [css-fonts-4](https://www.w3.org/TR/css-fonts-4/) | 🟡 | @font-face actual loading; font-optical-sizing | **#20** |
| CSS Intrinsic Sizing L3 | [css3-sizing](https://www.w3.org/TR/css3-sizing/) | 🟡 | min-content / max-content / fit-content | **#21** |
| CSS Overflow L3 (scroll) | [css-overflow-3](https://www.w3.org/TR/css-overflow-3/) | 🟡 | scrollable containers; overflow:scroll rendering | **#22** |
| CSS Text L3/L4 | [css3-text](https://www.w3.org/TR/css3-text/) | 🟡 | text-align-last; hyphens:auto | **#23** |
| CSS Transforms L2 | [css-transforms-2](https://www.w3.org/TR/css-transforms-2/) | 🟡 | perspective/3D; individual translate/rotate/scale props | **#24** |
| CSS Values L4/L5 | [css-values-4](https://www.w3.org/TR/css-values-4/) | 🟡 | env(); attr() with type; cq* units | **#25** |

### Tier 3 — Spec compliance (affect specific use-cases)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Scroll Snap L1 | [css-scroll-snap-1](https://www.w3.org/TR/css-scroll-snap-1/) | 🟡 | shell scroll integration | **#26** |
| CSS Multi-column L1 | [css3-multicol](https://www.w3.org/TR/css3-multicol/) | 🟡 | column-rule rendering; column-span; column-fill | **#27** |
| CSS Containment L2/L3 | [css-contain-2](https://www.w3.org/TR/css-contain-2/) | 🟡 | content-visibility skip-content; cq* units | **#28** |
| CSS Counter Styles L3 | [css-counter-styles-3](https://www.w3.org/TR/css-counter-styles-3/) | 🟡 | counter-reset/increment resolution; @counter-style ⬜ | **#29** |
| CSS Box Alignment L3 | [css3-align](https://www.w3.org/TR/css3-align/) | 🟡 | justify-items/justify-self for grid | **#30** |
| CSS Inline L3 | [css-inline-3](https://www.w3.org/TR/css-inline-3/) | 🟡 | line-height leading; baseline grid | **#31** |
| CSS Text Decoration L4 | [css-text-decor-4](https://www.w3.org/TR/css-text-decor-4/) | 🟡 | text-emphasis rendering; text-underline-offset | **#32** |
| CSS Scrollbars L1 | [css-scrollbars-1](https://www.w3.org/TR/css-scrollbars-1/) | 🟡 | scrollbar-width/color rendering | **#33** |
| CSS Basic UI L3/L4 | [css3-ui](https://www.w3.org/TR/css3-ui/) | 🟡 | resize drag-UI; appearance form widgets | **#34** |
| Media Queries L4/L5 | [mediaqueries-4](https://www.w3.org/TR/mediaqueries-4/) | 🟡 | prefers-reduced-motion; hover; pointer | **#35** |
| CSS Conditional L4 | [css-conditional-4](https://www.w3.org/TR/css-conditional-4/) | 🟡 | @supports full feature detection | **#36** |
| CSS Color Adjust L1 | [css-color-adjust-1](https://www.w3.org/TR/css-color-adjust-1/) | 🟡 | color-scheme UA switching | **#37** |
| CSS Box Sizing L4 | [css-sizing-4](https://www.w3.org/TR/css-sizing-4/) | 🟡 | contain-intrinsic-size; interpolate-size | **#38** |
| CSS Overflow L4 | [css-overflow-4](https://www.w3.org/TR/css-overflow-4/) | 🟡 | line-clamp multi-line truncation | **#39** |
| CSS Easing L1 | [css-easing-1](https://www.w3.org/TR/css-easing-1/) | 🟡 | cubic-bezier/steps interpolation wiring | **#40** |

### Tier 4 — Advanced / future

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Writing Modes L4 | [css-writing-modes-4](https://www.w3.org/TR/css-writing-modes-4/) | 🟡 | vertical-rl/lr layout axis swap | **#41** |
| CSS Grid L2 | [css-grid-2](https://www.w3.org/TR/css-grid-2/) | ⬜ | subgrid; masonry | **#42** |
| CSS Shapes L1 | [css-shapes-1](https://www.w3.org/TR/css-shapes-1/) | 🟡 | shape-outside float wrapping | **#43** |
| Motion Path L1 | [motion-1](https://www.w3.org/TR/motion-1/) | 🟡 | offset-path motion layout | **#44** |
| CSS Fragmentation L3 | [css3-break](https://www.w3.org/TR/css3-break/) | ⬜ | break-before/after/inside | **#45** |
| CSS Color L5 | [css-color-5](https://www.w3.org/TR/css-color-5/) | ⬜ | color-mix(); relative color syntax | **#46** |
| CSS Fonts L5 | [css-fonts-5](https://www.w3.org/TR/css-fonts-5/) | ⬜ | font-palette; @font-palette-values | **#47** |
| CSS Easing L2 | [css-easing-2](https://www.w3.org/TR/css-easing-2/) | ⬜ | linear() easing with keyframes | **#48** |
| CSS Overscroll L1 | [css-overscroll-1](https://www.w3.org/TR/css-overscroll-1/) | 🟡 | gesture boundary handling | **#49** |
| CSS Gap Decorations L1 | [css-gaps-1](https://www.w3.org/TR/css-gaps-1/) | ⬜ | decorative lines in gaps | **#50** |
| CSS Env Variables L1 | [css-env-1](https://www.w3.org/TR/css-env-1/) | ⬜ | env() safe-area-inset-* | **#51** |
| CSS Selectors L5 | [selectors-5](https://www.w3.org/TR/selectors-5/) | ⬜ | :nth-child(An+B of S) | **#52** |
| CSS Nesting (scope) | [css-scoping-1](https://www.w3.org/TR/css-scoping-1/) | ⬜ | @scope rule | **#53** |
| CSS Functions & Mixins | [css-mixins-1](https://www.w3.org/TR/css-mixins-1/) | ⬜ | @function rule | **#54** |
| Scroll-driven Animations | [scroll-animations-1](https://www.w3.org/TR/scroll-animations-1/) | ⬜ | scroll-timeline; animation-timeline | **#55** |
| CSS Anchor Positioning | [css-anchor-position-1](https://www.w3.org/TR/css-anchor-position-1/) | ⬜ | anchor-name; position-anchor; inset-area | **#56** |
| CSS View Transitions L1 | [css-view-transitions-1](https://www.w3.org/TR/css-view-transitions-1/) | ⬜ | view-transition-name (needs JS) | **#57** |
| CSS Fill & Stroke L3 | [fill-stroke-3](https://www.w3.org/TR/fill-stroke-3/) | ⬜ | SVG fill/stroke as CSS (needs SVG) | **#58** |
| CSS Scroll Snap L2 | [css-scroll-snap-2](https://www.w3.org/TR/css-scroll-snap-2/) | ⬜ | snapChanging/snapChanged events | **#59** |

### Out of scope 🚫

| Module | Spec | Reason |
|--------|------|--------|
| CSS Paged Media | [css3-page](https://www.w3.org/TR/css3-page/) | No print support planned |
| CSS Speech | [css3-speech](https://www.w3.org/TR/css3-speech/) | Audio/TTS not in Lumen scope |
| CSS Ruby Annotation | [css3-ruby](https://www.w3.org/TR/css3-ruby/) | Rare; deferred post-Phase 2 |
| CSS Shadow Parts | [css-shadow-parts-1](https://www.w3.org/TR/css-shadow-parts-1/) | Shadow DOM not planned |
| CSS Regions | [css3-regions](https://www.w3.org/TR/css3-regions/) | Deprecated direction by W3C |
| CSSOM JS API | [cssom](https://www.w3.org/TR/cssom/) | Requires JsRuntime (P3) |
| CSS Animation Worklet | [css-animation-worklet-1](https://www.w3.org/TR/css-animation-worklet-1/) | Houdini; post-Phase 2 |
| CSS Paint API | [css-paint-api-1](https://www.w3.org/TR/css-paint-api-1/) | Houdini; post-Phase 2 |
| CSS Layout API | [css-layout-api-1](https://www.w3.org/TR/css-layout-api-1/) | Houdini; post-Phase 2 |
| CSS Typed OM | [css-typed-om-1](https://www.w3.org/TR/css-typed-om-1/) | JS API; P3 territory |
| SVG Fill & Stroke | [fill-stroke-3](https://www.w3.org/TR/fill-stroke-3/) | SVG renderer not in scope Phase 0 |
| CSS Round Display | [css-round-display-1](https://www.w3.org/TR/css-round-display-1/) | Wearable/embedded display; not applicable |
| CSS TV/Mobile/Print Profiles | — | Non-browser profiles |

---

## Full Property Inventory

Properties grouped by module, modules ordered by tier (same as above).  
Implementation lives in `crates/layout/src/style.rs` unless noted.

---

### [T0] Cascade & Inheritance

| Property / Concept | Status | Notes |
|-------------------|--------|-------|
| Specificity | ✅ | (id, class, type) triple |
| `!important` | ✅ | origin override |
| Inheritance | ✅ | inheritable props propagate |
| `inherit` | ✅ | |
| `initial` | ✅ | |
| `unset` | ✅ | inherit if inheritable, else initial |
| `revert` | 🟡 | parsed; UA stylesheet revert ⬜ |
| `revert-layer` | ⬜ | CSS Cascading L5 |

### [T0] Box Model

| Property | Status | Notes |
|----------|--------|-------|
| `display` | ✅ | block/inline/none/flex/inline-flex/grid/inline-grid/inline-block/flow-root/contents/list-item |
| `width` | ✅ | auto, px/em/%, calc/min/max/clamp |
| `height` | ✅ | same as width |
| `min-width` | ✅ | lengths, auto=None |
| `max-width` | ✅ | lengths, none=None |
| `min-height` | ✅ | lengths, auto=None |
| `max-height` | ✅ | lengths, none=None |
| `margin` / `margin-*` | ✅ | auto for centering |
| `padding` / `padding-*` | ✅ | |
| `box-sizing` | ✅ | content-box, border-box |
| `overflow` / `overflow-x` / `overflow-y` | ✅ | visible/hidden/clip; scroll ⬜ rendering |
| `visibility` | ✅ | visible/hidden (space reserved) |
| `opacity` | ✅ | composited layer |
| `aspect-ratio` | ✅ | auto, W/H ratio |
| `text-overflow` | ✅ | clip, ellipsis |
| `float` | 🟡 | parsed; layout algorithm ⬜ |
| `clear` | 🟡 | parsed; clearance ⬜ |
| `-webkit-line-clamp` / `line-clamp` | 🟡 | parsed; multi-line truncation ⬜ |
| `contain-intrinsic-size` | 🟡 | parsed; intrinsic size hint ⬜ |

### [T0] Borders & Outlines

| Property | Status | Notes |
|----------|--------|-------|
| `border` / `border-*` (shorthand) | ✅ | |
| `border-*-width` | ✅ | f32 px |
| `border-*-style` | ✅ | solid/dashed/dotted/double |
| `border-*-color` | ✅ | CssColor; currentColor |
| `border-radius` / `border-*-*-radius` | ✅ | circular; elliptical (rx≠ry) ⬜ |
| `box-shadow` | ✅ | offset/blur/spread/color/inset; multiple |
| `outline` / `outline-*` | ✅ | width/style/color/offset |

### [T0] Colors

| Property | Status | Notes |
|----------|--------|-------|
| `color` | ✅ | named/hex/rgb/rgba/hsl/hsla/oklch; currentColor |
| `background-color` | ✅ | |
| `color-scheme` | 🟡 | parsed; UA switching ⬜ |
| `forced-color-adjust` | 🟡 | parsed; Forced Colors Mode ⬜ |
| `print-color-adjust` / `color-adjust` | 🟡 | parsed/stored; print rendering ⬜ |
| `accent-color` | 🟡 | parsed; UA default ⬜ |
| `color-mix()` | ⬜ | CSS Color L5 |

### [T0] Fonts

| Property | Status | Notes |
|----------|--------|-------|
| `font` / `font-size` / `font-weight` / `font-style` / `font-family` | ✅ | |
| `font-variant` / `font-variant-caps` | 🟡 | small-caps only; all-small-caps ⬜ |
| `font-stretch` | 🟡 | % parsed; matcher ⬜ |
| `font-variation-settings` | ✅ | fvar+avar normalization |
| `font-feature-settings` | ⬜ | OT feature flags |
| `font-size-adjust` | 🟡 | parsed; x-height scaling ⬜ |
| `font-optical-sizing` | 🟡 | parsed; opsz axis ⬜ |
| `font-palette` | ⬜ | CSS Fonts L5 |
| `@font-face` | 🟡 | all descriptors parsed; file loading ⬜ |
| `@font-palette-values` | ⬜ | CSS Fonts L5 |

### [T0] Text Styling

| Property | Status | Notes |
|----------|--------|-------|
| `text-align` | ✅ | start/end/left/center/right; LTR/RTL |
| `text-indent` | ✅ | |
| `text-transform` | ✅ | none/uppercase/lowercase/capitalize |
| `white-space` | ✅ | normal/nowrap/pre/pre-wrap/pre-line — UA default for &lt;pre&gt; |
| `word-spacing` / `letter-spacing` | ✅ | |
| `word-break` / `overflow-wrap` | ✅ | |
| `text-decoration` / `text-decoration-*` | ✅ | line/style/color/thickness |
| `text-shadow` | ✅ | |
| `vertical-align` | ✅ | baseline/top/middle/bottom/sub/super/length/% |
| `text-align-last` | 🟡 | parsed; last-line apply ⬜ |
| `hyphens` | 🟡 | none/manual ✅; auto (HyphenationProvider) ⬜ |
| `tab-size` | ✅ | parsed; \t expanded in pre/pre-wrap; renderer advances cursor by tab_size |
| `line-break` | 🟡 | parsed; CJK-aware breaking ⬜ |
| `text-wrap-mode` / `text-wrap-style` | 🟡 | parsed; integration ⬜ |
| `text-underline-position` / `text-underline-offset` | 🟡 | parsed; paint offset ⬜ |
| `text-emphasis` / `text-emphasis-*` | ✅ | per-char marks rendered (emit_text_emphasis_marks) |

### [T0] Selectors

| Selector | Status | Notes |
|----------|--------|-------|
| `*`, `E`, `.class`, `#id`, `[attr*]` | ✅ | all attribute operators |
| `A B`, `A > B`, `A + B`, `A ~ B` | ✅ | all combinators |
| `:root`, `:first/last-child`, `:nth-*`, `:only-*`, `:empty` | ✅ | |
| `:not(S)` | ✅ | L3 simple; L4 any selector |
| `:hover`, `:active` | 🟡 | parsed; shell wiring partial |
| `:focus`, `:focus-within` | 🟡 | parsed; focus tracking ⬜ |
| `:focus-visible` | ⬜ | Selectors L4 |
| `:link`, `:visited` | 🟡 | parsed; navigation state ⬜ |
| `:target` | ⬜ | fragment navigation |
| `:enabled`, `:disabled`, `:checked` | 🟡 | parsed; form state ⬜ |
| `:is(S)`, `:where(S)`, `:has(S)` | 🟡 | Selectors L4; matching ⬜ |
| `::before`, `::after` | ✅ | block-level ✅; inline ✅ (display:inline/inline-block in IFC) |
| `::first-line`, `::first-letter` | ⬜ | Pseudo-Elements L4 |
| `::marker`, `::placeholder`, `::selection` | ⬜ | Pseudo-Elements L4 |
| `:nth-child(An+B of S)` | ⬜ | Selectors L5 |

### [T0] Flexbox

| Property | Status | Notes |
|----------|--------|-------|
| `flex-direction` / `flex-wrap` / `flex-flow` | ✅ | |
| `flex-grow` / `flex-shrink` / `flex-basis` / `flex` | ✅ | |
| `order` | ✅ | |
| `align-items` / `align-self` / `align-content` | ✅ | |
| `justify-content` | ✅ | |
| `justify-items` / `justify-self` | 🟡 | parsed; grid cells only ⬜ |
| `gap` / `row-gap` / `column-gap` | ✅ | |

### [T0] Transforms

| Property | Status | Notes |
|----------|--------|-------|
| `transform` | ✅ | all 2D functions |
| `transform-origin` | ✅ | pivot via T(o)·M·T(-o) |
| `transform-style` | 🟡 | flat/preserve-3d; 3D context ⬜ |
| `perspective` / `perspective-origin` | 🟡 | parsed; 3D projection ⬜ |
| `backface-visibility` | 🟡 | parsed; 3D flip ⬜ |
| `translate` / `rotate` / `scale` | ⬜ | individual props (Transforms L2) |

### [T0] Logical Properties

| Property | Status | Notes |
|----------|--------|-------|
| `margin-block*` / `margin-inline*` | ✅ | LTR physical mapping |
| `padding-block*` / `padding-inline*` | ✅ | |
| `border-block*` / `border-inline*` | ✅ | |
| `inset-block*` / `inset-inline*` | ✅ | |
| `block-size` / `inline-size` | 🟡 | LTR: height/width; RTL/vertical ⬜ |
| `min/max-block-size` / `min/max-inline-size` | 🟡 | LTR only |

---

### [T1] CSS Custom Properties

| Property | Status | Notes |
|----------|--------|-------|
| `--*` declaration | ✅ | parsing + storage |
| `var()` substitution | 🟡 | partial; recursive custom props ⬜ |
| `@property` | ⬜ | typed custom properties (CSS Properties & Values API) |

### [T1] Transitions

| Property | Status | Notes |
|----------|--------|-------|
| `transition` (shorthand) | 🟡 | |
| `transition-property` | 🟡 | Vec<String>; "all" |
| `transition-duration` / `transition-delay` | 🟡 | Vec<f32> seconds |
| `transition-timing-function` | 🟡 | TimingFunction enum |
| Per-frame interpolation | ⬜ | lerp wiring in shell tick |

### [T1] Animations

| Property | Status | Notes |
|----------|--------|-------|
| `animation` (shorthand) | 🟡 | |
| `animation-name` / `animation-duration` / `animation-delay` | 🟡 | |
| `animation-timing-function` | 🟡 | |
| `animation-iteration-count` / `animation-direction` | 🟡 | |
| `animation-fill-mode` / `animation-play-state` | 🟡 | |
| `animation-timeline` / `animation-range` | ⬜ | Scroll-driven Animations |
| `@keyframes` | 🟡 | parsed; AnimationScheduler::tick ⬜ |

### [T1] CSS Nesting

| Feature | Status | Notes |
|---------|--------|-------|
| Nested rules `&` | ✅ | parse-time expansion: `& sel`, `& > sel`, `& + sel`, `& ~ sel`, `&.cls`; multi-parent + deep nesting |
| `@nest` (legacy) | ⬜ | |

### [T1] Table Layout

| Value | Status | Notes |
|-------|--------|-------|
| `display: table` | 🟡 | parsed; layout engine ⬜ |
| `display: table-row` | 🟡 | parsed |
| `display: table-cell` | 🟡 | parsed |
| `display: table-header-group` / `table-footer-group` | 🟡 | parsed |
| `border-collapse` / `border-spacing` | 🟡 | parsed |
| `caption-side` / `table-layout` | 🟡 | parsed |

### [T1] Positioning (sticky & z-index)

| Property | Status | Notes |
|----------|--------|-------|
| `position: static/relative/absolute/fixed` | ✅ | |
| `position: sticky` | 🟡 | parsed; scroll listener + layout ⬜ |
| `top` / `right` / `bottom` / `left` / `inset` | ✅ | |
| `z-index` | 🟡 | stacking context detection ✅; paint ordering ⬜ |

### [T1] Floats

| Property | Status | Notes |
|----------|--------|-------|
| `float` | 🟡 | left/right/none parsed; float layout ⬜ |
| `clear` | 🟡 | both/left/right parsed; clearance ⬜ |
| `shape-outside` | 🟡 | parsed; float shape wrapping ⬜ |

### [T1] Lists

| Property | Status | Notes |
|----------|--------|-------|
| `list-style` / `list-style-type` | 🟡 | disc/circle/square/decimal/roman parsed; marker render ⬜ |
| `list-style-position` | 🟡 | inside/outside; positioning ⬜ |
| `list-style-image` | 🟡 | url(); image marker ⬜ |
| `counter-reset` / `counter-increment` | 🟡 | Vec<(name,val)>; resolution ⬜ |
| `counter-set` | ⬜ | |
| `@counter-style` | ⬜ | |

### [T1] @layer / Cascade Layers

| Feature | Status | Notes |
|---------|--------|-------|
| `@layer` declaration | ✅ | parsed; cascade ordering wired: layer_priority sort key in compute_style |
| `@import layer()` | 🟡 | URL parsed; layer() modifier ⬜ |
| `revert-layer` | ⬜ | |

### [T1] Selectors L4

| Selector | Status | Notes |
|----------|--------|-------|
| `:is(S)` | 🟡 | parsed; full matching ⬜ |
| `:where(S)` | 🟡 | parsed; zero-specificity ⬜ |
| `:has(S)` | 🟡 | parsed; relational matching ⬜ |

### [T1] Media Queries

| Feature | Status | Notes |
|---------|--------|-------|
| `@media` | 🟡 | width/height/orientation condition ✅; re-eval on resize ⬜ |
| `prefers-color-scheme` | ✅ | |
| `prefers-reduced-motion` | 🟡 | parsed; skip animation ⬜ |
| `hover`, `pointer` | ⬜ | |
| `prefers-contrast` / `prefers-reduced-data` | ⬜ | MQ L5 |

---

### [T2] Filters

| Property | Status | Notes |
|----------|--------|-------|
| `filter` | ✅ | GPU pipeline: blur/brightness/contrast/grayscale/hue-rotate/invert/saturate/sepia/drop-shadow |
| `backdrop-filter` | 🟡 | parsed; backdrop GPU compositing ⬜ |

### [T2] Clipping & Masking

| Property | Status | Notes |
|----------|--------|-------|
| `clip-path` | ✅ | inset/circle/ellipse/polygon rendered (bbox-clip); complex paths ⬜ |
| `clip-rule` | ⬜ | evenodd/nonzero |
| `mask` (shorthand) | 🟡 | |
| `mask-image` | 🟡 | GPU mask composite pipeline 🟡 (PushMask/PopMask); full alpha compositing ⬜ |
| `mask-repeat` / `mask-size` / `mask-position` | 🟡 | parsed |
| `mask-origin` / `mask-clip` / `mask-composite` / `mask-mode` | 🟡 | parsed |

### [T2] Compositing

| Property | Status | Notes |
|----------|--------|-------|
| `mix-blend-mode` | 🟡 | 16 modes parsed; blend pipeline ⬜ |
| `isolation` | 🟡 | auto/isolate; stacking context ⬜ |

### [T2] Pseudo-Elements

| Element | Status | Notes |
|---------|--------|-------|
| `::before` / `::after` | ✅ | block-level generation ✅; inline ✅ |
| `::first-line` / `::first-letter` | ⬜ | line split required |
| `::marker` | ⬜ | list marker box |
| `::placeholder` | ⬜ | input placeholder |
| `::selection` | ⬜ | text selection highlight |

### [T2] Backgrounds & Images

| Property | Status | Notes |
|----------|--------|-------|
| `background` (shorthand) | 🟡 | single layer ✅; multiple ⬜ |
| `background-color` | ✅ | |
| `background-image` | 🟡 | url() ✅; linear/radial gradient ✅; conic-gradient ⬜ |
| `background-repeat` / `background-position` / `background-size` | ✅ | |
| `background-attachment` | 🟡 | parsed; scroll/fixed ⬜ |
| `background-origin` / `background-clip` | 🟡 | parsed; text clip ⬜ |
| `image-rendering` | ✅ | bilinear/nearest sampler |
| `object-fit` / `object-position` | ✅ | |
| `image-set()` | ⬜ | CSS Images L4 |
| `conic-gradient()` | ⬜ | CSS Images L4 |
| `cross-fade()` | ⬜ | CSS Images L4 |

### [T2] CSS Grid

| Property | Status | Notes |
|----------|--------|-------|
| `grid-template-columns` / `grid-template-rows` | 🟡 | px/fr/auto/repeat()/minmax() ✅ |
| `grid-template-areas` | ✅ | parsed + named area placement in lay_out_grid; GridLine::Named resolved |
| `grid-template` / `grid` (super-shorthand) | 🟡 | |
| `grid-auto-columns` / `grid-auto-rows` | 🟡 | |
| `grid-auto-flow` | 🟡 | row/column ✅; dense ⬜ |
| `grid-column*` / `grid-row*` / `grid-area` | 🟡 | auto/int/span |
| `subgrid` | ⬜ | CSS Grid L2 |
| `masonry` | ⬜ | CSS Grid L3 |

### [T2] Intrinsic Sizing

| Value | Status | Notes |
|-------|--------|-------|
| `min-content` | ⬜ | CSS Intrinsic Sizing L3 |
| `max-content` | ⬜ | |
| `fit-content` / `fit-content(L)` | ⬜ | |
| `stretch` / `available` | ⬜ | |

### [T2] Transforms L2 / 3D

| Property | Status | Notes |
|----------|--------|-------|
| `perspective` / `perspective-origin` | 🟡 | parsed; 3D projection ⬜ |
| `transform-style: preserve-3d` | 🟡 | parsed; 3D context ⬜ |
| `backface-visibility` | 🟡 | parsed; 3D flip ⬜ |
| `translate` / `rotate` / `scale` (individual) | ⬜ | CSS Transforms L2 |

### [T2] Values (advanced)

| Value | Status | Notes |
|-------|--------|-------|
| `env()` | ⬜ | safe-area-inset-*, titlebar-area-* |
| `attr()` with type | 🟡 | string only; type casting ⬜ |
| `cqw` / `cqh` / `cqi` / `cqb` | ⬜ | container query units |
| `svh` / `dvh` / `lvh` / `svw` / `dvw` / `lvw` | ✅ | = vh/vw (Phase 0 fixed viewport) |
| `svmin`/`dvmin`/`lvmin`, `svmax`/`dvmax`/`lvmax` | ✅ | = vmin/vmax |

---

### [T3] Scroll Snap

| Property | Status | Notes |
|----------|--------|-------|
| `scroll-snap-type` / `scroll-snap-align` / `scroll-snap-stop` | ✅ | find_scroll_snap_y + proximity snapping |
| `scroll-margin*` / `scroll-padding*` | 🟡 | parsed |
| `scroll-behavior` | 🟡 | auto/smooth parsed |
| `overscroll-behavior*` | 🟡 | parsed; gesture boundary ⬜ |
| `scroll-timeline` / `view-timeline` | ⬜ | Scroll-driven Animations |

### [T3] Multi-column

| Property | Status | Notes |
|----------|--------|-------|
| `column-count` / `column-width` / `columns` | ✅ | |
| `column-gap` | ✅ | |
| `column-rule` / `column-rule-*` | ✅ | rendered between columns (solid/dashed/dotted) |
| `column-span` | 🟡 | parsed; spanning ⬜ |
| `column-fill` | 🟡 | parsed; balancing ⬜ |
| `break-before` / `break-after` / `break-inside` | 🟡 | parsed/stored; fragmentation algorithm ⬜ |
| `orphans` / `widows` | 🟡 | parsed/stored; paged-media layout ⬜ |

### [T3] Container Queries

| Feature | Status | Notes |
|---------|--------|-------|
| `container-type` / `container-name` | ✅ | |
| `@container` | 🟡 | condition matching ✅; 2nd-pass re-layout ✅; cq* units ⬜ |
| Container query units (`cq*`) | ⬜ | |

### [T3] Counters & Lists (rendering)

| Property | Status | Notes |
|----------|--------|-------|
| `counter-reset` / `counter-increment` | 🟡 | parsed; resolution ⬜ |
| `counter()` / `counters()` in `content` | 🟡 | parsed; rendering ⬜ |
| `@counter-style` | ⬜ | custom counter symbols |

### [T3] Content & Pseudo-element content

| Property | Status | Notes |
|----------|--------|-------|
| `content` | 🟡 | string ✅; attr() ⬜; counter() ⬜; url() ⬜ |

### [T3] Box Alignment (grid)

| Property | Status | Notes |
|----------|--------|-------|
| `justify-items` | 🟡 | parsed; grid cells ⬜ |
| `justify-self` | 🟡 | parsed; grid items ⬜ |
| `place-items` / `place-self` / `place-content` | 🟡 | shorthands; grid ⬜ |

### [T3] Inline / Line Box

| Property | Status | Notes |
|----------|--------|-------|
| `line-height` | 🟡 | parsed; leading in line box ⬜ |
| `line-height-step` | ⬜ | CSS Rhythmic Sizing |

### [T3] Scrollbars

| Property | Status | Notes |
|----------|--------|-------|
| `scrollbar-width` / `scrollbar-color` / `scrollbar-gutter` | 🟡 | parsed; rendering ⬜ |

### [T3] UI / Input

| Property | Status | Notes |
|----------|--------|-------|
| `cursor` | ✅ | 17 keywords; OS cursor via winit |
| `user-select` | 🟡 | HitTestResult wire-up ✅; text selection enforcement ⬜ |
| `pointer-events` | 🟡 | none ✅ (cursor wired); auto/shell enforcement ⬜ |
| `touch-action` | 🟡 | parsed; gesture ⬜ |
| `resize` | 🟡 | parsed; drag-UI ⬜ |
| `appearance` | 🟡 | parsed; form widgets ⬜ |
| `caret-color` | 🟡 | parsed; text input ⬜ |
| `will-change` | 🟡 | parsed; GPU hints ⬜ |

### [T3] At-Rules

| Rule | Status | Notes |
|------|--------|-------|
| `@charset` | ✅ | parsed; ignored (UTF-8 only) |
| `@namespace` | ✅ | parsed; no XML namespaces |
| `@import` | 🟡 | URL extracted; file loading ⬜ |
| `@media` | 🟡 | condition eval partial; resize hook ⬜ |
| `@supports` | 🟡 | parsed; feature detection ⬜ |
| `@font-face` | 🟡 | descriptors parsed; loading ⬜ |
| `@keyframes` | 🟡 | parsed; scheduler ⬜ |
| `@layer` | ✅ | parsed; cascade ordering ✅ |
| `@container` | 🟡 | condition matching ✅; 2nd-pass re-layout ✅; cq* units ⬜ |
| `@color-profile` | ⬜ | CSS Color L5 |
| `@font-palette-values` | ⬜ | CSS Fonts L5 |
| `@counter-style` | ⬜ | CSS Counter Styles L3 |
| `@scope` | ⬜ | CSS Scoping |
| `@function` | ⬜ | CSS Functions & Mixins |

### [T3] Units & Values

| Value/Unit | Status | Notes |
|------------|--------|-------|
| `px`/`em`/`rem`/`%` | ✅ | |
| `vh`/`vw`/`vmin`/`vmax` | ✅ | |
| `pt`/`pc`/`in`/`cm`/`mm` | ✅ | absolute |
| `ch`/`ex` | ✅ | approximated as 0.5em (Phase 0) |
| `cap`/`lh` | ✅ | approximated as 0.7em / 1.2em (Phase 0) |
| `Q` | ✅ | = 0.25mm → px |
| `calc()` | ✅ | arithmetic |
| `min()`/`max()`/`clamp()` | ✅ | comparison |
| `var()` | 🟡 | partial substitution |
| `url()` | ✅ | |
| `svh`/`dvh`/`lvh`/`svw`/`dvw`/`lvw` | ✅ | = vh/vw (Phase 0 fixed viewport) |
| `svmin`/`dvmin`/`lvmin`/`svmax`/`dvmax`/`lvmax` | ✅ | = vmin/vmax |
| `cqw`/`cqh`/`cqi`/`cqb` | ⬜ | container query units |
| `env()` | ⬜ | |
| `attr()` | 🟡 | string; type casting ⬜ |
| `color-mix()` | ⬜ | CSS Color L5 |
| `counter()`/`counters()` | 🟡 | in content; resolution ⬜ |
| `linear()` | ⬜ | CSS Easing L2 |

---

### [T4] Writing Modes

| Property | Status | Notes |
|----------|--------|-------|
| `direction` | 🟡 | ltr/rtl; fragment mirroring ✅; UBA ⬜ |
| `writing-mode` | 🟡 | parsed; vertical-rl/lr layout ⬜ |
| `text-orientation` | 🟡 | parsed; glyph rotation ⬜ |
| `unicode-bidi` | 🟡 | parsed; full bidi ⬜ |

### [T4] Shapes & Motion Path

| Property | Status | Notes |
|----------|--------|-------|
| `shape-outside` / `shape-margin` / `shape-image-threshold` | 🟡 | parsed; float wrapping ⬜ |
| `offset` / `offset-path` / `offset-distance` / `offset-rotate` / `offset-anchor` | 🟡 | parsed; motion layout ⬜ |

### [T4] Containment (advanced)

| Property | Status | Notes |
|----------|--------|-------|
| `contain` | 🟡 | size/layout/paint enforcement ✅; content-visibility skip-content ⬜ |
| `content-visibility` | 🟡 | parsed; skip-content ⬜ |

### [T4] Scroll-driven Animations

| Property | Status | Notes |
|----------|--------|-------|
| `scroll-timeline` / `view-timeline` | ⬜ | |
| `animation-timeline` / `animation-range` | ⬜ | |

### [T4] Anchor Positioning

| Property | Status | Notes |
|----------|--------|-------|
| `anchor-name` / `position-anchor` / `inset-area` | ⬜ | entirely new spec |
| `anchor()` / `anchor-size()` functions | ⬜ | |

### [T4] Color L5

| Feature | Status | Notes |
|---------|--------|-------|
| `color-mix()` | ⬜ | |
| `color-contrast()` | ⬜ | |
| Relative color syntax `oklch(from ...)` | ⬜ | |
| `@color-profile` | ⬜ | |

---

## P4 Work Queue

Ordered list of 🟡→✅ promotions for the P4 developer. One item = one feature branch.

| # | Property / Feature | Effort | Blocker |
|---|-------------------|--------|---------|
| 1 | `var()` full recursive substitution | M | none |
| 2 | `transition` interpolation (per-frame lerp) | M | easing functions |
| 3 | `@keyframes` AnimationScheduler::tick wiring | L | transitions done |
| 4 | CSS Nesting — nested rule parser | L | none |
| 5 | `position: sticky` layout + scroll listener | M | none |
| 6 | `z-index` stacking context paint ordering | M | none |
| 7 | `float` + `clear` layout algorithm | L | none |
| 8 | `list-style-type` marker rendering | S | none |
| 9 | `@layer` cascade ordering | ✅ | done 2026-05-22 |
| 10 | `:is()` / `:where()` / `:has()` matching | M | none |
| 11 | `@media` resize hook re-evaluation | S | shell event |
| 12 | `filter` GPU offscreen pass | L | wgpu pipeline |
| 13 | `clip-path` basic shapes (inset/circle/ellipse/polygon) | M | none |
| 14 | `mix-blend-mode` blend pipeline | L | compositing |
| 15 | `::first-letter` / `::first-line` line split | M | inline layout |
| 16 | `::marker` rendering | S | float/list |
| 17 | `conic-gradient()` | S | gradient renderer |
| 18 | Multiple backgrounds | M | background layer stack |
| 19 | `grid-template-areas` named placement | ✅ | GridLine::Named + find_named_area + resolve_named_lines 2026-05-22 |
| 20 | `@font-face` actual file loading | L | network/P3 |
| 21 | `min-content` / `max-content` / `fit-content` | L | layout engine |
| 22 | `overflow: scroll` scrollable containers | L | shell scroll |
| 23 | `border-radius` elliptical (rx≠ry) | S | paint command |
| 24 | `column-rule` rendering | S | paint |
| 25 | `line-height` leading in line box | M | inline layout |
| 26 | Scroll snap shell integration | M | scroll event |
| 27 | `@container` 2nd-pass execution | L | container-type done |
| 28 | `backdrop-filter` GPU compositing pass | L | wgpu pipeline |
| 29 | `writing-mode: vertical-*` axis swap | L | layout engine |
| 30 | `subgrid` track inheritance | XL | grid engine |
