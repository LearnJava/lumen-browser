In progress: none
Next task: check Phase 2 queue for remaining layout/DOM work; most Phase 1-2 items await P3 JS integration or Phase 3+ work

Recent merge: svg-layout-advanced Phase 3 ✅ — nested SVG child propagation (SvgRoot now appears in child-bearing boxes), 5 unit tests PASS, 2026-05-30

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Note: fts-omnibox (Wave 1, P3-задача по домену) перенесена в STATUS-P3.md Queue (это не P1-домен, это knowledge/omnibox).
Note: Wave 2 очередь содержит P2-задачи (extras-p2, avif-decoder, webgl-context, font-hinting, svg-rasterizer) — они пока не для P1. Wave 2 P3-задачи (http2-client, preconnect-hints) тоже в P3 очереди. P1 берёт следующее из Phase 1/Phase 2 своих задач.

Recent:
- performance-observer-timing (6.9, Phase 1): PerformanceEntry/PerformanceObserver/PerformanceEntries types in lumen-dom, mark/measure methods in Document, PerformanceEntryType enum, Document.performance integration, shell ObserverKind::Performance variant, 16 unit tests PASS, clippy clean 2026-05-29 — Phase 2-3 P3 JS binding + observer callback delivery pending
- font-loading-api (6.8, Phase 1): FontFace/FontFaceStatus/FontFaceSet types in lumen-dom, document.fonts collection, rule_to_font_face() converter + extraction from stylesheets into Document.fonts, 7 unit tests PASS, clippy clean 2026-05-28 — Phase 2-3 P3 JS binding pending
- print-pdf-advanced (36): Phase 1 ✅ @page rule matching + margin-box layout (17 tests, 2026-05-28); Phase 2 ✅ page counters + content generation (PageCounters, ContentFunction, resolve_content_function, 17 new tests, 31 total PASS, clippy clean 2026-05-29) — Phase 3 (margin-box text layout + inline content rendering) pending
- svg-layout-advanced (35): Phase 1 ✅ (2026-05-28) PreserveAspectRatio + transform parser; Phase 2 ✅ nested SVG transforms (SvgTransform in BoxKind::SvgShape, svg_group_transform in LayoutBox, apply_transform_to_bbox, parent transform cascading, 5 new tests, 33 total PASS, clippy clean 2026-05-29); Phase 3 ✅ SvgRoot child propagation (nested SVG elements now have children in layout tree), 3 unit tests PASS, clippy clean 2026-05-30 — Phase 4 (paint/render for nested SVG viewBox+transforms) pending
- accessibility-forms-validation (6.2, Phase 3): FormSubmitEvent enum (Valid/Invalid variants), submit_form() function executing HTML5 §4.10.22 form submission algorithm with constraint validation, 8 comprehensive unit tests (valid/invalid scenarios, field collection, defaults), 157 tests PASS, clippy clean 2026-05-28 — P3 dispatch integration pending
- transition-advanced (6.7, Phase 2): fill-mode support (Backwards/Forwards/Both applied in delay/completion periods), interrupted transitions (captured value at interruption for smooth continuation), grouped property expansion (margin/padding/border/border-radius → component properties), 3 new unit tests (fill_mode_forwards, fill_mode_backwards, interrupted_detection), 90 tests PASS, clippy clean 2026-05-28
- ime-input (6.3, Phase 1): IME composition events infrastructure — CompositionEventType/CompositionData/CompositionEvent structures, CompositionState in Document, public API (begin_composition/update_composition/end_composition/get_composition), 8 unit tests (composition lifecycle, serialization, full IME sequence), 121 tests PASS, clippy clean 2026-05-28 — P3 shell integration pending
- accessibility-forms-validation (6.2, Phase 1): ValidityState integration in accessibility tree, compute_state() reads element_validity() to populate AXNode.invalid, aria-invalid="true" preserved as explicit override, 8 unit tests (required/email/length/valid scenarios), 125 tests PASS, clippy clean 2026-05-28
- contenteditable-advanced (32, Phase 2): undo/redo command history (DomCommand enum, CommandHistory struct, insert_text/delete_range/replace_text), paste support (PasteData with text/html/files, paste_into function), drag-drop support (DragData, drop_into function), 10 unit tests PASS, clippy clean 2026-05-28 — P3 shell integration pending
- shadow-dom-accessibility-forms-gc (6+, Phase 2B+2C): Phase 2B — accessibility tree composition in nested shadow trees, Document::find_by_id() ARIA relationship resolution; Phase 2C — transparent role handling (Presentation/None/Generic/Group skipped in parent role context validation), proper ARIA role inheritance across shadow boundaries, 117 tests PASS, clippy clean 2026-05-28
- click-hint-overlay (7B.2): enhance collect_clickable_elements with <details> support — add ClickableKind::Details variant, is_details_element() helper, comprehensive unit tests (6 new tests for link/button/input/details/mixed), P1 complete 2026-05-28 — P3 integration pending
- print-pdf-pagination (5++, Phase 1): PaginationContext + Page + PageFragment, paginate() algorithm for break-before/after/avoid, 7 unit tests, exports in lib.rs, clippy clean 2026-05-28
- bench-ram-axis (9G.5, ADR-008 performance gate): cross-platform RSS measurement (getrusage on Unix, GetProcessMemoryInfo on Windows), baseline.json established, UPDATE.md documentation 2026-05-28
- antidetect-webgl-normalize (9D.2, ADR-007 Layer 4): GpuFingerprint struct in paint/src/fingerprint.rs, from_adapter_info() normalization (always "WebKit"/"Generic GPU"), Renderer.gpu_fingerprint field, install_webgl_bindings() in js/src/webgl_bindings.rs stores _LUMEN_GPU_VENDOR/_RENDERER globals, 5 tests, P1 complete 2026-05-28 — P3 integration pending
- gpu-layer-lru (10F, Phase 1): LayerCache struct in paint/src/layer_cache.rs, LRU-tracked GPU layer metadata (LayerKey + LayerEntry), get_lru_candidates() for eviction, remove_keys() for memory reclaim, 256 MB default budget, 7 unit tests + Renderer integration, Phase 2 (10F.2) texture pool recycling pending 2026-05-28
- antidetect-canvas-randomization (9D.1): CanvasNoiseGenerator LCG RNG в canvas/src/fp_noise.rs, per-session deterministic XOR-noise R/G/B, Context2D::set_noise_generator() + get_image_data() stub для P3 JS-интеграции, 20 тестов 2026-05-28
- extras-p2 (5++): object-fit ✅ + variable fonts ✅ + Print PDF Phase 1 (pagination module) 2026-05-27
- glyph-atlas-eviction (10G.1): LRU tracking + get_lru_candidates() + remove_keys() для эвикции, 4 новых теста, Phase 1 завершена 2026-05-27
- fts-omnibox (8F.1, Wave 1): HistoryWithFts integration with lumen-storage::History — record_visit_with_text() + delete_with_fts() automatic sync hooks, 3 integration tests → 49 total PASS 2026-05-27
- lumen-a11y-full (8G, stage 3/3): ARIA attribute application (aria-current/modal/roledescription/valuenow/min/max/text) + computed role mapping with context validation (cell/columnheader/rowheader require row; row requires table; listitem requires list; tab requires tablist; option requires listbox; treeitem requires tree; menuitem requires menu) + relationship attributes (controls/owns/flowto/details) with NodeId storage pending Document::find_by_id() + 30 new tests → 104 total PASS 2026-05-27
- lumen-a11y-full (8G, stage 2/3): label association (explicit + implicit) + form control text alternatives + description edge cases + button icon handling + link/heading/summary explicit naming + 21 new tests → 75 total PASS 2026-05-27
- lumen-a11y-full (8G, stage 1/3): 18 extended ARIA roles (Alert/AlertDialog/Application/Feed/Log/Marquee/Note/RowHeader/Searchbox/Switch/Tab/TabList/TabPanel/Timer/Toolbar/Tooltip/Tree/TreeItem) + AXRole::parse + implicit_role for <input type="search"> → Searchbox, Serialize/Deserialize for AXNode/AXState/AXRole, serde integration for P3 snapshots, 16 new tests → 60 total tests PASS 2026-05-27
- icc-color-profiles (Wave 1): IccProfile struct in lumen-image, Optional<IccProfile> in Image, parse_png_icc_profile() for PNG iCCP chunk (flate2 deflate decompression), Image constructors updated, JPEG/PNG/GIF decoders wired 2026-05-27
- font-variable-opsz (Wave 1): VariationCoords struct in lumen-font, from_css_settings() builder, set_axis_by_tag() for P4 to inject opsz, CSS integration points marked 2026-05-27
- font-stretch-matcher (Wave 1, stage 2): FaceRecord::stretch field, 3-step match_face filter (stretch→style→weight), 5 stretch tests, CSS Fonts L4 §5.2 compliance 2026-05-27
- font-stretch-matcher (Wave 1, stage 1): Os2::width_class field, stretch_percent() method, 5 tests 2026-05-27
- gif-decoder (Wave 1): skeleton GIF87a/89a decoder + frame 0 support (LZW decoding, palette→RGBA), animation Wave 3 2026-05-27
- paint-pure-audit (10D.2 Invariant 3): audit lumen-paint::display_list на pure-function requirement 2026-05-27
- layout-pure-audit (10D.1 Invariant 3): audit на отсутствие static MUT / lazy_static / OnceCell в hot path 2026-05-27
- dom-arena-audit (10B Invariant 1, [P1+P3]): serde+bincode snapshot, DomSnapshotError, #[deny(clippy::rc_buffer)]+INVARIANT(10B/ADR-008) 2026-05-27

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

### Phase 1 ✅ (complete as of 2026-05-28)
21. ✅ lumen-a11y-full (8G, [P1+P3]) — ARIA role mapping (60+ variants), text alternatives (accname §4), computed role mapping, 104 tests PASS 2026-05-27
22. ✅ dom-arena-audit (10B Invariant 1, [P1+P3]) — NodeId(u32)-based, bincode snapshot, #[deny(clippy::rc_buffer)], 7 tests 2026-05-27
23. ✅ layout-pure-audit (10D.1 Invariant 3, [P1]) — no static MUT / lazy_static / OnceCell in hot path 2026-05-27
24. ✅ paint-pure-audit (10D.2 Invariant 3, [P1]) — display_list pure-function requirement 2026-05-27
25. ✅ antidetect-canvas-randomization (9D.1, [P1+P2]) — CanvasNoiseGenerator LCG RNG, per-session seed, 20 tests 2026-05-28
26. ✅ antidetect-webgl-normalize (9D.2, [P1+P2]) — GpuFingerprint struct, adapter normalization, 5 tests 2026-05-28
27. ✅ gpu-layer-lru (10F, [P1+P2+P3]) — LayerCache LRU + GPU memory budget, 7 tests 2026-05-28
28. ✅ glyph-atlas-eviction (10G, [P1+P2+P3]) — LRU eviction from atlas, 4 tests 2026-05-27

### Phase 2 (in progress / planned)
29. ✅ animation-keyframe-easing (6.6) — full timing functions (ease/cubic-bezier/steps) complete in Phase 0 (2026-05-20)
30. ✅ transition-advanced (6.7) — Phase 1: transition_fill_modes field + grouped property expansion ✅; Phase 2: fill-mode scheduling + interrupted transitions ✅; Phase 3: fill-mode tests ✅ (2026-05-28)
31. ✅ shadow-dom-accessibility-forms-gc (6+) — Phase 2A: slot delegation + fallback ✅; Phase 2B: accessibility tree composition + FlatTree integration + ARIA relationships ✅; Phase 2C: transparent role handling across boundaries ✅ (2026-05-28)
32. 🟡 contenteditable-advanced — undo/redo command history ✅; paste data support ✅; drag-drop support ✅; Phase 1-3 complete, P3 shell integration pending (2026-05-28)
33. 🟡 accessibility-forms-validation — Phase 1: ValidityState in AXTree ✅ (2026-05-28); Phase 2: enhanced constraint validation (all error types + custom messages) ✅ (2026-05-28); Phase 3: submit algorithm integration
34. 🟡 ime-input ([P1+P3]) — Phase 1: composition state tracking ✅ (2026-05-28); Phase 2: composition ranges + event data structures ✅ (2026-05-28); Phase 3: virtual keyboard hints and P3 shell integration pending
35. 🟡 svg-layout-advanced — Phase 1: SVG transforms, aspect-ratio preservation ✅ (2026-05-28); Phase 2: nested SVG transforms ✅ (2026-05-29); Phase 3: nested SVG layout (SvgRoot child propagation) ✅ (2026-05-30) — Phase 4 (paint/render for nested SVG viewBox+transforms) P2 pending
36. 🟡 print-pdf-advanced — Phase 1: @page matching, margin-box model ✅ (2026-05-28); Phase 2: page counters + content generation ✅ (2026-05-29); Phase 3: margin-box text layout + inline content rendering pending
37. 🟡 font-loading-api ([P1+P3]) — Phase 1: FontFace/FontFaceStatus/FontFaceSet types, document.fonts collection, @font-face extraction + population, 7 unit tests PASS 2026-05-28 — Phase 2-3 P3 JS binding pending
38. 🟡 performance-observer-timing ([P1+P3]) — Phase 1: PerformanceEntry/PerformanceObserver/PerformanceEntries types, mark/measure methods, Document.performance integration, 16 unit tests PASS 2026-05-29 — Phase 2-3 P3 JS binding + observer callback delivery pending

## Notes

- fts-omnibox (Wave 1, P3-domain) moved to STATUS-P3.md Queue — knowledge/omnibox is P3 domain, not P1
- Wave 2 (former P2/P3) queues removed — handled in respective STATUS-P2.md / STATUS-P3.md
- All Phase 0 items marked ✅ as complete per 2026-05-26 phase close
- Phase 1-2 items (21–40) represent next ~6 months of P1 work
- ADR-006 (automation), ADR-007 (anti-detection), ADR-008 (tab lifecycle) coordination embedded in tasks 21-30
- Coordination points with P2 (paint/render side) and P3 (shell/runtime integration) noted in brackets [Pn]

Recent: ime-input (6.3, Phase 2): composition range helpers (is_composing, get_composition_range, get_composition_target), fixed CompositionData.range semantics, extended CompositionEvent tests for P3 dispatching, P3 integration doc, 149 tests PASS 2026-05-28 — P3 shell integration pending; click-hint-overlay (7B.2, P1 complete 2026-05-28, P3 integration pending), print-pdf-pagination (Phase 1 complete, PaginationContext ready), bench-ram-axis (Phase 0 complete, cross-platform RSS baseline established)

## Recent (prior work history, last 30 days)

- 2026-05-28: ime-input (6.3, Phase 2): composition range helpers, improved semantics, extended tests, 149 total PASS
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
