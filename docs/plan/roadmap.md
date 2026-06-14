## Roadmap — приоритизация задач

**Перерабатано 2026-05-15 под 3 программистов с акцентом на параллельность.** Главный приём — **interface-first**: сначала публикуются типы/трейты (с `todo!()` или stub), дальше каждый реализует *против stub-а*, не дожидаясь impl у других. Реальная стыковка происходит drop-in (пустой stub → реальный impl, потребитель ничего не правит).

Маркеры `[P4]` из истории не удаляются (commits, prior fundamentals); в новом roadmap-е используются только `[P1] / [P2] / [P3]` и комбинации `[P1+P2] / [P1+P3] / [P2+P3] / [P1+P2+P3]`. Бывший P4 поглощён P3 — таблица в `CLAUDE.md` отражает это.

---

### Sprint 0 — Контракты (1-2 дня, общий PR `contracts-3-devs`)

Каждый кладёт **только сигнатуры** (типы/трейты), `todo!()` в теле. Этот PR разблокирует параллельные треки ниже без каких-либо ожиданий.

- ✅ **`[P1]`** `StackingContextId`, `PaintOrder`, `PropertyTrees { transform, scroll, effect, clip }`, типизированные `Length` / `Color` (как уже есть в `lumen-layout::values`, дополнить полями ComputedStyle), trait `AnimationInterpolator` со stub-реализацией. **Реализовано в ветке `sprint0-p1-contracts`**: модули `lumen-layout::stacking` (StackingContextId / PaintPhase / PaintOrder / StackingTree), `property_trees` (4 дерева + Mat4 + PropertyTreeNodeId), `animation` (AnimValue + AnimationInterpolator trait + NoopInterpolator step-half stub). ComputedStyle уже типизирован (Color struct, BorderStyle enum, resolved-px f32) — типизация declaration-level (text-value → AST) остаётся отдельной задачей 1B.
- **`[P2]`** `trait Compositor { fn commit(&mut self, trees: &PropertyTrees, layers: &LayerTree); }`, `trait LayerTree`, расширения `DisplayCommand` для blend / clip / layer / image upload.
- ✅ **`[P3]`** Все trait-anchors из политики зависимостей: `UnicodeProvider`, `IdnaProvider`, `PublicSuffixList`, `ContentDecoder` (с расширением для `br` / `zstd`), `FontFormat` (под WOFF2), `JsRuntime` (под rquickjs/V8), `SpellChecker`, `HyphenationProvider`. Каждый — со stub-реализацией, возвращающей «не поддерживается» (`NullJsRuntime`, `NullUnicodeProvider`, `NullIdnaProvider`, `NullPublicSuffixList`, `UnsupportedContentDecoder`, `NullFontFormat`, `NullSpellChecker`, `NullHyphenationProvider`). **Это — главный enabler параллельности.** Готово в ветке `contracts-p3` (merge 2026-05-15).

После merge `contracts-3-devs` стартуют три независимых трека.

---

### Параллельные треки (ничей impl никого не блокирует)

#### Track P1 — Frontend engine

> Подробности реализации каждой задачи — в [§История реализации](#история-реализации).

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| 1A | ✅ **`[P1]` Quirks-mode application** | Половина legacy-сайтов | 2026-05-20 |
| 1A.1 | ✅ unitless length quirk | `layout/src/style.rs:2945` | — |
| 1A.2 | ✅ IE7 line-height quirk | `layout/src/style.rs:5013` | — |
| 1A.3 | ✅ quirks test coverage | `layout/src/lib.rs:8449` | — |
| 1A.4 | ✅ HTML pres. hints: `<font size/face>`, `img hspace/vspace/border`, `align` | `layout/src/style.rs:5482` | 2026-05-20 |
| 1B | ✅ **`[P1]` Типизированные `Length`/`Color`** | P2 п.3A; P3 CSSOM | — |
| 1B.1 | ✅ Length type через все декларации каскада | `layout/src/style.rs` | 2026-05-19 |
| 1B.2 | ✅ Color type через все декларации каскада | `layout/src/style.rs` | 2026-05-19 |
| 2A | ✅ **`[P1+P2]` Stacking contexts impl** | P2 п.2A | — |
| 2B | ✅ **`[P2+P1]` Property trees построение** | P2 п.1B | — |
| 3A | ✅ **`[P1+P2+P3]` Web Animations interpolation** | P2 п.3B; P3 scheduling | 2026-05-20 |
| ~~3B~~ | ✅ **`[P1+P3]` Push-tokenizer + incremental tree builder** | P3 п.4B | — |
| 4A | ✅ **`[P1+P2]` `<picture>`/`srcset`/`sizes` finishing** | P3 lazy-loading | — |
| 4B | ✅ **`[P1]` CSS Grid + полный Flexbox** | Адаптивные сайты | 2026-05-20 |
| 4B.1 | ✅ flex-direction + flex-wrap properties | `layout/src/style.rs` | — |
| 4B.2 | ✅ flex-grow + flex-shrink + flex-basis | `layout/src/style.rs` | — |
| 4B.3 | ✅ flex item layout pass (main/cross axis) | `layout/src/box_tree.rs` | — |
| 4B.4 | ✅ flex gap application | `layout/src/box_tree.rs:509` | — |
| 4B.5 | ✅ flex wrapping + multi-line | `layout/src/box_tree.rs:509` | — |
| 4B.6 | ✅ CSS Grid properties (GridTrackSize/GridLine/GridAutoFlow) | `layout/src/style.rs` | — |
| 4B.7 | ✅ CSS Grid layout (lay_out_grid + blockify) | `layout/src/box_tree.rs` | — |
| 4C | ✅ **`[P1]` CSS Positioned Layout** | position: relative/absolute/fixed + inset | 2026-05-20 |
| 4C.1 | ✅ top/right/bottom/left + inset shorthand | `layout/src/style.rs` | — |
| 4C.2 | ✅ position: relative offset (shift_tree) | `layout/src/box_tree.rs` | — |
| 4C.3 | ✅ position: absolute/fixed (out-of-flow + PCB) | `layout/src/box_tree.rs` | — |
| 5 | ✅ **`[P1]` ICU4x segmenter + linebreak** | CJK типографика | — |
| 5.1 | ✅ ICU4x struct + segmenter init | `encoding/src/unicode_provider.rs` | — |
| 5.2 | ✅ line_break_opportunities impl | `encoding/src/unicode_provider.rs` | — |
| 5.3 | ✅ grapheme_boundaries impl | `encoding/src/unicode_provider.rs` | — |
| 5.4 | ✅ word_boundaries impl | `encoding/src/unicode_provider.rs` | — |
| 5.5 | ✅ bidi_runs impl | `encoding/src/unicode_provider.rs` | — |

#### Phase 1 ✅ (complete)

| # | Задача | impl / Разблокирует | Дата |
|---|---|---|---|
| 8G.1 | ✅ **`[P1+P3]` lumen-a11y-full stage 1** | ARIA role mapping (67 variants in `a11y/src/roles.rs`), text alternatives (accname §4) | Phase 1 ✅ 2026-05-27 |
| 8G.2 | ✅ **`[P1+P3]` lumen-a11y-full stage 2** | ARIA attributes (aria-current/modal/roledescription/valuenow/controls/owns/flowto/details), NodeId storage | Phase 1 ✅ 2026-05-27 |
| 8G.3 | ✅ **`[P1+P3]` lumen-a11y-full stage 3** | Full computed role mapping HTML-AAM, focus model, form control text alternatives, 125 tests PASS | Phase 1 ✅ 2026-05-27 |
| 10B | ✅ **`[P1+P3]` Invariant 1: DOM arena serialization** | `NodeId(u32)` no `Rc<RefCell>`, `Document::to_bytes`/`from_bytes` via bincode for T3 snapshot | ADR-008 Phase 1 ✅ 2026-05-27 |
| 10D.1 | ✅ **`[P1]` Invariant 3a: layout pure audit** | No `static MUT` / `lazy_static!` / `OnceCell` in `lumen-layout/src` hot path | ADR-008 Phase 1 ✅ 2026-05-27 |
| 10D.2 | ✅ **`[P1]` Invariant 3b: paint pure audit** | `paint/src/display_list.rs` pure-function requirement met (only comment marker present) | ADR-008 Phase 1 ✅ 2026-05-27 |
| 9D.1 | ✅ **`[P1+P2]` Canvas randomization** | `CanvasNoiseGenerator` LCG RNG in `canvas/src/fp_noise.rs`, per-session seed, 20 tests | ADR-007 Phase 1 ✅ 2026-05-28 |
| 9D.2 | ✅ **`[P1+P2]` WebGL normalize** | `GpuFingerprint` struct in `paint/src/fingerprint.rs`, adapter normalization, 5 tests | ADR-007 Phase 1 ✅ 2026-05-28 |
| 10F | ✅ **`[P1+P2+P3]` GPU layer LRU** | `LayerCache` in `paint/src/layer_cache.rs`, LRU + GPU memory budget, 7 tests | ADR-008 Phase 1 ✅ 2026-05-28 |
| 10G | ✅ **`[P1+P2+P3]` Glyph atlas eviction** | LRU eviction in `paint/src/atlas.rs` (`get_lru_candidates`, `remove_keys`), 4 tests | ADR-008 Phase 1 ✅ 2026-05-27 |

#### Phase 2-3 (planned)

| # | Задача | impl / Разблокирует | Дата |
|---|---|---|---|
| 6+ | 🟡 **`[P1+P3]` Shadow DOM / Accessibility / Forms / GC extended** | Advanced contenteditable, form validation, accessibility full integration | Phase 2-3. P1: Shadow DOM ✅, Forms ✅, GC ✅, Selection API ✅ (2026-05-30), platform a11y bridges Phase 0 ✅ (2026-06-09). P3: native pickers, validation tooltip UI, Phase 1 native UIA/NSAccessibility/AT-SPI2 bindings |
| 6.1 | ✅ contenteditable drag-drop + paste + undo/redo | Input dispatch coordination with shell | Phase 2 ✅ P1 done (2026-05-28); Phase 3 P3 shell integration pending |
| 6.2 | ✅ accessibility forms validation + visualization | Constraint validation in accessibility tree | Phase 1-3 ✅ P1 done (2026-05-28); P3 pending |
| 6.3 | ✅ ime-input composition events + ranges | Keyboard input for CJK/Cyrillic | Phase 1-3 ✅ P1 done (2026-05-31); Phase 2-3 P3 shell integration pending |
| 6.4 | ✅ svg-layout advanced transforms + viewport nesting | SVG aspect-ratio preservation | Phase 1-3 ✅ P1 done (2026-05-30); Phase 4 ✅ P2 done (2026-05-29): DrawSvgPath + tessellator |
| 6.5 | ✅ print-pdf advanced @page margin boxes + headers/footers | Full print pipeline from margin-box content | Phase 1-4 ✅ P1 done (2026-05-31); P1 inline content rendering ✅ done (2026-06-02) |
| 6.6 | ✅ animation keyframe easing (cubic-bezier/steps) | Full timing functions support | Phase 1-2 ✅ P1 done (2026-05-20); complete in Phase 0 |
| 6.7 | ✅ transition advanced (interrupted/fill-mode/grouped) | Animation lifecycle completeness | Phase 1-3 ✅ P1 done (2026-05-28) |
| 6.8 | ✅ **`[P1+P3]` font-loading API** | @font-face lifecycle, FontFace interface, document.fonts | Phase 1 ✅ P1 done; Phase 2 ✅ JS bindings (_lumen_fonts_*); Phase 3 P3 pending |
| 6.9 | 🟡 **`[P1+P3]` performance-observer timing** | PerformanceEntry/PerformanceObserver types, mark/measure, observer delivery | Phase 1 ✅ P1 done (DOM types + mark/measure); Phase 2+ P3 JS binding + observer callback |

#### Track P2 — Backend rendering

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| ~~1A~~ | ✅ **`[P2]` Font fallback/matcher** | — | — |
| 1B | ✅ **`[P2+P1]` Compositor thread + layer tree** | Off-main-thread scroll | shell-интеграция active_tree() — P3 |
| 1B.1 | ✅ CompositorThread struct + spawn loop | `paint/src/compositor.rs:277` | — |
| 1B.2 | ✅ vsync tick-loop 60fps | `paint/src/compositor.rs:277` | — |
| 1B.3 | ✅ PushBlendMode/PopBlendMode pipeline Phase 0 | `paint/src/renderer.rs:1834` | — |
| 1B.4 | ✅ off-screen opacity layer rendering | `paint/src/renderer.rs` | 2026-05-19 |
| 1B.5 | ✅ GPU texture upload for layer snapshots | `paint/src/renderer.rs` | 2026-05-19 |
| 2A | ✅ **`[P1+P2]` Painting order traversal** | — | — |
| 2B | ✅ **`[P2]` Stacking-aware hit testing** | P3 shell input handler | — |
| 3A | ✅ **`[P2]` Color management + Display P3/Rec2020** | Фотографии с P3-профилем | Только `lumen-paint` |
| 3A.1 | ✅ ColorSpace enum in ComputedStyle | `layout/src/style.rs` | 2026-05-20 |
| 3A.2 | ✅ Display P3 parsing in CSS color functions | `layout/src/style.rs` | 2026-05-20 |
| 3A.3 | ✅ HDR tone-mapping utilities (sRGB↔P3) | `layout/src/style.rs` | 2026-05-20 |
| 3A.4 | ✅ ColorFloat variant (f32 channels) | `layout/src/style.rs` | 2026-05-20 |
| 3A.5 | ✅ color space awareness in renderer | `paint/src/display_list.rs` | 2026-05-20 |
| 3B | ✅ **`[P1+P2+P3]` Web Animations compositor offload** | Smooth-анимации | 2026-05-20 |
| 3B.1 | ✅ CompositorAnimFrame + CompositorOverride types | `layout/src/animation.rs` | 2026-05-20 |
| 3B.2 | ✅ AnimationFrame::to_compositor_frame() | `layout/src/animation.rs` | 2026-05-20 |
| 3B.3 | ✅ transform_fns_to_matrix helper | `layout/src/property_trees.rs` | 2026-05-20 |
| 3B.4 | ✅ build_display_list_with_anim + walk_with_anim | `paint/src/display_list.rs` | 2026-05-20 |
| 3B.5 | ✅ shell wires anim_frame → DL rebuild without relayout | `shell/src/main.rs` | 2026-05-20 |
| 4 | ✅ **`[P1+P2]` mix-blend-mode/backdrop-filter pipeline** | Современные UI-эффекты | 2026-05-20 |
| 4.5 | ✅ **`[P2]` Transform pipeline (DisplayCommand + renderer)** | P4 transform matrix | 2026-05-20 |
| 4.5.1 | ✅ PushTransform/PopTransform DisplayCommand | `paint/src/display_list.rs` | 2026-05-20 |
| 4.5.2 | ✅ forward_box_transform публичный из layout | `layout/src/property_trees.rs` | 2026-05-20 |
| 4.5.3 | ✅ transform_stack + CPU-side vertex transformation | `paint/src/renderer.rs` | 2026-05-20 |
| 5A | ✅ **Canvas 2D basic context** — CPU rasterizer, `CanvasRenderingContext2D` Phase 0 | `engine/canvas/` | 2026-05-22 |
| 5A.2 | ✅ **Canvas 2D JS bindings** — `canvas.getContext('2d')` → `lumen_canvas::Context2D`; `BoxKind::Canvas` replaced element; `DrawImage` keyed `canvas:{nid}`; dirty-buffer flush to renderer | `js/src/canvas2d.rs` + `layout/box_tree.rs` + `paint/display_list.rs` + `shell` | 2026-06-02 |
| 5A.5 | ✅ **Canvas 2D Phase 5 — Path2D** (HTML LS §4.12.5.1.5) — `Path2dData` user-space segments + SVG parser; fill/stroke/clip_with_path2d; JS `Path2D` class; CTM applied at use-time | `engine/canvas/src/path2d.rs`, `js/src/canvas2d.rs`, `js/src/dom.rs` | 2026-06-11 |
| 5B | ✅ **WOFF2/WOFF1 decoder** — brotli + zlib, glyf transform, sfnt rebuild | `engine/font/src/woff2.rs` | 2026-05-22 |
| 5+ | ✅ **GPU linear/radial gradient pipeline** — WGSL шейдер + CPU uniform + DrawOp::Gradient | `paint/src/renderer.rs` | 2026-05-22 |
| 5++ | ✅ **Extras**: object-fit ✅, variable fonts ✅, Print PDF Phase 1 (✅ pagination module) | `layout/src/pagination.rs` | 2026-05-28 |

#### Track P3 — Runtime + system (объединённый домен — больше треков, но всё параллельно)

| # | Задача | impl / Разблокирует | НЕ блокирует |
|---|---|---|---|
| 1B | ✅ **`[P3]` rquickjs integration scaffold** | Forms, Animations, SWs, DevTools | `crates/js/` |
| 2A | ✅ **`[P3]` SOP/CORS/mixed-content/sandbox** | Публичная сеть | Только network + shell |
| 2A.1 | ✅ block blockable in HttpClient::fetch | `network/src/lib.rs:1478` | — |
| 2A.2 | ✅ sandbox_flags on iframe DOM element | `dom/src/lib.rs` | — |
| 2A.3 | ✅ script execution gate in shell | `shell/src/main.rs:752` | — |
| 2A.4 | ✅ form submission gate | `dom/src/lib.rs` | — |
| 2A.5 | ✅ navigation restriction enforcement | `dom/src/lib.rs` + `shell/src/main.rs` | — |
| 2C | ✅ **`[P3]` Tab session export/import** | UX | `storage/src/session_export.rs` | JSON v1, --import-session, auto-save на close, 12 тестов |
| 3A | ✅ **`[P3]` DPR + scroll в shell** | 4K + длинные страницы | shell + paint |
| 3A.1 | ✅ relayout-on-resize | `shell/src/main.rs` | — |
| 3A.2 | ✅ horizontal scroll | `shell/src/main.rs` | — |
| 3A.3 | ✅ momentum scroll | `shell/src/main.rs` | — |
| 3B | ✅ **`[P3]` HTML event loop в Lumen-loop** | P1/P2 rAF | Только `lumen-shell::runtime` |
| 3B.1 | ✅ reload via queue_task | `shell/src/main.rs` | — |
| 3B.2 | ✅ rendering steps ordering | `shell/src/main.rs` | — |
| 3B.3 | ✅ real observers | `shell/src/main.rs` | — |
| 4A | ✅ **`[P3]` JS↔DOM bindings** (после 1B) | Любая JS-динамика | Phase 0: getElementById/querySelector/textContent/setAttribute/createElement/appendChild |
| 4B | ✅ **`[P1+P3]` Streaming pipeline shell-side** | Первый кадр без задержки | EventLoop\<LoadEvent\>: окно сразу, HTML bytes-chunks → feed_bytes → IncrementalTreeBuilder, промежуточные кадры каждые 150 мс; encoding decode один раз в финальном pipeline |
| 4B.1 | ✅ preload scan call before DOM parse | `shell/src/main.rs:688` | — |
| 4B.2 | ✅ preload hint dispatcher | `shell/src/main.rs` | — |
| 4B.3 | ✅ preload URL resolution | `shell/src/main.rs` | — |
| 4B.4 | ✅ preload fetch deduplication | `shell/src/main.rs` | — |
| 4B.5 | ✅ preload priority + EventSink | `shell/src/main.rs` | — |
| 4B.6 | ✅ feed_bytes in IncrementalTreeBuilder + raw byte chunks in shell | `html-parser/src/tree_builder.rs`, `shell/src/main.rs` | — |
| 5A | ✅ **`[P3]` HTTP/2** | Latency | Только network |
| 5A.1 | ✅ ALPN h2 negotiation | `network/src/lib.rs` | h2 → Err placeholder, http/1.1 fallback |
| 5A.2 | ✅ Frame codec (RFC 9113 §6) | `network/src/h2/frame.rs` | 10 типов + Unknown, padding strip, +46 тестов |
| 5A.3 | ✅ HPACK (RFC 7541) | `network/src/h2/hpack.rs` | static+dynamic table, Huffman, integer enc, +24 тестов (RFC C.3/C.4 vectors) |
| 5A.4 | ✅ Connection + concurrent streams | `network/src/h2/conn.rs` | preface, SETTINGS, fetch (single-stream), send_request + read_response_for_stream (concurrent), +16 тестов |
| 5A.5 | ✅ Pool multiplexing | `network/src/h2/pool.rs` | H2Pool acquire/release/evict, интеграция в fetch_single |
| 5A.6 | ✅ Flow control + WINDOW_UPDATE | `network/src/h2/conn.rs` | WINDOW_UPDATE после каждого DATA, +3 теста |
| 5A.7 | ✅ HTTP response cache (RFC 7234) | `network/src/http_cache.rs` | Cache-Control + ETag + If-None-Match + heuristic freshness, +19 тестов |
| 5B | ✅ **`[P3]` HTTP Range requests** | `<video>` seek | — |
| 5C | ✅ **`[P3]` DevTools/CDP минимум** | Debug движка | `crates/devtools/` | WS сервер + Browser.getVersion + DOM.getDocument stub + --devtools-port |
| 6+ | ⬜ **`[P3]` knowledge / Profiles / Focus / IME / WebSockets / SW / V8 / AI** (Phase 2-3) | — | — |
| 7A | ✅ **`[P3]` Tab UX** (§12.13, Phase 2) | Современная модель вкладок; 7A.1–7A.5 ✅ | `shell/src/tabs/` + `shell/src/panels/` |
| 7A.1 | ✅ Vertical tabs panel (toggle, drag-reorder, collapse) | `shell/src/panels/vertical_tabs.rs` | P2 done 2026-06-01 (p2-vertical-tabs): 200px left dock, Ctrl+B |
| 7A.2 | ✅ Tree-style tabs (parent-child) | `shell/src/tabs/tree.rs`, `shell/src/panels/tree_tabs.rs` | — |
| 7A.3 | ✅ Workspaces (изолированные группы) | `shell/src/panels/workspace_panel.rs` + `storage/src/workspaces.rs` | P2 done 2026-06-01 (p2-workspaces-ui): bottom switcher, Ctrl+Shift+W |
| 7A.4 | ✅ **`[P3+P2]` Split view** (2-4 viewport на окно) | `shell/src/panels/split_view.rs` + `paint` multi-viewport | P2 done 2026-06-01 (p2-split-view): Ctrl+\ toggle, Ctrl+M focus |
| 7A.5 | ✅ Tab auto-archive (UX-фича: убрать вкладки старше 12 ч из tab strip в @archive) | `shell/src/tabs/archive.rs` | P2 done 2026-06-03 (p2-tab-auto-archive): `TabArchive` + archive toolbar button (36 px) + drop-down panel + auto-archive in `tick_lifecycle`. |
| 7B | ✅ **`[P3]` Power-user input** (§12.14, Phase 2-3) | Keyboard-first аудитория; 7B.1–7B.5 ✅ | `shell/src/input/` |
| 7B.1 | ✅ Vim-style key bindings (modal) | `shell/src/input/vim.rs` | P1 done 2026-06-01 (p1-vim-keybindings): Normal/Insert, j/k/d/u/gg/G/yy/H/L, Ctrl+Alt+V |
| 7B.2 | ✅ **`[P3+P1]` Click-hint overlay** | `shell/src/hints.rs` + `lumen-layout::collect_clickable_elements` | P1: iterator ✅ (p1-click-hint-overlay); P3: vimium-style F-overlay ✅ (p3-click-hint-overlay) |
| 7B.3 | ✅ Mouse gestures | `shell/src/input/gesture.rs` | P1 done 2026-06-01 (p1-mouse-gesture): RMB drag L/R/U/D/LD/RD → Back/Forward/CloseTab/NewTab |
| 7B.4 | ✅ Custom omnibox aliases | `shell/src/omnibox/mod.rs` + `storage` `OmniboxAliases` | P1 done 2026-06-01 (p1-omnibox-aliases): !g/!gh bang-алиасы, @-команды |
| 7B.5 | ✅ **`[P3+P1]` Find-in-page с regex** | `shell/src/find.rs` + `lumen-layout::text_iter` | P1: `collect_visible_text` + `TextFragment` ✅ (p1-visible-text-iter); P3: Ctrl+R regex UI + highlight overlay ✅ (p3-find-in-page-regex) |
| 7C | ✅ **`[P3]` Privacy UX** (§12.15, Phase 2) | Встроенная защита; 7C.1–7C.4 ✅ | `lumen-network::filter` + `shell` |
| 7C.1 | ✅ Block list engine (EasyList + hosts files) | `network/src/filter/easylist.rs` + `hosts.rs` + `CompositeFilter` | P1 done 2026-05-31: 26 тестов |
| 7C.2 | ✅ Per-site permission UI panel | `shell/src/panels/permission_panel.rs` | P2 done 2026-06-01 (p2-permission-panel): Camera/Mic/Notif/Clipboard, Ctrl+Shift+P |
| 7C.3 | ✅ Cookie-banner auto-dismiss | `js/src/cookie_banner.rs` | P2 done 2026-06-01 (p2-cookie-banner-dismiss): 30+ EasyList consent-селекторов, MutationObserver, Ctrl+Shift+K |
| 7C.4 | ✅ Shields toolbar widget (счётчик блокировок) | `shell/src/panels/shields_panel.rs` | P2 done 2026-06-01 (p2-shields-panel): blocked-счётчик, Ctrl+Shift+S |
| 7D | 🟡 **`[P3]` Web platform baseline** (§12.16, Phase 2-3) | Современная авторизация и изоляция; 7D.2/7D.3 ✅, 7D.1 🟡 | `lumen-network` + `shell` |
| 7D.1 | 🟡 Passkeys / WebAuthn (CTAP2 client + navigator.credentials) — **`navigator.credentials` + software authenticator + CTAP2-over-USB protocol stack готовы** (trait `CredentialProvider`, `VirtualAuthenticator` ES256, `CtapRoamingTransport`, `CompositeCredentialProvider`; HID framing + CBOR + CTAP2 commands; Phase 1: platform HID enumeration); 15 тестов. | `network/src/ctap2.rs` + `network/src/webauthn.rs` | `CtapRoamingTransport` + `CompositeCredentialProvider` ✅ |
| 7D.2 | ✅ Tab containers (storage partitioning) | `shell/src/tabs/containers.rs` + `shell` | P2 done 2026-06-01 (p2-tab-containers): ContainerKind + ContainerStore (origin→store_id), цветной border-top |
| 7D.3 | ✅ Sidebar web panels (мини-страница в sidebar) | `shell/src/panels/sidebar_panel.rs` | P2 done 2026-06-01 (p2-sidebar-panel): 300px right dock, `sidebar:<url>`, Ctrl+Shift+A |
| 7E | 🟡 **`[P3]` DevTools полный** (§12.12, Phase 4+) | Поверх существующего CDP-минимума (5C); 7E.1/7E.3/7E.4/7E.5 ✅, 7E.2 🟡 (P4 API ✅, панель за P1) | `crates/devtools/` + `shell/src/devtools/` |
| 7E.1 | ✅ **`[P2]` DOM inspector panel** (hover box-model overlay + click computed style) | `shell/src/devtools/inspector.rs` + read из `lumen-dom` | P2: Ctrl+Shift+I toggle — 2026-06-02 |
| 7E.2 | 🟡 **`[P3+P4]` Computed styles panel** | сериализация `ComputedStyle` | P4: ✅ `computed_style_json` / `InProcessSession::computed_style_json` (2026-06-10); панель поверх API — за P1 |
| 7E.3 | ✅ **`[P3+P2]` Box model overlay** (margin/border/padding overlay) | через display list | P2: overlay primitive в `DisplayCommand` — 2026-05-29 |
| 7E.4 | ✅ **`[P2]` Network panel (live request log)** | `shell/src/devtools/network_panel.rs` слушает `EventSink` (RequestStarted/Completed/Blocked) | P2: Ctrl+Shift+E toggle, method/status/timing/URL — 2026-06-02 |
| 7E.5 | ✅ JS console (eval в контексте страницы) | `shell/src/devtools/console_panel.rs` + `QuickJsRuntime::console_messages` | P1 done 2026-06-02 (p1-devtools-console): F12 toggle, log/warn/error подсветка, cap 500 |
| 8 | ⬜ **`[P3]` Automation API** (§6.11, [ADR-006](docs/decisions/ADR-006-automation-api.md)) | First-class automation surface, фундамент собственных тестов и внешних клиентов | `lumen-driver`, `lumen-mcp-server`, `lumen-bidi-server` |
| 8A | ✅ **`[P3]` `lumen-driver` крейт + `BrowserSession` trait + `InProcessSession`** (Phase 0) | 8A.1–8A.6 ✅; 8A.7 ⬜ (Phase 4) | `crates/driver/` |
| 8A.1 | ✅ `BrowserSession` trait в `lumen-core::ext` + `NullBrowserSession` заглушка (object-safe, `Send`) | `core/src/ext.rs:1514` | 2026-05-29 |
| 8A.2 | ✅ `InProcessSession` impl | `driver/src/session.rs:53` | 2026-05-28 |
| 8A.3 | ✅ **`[P3+P2]` Off-screen рендер** (`Renderer::render_to_image() -> Image`) | `paint/src/renderer.rs` | P2/P3: `new_headless()` без winit + `render_to_image()` GPU readback — 2026-05-27 |
| 8A.4 | ✅ **`[P3+P1]` Structural getters**: `layout_box`, `computed_style` через handle / selector | `layout` exposers: `find_box_by_selector`, `computed_style_by_selector`, `ComputedStyleSnapshot` | 2026-05-27 |
| 8A.5 | ✅ Software rasterizer для тестов (`tiny-skia` opt-in под `cfg(test)`) | `paint/src/cpu_raster.rs` | детерминизм пикселей Windows/macOS/Linux CI — 2026-05-27 |
| 8A.6 | ✅ Миграция `graphic_tests/`: structural-assert Rust-тесты + `graphic_tests/snapshots/*.png` эталоны | `driver/tests/test_00..49.rs` + `snapshot_cpu.rs` (57 страниц) | все 57 html-страниц graphic_tests покрыты CPU-снапшотами; `cargo test -p lumen-driver --features cpu-render` — 2026-05-31 |
| 8A.7 | ⬜ Шелл переписать как первого клиента trait-а (winit/wgpu → один из транспортов) | `shell/src/main.rs` | — |
| 8B | ✅ **`[P1]` `lumen-mcp` крейт** (Phase 1) | AI-агенты (Claude/GPT/Browser Use) без обёрток | `crates/mcp/` |
| 8B.1 | ✅ JSON-RPC over stdio + TCP socket transport | `mcp/src/transport.rs` | P1 done 2026-05-31: StdioTransport + TcpTransport + VecTransport (tests) |
| 8B.2 | ✅ Resources: `screenshot`, `a11y_tree`, `layout`, `console`, `network` | `mcp/src/server.rs` | P1 done 2026-05-31 |
| 8B.3 | ✅ Tools: `click`, `type`, `scroll`, `navigate`, `wait`, `eval`, `query` | `mcp/src/server.rs` | P1 done 2026-05-31 |
| 8B.4 | ✅ `lumen --mcp` / `--mcp-port N` CLI flags | `shell/src/main.rs` | P1 done 2026-05-31: extract_mcp_mode + run_mcp_mode |
| 8C | ⬜ **`[P3+shell]` Native input injection** (Phase 1) | Не-distinguishable от пользователя input, для AI-агентов и тестов | `shell/src/input/native.rs` |
| 8C.1 | ⬜ Mouse/keyboard events идут в event loop тем же путём, что winit-события от ОС | `shell/src/main.rs` | НЕ через JS `dispatchEvent` |
| 8C.2 | ✅ `event.isTrusted = true` для native-injected events | `dom/src/lib.rs` | 2026-05-28 |
| 8D | ✅ **`[P1]` Auto-wait inside engine** (Phase 1) | Anti-flake, замена SDK retry-loops | `driver/src/session.rs` | P1 done 2026-05-31 |
| 8D.1 | ✅ `wait_for(Cond::Visible)` — border_box.width/height > 0; display:none → no layout box → false | layout-аware | P1 done 2026-05-31 |
| 8D.2 | ✅ `wait_for(Cond::NetworkIdle)` — active_network_requests counter; saturating_sub after HTTP fetch | network-аware | P1 done 2026-05-31 |
| 8D.3 | ✅ `wait_for(Cond::JsIdle)` — pending_js_microtasks counter; set_pending_js_tasks() for shell hook | shell runtime-аware | P1 done 2026-05-31 |
| 8E | ✅ **`[P1]` Per-context isolation by default** (Phase 1) | `OriginIsolationContext`: CookieJar + localStorage/sessionStorage + IDB per origin-group | `driver/src/isolation.rs` | P1 done 2026-06-01: `OriginGroup`+`OriginIsolationContext`, `InProcessSession::with_origin_isolation()`, 22 тестов |
| 8F | ✅ **`[P1]` Deterministic mode** (Phase 1) | `driver/src/determinism.rs`: `DeterministicConfig`, `seed_from_url()`, `ClockMode::Monotonic{step_ms}`. `BrowserSession::set_clock/set_rng_seed/freeze_fingerprint`. `lumen-js::freeze_fingerprint()` (audio analyser + font). Shell `--rng-seed`/`--monotonic-clock`. P1 done 2026-06-08 |
| 8F.1 | ✅ `set_clock(ClockMode::Frozen / Real / Monotonic)` | `ClockMode::Monotonic{step_ms}` в lumen-core; `SessionContext::set_clock_mode()`/`read_clock_ms()`. P1 done 2026-06-08 |
| 8F.2 | ✅ `set_rng_seed(u64)` — детерминированный `Math.random()` | JS runtime hook через `set_deterministic_mode()` + `context.rng_seed`. P1 done 2026-06-08 |
| 8F.3 | ✅ `freeze_fingerprint(profile)` — фиксированные canvas/WebGL/audio/font enumeration | `QuickJsRuntime::freeze_fingerprint()` JS shim: AnalyserNode + document.fonts overrides. P1 done 2026-06-08 |
| 8G | ✅ **`[P3+P1]` A11y tree first-class** (Phase 1, **зависит от P1 `lumen-a11y`**) | Semantic locator surface для tests + AI agents | `lumen-a11y` published interface. P1 done 2026-05-31: `AXRole::as_str()`, `A11yState`, enriched `A11yNode` (node_id/description/placeholder/state), `a11y_tree()` uses `build_ax_tree()`, 14 тестов |
| 8G.1 | ✅ A11y tree доступна через `BrowserSession::a11y_tree()` | `driver/src/session.rs` uses `lumen_a11y::build_ax_tree()` | P1 done 2026-05-31 |
| 8G.2 | ✅ `Query::Role { role, name }` matching по a11y-tree (Playwright-стиль `getByRole`) | `driver/src/session.rs` `find_a11y_node`/`find_all_a11y_nodes` + `matches_query` | P1 done 2026-05-31 |
| 8H | 🟡 **`[P3]` `lumen-bidi-server` крейт** (Phase 2) | Playwright/Selenium 5/Cypress «из коробки» | `crates/bidi/` |
| 8H.1 | 🟡 WebSocket transport + W3C BiDi handshake | shell stub `shell/src/bidi/` (WS-кодек переиспользует `lumen-devtools::ws`); вынос в `bidi/src/transport.rs` отложен | — |
| 8H.2 | 🟡 BiDi modules core: `session`, `browsingContext`, `script`, `network`, `input` | `session.*` (status/new/subscribe/unsubscribe/end — реальное хранение подписок + event-gating), `browsingContext.*` (create/close/navigate/activate/getTree, multi-context state-machine, каскадное закрытие, вложенный getTree) в `shell/src/bidi/protocol.rs` (27 unit-тестов); `script`/`network`/`input` отложены | W3C Working Draft, May 2026 |
| 8H.3 | ⬜ **Ship BiDi gaps** (см. ADR-006): response body, locale/timezone/offline, per-context UA, viewport-before-popup, preload per-context, download lifecycle, cookie change events, per-origin clear | `bidi/src/extensions.rs` | gap-mapping в `subsystems/lumen-bidi-server.md` |
| 8H.4 | ✅ `lumen --bidi-port N` CLI flag | `shell/src/main.rs` (`extract_bidi_port` + `bidi::spawn`) | — |
| 8I | ⬜ **`[P3]` `lumen-cdp-shim` крейт** (Phase 3+, **opt-in, по реальному запросу**) | Legacy Puppeteer-совместимость | `crates/cdp-shim/` |
| 9 | 🟡 **`[P1]` Anti-detection privacy stack** (§9.5, [ADR-007](docs/decisions/ADR-007-anti-detection-stack.md)) | Privacy by default; устойчивость к Cloudflare/DataDome/Akamai false-positive. 9A ✅ Layer 1 (P1 2026-05-31); 9B ✅ TLS fingerprint (P1 2026-05-31); 9C ✅ HTTP fingerprint (P1 2026-05-31); 9D ✅ rendering fingerprint (P1 2026-06-02); 9E ✅ behavioral mimicry (P1 2026-06-01); остаток — 9F.3 Tor circuit | `lumen-network`, `lumen-js`, `lumen-shell`, `lumen-paint` (минимально), `lumen-canvas` |
| 9A | ✅ **`[P1]` Layer 1: surface API без automation-маркеров** (Phase 1) | navigator.webdriver отсутствует; нет chrome.runtime/cdc_*/__playwright/etc.; event.isTrusted=true для native input; nav.appName/vendor/product/plugins/mimeTypes совместимы с Chrome | `lumen-js/src/surface_api.rs` P1 done 2026-05-31 |
| 9A.1 | ✅ Audit JS bindings + `install_surface_api_protection` (hardening shim) | `js/src/surface_api.rs` (11 unit) + `js/tests/no_automation_markers.rs` (19 runtime) | — |
| 9A.2 | ✅ Negative tests: `webdriver` absent, no automation globals, isTrusted, standard browser props | `js/tests/no_automation_markers.rs` (19 тестов); source audit — `driver/tests/antidetect_surface_api.rs` (7 тестов) | — |
| 9B | ✅ **`[P1]` Layer 2: TLS fingerprint Chrome-matching** (Phase 1) | JA3/JA4 как у current stable Chrome; per-profile override | `lumen-network` rustls config (P1 done 2026-05-31) |
| 9B.1 | ✅ Cipher suite ordering matching Chrome | `network/src/tls/fingerprint.rs` (Chrome 130 AEAD order) | — |
| 9B.2 | ✅ Extension list + supported groups matching Chrome | `network/src/tls/fingerprint.rs` (kx X25519→secp256r1→secp384r1) | — |
| 9B.3 | ✅ ALPN order `h2`, `http/1.1` matching Chrome | `network/src/tls/mod.rs` `build_client_config()` | — |
| 9B.4 | ✅ JA3/JA4 snapshot test против известных Chrome JA3 | `network/tests/tls_integration.rs` (CHROME_130_JA3/JA4 snapshot) | обновляется per Chrome major release |
| 9B.5 | ✅ Per-profile TLS config (Standard / Strict / Tor) | `network/src/tls/mod.rs` `TlsProfile` enum | — |
| 9C | ✅ **`[P1]` Layer 3: HTTP fingerprint Chrome-matching** (Phase 1) | Header order + casing + HTTP/2 SETTINGS как у Chrome | `lumen-network` http/h2 P1 done 2026-05-31 |
| 9C.1 | ✅ HTTP/1.1 header order + casing matching Chrome | `network/src/http/headers.rs` + wired into `write_request()` | — |
| 9C.2 | ✅ HTTP/2 SETTINGS frame values matching Chrome | `network/src/h2/conn.rs` `connect_with_profile()` | — |
| 9C.3 | ✅ HTTP/2 stream priority pattern matching Chrome | `network/src/http/h2_settings.rs` `H2StreamPriority` | — |
| 9C.4 | ✅ Accept-Language default `en-US,en;q=0.9` (не палит реальную локаль) | `network/src/http/mod.rs` `DEFAULT_ACCEPT_LANGUAGE` | — |
| 9C.5 | ✅ Client Hints handling (опционально, выключено на Strict) | `network/src/http/client_hints.rs` | — |
| 9D | ✅ **`[P1+P2]` Layer 4: rendering fingerprint** (Phase 2) | Canvas/WebGL/audio randomization, Battery API disable, WebRTC mDNS-only | `lumen-canvas`, `lumen-paint`, `lumen-js` |
| 9D.1 | ✅ Canvas randomization (Brave-style per-session seed) | `canvas/src/fp_noise.rs` | — |
| 9D.2 | ✅ WebGL renderer/vendor normalization | `js/src/webgl_canvas.rs` | GpuFingerprint normalization (paint/fingerprint.rs) wired into the functional WebGL context: `getParameter(UNMASKED_VENDOR/RENDERER_WEBGL)` + `getParameter(VENDOR/RENDERER)` return normalized strings; `toDataURL`/`toBlob` blank. 2026-06-02 |
| 9D.3 | ✅ AudioContext fingerprint noise | `js/src/audio_bindings.rs` | 2026-05-30 |
| 9D.4 | ✅ Battery API disable on Strict | `js/src/battery_bindings.rs` | 2026-05-30: navigator.getBattery() → rejected Promise, 4 unit-тестов |
| 9D.5 | ✅ WebRTC mDNS-only host candidates | `js/src/webrtc_stub.rs` | 2026-06-01: `RTCPeerConnection` фаерит один UUID.local mDNS-кандидат (без утечки реального IP), 17 тестов |
| 9D.6 | ✅ Hardware concurrency / screen / timezone normalization per profile | `js/src/navigator_bindings.rs` | 2026-05-30: hardwareConcurrency=2, deviceMemory=8, platform=Win32, screen 1920×1080, getTimezoneOffset→0, 10 unit-тестов |
| 9E | ✅ **`[P1]` Layer 5: behavioral mimicry (opt-in)** (Phase 1, **для automation API**) | `InputMode::HumanLike` для тестировщиков | `shell/src/input/humanlike.rs` (P1 done 2026-06-01) |
| 9E.1 | ✅ Bézier-curve mouse paths between coordinates | `shell/src/input/humanlike.rs` (кубик Безье, 2 контр. точки) | — |
| 9E.2 | ✅ Variable inter-keystroke timing (Gaussian) | `shell/src/input/humanlike.rs` (Box-Muller) | — |
| 9E.3 | ✅ Pre-click dwell time | `shell/src/input/humanlike.rs` | — |
| 9F | ✅ **`[P1]` Layer 6: профили Standard/Strict/Tor** (Phase 2) | Per-profile config + per-context override через BrowserSession | `shell/src/config.rs` + `driver` (9F.1/9F.2/9F.3 ✅) |
| 9F.1 | ✅ Профильный конфиг fingerprint (объединяет слои 2/3/4): `fingerprint.toml` → `FingerprintProfile` (http+tls profile, navigator/screen/timezone), apply к `HttpClient` + process-global `NavigatorProfile` | `shell/src/config.rs` | — |
| 9F.2 | ✅ `BrowserSession::set_fingerprint_profile(profile)` per-context override | `driver` + `core::ext` | 2026-06-02: `FingerprintProfile::to_http_profile()` (Standard→Chrome / Strict→Strict / Tor→TorBrowser); `InProcessSession::build_http_client()` + winit_session применяют профиль (HTTP header order + derived TLS) к исходящим запросам |
| 9F.3 | ✅ Tor-mode профиль полный: SOCKS5 circuit (RFC 1928 клиент в `crates/network/src/socks5.rs`); auto-wiring к 127.0.0.1:9050 при `http_profile=TorBrowser`; screen/platform/language pinning (1000×900, Win32, en-US) в `navigator_profile()`; `no_persistent_state` поле; `effective_socks5_proxy()` API; `with_socks5_proxy()` в HttpClient. 2026-06-08 lumen-network + lumen-shell. | `network` + `shell` | — |
| 9G | ✅ **Red lines + perf gate — code-level enforcement** | Чтобы trigger-PR случайно не нарушил ADR-006 / ADR-007. 9G.1–9G.5 ✅ | — |
| 9G.1 | ✅ CI lint: запрет имён `*captcha*`, `*solver*`, `*ip_rotation*`, `*proxy_pool*` в crate-names | `scripts/check_crate_names.py` + `.github/workflows/red-lines.yml` | 2026-06-02: token-matcher (`resolver` не ложно-срабатывает), self-test, скан всех Cargo.toml |
| 9G.2 | ✅ README / маркетинговые тексты не используют «scraping», «stealth», «bypass» — линтер в CI | `scripts/check_marketing_words.py` + `.github/workflows/red-lines.yml` | 2026-06-02: whole-word matcher, scope = README.md (техдоки/ADR легитимно обсуждают термины), self-test |
| 9G.3 | ✅ **CI bench gate**: `cargo run -p lumen-bench --release` + сравнение с `bench/baseline.json` (median, p95) → fail PR при регрессе >5% в default-сборке. Применяется к PR, затрагивающим `lumen-driver` / `lumen-mcp-server` / `lumen-bidi-server` / `lumen-network` / `lumen-canvas` / `lumen-js` / `lumen-storage::profiles` / `lumen-shell::input` | `.github/workflows/bench-gate.yml` + `bench/baseline.json` + `bench/compare.py` (все есть) | binding по ADR-006 §«Performance gate» и ADR-007 §«Performance gate» |
| 9G.4 | ✅ **`bench/baseline.json` обновление-процедура**: разработчик руками `cargo run -p lumen-bench --release` затем обновляет JSON; коммит документирует архитектурное обоснование | `bench/UPDATE.md` | — |
| 9G.5 | ✅ **`lumen-bench` RAM-axis расширение** (требование [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)): добавить замеры `peak_rss`, `steady_state_rss` к существующим time-метрикам; get_rss_bytes() cross-platform (libc::getrusage на Unix, GetProcessMemoryInfo на Windows); baseline.json + UPDATE.md документация | `bench/src/main.rs` + `baseline.json` + `UPDATE.md` | binding по ADR-008 §«Performance gate» |
| 10 | ⬜ **`[P3]` Tab lifecycle: пятитайерная модель** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)) | Главный продуктовый дифференциатор по RAM: 50 вкладок ~400 MB vs Chrome 6-10 GB | `lumen-shell::tab_lifecycle`, `lumen-storage::tab_snapshot`, `lumen-core::ext::MemoryPressureSource` |
| 10A | ⬜ **`[P3]` `TabState` enum + state machine T0-T4** (Phase 1) | Базовая модель + transition triggers | `shell/src/tab_lifecycle/state.rs` |
| 10A.1 | ⬜ `enum TabState { Active, BackgroundRecent, BackgroundOld, Hibernated }` + transitions | `shell/src/tab_lifecycle/state.rs` | — |
| 10A.2 | ⬜ OR-of-conditions trigger: idle timeout + memory pressure + LRU within budget | `shell/src/tab_lifecycle/trigger.rs` | — |
| 10A.3 | ⬜ Per-user конфигурация таймаутов (5 мин / 30 мин / pinned-never) | `storage/src/profiles/tabs.rs` | — |
| 10B | ✅ **`[P3+P1]` Invariant 1: DOM arena serialization** | `NodeId(u32)` без `Rc<RefCell>`, `Document::to_bytes`/`from_bytes` via bincode | ADR-008 ✅ 2026-05-27 |
| 10B.1 | ✅ Audit `lumen-dom` — no `Rc<RefCell<Node>>`, всё на `NodeId(u32)` | `dom/src/lib.rs` | ADR-008 Invariant 1 ✅ |
| 10B.2 | ✅ `Document::to_bytes` / `Document::from_bytes` via bincode | `dom/src/lib.rs` | — |
| 10B.3 | ✅ Anti-pattern guard в `lumen-dom` | `dom/src/lib.rs` | — |
| 10C | ✅ **`[P3]` Invariant 2: JsRuntime suspend/resume API** | `pause/unpause/suspend/resume` в `JsRuntime` trait + QuickJS impl | ADR-008 ✅ 2026-05-27 |
| 10C.1 | ✅ `JsRuntime` trait: `pause()` / `unpause()` / `suspend()` / `resume()` | `core/src/ext/js.rs` | — |
| 10C.2 | 🟡 Имплементация для `rquickjs`: `pause`/`suspend`/`resume` есть; полная сериализация heap через `JS_WriteObject`/`JS_ReadObject` заблокирована native-function bindings (их `JS_ReadObject` не восстанавливает). Шелл re-runs inline scripts на restore (`restore_js_context`), heap-контент не потребляется. | `js/src/lib.rs` | — |
| 10C.3 | ✅ deflate-сжатие heap snapshot + cap 5 MB/tab (P1 2026-06-02): `heap_snapshot::compress_heap`/`decompress_heap`, magic `LJH1`, переиспользует vendored flate2; wired в `QuickJsRuntime::suspend`/`resume` | `js/src/heap_snapshot.rs` | — |
| 10C.4 | ⬜ V8 compatibility note при миграции в Phase 3 | `docs/decisions/` | check before Phase 3 |
| 10D | ✅ **`[P3+P1+P2]` Invariant 3: pure layout + paint** | 10D.1+10D.2+10D.3 ✅ | `lumen-layout` + `lumen-paint` |
| 10D.1 | ✅ Audit `lumen-layout` — no `static MUT` / `lazy_static` / `OnceCell` в hot path | `layout/src/lib.rs` | ADR-008 Invariant 3 ✅ 2026-05-27 |
| 10D.2 | ✅ Audit `lumen-paint::display_list` — pure-function requirement met | `paint/src/display_list.rs` | ADR-008 Invariant 3 ✅ 2026-05-27 |
| 10D.3 | ✅ Cross-tab caches — `EvictableCache` trait + `CacheRegistry` в `lumen-core::ext`; `GlyphAtlas`/`ImageDecodeCache`/`LayerCache` impl; 8 тестов | `core/src/ext.rs` + `paint/src/atlas.rs` + `paint/src/layer_cache.rs` + `image/src/decode_cache.rs` | 2026-05-30 |
| 10E | 🟡 **`[P3]` T0 экономия: image decode cache LRU + viewport-gating** (Phase 1, **главный источник экономии**) | 10E.1+10E.2 ✅; 10E.4 ⬜ | `image/src/decode_cache.rs` |
| 10E.1 | ✅ `ImageHandle` (`Arc<Image>`) + `ImageKey` — тонкий ref, экспортирован из `lumen-image` | `image/src/decode_cache.rs` | 2026-05-30 |
| 10E.2 | ✅ `ImageDecodeCache` с LRU + memory budget (256 MB default), 9 unit-тестов | `image/src/decode_cache.rs` | 2026-05-30 |
| 10E.3 | ✅ Viewport-gating в layout: `gate_image_requests(root, viewport, scroll_x, scroll_y)` — AABB intersection, HashSet<NodeId>, 7 тестов | `layout/src/image_gating.rs` | 2026-05-29 |
| 10E.4 | ⬜ Scroll-discard: при удалении от viewport на >3 экрана — handle освобождается | `shell/src/scroll/decode_gating.rs` | — |
| 10F | ✅ **`[P1+P3]` T0 экономия: GPU layer LRU + texture recycling** | `LayerCache` + texture pool — 7+2 тестов | `paint/src/layer_cache.rs` |
| 10F.1 | ✅ `LayerCache` с LRU + GPU memory budget | `paint/src/layer_cache.rs` | — |
| 10F.2 | ✅ Texture pool recycling (одна `wgpu::Texture` переиспользуется для разных layers) | `paint/src/texture_pool.rs` | P3 lifecycle integration pending |
| 10G | ✅ **`[P1+P2+P3]` T0 экономия: glyph atlas LRU eviction** | LRU eviction в `paint/src/atlas.rs` (`get_lru_candidates`, `remove_keys`) — 4 теста | `paint/src/atlas.rs` |
| 10H | ✅ **`[P2]` `MemoryPressureSource` trait** (Phase 1) | OS-сигналы памяти управляют tier-переходами | `core/src/ext.rs` + `core/src/memory_pressure.rs` |
| 10H.1 | ✅ Trait + `enum MemoryPressureLevel { Low, Medium, High }` + `NullMemoryPressureSource` | `core/src/ext.rs` (ADR-008); 3 unit-тесты | — |
| 10H.2 | ✅ Win32 impl: `Win32MemoryPressureSource` — `GlobalMemoryStatusEx` polling | `core/src/memory_pressure.rs`; 1 unit-тест (live Windows call) | — |
| 10H.3 | ✅ Linux impl: `LinuxMemoryPressureSource` — `/proc/pressure/memory` PSI polling | `core/src/memory_pressure.rs`; 3 unit-тесты | — |
| 10H.4 | ✅ macOS impl: `MacosMemoryPressureSource` — `host_statistics64(HOST_VM_INFO64)` polling | `core/src/memory_pressure.rs`; 4 unit-тесты (threshold logic) | — |
| 10H.5 | ✅ Подписка кэшей на pressure events: `on_memory_pressure(level)` | `ImageDecodeCache` (4 тесты) + `GlyphAtlas` (3 тесты) + `LayerCache` (3 тесты) | — |
| 10I | ⬜ **`[P3]` T2 → SQLite JS heap snapshot persistence** (Phase 2) | JS heap на диск при T2; restore при T0 | `storage/src/tab_snapshot.rs` |
| 10I.1 | ⬜ Schema: `tab_snapshots(tab_id, js_heap_blob, dom_blob, scroll, form_state, ts)` | `storage/migrations/0NN_tab_snapshots.sql` | — |
| 10I.2 | ⬜ Async-save при T1→T2 (без блокирования UI) | `shell/src/tab_lifecycle/persist.rs` | — |
| 10I.3 | ⬜ Async-load при T2→T0 (с indeterminate UI hint если > 100 ms) | `shell/src/tab_lifecycle/restore.rs` | — |
| 10J | ✅ **`[P1]` T3 hibernation: full DOM serialization** (Phase 2) | DOM в SQLite, в RAM только TabMetadata; bincode → deflate-сжатие blob | `storage/src/tab_snapshot.rs` |
| 10J.1 | ✅ DOM arena → bincode → deflate → SQLite blob | `storage/src/tab_snapshot.rs` (`compress_blob`/`decompress_blob`, magic `LZD1`, прозрачно в store/fetch; flate2/miniz_oxide, 3-5× shrink) | p1-hibernate-compress |
| 10J.2 | ✅ `TabMetadata { url, title, scroll, favicon }` остаётся в RAM | `shell/src/tab_lifecycle/restore.rs` (`TabMetadata`), scroll в SQLite | p1-session-persist |
| 10J.3 | ✅ Restore: deserialize → re-run scripts → full layout+paint + new `PersistentJs` | `shell/src/tab_lifecycle/hibernate.rs` (`restore_js_context`) + `restore_hibernated_tab` | p1-tab-auto-archive |
| 10K | ✅ **`[P3]` UI affordance: индикация tier'а в tab strip** (Phase 2) | Пользователь видит, что вкладка спит | `shell/src/tabs/strip.rs` |
| 10K.1 | ✅ Иконка "Z" / fade-opacity на T2/T3 tabs | `shell/src/tabs/strip.rs` (BADGE_HIBERNATE_COLOR/BADGE_SLEEP_COLOR, TAB_T2_BG/TAB_T3_BG) | p1-o12-restore-spinner |
| 10K.2 | ✅ Tooltip "Вкладка спит — клик восстановит за ~1 сек" с показом tier'а | `shell/src/tabs/strip.rs` (`build_tab_tooltip`) | p1-o12-restore-spinner |
| 10K.3 | ✅ Loading-spinner при restore > 200 ms | `shell/src/panels/restore_spinner.rs` (`build_spinner`, THRESHOLD_MS=200) | p1-o12-restore-spinner |
| 10L | ✅ **`[P1]` JS heap GC tuning per tier** (Phase 2) | Активная — мягкий GC, idle — агрессивный | `js/src/gc_policy.rs` (`GcLevel`; soft/moderate/aggressive) |
| 10M | ✅ **`[P2]` `samples/heavy.html`** — Habr-style тестовая страница для бенчей T0-heavy | `samples/heavy.html` | используется в `lumen-bench` |

---

### Точки координации (минимум, явно)

| Когда | Кто с кем | Что координируется |
|---|---|---|
| После Sprint 0 | Все трое | Зафиксированы контракты — больше совместного PR нет до точек ниже. |
| После P1 п.2A | P1 → P2 | P1 публикует «stacking impl готов» в commit-body; P2 валидирует, что painting order на реальных данных корректен (visual sample). Один день. |
| После P1 п.2B | P1 → P2 | Property trees: P2 включает их в compositor commit. Один день. |
| После P1 п.3A + P2 п.3B | P1 ↔ P2 | Web Animations: stub-interpolator меняем на реальный. Один день. |
| После P1 п.3B + P3 п.4B | P1 ↔ P3 | Streaming pipeline: P3 переключает shell с blocking на push-tokenizer. Совместный merge `streaming-go-live`. |
| После P3 п.4A | P3 ↔ P1 | DOM bindings: добавляем wrapper hooks в `lumen-dom` (P1 ревьюит). |

Все остальные стыковки — drop-in (пустой stub → реальный impl, потребитель ничего не правит).

---

### Crate ownership matrix (защита от merge-конфликтов)

| Крейт / модуль | Единственный владелец | Дополняют (PR с согласованием) |
|---|---|---|
| `lumen-html-parser`, `lumen-css-parser`, `lumen-dom`, `lumen-layout`, `lumen-encoding` | P1 | P3 — wrapper hooks для GC (по согласованию) |
| `lumen-paint`, `lumen-font`, `lumen-image` | P2 | — |
| `lumen-network`, `lumen-storage`, `lumen-knowledge`, `lumen-shell`, `lumen-js`, `lumen-ai` | P3 | — |
| `lumen-driver`, `lumen-mcp-server`, `lumen-bidi-server`, `lumen-cdp-shim` (когда появится) | P3 | P1 — accessor expose из `lumen-layout` / `lumen-a11y` (по согласованию); P2 — `Renderer::render_to_image()` off-screen path (по согласованию) |
| `lumen-a11y` | P1 | P3 — читает snapshot для `BrowserSession::a11y_tree()` |
| `lumen-core::ext` | P3 | P1/P2 — добавляют trait если им нужно (sole-author commit, post-factum review) |
| `lumen-paint::display_list` (`DisplayCommand` enum) | P2 | P1 — добавляет варианты для новых layout-фич (например, для Grid) — P2 ревьюит |
| `samples/page.html`, snapshot tests | Кто меняет — тот и трогает | — |
| `graphic_tests/snapshots/*.png` (Уровень 3 эталоны, §15) | Кто реализует свойство — тот и обновляет (через `cargo test --update-snapshots`) | — |
| `lumen-shell::tab_lifecycle`, `lumen-storage::tab_snapshot`, `lumen-bench::memory` + `tier_transitions` | P3 (трек 10, ADR-008) | P1 — DOM arena serialization (10B); P2 — pure paint audit (10D.2), GPU layer cache (10F); P1+P2 — glyph atlas eviction (10G) |
| `lumen-core::ext::MemoryPressureSource` (новый trait-anchor, ADR-008 task 10H) | P3 | OS-specific impls: win32/linux/macos в одном крейте |

**Главное правило:** новый PR трогает крейты **только одного владельца**, кроме редко согласованных кросс-крейтных стыковок выше.

---

### Provisional крейты к включению по приоритету

| Крейт | Программист | Когда брать | Trait-anchor (Sprint 0) |
|---|---|---|---|
| ✅ `brotli-decompressor` | P3 (готово) | Подключён через `BrotliContentDecoder` в `lumen-network` (merge network-brotli, 2026-05-15) | расширение `ContentDecoder` |
| ✅ `psl` (выбран вместо `publicsuffix` — compiled-in) | P3 (готово) | Подключён через `PslProvider` в `lumen-storage` (merge network-publicsuffix, 2026-05-15) | `PublicSuffixList` |
| `idna` (полный UTS#46) | P3 (по факту edge case) | Когда найдём реальный кейс, не покрытый Punycode | `IdnaProvider` |
| `icu4x.segmenter` + `icu4x.linebreak` | P1 п.5 | После CSS типизации (1B) | `UnicodeProvider` |
| `ruzstd` / `zstd-safe` | P3 (после brotli) | После Brotli — дёшево добавить | расширение `ContentDecoder` |
| `image-webp`, `zune-jpeg` | P2 (опционально) | Если P2 не загружен Phase 2 работой | `ImageDecoder` |
| `woff2` | P2 (Phase 2) | При WebFonts | `FontFormat` |
| `hyphenation` | P1 (Phase 2-3) | При типографике | `HyphenationProvider` |
| `hunspell-rs` / `spellbook` | P3 (Phase 3) | При spell-check | `SpellChecker` |
| `quinn` (HTTP/3) | P3 (никогда в обозримом) | — | расширение `NetworkTransport` |
| `tiny-skia` (CPU rasterizer, **только под `cfg(test)`**) | P2 (Phase 0, opt-in) | Для in-process pixel snapshot tests (§15 уровень 3) — детерминизм между Windows/macOS/Linux CI. Не попадает в release-бинарь. | расширение `Renderer` (off-screen CPU path) |

---

### Browser fundamentals (справочный список, с учётом новой раскладки)

Список упущений, обнаруженных при сравнении с реальными движками 2026-05-14. Все маркеры обновлены под P1-P3. Каждый пункт **критичен для функциональности** при достижении соответствующей фазы — без них браузер не работает как браузер, а не Phase-0-демо.

#### Phase 1 (Reader) — добавить к существующему scope

- **`[P3]` HTML event loop + microtasks + rendering steps + observers.** 🟡 **Framework + integration в winit-loop + task source priorities + requestIdleCallback + `run_idle_callbacks` в about_to_wait готовы** (`lumen-shell::runtime` + Lumen-handlers — см. [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-shell`). about_to_wait → step()×N (cap 256) → run_idle_callbacks(`IDLE_BUDGET_MS=10.0` если дошли до Idle, 0 ms иначе), Resized → deliver_observer_records(Resize), RedrawRequested → run_rendering_step(timestamp_ms) перед render(); `TaskQueue` обходит `TaskSource::PRIORITY_ORDER` на pop. **`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` + `scheduler.postTask`/`yield` реализованы как async JS timer queue**: `_lumen_tick_timers()` дрейнируется в about_to_wait, `_lumen_request_wakeup(deadline_ms)` уведомляет shell о следующем таймере → `ControlFlow::WaitUntil` (2026-05-25). **Layout invalidation after JS DOM mutations**: `dom_dirty: Arc<AtomicBool>` в `QuickJsRuntime` — проставляется всеми мутирующими биндингами (`set_attr`/`set_text_content`/`set_inner_html`/`append_child`/`remove_child`/`remove_attr`); `RedrawRequested` шаг 6 вызывает `relayout()` если флаг взведён (2026-05-25). **`MutationObserver` / `ResizeObserver` / `IntersectionObserver` + `getBoundingClientRect` готовы** (2026-05-26): JS-классы в `crates/js/src/dom.rs`, `_lumen_get_bounding_rect` / `_lumen_get_viewport_size` Rust-биндинги через `Arc<Mutex<HashMap<u32,[f32;4]>>>`, deliver вызывается из shell после каждого `relayout_page`. 200 тестов lumen-js. **Осталось:** правильный ordering rendering steps stage (style → layout → paint как cascade), PerformanceObserver.
- **`[P1+P2]` Stacking contexts + правильный CSS Painting Order** (CSS 2.1 Appendix E). 7-уровневый порядок paint (background → border → block descendants → floats → non-positioned inline → positioned по z-index, рекурсивно). **P1** — модель stacking-ов и order computation в layout; **P2** — paint-side traversal в правильном порядке.
- **`[P2+P1]` Compositor thread + property trees.** Отдельные TransformTree / ScrollTree / EffectTree / ClipTree, копируются на compositor thread. Двухбуферная commit-модель. Off-main-thread scroll. **P2** — compositor pipeline / GPU primitives / layer tree; **P1 — готово (`PropertyTrees::build`, `Mat4` builders + multiply, ветка `property-trees-build`)** — построение четырёх деревьев из style/layout с учётом transform-origin.
- ✅ **`[P2]` Stacking-aware hit testing.** Реализовано в `lumen-paint::hit_test` (см. § «P2 2B» и [SUBSYSTEMS.md](SUBSYSTEMS.md) → `lumen-paint`).
- ✅ **`[P1]` Quirks mode vs standards mode — application в layout/cascade.** Полностью: table UA-reset, line-height replaced, unitless length, hashless hex color, table cell width hint, §3.5 html `height:100vh` (viewport basis). 2026-05-24.
- 🟡 **`[P3]` Same-Origin Policy enforcement + CORS preflight.** `Origin` tuple реализован (`lumen-network::Origin`, HTML LS §7.5 — scheme/host/port + same_origin + is_potentially_trustworthy). **CORS preflight enforcement реализован полностью** (`HttpClient::with_cors_cache` + `fetch_cors(CorsRequest)` — OPTIONS preflight + cache + actual-response validation per hop, ветка `cors-preflight-enforcement`). SOP checks при postMessage / storage / cookies — следующая задача (применить classifier в shell). Phase 0 ограничения CORS: request body для POST/PUT (HttpClient GET-only), cookie-jar integration.
- 🟡 **`[P3]` Mixed-content blocking + `<iframe sandbox>`.** Classifier-ы реализованы (`lumen-network::classify_subresource_request` для blockable/optionally + `SandboxFlags`/`parse_sandbox_value` для всех 14 keyword-ов). Остаётся: enforcement в HttpClient (блочить blockable до TCP) + DOM-применение sandbox в shell.
- **`[P1+P3]` Preload scanner.** ✅ **Готово полностью.** P1: `lumen_html_parser::preload_scanner::scan_preload_hints`. P3: интегрирован в shell streaming pipeline — `LoadEvent::EarlyPreloadHints` эмитируется из background-потока ДО первого `HtmlChunk` (скан первых STREAM_CHUNK_BYTES байт, обычно весь `<head>`). `dispatch_preload_hints` принимает `seen: &mut HashSet<String>` для cross-call дедупликации. Phase 0: prefetch блокирующий; параллельный fetch — будущая задача.

#### RenderBackend abstraction (ADR-010) — Phase 2–3

> Full design — [ADR-010](docs/decisions/ADR-010-render-backend-abstraction.md) · Implementation state — [subsystems/paint.md](subsystems/paint.md) «Planned: RenderBackend abstraction»

| # | Задача | Владелец | Статус |
|---|---|---|---|
| RB-1 | `RenderBackend` trait + `RenderError` в `paint::backend` | P2 | ⬜ |
| RB-2 | `WgpuBackend` — обёртка над текущим `Renderer` | P2 | ⬜ |
| RB-3 | Feature-флаги в `lumen-paint/Cargo.toml` | P2 | ⬜ |
| RB-4 | Shell → `Box<dyn RenderBackend>` + `LUMEN_BACKEND` env var | P2 | ⬜ |
| RB-5 | `FemtovgBackend` скелет + базовые команды (`FillRect`, `FillRoundedRect`, `DrawText`, `DrawBorder`, `PushClipRect`) | P2 | ⬜ |
| RB-6 | `FemtovgBackend` полный (все ~30 `DisplayCommand` вариантов) | P2 | ⬜ |
| RB-7 | `VelloBackend` заглушка (компилируется, логирует, ничего не рисует) | P2 | ⬜ |
| RB-8 | `CompareBackend` + тест-раннер в `lumen-driver` (pixel diff двух бэкендов) | P2+P3 | ⬜ |
| RB-9 | `FemtovgBackend` → default; `WgpuBackend` → fallback | P2 | ⬜ |
| RB-10 | `VelloBackend` полный (когда vello API стабилизируется) | P2 | ⬜ Phase 3+ |

**Принцип изоляции vello:** все `vello::*` импорты только в `backends/vello_backend.rs`. При обновлении vello API — правим только этот файл. Остальной код (trait, shell, layout) не знает о vello.

**Параллельное использование:** `CompareBackend` рендерит одну страницу двумя бэкендами и считает pixel diff. Используется для валидации нового бэкенда перед промоутингом в default. Запуск: `cargo test -p lumen-driver --features compare-femtovg-vello`.

**Путь миграции:** wgpu (сейчас) → femtovg (Phase 2 default) → vello (Phase 3 default, когда API стабильно). Каждый предыдущий бэкенд остаётся как fallback.

#### Phase 2 (Interactive) — без этого современный веб не функционален

- 🟡 **`[P1+P3]` Shadow DOM + custom elements + `<template>` + `<slot>`.** **P1 done** — `ShadowRootMode`, `NodeData::ShadowRoot`, `Document::attach_shadow/shadow_root_of/is_shadow_host`, `FlatTree` + `build_flat_tree` (slot assignment, fallback content, zero-alloc fast path), layout wiring (`build_box`/`collect_inline_segments` use `flat.children_of`). **P3** — JS bindings (`Element.attachShadow`, `customElements.define`) + lifecycle dispatch. **P4** — `:host`, `::slotted` CSS pseudo-classes (marked with `// CSS: :host, ::slotted` in `build_box`).
- **`[P1+P3]` Accessibility tree + platform bridges.** **P1** — построение accessibility tree из DOM/layout + ARIA semantics + focus model; **P3** — platform bridges (UIA / AT-SPI / NSAccessibility) + focus dispatch.
- 🟡 **`[P1+P3]` Forms runtime.** **P1 done** — `ValidityState` + все флаги HTML5 §4.10.21.1 в `lumen-dom::element_validity`, `check_validity_form`/`invalid_controls_in_form`, `build_form_submit` блокирует submit при невалидных контролах. Validation pseudo-classes (`:valid`/`:invalid`/`:required` и т.д.) готовы ранее. **P3** — native pickers, autofill popup, validation tooltip UI.
- ✅ **`[P1+P2]` `<picture>` / `srcset` / `sizes` + `loading="lazy"`.** **P1**: готовы parser + pickers + `pick_picture_source` + L4 nested + IntersectionObserver event source для lazy (rootMargin 1-viewport-ahead, через `_lazy_io` внутри `_lumen_init_lazy_images`). **P2** — image-side GPU upload + integration в shell.
- **`[P3]` IME composition events** (`compositionstart` / `update` / `end`, `KeyboardEvent.isComposing`). Интегрируется через winit IME API + DOM events.
- **`[P3]` Range requests — готово** (single + multi + suffix + If-Range). `fetch_range` + `fetch_multi_range` + `parse_multipart_byteranges` в lumen-network — нормализуют все формы 206/multipart-ответов в типизированные `Range/MultiRangeResponse`. Осталось: shell-интеграция под resume downloads и `<video>` seek по таблице сегментов. (Brotli — **готово**: `BrotliContentDecoder` за `ContentDecoder` в `lumen-network`.)
- **`[P3]` DevTools / Inspector минимум.** DOM tree view + computed styles panel + network log. Стандарт — Chrome DevTools Protocol (CDP) как WebSocket-сервер.
- **`[P1+P2]` `mix-blend-mode` / `background-blend-mode` / `isolation` / `backdrop-filter`.** 16 blend modes; `backdrop-filter` — отдельный pass blur на снапшоте под элементом.

#### Phase 3+ — без этого браузер не полнофункциональный

- ✅ **`[P1]` Page Visibility API + document.readyState + navigator.sendBeacon.** `document.visibilityState`/`document.hidden` (W3C Page Visibility Level 2) — always `"visible"` in single-window Lumen; `visibilitychange` event on focus/blur via `_lumen_apply_visibility(hidden)`. `document.readyState` (HTML LS §8.2.3) — `"loading"` → `"interactive"` (after HTML parse + inline scripts; `readystatechange` + `DOMContentLoaded` dispatched) → `"complete"` (after all resources; `readystatechange` + `window.load` dispatched); `_lumen_apply_ready_state` JS driver; shell calls `notify_dom_content_loaded()` / `notify_window_loaded()` at correct lifecycle points. `navigator.sendBeacon(url, data)` (Beacon API §3.1) — `_lumen_send_beacon` native binding fires synchronous POST via `JsFetchProvider`; accepts string/URLSearchParams/FormData/Blob data. `window.addEventListener('load'/'DOMContentLoaded'/'visibilitychange')` + `window.onload` + fast-path for late listeners. `document.dispatchEvent` now actually dispatches to document-level listeners (was a no-op stub). 12 новых тестов (416 итого lumen-js). 2026-05-31.
- ✅ **`[P1]` Web Crypto API + structuredClone (W3C Web Cryptography API §3, HTML LS §2.7).** `window.crypto.getRandomValues(typedArray)` fills typed array from OS CSPRNG (`getrandom`); `window.crypto.randomUUID()` generates RFC 4122 v4 UUID. `window.crypto.subtle.digest(algo, data)` — SHA-1/256/384/512 via Rust `sha2`+`sha1` crates, returns Promise<ArrayBuffer>. `structuredClone(val)` — deep clone of primitives / Object / Array / Date / RegExp; `window.structuredClone` alias. 15 новых тестов (385 итого lumen-js). 2026-05-31.
- ✅ **`[P3]` Web Storage API (`localStorage` + `sessionStorage`).** `WebStorage` in `lumen-core::web_storage` (insertion-order key list, `get_item/set_item/remove_item/clear/key/len`). `_lumen_ls_*` + `_lumen_ss_*` native bindings in `lumen-js`. `_lumen_make_storage` factory + `localStorage`/`sessionStorage` globals in `WEB_API_SHIM`. Shell stores `HashMap<origin, Arc<Mutex<WebStorage>>>` per session; `ls_store_for_base` extracts SOP-partitioned store from `ResourceBase`; `sessionStorage` is fresh per page load. 8 tests. Phase 0: in-memory only (no disk persistence).
- ✅ **`[P1]` IndexedDB (Indexed Database API 3.0).** Pure-JS in-memory implementation in `WEB_API_SHIM` (`crates/js/src/dom.rs`): `indexedDB` (`open`/`deleteDatabase`/`databases`/`cmp`), `IDBDatabase`/`IDBTransaction`/`IDBObjectStore`/`IDBIndex`/`IDBCursor`/`IDBKeyRange`/`IDBRequest`/`IDBOpenDBRequest`. CRUD + indexes (unique/multiEntry) + cursors (forward/reverse/unique) + key ranges; key ordering number<date<string<array; dotted/array key paths; autoIncrement. Deferred-execution model: request actions run at dispatch time in FIFO order, `_lumen_idb_flush()` delivers events (queueMicrotask + shell tick). 23 tests. **Persistence:** Rust-backed via `IdbBackend` trait (`lumen-core::ext`) → `IdbStore` over `StorageBackend` (`lumen-storage`): the shim serializes all per-origin databases into one tagged-JSON snapshot (Dates preserved), persisted after each mutating flush (`_lumen_idb_persist`) and restored on init (`_lumen_idb_load`); databases survive page reload. Shell wires an `InMemoryStorage` backend (process lifetime, mirrors `localStorage`); disk durability is a one-line swap to `SqliteStorage`.
- ✅ **`[P1]` Web Speech API (W3C Web Speech §3–4).** `window.speechSynthesis` singleton (`speak/cancel/pause/resume/getVoices`); `SpeechSynthesisUtterance` (text/lang/voice/volume/rate/pitch, `onstart/onend/onerror` events, `addEventListener`); `SpeechSynthesisVoice` (one built-in "Lumen Voice" en-US). Platform TTS via `_lumen_speech_speak(text)` native binding — fire-and-forget background thread: PowerShell SAPI on Windows, `espeak`/`spd-say` on Linux, `say` on macOS. `SpeechRecognition` / `webkitSpeechRecognition` stub always rejects `start()` with `service-not-allowed` (no ML model in Phase 0). 25 integration tests. 2026-06-03.
- ✅ **`[P1]` `<script type=module>` ES Module support (HTML LS §8.1.3).** `<script type=module>` inline bodies are now evaluated as ES modules via `Module::evaluate()` (QuickJS `JS_EVAL_TYPE_MODULE`). `rquickjs` `loader` feature enabled; `LumenResolver` + `LumenLoader` in `lumen-js::esm` provide URL resolution (relative `./foo.js` resolved against base URL, absolute HTTP/data/blob passed through) and in-memory source registry (`ModuleRegistry`, `Arc<Mutex<HashMap<String,String>>>`). Classic scripts run before module scripts per HTML LS §8.1.3 execution order; module scripts are deferred after DOM is built. `QuickJsRuntime::eval_module(source)` + `register_module_source(specifier, source)` public API; both wired into `JsRuntime` trait with default no-op / fallback implementations. `collect_inline_scripts` in shell now distinguishes `<script>` (classic) from `<script type=module>` (module). Dynamic `import()` within modules resolves from the in-memory registry. `import.meta.url` is the virtual `lumen://inline-N` specifier. 8 esm unit-tests + 5 `eval_module` runtime tests + 1 shell test (`collect_inline_scripts_separates_modules`). lumen-js: 1044 lib, lumen-shell: 900. Clippy чист.
- ✅ **`[P3]` Persistent JS runtime + event bubbling.** `LayoutSource::document` и `ParsedPage::document` → `Arc<Mutex<Document>>`. `run_scripts_with_dom` возвращает живой `Option<Box<dyn PersistentJs>>` — рантайм не уничтожается после начальных скриптов. `Lumen::js_ctx` хранит контекст пока страница открыта. Клики диспатчируются через `_lumen_dispatch_bubble(nid,'click')` в JS — обход предков + document-level listeners. `document.addEventListener/removeEventListener` работают через sentinel NID=-1. `Event.cancelBubble` + `stopPropagation` + JS-triggered navigation после клика.
- ✅ **`[P3]` WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime.** ✅ WS: RFC 6455 upgrade + frame codec + JS API (`WebSocket` class, `JsWebSocketProvider`/`JsWsEvent`/`JsWebSocketSession` traits, background recv thread, `_lumen_pump_websockets()`, 12 тестов). ✅ SSE: `SseParser` + `EventSource` client + working `EventSource` JS API (`JsSseProvider`/`JsSseEvent`/`JsSseSession` traits, background recv thread, `_lumen_pump_sse()`, named events + lastEventId, shell-wired через `HttpClient`). ✅ Fetch: `fetch()` / `Request` / `Response` / `Headers` / `AbortController` / `AbortSignal` в JS shim; `JsFetchProvider` trait; `HttpClient` реализует.
- **`[P3]` HTTP auth — Basic + Digest готовы** (см. status). **Осталось:** Negotiate/NTLM, client certificates mTLS, UI-popup для credentials.
- **`[P3]` OCSP stapling + CT log enforcement + invalid cert UI.**
- **`[P3]` Safe Browsing — готово** (см. status). Отложено: 4-byte prefixes с full-hash callback, public-suffix list для безопасной обрезки host-suffixes ниже eTLD+1.
- **`[P3]` Back/forward cache (bfcache).** Снапшот DOM+JS heap для мгновенного back. Eligibility rules.
- **`[P3]` Navigation API + History API runtime.** ✅ `history.pushState/replaceState/go/back/forward` + `popstate` event + `location` object (href/protocol/hostname/host/port/pathname/search/hash/origin) инициализируется из page URL, `location.href=` / `assign()` / `replace()` / `reload()` → навигация через shell. Отложено: `navigate` event (Navigation API 2023).
- **`[P1+P2+P3]` Web Animations API runtime** поверх parsed `@keyframes` / transitions. **P1** — интерполяция; **P2** — compositor offload для transform / opacity; **P3** — animation timeline scheduling в rendering steps stage.
- **`[P1+P3]` `<contenteditable>` + Input Events Level 2 + Selection / Range API.** **P1** — DOM mutations + Selection / Range типы + `beforeinput` / `input`; **P3** — input dispatch (key + IME + drag-drop + paste), undo/redo stack в shell.
- **`[P3]` Service Worker runtime.** Fetch interception, push delivery, background sync, cache strategies. **P3** — и backend (fetch hook + storage), и JS worker context + lifecycle + `clients` API (бывший P4 объединён).
- **`[P3]` Spell check** через **provisional `hunspell-rs`** за `SpellChecker` от Sprint 0. Squiggly underline в render, context menu suggestions. Русский словарь — часть «русский first-class».
- **✅ `[P2]` Variable fonts axes runtime.** `font-variation-settings` CSS Fonts L4 §7 — cascade в lumen-layout (`FontVariationSetting { tag: [u8;4], value: f32 }`, inherited, `parse_font_variation_settings`), DrawText.font_variation_axes, normalization через fvar+avar в renderer.
- **`[P2]` Color management + Display P3 / Rec2020 / ICC profiles.** Для `<canvas>` / `<img>` / CSS `color()` функций (CSS Color L4).
- **`[P1+P2+P3]` Print pipeline runtime.** **P1** — pagination algorithm; **P2** — PDF rendering из display list; **P3** — print preview UI.
- 🟡 **`[P1+P3]` GC integration JS ↔ DOM (cycle collector).** **P1 done** — `Document::acquire_js_ref(NodeId) -> u32` / `release_js_ref` / `js_ref_count` / `is_detached` / `dead_node_ids()` в `lumen-dom`. `js_refs: HashMap<NodeId,u32>` (serde skip — не сериализуется при гибернации). 11 unit-тестов. **P3** — QuickJS finalizer callback вызывает `release_js_ref` + idle GC tick дренирует `dead_node_ids()` в shell.
- **`[P3]` Permission prompt UI + Download UI.**
- **`[P3]` GPU process / sandbox.** Реальный browser-grade sandbox: seccomp (Linux), AppContainer (Windows), App Sandbox (macOS), GPU процесс отдельно от renderer-а.

### Не приоритет, держим в голове

- **`[P2]`** Variable fonts parsers (fvar/gvar/avar/HVAR/VVAR/MVAR) в `lumen-font` — реализовано. **IUP + apply_glyph_deltas в `lumen-font::variation`** реализовано (ветка `variable-fonts-runtime`): `apply_variations_to_simple_outline(contours, variations, coords)` применяет gvar deltas к outline-контурам in-place с OpenType-spec IUP для untouched точек, 19 unit-тестов. **`Font::glyph_resolved_with_coords` (ветка `variable-fonts-glyph-resolved`)**: variable-fonts entry поверх resolve-pipeline (simple-outline + composite recursion, gvar парсится один раз перед спуском); пустой coords / font без gvar — short-circuit на `glyph_resolved`; component-level gvar variations отложены. 6 integration на synthetic-TTF + 2 на Inter (без gvar). **Atlas variation-aware cache key (ветка `atlas-variation-key`)**: `AtlasKey { face_id, glyph_id, size_bin, coords_hash: u64 }` через `AtlasKey::hash_coords(&[f32])` — variant glyph не перезаписывает base. `DisplayCommand::DrawText.font_variation_coords: Vec<f32>` — interface-first hook для P1 cascade (empty = default-instance, snapshot-тесты неизменны). Renderer зовёт `glyph_resolved_with_coords` и передаёт coords. Осталось: CSS `font-variation-settings` cascade в `lumen-layout` (parser longhand `font-variation-settings: "wght" 600, "wdth" 80`, computed-value-time clamp через `fvar.axes` + `avar.normalize`, заполнение DrawText.font_variation_coords).
- **`[P2]`** GSUB/GPOS shaping (для арабского, индийского, тайского). Текущая позиция — добавим как exception #5 (rustybuzz) или сами для базовых случаев.
- ✅ **`[P1]`** ADR-инфраструктура (`docs/decisions/`) — формализация decisions log. Реализовано 2026-05-25: TEMPLATE.md + README-индекс + ADR-001..005 (custom engine, dep policy, SQLite, JS runtime, image decoding).
- **`[P3]`** StorageBackend trait: добавить origin partitioning параметр (`(origin, top_level_site)`) ДО первой реализации, чтобы не переделывать.
- Composite glyphs с ARGS_ARE_XY_VALUES=0 (point alignment) — реализовано — см. `git log --oneline | grep composite-point-align`.
- CSS4 pseudo-class `:has(...)` — реализовано — см. `git log --oneline | grep css-has-pseudo`.

---

