In progress: none (ready for next task)
Next step: lumen-a11y-full (8G, Phase 1, ADR-006) — stage 1 of 3: role computation + text alternatives + ARIA semantics

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

## P1 Roadmap: 40 prioritized tasks (Phase 0–3)

### Phase 0 ✅ (complete as of 2026-05-26)
01. ✅ HTML parser: full named entities, RAWTEXT/RCDATA, DOCTYPE/quirks, srcset/sizes, picture, preload scanner, push-tokenizer
02. ✅ CSS parser: structural/functional/UI-state pseudo, :is/:where, @media, @property, custom properties, !important
03. ✅ Layout block-flow + inline-flow + replaced + CSS cascade with specificity + stacking contexts
04. ✅ Paint display list + wgpu rasterizer + glyph atlas + text rendering
05. ✅ Shell integration: opens samples/page.html with backgrounds and text
06. ✅ Quirks-mode application: table UA-reset, line-height replaced, unitless length, hashless hex color
07. ✅ Typed Length/Color: ComputedStyle fully typed, values ready for P2/P3
08. ✅ Stacking contexts impl: 7-level CSS Painting Order (background → border → descendants → floats → inline → positioned)
09. ✅ Web Animations interpolation: AnimValue + AnimationInterpolator trait + NoopInterpolator stub
10. ✅ Push-tokenizer + incremental tree builder: feed_bytes with partial UTF-8 buffering, Document state between chunks
11. ✅ Picture/srcset/sizes finishing: parser + pickers ready, L4 nested media conditions
12. ✅ CSS Grid + full Flexbox: layout algorithms for modern responsive sites
13. ✅ CSS Positioned Layout: position: relative/absolute/fixed + inset properties
14. ✅ ICU4x segmenter + linebreak stubs: CJK typography foundation
15. ✅ Shadow DOM (P1 part): ShadowRootMode, NodeData::ShadowRoot, FlatTree, slot assignment
16. ✅ Forms runtime (P1 part): ValidityState + all HTML5 §4.10.21 flags, validation pseudo-classes
17. ✅ Contenteditable (P1 part): DOM mutations + Selection/Range API + beforeinput/input events
18. ✅ Accessibility tree (P1 part): build_ax_tree, compute_name, compute_description, implicit_role HTML-AAM
19. ✅ Print pipeline (P1 part): pagination algorithm (break-before/after/avoid, orphans/widows)
20. ✅ Preload scanner: scan_preload_hints for early fetch hints in background

### Phase 1 (in progress / planned)
21. ⬜ lumen-a11y-full (8G, [P1+P3]) — stage 1: ARIA role mapping (60+ variants), text alternative computation (accname §4), CSS pseudo-classes for accessibility
22. ⬜ lumen-a11y-full (8G, stage 2) — ARIA attribute application (aria-current/modal/roledescription/valuenow/controls/owns/flowto/details), relationship attributes, NodeId storage for find_by_id
23. ⬜ lumen-a11y-full (8G, stage 3) — full computed role mapping HTML-AAM, focus model, form control text alternatives, edge cases
24. ⬜ dom-arena-audit (10B Invariant 1, [P1+P3]) — audit lumen-dom: ensure no Rc<RefCell<Node>> in graph, all NodeId(u32)-based, add bincode::serialize/deserialize for snapshot, #[deny(clippy::rc_buffer)]
25. ⬜ layout-pure-audit (10D.1 Invariant 3, [P1+P2+P3]) — audit lumen-layout: no static MUT / lazy_static / OnceCell in hot path, layout algorithm purity for T3 hibernation
26. ⬜ paint-pure-audit (10D.2 Invariant 3, [P1+P2+P3]) — audit lumen-paint::display_list: pure function requirement for serialization
27. ⬜ antidetect-canvas-randomization (9D.1, [P1+P2]) — Brave-style canvas randomization: getImageData returns RGBA with per-session deterministic seed noise (P1 texture read-back side)
28. ⬜ antidetect-webgl-normalize (9D.2, [P1+P2]) — WebGL renderer/vendor strings normalization to generic "WebKit"/"Generic GPU" (P1 wgpu adapter info collection side)
29. ⬜ gpu-layer-lru (10F, [P1+P2+P3]) — LayerCache with LRU + GPU memory budget; texture pool recycling for off-viewport stacking contexts (P1 wgpu texture pool in lumen-paint side)
30. ⬜ glyph-atlas-eviction (10G, [P1+P2+P3]) — LRU eviction of rarely-used glyphs from atlas (P1 atlas data structure in lumen-font side)

### Phase 2 (planned)
31. ⬜ shadow-dom-accessibility-forms-gc (6+) — Extended Shadow DOM / Accessibility / Forms / Garbage collection integration
32. ⬜ contenteditable-advanced — drag-drop, paste, undo/redo coordination with shell
33. ⬜ accessibility-forms-validation — full constraint validation visualization, form submission with accessibility tree
34. ⬜ ime-input ([P1+P3]) — IME composition events through DOM, composition ranges, virtual keyboard interaction
35. ⬜ svg-layout-advanced — SVG transforms, viewport nesting, aspect-ratio preservation
36. ⬜ print-pdf-advanced — @page margin boxes, page numbers, headers/footers from margin-box content
37. ⬜ animation-keyframe-easing — full timing functions (ease/ease-in/ease-out/ease-in-out/cubic-bezier/steps)
38. ⬜ transition-advanced — grouped properties, interrupted transitions, animation-fill-mode complete
39. ⬜ font-loading-api ([P1+P3]) — @font-face lifecycle, FontFace interface, document.fonts collection
40. ⬜ performance-observer-timing — PerformanceObserver for layout/paint timings, resource timing

## Notes

- fts-omnibox (Wave 1, P3-domain) moved to STATUS-P3.md Queue — knowledge/omnibox is P3 domain, not P1
- Wave 2 (former P2/P3) queues removed — handled in respective STATUS-P2.md / STATUS-P3.md
- All Phase 0 items marked ✅ as complete per 2026-05-26 phase close
- Phase 1-2 items (21–40) represent next ~6 months of P1 work
- ADR-006 (automation), ADR-007 (anti-detection), ADR-008 (tab lifecycle) coordination embedded in tasks 21-30
- Coordination points with P2 (paint/render side) and P3 (shell/runtime integration) noted in brackets [Pn]

Recent: click-hint-overlay (7B.2, P1 complete 2026-05-28, P3 integration pending), print-pdf-pagination (Phase 1 complete, PaginationContext ready), bench-ram-axis (Phase 0 complete, cross-platform RSS baseline established), antidetect-webgl-normalize (P1 complete, GPU fingerprint normalization, P3 integration pending), gpu-layer-lru (Phase 1 complete, LayerCache impl, Phase 2 pending), antidetect-canvas-randomization (P1 complete, CanvasNoiseGenerator LCG RNG, P3 JS integration pending), dom-arena-audit (serde+bincode snapshot infrastructure, 7 tests, 121 total), layout-find-by-selector (selector_query module, 14 tests, P3 blocking clear), line-clamp-layout (CSS Overflow §3.2 multi-line truncation, 6 tests, graphic test 48)

## Recent (prior work history, last 30 days)

- 2026-05-28: click-hint-overlay (7B.2): <details>/<summary> support, 6 new tests
- 2026-05-28: print-pdf-pagination (5++): PaginationContext + Page + PageFragment, 7 unit tests
- 2026-05-28: bench-ram-axis (9G.5): cross-platform RSS measurement, baseline.json
- 2026-05-28: antidetect-webgl-normalize (9D.2): GpuFingerprint struct, 5 tests
- 2026-05-28: gpu-layer-lru (10F): LayerCache struct, LRU-tracked metadata, 7 tests
- 2026-05-28: antidetect-canvas-randomization (9D.1): CanvasNoiseGenerator, 20 tests
- 2026-05-27: extras-p2 complete: object-fit ✅ + variable fonts ✅ + Print PDF Phase 1 ✅
- 2026-05-27: glyph-atlas-eviction (10G.1): LRU tracking + get_lru_candidates, 4 new tests
- 2026-05-27: full HTML5 named entities WHATWG (2125): gen_entities.py + bin-search + 338 tests
- 2026-05-26: phase-0-close: html-parser, css-parser, layout all marked ✅
- 2026-05-25: html5-insertion-modes-remaining: 23/23 modes, AAA, 349 total tests
- 2026-05-25: svg-layout-basic: SvgRoot/SvgShape, ViewBox scale+offset, 12 tests, graphic test 47
- 2026-05-24: quirks-mode-detection: DOCTYPE + prefix rules, table UA-reset complete
- 2026-05-20: web-animations-interpolation: AnimValue + AnimationInterpolator trait, NoopInterpolator
- 2026-05-20: css-grid-flexbox-complete: block-flow + inline-flow + flex + grid + positioned layout
