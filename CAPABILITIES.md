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
**same commit** as the `STATUS-PN.md` "Recent" update. This is the only file that must stay
true to code; keep it honest about ⬜ gaps too.

Snapshot: **Phase 2 «Interactive», app v0.2.0**. ~21 crates.

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
- ✅ `!important` extraction; at-rules parsed+stored: `@media` (cascade-integrated), `@font-face`, `@import`, `@property`, `@layer`, `@supports` (typed `evaluate()`), `@keyframes`, `@scope`, `@container`.
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
- ✅ Block + inline flow (line wrap, margin collapsing, `margin: 0 auto`).
- ✅ Flexbox (full: direction, grow/shrink/basis, justify/align, gap, wrap). ⬜ column-direction wrapping.
- ✅ CSS Grid (px/fr/auto/repeat/minmax, explicit+auto placement, dense, subgrid). ⬜ grid-template-areas, named lines.
- ✅ Table layout (colspan/rowspan, column widths) — live path `box_tree.rs` (note: `table.rs` is dead code).
- ✅ Positioned: relative, absolute/fixed (out-of-flow + containing-block threading); `position: sticky` partial (offsets computed, scroll wiring shell-side).
- ✅ SVG layout pass (viewBox, rect/circle/ellipse/line/path, `<use>` with cycle detection); vertical writing modes (`vertical-rl/lr`).
- ✅ Replaced: `<img>` (picture/srcset picker), `<iframe>` placeholder.
- ✅ Cascade: specificity + `!important`, RTL selector matching, all CSS3 structural + L4 form/UI pseudo, `:has()`, `::before/::after` (string content), `::first-line/::first-letter` (drop-cap float).
- ✅ Values: `calc/min/max/clamp` + math fns, `var()`, `@property` registration, viewport units, intrinsic sizing (`min/max/fit-content`).
- ✅ Animations/transitions scheduling (`@keyframes` interpolation, timing functions, transform/gradient/filter interpolation); `content-visibility: auto` skip; Shadow DOM flat-tree integration.
- ✅ Algorithm stubs awaiting P4 CSS wiring: anchor positioning, scroll-driven animations, subgrid context.
- ⬜ `float` (only first-letter drop-cap), `%` in margin/padding/width/height, `ch`/`ex` units, real `direction: rtl` reordering, CSS4 color spaces (lab/lch/oklab/oklch), `attr()`/`counter()` content. Many L3/L4 properties are **parse+store only** (text-emphasis, container queries, touch-action, appearance, resize, offset-path).

### lumen-paint (`crates/engine/paint`)
- ✅ **Live default render path is `FemtovgBackend`** (OpenGL ES via glutin), with wgpu auto-fallback; `LUMEN_BACKEND` overrides. **Paint bugs from graphic_tests are fixed in `femtovg_backend.rs`, not `renderer.rs`.**
- ✅ DisplayCommand primitives (all in enum + handled by femtovg): FillRect, FillRoundedRect (SDF), DrawBorder (solid/dashed/dotted/double), DrawText, DrawOutline, DrawImage (object-fit/position), Linear/Radial/Conic gradients (incl. repeating), SvgPath, clip, opacity, blend modes, transforms, filters, backdrop-filter, scroll layers, masks, layer snapshots, page breaks.
- ✅ Stacking contexts + paint order (CSS 2.1 Appendix E), stacking-aware hit testing (transform inversion).
- ✅ box-shadow (outset+inset), text-shadow, text-decoration (underline/overline/line-through, wavy/dotted/dashed/double, thickness), border-radius SDF.
- ✅ CSS filters (GPU color-matrix + Gaussian blur), backdrop-filter (LRU cache), clip-path (bbox approximation).
- ✅ 3D transforms (perspective, preserve-3d depth sort) in wgpu renderer; multi-size + variation-aware glyph atlas, per-char codepoint fallback cascade.
- ✅ Compositor scaffolding (two-buffer commit, threaded compositor, 60fps vsync); print (`render_print_pages` → images); CPU rasterizer (`cpu_raster.rs`, feature `cpu-render`, cross-OS bit-identical, snapshot gate); software WebGL 1.0 (flat fill, no GLSL).
- 🟡 femtovg gradient/image masks are **scissor bbox clip**, not true per-pixel alpha mask.
- ⬜ GPU shadow pipeline, Groove/Ridge/Inset/Outset borders, exact polygon clip-path, elliptical border-radius (rx≠ry), Vello backend (no-op stub).

### lumen-font (`crates/engine/font`)
- ✅ Table parsers (head/maxp/cmap fmt4+12 incl. SMP/emoji/hhea/hmtx/loca/glyf/name/OS2/post); rasterizer (simple + composite glyphs, 4×4 supersampling).
- ✅ Variable fonts runtime (fvar/avar/HVAR/VVAR/MVAR/gvar, IUP + deltas); font matching/fallback (`SystemFontIndex` scans OS fonts, weight/style matcher, per-char cascade); WOFF2 (Brotli) + WOFF1 (zlib) decode.
- ✅ **`font-display: swap` (PH3-19)**: `@font-face url()` sources fetched asynchronously off the critical paint path (FOUT). First paint uses Inter fallback; background thread fetch+decode → `FontLoaded` event → relayout with `MultiFontMeasurer` to swap in the web font. `local()` sources still loaded synchronously (no network round-trip needed).
- ⬜ **No shaping (GSUB/GPOS)** — no ligatures, kerning, Arabic/Indic. No CFF outlines (`.otf` PostScript won't render), no hinting, no color glyphs (COLR/CPAL/sbix), no bitmap strikes. Fallback covers only already-loaded faces.

### lumen-image (`crates/engine/image`)
- ✅ PNG, JPEG (baseline + progressive), WebP (VP8 + VP8L), **GIF** (static + animated), **AVIF** (behind `avif` feature).
- ✅ `resize_bilinear`, `ImageDecoder` trait, `ImageDecodeCache` (LRU 256 MB, `ImageHandle`/`ImageKey`).
- ⬜ JXL and HEIC are sniff-only Err stubs; ICC color pipeline not wired.

### lumen-canvas (`crates/engine/canvas`)
- ✅ Canvas 2D CPU rasterizer: rect ops, full path building (arc/arcTo/bezier/quadratic/ellipse), fill/stroke (even-odd), state stack + full CTM, `globalAlpha`, 16 composite/blend ops, line caps/joins.
- ✅ Gradients (linear/radial/conic), patterns (4 repeats), shadows (offset-only), `clip()` (boolean mask), image data (drawImage/putImageData/get/createImageData), text via `lumen_font::Rasterizer`, Path2D (SVG path strings).
- ⬜ Gaussian shadowBlur; gradient sampling is device-space (not spec user-space); canvas fingerprint noise.

---

## JS runtime & Web APIs

### lumen-js (`crates/js`) — QuickJS via `rquickjs` 0.11
Modern ES (ES2020+: classes, async/await, generators, Promise, Proxy, BigInt, modules) comes from QuickJS. ~90 Web-API modules wired by Lumen JS shims + `_lumen_*`/`__lumen_*` native bindings (`install_dom`, `lib.rs:502`).

- **DOM** — ✅ full read/write, querySelector(All) via real CSS3 engine, matches/closest, innerHTML, createElement, getBoundingClientRect (real layout), DOM mutation → auto relayout. Shadow DOM, Popover, `<dialog>`/CloseWatcher, inert, ElementInternals + CustomStateSet, DOMParser/XMLSerializer, SVG DOM, Sanitizer (Phase 0).
- **Events** — ✅ EventTarget (bubbling/capture/stopPropagation/composedPath), Mouse/Pointer/Keyboard/Drag events, Pointer Events L3 capture, Pointer Lock.
- **Networking** — ✅ fetch + Headers/Request/Response/AbortController (`.timeout/.any`), XMLHttpRequest, WebSocket, Server-Sent Events, URL/URLSearchParams. ⬜ WebRTC (mDNS-only stub, no IP leak), WebTransport (stub).
- **Graphics** — ✅ Canvas 2D (via `lumen_canvas`, flushed per frame), OffscreenCanvas, WebGL/WebGL2 (flat-shaded via software backend), Web Animations API (real interpolation). ⬜ GLSL execution, WebGPU (stub), toDataURL blank (anti-fingerprint).
- **Workers/Concurrency** — ✅ Web Workers (real threads, importScripts), SharedWorker, BroadcastChannel, Promise/microtasks + queueMicrotask, Web Locks, timers (setTimeout/Interval + precise wakeup), requestAnimationFrame, scheduler.postTask/yield.
- **Storage** — ✅ Web Storage (localStorage SOP-partitioned + persistent, sessionStorage per-load), Cookie Store, IndexedDB (full: stores/indexes/cursors/key ranges/autoIncrement, per-origin persist), Service Workers (lifecycle + persist; 🟡 fetch interception Phase 1 — on activate the SW script runs in a dedicated QuickJS thread, `FetchEvent`/`respondWith` dispatched by `ServiceWorkerInterceptor` on the network path, cache-first via the shared Cache API store; ⬜ no in-SW network fetch, so SW `cache.addAll()` precaching can't pull from network — only entries the page cached are served), StorageManager (OPFS stub), Cache API, Shared Storage (in-memory).
- **Media/Devices** — ✅ getUserMedia({audio}) + getDisplayMedia (live when provider installed; Win32 GDI capture), HTMLAudioElement (real playback), HTMLVideoElement (GIF), Picture-in-Picture + Document PiP, Web Speech TTS (OS), MediaSession, Clipboard, Geolocation (denied default). ⬜ WebHID/USB/Bluetooth/Serial/MIDI/WebXR/WebCodecs (NotSupported stubs), Web Audio (graph only, no DSP).
- **Observers/Timing** — ✅ MutationObserver, ResizeObserver, IntersectionObserver (drives loading=lazy), performance.now()/timeOrigin, Navigation Timing classes + delivery. ⬜ general PerformanceObserver.
- **Misc** — ✅ WebAuthn/passkeys (ES256), SubtleCrypto (real), Trusted Types L2, CSP, Permissions Policy, Idle Detection (Win32), Wake Lock, File API + File System Access, Intl (ECMA-402 shim en-US/ru-RU), Temporal (shim), URLPattern, Navigation API, View Transitions, anti-fingerprint layer (ADR-007, deterministic mode). ⬜ Privacy-Sandbox (Topics/Attribution/Background Fetch/Push — in-memory stubs), WebAssembly (validates magic only, no execution), heap-snapshot serialization (shell re-runs scripts on restore).

> Boundary note: "functional" APIs (Canvas2D, WebGL, getUserMedia, WebSocket, XHR, IndexedDB, Web Animations) actually do work; many depend on the shell installing a provider — without it they degrade to rejection. The long stub list (WebGPU/WebCodecs/WASM-exec/WebHID/…) resolves/rejects without doing work.

---

## Networking & storage

### lumen-network (`crates/network`)
- ✅ HTTP/1.1 (keep-alive, connection pool), HTTPS (rustls 0.23 + webpki-roots, ALPN), **HTTP/2** (frame codec, HPACK, pool multiplexing, recv flow control).
- ✅ Brotli content-decoding (⬜ no gzip/deflate HTTP decoder); redirects (≤5), chunked decode, IDN→Punycode.
- ✅ Cookie jar wired into client (inject/persist per hop); CORS preflight + enforcement; Origin/Mixed-Content/Sandbox/CSP/COOP classifiers.
- ✅ HTTP auth (Basic + Digest MD5/SHA-256, 401 retry), Range requests, HSTS (+ preload), SOCKS5 proxy (proxy-side DNS, Tor-ready).
- ✅ DNS: system + DoH (RFC 8484) + DoT (RFC 7858); `RequestFilter` hook; fingerprint/TLS profiles (Chrome/Firefox/Safari/Edge/Tor/Lumen/Strict — header order, H2 SETTINGS, Client Hints).
- ✅ WebSockets (+ permessage-deflate), EventSource, Fetch bridge, software WebAuthn `VirtualAuthenticator` + CTAP2-over-HID (no USB enumeration).
- ⬜ Cache-Control revalidation, mTLS/client certs, `qop=auth-int`, CORS POST/PUT bodies, H2 send-side flow control.

### lumen-ipc (`crates/ipc`)
- ✅ Length-prefixed bincode over TCP loopback; `IpcChannel/Server/Client` blocking RPC; messages `Fetch/Ping/Shutdown`; powers out-of-process network service (`--network-service`).
- ⬜ Fetch is GET-only (no full method/headers/body yet).

### lumen-storage (`crates/storage`)
- ✅ SQLite everywhere (rusqlite bundled, WAL, prepared-cached); origin-partitioned KV `(origin, top_level_site, key)`.
- ✅ Cookie jar over SQLite (SameSite, partitioning, PSL), History, Bookmarks (folders/tags), Web Storage backend, IndexedDB store, Service Worker store + interceptor, Cache Storage.
- ✅ Profile vault encryption (AES-256-GCM + PBKDF2 100k); HttpCache (RFC 9111 basic), HSTS store, DnsCache, SafeBrowsing (local SB v4), PSL provider.
- ✅ Many stores: Downloads, Permissions, Autofill, Notifications, Workspaces, TabSessions/Snapshots, SiteEngagement, SearchHistory, TabGroups, PushSubscriptions, BFCache.
- ⬜ ADR-012 partitioning is **strategy only** — no DB manager; ~36 stores each open their own SQLite file. No schema-migration framework.

### lumen-knowledge (`crates/knowledge`)
- ✅ FTS5 history search (bm25, snippets, diacritics-folding), Notes (§12.2), Read-later (§12.3, status/tags), OpenTabsIndex (§12.4, in-memory).
- ✅ `KnowledgeStore` trait + `DefaultKnowledgeStore`; omnibox `@history` / `@notes` prefixes wired.
- ⬜ Local AI / vector (HNSW) index, Russian Porter stemmer, `@read-later`/`@tabs` prefixes.

---

## Shell, automation & accessibility

### lumen-shell (`crates/shell`) — the user-facing browser
**Navigation/Tabs** — ✅ load file/http(s)/local HTML with streaming incremental parse+paint + progressive image loading; link-click + fragment nav (`:target`); reload; tab strip + groups (colour-coded) + containers (cookie/storage isolation) + context menu + auto-archive; vertical tabs, tree tabs, workspaces, split view; `about:newtab` speed-dial; omnibox FTS suggestions (`@history`). ⬜ `@notes`/`@read-later` prefixes; history/search in-memory only.

**Reading/Content** — ✅ reader view, find-in-page (Ctrl+F, highlights/next-prev/scroll-to), source view, read-later panel, note viewer.

**UI panels** — ✅ command palette, settings, bookmarks, history, AI sidebar (Ctrl+Shift+A, `AiBackend` trait, `NullAiBackend` default), Picture-in-Picture (+ OS window), certificate viewer, permission popover, a11y/focus/sidebar panels, light/dark/system themes + accents.

**Input** — ✅ Vimium-style click hints, vim mode, gestures, human-like + native input injection, HTML5 drag-and-drop, forms runtime (validation + picker overlays), per-tab zoom, smooth scroll + scrollbar (drag + track-click) + momentum. ⬜ no horizontal scroll; no relayout-on-resize (viewport hardcoded 1024×720).

**Privacy/Shields** — ✅ shields toolbar + panel, privacy panel, fingerprint config (`fingerprint.toml`), Tor mode (`--tor`/`--tor-port` → SOCKS5 + Tor profile + no-persistent-state), per-origin Web Storage.

**DevTools (in-app)** — ✅ JS console panel, DOM inspector (Computed + Styles tabs), network log panel.

**Lifecycle/Performance** — ✅ tab tiers (T1 active / T2 background-old / T3 hibernated, badges), restore spinner + sleep hints, cross-restart session persist, `content-visibility: auto` ratchet, persistent QuickJS (timers/observers/navigation under `--features quickjs`), memory-pressure poll + GC tick, download manager, OS notifications, system-font fallback chain.

**Automation surfaces** — ✅ `--devtools-port` (CDP), `--bidi-port` (in-shell BiDi stub), headless `--dump-source`/`--dump-layout`/`--dump-display-list`. ⬜ headless PNG pixel-dump from shell (use driver CPU snapshot).

### lumen-driver (`crates/driver`) — headless engine interface
- ✅ `BrowserSession` trait: 6 resources (screenshot/a11y_tree/layout/computed_style/network_log/console_log) + 6 tools (navigate/click/type/scroll/wait/eval/query); `InProcessSession` full headless pipeline; simple selector engine (tag/#id/.class); deterministic CPU snapshot (`screenshot_cpu_rgba/png`, cross-OS-identical, 57-page gate).
- ⬜ GPU `screenshot()` returns Err; `eval(js)` returns Err (8A.7); full auto-wait + native input + combinators/pseudo deferred.

### lumen-devtools (`crates/devtools`) — CDP-over-WebSocket (Phase 0 minimal)
- ✅ RFC 6455 WebSocket (handshake, frames, close/ping/pong, 1 MB guard); CDP `Browser.getVersion` (real), `DOM.getDocument` (stub), `*.enable` ACKs.
- ⬜ Real DOM tree, computed styles, Network events, Debugger domain, WSS.

### lumen-a11y (`crates/engine/a11y`)
- ✅ `build_ax_tree` over Shadow-DOM composed tree (`aria-hidden` pruned); 67 ARIA roles + implicit mapping; accessible name/description (WAI-ARIA §4); full state set + relationships; shell pushes tree after load/restore + focus-change on click.
- ⬜ Platform bridges (Windows UIA / macOS NSAccessibility / Linux AT-SPI) are **in-memory stubs** (no real OS bindings yet); live-region timing deferred.

### lumen-bidi-server (`crates/bidi-server`) — standalone WebDriver BiDi
- ✅ Rich protocol state machine: session.*, browsingContext.* (create/close/navigate/getTree/setViewport/handleUserPrompt), script.* (evaluate/callFunction/preloadScript/getRealms), network.* (intercept/continue/fail/getResponseBody), input.performActions, storage.*, browser.*, emulation.setUserAgentOverride; event emission for context/storage/network.
- ⬜ All commands operate on **in-memory `BidiState` only** — real navigation/script/network execution needs 8A.7 (shell-as-driver-client).

### lumen-mcp (`crates/mcp`) — Model Context Protocol server
- ✅ Wraps `BrowserSession`; `resources/list+read` (screenshot/a11y_tree/layout/console/network), `tools/list+call` (navigate/click/type/scroll/wait/eval/query).
- ⬜ Inherits driver limits (screenshot/eval return Err with `InProcessSession`).

---

## Known doc-drift (found while building this file)
These curated docs lag behind code; **trust this file + code over them**:
- `subsystems/css-parser.md` omits `@supports`/`@keyframes`/`@scope`/`@container`; stale test counts.
- `subsystems/dom.md` omits the whole `contenteditable.rs` editing module.
- `subsystems/paint.md` marks femtovg backend as ⬜ Phase 2 though it is the compiled **default** live path.
- `subsystems/image.md` lists GIF/AVIF as deferred though both decode (GIF animated; AVIF feature-gated).
- `subsystems/js.md` curates ~30 APIs; code wires ~90.
- `subsystems/network.md` lists cookie jar + SOCKS5/Tor as deferred though both are wired.
- `subsystems/storage.md` omits Service Worker store + Cache Storage.
- `docs/plan/phases.md:31` describes streaming as unbuilt though PH1-2 shipped (PH1-2a TCP body streaming + PH1-2b incremental layout + PH1-2c progressive image loading all done — see STATUS-P1 "Recent merges").
