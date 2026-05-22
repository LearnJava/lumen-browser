# CSS Snapshot 2026 — Lumen Compliance Report

Source: https://www.w3.org/TR/css-2026/ (W3C Group Note, 26 March 2026)  
Checked: 2026-05-20

Legend: ✅ implemented & rendered · 🟡 parsed/stored, not rendered · ⬜ not implemented

---

## §2.1 Official Definition (fully stabilised)

### CSS Level 2
| Property | Status | Notes |
|---|---|---|
| `display` | ✅ | block, inline, none; flex via CSS Flexbox |
| `visibility` | ✅ | visible / hidden |
| `color` | ✅ | includes currentColor, rgb/rgba/hsl/oklch |
| `background-color` | ✅ | |
| `margin` / `margin-*` | ✅ | including `auto` for centering |
| `padding` / `padding-*` | ✅ | |
| `border` / `border-*` | ✅ | width, style (solid/dashed/dotted/double), color |
| `width`, `height` | ✅ | |
| `min-width`, `max-width` | ✅ | |
| `min-height`, `max-height` | ✅ | |
| `position` | 🟡 | stored; offsets top/left/right/bottom parsed; real positioned layout not applied |
| `top`, `right`, `bottom`, `left` | 🟡 | parsed; not applied in layout |
| `z-index` | 🟡 | stored; stacking context detection logic present; paint ordering TBD |
| `overflow` | ✅ | hidden / visible / scroll (clip applied) |
| `overflow-x`, `overflow-y` | ✅ | |
| `list-style-type` | 🟡 | parsed; list markers not rendered |
| `list-style-position` | 🟡 | parsed |
| `list-style-image` | 🟡 | parsed |
| `list-style` | 🟡 | shorthand parsed |
| `cursor` | 🟡 | stored; shell does not yet switch OS cursor |
| `direction` | 🟡 | stored + RTL inline layout: Start/End resolve, fragment mirroring via align_lines; full UBA deferred |
| `vertical-align` | ✅ | baseline/top/middle/bottom/sub/super/length/percent applied as per-frag y_offset in InlineFrag |
| `content` | 🟡 | string content generated for `::before`/`::after` block containers; `attr()`/`counter()` — deferred |
| `counter-reset` | 🟡 | parsed |
| `counter-increment` | 🟡 | parsed |
| `table-*` | ⬜ | table layout not implemented |

### CSS Syntax Level 3
Handled by `lumen-css-parser`. Custom properties (`--name`) and `var()` substitution — ✅.

### CSS Values and Units Level 3
| Feature | Status |
|---|---|
| `px`, `em`, `rem`, `%`, `vw`, `vh` | ✅ |
| `vmin`, `vmax` | ✅ |
| `pt`, `pc`, `in`, `cm`, `mm`, `Q` | ✅ parsed → px (96dpi reference pixel) |
| `ch`, `ex` | ✅ approximated as 0.5em (Phase 0, no font metrics API) |
| `cap`, `lh` | ✅ approximated as 0.7em / 1.2em (Phase 0) |
| `svh`, `svw`, `dvh`, `dvw`, `lvh`, `lvw` | ✅ = vh/vw (Phase 0 fixed viewport) |
| `svmin`/`dvmin`/`lvmin`, `svmax`/`dvmax`/`lvmax` | ✅ = vmin/vmax (Phase 0) |
| `calc()` | ✅ |
| `min()`, `max()`, `clamp()` | ✅ |
| `currentColor` | ✅ |
| `initial`, `inherit`, `unset` | ✅ |

### CSS Box Model Level 3
| Property | Status |
|---|---|
| `box-sizing` | ✅ | content-box / border-box |
| `margin`, `padding` (all four sides) | ✅ | |

### CSS Color Level 4
| Property/Feature | Status | Notes |
|---|---|---|
| `color` | ✅ | rgb/rgba, hsl/hsla, oklch, hex, named colors |
| `opacity` | ✅ | renders as separate composited layer |
| Color spaces (sRGB, display-p3, oklch) | 🟡 | parsed, `color_space` stored; GPU path not wide-gamut yet |

### CSS Backgrounds and Borders Level 3
| Property | Status | Notes |
|---|---|---|
| `background-color` | ✅ | |
| `background-image` | 🟡 | `url()` painted with repeat/position/size; `linear-gradient`/`radial-gradient` emit DrawLinearGradient/DrawRadialGradient; P2 renders as avg-stop fill pending GPU gradient pipeline |
| `background-repeat` | ✅ | repeat/no-repeat/repeat-x/repeat-y applied; round/space ≈ repeat |
| `background-position` | ✅ | px and % offsets applied to tile origin |
| `background-size` | ✅ | auto/cover/contain/length; cover and contain require intrinsic image dimensions |
| `background-attachment` | 🟡 | parsed |
| `background-origin` | 🟡 | parsed |
| `background-clip` | 🟡 | parsed |
| `background` (shorthand) | 🟡 | color extracted ✅; image layer 🟡 |
| `border-*-width` | ✅ | |
| `border-*-style` | ✅ | solid / dashed / dotted / double |
| `border-*-color` | ✅ | |
| `border-radius` / `border-*-*-radius` | ✅ | elliptical border-radius not yet |
| `box-shadow` | ✅ | including inset, blur, spread |
| `outline` | ✅ | width, style, color; outline-offset |

### CSS Fonts Level 3
| Property | Status |
|---|---|
| `font-family` | ✅ stored; Phase 0 always renders Inter |
| `font-size` | ✅ |
| `font-weight` | ✅ |
| `font-style` | ✅ |
| `font-variant` | 🟡 small-caps parsed |
| `font-stretch` | 🟡 stored; not applied by font matcher |
| `font` (shorthand) | ✅ |
| `@font-face` | 🟡 family/src/weight/style/stretch/display/unicode-range/variant/feature-settings/variation-settings parsed; no fetch/font-loading yet |

### CSS Transforms Level 1
| Property | Status |
|---|---|
| `transform` | ✅ translate/translateX/Y · rotate · scale/X/Y · skewX/Y · matrix() · combined; PushTransform/PopTransform in display list; transform-stack in renderer |
| `transform-origin` | ✅ px values; pivot applied via T(origin)·M·T(-origin) |

### CSS Compositing and Blending Level 1
| Property | Status |
|---|---|
| `opacity` | ✅ |
| `mix-blend-mode` | 🟡 parsed; blend pipeline not implemented |
| `isolation` | 🟡 parsed |

### CSS Multi-column Layout Level 1
| Property | Status |
|---|---|
| `column-count` | ✅ | N equal columns; used with column-width as max cap |
| `column-width` | ✅ | computes N = floor((avail + gap) / (width + gap)) |
| `columns` | ✅ | shorthand resolved |
| `column-gap` | ✅ | spacing between columns (was ✅ for flex; now also multi-col) |
| `column-rule-*` | 🟡 parsed; column rule rendering — deferred |
| `column-span` | 🟡 parsed; spanning not implemented |
| `column-fill` | 🟡 parsed; balanced layout is default |

### CSS Flexible Box Layout Level 1 ← **primary**
| Property | Status | Notes |
|---|---|---|
| `display: flex` | ✅ | |
| `flex-direction` | ✅ | row / column / row-reverse / column-reverse |
| `flex-wrap` | ✅ | nowrap / wrap / wrap-reverse |
| `flex-flow` | ✅ | shorthand |
| `flex-grow` | ✅ | |
| `flex-shrink` | ✅ | |
| `flex-basis` | ✅ | length / auto / content |
| `flex` | ✅ | shorthand |
| `justify-content` | ✅ | flex-start / flex-end / center / space-between / space-around / space-evenly |
| `align-items` | ✅ | stretch / flex-start / flex-end / center / baseline |
| `align-self` | ✅ | |
| `align-content` | ✅ | multi-line; flex-start / flex-end / center / space-between / space-around / stretch |
| `gap`, `row-gap`, `column-gap` | ✅ | |
| `order` | ✅ | integer; sorts flex items by order value (stable sort) |

### CSS Basic User Interface Level 3
| Property | Status |
|---|---|
| `box-sizing` | ✅ |
| `outline` | ✅ |
| `outline-offset` | ✅ |
| `cursor` | 🟡 parsed; OS cursor not switched |
| `resize` | 🟡 | parsed/stored (none/both/horizontal/vertical/block/inline); drag-resize UI — P3 task |

### CSS Counter Styles Level 3
`list-style-type` values are parsed. Counter rendering itself — 🟡.

---

## §2.2 Reliable Candidate Recommendations

### CSS Scroll Snap Level 1
| Property | Status |
|---|---|
| `scroll-snap-type` | 🟡 parsed |
| `scroll-snap-align` | 🟡 parsed |
| `scroll-snap-stop` | 🟡 parsed |
| `scroll-margin-*` | 🟡 parsed |
| `scroll-padding-*` | 🟡 parsed |

### CSS Scrollbars Styling Level 1
| Property | Status |
|---|---|
| `scrollbar-width` | 🟡 parsed |
| `scrollbar-color` | 🟡 parsed |
| `scrollbar-gutter` | 🟡 parsed |

### CSS Grid Layout Level 1 / Level 2
| Property | Status |
|---|---|
| `display: grid` | ⬜ not implemented |
| All `grid-*` properties | ⬜ |

### CSS Color Adjustment Level 1
| Property | Status |
|---|---|
| `color-scheme` | 🟡 | parsed/stored (normal/light/dark/light dark/dark light/only light/only dark); UA theme switching — P2 |
| `forced-color-adjust` | 🟡 | parsed/stored (auto/none/preserve-parent-color); Forced Colors Mode application — P2 |
| `print-color-adjust` / `color-adjust` | 🟡 | parsed/stored (economy/exact); `color-adjust` legacy alias handled; print rendering — deferred |

---

## §2.3 Fairly Stable Modules

### CSS Display Level 3
| Value | Status |
|---|---|
| `block` | ✅ |
| `inline` | ✅ |
| `inline-block` | ✅ |
| `flex` | ✅ |
| `none` | ✅ |
| `grid` | 🟡 | parsed/stored; real grid layout — deferred (4B.6-7) |
| `inline-grid` | 🟡 | parsed/stored |
| `flow-root` | 🟡 | parsed/stored; treated as Block in layout |
| `contents` | 🟡 | parsed/stored; box-generation semantics — deferred |
| `list-item` | 🟡 | parsed/stored; marker box — deferred |
| `table` family (`table`, `inline-table`, `table-row-group`, `table-header-group`, `table-footer-group`, `table-row`, `table-column-group`, `table-column`, `table-cell`, `table-caption`) | 🟡 | parsed/stored; UA defaults for `<table>`, `<tr>`, `<td>` etc.; table layout — deferred |

### CSS Fragmentation Level 3
| Property | Status |
|---|---|
| `break-before`, `break-after`, `break-inside` | 🟡 parsed |
| `orphans`, `widows` | 🟡 parsed | parse + store; real fragmentation hints — deferred (requires paged-media layout) |

### CSS Box Alignment Level 3
Implemented for flex containers. Grid not applicable (grid not implemented).

| Property | Status |
|---|---|
| `justify-content` | ✅ (flex) |
| `align-items` | ✅ (flex) |
| `align-self` | ✅ (flex) |
| `align-content` | ✅ (flex multi-line) |
| `justify-items` | 🟡 parsed |
| `justify-self` | 🟡 parsed |
| `place-*` shorthands | 🟡 parsed |

### CSS Text Level 3
| Property | Status | Notes |
|---|---|---|
| `text-align` | ✅ | start / end / left / right / center; Start/End resolve per direction (CSS Text L3 §7.1) |
| `text-align-last` | 🟡 | parsed/stored (auto/start/end/left/right/center/justify); applies to last line |
| `text-indent` | ✅ | |
| `letter-spacing` | ✅ | |
| `word-spacing` | ✅ | |
| `white-space` | ✅ | normal / nowrap / pre / pre-wrap / pre-line |
| `overflow-wrap` / `word-wrap` | ✅ | |
| `word-break` | ✅ | |
| `line-break` | 🟡 | parsed/stored (auto/loose/normal/strict/anywhere); CJK line-break — deferred |
| `hyphens` | 🟡 parsed; no hyphenation engine |
| `tab-size` | 🟡 parsed; tab rendering partial |
| `text-transform` | ✅ | uppercase / lowercase / capitalize |

### CSS Text Decoration Level 3
| Property | Status |
|---|---|
| `text-decoration-line` | ✅ underline / overline / line-through |
| `text-decoration-color` | ✅ |
| `text-decoration-style` | ✅ solid / dashed / dotted / wavy / double |
| `text-decoration-thickness` | ✅ | `resolve_decoration_thickness()` in `display_list.rs`; auto/from-font=7%·em, length=px, pct=frac·em |
| `text-shadow` | ✅ |
| `text-underline-position` | 🟡 parsed | auto / from-font / under / left / right; real offset in underline paint — P2 task |

### CSS Masking Level 1
| Property | Status |
|---|---|
| `clip-path` | 🟡 parsed (basic shapes); clipping not applied in paint |
| `mask-image` | 🟡 display list + renderer (URL: GPU alpha-mask via mask_composite_pipeline; gradient: Phase 0 fallback at full opacity) |
| `mask-repeat`, `mask-size` | 🟡 parsed + wired (URL mask tiling via PopMask composite pass; gradient masks pending) |

### CSS Text Emphasis (Level 4 / Text Decoration Level 4)
| Property | Status |
|---|---|
| `text-emphasis-style` | 🟡 parsed |
| `text-emphasis-color` | 🟡 parsed |
| `text-emphasis-position` | 🟡 parsed |
| `text-emphasis` | 🟡 parsed |

---

## §2.4 Modules with Rough Interoperability

### CSS Transitions Level 1
| Property | Status |
|---|---|
| `transition-property` | 🟡 parsed; `TransitionScheduler::sync+tick` wires opacity/color/background-color/transform; P2 compositor integration pending |
| `transition-duration` | 🟡 parsed; used by TransitionScheduler |
| `transition-delay` | 🟡 parsed; used by TransitionScheduler |
| `transition-timing-function` | 🟡 parsed; used by TransitionScheduler |
| `transition` | 🟡 parsed |

### CSS Animations Level 1
| Property | Status |
|---|---|
| `animation-name` | 🟡 parsed |
| `animation-duration` | 🟡 parsed |
| `animation-timing-function` | 🟡 parsed |
| `animation-delay` | 🟡 parsed |
| `animation-iteration-count` | 🟡 parsed |
| `animation-direction` | 🟡 parsed |
| `animation-fill-mode` | 🟡 parsed |
| `animation-play-state` | 🟡 parsed |
| `animation` | 🟡 parsed |
| `@keyframes` | 🟡 parsed; `AnimationScheduler::sync+tick` wires @keyframes → `AnimatedStyle` per node; P2 compositor integration pending |

### CSS Will Change Level 1
| Property | Status |
|---|---|
| `will-change` | 🟡 parsed |

### Filter Effects Level 1
| Property | Status |
|---|---|
| `filter` | 🟡 parsed (blur/brightness/contrast/grayscale/etc.); not applied in paint |

### Filter Effects Level 2
| Property | Status | Notes |
|---|---|---|
| `backdrop-filter` | 🟡 | parsed/stored (same FilterFn list as `filter`); backdrop compositing — P2 task |

### CSS Box Sizing Level 3
| Property | Status |
|---|---|
| `box-sizing` | ✅ |
| `aspect-ratio` | ✅ enforced in block/flex/grid layout (border-box, height auto only) |

### CSS Transforms Level 2
| Property | Status |
|---|---|
| `perspective` | 🟡 parsed |
| `transform` 3D functions | 🟡 parsed |

### CSS Lists and Counters Level 3
| Property | Status |
|---|---|
| `list-style-*` | 🟡 parsed |
| `counter-reset`, `counter-increment` | 🟡 parsed |
| `content` | 🟡 string generation for `::before`/`::after` block containers; attr()/counter() — deferred |

### CSS Positioned Layout Level 3
| Property | Status |
|---|---|
| `position: static` | ✅ |
| `position: relative` | ✅ | `shift_tree` in `box_tree.rs` applies left/top/right/bottom after normal flow |
| `position: absolute` | 🟡 stored; OOF layout not implemented |
| `position: fixed` | 🟡 stored |
| `position: sticky` | 🟡 stored |
| `inset` (shorthand) | 🟡 parsed |
| `z-index` | 🟡 stored |

### CSS Fonts Level 4
| Property | Status |
|---|---|
| `font-variant-caps` | 🟡 parsed |
| `font-stretch` (% values) | 🟡 parsed |
| `font-size-adjust` | 🟡 | parsed/stored (none/auto/<number>); actual x-height based scaling — deferred (requires font metrics) |

### CSS Nesting Level 1
| Feature | Status |
|---|---|
| `&` selector nesting | ✅ | parse-time expansion: `& sel`, `& > sel`, `& + sel`, `& ~ sel`, `&.cls` + multi-parent + deep nesting |

### CSS Logical Properties Level 1
| Property | Status |
|---|---|
| `inset-inline-*`, `inset-block-*` | ✅ parse+store (LTR) |
| `margin-inline-*`, `margin-block-*` | ✅ parse+store (LTR) |
| `padding-inline-*`, `padding-block-*` | ✅ parse+store (LTR) |
| `border-inline-*`, `border-block-*` | ✅ parse+store (LTR) |

### CSS Overflow Scrolling
| Property | Status |
|---|---|
| `scroll-behavior` | 🟡 parsed |
| `overscroll-behavior` | 🟡 parsed |
| `text-overflow` | ✅ | clip (default) and ellipsis; truncation in layout via TextMeasurer |

### CSS Overflow Level 4
| Property | Status | Notes |
|---|---|---|
| `-webkit-line-clamp` / `line-clamp` | 🟡 parsed | parse + store; visual truncation after N lines — deferred |

### CSS Sizing Level 4
| Property | Status |
|---|---|
| `aspect-ratio` | ✅ enforced in block/flex/grid layout (border-box, height auto only) |

### CSS Images Level 3
| Property | Status |
|---|---|
| `object-fit` | ✅ |
| `object-position` | ✅ |
| `image-rendering` | ✅ | auto/smooth → bilinear sampler; pixelated/crisp-edges → nearest sampler; per-image bind groups in GPU renderer |

### CSS UI Level 4 extras
| Property | Status |
|---|---|
| `user-select` | 🟡 parsed |
| `caret-color` | 🟡 parsed |
| `accent-color` | 🟡 parsed |
| `pointer-events` | 🟡 parsed |
| `touch-action` | 🟡 | parsed/stored (auto/none/pan-x/pan-y/pan-left/pan-right/pan-up/pan-down/pinch-zoom/manipulation) |
| `appearance` / `-webkit-appearance` | 🟡 | parsed/stored (auto/none/compat) |

### CSS Containment Level 3
| Property | Status | Notes |
|---|---|---|
| `contain` | 🟡 | parsed/stored + enforced: `size`→auto height=0, `paint`→PushClipRect (border-box clip), `layout`/`paint`→establishes containing block for abs-pos descendants; `style`/inline-size — Phase 1 |
| `content-visibility` | 🟡 | parsed/stored (visible/auto/hidden); skip-content optimization — deferred |

### CSS Container Queries Level 1
| Property | Status | Notes |
|---|---|---|
| `container-type` | 🟡 | parsed/stored (normal/size/inline-size); @container size queries applied in 2nd layout pass |
| `container-name` | 🟡 | parsed/stored as `Vec<String>`; named @container rules matched against container-name |
| `@container` rule | 🟡 | min-width/max-width/width/min-height/max-height/height + and/or/not; nested containers; style re-applied + re-layout in 2nd pass |

### CSS Shapes Level 1
| Property | Status | Notes |
|---|---|---|
| `shape-outside` | 🟡 | parsed/stored as raw string (basic-shape/url/box); shape layout offset — deferred |
| `shape-margin` | 🟡 | parsed/stored (non-negative length/percentage) |
| `shape-image-threshold` | 🟡 | parsed/stored (0.0–1.0 clamped); image alpha extraction — deferred |

### CSS Motion Path Level 1
| Property | Status | Notes |
|---|---|---|
| `offset-path` | 🟡 | parsed/stored as raw string (path()/ray()/url()); motion layout — deferred |
| `offset-distance` | 🟡 | parsed/stored (length/percentage along path) |
| `offset-rotate` | 🟡 | parsed/stored (auto/reverse/`<angle>`/`auto <angle>`) |
| `offset-anchor` | 🟡 | parsed/stored using ObjectPosition (auto → None) |

---

## Summary by module

| Module | ✅ | 🟡 | ⬜ |
|---|---|---|---|
| CSS Level 2 core (box model, display, color) | ✅ | partial | table layout |
| CSS Flexbox L1 | ✅ | — | — |
| CSS Box Alignment L3 | ✅ (flex) | grid/block | — |
| CSS Text L3 | ✅ most | hyphens, tab, line-break | — |
| CSS Text Decoration L3 | ✅ most | thickness | underline-position |
| CSS Backgrounds L3 | ✅ (color/border/shadow/repeat/position/size) | clip/origin/attachment layers | — |
| CSS Fonts L3/L4 | ✅ (size/weight/style) | stretch/variant | @font-face parse✅ no fetch |
| CSS Compositing L1 | ✅ opacity | blend-mode/isolation | — |
| CSS Images L3 | ✅ object-fit/position/image-rendering | — | — |
| CSS Transforms L1 | — | parse-only | paint apply |
| CSS Animations L1 | — | parse-only | scheduler |
| CSS Transitions L1 | — | parse-only | engine |
| CSS Filters L1 | — | parse-only | paint apply |
| CSS Positioned Layout L3 | ✅ static | others parse-only | OOF layout |
| CSS Grid L1/L2 | — | parse+store (grid/inline-grid) | layout algorithm |
| CSS Logical Properties L1 | ✅ parse+store (LTR) | — | full RTL/vertical |
| CSS Nesting L1 | ✅ | — | — |
| CSS Multi-column L1 | ✅ count/width/gap | column-rule/span | — |
| CSS Scroll Snap L1 | — | parse-only | — |
| CSS Masking L1 | — | parse-only | — |
| CSS Lists L3 | — | parse-only | rendering |
| CSS Containment L3 | — | size/layout/paint enforced | style/inline-size Phase 1 |
| CSS Container Queries L1 | — | parse+store (container-type, container-name) | @container matching |
| CSS Shapes L1 | — | parse+store (shape-outside/margin/threshold) | float shape offset |
| CSS Motion Path L1 | — | parse+store (offset-path/distance/rotate/anchor) | path layout |
