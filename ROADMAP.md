# ROADMAP.md — Дерево задач Lumen Browser

Плоский, grep-friendly источник структуры для roadmap-деревьев (`docs/roadmap-*.html`).
Одна запись = одна строка. Размер файла нерелевантен: `grep "| U-6 " ROADMAP.md` достаёт ровно свою задачу.

Заменяет прежний вложенный `docs/roadmap.json`. После любой правки запусти:

```bash
python scripts/gen_roadmap.py
```

**Что здесь хранится:** структура фаз/задач (id, иерархия `parent`), курируемый статус и связи
баг→задача (колонка `bugs`). **Статусы и заголовки самих багов НЕ здесь** — они подтягиваются из
`BUGS.md` автоматически генератором.

**Статус задачи — производный.** `gen_roadmap.py` пересчитывает его из живых багов (все
FIXED/DEBTOR → `done`, IN PROGRESS → `active`) и из подзадач (все `done` → `done`). Колонка
`status` ниже — лишь запасной вариант для фич БЕЗ багов и БЕЗ подзадач (planned-фичи); у задач с
багами её править бесполезно — перезапишется при генерации.

**Иерархия:** `parent` пуст → задача висит прямо под фазой; `parent = <id>` → вкладывается в эту
задачу (рекурсивно).

**Статусы задач:** `done` · `active` · `blocker` · `wait` · `ready` · `queued` · `planned` · `opt`.
**Статусы багов** (из `BUGS.md`, здесь не задаются): `open` · `inprogress` · `fixed` · `wontfix`.

---

## Фазы

| id | status | date | title |
|---|---|---|---|
| P0 | done | 2026-05-26 | Фаза 0 — Прототип |
| P1 | done | 2026-06 | Фаза 1 — v0.1 «Reader» |
| P2 | done | app v0.5.0 | Фаза 2 — v0.5 «Interactive» (завершена) |
| P3 | planned | 36–48 мес | Фаза 3 — v1.0 «Full Browser» |
| P4 | planned | — | Фаза 4 — После 1.0 |

## Задачи

| id | phase | parent | status | size | bugs | note | title |
|---|---|---|---|---|---|---|---|
| P0-ws | P0 | | done | | | | Workspace + базовые крейты |
| P0-dom | P0 | | done | | | | DOM (арена NodeId, без Rc<RefCell>) |
| P0-layout | P0 | | done | | | | Block-flow layout + word-wrapping |
| P0-paint | P0 | | done | | | | Paint: FillRect через wgpu |
| P0-driver | P0 | | done | | | | Automation foundation: lumen-driver + off-screen рендер + tiny-skia |
| P0-tabinv | P0 | | done | | | | Tab lifecycle инварианты (DOM arena / JS suspend / pure layout) |
| P1-font | P1 | | done | | | | Font fallback / matcher (системный загрузчик) |
| P1-hidpi | P1 | | done | | | | HiDPI / DPR-awareness |
| P1-scroll | P1 | | done | | | | Scroll + базовый input в shell |
| P1-stream | P1 | | done | | | | Progressive / streaming rendering pipeline |
| PH1-2 | P1 | P1-stream | done | | | | PH1-2: window-first + 60Hz throttle + параллельный CSS |
| PH1-2a | P1 | P1-stream | done | | | | PH1-2a: TCP-level streaming HTTP body |
| PH1-2b | P1 | P1-stream | done | | | | PH1-2b: инкрементальный (dirty-subtree) layout |
| PH1-2c | P1 | P1-stream | done | | | | PH1-2c: прогрессивная подгрузка картинок |
| P1-url | P1 | | done | | | | Url как структурированный тип |
| P1-net | P1 | | done | | | | Network: EventSink + RequestFilter hook + bench baseline |
| P1-css21 | P1 | | done | | | | CSS 2.1 + flexbox |
| P1-img | P1 | | done | | | | Картинки (decode/paint) |
| P1-tabs | P1 | | done | | | | Вкладки, история, закладки |
| PH1-4 | P1 | | done | | | | PH1-4: Network service в отдельном процессе (lumen-ipc) |
| P1-storage | P1 | | done | | | | Storage service + базовый adblock + DoH |
| PH1-5 | P1 | | done | | | | PH1-5: пакеты Linux/macOS/Windows (CI/CD) |
| PH1-6 | P1 | | done | | | | PH1-6: Stacking contexts + CSS Painting Order |
| PH1-7 | P1 | | done | | | | PH1-7: Compositor thread + Property Trees |
| PH1-8 | P1 | | done | | | | PH1-8: Preload scanner |
| PH1-9 | P1 | | done | | | | PH1-9: lumen-mcp-server (Automation Phase 1) |
| P1-tablife1 | P1 | | done | | | | Tab lifecycle Phase 1 (TabState T0–T4, memory pressure, image LRU, T1 paused) |
| P2-usability | P2 | | active | | | ТОП-приоритет, режим одного программиста (всё на P1) | USABILITY-вертикаль — «зашёл на сайт и комфортно пользуешься» |
| U-0 | P2 | P2-usability | done | | | | U-0: --screenshot headless CPU-снимок |
| U-1.1 | P2 | P2-usability | done | | | | U-1 этап 1: неблокирующая навигация |
| U-2 | P2 | P2-usability | done | | | | U-2: шейпинг текста GSUB/GPOS + CFF-контуры |
| U-3 | P2 | P2-usability | done | | | | U-3: многовкладочность TAB-1…7 + IPC |
| U-4 | P2 | P2-usability | done | | | | U-4: WASM MVP-интерпретатор + graceful WebCodecs + i64↔BigInt |
| B-1 | P2 | P2-usability | done | XL | BUG-222 | | B-1: QuickJS вне UI-потока (разблокировка) |
| U-1.2 | P2 | P2-usability | done | L | BUG-171,BUG-172 | | U-1 этап 2: весь пайплайн вне UI-потока |
| U-5 | P2 | P2-usability | done | S | BUG-167 | | U-5: Fullscreen пересчитывает вьюпорт |
| U-6 | P2 | P2-usability | ready | | BUG-104,BUG-126,BUG-144,BUG-085 | | U-6: рендер-паритет high-deviation |
| OBS | P2 | P2-usability | done | M | BUG-221 | | Наблюдаемость: CPU-снимок = окно (полная замена gdigrab) |
| U-4opt | P2 | P2-usability | done | | | U-4a SIMD/threads/atomics + U-4b live-aliasing + U-4c WebGPU backend все ✅ 2026-06-20…21 | U-4 опции: WASM SIMD/threads, live-aliasing, WebGPU backend |
| P2-js | P2 | | done | | | | QuickJS интеграция + Tier 1 Web APIs |
| P2-forms | P2 | | done | | | | Формы, базовая интерактивность (Forms runtime) |
| P2-h2 | P2 | | done | | | | HTTP/2 |
| P2-gpu | P2 | | done | | | | GPU compositor (wgpu) |
| P2-grid | P2 | | done | | | | CSS Grid |
| PH2-2 | P2 | | done | | | | PH2-2: Site isolation Phase 1 (COOP/COEP/CORP) |
| PH2-3 | P2 | | done | | | | PH2-3: Профили + шифрование (AES-256-GCM vault) |
| P2-fp | P2 | | done | | | | Anti-fingerprinting |
| P2-knowledge | P2 | | done | | | F2-5 ✅ 2026-06-22 | Knowledge layer ядро (FTS, аннотации, read-later, focus mode) |
| P2-viewport | P2 | | done | | | | <meta viewport> parsing + page zoom |
| P2-ui | P2 | | done | | | F2-6 ✅ (light/dark/system themes + 6 accent presets; docked sidebars drag-resize + cross-dock move + persist); остаток F2-6 — инфраструктурный SurfaceManager (ADR-009), без новой UX-ценности | Кастомизация UI (drag&drop панелей, темы) |
| P2-shadow | P2 | | done | | | | Shadow DOM + custom elements + <template>/<slot> |
| PH2-7 | P2 | | done | | | | PH2-7: Accessibility tree + platform bridges Phase 1 |
| P2-picture | P2 | | done | | | | <picture>/srcset/sizes + loading=lazy |
| P2-ime | P2 | | done | | | | IME composition events |
| P2-find | P2 | | done | | | | Find in page (Ctrl+F) |
| P2-devtools | P2 | | done | | | | DevTools / Inspector через CDP (DOM + computed + network) |
| P2-blend | P2 | | done | | BUG-144 | | mix-blend-mode / backdrop-filter / isolation |
| PH3-2 | P2 | | done | | | | lumen-bidi-server (WebDriver BiDi, Automation Phase 2) |
| P2-tablife2 | P2 | | done | | | | Tab lifecycle Phase 2 (T2 heap snapshot, T3 hibernation, GPU/glyph LRU, UI affordance) |
| P2-canvas | P2 | | done | | BUG-099 | | Canvas 2D (Phases 1–5, Path2D) |
| P2-viewtrans | P2 | | done | | BUG-103 | F2-4 ✅ 2026-06-22 (root cross-fade); полный L1 — опц. остаток | View Transitions API |
| P2-masonry | P2 | | done | | BUG-105,BUG-143 | F2-3 ✅ 2026-06-22 (паритет с Edge-fallback; Edge не поддерживает masonry) | CSS Masonry layout |
| P2-scrolldriven | P2 | | planned | | BUG-127 | | CSS Scroll-Driven Animations L1 |
| P2-motionpath | P2 | | done | | BUG-125 | BUG-125 FIXED 2026-06-22 | CSS Motion Path L1 (offset-path) |
| P3-v8 | P3 | | planned | | | | Переход на V8 (rusty_v8) + Tier 2 Web APIs |
| P3-idb | P3 | | done | | | P3-idb ✅ 2026-06-25 (NativeIdbStore wired into shell; structured schema mirror via _lumen_idb_schema_op; snapshot-blob restore; JS integration test) | IndexedDB |
| P3-h3 | P3 | | planned | | | | HTTP/3 |
| PH3-20 | P3 | | done | | | | Service Workers (PH3-20 fetch interception Phase 1) |
| P3-woff2 | P3 | | done | | | | WebFonts WOFF2 |
| P3-ext | P3 | | planned | | | | Расширения (минимальный формат) |
| P3-wpt | P3 | | planned | | | | WPT pass rate ≥ 60% |
| P3-ai | P3 | | planned | | | | Опциональный AI-модуль (lumen-ai, семантический поиск/RAG) |
| P3-ws | P3 | | done | | | P3-ws ✅ 2026-06-25 (in-flight fetch abort + SSE non-blocking reconnect + WS sub-protocol/wasClean/state-machine + e2e suite; deep zero-poll WS push = optional shell refinement) | WebSockets + SSE + Fetch API + AbortController |
| P3-auth | P3 | | done | | | | HTTP auth (Basic + Digest) |
| P3-safebrowse | P3 | | done | | | | Safe Browsing equivalent |
| P3-bfcache | P3 | | planned | | | | Back/forward cache (bfcache) |
| P3-navapi | P3 | | planned | | | | Navigation API + History API runtime |
| PH3-8 | P3 | | done | | | | Web Animations API runtime (PH3-8) |
| PH3-7 | P3 | | done | | | | contentEditable + Input Events L2 + Selection/Range (PH3-7) |
| P3-spell | P3 | | planned | | | | Spell check (Hunspell, русский словарь) |
| P3-varfonts | P3 | | planned | | BUG-109 | | Variable fonts axes runtime (font-variation-settings) |
| P3-color | P3 | | active | | | | Color management + Display P3 / Rec2020 / ICC |
| P3-print | P3 | | done | | | | Print pipeline runtime (pagination + PDF) |
| P3-media | P3 | | done | | | | Медиа Phase 1: getUserMedia / <audio> / <video> / Screen Capture / Pointer Lock / Idle / Wake Lock / File System Access |
| P3-dnd | P3 | | done | | | | HTML5 Drag and Drop API (PH3-9) |
| P3-subgrid | P3 | | planned | | | | CSS Subgrid (grid-template-rows/columns: subgrid) |
| P3-anchorpos | P3 | | planned | | | | CSS Anchor Positioning L1 (anchor()/anchor-size()/position-area) |
| P3-nesting | P3 | | planned | | | | CSS Nesting (полный спек: & и вложенные правила) |
| P3-scope | P3 | | planned | | | | CSS @scope (scoped styling + donut scope) |
| P3-stylequery | P3 | | planned | | | | CSS Container Style Queries (style()/state()) |
| P3-textwrap | P3 | | planned | | | | CSS text-wrap: balance / pretty |
| P3-has | P3 | | planned | | | | Селектор :has() (полная поддержка в каскаде) |
| P3-colormix | P3 | | planned | | | | CSS color-mix() + relative color syntax |
| P3-regprop | P3 | | planned | | | | CSS @property (registered custom props + Typed OM) |
| P3-counterstyle | P3 | | planned | | | | CSS @counter-style (кастомные маркеры списков) |
| P3-multicol | P3 | | planned | | | | CSS Multi-column layout (column-count/width/gap/rule) |
| P3-contentvis | P3 | | planned | | | | CSS content-visibility + contain-intrinsic-size |
| P3-fragmentation | P3 | | planned | | | | CSS Fragmentation (break-inside / widows / orphans) |
| P3-initialletter | P3 | | planned | | | | CSS initial-letter (буквица drop-cap) |
| P3-vertical | P3 | | planned | | | | CSS writing-mode: вертикальный текст (полный layout) |
| P3-resizeobs | P3 | | planned | | | | ResizeObserver |
| P3-intersectobs2 | P3 | | planned | | | | IntersectionObserver v2 (visibility tracking) |
| P3-streams | P3 | | planned | | | | Streams API (Readable/Writable/Transform) |
| P3-webcrypto | P3 | | planned | | | | Web Crypto SubtleCrypto (полный набор алгоритмов) |
| P3-weblocks | P3 | | planned | | | | Web Locks API |
| P3-broadcast | P3 | | planned | | | | BroadcastChannel |
| P3-structclone | P3 | | planned | | | | structuredClone + Transferable objects |
| P3-ricallback | P3 | | planned | | | | requestIdleCallback / IdleDeadline |
| P3-clipboard | P3 | | planned | | | | Async Clipboard API (read/write text+image) |
| P3-trustedtypes | P3 | | planned | | | | Trusted Types (защита от DOM XSS) |
| P3-sanitizer | P3 | | planned | | | | HTML Sanitizer API |
| P3-customstate | P3 | | planned | | | | ElementInternals + custom element states (:state()) |
| P3-pointerfull | P3 | | planned | | | | Pointer Events L3 (coalesced / predicted events) |
| P3-compressionstream | P3 | | planned | | | | Compression Streams (gzip/deflate/brotli) |
| P3-cookiestore | P3 | | planned | | | | Cookie Store API |
| P3-cacheapi | P3 | | planned | | | | Cache API + offline-first навигация |
| P3-webtransport | P3 | | planned | | | | WebTransport (поверх HTTP/3) |
| P3-reporting | P3 | | planned | | | | Reporting API + Network Error Logging |
| P3-earlyhints | P3 | | planned | | | | fetch Priority Hints + 103 Early Hints |
| P3-storagebuckets | P3 | | done | | | P1 2026-06-26 (navigator.storageBuckets open/keys/delete + StorageBucket persist/estimate/durability/expires/getDirectory, Phase 0 in-memory) | Storage Buckets API + quota management |
| P3-permissions | P3 | | planned | | | | Permissions API (query / onchange) |
| P3-notifications | P3 | | planned | | | | Notifications API + системные уведомления |
| P3-pushapi | P3 | | planned | | | | Push API (через Service Worker) |
| P3-offscreencanvas | P3 | | planned | | | | OffscreenCanvas + рендеринг в Worker |
| P3-webgl2 | P3 | | planned | | | | WebGL2 подмножество |
| P3-avif | P3 | | planned | | | | AVIF / JPEG XL декодирование |
| P3-webvtt | P3 | | planned | | | | WebVTT субтитры для <video> (<track>) |
| P3-mediasession | P3 | | planned | | | | Media Session API |
| P3-pip | P3 | | planned | | | | Picture-in-Picture API |
| P3-imagebitmap | P3 | | planned | | | | createImageBitmap + ImageBitmapRenderingContext |
| P3-dialog | P3 | | planned | | | | <dialog> модальные окна + атрибут inert |
| P3-popover | P3 | | planned | | | | Popover API (атрибут popover + ::backdrop) |
| P3-selectmenu | P3 | | planned | | | | Кастомизируемый <select> (appearance: base-select) |
| P3-lazyembed | P3 | | planned | | | | loading=lazy для <iframe> + fetchpriority |
| P3-viewtransnav | P3 | | planned | | | | View Transitions для cross-document навигации (MPA) |
| RP | P3 | | active | | | Открывать произвольные сайты так же, как Edge; декомпозиция в RP-1…RP-4 | RP: Рендер-паритет реального веба |
| RP-1 | P3 | RP | done | M | | %-длины width/height/margin/padding в block-потоке резолвятся против containing-block (горизонталь+vertical pad/margin → cb-width; height → cb-height-if-definite). Уже было реализовано в движке (cb=available_width с 1B.1, definite-height threading с BUG-136); RP-1 закрепил поведение 5 регресс-тестами (box_tree.rs mod rp1_percentage_sizing) | RP-1: Проценты в block-потоке |
| RP-2 | P3 | RP | done | M | | layout-viewport отслеживает живой inner_size окна. Resized→resize→relayout уже был; RP-2 закрепил: relayout берёт CSS-viewport из content_layout_viewport (живой surface минус tab-strip + workspace-switcher по высоте), а не из полного окна — vw/vh/%/@media следуют за окном, headless остаётся 1024×720; 6 регресс-тестов | RP-2: Relayout под живой размер окна |
| RP-3 | P3 | RP | done | S | | gzip/deflate ContentDecoder (flate.rs) + объявлены в Accept-Encoding (`br, gzip, deflate`); GzipContentDecoder=MultiGzDecoder, DeflateContentDecoder=ZlibDecoder с raw-fallback; зарегистрированы на всех prod-сайтах HttpClient | RP-3: HTTP gzip/deflate декодер |
| RP-4 | P3 | RP | ready | L | | float:left/right + clear для любых блоков (FloatSide/ClearSide/wrap-машинерия есть, дёргается только для ::first-letter drop-cap) | RP-4: Общий float-поток |
| P4-webgl | P4 | | active | | | | Подмножество WebGL (базовый CPU-контекст готов) |
| P4-mobile | P4 | | planned | | | | Mobile (Android NDK) |
| P4-sync | P4 | | planned | | | | Sync через E2E (self-host / P2P) |
| P4-kgraph | P4 | | planned | | | | Граф знаний (визуализация коллекции) |
| P4-l10n | P4 | | planned | | | | Локализация UI |
