# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress

—

---

## Next

### PH1 — Phase 1: v0.1 «Reader»

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH1-8 | **Preload scanner** (HTML LS §13.2.6.4.7) — отдельный pre-parser стартует fetch до DOM construction; P1 — отдельный mode tokenizer-а поверх существующего `scan_preload_hints` | M | `lumen-html-parser`, `lumen-shell` |
| PH1-9 | **lumen-mcp-server крейт** — Model Context Protocol over stdio/UNIX socket; Resources: screenshot, a11y_tree, layout, console, network; Tools: click, type, scroll, navigate, wait, eval; `lumen --mcp` / `lumen --mcp-port N` | L | `lumen-shell` |
| PH1-10 | **Auto-wait внутри движка** — `wait_for(Cond::Visible/Stable/NetworkIdle/JsIdle)` на тиках layout/network/JS, не retry-loop в SDK | M | `lumen-driver`, `lumen-shell` |
| PH1-11 | **Per-context isolation** — каждая `BrowserSession` изолирована (cookies/storage/cache/viewport/UA/fingerprint) | M | `lumen-driver`, `lumen-storage` |
| PH1-12 | **A11y tree first-class** — `lumen-a11y` крейт как primary locator surface; `BrowserSession::query(Role/Name/Text)` использует a11y-дерево, а не DOM-селекторы | M | `lumen-a11y`, `lumen-driver` |
| PH1-13 | **TabState + T0-T4 state machine** (трек 10A) — состояния T0 Active/T1 Paused/T2 SnapshotHeap/T3 Hibernated/T4 Recoverable, transitions по timer + memory pressure, per-user конфиг таймаутов | M | `lumen-shell`, `lumen-core` |
| PH1-14 | **Image decode cache LRU + viewport-gating** (трек 10E) — `ImageHandle` индирекция вместо прямых `DecodedImage`; decode только viewport ± 2 экрана; scroll-discard | M | `lumen-image`, `lumen-layout`, `lumen-paint` |
| PH1-15 | **T1 (paused)** — JS event loop pause/unpause при hide/show вкладки; `JsRuntime::pause()` / `unpause()` уже в трейте | S | `lumen-js`, `lumen-shell` |

### PH2 — Phase 2: v0.5 «Interactive»

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH2-1 | **HTTP/2** — h2 framing, HPACK header compression, server push, stream multiplexing; поверх существующего TLS (rustls); `quinn` или собственная реализация | XL | `lumen-network` |
| PH2-2 | **Site isolation** — origin-keyed process model, multi-process security boundary | XL | `lumen-shell`, `lumen-network` |
| PH2-3 | **Профили пользователя + шифрование** — user profiles (multiple identities), encrypted storage (AES-GCM per-profile key) | L | `lumen-storage`, `lumen-shell` |
| PH2-4 | **Anti-fingerprinting 6-слойный стек** (ADR-007) — surface API без automation-маркеров; TLS JA3 как у Chrome; HTTP/HTTP2 layer matching Chrome; Brave-style rendering fp; opt-in behavioral mimicry; профили Standard/Strict/Tor | L | `lumen-network`, `lumen-js`, `lumen-shell` |
| PH2-5 | **`<meta viewport>` parsing + page zoom (Ctrl+/Ctrl-)** — mobile viewport model; `initial-scale`, `width=device-width`; manual zoom in/out via Ctrl+/Ctrl- | M | `lumen-html-parser`, `lumen-layout`, `lumen-shell` |
| PH2-6 | **Shadow DOM + custom elements + `<template>` + `<slot>`** — Web Components; cascade + composed tree; `<template>` / `<slot>` tree-builder integration; declarative Phase 0 уже есть (`V-1`), нужен runtime JS API | L | `lumen-html-parser`, `lumen-dom`, `lumen-layout`, `lumen-js` |
| PH2-7 | **Accessibility tree + platform bridges** (полноценный runtime) — AX tree из DOM/layout + ARIA + focus model; UIA (Win32) / AT-SPI (Linux) / NSAccessibility (macOS); Phase 0 stubs готовы (`O-5`), нужен реальный runtime | L | `lumen-a11y`, `lumen-shell` |
| PH2-8 | **Forms runtime** — полный `ValidityState` + `validation pseudo-classes` + submission algorithm; file picker уже есть (`M-4`); autofill popup UI поверх storage; `<input type=date>` нативный пикер полный | L | `lumen-js`, `lumen-layout`, `lumen-shell` |
| PH2-9 | **IME composition events** — японский / китайский / корейский ввод; `compositionstart` / `compositionupdate` / `compositionend`; winit IME events → JS delivery | M | `lumen-shell`, `lumen-js` |
| PH2-10 | **mix-blend-mode / backdrop-filter / isolation** (P1 часть) — isolation groups в compositor pipeline; P1 — parsing + stacking model; P2 — paint pipeline + isolation groups (координация P1↔P2) | L | `lumen-layout`, `lumen-paint` |
| PH2-11 | **lumen-bidi-server крейт Phase 1** — WebDriver BiDi subset over WebSocket: `playwright.connect('ws://localhost:9222/session')` из коробки; `lumen --bidi-port N`; Phase 0 transport готов (`O-1`) | L | `lumen-shell` |
| PH2-12 | **BiDi-gaps как built-in** — то чего нет в W3C BiDi spec: full response body access, resourceType, locale/timezone/offline emulation, per-context UA, preload scripts, download lifecycle, cookie change events | M | `lumen-shell` |
| PH2-13 | **T2 (JS heap snapshot)** (трек 10I) — async-save при T1→T2 в SQLite; async-load с indeterminate UI hint при > 100ms; zstd compression; cap 5 MB/tab disk | M | `lumen-js`, `lumen-storage` |
| PH2-14 | **T3 (full hibernation)** (трек 10J) — DOM serialization через `bincode + deflate` в SQLite; в RAM остаётся только `TabMetadata` (URL, title, scroll, favicon) <200 KB/tab | L | `lumen-dom`, `lumen-storage` |
| PH2-15 | **GPU layer LRU + texture recycling** (трек 10F) — `wgpu::Texture` pool для off-viewport stacking contexts | M | `lumen-paint` |
| PH2-16 | **Glyph atlas LRU eviction** (трек 10G) — LRU eviction в glyph atlas при memory pressure; implements `EvictableCache` | S | `lumen-paint`, `lumen-font` |

---

## Recent merges

| Дата | Задача | Описание |
|------|--------|---------|
| 2026-06-15 | PH1-8: Preload scanner | `PreloadScanner` struct поверх `PushTokenizer`: инкрементальный scan каждого HtmlChunk вместо первых 8 КБ. `collect_hints_from_tokens` — общая логика без дублирования. Shell `start_streaming_load` обновлён: hints + CSS-загрузчики стартуют из каждого chunk-а. 35 тестов (27 batch + 8 streaming). |
| 2026-06-15 | PH1-7: Compositor thread + Property Trees | `InProcessCompositor` + `ThreadedCompositor` подключены в `session.rs` / `winit_session.rs`. `PropertyTrees::build()` вызывается после layout при каждой навигации и коммитируется в compositor. `scroll_page_by(dx, dy)` — off-main-thread scroll без relayout (обновляет `ScrollNode.offset`, recommit). 15 тестов в `test_compositor.rs`. |
| 2026-06-15 | PH1-6: Stacking contexts + CSS Painting Order | Подключён `build_display_list_ordered` (StackingTree + PaintOrder) к 4 точкам driver: `InProcessSession.screenshot()`, `screenshot_cpu_rgba()`, `display_list_for_compare()`, `WinitSession.screenshot()`. 3 новых теста в `test_stacking_order.rs` верифицируют CSS 2.1 Appendix E порядок по FillRect-цвету. |
| 2026-06-15 | PH1-5: Packages для Linux / macOS / Windows | `.github/workflows/ci.yml` — кросс-платформенная проверка (Linux/macOS/Windows) + unit-тесты 12 non-GUI крейтов; `.github/workflows/release.yml` — 4 бинарных пакета (linux-x86_64/macos-aarch64/macos-x86_64/windows-x86_64) → GitHub Release на тег v*.*.*. |
| 2026-06-15 | PH1-4: Network service в отдельном процессе | `lumen-ipc` крейт (IpcChannel/IpcServer/IpcClient, 4 теста); `RemoteNetworkTransport`; `lumen-network-service` бинарник; shell `--network-service` флаг + `NetworkServiceHandle::spawn()`. |
| 2026-06-15 | PH1-15: T1 (paused) | `pause_event_loop()`/`unpause_event_loop()` в `PersistentJs`; `QuickPersistentJs` делегирует `set_document_visibility()`; вызовы в `switch_tab` (T0→T1 и T1→T0); 6 тестов. |
| 2026-06-15 | PH1-2: Progressive / streaming rendering pipeline | 60 Hz throttle (16 мс); `LoadEvent::CssLoaded`; `load_css_for_streaming()`; параллельная загрузка CSS из EarlyPreloadHints; `stream_sheet` накапливает CSS для промежуточных кадров; 3 unit-теста. |
| 2026-06-14 | JJ-phase5: Modern HTML5 APIs Phase 5 | `checkVisibility(opts?)` (W3C Viewport API §4.1), `setHTMLUnsafe(html)`, `getHTML(opts?)` (WHATWG HTML LS §14.5), `moveBefore(node, child?)` (DOM LS / Chrome 133+); 11 тестов; 2014 всего в lumen-js. |
| 2026-06-14 | PH1-1: Font fallback / matcher | `resolve_font_chain` в FemtovgBackend: CSS font-family list → FontProvider → femtovg FontId цепочка; eager preload CURATED_FALLBACK_FAMILIES; DrawText подключает font_family/weight/style. |
| 2026-06-14 | P0-2: Pure layout + paint audit | Аудит: нет static mut/lazy_static/OnceCell в hot path; thread_local корректно сброшены; GlyphAtlas+ImageDecodeCache per-renderer; исправлен layout() — добавлен invalidate_rule_idx_cache(). |
| 2026-06-14 | P0-1: DOM arena audit | Аудит подтвердил: NodeId(u32) арена без Rc<RefCell>, to_bytes/from_bytes с 214 тестами. Добавлен compile-time Send+Sync gate (ADR-008 §11.4). |
| 2026-06-14 | II-2: WebAuthn platform HID enumeration Phase 1 | `platform_enumerate_ctap2_devices()` + `win_hid::enumerate()` (SetupDi + HidP_GetCaps фильтр FIDO_USAGE_PAGE) + `linux_hid::enumerate()` (hidraw0..31 + sysfs HID-дескриптор); inline FFI без новых зависимостей; 10 unit-тестов |
| 2026-06-14 | GG-5: Tab hibernation Phase 2 (LZ4) | `lz4_flex` compress/decompress для `js_heap_blob`; `compressed INTEGER` колонка + ALTER TABLE миграция; 5 unit-тестов; 582 итого в lumen-storage |
| 2026-06-14 | GG-4: Vertical tabs layout mode | `TabLayout::Horizontal/Vertical` enum; `VerticalTabsPanel`; `BrowserSettings.tab_layout` persist; 8 тестов |
| 2026-06-14 | GG-3: Privacy shields Phase 1 | `/regex/` поддержка в `EasyListFilter`; `DefaultFilterList` ~30 правил (Google Analytics, DoubleClick, Facebook и пр.); 50 filter-тестов |
| 2026-06-14 | GG-1: AI sidebar Phase 0 | `AiBackend` trait + `NullAiBackend`; `ai_panel.rs` 200px right-docked; `Ctrl+Shift+A`; 14 тестов |
| 2026-06-14 | FF-4: Cache API Phase 1 | `CacheBackend` trait в `lumen-core::ext`; `impl CacheBackend for CacheStorage` SQLite; 12 unit-тестов |
| 2026-06-14 | EE-5: rAF scheduling Phase 2 | vsync gate 16.67 мс; `has_raf_pending()` non-consuming peek; uniform `DOMHighResTimeStamp`; 5 тестов |
| 2026-06-14 | JJ-1..5: Modern HTML5 APIs Phase 4 | CloseWatcher, `<details name>` accordion, `showPicker()`, `popover="hint"`, `caretPositionFromPoint()`; 17 тестов |
| 2026-06-14 | GG-2: @notes omnibox Phase 1 | `OmniboxSuggestion::Note`; `NoteViewerPanel` floating overlay; `note-viewer:<id>` схема; 13 тестов |
| 2026-06-13 | BB-8: CSS Anchor Positioning Phase 1 | `AnchorScope`; `anchor-size()`; `resolve_inset_area_scoped()`; `apply_anchor_positions_rec`; 11 тестов |
