> **Справочник.** Высокоуровневый план фаз (Phase 0 — Phase N). Актуальный прогресс — в `STATUS-PN.md` и `docs/plan/status.md`. Этот файл читать только при планировании новой фазы.

## 16. Фазы разработки (реалистично)

### Фаза 0 — Прототип (3 месяца) ✅ закрыта 2026-05-26
- ✅ Workspace, base crates.
- 🟡 HTML parser — минимум готов (см. выше).
- 🟡 CSS parser — минимум готов (см. выше).
- ✅ DOM (арена + базовые типы).
- ✅ Layout: block-flow + word-wrapping (TextMeasurer + FontMeasurer).
- 🟡 Paint: FillRect через wgpu готов; глифы — позже.
- 🟡 UI: одно окно (готово), вкладки и адресная строка — нет.
- ⬜ HTTP/1.1 + HTTPS.
- ⬜ **Automation foundation (ADR-006, §6.11)** — критично для собственного тестирования, **не отложить на потом**:
  - **`lumen-driver` крейт** с trait `BrowserSession` и `InProcessSession`. Шелл переписать как первый клиент trait-а (окно/winit/wgpu становятся одним из транспортов, не центром).
  - **Off-screen рендер** в `lumen-paint` (`Renderer::render_to_image() -> Image`) для `session.screenshot()` без winit-окна.
  - **Software rasterizer для тестов** (`tiny-skia`, opt-in под `cfg(test)`) — детерминизм пикселей между Windows/macOS/Linux CI.
  - **Тестовая пирамида уровни 2-3 включены** (§15): структурные ассерты + in-process snapshot вместо текущей ffmpeg/gdigrab-схемы. Уровень 4 (vs Edge) переезжает в ночной job.
  - **Миграция `graphic_tests/`**: каждый из 22 текущих HTML-тестов получает (а) Rust-тест в `crates/lumen-driver/tests/` со структурными ассертами по `COVERAGE.md`, (б) PNG-эталон в `graphic_tests/snapshots/`.
- ⬜ **Tab lifecycle архитектурные инварианты** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)) — **обязательно до Phase 1 finalize**, иначе ретрофит 5-10×:
  - **Invariant 1: DOM arena** — `lumen-dom` audit: убедиться что node graph на `NodeId(u32)` без `Rc<RefCell>`; добавить `bincode::serialize` для DOM snapshot; clippy lint запрещает `Rc<RefCell>` в node-модулях (трек 10B).
  - **Invariant 2: JsRuntime suspend/resume API** — расширить trait в `lumen-core::ext::JsRuntime` методами `pause()` / `unpause()` / `suspend()` / `resume()`; имплементация для `rquickjs` через `JS_WriteObject`/`JS_ReadObject` (трек 10C).
  - **Invariant 3: pure layout + paint** — audit `lumen-layout` и `lumen-paint::display_list` на отсутствие `static MUT` / `lazy_static` / `OnceCell` внутри hot path; cross-tab кэши (glyph atlas, image decode) — отдельные крейты с explicit eviction (трек 10D).
- **Цель:** открыть простую текстовую статью без стилей. Доказательство концепции, **проверяемое из Rust без запуска отдельного процесса**, с зафиксированными tier-инвариантами для будущей лёгкости вкладки.

### Фаза 1 — v0.1 «Reader» (9 месяцев от старта) ✅ в основном выполнена
- **Базовая пригодность shell** — без этого «открыть Habr-статью» невозможно как демо:
  - **Font fallback / matcher.** Рендерер сейчас всегда `Inter Regular` — любая страница с эмодзи / CJK / `font-family: Roboto` падает в `?`-глифы. Минимум: системный font-loader (Win32 GDI / fontconfig / CoreText — без сторонних crate-ов), cascade «Inter → системный по unicode-блоку». Парсер `font-family` уже есть, не используется в paint.
  - **HiDPI / DPR-awareness.** ✅ paint-side: `Renderer` хранит `scale_factor` и делит viewport uniform на него (1 CSS px = `scale_factor` device px на 4K). ✅ Layout-side: viewport читается из `Renderer::viewport_size()` на каждый resize; `LayoutSource {document, stylesheet}` хранится в `Lumen` и переиспользуется для relayout без re-fetch/re-parse.
  - **Scroll + базовый input в shell.** Без scroll длинные страницы недоступны.
  - **Progressive / streaming rendering pipeline.** Сейчас shell блокирующий: окно создаётся **после** того, как HTML загружен, все `<link rel=stylesheet>` фетчатся **последовательно**, и только потом layout/paint. На странице с 30+ внешними CSS (Habr, любой современный сайт) пользователь смотрит в чёрный экран 5–15 секунд, после чего сразу появляется готовая страница. Это противоречит привычной модели браузера. Требуемая архитектура: (1) окно создаётся **первым**, до любых fetch-ей, пустое до прихода данных; (2) HTML fetch в фоновом потоке, chunks через channel в main thread; (3) tokenizer переделать на push-based (скармливаешь chunks — получаешь events), tree builder инкрементальный (новые узлы добавляются в существующий DOM); (4) subresources (CSS, картинки) фетчатся параллельно через thread pool / async; до прихода CSS — применяется UA stylesheet; (5) layout/paint reruns on dirty (relayout только поддерева, не всего дерева) с throttling до ~60 Гц. Касается shell + html-parser + network + layout. Большая задача, требует **архитектурного перепроектирования** main-loop shell-а и tokenizer-а. Прямо примыкает к «Network service в отдельном процессе» из той же фазы — оба про async-fetch, но streaming-парсинг и инкрементальный DOM из site isolation не следуют автоматически.
- **`Url` как структурированный тип** — `struct { scheme, host, port, path, query, fragment }`. Сейчас `Url` это тонкая обёртка над String, network ad-hoc парсит то же самое. Дедуплицировать парсинг до того, как появятся CSP / cookie jar / cross-origin checks. Несколько часов работы пока потребителей мало.
- ✅ **EventSink в network (network log).** `HttpClient::with_sink/with_tab` builder, эмит `RequestStarted` (после `parse_url`, до сокета) и `RequestCompleted` (после статус-строки, до анализа кода) — отдельная пара на каждый редирект-хоп. `StdoutEventSink` в shell печатает `→ GET <url>` / `← <status> <url>` / `✗ <url> (<reason>)`.
- ✅ **`RequestFilter` hook + `Event::RequestBlocked`.** `HttpClient::with_filter(Arc<dyn RequestFilter>)`: trait `should_block(&Url) -> Option<String>` живёт в `lumen-core::ext`, отделён от `FilterListSource` (загрузчика правил). При срабатывании эмитится `RequestBlocked { tab_id, url, reason }` ДО `RequestStarted` и до TCP — блокированный запрос не покидает клиент. Каждый redirect-hop проверяется независимо. Реализаций фильтров пока нет — место для интеграции с EasyList / собственным adblock-матчером готово.
- ✅ **`cargo bench` baseline (lumen-bench).** Бинарь, прогоняющий `decode → parse → layout → paint` на `samples/page.html` нужное число итераций и печатающий min/median/mean/p95/max на фазу + TOTAL; без сторонних deps, `LUMEN_BENCH_ITERS` env override. Регрессии при росте функциональности теперь отслеживаются (300ms cold start, <100MB RAM — точки отсчёта зафиксированы).
- ✅ **`[profile.dev.package."*"] opt-level=3`** — full optimization для зависимостей (wgpu, winit, rustls) в dev профиле, наш код остаётся на opt-level=1. wgpu в чистом debug режиме невыносим.
- CSS 2.1 + flexbox.
- Картинки.
- Вкладки, история, закладки.
- Network service в отдельном процессе.
- Storage service.
- Базовый adblock, DoH.
- **Tab session export / import** (§12.7) — простая фича, экономит много боли.
- Пакеты под Linux/macOS/Windows.
- **Browser fundamentals — критичные подсистемы, обнаруженные при аудите против Chromium / Firefox / Servo / Ladybird** (полный список с обоснованиями — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **HTML event loop + microtasks + rendering steps + observers** (`[P4]`) — контракт shell-а, не JS-движка. Без него ни Promise.then, ни ResizeObserver/IntersectionObserver/MutationObserver/PerformanceObserver, ни rAF не работают.
  - **Stacking contexts + правильный CSS Painting Order** (`[P1+P2]`, CSS 2.1 Appendix E) — сейчас paint в порядке DOM-обхода, z-index работает случайно. P1 — модель stacking-ов в layout; P2 — paint-side traversal.
  - **Compositor thread + property trees** (`[P2+P1]`) — TransformTree/ScrollTree/EffectTree/ClipTree на отдельном thread, off-main-thread scroll. Расширяет существующий план `compositor` крейта архитектурой. P2 — compositor pipeline + GPU; P1 — property trees от style/layout.
  - **Stacking-aware hit testing** (`[P2]`) — отдельная структура с z-index/pointer-events awareness, привязана к compositor layer tree.
  - ✅ **Quirks mode vs standards mode** (`[P1]`) — detection + application полностью реализованы 2026-05-24.
  - **Same-Origin Policy enforcement + CORS preflight** (`[P3]`) — SOP checks при fetch/postMessage/storage; OPTIONS preflight для non-simple requests.
  - **Mixed-content blocking + `<iframe sandbox>`** (`[P3]`) — HTTPS не грузит HTTP-script; sandbox flags.
  - **Preload scanner** (`[P1+P4]`) — отдельный pre-parser стартует fetch до DOM construction. Особенно важно над streaming pipeline. P1 — отдельный mode tokenizer-а; P4 — shell оркестрация.
- **Automation Phase 1 (ADR-006, §6.11):**
  - **`lumen-mcp-server` крейт** — Model Context Protocol over stdio/UNIX socket. Resources: `screenshot`, `a11y_tree`, `layout`, `console`, `network`. Tools: `click`, `type`, `scroll`, `navigate`, `wait`, `eval`. Запуск через `lumen --mcp` или `lumen --mcp-port N`. Это первый внешний транспорт — фастрастущий сегмент AI-агентов (Claude Computer Use, OpenAI Operator, Browser Use). MCP проще BiDi: JSON-RPC, маленькая спека.
  - **Native input injection** в шелле — `BrowserSession::input_event()` подаёт события в event loop тем же путём, что winit-события от ОС. Никаких `dispatchEvent` синтетических.
  - **Auto-wait внутри движка** — `wait_for(Cond::Visible/Stable/NetworkIdle/JsIdle)` на тиках layout/network/JS, не в SDK retry-loop.
  - **Per-context isolation по умолчанию** — каждая `BrowserSession` изолирована (cookies/storage/cache/viewport/UA/fingerprint).
  - **Deterministic mode** — `set_clock` / `set_rng_seed` / `freeze_fingerprint` для repeatable-тестов. Опирается на §9.5 anti-fingerprinting инфраструктуру.
  - **A11y tree first-class** — крейт `lumen-a11y` (P1) поднимается до уровня semantic locator surface; `BrowserSession::query(Role/Name/Text)` использует его, а не DOM-селекторы.
- **Tab lifecycle Phase 1** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):
  - **`TabState` enum + state machine T0-T4** (трек 10A) — состояния, transitions, per-user конфиг таймаутов.
  - **`MemoryPressureSource` trait** ✅ + три OS-impls (Win32 / Linux PSI / macOS `host_statistics64`) (трек 10H).
  - **Image decode cache LRU + viewport-gating** (трек 10E) — главный источник экономии T0: `ImageHandle` индирекция вместо прямых `DecodedImage` ссылок; decode только viewport ± 2 экрана; scroll-discard.
  - **Базовый T1 (paused)** — JS event loop pause/unpause при hide/show вкладки.
- **Цель:** ежедневный браузер для чтения статей; AI-агенты могут управлять Lumen через MCP без обёрток; **простая вкладка занимает ≤ 100 MB peak RSS**.

### Фаза 2 — v0.5 «Interactive» (18–24 месяца) 🟡 текущая фаза (app v0.2.0 → v0.5 по завершении)
- QuickJS интеграция.
- Tier 1 Web APIs.
- Формы, базовая интерактивность.
- HTTP/2.
- GPU compositor (wgpu).
- CSS Grid.
- Site isolation.
- Профили, шифрование.
- Anti-fingerprinting.
- **Knowledge layer ядро (§12):**
  - `lumen-knowledge` крейт: FTS-индекс над историей (§12.1).
  - Аннотации и заметки (§12.2).
  - Read-later / офлайн-чтение (§12.3).
  - Поиск по содержимому открытых вкладок (§12.4).
  - Focus mode (§12.6).
- **`<meta viewport>` parsing + page zoom (Ctrl+/Ctrl-).** Без этого мобильная вёрстка всегда «как desktop», и нет ручного управления масштабом.
- **Кастомизация UI** — drag&drop панелей, темы (§12.10).
- **Browser fundamentals — Phase 2** (полный список — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **Shadow DOM + custom elements + `<template>` + `<slot>`** (`[P1+P4]`) — Web Components. Без них половина современных сайтов сломается. P1 — cascade + composed tree + template/slot tree-builder; P4 — JS bindings + lifecycle.
  - **Accessibility tree + platform bridges** (`[P1+P4]`) — обязательно для NVDA / Orca / VoiceOver. «Русский first-class» требует. P1 — tree construction из DOM/layout + ARIA + focus model; P4 — platform bridges (UIA / AT-SPI / NSAccessibility) + focus dispatch.
  - **Forms runtime** (`[P1+P4]`) — Constraint Validation API, submission algorithm, file picker, autofill UI поверх существующего storage. P1 — ValidityState + validation pseudo-classes + submission algorithm; P4 — native pickers + autofill popup + validation tooltip.
  - ✅ **`<picture>` / `srcset` / `sizes` + `loading="lazy"`** (`[P1+P2]`) — P1 завершён: srcset, sizes, picture-picker, IntersectionObserver event source для lazy (rootMargin). P2 — image GPU upload.
  - **IME composition events** (`[P4]`) — без них японский / китайский / корейский ввод сломан.
  - ✅ **Connection pooling + keep-alive + Brotli + Range requests** (`[P3]`) — pooling/keep-alive (`with_pool`, LIFO idle, retry-on-stale), Brotli (`BrotliContentDecoder`), Range (single + multi + suffix + If-Range, `fetch_range`/`fetch_multi_range`) — все реализованы в `lumen-network` (70 range-тестов). Без keep-alive реальный сайт = 50× TCP handshakes.
  - **Find in page (Ctrl+F)** (`[P4]`).
  - **DevTools / Inspector минимум через CDP** (`[P4]`) — DOM tree + computed styles + network log. Без этого debug собственного движка невозможен.
  - **`mix-blend-mode` / `backdrop-filter` / `isolation`** (`[P1+P2]`) — нужны isolation groups в compositor pipeline. P1 — parsing + stacking model; P2 — paint pipeline + isolation groups.
- **Automation Phase 2 (ADR-006, §6.11):**
  - **`lumen-bidi-server` крейт** — WebDriver BiDi subset over WebSocket. Цель: `playwright.connect('ws://localhost:9222/session')` работает из коробки. Запуск через `lumen --bidi-port N`.
  - **Ship BiDi-gaps как built-in** — то, чего нет в W3C Working Draft (см. Playwright #32577, Cypress #30447): full response body access, `resourceType`, locale/timezone/offline emulation, per-context UA + extra headers, viewport-before-popup, per-context preload scripts, full download lifecycle, cookie change events, per-origin storage clear, дешёвая network interception. Документировать gap-mapping в `subsystems/lumen-bidi-server.md`.
  - **Espresso/Computer-use bridge для тестировщиков** — заранее закладывается accessibility-tree query API через MCP, аналогичный Playwright `getByRole`, чтобы тесты не зависели от CSS-классов и переживали DOM-рефакторы.
- **Tab lifecycle Phase 2** (§11.4, [ADR-008](docs/decisions/ADR-008-tab-lifecycle-memory-tiers.md)):
  - **T2 (JS heap snapshot)** — async-save в SQLite при T1→T2 (трек 10I); async-load с indeterminate UI hint при > 100ms; zstd compression; cap 5 MB/tab disk.
  - **T3 (full hibernation)** — DOM serialization через `bincode + deflate` в SQLite (трек 10J); в RAM остаётся только `TabMetadata` (URL, title, scroll, favicon) <200 KB/tab.
  - **GPU layer LRU + texture recycling** (трек 10F) — `wgpu::Texture` pool для off-viewport stacking contexts.
  - **Glyph atlas LRU eviction** (трек 10G).
  - **UI affordance** (трек 10K) — иконка "Z" / fade-opacity на спящих вкладках в tab strip, tooltip с tier-info, loading-spinner при restore > 200ms.
  - **JS heap GC tuning per tier** (трек 10L) — мягкий GC для активной, агрессивный для idle.
- **Цель:** публичная альфа, форумы и простые SPA, в Lumen начинают **жить** долго; Playwright/Selenium/Cypress тесты сторонних команд работают на Lumen; **50 открытых вкладок ≤ 600 MB total RAM**.

### Фаза 3 — v1.0 (36–48 месяцев)
- Переход на V8 (`rusty_v8`).
- Tier 2 Web APIs.
- IndexedDB, Canvas 2D.
- HTTP/3.
- Service Workers.
- WebFonts (WOFF2).
- Расширения (свой минимальный формат).
- WPT pass rate ≥ 60%.
- **Опциональный AI-модуль (§12.5):** `lumen-ai` крейт за feature-флагом. Семантический поиск, суммаризация, RAG над собственной историей. Bundle без AI остаётся basic-вариантом.
- **Семантические закладки (§12.8)** — опционально, требует AI.
- **Browser fundamentals — Phase 3+** (полный список — в [CLAUDE.md](CLAUDE.md) → roadmap «Browser fundamentals»):
  - **WebSockets (RFC 6455) + Server-Sent Events + Fetch API runtime с AbortController** (`[P3]`).
  - **HTTP auth (Basic + Digest)** (`[P3]`, готово) — `HttpClient::with_credentials` + RFC 7617/7616 в `lumen-network::auth`. Negotiate/NTLM + client certificates (mTLS) — отложены.
  - **OCSP stapling + CT log enforcement + invalid cert UI** (`[P3]`).
  - **Safe Browsing equivalent** (`[P3]`, готово) — `SafeBrowsingList` (SQLite) + `SafeBrowsingFilter` поверх `RequestFilter`-точки; полные SHA-256 + 20 канонических вариантов на URL; без облачного API.
  - **Back/forward cache (bfcache)** (`[P4]`).
  - **Navigation API + History API runtime** (`[P4]`).
  - **Web Animations API runtime** (`[P1+P2+P4]`) — compositor-driven для transform/opacity. P1 — value interpolation в момент t; P2 — compositor offload; P4 — animation timeline scheduling.
  - **`<contenteditable>` + Input Events Level 2 + Selection / Range API** (`[P1+P4]`) — P1 — DOM mutations + Selection/Range типы + `beforeinput`/`input` event firing; P4 — input dispatch (keyboard / IME / drag-drop / paste) + undo stack.
  - **Service Worker runtime** (`[P3+P4]`) — fetch interception / push / background sync. P3 — fetch interception API + push delivery + bg sync queue; P4 — отдельный JS worker context + lifecycle.
  - **Spell check** (`[P3+P4]`) через Hunspell-словари — русский словарь обязателен. P3 — словарь loader / Hunspell-формат parser / storage; P4 — squiggly render + context menu + OS API integration.
  - **Variable fonts axes runtime** (`[P2]`) — `font-variation-settings`.
  - **Color management + Display P3 / Rec2020 / ICC** (`[P2]`).
  - **Print pipeline runtime** (`[P1+P2+P4]`) — pagination algorithm над уже parsed `@page` и break-* properties, PDF generation. P1 — pagination algorithm; P2 — PDF rendering из display list; P4 — print preview UI.
  - **GC integration JS ↔ DOM** (`[P1+P4]`) — cycle collector между Rust DOM и JS engine. Архитектурная задача при интеграции QuickJS / V8. P1 — DOM wrapper hooks; P4 — JS engine integration + cycle collector algorithm.
  - **Permission prompt UI + Download UI** (`[P4]`) поверх существующего permissions/downloads storage.
  - **GPU process / sandbox** (`[P4]`) — seccomp / AppContainer / App Sandbox, расширение site isolation.
- **Automation Phase 3 (опционально, по запросу):**
  - **`lumen-cdp-shim` крейт** — Chrome DevTools Protocol subset как **thin adapter** поверх `BrowserSession`. Triggered only by real named demand from a legacy Puppeteer-using project. До этого CDP-кода в Lumen нет (см. ADR-006 «Graduation triggers»).
- **Цель:** стабильный релиз.

### Фаза 4 — После 1.0
- Подмножество WebGL (по запросам). 🟡 Базовый функциональный контекст готов (§7F): `canvas.getContext('webgl')` → `lumen_paint::SoftwareWebGl` (CPU-растеризатор), buffers/shaders/programs/attribs/uniform4f/drawArrays/readPixels. GLSL не исполняется — плоская заливка цветом из uniform4f.
- Mobile (Android через NDK; iOS — упрётся в Apple-policy).
- **Sync через E2E (§12.11)** — self-host или P2P. Mobile-клиент критичен для real use-case.
- **Граф знаний (§12.9)** — визуализация коллекции.
- Локализация UI.

---

