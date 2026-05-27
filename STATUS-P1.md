In progress: none (ready for next task)
Next step: (check lumen-plan.md Phase 2 — next P1 from Wave 2 Queue or Phase 2 system tasks)

CSS rule: P1 does NOT implement CSS properties. P4 owns all CSS.
  P1 writes layout algorithms and box-tree structure only.
  When a new algorithm needs a CSS property → add // CSS: <prop> comment at
  the call site and add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P1 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Recent:
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

Next (Wave 1 — бывшие P2-задачи):
(All completed — next waves in Queue)

Next (Wave 1 — бывшие P3-задачи):
- fts-omnibox: lumen-knowledge::HistoryFts + omnibox @history prefix + Porter stemmer для RU
  → STATUS-P3.md:12

Queue (Wave 2 — бывший P2):
- extras-p2: object-fit, variable fonts, Print PDF
  → lumen-plan.md:195
- avif-decoder: AVIF/AV1 декодер через rav1d (provisional dep)
  → STATUS-P2.md:10
- webgl-context: WebGL 1.0 поверх wgpu (WebGL API → wgpu calls)
  → STATUS-P2.md:11
- font-hinting: TrueType bytecode hinting в rasterizer
  → STATUS-P2.md:12
- subpixel-text: subpixel LCD rendering — RGB-stripe фильтр; toggleable prefers-reduced-motion
  → STATUS-P2.md:13
- svg-rasterizer: SVG basic shapes (path/circle/rect) через paint pipeline
  → STATUS-P2.md:14

Queue (Wave 2 — бывший P3):
- http2-client: HTTP/2 через h2 crate — multiplexing; бэкэнд-замена HttpClient без смены API
  → STATUS-P3.md:15
- preconnect-hints: <link rel=preconnect> из preload_scanner — открыть TCP+TLS заранее
  → STATUS-P3.md:16

Queue (Phase 2 — системные P3-задачи):
- sop-remaining: SOP checks при postMessage/storage/cookies; mixed-content блокировка до TCP;
  DOM-применение sandbox в shell (CORS preflight ✅, enforcement остался)
  → lumen-plan.md:202 (таблица), lumen-plan.md:324-325 (детали)
- forms-ui: native pickers, autofill popup, validation tooltip UI (P1-логика ValidityState ✅)
  → lumen-plan.md:332
- picture-lazy-intersect: IntersectionObserver event source для loading="lazy" +
  shell GPU image upload integration (P1-парсер ✅)
  → lumen-plan.md:333
- ime-input: IME composition events через winit IME API (compositionstart/update/end)
  → lumen-plan.md:334
- shadow-dom-bindings: JS bindings Element.attachShadow/customElements (P1 FlatTree ✅)
  → lumen-plan.md:330

Queue (Phase 2 — Tab UX, [P3] 7A):
- vertical-tabs: Vertical tabs panel (toggle, drag-reorder, collapse) → shell/src/tabs/vertical.rs
  → lumen-plan.md:237
- tree-tabs: Tree-style tabs (parent-child) → shell/src/tabs/tree.rs
  → lumen-plan.md:238
- workspaces: Workspaces (изолированные группы) → shell + storage/src/workspaces.rs
  → lumen-plan.md:239
- split-view: Split view 2-4 viewport → shell + paint multi-viewport (координация с бывшим P2)
  → lumen-plan.md:240
- tab-auto-archive: Tab auto-archive (hibernate по возрасту) → shell/src/tabs/archive.rs
  → lumen-plan.md:241

Queue (Phase 2-3 — Power-user input, [P3] 7B):
- vim-keys: Vim-style key bindings (modal) → shell/src/input/vim.rs
  → lumen-plan.md:243
- click-hint-overlay: Click-hint vimium-style overlay (layout-итератор clickable готов)
  → lumen-plan.md:244
- mouse-gestures: Mouse gestures → shell/src/input/gestures.rs
  → lumen-plan.md:245
- omnibox-aliases: Custom omnibox aliases → shell + user config
  → lumen-plan.md:246
- find-in-page-regex: regex UI + highlight overlay (collect_visible_text P1 ✅)
  → lumen-plan.md:247

Queue (Phase 2 — Privacy UX, [P3] 7C):
- block-list-engine: EasyList + hosts files → network/src/filter/easylist.rs (impl RequestFilter)
  → lumen-plan.md:249
- per-site-permissions: Per-site permission UI panel → shell/src/site_settings/
  → lumen-plan.md:250
- cookie-banner: Cookie-banner auto-dismiss → shell/src/cookies/banner.rs (JsRuntime)
  → lumen-plan.md:251
- shields-widget: Shields toolbar widget (счётчик блокировок) → shell/src/toolbar/shields.rs
  → lumen-plan.md:252

Queue (Phase 2-3 — Web platform baseline, [P3] 7D):
- webauthn: Passkeys/WebAuthn CTAP2 + navigator.credentials → network/src/webauthn.rs
  → lumen-plan.md:254
- tab-containers: Tab containers (storage partitioning) → storage/src/partition.rs
  → lumen-plan.md:255
- sidebar-panels: Sidebar web panels → shell/src/sidebar/web_panel.rs
  → lumen-plan.md:256

Queue (Phase 3+ — бывший P3):
- service-workers: Service Worker API (fetch intercept + cache API + background sync)
  → STATUS-P3.md:19
- push-api: Web Push + Notifications API (VAPID, push subscription)
  → STATUS-P3.md:20
- profiles-system: multi-profile — cookies/history/storage per profile
  → STATUS-P3.md:21
- devtools-protocol: CDP subset — Elements + Console + Network
  → STATUS-P3.md:23

Queue (Phase 4+ — DevTools полный, [P3] 7E):
- dom-inspector: DOM inspector panel (tree + attributes) → devtools + lumen-dom
  → lumen-plan.md:258
- computed-styles-panel: Computed styles panel → сериализация ComputedStyle (P4 expose JSON)
  → lumen-plan.md:259
- box-model-overlay: Box model overlay (margin/border/padding) → DisplayCommand overlay
  → lumen-plan.md:260
- network-panel: Network panel (live request log) → devtools слушает NetworkTransport events
  → lumen-plan.md:261
- js-console: JS console (eval в контексте страницы) → devtools + JsRuntime::eval
  → lumen-plan.md:262

Queue (Phase 1 — Automation API из ADR-006):
- lumen-a11y-full (8G, [P1+P3]): расширить lumen-a11y crate до полного AccessibilityTree — text alternative computation (accname §4), ARIA attribute application, focus model, computed role mapping HTML-AAM полностью (базовая 36-тестовая accessibility-aria уже есть). Используется в BrowserSession::a11y_tree() + BrowserSession::query(Role/Name) — Playwright-стиль getByRole для тестов и AI-агентов. → lumen-plan.md трек 8G + ADR-006.

Queue (Phase 2 — ADR-007 anti-detection, rendering fingerprint side; бывший [P3+P2] → P1+P3):
- antidetect-canvas-randomization (9D.1): Brave-style canvas randomization — getImageData возвращает RGBA с per-session deterministic seed noise. P1 owns canvas/paint side (где данные читаются обратно из texture); P3 owns JS bindings к Canvas API. → lumen-plan.md трек 9D.1 + ADR-007.
- antidetect-webgl-normalize (9D.2): WebGL renderer/vendor strings normalization (обобщённые «Generic GPU» / «WebKit»). P1 — где wgpu adapter info собирается; P3 — JS-side getParameter() handling. → lumen-plan.md трек 9D.2 + ADR-007.

Queue (Phase 2 — ADR-008 tab lifecycle T0/T2 экономия; бывший [P3+P2] → P1+P3):
- gpu-layer-lru (10F): LayerCache с LRU + GPU memory budget; texture pool recycling (одна wgpu::Texture переиспользуется для разных layers). Off-viewport stacking contexts освобождают textures при удалении от viewport на >3 экрана. P3 owns lifecycle integration (через MemoryPressureSource events); P1 owns wgpu/texture pool в lumen-paint. → lumen-plan.md трек 10F + ADR-008.
- glyph-atlas-eviction (10G): LRU eviction редко используемых глифов из атласа. Атлас не растёт безгранично. P1 owns atlas data structure в lumen-font; P3 owns подписка на MemoryPressureSource events. → lumen-plan.md трек 10G + ADR-008.

Queue (Phase 2-3 — оригинальный P1 backlog):
- shadow-dom-accessibility-forms-gc: Shadow DOM / Accessibility / Forms / GC
  → lumen-plan.md:160

Recent: dom-arena-audit (serde+bincode snapshot для T3 hibernation, DomSnapshotError, #[deny(clippy::rc_buffer)]+INVARIANT(10B/ADR-008), 7 тестов roundtrip, 121 итого) 2026-05-27, off-screen-render (Renderer::new_headless + render_to_image: headless wgpu adapter, RENDER_ATTACHMENT+COPY_SRC texture, staging buffer readback, resize support; 3 ignored GPU тестов в headless_tests.rs) 2026-05-27, layout-find-by-selector (find_box_by_selector + computed_style_by_selector + find_all_by_selector + ComputedStyleSnapshot в lumen-layout::selector_query; parse_selector_list в lumen-css-parser; 14 тестов; разблокирует P3 8A.2) 2026-05-27, line-clamp-layout (apply_line_clamp() в box_tree.rs: CSS Overflow L4 §3.2 multi-line truncation + ellipsis, приоритет над text-overflow:ellipsis; 6 тестов; graphic test 48) 2026-05-27, visible-text-iter (collect_visible_text + TextFragment в lumen-layout::text_iter, 10 тестов; P3 ready for find-in-page-regex) 2026-05-27, contenteditable-dom-mutations (EditInputType 12 вариантов §4.1.3, InputEvent, split_text_node, insert_text_at, delete_range, insert_paragraph_break, Document::insert_after; 15 тестов → 105 итого) 2026-05-27, details-clickable (<details>/<summary> open/closed rendering HTML5 §4.11.1: build_box filters non-summary children when open absent; collect_clickable_elements(LayoutBox, Document) → Vec<ClickableElement> для P3 click-hint overlay; ClickableKind::Link/Button/Input/Generic; 7 тестов) 2026-05-27, forms-validity (ValidityState HTML5 §4.10.21 в lumen-dom: valueMissing/typeMismatch/tooLong/tooShort/rangeUnderflow/rangeOverflow + check_validity_form + invalid_controls_in_form, build_form_submit блокирует невалидный submit, form_validity делегирует в dom, 21 тест → 89 итого) 2026-05-27, selection-range (DomPosition+Range+Selection в lumen-dom, source_node+source_char_offset в InlineSegment+InlineFrag, caret_at_point+selection_rects в lumen-layout/selection.rs, 26 тестов) 2026-05-27, html5-insertion-modes-remaining (InsertionMode 23 из 23: InHeadNoscript+InFrameset+AfterFrameset+AfterAfterFrameset, InSelectInTable полная, reset_insertion_mode frameset/select-in-table, scripting_enabled flag, 15 новых тестов → 374 всего) 2026-05-27, accessibility-aria (crate lumen-a11y: AXRole 60+ variant, AXState 17 полей, AXNode, AXTree, build_ax_tree, compute_name, compute_description, implicit_role HTML-AAM, 36 тестов) 2026-05-27, svg-layout-basic (BoxKind::SvgRoot/SvgShape, SvgShapeKind rect/circle/ellipse/line/path, ViewBox scale+offset, collect_svg_shapes flat traversal, lay_out_svg_root replaced-element sizing, paint emit_svg_shape stub, 12 тестов, graphic test 47) 2026-05-27, colspan-rowspan (col_span/row_span на LayoutBox, span-aware column width + placement + rowspan height post-fix, 7 тестов) 2026-05-27, html-template-content (DocumentFragment + InTemplate mode + <template> парсинг content во fragment, 12 тестов) 2026-05-27, form-submit (Event::FormSubmit + find_ancestor_form + collect_dom_form_fields + build_form_submit + make_get_url + GET-навигация, 20 тестов) 2026-05-27, css-first-line-letter (PseudoKind::FirstLetter на первом тексте, is_first_line на lines[0], 3 новых теста) 2026-05-27, html-loading-lazy (loading="lazy" ImageRequest.is_lazy + JS _lumen_init_lazy_images/_lumen_deliver_lazy_images + shell proximity fetch, 9 тестов) 2026-05-26, html-full-tree-builder (HTML5 §13.2 insertion modes + adoption agency, 17 режимов, AAA, 349 тестов) 2026-05-26, phase0-close (Phase 0 закрыта, маркеры ✅ для html-parser/css-parser/layout) 2026-05-26, fix-inline-block-baseline (BUG-023 P1-часть — strut только для baseline-строк; TEST-12 PASS 0.18%, TEST-13 PASS 0.24%) 2026-05-26, fix-max-height (BUG-025 подтверждён в layout — release-бинарь был устаревшим, TEST-11 PASS 0.43%, unit tests для max-height/min-height/vertical-align:bottom добавлены) 2026-05-25, full HTML5 named entities WHATWG 2125 (gen_entities.py + бинпоиск + 338 тестов) 2026-05-25, push-tokenizer feed_bytes(&[u8]) с буферизацией partial UTF-8, 7 тестов (342 итого) 2026-05-25, ADR-инфраструктура docs/decisions/ (TEMPLATE.md + README + ADR-001..005) 2026-05-25
