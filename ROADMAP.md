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
| P2-usability | P2 | | ready | | | Все подзадачи done, остался U-6 (ready) — остаточные рендер-баги через BUGS.md | USABILITY-вертикаль — «зашёл на сайт и комфортно пользуешься» |
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
| P2-scrolldriven | P2 | | planned | | BUG-127 | PARTIAL (ревизия P3 2026-07-04): парсинг + resolve прогресса от scroll/view готовы (scroll_timeline.rs), композит animated background-color готов (BUG-231 FIXED); TEST-78 остаток = font-parity (BUG-127 → KNOWN_DEBTOR 4.64) | CSS Scroll-Driven Animations L1 |
| P2-motionpath | P2 | | done | | BUG-125 | BUG-125 FIXED 2026-06-22 | CSS Motion Path L1 (offset-path) |
| P3-v8 | P3 | | planned | XL | | Разблокирован (v0.5.0 вышел). Единственное лекарство для тяжёлых SPA: аудит 2026-07-02 — github.com не дорендерился за 280 с (ресурсы загружены к 6.6 с, затык в QuickJS-интерпретаторе без JIT). Рекомендация: делать ПОСЛЕ дешёвых RP-5…RP-9 (они закрывают бо́льшую часть «не как в Edge» и не зависят от JS-движка), затем V8 как флагман Phase 3. Опц. промежуточный митигейт — watchdog/бюджет исполнения JS, чтобы страницы падали graceful, а не висли. Бриф docs/tasks/ph3-v8-migration.md | Переход на V8 (rusty_v8) + Tier 2 Web APIs |
| P3-idb | P3 | | done | | | P3-idb ✅ 2026-06-25 (NativeIdbStore wired into shell; structured schema mirror via _lumen_idb_schema_op; snapshot-blob restore; JS integration test) | IndexedDB |
| P3-h3 | P3 | | active | | | В работе (P1): срез 1 ✅ 2026-07-04 — QUIC varint (RFC 9000 §16) + HTTP/3 frame codec (RFC 9114 §7.2); срез 2 ✅ 2026-07-04 — QPACK field-section codec (RFC 9204, static table only = поведение при SETTINGS_QPACK_MAX_TABLE_CAPACITY=0; переиспользует HPACK-примитивы); срез 3 ✅ 2026-07-04 — Alt-Svc parser + AltSvcCache (RFC 7838, h3-only, TTL-expiry, детерминируемый по Instant), pure parse без IO; срез 4 ✅ 2026-07-04 — QUIC transport frame codec (RFC 9000 §19, все типы фреймов PADDING…HANDSHAKE_DONE, pure parse/serialize поверх varint, +22 теста); срез 5 ✅ 2026-07-04 — QUIC packet header codec (RFC 9000 §17, все формы пакетов: Initial/0-RTT/Handshake/Retry/Version Negotiation/short 1-RTT, header-protected биты и AEAD-payload сохраняются как opaque, pure parse/serialize, +19 тестов); срез 6 ✅ 2026-07-04 — QPACK dynamic table + encoder/decoder instruction streams (RFC 9204 §3.2/§4.3/§4.4: byte-budget capacity, oldest-first eviction, absolute/relative indexing, Set Capacity/Insert Name Ref/Insert Literal Name/Duplicate, Section Ack/Stream Cancellation/Insert Count Increment; apply() воспроизводит таблицу пира; переиспользует qpack string/static-table pub(crate), +25 тестов); срез 7 ✅ 2026-07-04 — QUIC RTT-эстиматор + NewReno congestion controller (RFC 9002 §5/§7: smoothed_rtt/rttvar/min_rtt EWMA 7-8·3-4, PTO §6.2.1; cwnd через slow start/congestion avoidance/recovery, halving §7.3.2, collapse под persistent congestion §7.6; чистые state-machine, драйвятся acked/lost пакетами, без sent-packet registry и IO, +24 теста); срез 8 ✅ 2026-07-04 — QUIC sent-packet registry + loss detection (RFC 9002 §6: per-space SentPacketRegistry на BTreeMap, on_ack_received удаляет newly-acked + RTT-сэмпл §5.1, detect_and_remove_lost_packets по packet-threshold §6.1.1 и time-threshold §6.1.2, loss_time для таймера, establishes_persistent_congestion §7.6.2; чистая state-machine, now передаётся снаружи, драйвит recovery; переиспользует recovery::K_GRANULARITY + quic_frame::AckRange, +19 тестов); срез 9 ✅ 2026-07-04 — QUIC loss-detection timer + PTO (RFC 9002 §6.2, Appendix A: LossDetection связывает 3 per-space registry + RttEstimator + pto_count; set_loss_detection_timer = earliest loss_time, иначе PTO с exp-backoff 2^pto_count, иначе disarm; get_pto_time_and_space с anti-deadlock от now (§6.2.2.1) и пропуском App Data до handshake_confirmed; on_timeout = declare time-threshold losses ЛИБО SendProbe 1/2 + bump backoff; App Data PTO добавляет max_ack_delay; discard_space §A.4; чистая state-machine, now снаружи, saturating arithmetic, +14 тестов); срез 10 ✅ 2026-07-04 — QUIC stream data model (RFC 9000 §2/§3/§4: RecvStream = out-of-order/overlapping reassembly с предпочтением уже принятых байт §2.2, receive flow-control §4.1, final-size инварианты §4.5, recv state-machine §3.2; SendStream = буфер + emit STREAM-фреймов bounded peer max_stream_data §4.1, on_ack→DataRecvd, reset, send state-machine §3.1; stream-id helpers §2.1; чистая state-machine, без connection-level flow control/ретрансмиссии/IO, +29 тестов); срез 11 ✅ 2026-07-04 — QUIC connection-level flow control + stream-count limits (RFC 9000 §4.1/§4.6: SendConnFlow/RecvConnFlow = connection-wide MAX_DATA budget поверх per-stream MAX_STREAM_DATA — send-side available/allowed/blocked_at (DATA_BLOCKED §19.12), recv-side record_received enforcement + record_read/window_update re-advertise; SendStreamLimit/RecvStreamLimit = MAX_STREAMS budget по направлению — send-side open() выдаёт client-initiated stream-id n*4+dir_bit / blocked_at (STREAMS_BLOCKED §19.14), recv-side record_open enforcement + record_closed/window_update; StreamDir/stream_count helpers §2.1; ConnError→FLOW_CONTROL_ERROR/STREAM_LIMIT_ERROR; чистая state-machine без IO, +22 теста); срез 12 ✅ 2026-07-04 — HTTP/3 stream layer (RFC 9114 §6.2/§7.1/§4.1: UniStreamType демультиплексирует однонаправленные стримы по type-префиксу — control/push+PushID/QPACK enc/QPACK dec/reserved; UniStreamRegistry = «ровно один» control/QPACK-encoder/QPACK-decoder → H3_STREAM_CREATION_ERROR на дубль, H3_CLOSED_CRITICAL_STREAM на закрытие критического; ControlStream = первый фрейм SETTINGS иначе H3_MISSING_SETTINGS, SETTINGS максимум раз, без request-фреймов; RequestStream = грамматика HEADERS+→DATA*→опц. trailer HEADERS с интерливом PUSH_PROMISE, всё прочее H3_FRAME_UNEXPECTED; переиспользует frame type-коды + H3_FRAME_UNEXPECTED; чистая state-machine без IO, +27 тестов); срез 13 ✅ 2026-07-04 — QUIC key schedule (RFC 9001 §5.1/§5.2: TLS 1.3 HKDF over SHA-256 — HMAC-SHA256 поверх имеющегося sha2 без новой зависимости, HKDF-Extract/Expand/Expand-Label с префиксом «tls13 »; INITIAL_SALT_V1 + Initial-secret chain из client DCID → client_initial_secret/server_initial_secret; PacketProtectionKeys{key/iv/hp/secret} = quic key/quic iv/quic hp, Debug редактирует ключевой материал; InitialKeys::derive обе стороны; next_generation_secret = quic ku key update §6.1; сверено с RFC 9001 Appendix A.1 векторами + RFC 4231/5869; +14 тестов); срез 14 ✅ 2026-07-04 — QUIC packet protection (RFC 9001 §5.3/§5.4, h3::packet_protect): AEAD payload protection AES-128-GCM (aes_128_gcm_seal/open — nonce = iv XOR packet_number правым выравниванием, unprotected header как AAD, ciphertext‖16-байт tag) + header protection (aes_128_hp_mask = AES-ECB(hp, 16-байт sample) → 5-байт маска, apply/remove_header_protection маскируют младшие биты первого байта [4 бита long / 5 бит short] и октеты packet number, sample всегда на pn_offset+4); AES-suite only (ChaCha20 §5.4.4 отложен); новые dep aes-gcm/aes (уже в воркспейсе у lumen-js/lumen-storage); чистые функции над буферами, сверено с RFC 9001 Appendix A.2/A.3 + McGrew–Viega GCM KAT; +16 тестов) (crates/network/src/h3/, 275 тестов). Остаётся: TLS 1.3 handshake (Handshake/1-RTT secrets) + UDP датаграммы/арминг таймера + сборка probe-пакетов, h3_do_request | HTTP/3 |
| PH3-20 | P3 | | done | | | | Service Workers (PH3-20 fetch interception Phase 1) |
| P3-woff2 | P3 | | done | | | | WebFonts WOFF2 |
| P3-ext | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): расширения Phase 0 (MV3-subset: manifest.json loader + content_scripts + chrome.runtime стаб) — shell/extensions | Расширения (минимальный формат) |
| P3-wpt | P3 | | planned | | | | WPT pass rate ≥ 60% |
| P3-ai | P3 | | planned | | | | Опциональный AI-модуль (lumen-ai, семантический поиск/RAG) |
| P3-ws | P3 | | done | | | P3-ws ✅ 2026-06-25 (in-flight fetch abort + SSE non-blocking reconnect + WS sub-protocol/wasClean/state-machine + e2e suite; deep zero-poll WS push = optional shell refinement) | WebSockets + SSE + Fetch API + AbortController |
| P3-auth | P3 | | done | | | | HTTP auth (Basic + Digest) |
| P3-safebrowse | P3 | | done | | | | Safe Browsing equivalent |
| P3-bfcache | P3 | | active | | | Незавершённая работа в ветке `p1-ph3-bfcache` (не на main, см. HEALTH-LOG 2026-07-01); lifecycle-срез (freeze/thaw) уже влит в main отдельно | Back/forward cache (bfcache) |
| P3-navapi | P3 | | active | | | Незавершённая работа в ветке `p1-ph3-navapi` (не на main, см. HEALTH-LOG 2026-07-01); фазы 1a/1b — см. бриф docs/tasks/ph3-navigation-history-api.md | Navigation API + History API runtime |
| PH3-8 | P3 | | done | | | | Web Animations API runtime (PH3-8) |
| PH3-7 | P3 | | done | | | | contentEditable + Input Events L2 + Selection/Range (PH3-7) |
| P3-spell | P3 | | active | | | PARTIAL 2026-07-04 (срезы 1-3): lumen_core::spell — pure-Rust движок Hunspell + срез 2: shell::spellcheck (load_dictionaries из data/spell, MultiDictionary, extract_words/misspelled_ranges, build_spell_overlay) + обвязка main.rs (squiggly для фокусного input/textarea) + срез 3 (P1, 2026-07-04): squiggly в contenteditable (find_editing_host), контекст-меню page_context_menu (right-click → ≤5 подсказок + «Добавить в словарь»/user_words.txt + «Пропустить»/сессия) для однострочного input, misspelled_ranges_with(allow-set), 25 тестов; дошить — apply в textarea/contenteditable, OS-провайдер ISpellChecker (опц.) | Spell check (Hunspell, русский словарь) |
| P3-varfonts | P3 | | planned | | BUG-109 | PARTIAL (сверка с кодом 2026-07-02): fvar/gvar/avar/HVAR/MVAR + apply_variations в CPU/wgpu (variation.rs); дошить — проводка в живое окно femtovg (BUG-109, сейчас дефолт-instance) | Variable fonts axes runtime (font-variation-settings) |
| P3-color | P3 | | active | | | | Color management + Display P3 / Rec2020 / ICC |
| P3-print | P3 | | done | | | | Print pipeline runtime (pagination + PDF) |
| P3-media | P3 | | done | | | | Медиа Phase 1: getUserMedia / <audio> / <video> / Screen Capture / Pointer Lock / Idle / Wake Lock / File System Access |
| P3-dnd | P3 | | done | | | | HTML5 Drag and Drop API (PH3-9) |
| P3-subgrid | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): алгоритм subgrid.rs наследует треки родителя; дошить — CSS-проводка keyword subgrid в каскад (CSS-SPECS §42) | CSS Subgrid (grid-template-rows/columns: subgrid) |
| P3-anchorpos | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): анкер-алгоритм anchor.rs (resolve_anchor/inset_area) готов; дошить — CSS-проводка anchor-name/position-anchor/inset-area/anchor-size() в каскад (BUG-126) | CSS Anchor Positioning L1 (anchor()/anchor-size()/position-area) |
| P3-nesting | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): полный CSS Nesting L1 — parser.rs expand_nesting (& явный/неявный + вложенные @media/@supports/@layer/@container), 20 тестов | CSS Nesting (полный спек: & и вложенные правила) |
| P3-scope | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): @scope root-matching готов (node_is_in_scope); дошить — donut/limit (inner-scope), CSS-SPECS §53 Phase 2 | CSS @scope (scoped styling + donut scope) |
| P3-stylequery | P3 | | done | | | Phase 0 ✅ 2026-07-02 (смешанное программирование, nemotron-3-ultra): style(--prop[: value]) в @container — evaluate_container_condition + ContainerContext.custom_props; одна декларация, литеральное сравнение; state() ⬜ | CSS Container Style Queries (style()/state()) |
| P3-textwrap | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): text-wrap-mode/style парсятся в ComputedStyle; дошить — алгоритм balance/pretty в line-break (сейчас поля не влияют на перенос) | CSS text-wrap: balance / pretty |
| P3-has | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): :has() relational matching в каскаде — style.rs matches_relative (Selectors L4 §17.2) | Селектор :has() (полная поддержка в каскаде) |
| P3-colormix | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): color-mix() + relative color syntax — color_mix.rs (все color-spaces srgb/hsl/hwb/lab/lch/oklab/oklch/xyz) | CSS color-mix() + relative color syntax |
| P3-regprop | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): @property registration + typed enforcement — style.rs PropertyRule/registry (syntax/inherits/initial-value) | CSS @property (registered custom props + Typed OM) |
| P3-counterstyle | P3 | | done | | | Статус был протухшим: сверка с кодом 2026-07-02 (вечер) — @counter-style реализован ПОЛНОСТЬЮ (parse_counter_style_rule + CounterStyleRegistry + ListStyleType::Custom + build_list_marker_text, тесты в parser.rs и counters.rs) | CSS @counter-style (кастомные маркеры списков) |
| P3-multicol | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): column-count/width/gap/rule + балансировка высоты — box_tree.rs multi_column_layout + column-rule paint | CSS Multi-column layout (column-count/width/gap/rule) |
| P3-contentvis | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): content-visibility hidden/auto работают (skip + ratchet); дошить — применение contain-intrinsic-size как размер-хинта в layout | CSS content-visibility + contain-intrinsic-size |
| P3-fragmentation | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): break-before/after/inside + orphans/widows парсятся, применяются в pagination.rs (print); дошить — применение вне печати (multicol/regions) | CSS Fragmentation (break-inside / widows / orphans) |
| P3-initialletter | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): Phase 0 drop-cap float (size×line-height, sink); дошить — точное cap-height/baseline выравнивание, raised-cap, RTL | CSS initial-letter (буквица drop-cap) |
| P3-vertical | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): block axis-swap vertical-rl/lr готов (vertical.rs); дошить — вертикальный inline-поток (Phase 2, lay_out_vertical_inline_run) | CSS writing-mode: вертикальный текст (полный layout) |
| P3-resizeobs | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): ResizeObserver observe/unobserve/disconnect + delivery после relayout — dom.rs | ResizeObserver |
| P3-intersectobs2 | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): IntersectionObserver + threshold/isIntersecting/rootMargin — dom.rs (используется lazy-load) | IntersectionObserver v2 (visibility tracking) |
| P3-streams | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): ReadableStream/WritableStream/TransformStream (getReader/tee/pipeTo/pipeThrough) — dom.rs (Phase 0, без pull-backpressure) | Streams API (Readable/Writable/Transform) |
| P3-webcrypto | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): ECDSA-P256 + HMAC-SHA* + AES-GCM (subtle_crypto.rs); дошить — RSA/Ed25519/PBKDF2/HKDF/AES-CBC-CTR/standalone digest (~20 алгоритмов спека) | Web Crypto SubtleCrypto (полный набор алгоритмов) |
| P3-weblocks | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): navigator.locks request/query (exclusive/shared, ifAvailable, steal, signal) — web_locks.rs (Phase 0, без cross-tab) | Web Locks API |
| P3-broadcast | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): BroadcastChannel cross-runtime (process-global hub + pump в event loop) — broadcast_channel.rs | BroadcastChannel |
| P3-structclone | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): structuredClone (примитивы/Map/Set/массивы/объекты) — dom.rs; дошить — Transferable (ArrayBuffer/ImageBitmap/OffscreenCanvas transfer) | structuredClone + Transferable objects |
| P3-ricallback | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): requestIdleCallback/IdleDeadline (timeout honored) — dom.rs (idle эмулируется таймером) | requestIdleCallback / IdleDeadline |
| P3-clipboard | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): navigator.clipboard readText/writeText через platform provider — clipboard.rs (Phase 0, текст; image — стаб) | Async Clipboard API (read/write text+image) |
| P3-trustedtypes | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): Trusted Types L2 createPolicy/TrustedHTML|Script|ScriptURL — trusted_types.rs (Phase 0, без sink-enforcement) | Trusted Types (защита от DOM XSS) |
| P3-sanitizer | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): Sanitizer/setHTML — sanitizer.rs (Phase 0, regex: срез <script> + on*-атрибуты) | HTML Sanitizer API |
| P3-customstate | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): ElementInternals + CustomStateSet готовы (element_internals.rs); дошить — CSS-селектор :state() в каскаде (P4-handoff) | ElementInternals + custom element states (:state()) |
| P3-pointerfull | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): base PointerEvent L2 работает; дошить — L3 getCoalescedEvents/getPredictedEvents (сейчас возвращают []) | Pointer Events L3 (coalesced / predicted events) |
| P3-compressionstream | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): классы CompressionStream/DecompressionStream есть, но throw для всех форматов; дошить — реальный gzip/deflate/brotli (переиспользовать декодеры lumen-network flate/brotli) | Compression Streams (gzip/deflate/brotli) |
| P3-cookiestore | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): cookieStore get/getAll/set/delete + CookieChangeEvent — cookie_store.rs (Phase 0, in-memory) | Cookie Store API |
| P3-cacheapi | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): caches.open/match/delete/keys/has + backend memory|SQLite, wired в SW fetch — dom.rs + cache bindings | Cache API + offline-first навигация |
| P3-webtransport | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): стаб webtransport.rs всегда reject (нет QUIC); дошить — зависит от QUIC/H3 (P3-h3) | WebTransport (поверх HTTP/3) |
| P3-reporting | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): ReportingObserver observe/disconnect/takeRecords — reporting_api.rs | Reporting API + Network Error Logging |
| P3-earlyhints | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): FetchPriority enum (High/Med/Low) есть — core/event.rs; дошить — обработка 103 Early Hints ответа | fetch Priority Hints + 103 Early Hints |
| P3-storagebuckets | P3 | | done | | | P1 2026-06-26 (navigator.storageBuckets open/keys/delete + StorageBucket persist/estimate/durability/expires/getDirectory, Phase 0 in-memory) | Storage Buckets API + quota management |
| P3-permissions | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): navigator.permissions.query -> PermissionStatus — dom.rs (desktop-политика) | Permissions API (query / onchange) |
| P3-notifications | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): Notification + requestPermission + show/close/click, OS-доставка через шелл — notifications_bindings.rs | Notifications API + системные уведомления |
| P3-pushapi | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): PushManager subscribe/getSubscription стаб (push_api.rs), подписки in-memory; дошить — реальная доставка push-endpoint | Push API (через Service Worker) |
| P3-offscreencanvas | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): OffscreenCanvas getContext/transferToImageBitmap + transferControlToOffscreen, работает в Worker — offscreen_canvas.rs | OffscreenCanvas + рендеринг в Worker |
| P3-webgl2 | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): WebGL2-контекст + GLSL ES 3.0 интерпретатор (webgl_bindings.rs); дошить — present фреймбуфера в страничный <canvas> (как P4-webgl, сейчас только readPixels) | WebGL2 подмножество |
| P3-avif | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): AVIF-декодер за feature-флагом avif (libavif+cmake+nasm), в дефолт-сборке не декодит; дошить — AVIF в дефолте + JPEG XL (сейчас sniff-only Err) | AVIF / JPEG XL декодирование |
| P3-webvtt | P3 | | done | | | DONE 2026-07-04 (P1, срез 4): lumen_dom::vtt (парсер cue + collect_video_tracks + active_cues/strip_cue_markup/resolve_cue_box) + shell::tracks (load_video_tracks, build_cue_overlay, collect_video_rects) + TextTrack JS API (video.textTracks/TextTrack/TextTrackCue/activeCues/cuechange через TextTrackStore-мост) + playback-клок из VideoGifStore (фолбэк nav_start для не-GIF). Остаток вне WebVTT: addTextTrack() и playback-клок для не-GIF <video> (зависит от HTML5-video decode) | WebVTT субтитры для <video> (<track>) |
| P3-mediasession | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): MediaSession metadata/playbackState/setActionHandler/setPositionState — media_session.rs (Phase 0, JS-сторона) | Media Session API |
| P3-pip | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): Document PiP requestWindow готов (document_pip.rs), Element.requestPictureInPicture частично; дошить — element-PiP + полноценное OS-окно | Picture-in-Picture API |
| P3-imagebitmap | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): createImageBitmap(ImageData) + transferToImageBitmap (offscreen_canvas.rs); дошить — ImageBitmapRenderingContext (bitmaprenderer) | createImageBitmap + ImageBitmapRenderingContext |
| P3-dialog | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): <dialog> show/showModal/close + returnValue + inert + возврат фокуса — dom.rs + layout | <dialog> модальные окна + атрибут inert |
| P3-popover | P3 | | done | | | реализовано (сверка с кодом 2026-07-02): Popover API showPopover/hidePopover/togglePopover + :popover-open — dom.rs + parser | Popover API (атрибут popover + ::backdrop) |
| P3-selectmenu | P3 | | planned | | | OPEN (сверка с кодом 2026-07-02, отдан P4): кастомный <select> appearance:base-select — CSS есть, HTMLSelectMenu нет | Кастомизируемый <select> (appearance: base-select) |
| P3-lazyembed | P3 | | done | | | Phase 0 ✅ 2026-07-02 (смешанное программирование, nano-omni worker + ревьюер): IframeInfo.loading_lazy/fetch_priority (dom) + ImageRequest.fetch_priority (layout), нормализация high/low по HTML LS §2.5.7; потребление приоритета шеллом (сортировка фетчей) и загрузка iframe — Phase 1 | loading=lazy для <iframe> + fetchpriority |
| P3-viewtransnav | P3 | | planned | | | PARTIAL (сверка с кодом 2026-07-02): SPA startViewTransition + ::view-transition-* готовы (layout); дошить — cross-document (MPA) навигационные переходы | View Transitions для cross-document навигации (MPA) |
| RP | P3 | | active | | | Открывать произвольные сайты так же, как Edge. RP-1…RP-4 done; RP-5…RP-9 заведены по аудиту 14 сайтов vs Edge 2026-07-02 (см. память realworld-site-audit-2026-07): внешние SVG, синтетический bold, анти-бот 403, CPU-растеризатор, print-стили | RP: Рендер-паритет реального веба |
| RP-1 | P3 | RP | done | M | | %-длины width/height/margin/padding в block-потоке резолвятся против containing-block (горизонталь+vertical pad/margin → cb-width; height → cb-height-if-definite). Уже было реализовано в движке (cb=available_width с 1B.1, definite-height threading с BUG-136); RP-1 закрепил поведение 5 регресс-тестами (box_tree.rs mod rp1_percentage_sizing) | RP-1: Проценты в block-потоке |
| RP-2 | P3 | RP | done | M | | layout-viewport отслеживает живой inner_size окна. Resized→resize→relayout уже был; RP-2 закрепил: relayout берёт CSS-viewport из content_layout_viewport (живой surface минус tab-strip + workspace-switcher по высоте), а не из полного окна — vw/vh/%/@media следуют за окном, headless остаётся 1024×720; 6 регресс-тестов | RP-2: Relayout под живой размер окна |
| RP-3 | P3 | RP | done | S | | gzip/deflate ContentDecoder (flate.rs) + объявлены в Accept-Encoding (`br, gzip, deflate`); GzipContentDecoder=MultiGzDecoder, DeflateContentDecoder=ZlibDecoder с raw-fallback; зарегистрированы на всех prod-сайтах HttpClient | RP-3: HTTP gzip/deflate декодер |
| RP-4 | P3 | RP | done | L | | float:left/right + clear для любых блоков: float-контекст пробрасывается во вложенные не-BFC блоки (line-box'ы сужаются вместо клипа), BFC-блоки сдвигаются, clear/вложенные float на глубине; drop-cap не регрессировал | RP-4: Общий float-поток |
| RP-5 | P3 | RP | done | L | | 2026-07-02: внешние SVG рисуются — `lumen_image::is_svg` сниффит разметку, шелл (`svg_image.rs`) оборачивает её в минимальный HTML и рендерит через обычный детерминированный pipeline (`parse_and_layout` → `paint_ordered` → `render_to_image_cpu`) в intrinsic-размере (width/height → viewBox → 300×150). Покрыты `<img>` (decode_image) и `background-image` (fetch_and_decode_background_images). Отложено: перерастеризация под CSS-размер бокса, currentColor хоста. Бриф docs/tasks/rp5-external-svg-images.md | RP-5: Внешние SVG-картинки |
| RP-6 | P3 | RP | done | M | | Срез A 2026-07-02: синтетический bold (двойной блит/повторный fill_text со сдвигом ≤2px, advance-метрики не меняются) + fake-italic (shear 12°: CPU — наклон контура глифа, femtovg — Transform2D вокруг baseline) как fallback в обоих текст-путях; настоящий bold/italic-фейс от FontProvider имеет приоритет (resolve_font_chain возвращает true_bold/true_italic). Срез B (Inter-Bold/-Italic в бандл, системный подбор в headless) отложен. Бриф docs/tasks/rp6-synthetic-bold-font-match.md | RP-6: Синтетический bold + подбор шрифта |
| RP-7 | P3 | RP | done | M | | 2026-07-04: root cause найден по коду — **HTTP/2-путь не слал fingerprint-заголовки**. `build_request_headers` (UA/Accept/Accept-Language/Accept-Encoding/Sec-Fetch/DNT) вызывался только в HTTP/1.1-ветке (`write_request`); H2-запрос (`h2.fetch`) уходил с одними псевдо-заголовками + cookie/cache/CORS → к Cloudflare-сайту приходил запрос без User-Agent = очевидный бот → 403. Фикс: `http::h2_fingerprint_headers` (тот же per-profile набор, что и H1, минус Host/Connection, lowercase) + `build_h2_headers` вклеивает в оба H2-пути (свежий и pooled) с dedup по имени и advertise Accept-Encoding (заодно включило сжатие на H2). Гипотеза 1: Chrome/Edge Accept навигации `*/*`→полный `text/html,...` (`CHROME_NAVIGATE_ACCEPT`). Гипотеза 3: порядок псевдо-заголовков H2 per-family (Chrome `:method :authority :scheme :path`). Cloudflare JS-challenge (`cf_clearance`) намеренно не трогали — отдельная связка cookie-jar+JS. Бриф docs/tasks/rp7-antibot-403.md | RP-7: Устойчивость к анти-бот 403 |
| RP-8 | P3 | RP | done | L | BUG-267 | Срез A 2026-07-03: dirty-bbox на слоях (CpuLayer), PopFilter/PushBackdropFilter кропают blur до bbox ink ⊕ 3r (бенч теней 169.5с → 1.5с, ×113). Срезы B–D 2026-07-03: clip/mask/opacity/blend-композиты по dirty-окну ⊕ 2px AA (бенч clip+mask+opacity+blend 40.1с → 0.34с, ×117); PNG байт-в-байт, 145/145 graphic_tests идентичны; transform-композит не кропается (bilinear не бит-идентичен). См. BUG-267 | RP-8: Ускорение CPU-растеризатора (layer bbox) |
| RP-9 | P3 | RP | planned | S | BUG-268 | Аудит 2026-07-02: шелл грузит `<link rel=stylesheet media="print">` в экранный каскад → print-стили протекают (URL после ссылок на w3.org). Гейт по media у `<link>`. См. BUG-268 | RP-9: Фильтр print-таблиц стилей |
| SDC | P1 | | done | | | Shell-as-driver-client: управление ЖИВЫМ видимым окном Lumen напрямую командами (Selenium/WebDriver-подобно, без headless-прокладки) — закрытие 8A.7 Фазы 4 по `docs/plans/8A.7-shell-as-driver-client-plan.md` + ADR-006. Разрез по владению: SDC-1a (P1 driver) → SDC-1b (P3 shell) → {SDC-2 (P1 фронты), SDC-3 (P3 графтест)}. Цепочка зависимостей строгая | SDC: Управление живым окном (8A.7 Ф4) |
| SDC-1a | P1 | SDC | done | M | | 8A.7 Ф4 (driver-часть): `click/type_text/scroll/eval/wait` в `crates/driver/src/winit_session.rs` реализованы headless-семантикой (click follows `<a href>`/toggles checkbox-radio `checked`; type_text пишет `value`; eval — one-shot QuickJS под `--features quickjs`, `install_dom` на снимке текущего DOM) + `AutomationCommand`/`AutomationReply` опубликованы из `lumen-driver`. Юнит-тесты `crates/driver/tests/cases/test_automation_commands.rs` | SDC-1a: WinitSession-команды (driver) |
| SDC-1b | P3 | SDC | done | M | | 8A.7 Ф4 (shell-часть): `AutomationCommand` подключён в живое окно (`main.rs` automation-dispatch блок, ~8000): Navigate/Click/Type/Scroll реальны; `Selector`-таргетинг заработал через `resolve_automation_target` (`lumen_layout::selector_query::find_all_by_selector`); Eval возвращает реальный `AutomationReply::Eval(json)` (`PersistentJs::eval_js_value`, использует `JsValue::to_json_string` из SDC-1a); Screenshot рендерит текущий `display_list` в PNG (`render_current_page_to_png`, CPU-путь); Wait поллится раз/кадр через `pending_waits` (не блокирует event loop) | SDC-1b: канал команд в живое окно (shell) |
| SDC-2 | P1 | SDC | done | M | | Протокол-фронты на живое окно через канал SDC-1b. `lumen-bidi-server` (`--bidi-port` + окно): `browsingContext.navigate`/`captureScreenshot`, `script.evaluate`, `input.performActions` (pointer+key) против `LiveWindowSession`; `lumen-mcp --mcp-live-port <N>` на тот же путь (`screenshot`/`eval`/`query`/`a11y_tree` больше не `Err`). Общий адаптер `lumen_driver::LiveWindowSession: BrowserSession` + `AutomationHandle`/`AutomationRequest` (запрос несёт свой канал ответа — SDC-1b реплаи раньше уходили в никуда). По ADR-006 BiDi+MCP, CDP опц./последним | SDC-2: BiDi/MCP на живое окно (MVP) |
| SDC-3 | P3 | SDC | done | M | | `graphic_tests/run.py --live`: один живой `lumen --mcp-live-port` процесс/окно на весь прогон вместо kill+relaunch на каждый тест (`LiveWindowClient`, MCP JSON-RPC — `navigate`+`wait(document_ready)` вместо блокирующего `time.sleep(LUMEN_WAIT_SEC=5)`); пиксельный снимок остался gdigrab-ом реального femtovg-окна (не CPU `resource://screenshot`, тот же разрыв паритета что у `--ipc`), поэтому проходят и JS-тесты (57, 129–138). TEST-00 калибрует crop offset один раз, как раньше. Валидировано полным прогоном: результат идентичен baseline (`Изменений нет`), 139 тестов за ~10.5 мин. По пути найдены и исправлены 2 бага живого окна, важных и для SDC-2: `AutomationCommand::Navigate` не поддерживал `file://` (всегда `PageSource::Url` → сетевой `HttpClient` → "unsupported scheme: file"); команда с BiDi/MCP-потока не будила запаркованный `ControlFlow::Wait` event loop (добавлен `LoadEvent::AutomationWake` + `AutomationHandle::set_wake`) | SDC-3: графтест на одном живом окне |
| P4-webgl | P4 | | active | | | | Подмножество WebGL (базовый CPU-контекст готов) |
| P4-mobile | P4 | | planned | | | | Mobile (Android NDK) |
| P4-sync | P4 | | planned | | | | Sync через E2E (self-host / P2P) |
| P4-kgraph | P4 | | planned | | | | Граф знаний (визуализация коллекции) |
| P4-l10n | P4 | | planned | | | | Локализация UI |
