# CSS Specifications & Property Roadmap

Canonical reference for CSS compliance work in Lumen. Tracks which W3C modules and properties are implemented, partial, or pending.

**Source of truth for specs:** https://www.w3.org/Style/CSS/specs.en.html  
**Implementation tracking:** P4 developer owns this file. Update on every property merge.

Legend: ✅ implemented · 🟡 parsed/stored, rendering deferred · ⬜ not started · 🚫 out of scope

---

## Quick stats (2026-07-02, recounted by table rows: `grep -c "^| .*<marker>"`)

| Status | Properties |
|--------|-----------|
| ✅ Fully implemented | ~237 |
| 🟡 Partial (parsed, not rendered) | ~135 |
| ⬜ Not started | ~114 |
| 🚫 Out of scope | ~20 (props in "Out of scope" modules) |

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
| CSS Color L4 | [css-color-4](https://www.w3.org/TR/css-color-4/) | 🟡 | oklch ✅; color-mix() ✅ (p4-color-mix-parsing 2026-06-08); system color keywords ✅ (p4-system-colors 2026-06-13); color() predefined spaces ✅ (srgb-linear/a98-rgb/prophoto-rgb/xyz/xyz-d65/xyz-d50, p4-color-function-spaces 2026-06-13); wide-gamut display output ⬜ |

### Tier 1 — Critical gaps (break most web pages when missing)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Custom Properties L1 | [css-variables](https://www.w3.org/TR/css-variables/) | ✅ | var() recursive + @property + env() | **#1** |
| CSS Transitions | [css3-transitions](https://www.w3.org/TR/css3-transitions/) | ✅ | TransitionScheduler wired: sync()+tick() in shell loop | **#2** |
| CSS Animations L1 | [css-animations-1](https://www.w3.org/TR/css-animations-1/) | ✅ | AnimationScheduler::tick() wired in shell RedrawRequested | **#3** |
| CSS Nesting | [css-nesting-1](https://www.w3.org/TR/css-nesting-1/) | ✅ | `&`-explicit + implicit `.foo{}`/`>.foo{}` nesting + nested `@media`/`@supports`/`@layer`/`@container`; 20 tests | **#4** |
| CSS Display L3 (table) | [css-display-3](https://www.w3.org/TR/css-display-3/) | ✅ | BoxKind::Table + BoxKind::TableRowGroup; global col-width pass; thead/tbody/tfoot; 6 tests 2026-05-24 | **#5** |
| CSS Positioning L3 (sticky) | [css3-positioning](https://www.w3.org/TR/css3-positioning/) | ✅ | BeginStickyLayer/EndStickyLayer in DL + sticky_offset_dy/dx in renderer; 5 display-list tests + graphic test 42 2026-05-24 | **#6** |
| CSS Positioning L3 (z-index) | [css3-positioning](https://www.w3.org/TR/css3-positioning/) | ✅ | StackingTree + PaintOrder + build_display_list_ordered wired in shell | **#7** |
| CSS 2.1 floats | [CSS2](https://www.w3.org/TR/CSS2/) | ✅ | FloatContext placement + FloatSide/ClearSide + 10 tests | **#8** |
| CSS Lists L3 | [css3-lists](https://www.w3.org/TR/css3-lists/) | ✅ | disc/circle/square geometric shapes + decimal/roman/alpha/greek text markers; 7 tests 2026-05-24 | **#9** |
| CSS Cascading L4/L5 | [css-cascade-4](https://www.w3.org/TR/css-cascade-4/) | ✅ | @layer cascade ordering: layer_priority in sort key, 6 tests | **#10** |
| Selectors L4 | [selectors4](https://www.w3.org/TR/selectors4/) | ✅ | :is()/:where()/:has() matching + all L4 pseudo-classes 2026-05-24 | **#11** |
| Media Queries L3 | [mediaqueries-3](https://www.w3.org/TR/mediaqueries-3/) | ✅ | width/height exact ✅; em/rem in features ✅; aspect-ratio ✅; re-eval on resize ✅; prefers-reduced-motion ✅; 11 tests; graphic test 44 2026-05-24 | **#12** |

### Tier 2 — High visual value (visually broken without these)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| Filter Effects L1 | [filter-effects](https://www.w3.org/TR/filter-effects/) | ✅ | backdrop-filter GPU compositing: PushBackdropFilter/PopBackdropFilter + 4 display-list tests + 6 layout tests + graphic test 30 2026-05-24 | **#13** |
| CSS Masking | [css-masking](https://www.w3.org/TR/css-masking/) | 🟡 | mask-image GPU compositing: PushMaskLayer/PopMaskLayer ✅ (alpha + luminance modes, REPLACE blend, scratch copy); PushMaskImage/PopMask ✅; gradient masks ✅ 2026-05-29 | **#14** |
| Compositing & Blending | [compositing](https://www.w3.org/TR/compositing/) | ✅ | mix-blend-mode blend pipeline ✅; background-blend-mode comma-list cycling ✅ 2026-05-27 | **#15** |
| CSS Pseudo-Elements L4 | [css-pseudo-4](https://www.w3.org/TR/css-pseudo-4/) | 🟡 | ::first-line/::first-letter split; ::marker; ::selection | **#16** |
| CSS Images L3 | [css3-images](https://www.w3.org/TR/css3-images/) | ✅ | conic-gradient() ✅ 2026-05-24; multiple bg layers ✅ 2026-05-26 | **#17** |
| CSS Images L4 | [css4-images](https://www.w3.org/TR/css4-images/) | 🟡 | image-set() ✅ 2026-06-02; cross-fade() ✅ 2026-06-02; gradient `<color-interpolation-method>` (`in <space>`) ✅ 2026-06-14 (p4-gradient-interpolation: srgb/srgb-linear/oklab/lab/hsl/hwb/xyz via dense-stop polyfill; polar oklch/lch blocked by BUG-154) | **#18** |
| CSS Grid L1 | [css-grid-1](https://www.w3.org/TR/css-grid-1/) | 🟡 | grid-template-areas ✅ 2026-05-22; dense auto-flow ✅ 2026-05-24 | **#19** |
| CSS Fonts L4 | [css-fonts-4](https://www.w3.org/TR/css-fonts-4/) | 🟡 | @font-face actual loading ⬜; font-optical-sizing ✅ 2026-05-29 | **#20** |
| CSS Intrinsic Sizing L3 | [css3-sizing](https://www.w3.org/TR/css3-sizing/) | ✅ | min-content/max-content/fit-content/fit-content(L) for width/height/min-max; 11 tests 2026-05-24 | **#21** |
| CSS Overflow L3 (scroll) | [css-overflow-3](https://www.w3.org/TR/css-overflow-3/) | 🟡 | scrollable containers; overflow:scroll rendering | **#22** |
| CSS Text L3/L4 | [css3-text](https://www.w3.org/TR/css3-text/) | 🟡 | text-align-last ✅ 2026-06-08; hyphens:auto ⬜ | **#23** |
| CSS Transforms L2 | [css-transforms-2](https://www.w3.org/TR/css-transforms-2/) | 🟡 | individual translate/rotate/scale ✅ 2026-05-26; 3D matrix primitive + perspective-correct rendering ✅ 2026-05-29 (P2); 3D function parsing/`perspective` wiring ⬜ (P4) | **#24** |
| CSS Values L4/L5 | [css-values-4](https://www.w3.org/TR/css-values-4/) | 🟡 | env(); attr() with type; cq* units | **#25** |

### Tier 3 — Spec compliance (affect specific use-cases)

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Scroll Snap L1 | [css-scroll-snap-1](https://www.w3.org/TR/css-scroll-snap-1/) | ✅ | scroll-snap-type (y/x/both mandatory+proximity), scroll-snap-align (start/end/center), scroll-snap-stop (always); shell integration: collect_snap_containers + find_snap_target wired to start_smooth_scroll/scroll_x_by with viewport snap-port 2026-06-03 | **#26** |
| CSS Multi-column L1 | [css3-multicol](https://www.w3.org/TR/css3-multicol/) | 🟡 | column-rule rendering; column-span; column-fill | **#27** |
| CSS Containment L2/L3 | [css-contain-2](https://www.w3.org/TR/css-contain-2/) | 🟡 | content-visibility skip-content; cq* units | **#28** |
| CSS Counter Styles L3 | [css-counter-styles-3](https://www.w3.org/TR/css-counter-styles-3/) | 🟡 | counter-reset/increment resolution ✅ 2026-05-25; @counter-style ⬜ | **#29** |
| CSS Box Alignment L3 | [css3-align](https://www.w3.org/TR/css3-align/) | 🟡 | justify-items/justify-self for grid | **#30** |
| CSS Inline L3 | [css-inline-3](https://www.w3.org/TR/css-inline-3/) | 🟡 | line-height leading; baseline grid; `baseline-shift` ✅ 2026-06-21 (p4-baseline-shift: SVG 1.1 §10.9.2 / CSS Inline L3 §5.2 — non-inherited `SvgBaselineShift` enum Baseline/Sub/Super/Length/Percentage; presentational attribute + CSS property; CSS overrides attr; wired through `emit_svg_text` as vertical y-shift; `sub` lowers by 0.2×font-size, `super` raises by 0.4×font-size, positive length raises) | **#31** |
| CSS Text Decoration L4 | [css-text-decor-4](https://www.w3.org/TR/css-text-decor-4/) | 🟡 | text-emphasis rendering; text-underline-offset ✅ 2026-06-10 | **#32** |
| CSS Scrollbars L1 | [css-scrollbars-1](https://www.w3.org/TR/css-scrollbars-1/) | 🟡 | scrollbar-width/color rendering | **#33** |
| CSS Basic UI L3/L4 | [css3-ui](https://www.w3.org/TR/css3-ui/) | 🟡 | resize drag-UI; appearance form widgets; field-sizing ✅ 2026-06-13 | **#34** |
| Media Queries L4/L5 | [mediaqueries-4](https://www.w3.org/TR/mediaqueries-4/) | 🟡 | prefers-reduced-motion ✅; hover/any-hover/pointer/any-pointer ✅ 2026-06-14 (p4-media-hover-pointer: desktop defaults hover/fine); prefers-contrast/prefers-reduced-data ✅ 2026-06-16 (p4-media-contrast-data); prefers-reduced-transparency ✅ 2026-06-19 (p4-prefers-reduced-transparency); scripting ✅ 2026-06-19 (p4-media-scripting: `MediaScripting` none/initial-only/enabled, desktop default `enabled` — Lumen ships QuickJS, matches Edge); inverted-colors ✅ 2026-06-20 (p4-media-inverted-colors: `MediaInvertedColors` none/inverted, desktop default `none`, matches Edge) | **#35** |
| CSS Conditional L4 | [css-conditional-4](https://www.w3.org/TR/css-conditional-4/) | ✅ | @supports `selector()` ✅ 2026-06-17 (p4-supports-selector: `ComplexSelector::is_supported` recurses through `:is()`/`:not()`/`:where()`/`:has()`/`:nth-child(… of …)`/`:host()`/`::slotted()`, false on any `Unsupported`/`Unknown`); `font-tech()`/`font-format()` ✅ 2026-06-19 (p4-supports-font-tech: `SupportsCondition::FontTech`/`FontFormat` evaluated against lumen-font capabilities — features-opentype/variations + truetype/opentype/woff/woff2 supported, colour glyphs/palettes/AAT/Graphite/collection/EOT/SVG rejected) | **#36** |
| CSS Color Adjust L1 | [css-color-adjust-1](https://www.w3.org/TR/css-color-adjust-1/) | 🟡 | color-scheme UA switching | **#37** |
| CSS Box Sizing L4 | [css-sizing-4](https://www.w3.org/TR/css-sizing-4/) | ✅ | contain-intrinsic-size ✅ 2026-06-14 (p4-contain-intrinsic-size: longhands + logical aliases + shorthand; size-containment wiring for block/flex/grid height + inline-block width); interpolate-size ✅ | **#38** |
| CSS Overflow L4 | [css-overflow-4](https://www.w3.org/TR/css-overflow-4/) | ✅ | line-clamp multi-line truncation (layout algorithm done; -webkit-line-clamp/line-clamp, ellipsis, N-line truncation) | **#39** |
| CSS Easing L1 | [css-easing-1](https://www.w3.org/TR/css-easing-1/) | 🟡 | cubic-bezier/steps interpolation wiring | **#40** |

### Tier 4 — Advanced / future

| Module | Spec | Status | Missing piece | Priority |
|--------|------|--------|--------------|---------|
| CSS Writing Modes L4 | [css-writing-modes-4](https://www.w3.org/TR/css-writing-modes-4/) | 🟡 | vertical-rl/lr layout axis swap | **#41** |
| CSS Grid L2 | [css-grid-2](https://www.w3.org/TR/css-grid-2/) | 🟡 | subgrid layout algorithm ✅ 2026-06-03 (`subgrid.rs`, `GridTrackSize::Subgrid`, thread-local track inheritance); masonry ✅ 2026-06-10 (`masonry.rs`, `GridTrackSize::Masonry`, greedy waterfall algorithm) | **#42** |
| CSS Shapes L1 | [css-shapes-1](https://www.w3.org/TR/css-shapes-1/) | 🟡 | circle() ✅ 2026-06-03; polygon/ellipse ✅ (`shape_polygons`/`shape_ellipses`); inset() ✅ 2026-06-10 (`shape_insets`, `parse_shape_inset_px`, rounded corners); `clip-path: path()` ✅ 2026-06-14 (p4-clip-path-path); `path()`/`polygon()` `<fill-rule>` evenodd/nonzero ✅ 2026-06-14 (p4-clip-path-fill-rule); `shape-outside: path()` ✅ 2026-06-14 (p4-shape-outside-path: `parse_shape_path_px` flattens SVG path → wrapping polygon) | **#43** |
| Motion Path L1 | [motion-1](https://www.w3.org/TR/motion-1/) | 🟡 | `offset-path: path()` ✅ 2026-06-10 (P4: ComputedStyle fields + resolve_motion_transform wiring in property_trees); `offset-distance`/`offset-rotate` ✅; `ray(<angle>)` ✅ 2026-06-13 (p4-offset-ray: deg/grad/rad/turn, size/contain/at parsed-and-ignored for px distance); `offset-anchor` ⬜ Phase 3; `url()` paths ⬜ | **#44** |
| CSS Fragmentation L3 | [css3-break](https://www.w3.org/TR/css3-break/) | ✅ | break-before/after/inside + orphans/widows in `ComputedStyle`; `pagination.rs` applies rules | **#45** |
| CSS Color L5 | [css-color-5](https://www.w3.org/TR/css-color-5/) | ✅ | color-mix() ✅ (p4-color-mix-parsing 2026-06-08); relative color syntax ✅ (p4-relative-color 2026-06-13) | **#46** |
| CSS Fonts L5 | [css-fonts-5](https://www.w3.org/TR/css-fonts-5/) | ⬜ | font-palette; @font-palette-values | **#47** |
| CSS Easing L2 | [css-easing-2](https://www.w3.org/TR/css-easing-2/) | ✅ | linear() easing TimingFunction::LinearStops 2026-05-24 | **#48** |
| CSS Overscroll L1 | [css-overscroll-1](https://www.w3.org/TR/css-overscroll-1/) | 🟡 | gesture boundary handling | **#49** |
| CSS Gap Decorations L1 | [css-gaps-1](https://www.w3.org/TR/css-gaps-1/) | ✅ | `gap-rule-width/style/color` shorthand+longhands; `collect_gap_segments()` in display_list.rs; flex + grid containers wired (p4-gap-rule, 2026-06-10) | **#50** |
| CSS Env Variables L1 | [css-env-1](https://www.w3.org/TR/css-env-1/) | ✅ | `env()` + fallback + nested `calc(env(...)+...)` implemented in `style.rs:8798` (`expand_env_vars`); `safe-area-inset-*` returns fallback when not set | **#51** |
| CSS Selectors L5 | [selectors-5](https://www.w3.org/TR/selectors-5/) | ✅ | `:nth-child(An+B of S)` selector filter implemented in `style.rs:6464` + `css-parser` parser; 4 layout tests | **#52** |
| CSS Nesting (scope) | [css-scoping-1](https://www.w3.org/TR/css-scoping-1/) | 🟡 | @scope root matching ✅ (P1 2026-06-03); limit/inner-scope — Phase 2 | **#53** |
| CSS Functions & Mixins | [css-mixins-1](https://www.w3.org/TR/css-mixins-1/) | ⬜ | @function rule | **#54** |
| Scroll-driven Animations | [scroll-animations-1](https://www.w3.org/TR/scroll-animations-1/) | ✅ | scroll-timeline-name/axis, view-timeline-name/axis, animation-timeline (auto/scroll()/view()/named); collect_named_* walks layout tree; P4 2026-06-10 | **#55** |
| CSS Anchor Positioning | [css-anchor-position-1](https://www.w3.org/TR/css-anchor-position-1/) | 🟡 | algorithm stub ready (P1 2026-06-03): AnchorRegistry, collect_anchors, resolve_anchor_function, resolve_inset_area; CSS wiring pending (P4) | **#56** |
| CSS View Transitions L1 | [css-view-transitions-1](https://www.w3.org/TR/css-view-transitions-1/) | 🟡 | `document.startViewTransition` JS API + 300 ms cross-fade ✅ 2026-06-03; `view-transition-name` ✅ P4 2026-06-10 (ComputedStyle field + parsing + collect_view_transition_names); `::view-transition-*` pseudos ⬜ Phase 3 | **#57** |
| CSS Fill & Stroke L3 | [fill-stroke-3](https://www.w3.org/TR/fill-stroke-3/) | 🟡 | fill/stroke/fill-opacity/stroke-opacity/stroke-width ✅ 2026-05-27; fill-rule/stroke-linecap/linejoin/miterlimit/dasharray/dashoffset ✅; paint-order ✅ 2026-06-14 (p4-paint-order: `SvgPaintOrder` inherited field + `emit_svg_shape` fill/stroke reorder); `text-anchor`/`dominant-baseline` as CSS properties ✅ 2026-06-21 (p4-svg-text-anchor: inherited `Option` fields folded through `apply_svg_presentational_hints` so author CSS overrides the presentation attribute and inherits from `<g>`) | **#58** |
| CSS Scroll Snap L2 | [css-scroll-snap-2](https://www.w3.org/TR/css-scroll-snap-2/) | 🟡 | snapchanging/snapchanged events: SnapChangeEvent (snapTargetBlock/Inline) + лэйаут-резолв снапнутых узлов (find_snapped_nodes/SnapTargets) + QuickJsRuntime::fire_snap_changing/changed; shell-диспатч при scroll-snap завершении — Phase 1 2026-06-10 | **#59** |

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
| `revert-layer` | ✅ | CSS Cascade L5 §6.4.6; pre-pass in compute_style drops winning layer; 5 tests; test 98 (P4 2026-06-13) |

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
| `float` | ✅ | left/right/none — FloatContext placement; shrink-to-fit width |
| `clear` | ✅ | left/right/both — FloatContext.clear_y() clearance |
| `-webkit-line-clamp` / `line-clamp` | ✅ | parsed + layout algorithm: truncate lines, ellipsis, priority over text-overflow |
| `contain-intrinsic-size` | 🟡 | parsed; intrinsic size hint ⬜ |

### [T0] Borders & Outlines

| Property | Status | Notes |
|----------|--------|-------|
| `border` / `border-*` (shorthand) | ✅ | |
| `border-*-width` | ✅ | f32 px |
| `border-*-style` | ✅ | solid/dashed/dotted/double |
| `border-*-color` | ✅ | CssColor; currentColor |
| `border-radius` / `border-*-*-radius` | ✅ | circular SDF rendering ✅; elliptical (rx≠ry syntax `10px / 20px`) ✅ FemtovgBackend |
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
| `accent-color` | ✅ | parsed + wired to form controls (checkbox/radio/range/progress) in display_list.rs (P4 2026-06-14); 5 tests + graphic 110 |
| `color-mix()` | ✅ | parse_color_mix() in style.rs (P4 2026-06-08); 3 tests |
| `color()` predefined spaces | ✅ | srgb/display-p3/rec2020 + srgb-linear/a98-rgb/prophoto-rgb/xyz/xyz-d65/xyz-d50 (P4 2026-06-13); non-displayable gamut-mapped to sRGB; 11 tests; test 96 |

### [T0] Fonts

| Property | Status | Notes |
|----------|--------|-------|
| `font` / `font-size` / `font-weight` / `font-style` / `font-family` | ✅ | |
| `font-variant` / `font-variant-caps` | 🟡 | small-caps only; all-small-caps ⬜ |
| `font-stretch` | 🟡 | % parsed; matcher ⬜ |
| `font-variation-settings` | ✅ | fvar+avar normalization; applied on CPU/wgpu paths, femtovg window renders default instance (see CAPABILITIES) |
| `font-feature-settings` | ✅ | parse + ComputedStyle (inherited) + DrawText.font_features; shaper overrides default GSUB/GPOS set (liga/clig/calt/rlig/ccmp + kern) on CPU path & femtovg varied-text path; native femtovg text shapes itself (class BUG-109) |
| `font-size-adjust` | ✅ | real OS/2 x-height scaling (P4 2026-06-13); тест 95 |
| `font-optical-sizing` | ✅ | auto injects opsz=font-size into variation axes; none skips |
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
| `text-align-last` | ✅ | parsed + wired in align_lines; last-line override (CSS Text L3 §7.2); 4 tests |
| `hyphens` | 🟡 | none/manual ✅; auto (HyphenationProvider) ⬜ |
| `tab-size` | ✅ | parsed; \t expanded in pre/pre-wrap; renderer advances cursor by tab_size |
| `line-break` | 🟡 | parsed; CJK-aware breaking ⬜ |
| `text-wrap-mode` / `text-wrap-style` | 🟡 | parsed; integration ⬜ |
| `text-underline-position` / `text-underline-offset` | ✅ | wired in push_text_decoration(); Under→fs*0.25; offset adds to base (p4-text-underline 2026-06-10) |
| `text-emphasis` / `text-emphasis-*` | ✅ | per-char marks rendered (emit_text_emphasis_marks) |

### [T0] Selectors

| Selector | Status | Notes |
|----------|--------|-------|
| `*`, `E`, `.class`, `#id`, `[attr*]` | ✅ | all attribute operators |
| `A B`, `A > B`, `A + B`, `A ~ B` | ✅ | all combinators |
| `:root`, `:first/last-child`, `:nth-*`, `:only-*`, `:empty` | ✅ | |
| `:not(S)` | ✅ | L3 simple; L4 any selector |
| `:hover`, `:active` | ✅ | shell hit-test wiring 2026-06-03; ancestor propagation per spec |
| `:focus`, `:focus-within` | ✅ | shell click-focus wiring 2026-06-03 |
| `:focus-visible` | ✅ | Phase 0: synonym for `:focus` 2026-06-03 |
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
| `translate` / `rotate` / `scale` | ✅ | individual props (Transforms L2); compose before `transform` ✅ 2026-05-26 |

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
| `var()` substitution | ✅ | recursive + fallback + calc() + env() + cycle guard |
| `@property` | ✅ | syntax/inherits/initial-value; inherits:false blocks cascade |

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
| `animation-timeline` / `animation-range` | ✅ | animation-timeline parsed (Auto/Scroll/View/Named); P4 2026-06-10 |
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
| `border-collapse` | ✅ | ComputedStyle.border_collapse wired; collapse zeroes spacing; 5 unit-тестов + graphic test 80 (P4 2026-06-10) |
| `border-spacing` | ✅ | border_spacing_h/v in ComputedStyle; zero when collapse mode |
| `empty-cells` | ✅ | ComputedStyle.empty_cells (inherited); `hide` suppresses border+bg of empty cells in separate mode; wired in emit_table_cell; 6 unit + 5 paint tests + graphic test 115 (P4 2026-06-14) |
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
| `float` | ✅ | left/right/none; FloatContext axis-aligned placement + shrink-to-fit |
| `clear` | ✅ | left/right/both; FloatContext.clear_y() |
| `shape-outside` | 🟡 | parsed; float shape wrapping ⬜ |

### [T1] Lists

| Property | Status | Notes |
|----------|--------|-------|
| `list-style` / `list-style-type` | 🟡 | disc/circle/square/decimal/roman parsed; marker render ⬜ |
| `list-style-position` | 🟡 | inside/outside; positioning ⬜ |
| `list-style-image` | ✅ | url() parsed; image marker rendered (DrawImage replaces bullet, CSS Lists L3 §2.3) |
| `counter-reset` / `counter-increment` | 🟡 | Vec<(name,val)>; resolution ⬜ |
| `counter-set` | ✅ | CSS Lists L3 §4; Vec<(name,val)>; apply_set после reset/increment; тест 97 2026-06-13 |
| `@counter-style` | ⬜ | |

### [T1] @layer / Cascade Layers

| Feature | Status | Notes |
|---------|--------|-------|
| `@layer` declaration | ✅ | parsed; cascade ordering wired: layer_priority sort key in compute_style |
| `@import layer()` | 🟡 | URL parsed; layer() modifier ⬜ |
| `revert-layer` | ✅ | CSS Cascade L5 §6.4.6; reverts current cascade layer (P4 2026-06-13) |

### [T1] Selectors L4

| Selector | Status | Notes |
|----------|--------|-------|
| `:is(S)` | 🟡 | parsed; full matching ⬜ |
| `:where(S)` | 🟡 | parsed; zero-specificity ⬜ |
| `:has(S)` | 🟡 | parsed; relational matching ⬜ |

### [T1] Media Queries

| Feature | Status | Notes |
|---------|--------|-------|
| `@media` | ✅ | width/height exact ✅; min/max ✅; em/rem units ✅; orientation ✅; aspect-ratio ✅; re-eval on resize ✅ |
| `prefers-color-scheme` | ✅ | |
| `prefers-reduced-motion` | ✅ | parsed + matched; OS integration deferred (always `no-preference` until shell wires OS pref) |
| `hover`, `pointer` | ✅ | Media Queries L4 §5.3-5.6; `hover`/`any-hover` (none/hover) + `pointer`/`any-pointer` (none/coarse/fine); desktop defaults hover/fine in `MediaContext`; 8 tests + graphic 118 (P4 2026-06-14) |
| `prefers-contrast` / `prefers-reduced-data` | ✅ | Media Queries L5 §5.5-5.6; `prefers-contrast` (no-preference/more/less/custom) + `prefers-reduced-data` (no-preference/reduce); desktop defaults no-preference in `MediaContext`; OS/UA integration deferred; 6 tests + graphic 120 (P4 2026-06-16) |
| `prefers-reduced-transparency` | ✅ | Media Queries L5 §5.7; no-preference/reduce; desktop default no-preference in `MediaContext`; OS/UA integration deferred; 3 tests + graphic 124 (P4 2026-06-19) |

---

### [T2] Filters

| Property | Status | Notes |
|----------|--------|-------|
| `filter` | ✅ | GPU pipeline: blur/brightness/contrast/grayscale/hue-rotate/invert/saturate/sepia/drop-shadow |
| `backdrop-filter` | 🟡 | parsed; backdrop GPU compositing ⬜ |

### [T2] Clipping & Masking

| Property | Status | Notes |
|----------|--------|-------|
| `clip-path` | ✅ | inset/circle/ellipse/polygon/path() rendered; `<fill-rule>` (nonzero/evenodd) in path()/polygon() ✅ 2026-06-14 |
| `clip-rule` | ⬜ | evenodd/nonzero (SVG-only; CSS clip-path uses path()/polygon() fill-rule ✅ 2026-06-14) |
| `mask` (shorthand) | 🟡 | |
| `mask-image` | 🟡 | GPU mask composite pipeline ✅ (PushMask/PopMask + PushMaskLayer/PopMaskLayer); alpha compositing ✅; luminance mode ✅ 2026-05-29 |
| `mask-repeat` / `mask-size` / `mask-position` | 🟡 | parsed; `mask-position` wired into `PushMaskImage` (initial `center`, CSS Masking L1 §4.4) 2026-06-22; `mask-repeat` tiling ⬜ |
| `mask-mode` | ✅ | `alpha` / `luminance` / `match-source` (CSS Masking L1 §6.4); gradient masks bake `luminance(rgb)·alpha` into stop alpha (BUG-218, 2026-06-19) |
| `mask-origin` | 🟡 | wired: sets the mask positioning area (border/padding/content box) via `background_origin_rect`, initial `border-box` (§4.5) 2026-06-22 |
| `mask-clip` / `mask-composite` | 🟡 | parsed only; `mask-clip` painting-area clip ⬜ (needs clip rect through PushMask*/PopMask + backend scissor); `mask-composite` multi-layer ⬜ |

### [T2] Compositing

| Property | Status | Notes |
|----------|--------|-------|
| `mix-blend-mode` | ✅ | 17 modes; GPU blend pipeline; stacking context isolation 2026-05-27 |
| `background-blend-mode` | ✅ | 17 modes; comma-list cycling over bg layers; PushBlendMode/PopBlendMode per layer 2026-05-27 |
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
| `background-image` | 🟡 | url() ✅; linear/radial/repeating gradient GPU ✅; conic-gradient ✅ |
| `background-repeat` / `background-position` / `background-size` | ✅ | |
| `background-attachment` | 🟡 | parsed; scroll/fixed ⬜ |
| `background-origin` / `background-clip` | 🟡 | parsed; text clip ⬜ |
| `image-rendering` | ✅ | bilinear/nearest sampler |
| `object-fit` / `object-position` | ✅ | |
| `image-set()` | ⬜ | CSS Images L4 |
| `conic-gradient()` | ✅ | ParsedGradient::Conic + DrawConicGradient + GPU shader 2026-05-24 |
| gradient `in <space>` (color-interpolation-method) | 🟡 | rectangular + hsl/hwb ✅ 2026-06-14 (dense-stop polyfill via color-mix); polar oklch/lch ⬜ BUG-154 |
| `cross-fade()` | ⬜ | CSS Images L4 |

### [T2] CSS Grid

| Property | Status | Notes |
|----------|--------|-------|
| `grid-template-columns` / `grid-template-rows` | 🟡 | px/fr/auto/repeat()/minmax() ✅ |
| `grid-template-areas` | ✅ | parsed + named area placement in lay_out_grid; GridLine::Named resolved |
| `grid-template` / `grid` (super-shorthand) | 🟡 | |
| `grid-auto-columns` / `grid-auto-rows` | 🟡 | |
| `grid-auto-flow` | ✅ | row/column/dense/column dense ✅ 2026-05-24 |
| `grid-column*` / `grid-row*` / `grid-area` | 🟡 | auto/int/span |
| `subgrid` | 🟡 | CSS Grid L2; layout algorithm ✅ 2026-06-03; CSS parsing ✅ (subgrid keyword) |
| `masonry` | 🟡 | CSS Grid L3; layout algorithm ✅ 2026-06-10 (`masonry.rs`, greedy waterfall); CSS: masonry-auto-flow P4 |

### [T2] Intrinsic Sizing

| Value | Status | Notes |
|-------|--------|-------|
| `min-content` | ✅ | Length::MinContent; phase-0 approx = longest-word width 2026-05-24 |
| `max-content` | ✅ | Length::MaxContent; max_content_outer_width() measures text 2026-05-24 |
| `fit-content` / `fit-content(L)` | ✅ | Length::FitContent(Option<Box<Length>>); capped at available 2026-05-24 |
| `stretch` / `available` | 🟡 | parsed as FitContent(None) |

### [T2] Transforms L2 / 3D

| Property | Status | Notes |
|----------|--------|-------|
| `perspective` / `perspective-origin` | 🟡 | parsed; 3D projection ⬜ |
| `transform-style: preserve-3d` | 🟡 | parsed; 3D context ⬜ |
| `backface-visibility` | 🟡 | parsed; 3D flip ⬜ |
| `translate` / `rotate` / `scale` (individual) | ✅ | CSS Transforms L2; compose before `transform` 2026-05-26 |

### [T2] Values (advanced)

| Value | Status | Notes |
|-------|--------|-------|
| `env()` | ⬜ | safe-area-inset-*, titlebar-area-* |
| `attr()` with type | 🟡 | string only; type casting ⬜ |
| `cqw` / `cqh` / `cqi` / `cqb` / `cqmin` / `cqmax` | ✅ | container query units; thread-local CONTAINER_CQ; 4 tests 2026-05-25 |
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
| `scroll-timeline` / `view-timeline` | ✅ | scroll-timeline-name/axis, view-timeline-name/axis shorthands+longhands; collect_named_* wired; P4 2026-06-10 |

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
| `@container` | ✅ | condition matching ✅; 2nd-pass re-layout ✅; cq* units ✅ 2026-05-25 |
| Container query units (`cq*`) | ✅ | cqw/cqh/cqi/cqb/cqmin/cqmax 2026-05-25 |
| Style queries `style(--prop[: value])` | 🟡 | Phase 0 2026-07-02: single declaration, literal compare, custom props only; `state()` ⬜ |

### [T3] Counters & Lists (rendering)

| Property | Status | Notes |
|----------|--------|-------|
| `counter-reset` / `counter-increment` | ✅ | precompute_counters() pre-order DOM walk 2026-05-25 |
| `counter()` / `counters()` in `content` | ✅ | resolved in content_to_inline_segments 2026-05-25 |
| `@counter-style` | ⬜ | custom counter symbols |

### [T3] Content & Pseudo-element content

| Property | Status | Notes |
|----------|--------|-------|
| `content` | 🟡 | string ✅; attr() ✅ 2026-05-25; counter()/counters() ✅ 2026-05-25; open-quote/close-quote ✅ 2026-06-14; url() ⬜ |
| `quotes` | ✅ | CSS Generated Content L3 §3.2; auto/none/[<string> <string>]+; nesting depth tracked in document order via counters pre-pass; тест 117 2026-06-14 |

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
| `line-height-step` | ✅ | CSS Rhythmic Sizing L1 §2 (p4-line-height-step 2026-06-19): inherited `line_height_step` px field; line boxes rounded up to nearest multiple in box_tree + paint; тест 122 |
| `initial-letter` | 🟡 | CSS Inline L3 §5 (ph3-initialletter 2026-06-29): `normal \| <number> <integer>?` parsed → non-inherited `initial_letter_size`/`initial_letter_sink`; Phase 0 layout promotes the first-letter unit to an inline-start float drop cap spanning `size × line-height`, reserving `sink` (default `floor(size)`) text lines beside it; works on the element or via `::first-letter`. Deferred: precise cap-height/baseline alignment, raised-cap above first line (sink<size clipped), `initial-letter-align`, RTL inline-start. |

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
| `appearance` | ✅ | none/auto/compat; `appearance:none` strips UA box + suppresses native indicator (p4-appearance-none 2026-06-14) |
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
| `@container` | ✅ | condition matching ✅; 2nd-pass re-layout ✅; cq* units ✅ 2026-05-25 |
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
| `cqw`/`cqh`/`cqi`/`cqb`/`cqmin`/`cqmax` | ✅ | container query units 2026-05-25 |
| `env()` | ⬜ | |
| `attr()` | 🟡 | string ✅ 2026-05-25 in content; type casting ⬜ |
| `color-mix()` | ✅ | CSS Color L5; parse_color_mix() 2026-06-08 |
| `counter()`/`counters()` | ✅ | in content; resolution 2026-05-25 |
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
| `offset` / `offset-path` / `offset-distance` / `offset-rotate` / `offset-anchor` | 🟡 | parsed; motion layout algorithm stub ready (P1 2026-06-02); CSS wiring pending (P4) |

### [T4] Containment (advanced)

| Property | Status | Notes |
|----------|--------|-------|
| `contain` | 🟡 | size/layout/paint enforcement ✅; content-visibility skip-content ⬜ |
| `content-visibility` | 🟡 | hidden ✅ (P1 2026-06-03); auto ✅ below-viewport skip + shell ratchet/relayout (P1 BB-4 2026-06-13); above-viewport skip + contain-intrinsic-size ⬜ |

### [T4] Scroll-driven Animations

| Property | Status | Notes |
|----------|--------|-------|
| `scroll-timeline` / `view-timeline` | ✅ | CSS wiring done: P4 2026-06-10 |
| `animation-timeline` / `animation-range` | ✅ | animation-timeline parsed; P4 2026-06-10 |

### [T4] Anchor Positioning

| Property | Status | Notes |
|----------|--------|-------|
| `anchor-name` / `position-anchor` / `inset-area` | ✅ | ComputedStyle + collect_anchors + apply_anchor_positions post-layout pass; position-area alias |
| `anchor()` / `anchor-size()` functions | ⬜ | |

### [T4] Color L5

| Feature | Status | Notes |
|---------|--------|-------|
| `color-mix()` | ✅ | parse_color_mix() 2026-06-08 |
| `color-contrast()` | ⬜ | |
| Relative color syntax `oklch(from ...)` | ⬜ | |
| `@color-profile` | ⬜ | |

---

## P4 Work Queue

Ordered list of 🟡→✅ promotions for the P4 developer. One item = one feature branch.

| # | Property / Feature | Effort | Blocker |
|---|-------------------|--------|---------|
| 1 | `var()` full recursive substitution | ✅ | expand_vars() recursive + @property + env() + 40 unit tests + graphic test 50; 2026-05-29 |
| 2 | `transition` interpolation (per-frame lerp) | M | easing functions |
| 3 | `@keyframes` AnimationScheduler::tick wiring | L | transitions done |
| 4 | CSS Nesting — nested rule parser | L | none |
| 5 | `position: sticky` layout + scroll listener | M | none |
| 6 | `z-index` stacking context paint ordering | ✅ | StackingTree+PaintOrder wired in shell; build_display_list_ordered_with_anim 2026-05-23 |
| 7 | `float` + `clear` layout algorithm | ✅ | FloatContext + FloatSide/ClearSide + 10 tests 2026-05-22 |
| 8 | `list-style-type` marker rendering | S | none |
| 9 | `@layer` cascade ordering | ✅ | done 2026-05-22 |
| 10 | `:is()` / `:where()` / `:has()` matching | M | none |
| 11 | `@media` resize hook re-evaluation | S | shell event |
| 12 | `filter` GPU offscreen pass | L | wgpu pipeline |
| 13 | `clip-path` basic shapes (inset/circle/ellipse/polygon) | M | none |
| 14 | `mix-blend-mode` + `background-blend-mode` | ✅ | 17 GPU blend modes + comma-list cycling 2026-05-27 |
| 15 | `::first-letter` / `::first-line` line split | M | inline layout |
| 16 | `::marker` rendering | S | float/list |
| 17 | `conic-gradient()` | ✅ | ParsedGradient::Conic + DrawConicGradient + WGSL kind=2 + 9 tests + graphic test 40 2026-05-24 |
| 18 | Multiple backgrounds | ✅ | BackgroundLayer struct + Vec<BackgroundLayer> in ComputedStyle + parse_single_bg_layer + cycling shorthand + 6 tests + graphic test 45 2026-05-26 |
| 19 | `grid-template-areas` named placement | ✅ | GridLine::Named + find_named_area + resolve_named_lines 2026-05-22 |
| 20 | `@font-face` actual file loading | L | network/P3 |
| 21 | `min-content` / `max-content` / `fit-content` | L | layout engine |
| 22 | `overflow: scroll` scrollable containers | L | shell scroll |
| 23 | `border-radius` elliptical (rx≠ry) | ✅ | border_{corner}_radius_y + RRectVertex radii_x/y + WGSL sdf_rrect elliptical SDF + 12 tests + graphic test 36 2026-05-24 |
| 24 | `column-rule` rendering | S | paint |
| 25 | `line-height` leading in line box | ✅ | half_leading=(line_h-em)/2 в apply_inline_vertical_align + ascent_px() в TextMeasurer + 4 тесты 2026-05-24 |
| 26 | Scroll snap shell integration | M | scroll event |
| 27 | `@container` 2nd-pass execution | L | container-type done |
| 28 | `backdrop-filter` GPU compositing pass | L | wgpu pipeline |
| 29 | `writing-mode: vertical-*` axis swap | L | layout engine |
| 30 | `subgrid` track inheritance | XL | grid engine |
| 48 | `linear()` easing function | ✅ | TimingFunction::LinearStops + parse_linear_easing_stops + linear_stops_progress 2026-05-24 |
