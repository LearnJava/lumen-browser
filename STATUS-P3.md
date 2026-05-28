In progress: —
Next step: Pick next task from Queue (Wave 2/3)

Recent (Wave 2/3 - automation API + completed tasks):
  - ✓ fts-omnibox (Wave 2): HistoryFts + SearchHistory integration in shell: @history prefix, dropdown, prefix-match, SearchHistory recording 2026-05-28
  - ✓ http2-client (5A.4 ext, concurrent streams): send_request/read_response_for_stream, StreamState mgmt, +5 tests 2026-05-28
  - ✓ tab-lifecycle-invariants (10B/10C/10D, ADR-008): all three invariants verified/implemented:
    · 10B (DOM arena): NodeId(u32) arena, Serialize/Deserialize, to_bytes/from_bytes ✓
    · 10C (JS suspend): pause/unpause/suspend/resume + SuspendedHeap struct ✓
    · 10D (pure-layout): audit complete, no hidden state, cache isolation ✓ 2026-05-28
  - ✓ invariant-2-js-suspend (10C, JsRuntime trait): pause/unpause/suspend/resume methods + SuspendedHeap struct for T2→T3 hibernation 2026-05-28
  - ✓ invariant-1-dom-arena (10B, [P3+P1], audit complete): NodeId(u32) arena verified, to_bytes/from_bytes tests exist, clippy::rc_buffer enforced 2026-05-28
  - shell-as-driver-client-8a7-phase4c (reload() migration to GpuSession: WinitSession integration in shell, backward-compatible fallback for Snapshot) 2026-05-28
  - bench-ram-axis (9G.5, Phase 1 complete: peak_rss + steady_state_rss tracking in lumen-bench, compare.py update, baseline.json restructure) 2026-05-28
  - samples-heavy-html (10M, Phase 1 complete: Habr-style benchmark page with 35+ posts, sidebar, sticky header, 1258 lines, ~2273 DOM nodes) 2026-05-28
  - antidetect-http-fingerprint-9c (Phase 1 complete: HttpProfile enum + header order + build_request_headers + fingerprint_profile getter/setter + 6 integration tests) 2026-05-28
  - tab-state-machine-10a (Phase 1: TabState enum T0-T4 + state machine + transitions + idle timeout + memory pressure triggers + 5 tests) 2026-05-28
  - antidetect-tls-fingerprint-9b (Phase 1: TlsProfile enum + build_client_config + JA3 snapshot CHROME_130 + 10 tests) 2026-05-28
  - auto-wait-engine-8d (Phase 1: polling-based wait_for(Visible/Stable/NetworkIdle/JsIdle) in InProcessSession + WinitSession, 6 unit tests) 2026-05-28
  - antidetect-surface-api (9A, Phase 1 complete: code-review audit + 8 negative tests for navigator.webdriver/chrome.runtime/cdc_*/__playwright/window.devtools absence) 2026-05-28
  - a11y-tree-via-driver (8G, Phase 1 complete: AxQuery enum + query_a11y/query_a11y_all + Role/NameContains matching + lumen-a11y integration) 2026-05-28
  - deterministic-mode-8f (Phase 1 complete: frozen_clock_ms / rng_seed / freeze_fingerprint in SessionContext + BrowserSession trait + 9 methods + 5 tests) 2026-05-28
  - per-context-isolation-8e (Phase 1 complete: SessionContext + FingerprintProfile + cookies/storage/HTTP cache + 13 tests) 2026-05-28
  - native-input-injection-8c (Phase 1 skeleton: InputCommand enum + click/type_text + shell/src/input.rs) 2026-05-28 
  - lumen-mcp-server-8b (Model Context Protocol transport, 5 resources + 7 tools, stdio server) 2026-05-28
  - shell-as-driver-client-8a7-phase4b (GpuSession impl + render_to_gpu) 2026-05-28
  - graphic-tests-migration-8a6 (50 Rust tests + PNG snapshots in crates/driver/tests/) 2026-05-28

Phase 4 (shell-as-driver-client): completed 4a-4b-4c
  ✓ 4a: GpuSession trait created (RenderedPage, gpu_session.rs)
  ✓ 4a: WinitSessionState extended with GPU data (display_list, title, images, font_registry)
  ✓ 4b: Implement render_to_gpu() method in WinitSession (build_display_list, extract title/images)
  ✓ 4b: GpuSession trait impl for WinitSession (set_scroll, viewport, navigate_streaming)
  ✓ 4b: Helper functions (extract_title, extract_images, walk_find_title, walk_collect_images)
  ✓ 4b: Tests for GpuSession (test_gpu_session_render_to_gpu, scroll_position, viewport)
  ✓ 4c: Migrate lumen-shell::reload() to use render_to_gpu() (reload_via_gpu_session() method, WinitSession integration, backward-compatible fallback for Snapshot)

Recent: shell-as-driver-client-8a7-phase4b (GpuSession impl for WinitSession) 2026-05-28

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P3 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next: — (старые runtime задачи P3 переданы P1 2026-05-27 → STATUS-P1.md; новые задачи ниже из ADR-006/007/008 остаются за P3)

Note: см. также STATUS-P1.md — туда переданы прежние runtime задачи P3 (Wave 1 строка 22, Wave 2 строки 44-45, Phase 2+ строки 47-100). Ниже — новые задачи P3 из ADR-006 (automation), ADR-007 (anti-detection), ADR-008 (tab lifecycle).

Note (2026-05-28): fts-omnibox из STATUS-P1.md Wave 1 (P3-задача) переместили в P3 Queue ниже, т.к. это домен P3 (knowledge/omnibox). P1 берёт следующее из своих Phase 1 задач (lumen-a11y-full).

Queue (Wave 2 — приоритет на тестирование изнутри + tab-lifecycle инварианты, см. lumen-plan.md §6.11 / §11.4 / §15 / ADR-006 / ADR-008):
- ✓ fts-omnibox (Wave 1, переместилась из P1 в P3 домен): `lumen-knowledge::HistoryFts` + omnibox `@history` prefix + dropdown + ArrowUp/Down + SearchHistory recording. Porter-stemmer для RU отложен на Phase 2. 2026-05-28
- lumen-driver-trait (8A.1+8A.2): новый крейт crates/driver/ с trait BrowserSession (resources: screenshot, a11y_tree, layout, computed_style, network, console; tools: navigate, click, type, scroll, wait, eval, query) + InProcessSession impl, переиспользующая существующий pipeline lumen-shell. Высокий приоритет: открывает уровни 2-3 тестирования (§15) — структурные ассерты и in-process snapshot для graphic_tests/. Без этого тесты остаются завязаны на ffmpeg/gdigrab/Edge и Windows-only.
- off-screen-render (8A.3, [P3+P2]): Renderer::render_to_image() -> Image без winit-окна. Координация с P2 — нужен отдельный wgpu surface path.
- structural-getters (8A.4, [P3+P1]): pub accessors на LayoutBox / ComputedStyle по селектору в lumen-layout. Координация с P1.
- ✓ graphic-tests-migration (8A.6): 50 Rust-тестов + PNG-снимки в crates/driver/tests/, structural-ассерты по COVERAGE.md. Старый run.py = уровень 4 (cross-browser vs Edge). Завершено 2026-05-28.
- shell-as-driver-client (8A.7): переписать lumen-shell как первого клиента BrowserSession — winit/wgpu становятся одним из транспортов, не центром.
- bench-gate-ci (9G.3): CI workflow `.github/workflows/bench-gate.yml` — `cargo run -p lumen-bench --release` + `bench/compare.py` vs `bench/baseline.json` → fail PR при регрессе >5% median/p95. Применяется к PR, затрагивающим lumen-driver / lumen-mcp-server / lumen-bidi-server / lumen-network / lumen-canvas / lumen-js / lumen-storage::profiles / lumen-shell::input. Делать **рано** — защищает все Wave 3/4 задачи automation и anti-detection от тихих регрессий. Binding по ADR-006 §«Performance gate» и ADR-007 §«Performance gate».
- bench-baseline-procedure (9G.4): `bench/UPDATE.md` процедура обновления baseline.json (после accepted-регрессии, с архитектурным обоснованием в commit body).
- ✓ bench-ram-axis (9G.5): расширить lumen-bench: добавить peak_rss / steady_state_rss / tier_transition_rss замеры; CI gate фейлит PR при >5% регрессе RAM или >20% регрессе любого tier-transition. Binding по ADR-008 §«Performance gate». Делать **рано** — критично для всех последующих T0-экономия задач. **Phase 1 complete 2026-05-28**: peak_rss/steady_state_rss impl + compare.py update + baseline.json restructure.
- tab-lifecycle-invariants (трек 10B+10C+10D): ТРИ архитектурных инварианта по ADR-008, должны быть приняты ДО Phase 1 finalize соответствующих крейтов (иначе ретрофит 5-10×). Каждый — отдельная coordination-задача:
  * **invariant-1-dom-arena (10B, [P3+P1])**: audit lumen-dom — убедиться что node graph на NodeId(u32) без Rc<RefCell>; добавить bincode::serialize/deserialize для DOM snapshot; clippy lint запрещает Rc<RefCell> в lumen-dom::node модулях. **P1 owns lumen-dom**, P3 координирует через коммит с обоснованием.
  * **invariant-2-js-suspend (10C)**: расширить lumen-core::ext::JsRuntime trait методами pause()/unpause()/suspend()/resume(); имплементация для rquickjs через JS_WriteObject/JS_ReadObject; zstd-сжатие snapshot; cap 5 MB/tab disk. V8 compatibility note для Phase 3.
  * **invariant-3-pure-layout (10D, [P3+P1+P2])**: audit lumen-layout (P1) и lumen-paint::display_list (P2) на отсутствие static MUT / lazy_static / OnceCell внутри hot path. Cross-tab кэши (glyph atlas, image decode) — отдельные крейты с explicit eviction API.
- http-profiles-expansion (9C Phase 1.x): добавить профили браузеров за Chrome (следует за Wave 2 http-fingerprint):
  * firefox: Firefox 130+ SETTINGS, header order, TLS fingerprint (JA3 snapshot)
  * safari: Safari 18+ SETTINGS, minimal headers (Sec-* subset)
  * edge: Edge 130+ SETTINGS = близко к Chrome, но с отличиями в alpn/extensions
  * tor-native: Tor Browser SETTINGS + заголовки (перейти с текущего Tor на TorBrowser-matched)
  * lumen-native: собственный профиль Lumen (уже добавлен, оптимизированный для lightweight)
  Binding: ADR-007 §«Per-profile HTTP configs» + дешёвая работа (переиспользует SETTINGS/TLS/Headers инфраструктуру).
- preconnect-hints: обработать <link rel=preconnect> из preload_scanner — открыть TCP+TLS соединение заранее

Queue (Wave 3 — Automation Phase 1 + Anti-detection Phase 1 + tier mechanics, см. ADR-006 + ADR-007 + ADR-008):
- lumen-mcp-server (8B): новый крейт crates/mcp/ — Model Context Protocol over stdio + UNIX/TCP socket. Resources: screenshot, a11y_tree, layout, console, network. Tools: click, type, scroll, navigate, wait, eval, query. CLI: lumen --mcp / --mcp-port N. Phase 1.
- native-input-injection (8C, [P3+shell]): mouse/keyboard события идут через event loop тем же путём, что winit-события ОС — НЕ через JS dispatchEvent. event.isTrusted = true. Phase 1.
- auto-wait-engine (8D): wait_for(Cond::Visible / Stable / NetworkIdle / JsIdle) на тиках layout / network / shell runtime. Заменяет SDK retry-loops. Phase 1.
- per-context-isolation (8E): BrowserSession изолирована по умолчанию (cookies/storage/cache/viewport/UA/fingerprint per session). Phase 1.
- deterministic-mode (8F): set_clock / set_rng_seed / freeze_fingerprint — repeatable tests. Опирается на anti-fingerprinting §9.5. Phase 1.
- a11y-tree-via-driver (8G, [P3+P1]): BrowserSession::a11y_tree() читает snapshot из lumen-a11y (P1 owns construction); Query::Role { role, name } matching (Playwright-стиль getByRole). Phase 1, зависит от P1 lumen-a11y.
- ✓ antidetect-surface-api (9A): audit JS bindings на отсутствие automation hooks (navigator.webdriver / chrome.runtime / cdc_* / __playwright / etc.) + negative tests в integration-suite. Phase 1, по сути уже встроено архитектурно через ADR-006 — но нужны тесты-стражники. Завершено 2026-05-28.
- antidetect-tls-fingerprint (9B): cipher suite ordering + extension list + supported groups + ALPN order в rustls matching current Chrome. JA3/JA4 snapshot test против Chrome (обновляется per Chrome major release). Per-profile TLS config (Standard / Strict / Tor). Phase 1.
- antidetect-http-fingerprint (9C): HTTP/1.1 header order + casing matching Chrome; HTTP/2 SETTINGS frame values matching Chrome; HTTP/2 stream priority pattern; Accept-Language default `en-US,en;q=0.9`; Client Hints handling per-profile. Phase 1.
- behavioral-input-humanlike (9E, opt-in для automation API): InputMode::HumanLike в native input — Bézier mouse paths + Gaussian inter-keystroke timing + pre-click dwell. ДЛЯ ТЕСТИРОВЩИКОВ, не stealth-фича. Phase 1.
- tab-state-machine (10A): TabState enum {Active, BackgroundRecent, BackgroundOld, Hibernated} + state machine в lumen-shell::tab_lifecycle; OR-of-conditions trigger (idle timeout + memory pressure + LRU within budget); per-user конфиг таймаутов. Phase 1.
- memory-pressure-source (10H): новый trait MemoryPressureSource в lumen-core::ext с enum {Low, Medium, High}; OS-impls — Win32 (QueryMemoryResourceNotification + MEMORYSTATUSEX polling), Linux (/proc/pressure/memory PSI events ≥ kernel 4.20), macOS (dispatch_source_create MEMORYPRESSURE); подписка кэшей (image, glyph, layer) на события. Phase 1.
- image-decode-handle-cache (10E): ImageDecoder::decode возвращает ImageHandle (тонкий ref), не DecodedImage; ImageDecodeCache с LRU + memory budget 256 MB default; viewport-gating в layout (decode только для bounding box ∈ viewport ± 2 экрана); scroll-discard при удалении на >3 экрана. **Главный источник T0 RAM-экономии** (картинка 1920×1080 = 8 MB RGBA; страница с 30 картинок без gating = 240 MB только на images). Phase 1.
- t1-pause (10A.x): JS event loop pause/unpause при hide/show вкладки. Простая часть tier-модели, опирается на 10C invariant-2-js-suspend. Phase 1.
- samples-heavy-html (10M): samples/heavy.html — Habr-style тестовая страница (~150 MB target) для T0-heavy бенчей. Phase 1 (нужна для baseline).

Queue (Wave 4 — Automation Phase 2 + Anti-detection Phase 2 + T2/T3 hibernation):
- lumen-bidi-server (8H): новый крейт crates/bidi/ — WebDriver BiDi subset over WebSocket. CLI: lumen --bidi-port N. Ship BiDi-gaps как built-in (см. ADR-006): full response body, locale/timezone/offline emulation, per-context UA, viewport-before-popup, preload-script per-context, full download lifecycle, cookie change events, per-origin storage clear, дешёвая network interception. Gap-mapping документировать в subsystems/lumen-bidi-server.md. Phase 2.
- antidetect-rendering-fp (9D, [P3+P2]): Brave-style canvas randomization (per-session seed) + WebGL renderer/vendor normalization + AudioContext noise + Battery API disable on Strict + WebRTC mDNS-only + hardware concurrency/screen/timezone normalization per profile. P2 owns canvas/paint side; P3 owns JS bindings. Phase 2.
- antidetect-profiles (9F): объединённый профильный конфиг fingerprint (объединяет TLS + HTTP + rendering слои) в lumen-storage; BrowserSession::set_fingerprint_profile() per-context override (связка с 8F.3); Tor-mode профиль отдельной задачей (9F.3 — Phase 3). Phase 2.
- antidetect-redlines-ci (9G): CI-lint запрещает имена *captcha* / *solver* / *ip_rotation* / *proxy_pool* в crate-names; маркетинговый-words линтер на README. Чтобы случайный PR не нарушил ADR-007 red lines. Phase 2 (или раньше, дёшево).
- service-workers: Service Worker API (fetch intercept + cache API + background sync); Phase 2
- push-api: Web Push + Notifications API (VAPID, push subscription); Phase 2
- profiles-system: multi-profile — отдельные хранилища cookies/history/storage per profile; Phase 2
- ime-input: IME ввод для CJK/русского через OS compositor API (winit CompositionEvent); Phase 2
- tab-t2-snapshot (10I): T2 переход — async-save JS heap snapshot в SQLite (lumen-storage::tab_snapshot); async-load при T2→T0 с indeterminate UI hint если > 100 ms. Phase 2.
- tab-t3-hibernate (10J): T3 переход — DOM serialize через bincode+zstd в SQLite; в RAM только TabMetadata {url, title, scroll, favicon} <200 KB target; restore — deserialize + re-run scripts + full layout+paint, target ≤ 1500 ms. Phase 2.
- gpu-layer-lru (10F, [P3+P2]): LayerCache с LRU + GPU memory budget; texture pool recycling (одна wgpu::Texture для разных layers). P2 coordination. Phase 2.
- glyph-atlas-eviction (10G, [P3+P2]): LRU eviction редко используемых глифов из атласа. P2 coordination. Phase 2.
- tab-strip-tier-affordance (10K): UI индикация tier'а в tab strip — иконка "Z" / fade-opacity на T2/T3 tabs; tooltip с tier-info; loading-spinner при restore > 200 ms. Phase 2.
- js-gc-per-tier (10L): JS heap GC tuning per tier — мягкий GC для активной, агрессивный для idle. Через QuickJS thresholds API. Phase 2.

Queue (Wave 5+ — opt-in, по реальному спросу):
- lumen-cdp-shim (8I): Chrome DevTools Protocol subset как thin shim поверх BrowserSession — ТОЛЬКО при реальном named demand от legacy Puppeteer-проекта. До этого CDP-кода в Lumen нет (ADR-006 «Graduation triggers»). Phase 3+.
- devtools-protocol: Chrome DevTools Protocol (CDP) для собственного DevTools UI (5C) — отделить от lumen-cdp-shim, это разные задачи.

Recent: bench-baseline-procedure (9G.4: expanded bench/UPDATE.md with detailed procedures, examples, troubleshooting) 2026-05-28, bench-gate-ci (9G.3: CI workflow + baseline.json + compare.py for 5% regression gate) 2026-05-28, shell-as-driver-client-8a7-phase1-3 (WinitSession Phases 1-3: BrowserSession trait impl + resource access + interaction methods + 40+ tests) 2026-05-28, tinyskia-cpu-raster (tiny-skia CPU rendering feature, 3 tests, deterministic pixels CI) 2026-05-27, structural-getters (layout_box_by_selector + all_layout_boxes_by_selector + 3 tests) 2026-05-27, mock-http-client (MockTransport для перехвата HTTP в тестах, 5 unit + 4 integration tests) 2026-05-27, off-screen-render (Renderer::render_to_image + InProcessSession::screenshot PNG) 2026-05-27, lumen-driver-trait (BrowserSession trait + InProcessSession headless impl, 12 tests) 2026-05-27, fts-omnibox (OmniboxSuggestion + @history prefix + dropdown + ArrowUp/Down + SearchHistory recording) 2026-05-27, sandbox-dom-apply (IframeInfo + collect_iframes + check_popup_gate + shell-гейты) 2026-05-27, find-in-page-regex (Ctrl+R regex mode + collect_visible_text + TextFragment matching) 2026-05-27, mixed-content-enforcement (classify_subresource_request в HttpClient) 2026-05-27, click-hint-overlay (F + hint-key vimium-style kbd-навигация) 2026-05-27, http-tls-client (BrotliContentDecoder + Ctrl+L адресная строка для URL-навигации) 2026-05-27, sop-enforcement (postMessage targetOrigin check + CookieProvider + document.cookie) 2026-05-27, rendering-steps-order (spec-correct render loop order + PerformanceObserver + paint timing) 2026-05-27, shadow-dom-js (Element.attachShadow, shadowRoot, customElements.define/get/whenDefined, lifecycle callbacks) 2026-05-27, no-scrollbar-flag (--no-scrollbar CLI флаг для screenshot-пайплайна) 2026-05-26, observers-api (MutationObserver + ResizeObserver + IntersectionObserver + getBoundingClientRect) 2026-05-26, raf-js (requestAnimationFrame / cancelAnimationFrame) 2026-05-25, dom-dirty-relayout (layout invalidation after JS DOM mutations) 2026-05-25, timers-async (setTimeout/setInterval/scheduler.postTask) 2026-05-25, web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25
