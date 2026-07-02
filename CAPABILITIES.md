# CAPABILITIES.md — what Lumen can do right now

**Single source of truth for "what the browser can already do".** Ground truth verified
against code (not plans) on 2026-06-16. Organized by subsystem/crate.

- ✅ = implemented and working in code today
- 🟡 = partial / works with caveats
- ⬜ = notable gap, deferred (listed so the boundary is explicit)

**This file answers "что уже умеет браузер" — read ONLY this, do not re-read `docs/plan/*`,
`phases.md`, or `STATUS-PN.md`.** Those track *intent* and *task queues*, not shipped
capability, and drift from code (see CLAUDE.md). For per-CSS-property detail see
[CSS-SPECS.md](CSS-SPECS.md); for per-crate design prose see [subsystems/](subsystems/).

**Maintenance rule:** when a feature merges to `main`, add/adjust one line here in the
**same commit** as deleting the completed task's `STATUS-PN.md` pointer line. This is the only file that must stay
true to code; keep it honest about ⬜ gaps too.

Snapshot: **Phase 2 «Interactive» (complete), app v0.5.0**. ~21 crates.

---

## Engine — source pipeline

### lumen-dom (`crates/engine/dom`)
- ✅ Arena DOM: `Vec<Node>` + `NodeId(u32)`, no `Rc/RefCell`, no cycles (deny-linted).
- ✅ Node model: Document / Doctype / Element / Text / Comment / ShadowRoot; `QualName`, 6 namespaces, attributes.
- ✅ Core API: create/append/detach/get, `base_href()`, `find_first_element(predicate)`, tree-print.
- ✅ `InputType` (22 HTML5 input types), `DocumentMode` (NoQuirks/Quirks/LimitedQuirks, set by parser).
- ✅ Shadow DOM: `attach_shadow`, `FlatTree` + `build_flat_tree` with `<slot name>` assignment.
- ✅ Hibernation snapshot: `Document::to_bytes()/from_bytes()` (bincode); JS-wrapper refcounting for GC (`acquire/release_js_ref`, `dead_node_ids()`).
- ✅ Drag-and-drop draggability; contenteditable editing layer (`Range`, `Selection`, `CommandHistory` undo/redo, paste/drag transfer).
- ⬜ Arena compaction / free-list; auto-set `:target` from URL fragment is shell-side.
- ~232 tests.

### lumen-html-parser (`crates/engine/html-parser`)
- ✅ Iterator FSM tokenizer (RAWTEXT/RCDATA/DOCTYPE/comments); all 23 HTML5 insertion modes.
- ✅ ~250 named entities + numeric refs; DOCTYPE public/system id; quirks-mode detection (detection only).
- ✅ `srcset` + `sizes` (media conditions incl. `prefers-color-scheme`), `<picture>`/`<source>` selection.
- ✅ Preload scanner (`scan_preload_hints`); push/incremental parsing (`PushTokenizer`, `IncrementalTreeBuilder` with partial-UTF-8 buffering, byte-equal to pull parse).
- ✅ Declarative Shadow DOM (`<template shadowrootmode>`).
- ⬜ CDATA, legacy entities without `;`, `<plaintext>`/`<noembed>`, `calc()` in sizes, `loading="lazy"`.
- ~394 tests.

### lumen-css-parser (`crates/engine/css-parser`)
- Parses selectors + **untyped string declarations**; typed values + cascade live in `lumen-layout/style.rs` (~139 properties wired end-to-end — see CSS-SPECS.md).
- ✅ Selectors L3 full set + L4: attribute operators (`= ~= |= ^= $= *=`, case flag), structural pseudo, form/UI-state pseudo (DOM-attr-based), `:nth-*(of …)`, `:not/:is/:where`, `:has` (in layout).
- ✅ `:lang/:dir/:link/:visited(always false)/:scope/:target`; interactive pseudo (`:hover/:focus`) parsed as always-false (runtime state applied in layout).
- ✅ `!important` extraction; at-rules parsed+stored: `@media` (cascade-integrated), `@font-face`, `@import`, `@property`, `@layer`, `@supports` (typed `evaluate()` — incl. `selector()`, `font-tech()`/`font-format()` matched against lumen-font capabilities), `@keyframes`, `@scope`, `@container`.
- ⬜ Namespace prefixes; cascade wiring for `@layer`/`@scope`/`@container`.
- ~292 tests.

### lumen-encoding (`crates/engine/encoding`)
- ✅ Decoders: Windows-1251, KOI8-R, CP866, UTF-16 LE/BE (surrogates), UTF-8; BOM strip; `from_label` (WHATWG aliases).
- ✅ `detect()` chain: BOM → `<meta charset>` → HTTP hint → valid-UTF-8 → Russian-frequency heuristic.
- ✅ ICU4x 2.2 unicode provider (line-break UAX#14, grapheme/word UAX#29, bidi UAX#9); Knuth-Liang hyphenation (11 locales, used when `hyphens: auto`).
- ⬜ ISO-8859-5, MacCyrillic, full HTML5 prescan, UTF-32.
- ~90 tests.

---

## Engine — layout & rendering

### lumen-layout (`crates/engine/layout`)
- ✅ Block + inline flow (line wrap, margin collapsing, `margin: 0 auto`, `line-height-step` vertical rhythm).
- ✅ Flexbox (full: direction, grow/shrink/basis, justify/align, gap, wrap). ⬜ column-direction wrapping.
- ✅ CSS Grid (px/fr/auto/repeat/minmax, explicit+auto placement, dense, subgrid, `order`). ⬜ grid-template-areas, named lines. `grid-template-*: masonry` / `display: masonry` fall back to a regular grid / multicol (matches Edge, which ships no CSS masonry).
- ✅ Multi-column (`column-count`/`column-width`/`column-gap`/`column-rule`/`column-span`, `column-fill: balance|auto` — balanced atomic-box distribution via binary-searched column height).
- ✅ Table layout (colspan/rowspan, column widths) — live path `box_tree.rs` (note: `table.rs` is dead code).
- ✅ Positioned: relative, absolute/fixed (out-of-flow + containing-block threading); `position: sticky` partial (offsets computed, scroll wiring shell-side).
- ✅ SVG layout pass (viewBox, rect/circle/ellipse/line/path, `<use>` with cycle detection); `<text>` with `text-anchor`/`dominant-baseline` (presentation attribute **and** CSS property — CSS overrides the attribute and inherits from `<g>`); vertical writing modes (`vertical-rl/lr`).
- ✅ Replaced: `<img>` (picture/srcset picker), `<iframe>` placeholder.
- ✅ Cascade: specificity + `!important`, RTL selector matching, all CSS3 structural + L4 form/UI pseudo, `:has()`, `::before/::after` (string content), `::first-line/::first-letter` (drop-cap float), `initial-letter` (size/sink drop cap, Phase 0).
- ✅ Values: `calc/min/max/clamp` + math fns, `var()`, `@property` registration, viewport units, intrinsic sizing (`min/max/fit-content`).
- ✅ Animations/transitions scheduling (`@keyframes` interpolation, timing functions, transform/gradient/filter interpolation; `background:<color>` shorthand in keyframes); `content-visibility: auto` skip; Shadow DOM flat-tree integration.
- 🟡 Scroll-Driven Animations L1: `animation-timeline: scroll()|view()|<named>` drives animation progress from scroll/viewport position (not the clock) — opacity/transform render in the live window; animated `background-color` not yet composited (BUG-231).
- ✅ Algorithm stubs awaiting P4 CSS wiring: anchor positioning, subgrid context.
- ✅ `%` lengths in block flow (RP-1): `width`/`margin-*`/`padding-*` resolve against the containing block's **width** (vertical margin/padding included, per CSS 2.1 §8); percentage `height` resolves against the CB **height** when definite, else computes to `auto`.
- ✅ `float: left/right` + `clear` (RP-4, CSS 2.1 §9.5): placement, shrink-to-fit auto width, wrap to next line (rule 8), float context establishes a BFC, container height encloses floats, inline text wraps around floats, `clear` clearance (margin-absorbing), `shape-outside` (circle/ellipse/inset/polygon/path). A non-BFC block beside a float keeps its full containing-block width — only its (and its descendants') line boxes are shortened, propagated into nested in-flow blocks; BFC blocks shift past floats; `clear` and nested floats work at depth. Remaining edge: per-line widening of an inline run *below* a shorter float (single-width approximation).
- ⬜ `ch`/`ex` units, real `direction: rtl` reordering, CSS4 color spaces (lab/lch/oklab/oklch), `attr()`/`counter()` content. Many L3/L4 properties are **parse+store only** (text-emphasis, container queries, touch-action, appearance, resize).

### lumen-paint (`crates/engine/paint`)
- ✅ **Live default render path is `FemtoovgBackend`** (OpenGL ES via glutin), with wgpu auto-fallback; `LUMEN_BACKEND` overrides. **Paint bugs from graphic_tests are fixed in `femtovg_backend.rs`, not `renderer.rs`.**
- ✅ **Wide-gamut output surface (wgpu backend only)** — wgpu renderer selects a wide-gamut swap-chain format (`Rgba16Float` for DisplayP3/Rec2020, non-sRGB fallback for sRGB) based on the display profile queried at startup (`Lumen::target_color_space()`); canvas background is colour-managed from sRGB into the destination encoding before the clear op commits the frame. Femtoovg remains 8-bit sRGB (platform API limitation).
- ✅ **Target-aware image decode pipeline (ph3 Step 6)** — `lumen-image::decode_to(bytes, target)` colour-converts decoded RGB images to the display target (DisplayP3/Rec2020/sRGB) at decode time, replacing the previous sRGB-only path. Shell propagates `self.target_color_space()` through all image decoders (`decode_image`, `fetch_and_decode_images`, `fetch_and_decode_background_images`, `render_bytes`, `parse_and_layout`). 8-bit precision retained; per-pixel converters for P3↔Rec2020 added alongside the existing ICC matrix-shaper path.
- ✅ DisplayCommand primitives (all in enum + handled by femtovg): FillRect, FillRoundedRect (SDF), DrawBorder (solid/dashed/dotted/double), DrawText, DrawOutline, DrawImage (object-fit/position), Linear/Radial/Conic gradients (incl. repeating; radial `circle`/`ellipse` ending shape + size keywords via `radial_gradient_radii`, CSS Images L3 §3.5 — BUG-239; repeating/radial rendered per-pixel to bypass the 256-texel LUT — BUG-085; premultiplied stop interpolation for fades to `transparent`, CSS Images L4 §3.1 — BUG-190), SvgPath, clip, opacity, blend modes, transforms, filters, backdrop-filter, scroll layers, masks, layer snapshots, page breaks.
- ✅ Stacking contexts + paint order (CSS 2.1 Appendix E), stacking-aware hit testing (transform inversion).
- ✅ CSS Motion Path L1 (`offset-path: path()`/`ray()`, `offset-distance`, `offset-rotate` auto/reverse/fixed, `offset-anchor`) — box's anchor placed on the path point, rotated around it (`forward_box_transform` + property-tree `walk`). TEST-76 boxes pixel-identical to Edge.
- ✅ box-shadow (outset+inset), text-shadow, text-decoration (underline/overline/line-through, wavy/dotted/dashed/double, thickness), border-radius SDF.
- ✅ CSS filters (GPU color-matrix + Gaussian blur), backdrop-filter (LRU cache), clip-path (bbox approximation).
- ✅ 3D transforms (perspective, preserve-3d depth sort) in wgpu renderer; multi-size + variation-aware glyph atlas, per-char codepoint fallback cascade.
- ✅ **Synthetic bold/italic fallback (RP-6, 2026-07-02)** — when no true bold/italic face is available, `font-weight >= 600` is emulated by a double glyph blit with a small horizontal offset and `font-style: italic/oblique` by a ~12° shear, in **both** text engines: CPU/headless (`cpu_raster::rasterize_text` — outline shear + double coverage blit; the bundled Inter Regular finally renders headings bold) and the live femtovg window (`draw_text_styled` — second `fill_text` + `Transform2D` shear about the baseline; skipped when `FontProvider` resolves a real bold/italic face). Advance metrics are intentionally unchanged — layout is unaffected.
- ✅ Compositor scaffolding (two-buffer commit, threaded compositor, 60fps vsync); print (`render_print_pages` → images); CPU rasterizer (`cpu_raster.rs`, feature `cpu-render`, cross-OS bit-identical, snapshot gate; **femtovg-parity for `<img>` decode+`object-fit`+area-averaged downscale, circular **and elliptical** radial gradients (`circle`/`ellipse` shape+size — BUG-239), clamped `border-radius` fills — BUG-221; `background-image` url()/`image-set()` tiling (`background-size`/`position`/`repeat` via shared `bg_tile_geometry`) and `cross-fade()` blending**); software WebGL 1.0 (GLSL ES vertex+fragment interpreter executes — `glsl.rs`; framebuffer readable via `readPixels`, not yet presented to the page `<canvas>`).
- 🟡 femtovg `mask-image` gradient masks are **true per-pixel alpha masks** (offscreen FBO + `DestinationIn`, linear/radial/conic; BUG-183). `mask-mode: luminance` ✅ wired (BUG-218): `emit_push_mask` bakes `luminance(rgb)·alpha` into each gradient stop's alpha, so both femtovg and CPU paths honour it. `mask-origin` ✅ wired (2026-06-22): the mask positioning rect comes from `background_origin_rect` (border/padding/content box, initial `border-box`); `mask-position` threaded into `PushMaskImage` (initial `center`). `url()` image masks still scissor bbox (no decoded source), so `mask-position`/`mask-repeat` tiling have no visible effect yet. `mask-clip` (painting-area clip) and `mask-composite` (multi-layer) not wired.
- ⬜ GPU shadow pipeline, Groove/Ridge/Inset/Outset borders, exact polygon clip-path, elliptical border-radius (rx≠ry), Vello backend (no-op stub).

### lumen-font (`crates/engine/font`)
- ✅ Table parsers (head/maxp/cmap fmt4+12 incl. SMP/emoji/hhea/hmtx/loca/glyf/name/OS2/post); rasterizer (simple + composite glyphs, 4×4 supersampling).
- 🟡 Variable fonts runtime (fvar/avar/HVAR/VVAR/MVAR/gvar, IUP + deltas) — applied on the **CPU rasterizer / wgpu paths** (`--screenshot`, snapshot gate) via `apply_variations_to_simple_outline`. **Not** wired into the live femtovg window (femtovg renders the default instance via its own `fill_text`), same fork as GSUB/GPOS shaping below. font matching/fallback (`SystemFontIndex` scans OS fonts, weight/style matcher, per-char cascade); WOFF2 (Brotli) + WOFF1 (zlib) decode.
- ✅ **`font-display: swap` (PH3-19)**: `@font-face url()` sources fetched asynchronously off the critical paint path (FOUT). First paint uses Inter fallback; background thread fetch+decode → `FontLoaded` event → relayout with `MultiFontMeasurer` to swap in the web font. `local()` sources still loaded synchronously (no network round-trip needed).
- 🟡 **Shaping (GSUB/GPOS) — U-2 stage 1**: `Shaper::shape()` applies GSUB ligatures (Type 1 single + Type 4 ligature, incl. Type 7 extension) and GPOS kerning (Type 1 single + Type 2 pair, formats 1/2, incl. Type 9 extension) for Latin/Cyrillic; default features `liga`/`clig`/`calt`/`rlig`/`ccmp` (GSUB) + `kern` (GPOS). Wired into the **CPU rasterizer** (`render_to_image_cpu` → `--screenshot`, snapshot gate). **Not** wired into the live femtovg window (femtovg shapes via its own `fill_text`) nor the per-char layout measurement. Out of scope: contextual lookups (GSUB 5/6, GPOS 7/8), mark positioning (GPOS 3–6), complex scripts (Arabic/Indic), LookupFlag mark filtering.
- ✅ **CFF outlines (`.otf` PostScript) — U-2 stage 2**: `lumen-font::cff` parses the `CFF ` table (INDEX/DICT, Private DICT, global+local subrs) and interprets Type 2 charstrings (all path/hint operators, the four flex ops, subr bias, `seac` composites). Cubics are flattened to on-curve segments so the existing rasterizer is reused. CID-keyed fonts (`ROS`/`FDArray`/`FDSelect` fmt 0+3) supported. Routed transparently through `Font::glyph_resolved`, so CPU raster, the wgpu renderer, and Canvas 2D all draw `.otf` text. Deferred: CFF2 (variable PostScript), charstring arithmetic ops.
- ⬜ No hinting, no color glyphs (COLR/CPAL/sbix), no bitmap strikes. Fallback covers only already-loaded faces.

### lumen-image (`crates/engine/image`)
- ✅ PNG, JPEG (baseline + progressive), WebP (VP8 + VP8L), **GIF** (static + animated), **AVIF** (behind `avif` feature).
- ✅ **External SVG images (RP-5, 2026-07-02)** — `<img src=*.svg>` and `background-image: url(*.svg)` render. `lumen_image::is_svg` sniffs SVG markup (BOM/whitespace skip; `<svg`, `<!DOCTYPE svg`, or `<?xml` prolog + `<svg` in the first 4 KB, case-insensitive; `image/svg+xml` added to `supported_mime_types`); the shell (`svg_image.rs`) wraps the markup in a minimal HTML document and renders it through the normal deterministic layout/paint pipeline (`parse_and_layout` → `paint_ordered` → `render_to_image_cpu`) at the intrinsic size (`width`/`height` attrs → `viewBox` → CSS default 300×150, clamped 1..=4096). Deferred: re-raster at CSS box size (icons upscale from intrinsic), `currentColor` from the host element.
- ✅ `resize_bilinear`, `ImageDecoder` trait, `ImageDecodeCache` (LRU 256 MB, `ImageHandle`/`ImageKey`).
- ⬜ JXL and HEIC are sniff-only Err stubs.
- ✅ ICC colour management (full CMM for RGB + CMYK, ICC-1…ICC-6): real read-only ICC parser (`lumen_core::icc::IccProfile` — header, tag table, `rXYZ/gXYZ/bXYZ`, `rTRC/gTRC/bTRC`, `wtpt`, raw `A2B0/B2A0`); RGB profiles classified by colorant primaries (sRGB/Display-P3/Rec.2020); CIE XYZ/Lab PCS + Bradford adaptation (`lumen_core::pcs`, `ColorSpace::Lab`). **matrix-shaper RGB→sRGB transform** (`IccProfile::build_rgb_transform` — real per-channel TRC evaluation + colorant matrix → D65 → sRGB), so any RGB ICC profile (P3, Rec.2020, Adobe RGB, ProPhoto, …) renders colour-correct in the femtovg window, CPU snapshot and PDF export. **CMYK→sRGB LUT transform** (`IccProfile::build_cmyk_transform` — parses `A2B0` `lut8`/`lut16`/`lutAToBType`, multilinear CLUT interpolation, XYZ/Lab PCS → D65 → sRGB), wired into the JPEG decoder: CMYK/YCCK JPEGs with an embedded CMYK profile decode through the profile (Adobe-inversion aware) instead of zune's naïve CMYK→RGB. **Colour management runs once at decode** (`lumen_image::decode` → `color_manage_in_place`) with a process-wide transform cache keyed on profile bytes (`cached_rgb_transform`/`cached_cmyk_transform`), so each image is transformed exactly once and a profile is parsed/compiled at most once. PNG `iCCP` profiles are inflated correctly (zlib, BUG-229). Verified end-to-end by `graphic_tests/128-icc-color-management.html` (Display-P3 PNG, pixel-identical to Edge) and `crates/engine/image/tests/icc_color_management.rs` (P3 + CMYK).

### lumen-canvas (`crates/engine/canvas`)
- ✅ Canvas 2D CPU rasterizer: rect ops, full path building (arc/arcTo/bezier/quadratic/ellipse), fill/stroke (even-odd), state stack + full CTM, `globalAlpha`, 16 composite/blend ops, line caps/joins.
- ✅ Gradients (linear/radial/conic), patterns (4 repeats), shadows (offset-only), `clip()` (boolean mask), image data (drawImage/putImageData/get/createImageData), text via `lumen_font::Rasterizer`, Path2D (SVG path strings). `drawImage` source may be `<canvas>` or `<img>` element (all 3/5/9-arg forms).
- ⬜ Gaussian shadowBlur; gradient sampling is device-space (not spec user-space); canvas fingerprint noise.

---

## JS runtime & Web APIs

### lumen-js (`crates/js`) — QuickJS via `rquickjs` 0.11
Modern ES (ES2020+: classes, async/await, generators, Promise, Proxy, BigInt, modules) comes from QuickJS. ~90 Web-API modules wired by Lumen JS shims + `_lumen_*`/`__lumen_*` native bindings (`install_dom`, `lib.rs:502`).

- **DOM** — ✅ full read/write, querySelector(All) via real CSS3 engine, matches/closest, innerHTML, createElement, getBoundingClientRect (real layout), DOM mutation → auto relayout. Shadow DOM, Popover, `<dialog>`/CloseWatcher, inert, ElementInternals + CustomStateSet, DOMParser/XMLSerializer, SVG DOM, Sanitizer (Phase 0).
- **Events** — ✅ EventTarget (bubbling/capture/stopPropagation/composedPath), Mouse/Pointer/Keyboard/Drag events, Pointer Events L3 capture, Pointer Lock.
- **Networking** — ✅ fetch + Headers/Request/Response/AbortController (`.timeout/.any`), XMLHttpRequest, WebSocket, Server-Sent Events, URL/URLSearchParams. ⬜ WebRTC (mDNS-only stub, no IP leak), WebTransport (stub).
- **Graphics** — ✅ Canvas 2D (via `lumen_canvas`, flushed per frame), OffscreenCanvas, WebGL/WebGL2 (software backend with a real GLSL ES interpreter — vertex+fragment shaders execute, `glsl.rs`; framebuffer readable via `readPixels`), Web Animations API (real interpolation). ⬜ WebGL framebuffer not yet presented to the page `<canvas>` (only `readPixels`, unlike the WebGPU present path), toDataURL blank (anti-fingerprint). 🟡 WebGPU (`navigator.gpu`): real GPU adapter info + WGSL validation via wgpu (U-4c Stage 1); real `GPUBuffer` create/write/map-readback + `copyBufferToBuffer` submit through GPU memory (U-4c Stage 2 sub-step 1, feature `webgpu`); real compute pipelines + bind groups + `dispatchWorkgroups` execute WGSL on the GPU (U-4c Stage 2 sub-step 2); real render pipelines + offscreen `GPUTexture` render targets + `beginRenderPass`/`draw` + `copyTextureToBuffer` readback execute on the GPU (U-4c Stage 3 sub-step 1); `canvas.getContext('webgpu')` presents the rendered texture onto the page `<canvas>` — `configure`/`getCurrentTexture` allocate a real render target, and after a render-pass `submit` the frame is read back and composited as `canvas:{nid}` (U-4c Stage 3 sub-step 2). **U-4c WebGPU backend complete.**
- **Workers/Concurrency** — ✅ Web Workers (real threads, importScripts), SharedWorker, BroadcastChannel, Promise/microtasks + queueMicrotask, Web Locks, timers (setTimeout/Interval + precise wakeup), requestAnimationFrame, scheduler.postTask/yield.
- **Storage** — ✅ Web Storage (localStorage SOP-partitioned + persistent, sessionStorage per-load), Cookie Store, Storage Buckets (`navigator.storageBuckets` open/keys/delete + per-bucket persisted/persist/estimate/durability/setExpires/expires/getDirectory, `indexedDB`/`caches` accessors; Phase 0 in-memory), IndexedDB (full: stores/indexes/cursors/key ranges/autoIncrement; per-origin persist into a dedicated structured SQLite file `<exe>/data/idb/<key>.db` — `NativeIdbStore`: schema mirrored into `idb_meta`/`idb_stores`/`idb_indexes`, records in a lossless snapshot blob), Service Workers (lifecycle + persist; 🟡 fetch interception Phase 1 — on activate the SW script runs in a dedicated QuickJS thread, `FetchEvent`/`respondWith` dispatched by `ServiceWorkerInterceptor` on the network path, cache-first via the shared Cache API store; ⬜ no in-SW network fetch, so SW `cache.addAll()` precaching can't pull from network — only entries the page cached are served), StorageManager (OPFS stub), Cache API, Shared Storage (in-memory).
- **Media/Devices** — ✅ getUserMedia({audio}) + getDisplayMedia (live when provider installed; Win32 GDI capture), HTMLAudioElement (real playback), HTMLVideoElement (GIF), Picture-in-Picture + Document PiP, Web Speech TTS (OS), MediaSession, Clipboard, Geolocation (denied default). 🟡 WebVTT: cue-парсер + сбор <track> (lumen_dom::vtt, 2026-07-02) — фетч/TextTrack API/рендер cue ⬜. ⬜ WebHID/USB/Bluetooth/Serial/MIDI/WebXR/WebCodecs (NotSupported stubs), Web Audio (graph only, no DSP).
- **Observers/Timing** — ✅ MutationObserver, ResizeObserver, IntersectionObserver (drives loading=lazy), performance.now()/timeOrigin, Navigation Timing classes + delivery. ⬜ general PerformanceObserver.
- **Misc** — ✅ WebAuthn/passkeys (ES256), SubtleCrypto (real), Trusted Types L2, CSP, Permissions Policy, Idle Detection (Win32), Wake Lock, File API + File System Access, Intl (ECMA-402 shim en-US/ru-RU), Temporal (shim), URLPattern, Navigation API, View Transitions, anti-fingerprint layer (ADR-007, deterministic mode). 🟡 WebAssembly MVP — pure-Rust interpreter (`lumen-js::wasm`): decodes the WASM 1.0 core binary format and **executes** it. `compile`/`validate`/`instantiate` work; `Instance.exports` are callable functions; linear memory, globals, tables, `call_indirect`, and JS function imports are supported. Numeric values cross the JS↔WASM boundary by type — `i64` as a JS `BigInt` (full 64-bit precision, per the W3C WebAssembly JS Interface), the rest as `Number` — for exported functions, host imports, and globals. Fixed-width **SIMD** (`v128`, the `0xFD` prefix) is fully supported (`lumen-js::wasm::simd`), as is **relaxed-SIMD** (`0xFD` sub-opcodes `0x100..=0x113` — madd/nmadd, laneselect, relaxed min/max, relaxed trunc, swizzle, q15mulr, the i8×i7 dots — computed with deterministic strict semantics, a conforming choice). **Threads / atomics** (the `0xFE` prefix — atomic load/store/rmw/cmpxchg, `memory.atomic.wait*`/`notify`, `atomic.fence`) execute with single-threaded semantics (one agent, so every op is trivially atomic; `wait` never blocks), and shared-memory modules decode. JS-level `SharedArrayBuffer` + `Atomics` are available (QuickJS-native: `load`/`store`/`add`/`sub`/`and`/`or`/`xor`/`exchange`/`compareExchange`/`notify`/`isLockFree`, growable SAB) plus shimmed `Atomics.waitAsync` (ES2024) and `Atomics.pause` (ES2025); synchronous `Atomics.wait` throws on the single non-blocking agent, as on a browser main thread. Exported **`Memory.buffer` aliases live** (U-4b): it is one stable JS `ArrayBuffer` synced with Rust-owned linear memory at call boundaries, so the emscripten `HEAP32 = new Int32Array(memory.buffer)` pattern is coherent in both directions (and a captured view survives across calls; growth detaches and replaces it). Boundaries: a host import can't observe writes made earlier in the same in-flight call; an *imported* `Memory` is not aliased to the instance; `Memory.buffer` is not backed by a `SharedArrayBuffer`; no multi-memory. ⬜ Privacy-Sandbox (Topics/Attribution/Background Fetch/Push — in-memory stubs), heap-snapshot serialization (shell re-runs scripts on restore).

> Boundary note: "functional" APIs (Canvas2D, WebGL, getUserMedia, WebSocket, XHR, IndexedDB, Web Animations, WebAssembly MVP) actually do work; many depend on the shell installing a provider — without it they degrade to rejection. The long stub list (WebGPU/WebCodecs/WebHID/…) resolves/rejects without doing work; WebCodecs `configure()` reports unsupported codecs via the async error callback (not a synchronous throw).

---

## Networking & storage

### lumen-network (`crates/network`)
- ✅ HTTP/1.1 (keep-alive, connection pool), HTTPS (rustls 0.23 + webpki-roots, ALPN), **HTTP/2** (frame codec, HPACK, pool multiplexing, recv flow control).
- ✅ Brotli + gzip + deflate content-decoding (`Accept-Encoding: br, gzip, deflate`); redirects (≤5), chunked decode, IDN→Punycode.
- ✅ Cookie jar wired into client (inject/persist per hop); CORS preflight + enforcement; Origin/Mixed-Content/Sandbox/CSP/COOP classifiers.
- ✅ HTTP auth (Basic + Digest MD5/SHA-256, 401 retry), Range requests, HSTS (+ preload), SOCKS5 proxy (proxy-side DNS, Tor-ready).
- ✅ DNS: system + DoH (RFC 8484) + DoT (RFC 7858); `RequestFilter` hook (EasyList/hosts ad-block; **Phase 2 `$`-options** — resource-type `$script`/`$image`/`$stylesheet`/`$font`/`$xmlhttprequest`/`$subdocument`/`$media`/`$other` + `~`-negation, plus `$third-party`/`$first-party`, matched against a per-request `RequestContext`; `domain=` parsed-but-ignored); fingerprint/TLS profiles (Chrome/Firefox/Safari/Edge/Tor/Lumen/Strict — header order, H2 SETTINGS, Client Hints).
- ✅ WebSockets (+ permessage-deflate, `Sec-WebSocket-Protocol` sub-protocol negotiation surfaced as `ws.protocol`, `CloseEvent.wasClean`), EventSource, Fetch bridge, software WebAuthn `VirtualAuthenticator` + CTAP2-over-HID (no USB enumeration).
- ✅ **Cross-navigation HTTP cache wired into the shell (RFC 7234):** one shared `DiskHttpCache` (`<exe_dir>/data/cache/http_cache.db`, survives restart) attached to every client via `apply_http`; subresources/`fetch()` are served fresh-hit or revalidated with conditional GET (304) on repeat visits instead of re-downloaded. Private/Tor sessions use an in-memory cache (nothing on disk).
- ⬜ mTLS/client certs, `qop=auth-int`, CORS POST/PUT bodies, H2 send-side flow control.

### lumen-ipc (`crates/ipc`)
- ✅ Length-prefixed bincode over TCP loopback; `IpcChannel/Server/Client` blocking RPC; messages `Fetch/Ping/Shutdown`; powers out-of-process network service (`--network-service`).
- ✅ Tab control channel (TAB-4/5): `CreateTab/NavigateTab/Screenshot/CloseTab` + `TabId`; shell `--ipc-server` is the TCP server, an external controller drives headless tabs and pulls deterministic CPU-rendered PNGs over IPC (no window/gdigrab/ffmpeg).
- ⬜ Fetch is GET-only (no full method/headers/body yet). Tab control is single-client sequential (no multiplexing).

### lumen-storage (`crates/storage`)
- ✅ SQLite everywhere (rusqlite bundled, WAL, prepared-cached); origin-partitioned KV `(origin, top_level_site, key)`.
- ✅ Cookie jar over SQLite (SameSite, partitioning, PSL), History, Bookmarks (folders/tags), Web Storage backend, IndexedDB store, Service Worker store + interceptor, Cache Storage.
- ✅ Profile vault encryption (AES-256-GCM + PBKDF2 100k); HttpCache (RFC 9111 basic), HSTS store, DnsCache, SafeBrowsing (local SB v4), PSL provider.
- ✅ Many stores: Downloads, Permissions, Autofill, Notifications, Workspaces, TabSessions/Snapshots, SiteEngagement, SearchHistory, TabGroups, PushSubscriptions, BFCache.
- ⬜ ADR-012 partitioning is **strategy only** — no DB manager; ~36 stores each open their own SQLite file. No schema-migration framework.

### lumen-knowledge (`crates/knowledge`)
- ✅ FTS5 history search (bm25, snippets, diacritics-folding), Notes (§12.2), Read-later (§12.3, status/tags), OpenTabsIndex (§12.4, in-memory).
- ✅ `KnowledgeStore` trait + `DefaultKnowledgeStore`; omnibox `@history` / `@notes` / `@read-later` / `@tabs` prefixes wired (read-later/tabs = FTS/substring search → navigate / switch-tab).
- ⬜ Local AI / vector (HNSW) index, Russian Porter stemmer.

---

## Shell, automation & accessibility

### lumen-shell (`crates/shell`) — the user-facing browser
**Navigation/Tabs** — ✅ load file/http(s)/local HTML with streaming incremental parse+paint + progressive image loading; **non-blocking navigation** (every navigation — link click, address bar, back/forward, JS `location.href=`, reload — runs through the same off-UI-thread streaming pipeline as the initial load; the window stays responsive and paints progressive frames instead of freezing. U-1 stage 1 moved the fetch off-thread; **BUG-171 stage 2** moved the entire final render — script fetch + QuickJS + image/CSS/font fetch + layout — onto a worker thread too, posting the finished page back via `LoadEvent::RenderDone`, so even the ~1.9 s JS+layout CPU phase no longer freezes the UI); link-click + fragment nav (`:target`); same-document `location.hash=` setter (HTML LS): changing the fragment navigates without a reload, adds a history entry, and fires a `hashchange` event (`window.onhashchange` + `addEventListener('hashchange')`); reload; tab strip + groups (colour-coded) + containers (cookie/storage isolation) + context menu + auto-archive; vertical tabs, tree tabs, workspaces, split view; `about:newtab` speed-dial; omnibox FTS suggestions (`@history` / `@notes` / `@read-later` / `@tabs` — search history/notes/read-later/open-tabs, selecting navigates or switches tab). `window.navigation` singleton with `navigate()`/`back()`/`forward()`/`traverseTo()` + `navigate`/`navigatesuccess`/`navigateerror`/`currententrychange` events, wired to the real shell `nav_back`/`nav_fwd` stack; `NavigateEvent.intercept()` + `preventDefault()` round-trip working (same-document SPA without reload, commit via `InterceptedSuccess`). 🟡 back/forward cache (bfcache, HTML LS §8.6): in-memory LRU of HTML snapshots keyed by URL — back/forward restores instantly without a network round-trip, and `pagehide`/`pageshow` lifecycle events fire on the page (`pageshow.persisted=true` on a bfcache restore); full **DOM freeze/thaw is wired** (`FrozenPage`): navigate-away serializes the DOM arena + retains the parsed stylesheet shell-side, back/forward thaws it in place without reload or script re-run (DOM keeps all JS mutations; verified live). JS-heap freeze still gated on 10C.2 (QuickJS heap serialization) — the thaw installs a fresh runtime, so listeners/globals do not survive. ⬜ history/search in-memory only.

**Reading/Content** — ✅ reader view, find-in-page (Ctrl+F, highlights/next-prev/scroll-to), source view, read-later panel, note viewer.

**UI panels** — ✅ command palette, settings, bookmarks, history, AI sidebar (Ctrl+Shift+A, `AiBackend` trait, `NullAiBackend` default), Picture-in-Picture (+ OS window), certificate viewer, permission popover, a11y/focus/sidebar panels, light/dark/system themes + accents (a central `Palette` drives the tab strip, address bar, **and all ~22 secondary panels** — each follows the light/dark setting via a threaded `&Palette` of role-named tokens). Docked sidebars (vertical tabs, tree tabs, AI, web sidebar) are **drag-resizable** — drag a panel's inner edge to change its width; the web sidebar **reflows its page content to the new width** on release; the layout persists across restarts in `<exe_dir>/data/ui/panel_layout.txt` (`panel_layout::PanelLayout`). 🟡 Cross-dock (moving a sidebar left↔right via `Ctrl+Alt+B`, persisted) works for **all four** docked sidebars — vertical tabs, tree tabs, AI, and web sidebar; the only F2-6 remainder is the infrastructure-only `SurfaceManager` (ADR-009) migration of the live shell (no new user-facing capability).

**Input** — ✅ Vimium-style click hints, vim mode, gestures, human-like + native input injection, HTML5 drag-and-drop, forms runtime (validation + picker overlays), per-tab zoom, smooth scroll + scrollbar (drag + track-click) + momentum, relayout-on-resize (layout viewport tracks live `inner_size` minus tab strip; `vw`/`vh`/`%`/`@media` follow the window). 🟡 Spell check: pure-Rust движок Hunspell-формата (`lumen_core::spell::HunspellDictionary`, .aff/.dic сабсет: TRY/SFX/PFX + cross-product, check/suggest/locale, 2026-07-02) — реальные ru/en словари, подключение к contenteditable/forms и squiggly-подчёркивание ⬜. ⬜ no horizontal scroll.

**Privacy/Shields** — ✅ shields toolbar + panel, privacy panel, fingerprint config (`fingerprint.toml`), Tor mode (`--tor`/`--tor-port` → SOCKS5 + Tor profile + no-persistent-state), per-origin Web Storage.

**DevTools (in-app)** — ✅ JS console panel, DOM inspector (Computed + Styles tabs), network log panel.

**Lifecycle/Performance** — ✅ tab tiers (T1 active / T2 background-old / T3 hibernated, badges), restore spinner + sleep hints, cross-restart session persist, `content-visibility: auto` ratchet, persistent QuickJS (timers/observers/navigation under `--features quickjs`), memory-pressure poll + GC tick, download manager, OS notifications, system-font fallback chain.

**Automation surfaces** — ✅ `--devtools-port` (CDP), `--bidi-port` (BiDi — real navigate/eval/captureScreenshot/input against a live window, see SDC-2 below), `--mcp-live-port <N>` (MCP over TCP against a live window, SDC-2), headless `--dump-source`/`--dump-layout`/`--dump-display-list`, **`--screenshot <out.png> <url>`** (full-page deterministic CPU snapshot via `cpu-render`, no window/Edge/ffmpeg), **`--ipc-server`** (headless tab-control IPC: `CreateTab`/`NavigateTab`/`Screenshot`/`CloseTab` over TCP loopback, PNGs without gdigrab — TAB-4/5), `--print-to-pdf`; **live-window automation channel** (SDC-1b/SDC-2, 8A.7 Ф4): `AutomationCommand`/`AutomationReply` (`lumen-driver`) drained each frame in the shell's `ApplicationHandler`, each request carrying its own reply channel (`AutomationRequest`, SDC-2 — no more replies vanishing into an unread channel) — `Navigate`/`Click`/`Type`/`Scroll` (incl. `Target::Selector` resolution via `lumen_layout::selector_query`) act on the live tab; `Eval` returns the real JS value as JSON; `Screenshot` renders the current content display-list to PNG (CPU path); `Query`/`A11yTree` (SDC-2) return DOM matches / the accessibility tree; `Wait` polls once per frame without blocking the event loop. `lumen_driver::LiveWindowSession` (SDC-2) wraps this channel as a full `BrowserSession` impl — the same trait `InProcessSession`/`WinitSession` implement — so `lumen-bidi-server` and `lumen-mcp` drive a real window with no protocol-specific glue. Four follow-up fixes found by manually driving a real window end-to-end (click→navigate scenarios over both MCP and BiDi): (1) `AutomationCommand::Navigate` built `PageSource::Url(url)` unconditionally, so `file://` URLs (used by every non-web-facing automation caller — graphic_tests, most BiDi/MCP test clients) hit the network `HttpClient` and failed with "unsupported scheme: file"; it now parses `file://`/bare paths into `PageSource::File` (`page_source_for_automation_url`). (2) A command enqueued from a BiDi/MCP thread had no way to interrupt a parked `ControlFlow::Wait` event loop (no OS event/timer/redraw is inherently triggered by an unrelated thread's `mpsc` send) and could sit undrained indefinitely; `AutomationHandle` now carries a wake callback (`set_wake`, shared via `Arc<Mutex<..>>` so handles created before the window exists still pick it up) that the shell wires to `EventLoopProxy::send_event(LoadEvent::AutomationWake)` once the loop is running. (3) `Click`/`Type` resolved a `NodeId`/`Selector` target to *page*-space coordinates but fed them into `handle_click_at`, which expects *OS window*-space (real OS mouse events) — a click landed wherever page coordinates happened to fall in window space, "working" only by scroll-zero coincidence; `resolve_automation_target` now applies the inverse of `page_point()`. (4) `Target::Point` (BiDi `input.performActions` pointer coordinates, MCP `click{point}`) was passed straight through unconverted — off by exactly `TAB_BAR_HEIGHT` (36px) since pointer coordinates are in *content-viewport* space (the same space `captureScreenshot` renders — no scroll subtraction needed, unlike `NodeId`/`Selector`) — now gets the tab-bar/panel offset added. Verified end-to-end via both protocols: navigate → click a real `<a href>` (by selector, and separately by raw pointer coordinates read straight off a screenshot) → real cross-page navigation, confirmed by screenshot/`a11y_tree`/`query()`.

### lumen-driver (`crates/driver`) — headless engine interface
- ✅ `BrowserSession` trait: 6 resources (screenshot/a11y_tree/layout/computed_style/network_log/console_log) + 6 tools (navigate/click/type/scroll/wait/eval/query); `InProcessSession` full headless pipeline; simple selector engine (tag/#id/.class); deterministic CPU snapshot (`screenshot_cpu_rgba/png`, cross-OS-identical, 57-page gate).
- ✅ `WinitSession` (SDC-1a, 8A.7 Ф4 driver-side): `click`/`type_text` implement headless DOM-level semantics (click follows `<a href>` / toggles checkbox-radio `checked`; type_text writes `value`); `eval(js)` runs a one-shot QuickJS runtime bound to a snapshot of the current DOM under `--features quickjs` (returns a JSON-string result); `AutomationCommand`/`AutomationReply` published from `lumen-driver`, consumed by the live shell window (SDC-1b, done — see Automation surfaces above).
- ⬜ `InProcessSession`'s own click/type/scroll/wait/eval remain headless-pipeline stubs (no persistent JS runtime there); `WinitSession.eval()` without `--features quickjs` still errors; auto-wait combinators/pseudo-selectors deferred; `WinitSession` itself has no native OS-level input dispatch (that lives only in the shell window's own code path, not this headless session).

### lumen-devtools (`crates/devtools`) — CDP-over-WebSocket (Phase 0 minimal)
- ✅ RFC 6455 WebSocket (handshake, frames, close/ping/pong, 1 MB guard); CDP `Browser.getVersion` (real), `DOM.getDocument` (stub), `*.enable` ACKs.
- ⬜ Real DOM tree, computed styles, Network events, Debugger domain, WSS.

### lumen-a11y (`crates/engine/a11y`)
- ✅ `build_ax_tree` over Shadow-DOM composed tree (`aria-hidden` pruned); 67 ARIA roles + implicit mapping; accessible name/description (WAI-ARIA §4); full state set + relationships; shell pushes tree after load/restore + focus-change on click.
- ⬜ Platform bridges (Windows UIA / macOS NSAccessibility / Linux AT-SPI) are **in-memory stubs** (no real OS bindings yet); live-region timing deferred.

### lumen-bidi-server (`crates/bidi-server`) — standalone WebDriver BiDi
- ✅ Rich protocol state machine: session.*, browsingContext.* (create/close/navigate/getTree/captureScreenshot/setViewport/handleUserPrompt), script.* (evaluate/callFunction/preloadScript/getRealms), network.* (intercept/continue/fail/getResponseBody), input.performActions, storage.*, browser.*, emulation.setUserAgentOverride; event emission for context/storage/network.
- ✅ **SDC-2 live wiring**: when `--bidi-port` is combined with an open window, each connection's `BidiState` holds a `LiveWindowSession` (`BidiState::with_live_session`) — `browsingContext.navigate`, `script.evaluate` (primitives get a real BiDi `RemoteValue`; objects/arrays fall back to their JSON text), `browsingContext.captureScreenshot` (base64 PNG), and the pointer-click/key-input subset of `input.performActions` execute for real against the live shell window.
- ⬜ Without a live window (`--bidi-port` alone, e.g. with `--dump`/`--mcp`), or for commands outside the SDC-2 MVP set (network interception, cookie events, full input-action-chain fidelity, `domContentLoaded`), state stays **in-memory `BidiState` only** — full wiring is an 8H.3 handoff.

### lumen-mcp (`crates/mcp`) — Model Context Protocol server
- ✅ Wraps `BrowserSession`; `resources/list+read` (screenshot/a11y_tree/layout/console/network), `tools/list+call` (navigate/click/type/scroll/wait/eval/query).
- ✅ **SDC-2 live wiring**: `lumen --mcp-live-port <N> <path-or-url>` opens a real window and serves MCP over TCP against `LiveWindowSession` — `screenshot`/`eval`/`query`/`a11y_tree`/navigate/click/type/scroll/wait return real results instead of `Err`.
- ⬜ Headless `--mcp`/`--mcp-port` (`InProcessSession`) unaffected by SDC-2: `screenshot`/`eval` still `Err` there (no persistent JS runtime, no GPU-backed rasterizer in that path); `LiveWindowSession`'s `layout_snapshot`/`computed_style`/`network_log`/`console_log`/fingerprint-isolation methods are local stub defaults — not yet threaded through the automation channel.

---

## Known doc-drift
All drift items found while building this file were fixed in the 2026-07-02 docs sweep
(`p5-docs-cleanup`). When curated docs and this file disagree, **trust this file + code**
and log the drift here.
