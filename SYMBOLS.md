# SYMBOLS

Auto-generated public API index. Regenerate: `python scripts/gen_symbols.py`

**Usage:** grep for a symbol → get `file:line` → `Read file offset=N limit=30`.

## lumen-a11y  (12 symbols)

`crates/engine/a11y/src/lib.rs:24` **enum** `LiveRegion` — `aria-live` values per WAI-ARIA §6.6
`crates/engine/a11y/src/lib.rs:33` **enum** `AriaCurrent` — `aria-current` values per WAI-ARIA §5.4.1
`crates/engine/a11y/src/lib.rs:52` **struct** `AXState` — ARIA state and property flags for one accessibility node
`crates/engine/a11y/src/lib.rs:113` **struct** `AXNode` — One node in the accessibility tree
`crates/engine/a11y/src/lib.rs:145` **struct** `AXTree` — Accessibility tree rooted at a document node
`crates/engine/a11y/src/lib.rs:160` **fn** `build_ax_tree` — Build an `AXTree` from a `Document` starting at `root_id`
`crates/engine/a11y/src/names.rs:18` **fn** `compute_name` — Compute the accessible name for a DOM node (ACCNAME-1.2 §4.3)
`crates/engine/a11y/src/names.rs:176` **fn** `compute_description` — Compute the accessible description for a DOM node (ACCNAME-1.2 §4.3.2)
`crates/engine/a11y/src/roles.rs:14` **enum** `AXRole` — All WAI-ARIA 1.2 roles
`crates/engine/a11y/src/roles.rs:185` **fn** `as_str` — Canonical lowercase WAI-ARIA role string
`crates/engine/a11y/src/roles.rs:266` **fn** `parse` — Parse a WAI-ARIA role string (case-insensitive)
`crates/engine/a11y/src/roles.rs:349` **fn** `implicit_role` — Compute the implicit WAI-ARIA role for a DOM node per HTML-AAM §5

## lumen-bench  (3 symbols)

`crates/bench/src/ci_gate.rs:36` **fn** `run_ci_gate` — Run the CI performance gate
`crates/bench/src/util.rs:9` **fn** `get_rss_bytes` — Returns the current process RSS (resident set size) in bytes
`crates/bench/src/util.rs:48` **fn** `extract_style_blocks` — Concatenates all `<style>` text blocks from the document

## lumen-canvas  (33 symbols)

`crates/engine/canvas/src/color.rs:3` **struct** `CanvasColor` — RGBA color used by the Canvas 2D API
`crates/engine/canvas/src/color.rs:11` **fn** `rgba`
`crates/engine/canvas/src/color.rs:16` **fn** `with_alpha_mult` — Multiply `self.a` by `alpha` (0.0–1.0)
`crates/engine/canvas/src/color.rs:25` **fn** `from_css_str` — Parse a CSS color string.  Supports:
`crates/engine/canvas/src/fp_noise.rs:17` **struct** `CanvasNoiseGenerator` — Per-session canvas fingerprint noise generator
`crates/engine/canvas/src/fp_noise.rs:27` **fn** `new` — Create a new noise generator with the given per-session seed
`crates/engine/canvas/src/fp_noise.rs:48` **fn** `next_noise_u8` — Generate next noise byte (0..=255) clamped to safe range
`crates/engine/canvas/src/fp_noise.rs:56` **fn** `apply_noise_to_pixel` — Add per-channel noise to an RGBA pixel
`crates/engine/canvas/src/fp_noise.rs:66` **fn** `apply_noise_to_buffer` — Apply noise to an entire RGBA buffer (row-major, top-left origin)
`crates/engine/canvas/src/fp_noise.rs:77` **fn** `reset` — Reset the RNG state to the seed (for reproducibility)
`crates/engine/canvas/src/lib.rs:33` **struct** `Context2D` — HTML Canvas 2D rendering context
`crates/engine/canvas/src/lib.rs:57` **fn** `new` — Create a new context with a transparent black buffer
`crates/engine/canvas/src/lib.rs:78` **fn** `set_noise_generator` — Set the optional noise generator for fingerprint randomization
`crates/engine/canvas/src/lib.rs:86` **fn** `get_image_data` — Get a copy of pixel data with optional noise applied (for `getImageData()`)
`crates/engine/canvas/src/lib.rs:98` **fn** `from_pixels` — Create a context pre-filled with the given RGBA8 pixel buffer
`crates/engine/canvas/src/lib.rs:107` **fn** `width`
`crates/engine/canvas/src/lib.rs:108` **fn** `height`
`crates/engine/canvas/src/lib.rs:111` **fn** `pixels` — Raw RGBA8 pixel data
`crates/engine/canvas/src/lib.rs:114` **fn** `resize` — Resize the canvas (clears the buffer)
`crates/engine/canvas/src/lib.rs:126` **fn** `clear_rect` — `clearRect(x, y, w, h)` — erase region to transparent black
`crates/engine/canvas/src/lib.rs:141` **fn** `fill_rect` — `fillRect(x, y, w, h)` — fill region with current `fillStyle`
`crates/engine/canvas/src/lib.rs:147` **fn** `stroke_rect` — `strokeRect(x, y, w, h)` — stroke the outline of a rectangle
`crates/engine/canvas/src/lib.rs:161` **fn** `begin_path` — `beginPath()` — discard current path
`crates/engine/canvas/src/lib.rs:167` **fn** `move_to` — `moveTo(x, y)` — start a new sub-path
`crates/engine/canvas/src/lib.rs:176` **fn** `line_to` — `lineTo(x, y)` — add a line segment
`crates/engine/canvas/src/lib.rs:187` **fn** `close_path` — `closePath()` — add a line back to the sub-path start
`crates/engine/canvas/src/lib.rs:197` **fn** `arc` — `arc(cx, cy, r, start_angle, end_angle[, anticlockwise])` — add an arc
`crates/engine/canvas/src/lib.rs:217` **fn** `fill` — `fill()` — fill the current path with `fillStyle`
`crates/engine/canvas/src/lib.rs:224` **fn** `stroke` — `stroke()` — stroke the current path with `strokeStyle`
`crates/engine/canvas/src/path.rs:3` **enum** `PathSegment` — A single segment in a 2D path
`crates/engine/canvas/src/path.rs:11` **type** `PathCommand` — Alias kept for API symmetry with the HTML spec (`PathCommand` = verb)
`crates/engine/canvas/src/rasterize.rs:4` **fn** `fill_path` — Fill `path` using the even-odd scanline algorithm
`crates/engine/canvas/src/rasterize.rs:41` **fn** `stroke_path` — Stroke `path` by drawing each line segment as a thick rectangle

## lumen-core  (193 symbols)

`crates/core/src/capability.rs:7` **enum** `Capability`
`crates/core/src/capability.rs:27` **struct** `CapabilityToken`
`crates/core/src/crash.rs:65` **struct** `CrashRecorder` — Рекордер событий с кольцевым буфером и дампом при панике
`crates/core/src/crash.rs:79` **fn** `new` — Рекордер с ёмкостью буфера по умолчанию ([`DEFAULT_CAPACITY`]) и без
`crates/core/src/crash.rs:86` **fn** `with_capacity` — Рекордер с заданной ёмкостью буфера и без downstream-sink-а
`crates/core/src/crash.rs:101` **fn** `with_downstream` — Рекордер, форвардящий каждое событие дальше указанному sink-у после
`crates/core/src/crash.rs:111` **fn** `recent_events` — Снимок текущего содержимого буфера в виде готовых строк дампа
`crates/core/src/crash.rs:127` **fn** `total_recorded` — Сколько событий записано всего с момента старта (включая вытесненные
`crates/core/src/crash.rs:142` **fn** `install_panic_hook` — Установить process-global panic-hook, который при панике пишет дамп
`crates/core/src/crash.rs:192` **fn** `format_crash_dump` — Собрать текст crash-дампа из снимка событий и сообщения паники
`crates/core/src/crash.rs:224` **fn** `write_crash_dump` — Записать готовый текст дампа в новый файл `lumen-crash-<unix_ms>.log`
`crates/core/src/error.rs:7` **enum** `Error`
`crates/core/src/error.rs:35` **type** `Result`
`crates/core/src/event.rs:9` **struct** `TabId`
`crates/core/src/event.rs:18` **enum** `RequestStage` — Стадия сетевого запроса, на которой произошёл сбой
`crates/core/src/event.rs:39` **fn** `as_str` — Машинно-читаемый тег стадии для логов и сериализации (`"dns"`/`"tcp"`/
`crates/core/src/event.rs:52` **enum** `SubresourceKind` — Тип subresource-ресурса, найденного preload-сканером
`crates/core/src/event.rs:67` **enum** `FetchPriority` — Приоритет выборки subresource-а. Отражает HTML Living Standard §17.2.3
`crates/core/src/event.rs:79` **fn** `for_kind` — Приоритет по типу subresource (Fetch Standard §2.2)
`crates/core/src/event.rs:91` **enum** `Event`
`crates/core/src/ext.rs:19` **trait** `NetworkTransport` — Сетевой транспорт. Подменяется на mock для тестов или на альтернативный стек
`crates/core/src/ext.rs:39` **trait** `EventSink` — Приёмник событий из подсистем (network, навигация, вкладки)
`crates/core/src/ext.rs:46` **struct** `NoopEventSink` — EventSink, который молча игнорирует все события. Дефолт для подсистем,
`crates/core/src/ext.rs:57` **trait** `StorageBackend` — Хранилище ключ/значение для cookies, истории, кэша
`crates/core/src/ext.rs:89` **trait** `SearchProvider` — Поисковая система для omnibox
`crates/core/src/ext.rs:100` **trait** `FilterListSource` — Источник списка фильтров рекламы / трекеров
`crates/core/src/ext.rs:116` **trait** `RequestFilter` — Решение «блокировать ли исходящий запрос». Реализация смотрит URL и
`crates/core/src/ext.rs:140` **trait** `DnsResolver` — DNS-резолвер: hostname → список IP-адресов (с портом, готовых к connect)
`crates/core/src/ext.rs:165` **trait** `HstsEnforcement` — HSTS-политика: должны ли HTTP-запросы к данному host принудительно
`crates/core/src/ext.rs:189` **enum** `HttpAuthScheme` — HTTP authentication scheme, разрешённый `HttpClient` для re-request
`crates/core/src/ext.rs:200` **fn** `as_str`
`crates/core/src/ext.rs:221` **struct** `HttpAuthChallenge` — Запрос учётных данных от credential-провайдера. Передаётся в
`crates/core/src/ext.rs:234` **struct** `HttpCredentials` — Учётные данные для HTTP auth: username + plaintext password
`crates/core/src/ext.rs:257` **trait** `HttpCredentialProvider` — Поставщик учётных данных HTTP-auth
`crates/core/src/ext.rs:266` **trait** `CookieProvider` — HTTP cookie storage provider. Bridges lumen-network (fetch pipeline) to
`crates/core/src/ext.rs:305` **trait** `EncodingDetector` — Определение кодировки HTML-документа. Для кириллицы критично уметь
`crates/core/src/ext.rs:315` **enum** `FontStyle` — Начертание face-а: `font-style` из CSS Fonts L4. Phase 0 — три
`crates/core/src/ext.rs:324` **fn** `parse_keyword` — Парсит CSS-ключевое слово `normal | italic | oblique` (case-insensitive)
`crates/core/src/ext.rs:346` **struct** `FaceRecord` — Метаданные одного face-а в индексе шрифтов
`crates/core/src/ext.rs:386` **trait** `FontProvider` — Источник системных шрифтов. Реализация — в `lumen-font::system_fonts`
`crates/core/src/ext.rs:440` **fn** `match_face` — CSS Fonts L4 §5.2 алгоритм матчинга — извлечён из trait-а в свободную
`crates/core/src/ext.rs:479` **fn** `match_face_no_stretch` — Legacy функция match_face для backward compatibility (без stretch)
`crates/core/src/ext.rs:779` **trait** `JsRuntime` — JavaScript runtime — исполнение JS-кода (HTML inline scripts, `eval`,
`crates/core/src/ext.rs:845` **struct** `SuspendedHeap` — Serialized JS heap snapshot for T2→T3 hibernation (ADR-008, Invariant 2)
`crates/core/src/ext.rs:852` **fn** `new` — Create a new suspended heap from compressed bytes
`crates/core/src/ext.rs:857` **fn** `len` — Get the size in bytes of the compressed snapshot
`crates/core/src/ext.rs:862` **fn** `is_empty` — Check if the snapshot is empty
`crates/core/src/ext.rs:869` **enum** `JsValue` — Простые JSON-совместимые типы для передачи через trait-границу
`crates/core/src/ext.rs:882` **fn** `object` — Хелпер: построить object из key-value пар
`crates/core/src/ext.rs:892` **enum** `JsError` — Ошибка исполнения JavaScript: либо syntax error (parse), либо runtime
`crates/core/src/ext.rs:913` **type** `JsResult`
`crates/core/src/ext.rs:918` **struct** `NullJsRuntime` — Null implementation — всегда возвращает `JsError::NotImplemented`
`crates/core/src/ext.rs:968` **trait** `UnicodeProvider` — Unicode-таблицы: line break (UAX #14), grapheme/word segmentation
`crates/core/src/ext.rs:993` **struct** `NullUnicodeProvider` — Null-реализация `UnicodeProvider` — все методы возвращают пустые векторы
`crates/core/src/ext.rs:1021` **trait** `IdnaProvider` — IDN (Internationalized Domain Names) полный UTS #46. Свой Punycode-encoder
`crates/core/src/ext.rs:1031` **struct** `NullIdnaProvider` — Null-реализация `IdnaProvider` — все методы возвращают `None`. Потребитель
`crates/core/src/ext.rs:1056` **trait** `PublicSuffixList` — Public Suffix List — отделение публичных суффиксов от регистрируемых
`crates/core/src/ext.rs:1077` **struct** `NullPublicSuffixList` — Null-реализация `PublicSuffixList` — все запросы возвращают `None`/`false`
`crates/core/src/ext.rs:1103` **trait** `ContentDecoder` — HTTP `Content-Encoding` декодер. Один экземпляр trait-а = один кодек
`crates/core/src/ext.rs:1118` **struct** `UnsupportedContentDecoder` — Stub-реализация `ContentDecoder` для encoding-а, на который нет
`crates/core/src/ext.rs:1149` **trait** `FontFormat` — Декодер альтернативных файловых форматов шрифта (WOFF2, WOFF) в raw
`crates/core/src/ext.rs:1167` **struct** `NullFontFormat` — Null-реализация `FontFormat` — `can_decode` всегда `false`,
`crates/core/src/ext.rs:1192` **trait** `ImageDecoder` — Plug-in декодер растровых изображений для форматов, не встроенных в
`crates/core/src/ext.rs:1219` **trait** `SpellChecker` — Spell checker — проверка орфографии для form field / contenteditable
`crates/core/src/ext.rs:1233` **struct** `NullSpellChecker` — Null-реализация `SpellChecker` — `check` всегда возвращает `true`, чтобы
`crates/core/src/ext.rs:1250` **trait** `HyphenationProvider` — Hyphenation — поиск позиций мягких переносов для CSS `hyphens: auto`
`crates/core/src/ext.rs:1261` **struct** `NullHyphenationProvider` — Null-реализация `HyphenationProvider` — никаких переносов не предлагается
`crates/core/src/ext.rs:1278` **enum** `WsMessage` — Сообщение, полученное от WebSocket-сервера (RFC 6455 §5.6)
`crates/core/src/ext.rs:1294` **trait** `WebSocketSession` — Открытое WebSocket-соединение. Объект владеет TCP/TLS-стримом
`crates/core/src/ext.rs:1310` **trait** `WebSocketProvider` — Фабрика WebSocket-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1328` **struct** `SseEvent` — Полностью разобранное SSE-событие (HTML Living Standard §9.2.6)
`crates/core/src/ext.rs:1344` **trait** `SseSession` — Открытое SSE-соединение (EventSource). Блокирующий интерфейс
`crates/core/src/ext.rs:1361` **trait** `SseProvider` — Фабрика SSE-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1377` **enum** `JsSseEvent` — A single queued event from an SSE connection, ready for delivery to JS
`crates/core/src/ext.rs:1401` **trait** `JsSseSession` — A live SSE connection from the JS runtime's perspective
`crates/core/src/ext.rs:1412` **trait** `JsSseProvider` — Factory that opens SSE connections for the JS runtime
`crates/core/src/ext.rs:1438` **trait** `FetchInterceptor` — Перехватчик fetch-запросов уровня Service Worker
`crates/core/src/ext.rs:1450` **struct** `JsFetchResult` — Full HTTP response for a synchronous JS `fetch()` call
`crates/core/src/ext.rs:1469` **trait** `JsFetchProvider` — Synchronous HTTP fetch bridge for the JS runtime
`crates/core/src/ext.rs:1504` **trait** `ClipboardProvider` — Synchronous access to the host platform clipboard for the JS runtime
`crates/core/src/ext.rs:1525` **enum** `WebAuthnError` — Failure reason from a [`CredentialProvider`] operation
`crates/core/src/ext.rs:1543` **fn** `dom_exception_name` — The `DOMException` name `lumen-js` should reject the promise with
`crates/core/src/ext.rs:1559` **struct** `WebAuthnCreateRequest` — A WebAuthn credential-creation (registration) request
`crates/core/src/ext.rs:1589` **struct** `WebAuthnCreateResponse` — The result of a successful [`CredentialProvider::create`]
`crates/core/src/ext.rs:1612` **struct** `WebAuthnGetRequest` — A WebAuthn assertion (authentication) request
`crates/core/src/ext.rs:1629` **struct** `WebAuthnGetResponse` — The result of a successful [`CredentialProvider::get`]
`crates/core/src/ext.rs:1659` **trait** `CredentialProvider` — Provider of WebAuthn / passkey credentials, backing `navigator.credentials`
`crates/core/src/ext.rs:1679` **enum** `JsWsEvent` — A single queued event from a WebSocket connection, ready for delivery to JS
`crates/core/src/ext.rs:1709` **trait** `JsWebSocketSession` — A live WebSocket connection from the JS runtime's perspective
`crates/core/src/ext.rs:1724` **trait** `JsWebSocketProvider` — Factory that opens WebSocket connections for the JS runtime
`crates/core/src/ext.rs:1751` **trait** `IdbBackend` — Persistence boundary for the IndexedDB JS shim
`crates/core/src/ext.rs:1774` **trait** `SwBackend` — Per-origin Service Worker registration persistence
`crates/core/src/ext.rs:1791` **enum** `ClockMode` — Clock mode for deterministic testing (BrowserSession::set_clock, 8F.1)
`crates/core/src/ext.rs:1815` **trait** `BrowserSession` — Browser automation session — unified interface for in-process tests, MCP agents,
`crates/core/src/ext.rs:1930` **struct** `NullBrowserSession` — Null implementation of `BrowserSession` — all methods return `NotImplemented`
`crates/core/src/ext.rs:2036` **enum** `MemoryPressureLevel` — OS memory pressure level (ADR-008, task 10H)
`crates/core/src/ext.rs:2056` **trait** `MemoryPressureSource` — Source of OS memory pressure signals (ADR-008, task 10H)
`crates/core/src/ext.rs:2063` **struct** `NullMemoryPressureSource` — Null implementation — always reports `Low`. For tests and platforms without
`crates/core/src/ext.rs:2085` **trait** `EvictableCache` — Common interface for all cross-tab shared memory caches (ADR-008, task 10D.3)
`crates/core/src/ext.rs:2119` **struct** `CacheRegistry` — Registry of all cross-tab shared memory caches (ADR-008, task 10D.3)
`crates/core/src/ext.rs:2125` **fn** `new` — Create an empty registry
`crates/core/src/ext.rs:2130` **fn** `register` — Register a cache. Caches are notified in registration order
`crates/core/src/ext.rs:2135` **fn** `broadcast_pressure` — Broadcast a memory pressure event to all registered caches
`crates/core/src/ext.rs:2142` **fn** `total_used_bytes` — Total memory currently used across all registered caches, in bytes
`crates/core/src/ext.rs:2150` **fn** `total_budget_bytes` — Total memory budget across all caches with a finite budget, in bytes
`crates/core/src/ext.rs:2159` **fn** `clear_all` — Evict all entries in every registered cache
`crates/core/src/ext.rs:2166` **fn** `len` — Number of registered caches
`crates/core/src/ext.rs:2171` **fn** `is_empty` — `true` if no caches are registered
`crates/core/src/form.rs:15` **struct** `FormEntry` — Запись формы — пара (name, value) с опциональным filename (для multipart)
`crates/core/src/form.rs:21` **enum** `FormValue`
`crates/core/src/form.rs:33` **fn** `text`
`crates/core/src/form.rs:40` **fn** `file`
`crates/core/src/form.rs:62` **fn** `encode_form_urlencoded` — Сериализует form-set как `application/x-www-form-urlencoded`
`crates/core/src/form.rs:97` **fn** `decode_form_value` — Decode urlencoded form value: `+` → пробел; `%HH` → байт. Не-валидные
`crates/core/src/form.rs:129` **fn** `encode_form_multipart` — Сериализует form-set как `multipart/form-data` (RFC 7578)
`crates/core/src/geom.rs:9` **struct** `Point`
`crates/core/src/geom.rs:23` **struct** `Size`
`crates/core/src/geom.rs:40` **struct** `Rect`
`crates/core/src/geom.rs:73` **fn** `origin`
`crates/core/src/geom.rs:80` **fn** `size`
`crates/core/src/geom.rs:87` **fn** `right`
`crates/core/src/geom.rs:91` **fn** `bottom`
`crates/core/src/hash.rs:30` **fn** `sha256` — SHA-256 хеш произвольных байт по FIPS 180-4
`crates/core/src/hash.rs:122` **fn** `hex_lower` — Закодировать байты в lowercase hex (без префиксов, без separator-ов)
`crates/core/src/hash.rs:135` **fn** `sha256_hex` — `hex_lower(&sha256(input))` — самая частая комбинация (HTTP Digest auth,
`crates/core/src/hash.rs:145` **fn** `sha1` — SHA-1 хеш произвольных байт по FIPS 180-3
`crates/core/src/hash.rs:207` **fn** `base64_encode` — Кодировать байты в Base64 по RFC 4648 §4 (стандартный алфавит, padding '=')
`crates/core/src/hash.rs:228` **fn** `ws_accept_key`
`crates/core/src/idn.rs:24` **fn** `domain_to_ascii` — Преобразует домен в ASCII-форму (IDNA `ToASCII`)
`crates/core/src/idn.rs:53` **fn** `ensure_ascii` — Идемпотентная версия [`domain_to_ascii`] — если вход уже ASCII (например,
`crates/core/src/idn.rs:59` **type** `IdnError` — Ошибка для случаев, когда метка не может быть закодирована. Пока
`crates/core/src/json.rs:15` **enum** `JsonValue`
`crates/core/src/json.rs:27` **fn** `as_str`
`crates/core/src/json.rs:35` **fn** `as_number`
`crates/core/src/json.rs:43` **fn** `as_bool`
`crates/core/src/json.rs:51` **fn** `as_array`
`crates/core/src/json.rs:59` **fn** `as_object`
`crates/core/src/json.rs:67` **fn** `get`
`crates/core/src/json.rs:73` **enum** `JsonError`
`crates/core/src/json.rs:159` **type** `JsonResult`
`crates/core/src/json.rs:161` **fn** `parse`
`crates/core/src/memory_pressure.rs:22` **struct** `Win32MemoryPressureSource` — Win32 memory pressure source via `GlobalMemoryStatusEx` polling
`crates/core/src/memory_pressure.rs:28` **struct** `MemoryStatusEx` — MEMORYSTATUSEX (Windows SDK, winbase.h)
`crates/core/src/memory_pressure.rs:42` **fn** `GlobalMemoryStatusEx`
`crates/core/src/memory_pressure.rs:46` **fn** `memory_load_percent` — Returns memory load as a percentage (0–100), or `None` on API failure
`crates/core/src/memory_pressure.rs:94` **struct** `LinuxMemoryPressureSource` — Linux memory pressure source via `/proc/pressure/memory` PSI polling
`crates/core/src/memory_pressure.rs:143` **struct** `MacosMemoryPressureSource` — macOS memory pressure source via `host_statistics64(HOST_VM_INFO64)` polling
`crates/core/src/memory_pressure.rs:153` **struct** `VmStatistics64` — Subset of `vm_statistics64` from `<mach/vm_statistics.h>` needed for
`crates/core/src/memory_pressure.rs:189` **fn** `mach_host_self` — Returns the mach port for the current host (libSystem, always available)
`crates/core/src/memory_pressure.rs:193` **fn** `host_statistics64` — Fills `host_info_out` with `HOST_VM_INFO64_COUNT` × `u32` words of
`crates/core/src/memory_pressure.rs:202` **fn** `vm_used_total` — Polls VM statistics and returns `(used_pages, total_pages)`, or `None` on error
`crates/core/src/module.rs:9` **trait** `Module`
`crates/core/src/punycode.rs:49` **fn** `encode` — Кодирует Unicode-строку в Punycode согласно RFC 3492
`crates/core/src/sandbox.rs:22` **struct** `SandboxFlags` — Битовое поле sandbox-ограничений. Конкретный бит == «**запрет** этой
`crates/core/src/sandbox.rs:67` **fn** `empty` — Пустой набор — sandbox не активен (без ограничений)
`crates/core/src/sandbox.rs:73` **fn** `all_restrictions` — Все ограничения активны — стартовое состояние для `<iframe sandbox>`
`crates/core/src/sandbox.rs:98` **fn** `contains` — `true` если **все** биты из `other` установлены в `self` —
`crates/core/src/sandbox.rs:104` **fn** `is_empty` — `true` если ни один бит не установлен (sandbox = пустой набор
`crates/core/src/sandbox.rs:109` **fn** `remove` — Снять биты `other` из `self` — используется парсером для `allow-*`
`crates/core/src/sandbox.rs:114` **fn** `insert` — Добавить биты `other`
`crates/core/src/sandbox.rs:119` **fn** `bits` — Удобство для тестов / shell-а: получить сырой битсет
`crates/core/src/sandbox.rs:150` **fn** `parse_sandbox_value` — Парсит значение HTML атрибута `sandbox` в [`SandboxFlags`]
`crates/core/src/sri.rs:16` **enum** `SriAlgorithm` — Алгоритм хеширования в SRI metadata
`crates/core/src/sri.rs:23` **fn** `as_str`
`crates/core/src/sri.rs:32` **fn** `digest_size` — Размер digest-а в байтах: SHA-256 → 32, SHA-384 → 48, SHA-512 → 64
`crates/core/src/sri.rs:52` **struct** `SriHash` — Одна запись `integrity` (один алгоритм + ожидаемый digest)
`crates/core/src/sri.rs:61` **struct** `IntegrityList` — Полный `integrity`-список (whitespace-separated). Если список пуст —
`crates/core/src/sri.rs:70` **fn** `parse` — Парсит integrity-атрибут. Whitespace-separated список `algo-base64`
`crates/core/src/sri.rs:85` **fn** `verify` — Проверить body через provider-хешер. Возвращает `Ok(true)` если
`crates/core/src/sri.rs:193` **trait** `DigestProvider` — Trait для подключения hash-implementaции извне
`crates/core/src/sri.rs:200` **enum** `SriError`
`crates/core/src/sri.rs:218` **type** `SriResult`
`crates/core/src/url.rs:23` **struct** `Url`
`crates/core/src/url.rs:36` **fn** `parse` — Распарсить URL. Минимально требуется непустая `scheme:`
`crates/core/src/url.rs:94` **fn** `scheme`
`crates/core/src/url.rs:98` **fn** `host`
`crates/core/src/url.rs:102` **fn** `port`
`crates/core/src/url.rs:106` **fn** `path`
`crates/core/src/url.rs:110` **fn** `query`
`crates/core/src/url.rs:114` **fn** `fragment`
`crates/core/src/url.rs:118` **fn** `as_str`
`crates/core/src/url.rs:123` **fn** `effective_port` — Порт с учётом дефолтов известных схем
`crates/core/src/url.rs:129` **fn** `host_ascii` — Host в ASCII-форме (Punycode) — для DNS, TLS SNI, Host header
`crates/core/src/url.rs:139` **fn** `path_and_query` — Path + `?query` (без fragment) — для HTTP request line
`crates/core/src/url.rs:148` **fn** `resolve` — Разрешить относительный или абсолютный `reference` относительно `self`
`crates/core/src/web_storage.rs:12` **struct** `WebStorage` — In-memory Web Storage partition (localStorage or sessionStorage)
`crates/core/src/web_storage.rs:19` **fn** `len` — Number of stored key-value pairs
`crates/core/src/web_storage.rs:24` **fn** `is_empty` — Returns `true` if the storage contains no items
`crates/core/src/web_storage.rs:29` **fn** `key` — Return the nth key in insertion order, or `None` if out of range
`crates/core/src/web_storage.rs:34` **fn** `get_item` — Return the value for `key`, or `None` if absent
`crates/core/src/web_storage.rs:39` **fn** `set_item` — Set `key` to `value`.  New keys are appended in insertion order
`crates/core/src/web_storage.rs:47` **fn** `remove_item` — Remove `key` and its value.  No-op if absent
`crates/core/src/web_storage.rs:54` **fn** `clear` — Remove all key-value pairs

## lumen-css-parser  (51 symbols)

`crates/engine/css-parser/src/parser.rs:38` **enum** `SimpleSelector`
`crates/engine/css-parser/src/parser.rs:50` **struct** `AttrSelector`
`crates/engine/css-parser/src/parser.rs:61` **enum** `AttrOp`
`crates/engine/css-parser/src/parser.rs:77` **enum** `PseudoClass`
`crates/engine/css-parser/src/parser.rs:345` **enum** `PseudoElementKind` — Pseudo-element селекторы (CSS Pseudo-Elements L4)
`crates/engine/css-parser/src/parser.rs:379` **enum** `DirArg` — Аргумент `:dir(...)` pseudo-class (CSS Selectors L4 §13.2)
`crates/engine/css-parser/src/parser.rs:390` **struct** `RelativeSelector` — Один элемент relative-selector-list-а из `:has()`. `combinator` — если
`crates/engine/css-parser/src/parser.rs:403` **struct** `NthSpec` — Формула `an+b` из CSS Selectors §6.6.5.1. Элемент с 1-based индексом `i`
`crates/engine/css-parser/src/parser.rs:413` **fn** `matches` — Возвращает true, если элемент с 1-based индексом `index` матчит формулу
`crates/engine/css-parser/src/parser.rs:432` **struct** `CompoundSelector`
`crates/engine/css-parser/src/parser.rs:437` **enum** `Combinator`
`crates/engine/css-parser/src/parser.rs:449` **struct** `ComplexSelector`
`crates/engine/css-parser/src/parser.rs:463` **fn** `specificity` — Specificity по CSS Selectors Level 3 §16:
`crates/engine/css-parser/src/parser.rs:553` **struct** `Specificity`
`crates/engine/css-parser/src/parser.rs:572` **struct** `Declaration`
`crates/engine/css-parser/src/parser.rs:581` **struct** `Rule`
`crates/engine/css-parser/src/parser.rs:592` **struct** `PropertyRule` — CSS Properties and Values L1 §1.1 — регистрация custom property через
`crates/engine/css-parser/src/parser.rs:600` **struct** `Stylesheet`
`crates/engine/css-parser/src/parser.rs:675` **struct** `FontPaletteValuesRule` — `@font-palette-values --name { font-family: ...; base-palette: N; override-colors: ... }`
`crates/engine/css-parser/src/parser.rs:690` **struct** `ContainerRule` — `@container <name>? <condition> { rules }` — CSS Containment L3 §3
`crates/engine/css-parser/src/parser.rs:703` **struct** `CounterStyleRule` — `@counter-style <name> { ... }` — CSS Counter Styles L3 §2
`crates/engine/css-parser/src/parser.rs:712` **struct** `PageRule` — `@page <selector>? { decls }` — CSS Paged Media L3 §3
`crates/engine/css-parser/src/parser.rs:723` **struct** `ScopeRule` — `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6
`crates/engine/css-parser/src/parser.rs:736` **struct** `StartingStyleRule` — `@starting-style { rules }` — CSS Transitions L2 §3.4. Контейнер
`crates/engine/css-parser/src/parser.rs:742` **struct** `KeyframesRule` — `@keyframes name { offset { decls } ... }` — CSS Animations L1 §3
`crates/engine/css-parser/src/parser.rs:751` **struct** `Keyframe`
`crates/engine/css-parser/src/parser.rs:760` **struct** `SupportsRule` — `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2
`crates/engine/css-parser/src/parser.rs:777` **enum** `SupportsCondition` — Условие в `@supports (...)`. Грамматика:
`crates/engine/css-parser/src/parser.rs:800` **fn** `evaluate` — Вычислить условие: вернуть `true`, если потребитель поддерживает
`crates/engine/css-parser/src/parser.rs:815` **struct** `LayerRule` — `@layer name { rules }` блок
`crates/engine/css-parser/src/parser.rs:825` **struct** `ImportRule` — `@import` декларация. Per CSS Cascade L4 §6.5 + Media Queries L4:
`crates/engine/css-parser/src/parser.rs:839` **struct** `FontFaceRule` — `@font-face { font-family: ...; src: url(...) format(...); ... }`
`crates/engine/css-parser/src/parser.rs:864` **struct** `FontFaceSource`
`crates/engine/css-parser/src/parser.rs:873` **enum** `FontFaceSourceKind`
`crates/engine/css-parser/src/parser.rs:882` **struct** `MediaRule` — Группа CSS-правил, вложенных в `@media`-блок
`crates/engine/css-parser/src/parser.rs:890` **struct** `MediaQuery` — Media query — OR-список AND-clauses (Media Queries L4 §3). Пустой
`crates/engine/css-parser/src/parser.rs:904` **struct** `MediaQueryClause` — Одна clause в media query — AND-список feature/media-type условий
`crates/engine/css-parser/src/parser.rs:916` **enum** `MediaCondition`
`crates/engine/css-parser/src/parser.rs:929` **enum** `MediaFeature`
`crates/engine/css-parser/src/parser.rs:953` **enum** `MediaOrientation`
`crates/engine/css-parser/src/parser.rs:959` **enum** `ColorScheme`
`crates/engine/css-parser/src/parser.rs:968` **struct** `MediaContext` — Контекст, против которого матчатся media queries. Заполняется
`crates/engine/css-parser/src/parser.rs:996` **fn** `matches` — Пустой query (= `@media all`) — true. Иначе хотя бы одна
`crates/engine/css-parser/src/parser.rs:1011` **fn** `matches` — Per Media Queries L4 §3.2: пустая `conditions` — clause invalid
`crates/engine/css-parser/src/parser.rs:1028` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:1038` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:1076` **fn** `parse`
`crates/engine/css-parser/src/parser.rs:1084` **fn** `parse_inline_style` — Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
`crates/engine/css-parser/src/parser.rs:1091` **fn** `parse_selector_list` — Парсит строку CSS selector list (через запятую) и возвращает разобранные
`crates/engine/css-parser/src/parser.rs:1249` **fn** `parse_supports_condition` — Парсит `@supports`-условие из строки между `@supports` и `{`
`crates/engine/css-parser/src/parser.rs:1447` **fn** `parse_media_query` — Распарсить media query из строки между `@media` и `{`. Принимает

## lumen-devtools  (8 symbols)

`crates/devtools/src/cdp.rs:18` **fn** `dispatch` — Обработать одно CDP сообщение, вернуть JSON-строку для отправки клиенту
`crates/devtools/src/server.rs:11` **struct** `DevToolsServer` — Фоновый DevTools сервер. Живёт пока не дропнется (join handle отсоединён)
`crates/devtools/src/server.rs:19` **fn** `spawn` — Запустить сервер на `127.0.0.1:port`. Не блокирует — поток в фоне
`crates/devtools/src/server.rs:28` **fn** `port`
`crates/devtools/src/ws.rs:12` **enum** `WsError`
`crates/devtools/src/ws.rs:42` **fn** `upgrade` — Прочитать HTTP Upgrade запрос, проверить заголовки, отправить 101
`crates/devtools/src/ws.rs:104` **fn** `read_text_frame` — Прочитать один WebSocket фрейм (RFC 6455 §5.2)
`crates/devtools/src/ws.rs:125` **fn** `write_text_frame` — Отправить text фрейм (server→client, без маски)

## lumen-dom  (205 symbols)

`crates/engine/dom/src/contenteditable.rs:10` **enum** `DomCommand` — A single, reversible DOM modification
`crates/engine/dom/src/contenteditable.rs:40` **struct** `PasteData` — Data from a paste operation (clipboard or drag-drop)
`crates/engine/dom/src/contenteditable.rs:54` **struct** `DragData` — Data transferred in a drag-drop operation
`crates/engine/dom/src/contenteditable.rs:69` **fn** `new` — Create empty paste data
`crates/engine/dom/src/contenteditable.rs:74` **fn** `with_text` — Set text content
`crates/engine/dom/src/contenteditable.rs:80` **fn** `with_html` — Set HTML content
`crates/engine/dom/src/contenteditable.rs:86` **fn** `add_file` — Add a file to the paste data
`crates/engine/dom/src/contenteditable.rs:92` **fn** `preferred_content` — Preferred content for insertion: HTML (if available), else plain text
`crates/engine/dom/src/contenteditable.rs:99` **fn** `new` — Create empty drag data
`crates/engine/dom/src/contenteditable.rs:104` **fn** `with_text` — Set text content
`crates/engine/dom/src/contenteditable.rs:110` **fn** `with_html` — Set HTML content
`crates/engine/dom/src/contenteditable.rs:116` **fn** `add_url` — Add a URL to the drag data
`crates/engine/dom/src/contenteditable.rs:122` **fn** `add_file` — Add a file to the drag data
`crates/engine/dom/src/contenteditable.rs:128` **fn** `mark_move` — Mark this as a move operation (not copy)
`crates/engine/dom/src/contenteditable.rs:134` **fn** `preferred_content` — Preferred content for insertion: HTML (if available), else plain text
`crates/engine/dom/src/contenteditable.rs:145` **struct** `CommandHistory` — History of executed commands for undo/redo
`crates/engine/dom/src/contenteditable.rs:156` **fn** `new` — Create an empty history
`crates/engine/dom/src/contenteditable.rs:164` **fn** `insert_text` — Execute InsertText command: insert text at position and record
`crates/engine/dom/src/contenteditable.rs:174` **fn** `delete_range` — Execute DeleteRange command: delete range and record (with deleted text)
`crates/engine/dom/src/contenteditable.rs:192` **fn** `replace_text` — Execute ReplaceText command: replace range with new text and record
`crates/engine/dom/src/contenteditable.rs:217` **fn** `undo` — Undo the last command (move backward in history)
`crates/engine/dom/src/contenteditable.rs:261` **fn** `redo` — Redo the last undone command (move forward in history)
`crates/engine/dom/src/contenteditable.rs:291` **fn** `can_undo` — True if undo is possible
`crates/engine/dom/src/contenteditable.rs:296` **fn** `can_redo` — True if redo is possible
`crates/engine/dom/src/contenteditable.rs:301` **fn** `clear` — Clear all history
`crates/engine/dom/src/contenteditable.rs:307` **fn** `len` — Return the number of commands in history
`crates/engine/dom/src/contenteditable.rs:312` **fn** `is_empty` — True if there are no commands in history
`crates/engine/dom/src/contenteditable.rs:317` **fn** `current_pos` — Return the current position in history (how many commands have been executed/redone)
`crates/engine/dom/src/contenteditable.rs:329` **fn** `paste_into` — Handle paste operation: insert paste data at selection or cursor position
`crates/engine/dom/src/contenteditable.rs:361` **fn** `drop_into` — Handle drop operation: insert drag data at drop position
`crates/engine/dom/src/lib.rs:28` **enum** `ViewportWidth` — Width dimension of a `<meta name=viewport>` tag
`crates/engine/dom/src/lib.rs:40` **struct** `ViewportMeta` — Parsed `<meta name="viewport" content="…">` descriptor
`crates/engine/dom/src/lib.rs:55` **enum** `DomSnapshotError` — Error returned by [`Document::to_bytes`] and [`Document::from_bytes`]
`crates/engine/dom/src/lib.rs:86` **struct** `NodeLimitExceeded` — Returned by [`Document::try_create_element`] when [`MAX_DOM_NODES`] is reached
`crates/engine/dom/src/lib.rs:97` **struct** `NodeId`
`crates/engine/dom/src/lib.rs:100` **fn** `index`
`crates/engine/dom/src/lib.rs:104` **fn** `from_index`
`crates/engine/dom/src/lib.rs:110` **enum** `Namespace`
`crates/engine/dom/src/lib.rs:120` **struct** `QualName`
`crates/engine/dom/src/lib.rs:126` **fn** `html`
`crates/engine/dom/src/lib.rs:135` **struct** `Attribute`
`crates/engine/dom/src/lib.rs:145` **enum** `ShadowRootMode` — Shadow root mode per Shadow DOM spec §4.2
`crates/engine/dom/src/lib.rs:160` **enum** `NodeData`
`crates/engine/dom/src/lib.rs:193` **struct** `Node`
`crates/engine/dom/src/lib.rs:200` **fn** `element_name`
`crates/engine/dom/src/lib.rs:209` **fn** `get_attr` — Возвращает значение атрибута по имени (ASCII case-insensitive). На
`crates/engine/dom/src/lib.rs:225` **fn** `sandbox_flags` — Sandbox-ограничения для `<iframe sandbox="...">` по HTML LS §7.6.5
`crates/engine/dom/src/lib.rs:237` **fn** `input_type` — HTML5 form input type для `<input type="...">`. Возвращает None
`crates/engine/dom/src/lib.rs:251` **fn** `input_mode` — Virtual keyboard hint for `<input inputmode="...">` and `<textarea inputmode="...">`
`crates/engine/dom/src/lib.rs:266` **enum** `InputType` — HTML5 form input types (HTML Standard §4.10.5). Спека определяет
`crates/engine/dom/src/lib.rs:318` **fn** `parse` — Распарсить значение `type`-атрибута. Case-insensitive по
`crates/engine/dom/src/lib.rs:347` **fn** `as_str`
`crates/engine/dom/src/lib.rs:378` **fn** `is_textual` — Текстовая семантика — поле с буквенным контентом, на котором
`crates/engine/dom/src/lib.rs:388` **fn** `is_button_like` — Кнопочная семантика — submit/reset/button/image, рендерится
`crates/engine/dom/src/lib.rs:402` **enum** `InputMode` — HTML Living Standard `inputmode` attribute values — hint to user agent about
`crates/engine/dom/src/lib.rs:424` **fn** `parse` — Parse `inputmode` attribute value. Case-insensitive per HTML spec
`crates/engine/dom/src/lib.rs:437` **fn** `as_str`
`crates/engine/dom/src/lib.rs:453` **struct** `FormInfo` — Данные `<form>` элемента — URL назначения, метод и число полей ввода
`crates/engine/dom/src/lib.rs:469` **enum** `FormSubmitEvent` — Результат попытки отправить форму (HTML5 §4.10.22 form submission algorithm)
`crates/engine/dom/src/lib.rs:495` **enum** `DocumentMode` — Парсинг-режим документа по HTML5 §13.2.6.2 «The insertion mode»
`crates/engine/dom/src/lib.rs:518` **struct** `DomPosition` — A position within the document (WHATWG DOM §4.4)
`crates/engine/dom/src/lib.rs:531` **struct** `Range` — A contiguous range of document content (WHATWG DOM §4.5)
`crates/engine/dom/src/lib.rs:540` **fn** `collapsed` — Collapsed range: both endpoints at `pos`
`crates/engine/dom/src/lib.rs:545` **fn** `is_collapsed` — True when start and end are the same position
`crates/engine/dom/src/lib.rs:557` **struct** `Selection` — The current document text selection (WHATWG Selection API)
`crates/engine/dom/src/lib.rs:566` **fn** `is_collapsed` — True when anchor == focus (or no selection)
`crates/engine/dom/src/lib.rs:575` **fn** `get_range` — The selection as a normalised Range (start ≤ end in node order)
`crates/engine/dom/src/lib.rs:590` **fn** `collapse` — Collapse the selection to a single point
`crates/engine/dom/src/lib.rs:596` **fn** `extend_focus` — Extend the focus end to `pos` (anchor stays fixed)
`crates/engine/dom/src/lib.rs:601` **fn** `clear` — Remove the selection entirely
`crates/engine/dom/src/lib.rs:620` **struct** `CompositionState` — Tracks the current IME composition session
`crates/engine/dom/src/lib.rs:635` **enum** `FontFaceStatus` — The status of a FontFace: whether it's been loaded, is loading, or failed
`crates/engine/dom/src/lib.rs:649` **struct** `FontFace` — Represents a @font-face rule and its loading status
`crates/engine/dom/src/lib.rs:668` **fn** `new` — Create a new FontFace from @font-face rule components
`crates/engine/dom/src/lib.rs:691` **struct** `FontFaceSet` — A collection of FontFace objects representing all @font-face rules in the document
`crates/engine/dom/src/lib.rs:698` **fn** `new` — Create a new empty FontFaceSet
`crates/engine/dom/src/lib.rs:705` **fn** `add` — Add a FontFace to the set
`crates/engine/dom/src/lib.rs:710` **fn** `size` — Get the number of FontFaces in the set
`crates/engine/dom/src/lib.rs:715` **fn** `has_family` — Check if the set contains a FontFace with a specific family name
`crates/engine/dom/src/lib.rs:720` **fn** `get_by_family` — Get all FontFaces with a specific family name
`crates/engine/dom/src/lib.rs:725` **fn** `all` — Get all FontFaces
`crates/engine/dom/src/lib.rs:730` **fn** `clear` — Clear all FontFaces from the set
`crates/engine/dom/src/lib.rs:737` **enum** `PerformanceEntryType` — Type of a performance entry (mark, measure, navigation, resource, etc.)
`crates/engine/dom/src/lib.rs:768` **struct** `PerformanceEntry` — A single performance entry (mark, measure, or resource timing)
`crates/engine/dom/src/lib.rs:781` **fn** `new` — Create a new performance entry
`crates/engine/dom/src/lib.rs:796` **fn** `end_time` — Get the end time of this entry (start_time + duration)
`crates/engine/dom/src/lib.rs:804` **struct** `PerformanceEntries` — Collection of performance entries
`crates/engine/dom/src/lib.rs:811` **fn** `new` — Create a new empty performance entries collection
`crates/engine/dom/src/lib.rs:818` **fn** `add_entry` — Add a performance entry
`crates/engine/dom/src/lib.rs:823` **fn** `all` — Get all performance entries
`crates/engine/dom/src/lib.rs:828` **fn** `get_by_type` — Get entries by type (mark, measure, etc.)
`crates/engine/dom/src/lib.rs:836` **fn** `get_by_name` — Get entries by name
`crates/engine/dom/src/lib.rs:844` **fn** `get_first_by_name` — Get a single entry by name (returns the first match)
`crates/engine/dom/src/lib.rs:849` **fn** `clear` — Clear all performance entries
`crates/engine/dom/src/lib.rs:854` **fn** `len` — Get the count of entries
`crates/engine/dom/src/lib.rs:859` **fn** `is_empty` — Check if the collection is empty
`crates/engine/dom/src/lib.rs:867` **struct** `PerformanceObserver` — Placeholder for PerformanceObserver observer registration
`crates/engine/dom/src/lib.rs:876` **fn** `new` — Create a new PerformanceObserver
`crates/engine/dom/src/lib.rs:884` **fn** `observe` — Add entry types to observe
`crates/engine/dom/src/lib.rs:889` **fn** `disconnect` — Disconnect the observer
`crates/engine/dom/src/lib.rs:895` **fn** `observed_types` — Get the observed entry types
`crates/engine/dom/src/lib.rs:900` **fn** `is_observing` — Check if this observer is watching a specific entry type
`crates/engine/dom/src/lib.rs:905` **fn** `set_handle` — Set the observer handle (assigned by shell runtime when registered)
`crates/engine/dom/src/lib.rs:910` **fn** `handle` — Get the observer handle
`crates/engine/dom/src/lib.rs:922` **struct** `Document`
`crates/engine/dom/src/lib.rs:979` **fn** `new`
`crates/engine/dom/src/lib.rs:1002` **fn** `root`
`crates/engine/dom/src/lib.rs:1010` **fn** `mode` — Текущий парсинг-режим. Tree builder выставляет его при
`crates/engine/dom/src/lib.rs:1016` **fn** `set_mode` — Установить режим. Использует tree builder при инициализации
`crates/engine/dom/src/lib.rs:1021` **fn** `viewport_meta` — Parsed `<meta name="viewport">` descriptor, if the page declared one
`crates/engine/dom/src/lib.rs:1027` **fn** `set_viewport_meta` — Set the viewport meta descriptor. Called by the HTML parser when it
`crates/engine/dom/src/lib.rs:1033` **fn** `get_selection` — Current selection. The shell updates this on mouse events; JS reads it
`crates/engine/dom/src/lib.rs:1038` **fn** `set_selection` — Replace the current selection
`crates/engine/dom/src/lib.rs:1043` **fn** `clear_selection` — Clear the selection
`crates/engine/dom/src/lib.rs:1058` **fn** `target` — Текущий target — id из URL fragment (без ведущего `#`), к которому
`crates/engine/dom/src/lib.rs:1065` **fn** `set_target` — Установить current target (id без `#`). `None` — нет fragment-а в URL
`crates/engine/dom/src/lib.rs:1077` **fn** `attach_shadow` — Attach a shadow root to `host` and return its `NodeId`
`crates/engine/dom/src/lib.rs:1084` **fn** `shadow_root_of` — Return the shadow root attached to `host`, or `None` if not a shadow host
`crates/engine/dom/src/lib.rs:1089` **fn** `is_shadow_host` — Whether `id` is a shadow host (has an attached shadow root)
`crates/engine/dom/src/lib.rs:1093` **fn** `get`
`crates/engine/dom/src/lib.rs:1097` **fn** `get_mut`
`crates/engine/dom/src/lib.rs:1101` **fn** `len`
`crates/engine/dom/src/lib.rs:1105` **fn** `is_empty`
`crates/engine/dom/src/lib.rs:1117` **fn** `base_href` — HTML5 §4.2.3 — найти первый `<base href="...">` в документе и
`crates/engine/dom/src/lib.rs:1128` **fn** `body` — Returns the `<body>` element's `NodeId`, walking root → `<html>` → `<body>`
`crates/engine/dom/src/lib.rs:1140` **fn** `find_first_element` — Найти первый элемент, удовлетворяющий предикату. Pre-order обход
`crates/engine/dom/src/lib.rs:1161` **fn** `find_by_id` — Find a node by its `id` attribute (case-sensitive, per HTML spec)
`crates/engine/dom/src/lib.rs:1189` **fn** `node_count` — Number of nodes currently allocated in this document's arena (including the root)
`crates/engine/dom/src/lib.rs:1195` **fn** `create_element` — Create an element unconditionally. Used by the HTML parser — does **not** enforce
`crates/engine/dom/src/lib.rs:1207` **fn** `try_create_element` — Create an element, returning `Err(`[`NodeLimitExceeded`]`)` if the arena already
`crates/engine/dom/src/lib.rs:1217` **fn** `create_text`
`crates/engine/dom/src/lib.rs:1221` **fn** `create_comment`
`crates/engine/dom/src/lib.rs:1231` **fn** `create_fragment` — Allocate a `DocumentFragment` node in the arena
`crates/engine/dom/src/lib.rs:1239` **fn** `set_template_content` — Register `fragment` as the content container for `template`
`crates/engine/dom/src/lib.rs:1245` **fn** `template_content` — Return the content `DocumentFragment` for a `<template>` element, or
`crates/engine/dom/src/lib.rs:1249` **fn** `create_doctype`
`crates/engine/dom/src/lib.rs:1263` **fn** `append_child` — Append `child` as the last child of `parent`. If `child` already has a parent, it is detached first
`crates/engine/dom/src/lib.rs:1275` **fn** `insert_after` — Insert `new_node` immediately after `reference` in their shared parent
`crates/engine/dom/src/lib.rs:1294` **fn** `detach` — Remove `node` from its current parent. The node itself stays in the arena and can be re-attached
`crates/engine/dom/src/lib.rs:1308` **fn** `insert_before` — Insert `new_node` immediately before `reference` in `reference`'s parent
`crates/engine/dom/src/lib.rs:1328` **fn** `deep_clone` — Deep-clone `node` and (if `deep`) all its descendants
`crates/engine/dom/src/lib.rs:1352` **fn** `acquire_js_ref` — Increment the JS wrapper reference count for `node_id`
`crates/engine/dom/src/lib.rs:1370` **fn** `release_js_ref` — Decrement the JS wrapper reference count for `node_id`
`crates/engine/dom/src/lib.rs:1386` **fn** `js_ref_count` — Returns the number of live JS wrapper objects currently referencing `node_id`
`crates/engine/dom/src/lib.rs:1399` **fn** `is_detached` — Returns `true` if `node_id` is not reachable from the document tree
`crates/engine/dom/src/lib.rs:1426` **fn** `dead_node_ids` — Returns the IDs of all nodes that are safe to collect from the arena
`crates/engine/dom/src/lib.rs:1469` **fn** `begin_composition` — Begin a new IME composition session in the given editable element
`crates/engine/dom/src/lib.rs:1486` **fn** `update_composition` — Update the active composition with new preedit text and selection range
`crates/engine/dom/src/lib.rs:1500` **fn** `end_composition` — End the active composition and return its final state
`crates/engine/dom/src/lib.rs:1510` **fn** `get_composition` — Get the current composition state without removing it
`crates/engine/dom/src/lib.rs:1518` **fn** `is_composing` — Check if an IME composition is currently active
`crates/engine/dom/src/lib.rs:1526` **fn** `get_composition_range` — Get the composition range (offset and length) if composition is active
`crates/engine/dom/src/lib.rs:1534` **fn** `get_composition_target` — Get the target node that is receiving composition input
`crates/engine/dom/src/lib.rs:1540` **fn** `fonts` — Get a reference to the document's FontFaceSet collection
`crates/engine/dom/src/lib.rs:1546` **fn** `fonts_mut` — Get a mutable reference to the document's FontFaceSet collection
`crates/engine/dom/src/lib.rs:1554` **fn** `set_timing_origin` — Set the timing origin (navigation start time in milliseconds since epoch)
`crates/engine/dom/src/lib.rs:1560` **fn** `current_time` — Get the current time relative to timing_origin (milliseconds)
`crates/engine/dom/src/lib.rs:1568` **fn** `mark` — Record a performance mark at the current time
`crates/engine/dom/src/lib.rs:1577` **fn** `measure` — Record a performance measure between two marks
`crates/engine/dom/src/lib.rs:1591` **fn** `performance_entries` — Get a reference to the performance entries collection
`crates/engine/dom/src/lib.rs:1597` **fn** `performance_entries_mut` — Get a mutable reference to the performance entries collection
`crates/engine/dom/src/lib.rs:1602` **fn** `performance_entries_by_type` — Get all performance entries of a specific type
`crates/engine/dom/src/lib.rs:1610` **fn** `performance_entries_by_name` — Get all performance entries with a specific name
`crates/engine/dom/src/lib.rs:1615` **fn** `clear_performance_entries` — Clear all performance entries
`crates/engine/dom/src/lib.rs:1628` **fn** `to_bytes` — Serialise the entire document to a compact binary blob (bincode)
`crates/engine/dom/src/lib.rs:1633` **fn** `from_bytes` — Deserialise a document from a binary blob produced by [`to_bytes`]
`crates/engine/dom/src/lib.rs:1730` **fn** `check_form_gate` — Гейт отправки форм по sandbox-флагу HTML §7.6.5
`crates/engine/dom/src/lib.rs:1751` **fn** `find_ancestor_form` — Найти ближайший предок `<form>` для узла `node`
`crates/engine/dom/src/lib.rs:1774` **fn** `collect_dom_form_fields` — Собрать имена и значения submittable-контролов формы из DOM-атрибутов
`crates/engine/dom/src/lib.rs:1876` **struct** `ValidityState` — Validity state for a form control — HTML5 §4.10.21.1 `ValidityState` interface
`crates/engine/dom/src/lib.rs:1901` **fn** `valid` — Returns `true` when all flags are `false` (element satisfies all constraints)
`crates/engine/dom/src/lib.rs:1922` **fn** `element_validity` — Returns the validity state for `node`, or `None` if the node is not a
`crates/engine/dom/src/lib.rs:2025` **fn** `check_validity_form` — Returns `true` if all submittable controls in `form_id` satisfy their
`crates/engine/dom/src/lib.rs:2033` **fn** `invalid_controls_in_form` — Returns the `NodeId`s of all invalid (failing constraint validation) controls
`crates/engine/dom/src/lib.rs:2050` **fn** `submit_form` — Execute HTML5 form submission algorithm (§4.10.22 «Form submission»)
`crates/engine/dom/src/lib.rs:2189` **struct** `AnchorInfo` — Информация об якорной ссылке (`<a href>`), найденной в документе
`crates/engine/dom/src/lib.rs:2222` **struct** `FlatTree` — Pre-computed composed tree (flat tree) for Shadow DOM layout traversal
`crates/engine/dom/src/lib.rs:2232` **fn** `children_of` — Composed-tree children of `id`
`crates/engine/dom/src/lib.rs:2247` **fn** `build_flat_tree` — Build the composed (flat) tree for the document
`crates/engine/dom/src/lib.rs:2342` **fn** `check_navigation_gate` — Гейт навигации по sandbox-флагу HTML §7.6.5
`crates/engine/dom/src/lib.rs:2366` **struct** `IframeInfo` — Данные `<iframe>` элемента — URL содержимого и sandbox-ограничения
`crates/engine/dom/src/lib.rs:2401` **fn** `collect_iframes` — Собрать все `<iframe>` элементы документа с их sandbox-ограничениями
`crates/engine/dom/src/lib.rs:2412` **fn** `check_popup_gate` — Гейт открытия popup-ов (`window.open()`, `target="_blank"`) по sandbox HTML §7.6.5
`crates/engine/dom/src/lib.rs:2431` **enum** `EditInputType` — Input event type per Input Events Level 2 §4.1.3
`crates/engine/dom/src/lib.rs:2462` **fn** `as_str` — The canonical `inputType` string for the `InputEvent` interface
`crates/engine/dom/src/lib.rs:2485` **struct** `InputEvent` — Data for a `beforeinput` or `input` DOM event (Input Events Level 2 §4.1)
`crates/engine/dom/src/lib.rs:2504` **fn** `trusted` — Construct a trusted input event (native input pipeline or automation
`crates/engine/dom/src/lib.rs:2515` **fn** `untrusted` — Construct an untrusted input event (synthesized by page script via
`crates/engine/dom/src/lib.rs:2536` **enum** `CompositionEventType` — Type of IME composition event (UI Events §5.2.5)
`crates/engine/dom/src/lib.rs:2547` **fn** `as_str` — The canonical DOM event name per UI Events §5.2.5
`crates/engine/dom/src/lib.rs:2561` **struct** `CompositionData` — Data for a `compositionstart` / `compositionupdate` / `compositionend` event
`crates/engine/dom/src/lib.rs:2588` **struct** `CompositionEvent` — An IME composition event (compositionstart / update / end)
`crates/engine/dom/src/lib.rs:2607` **fn** `new` — Create a new trusted composition event (native IME pipeline)
`crates/engine/dom/src/lib.rs:2619` **fn** `untrusted` — Create an untrusted composition event (synthesized by page script)
`crates/engine/dom/src/lib.rs:2630` **fn** `start` — Create a `compositionstart` event with initial IME text
`crates/engine/dom/src/lib.rs:2645` **fn** `update` — Create a `compositionupdate` event for interim preedit text
`crates/engine/dom/src/lib.rs:2660` **fn** `end` — Create a `compositionend` event for final committed text
`crates/engine/dom/src/lib.rs:2688` **fn** `split_text_node` — Split a text node at `byte_offset`, creating a second text node with the
`crates/engine/dom/src/lib.rs:2730` **fn** `insert_text_at` — Insert `text` into the text node at `pos`, returning the caret position
`crates/engine/dom/src/lib.rs:2788` **fn** `delete_range` — Delete the content of `range` from the document, returning a collapsed
`crates/engine/dom/src/lib.rs:2842` **fn** `insert_paragraph_break`
`crates/engine/dom/src/lib.rs:2871` **fn** `node_text_content` — Returns the full text content of `node` — concatenation of all descendant text nodes
`crates/engine/dom/src/lib.rs:2880` **fn** `node_child_count` — Number of direct DOM children of `node`
`crates/engine/dom/src/lib.rs:2889` **fn** `node_length` — DOM-spec "length" of `node`: UTF-16 code-unit count for text nodes, child
`crates/engine/dom/src/lib.rs:2901` **fn** `range_text` — Extracts the text covered by `range` (WHATWG DOM §4.6 `stringification`)

## lumen-driver  (83 symbols)

`crates/driver/src/context.rs:22` **struct** `SessionContext` — Isolated context for a single BrowserSession
`crates/driver/src/context.rs:45` **fn** `new` — Create a new context with default (Standard) fingerprint profile and real system clock
`crates/driver/src/context.rs:60` **fn** `with_fingerprint_profile` — Create a context with a specific fingerprint profile and real system clock
`crates/driver/src/context.rs:74` **fn** `fingerprint_profile`
`crates/driver/src/context.rs:78` **fn** `set_fingerprint_profile`
`crates/driver/src/context.rs:88` **fn** `user_agent`
`crates/driver/src/context.rs:94` **fn** `set_user_agent`
`crates/driver/src/context.rs:104` **fn** `clear_user_agent_override`
`crates/driver/src/context.rs:109` **fn** `clock_mode` — Returns the active clock mode
`crates/driver/src/context.rs:118` **fn** `set_clock_mode` — Set clock mode for `Date.now()` / `performance.now()` overrides (8F.1)
`crates/driver/src/context.rs:128` **fn** `read_clock_ms` — Read the current clock value in ms, advancing the monotonic counter if active
`crates/driver/src/context.rs:141` **fn** `frozen_clock_ms` — Convenience: returns `Some(ms)` only when clock is frozen (backward-compat)
`crates/driver/src/context.rs:149` **fn** `set_frozen_clock` — Set frozen clock (backward-compat wrapper; use `set_clock_mode` for new code)
`crates/driver/src/context.rs:154` **fn** `clear_frozen_clock` — Restore system clock (backward-compat wrapper; use `set_clock_mode` for new code)
`crates/driver/src/context.rs:159` **fn** `rng_seed` — Get RNG seed for deterministic randomness, or None if OS entropy is used
`crates/driver/src/context.rs:165` **fn** `set_rng_seed` — Set RNG seed for deterministic random numbers in JS Math.random() and crypto.getRandomValues()
`crates/driver/src/context.rs:170` **fn** `clear_rng_seed` — Clear RNG seed; resume using OS entropy
`crates/driver/src/context.rs:175` **fn** `is_fingerprint_frozen` — Check if fingerprint profile is frozen (cannot be changed)
`crates/driver/src/context.rs:181` **fn** `freeze_fingerprint` — Freeze current fingerprint profile: prevent further changes to set_fingerprint_profile()
`crates/driver/src/context.rs:186` **fn** `unfreeze_fingerprint` — Unfreeze fingerprint profile; allow changes again
`crates/driver/src/context.rs:190` **fn** `get_cookies_for_request`
`crates/driver/src/context.rs:195` **fn** `process_set_cookie`
`crates/driver/src/context.rs:202` **fn** `clear_cookies`
`crates/driver/src/context.rs:206` **fn** `get_storage`
`crates/driver/src/context.rs:212` **fn** `set_storage`
`crates/driver/src/context.rs:219` **fn** `clear_origin_storage`
`crates/driver/src/context.rs:223` **fn** `clear_all_storage`
`crates/driver/src/context.rs:227` **fn** `storage_keys`
`crates/driver/src/context.rs:234` **fn** `get_cached_response`
`crates/driver/src/context.rs:238` **fn** `cache_response`
`crates/driver/src/context.rs:242` **fn** `clear_http_cache`
`crates/driver/src/determinism.rs:39` **struct** `DeterministicConfig` — Configuration bundle for enabling deterministic mode on a `BrowserSession`
`crates/driver/src/determinism.rs:65` **fn** `with_seed` — Convenience constructor: fully deterministic mode with a specific RNG seed
`crates/driver/src/determinism.rs:77` **fn** `for_snapshot` — Convenience constructor for snapshot testing
`crates/driver/src/determinism.rs:89` **fn** `apply` — Apply this configuration to `session`
`crates/driver/src/determinism.rs:103` **fn** `seed_from_url` — Returns a deterministic u64 seed derived from a URL string
`crates/driver/src/gpu_session.rs:21` **struct** `RenderedPage` — Rendered page result from GpuSession rendering operations
`crates/driver/src/gpu_session.rs:53` **struct** `JsNavigateRequest` — Navigation request initiated by JS code (location.href=, history.pushState, etc)
`crates/driver/src/gpu_session.rs:64` **trait** `GpuSession` — Extended `BrowserSession` trait for GPU and streaming operations
`crates/driver/src/isolation.rs:40` **struct** `OriginGroup` — eTLD+1 site identifier used to group related origins
`crates/driver/src/isolation.rs:53` **fn** `for_origin` — Derive the origin group from a full origin URL or host string
`crates/driver/src/isolation.rs:70` **struct** `OriginIsolationContext` — Per-origin-group isolation container
`crates/driver/src/isolation.rs:89` **fn** `new` — Create a new isolation context for the given origin (URL or host string)
`crates/driver/src/isolation.rs:107` **fn** `site` — The site identifier (eTLD+1) of this context's origin group
`crates/driver/src/isolation.rs:115` **fn** `local_storage_for` — Get (or create) the `localStorage` partition for `origin`
`crates/driver/src/isolation.rs:126` **fn** `session_storage_for` — Get (or create) the `sessionStorage` partition for `origin`
`crates/driver/src/isolation.rs:134` **fn** `clear_session_storage_for` — Clear `sessionStorage` for `origin` (spec: cleared on top-level navigation)
`crates/driver/src/isolation.rs:139` **fn** `clear_all_session_storage` — Clear all `sessionStorage` partitions in this context
`crates/driver/src/isolation.rs:148` **fn** `idb_store_for` — Create an `IdbStore` scoped to `origin` using this context's backend
`crates/driver/src/isolation.rs:153` **fn** `idb_save` — Save an IndexedDB JSON snapshot for `origin`
`crates/driver/src/isolation.rs:158` **fn** `idb_load` — Load the IndexedDB JSON snapshot for `origin`, or `None` if absent
`crates/driver/src/isolation.rs:166` **fn** `cookie_jar` — Shared `Arc<CookieJar>` for this origin group
`crates/driver/src/isolation.rs:171` **fn** `same_group` — Check whether two origins belong to the same origin group (same eTLD+1)
`crates/driver/src/lib.rs:62` **trait** `BrowserSession` — Программный интерфейс к браузерному сеансу
`crates/driver/src/session.rs:52` **struct** `InProcessSession` — Headless in-process сессия браузера
`crates/driver/src/session.rs:83` **fn** `new` — Создать сессию с viewport 1024×720
`crates/driver/src/session.rs:98` **fn** `with_viewport` — Создать сессию с заданным размером viewport (логические пиксели)
`crates/driver/src/session.rs:129` **fn** `with_origin_isolation` — Create a session with per-origin-group isolation (Phase 1: 8E)
`crates/driver/src/session.rs:147` **fn** `isolation_context` — Access the per-origin-group isolation context, if this session was
`crates/driver/src/session.rs:152` **fn** `isolation_context_mut` — Mutable access to the per-origin-group isolation context
`crates/driver/src/session.rs:162` **fn** `set_pending_js_tasks` — Установить количество pending JS microtask/callback для условия `JsIdle`
`crates/driver/src/session.rs:184` **fn** `navigate_html` — Загрузить HTML-строку без навигации по URL. Используется для тестов
`crates/driver/src/session.rs:243` **fn** `screenshot_cpu_rgba` — Детерминированный CPU-рендер текущей страницы в RGBA8 (tiny-skia)
`crates/driver/src/session.rs:259` **fn** `screenshot_cpu_png` — Детерминированный CPU-рендер текущей страницы в PNG (tiny-skia)
`crates/driver/src/session.rs:273` **fn** `display_list_for_compare` — Строит [`lumen_paint::DisplayList`] из текущего состояния страницы
`crates/driver/src/types.rs:15` **struct** `NodeRef` — Ссылка на DOM-узел, возвращаемая [`BrowserSession::query`]
`crates/driver/src/types.rs:30` **enum** `Target` — Цель для команд [`BrowserSession::click`], [`type_text`](BrowserSession::type_text),
`crates/driver/src/types.rs:41` **struct** `ScrollDelta` — Дельта скролла для [`BrowserSession::scroll`]
`crates/driver/src/types.rs:50` **enum** `WaitCondition` — Условие ожидания для [`BrowserSession::wait`]
`crates/driver/src/types.rs:65` **struct** `BoxModel` — Box-model одного узла из [`BrowserSession::layout_snapshot`]
`crates/driver/src/types.rs:82` **struct** `A11yState` — ARIA state flags for an accessibility node, derived from `lumen-a11y::AXState`
`crates/driver/src/types.rs:112` **struct** `A11yNode` — Узел accessibility-дерева из [`BrowserSession::a11y_tree`]
`crates/driver/src/types.rs:136` **struct** `NetworkEntry` — Запись из сетевого лога [`BrowserSession::network_log`]
`crates/driver/src/types.rs:149` **struct** `ConsoleEntry` — Запись из консоли [`BrowserSession::console_log`]
`crates/driver/src/types.rs:158` **enum** `ConsoleLevel` — Уровень console-сообщения
`crates/driver/src/types.rs:170` **struct** `ComputedProperties` — Значения вычисленных CSS-свойств элемента из [`BrowserSession::computed_style`]
`crates/driver/src/types.rs:185` **enum** `InputCommand` — Команда для injection в event-loop браузера с целью создания нативных DOM-событий
`crates/driver/src/types.rs:239` **enum** `AxQuery` — Запрос к accessibility-дереву для [`BrowserSession::query_a11y`] и [`query_a11y_all`](BrowserSession::query_a11y_all)
`crates/driver/src/types.rs:275` **enum** `FingerprintProfile` — Профиль отпечатка браузера (fingerprint profile) для BrowserSession
`crates/driver/src/types.rs:297` **fn** `to_http_profile` — Map this session-level profile to the network [`HttpProfile`] that drives
`crates/driver/src/winit_session.rs:65` **struct** `WinitSession` — Оконная сессия браузера
`crates/driver/src/winit_session.rs:86` **fn** `new` — Создать сессию с viewport 1024×720
`crates/driver/src/winit_session.rs:100` **fn** `with_viewport` — Создать сессию с заданным размером viewport (логические пиксели)

## lumen-encoding  (13 symbols)

`crates/engine/encoding/src/decoder.rs:14` **fn** `decode` — Декодирует байты в строку. Алиас для [`decode_to_string`], короткий и
`crates/engine/encoding/src/decoder.rs:21` **fn** `decode_to_string` — То же, что [`decode`], но с явным именем — для случаев, когда из
`crates/engine/encoding/src/detect.rs:16` **fn** `detect` — Главная точка входа. Возвращает кодировку, в которой следует декодировать
`crates/engine/encoding/src/detect.rs:89` **fn** `sniff_meta_charset` — Ищет `<meta charset>` или `<meta http-equiv="Content-Type" content="...; charset=X">`
`crates/engine/encoding/src/ext_impl.rs:17` **struct** `HeuristicDetector` — Детектор кодировок по умолчанию
`crates/engine/encoding/src/hyphenation_impl.rs:18` **struct** `KnuthLiangHyphenation` — Knuth–Liang hyphenation with per-locale lazy-loaded embedded dictionaries
`crates/engine/encoding/src/hyphenation_impl.rs:24` **fn** `new` — Create a new provider with an empty cache
`crates/engine/encoding/src/lib.rs:41` **enum** `Encoding` — Поддерживаемые в Phase 0 кодировки
`crates/engine/encoding/src/lib.rs:59` **fn** `name` — Стабильное имя кодировки. Используется в API детектора
`crates/engine/encoding/src/lib.rs:79` **fn** `from_label` — Парсит label кодировки (case-insensitive, с алиасами)
`crates/engine/encoding/src/unicode_provider.rs:23` **struct** `Icu4xUnicodeProvider` — ICU4x-провайдер Unicode-операций
`crates/engine/encoding/src/unicode_provider.rs:31` **fn** `new` — Создаёт провайдер с auto-режимом (LSTM/dictionary для CJK/Thai/etc)
`crates/engine/encoding/src/unicode_provider.rs:40` **fn** `new_latin` — Облегчённая версия — только Latin + UAX #14 rules, без LSTM

## lumen-font  (173 symbols)

`crates/engine/font/src/avar.rs:32` **struct** `AxisValueMap` — Одна пара (fromCoord → toCoord) в segment map оси. Координаты в
`crates/engine/font/src/avar.rs:44` **struct** `SegmentMap` — Segment map для одной оси: список пар, отсортированных по `from`
`crates/engine/font/src/avar.rs:55` **fn** `normalize` — Применяет piecewise-linear перенормализацию: ищет сегмент, в
`crates/engine/font/src/avar.rs:89` **struct** `Avar`
`crates/engine/font/src/avar.rs:97` **fn** `parse`
`crates/engine/font/src/avar.rs:131` **fn** `normalize` — Перенормализация для axis под индексом `axis_index`. `coord`
`crates/engine/font/src/binary.rs:8` **struct** `BinaryReader`
`crates/engine/font/src/binary.rs:14` **fn** `new`
`crates/engine/font/src/binary.rs:18` **fn** `position`
`crates/engine/font/src/binary.rs:22` **fn** `seek`
`crates/engine/font/src/binary.rs:26` **fn** `remaining`
`crates/engine/font/src/binary.rs:30` **fn** `skip`
`crates/engine/font/src/binary.rs:39` **fn** `read_bytes`
`crates/engine/font/src/binary.rs:46` **fn** `read_u8`
`crates/engine/font/src/binary.rs:52` **fn** `read_u16`
`crates/engine/font/src/binary.rs:57` **fn** `read_u32`
`crates/engine/font/src/binary.rs:62` **fn** `read_i16`
`crates/engine/font/src/binary.rs:67` **fn** `read_i32`
`crates/engine/font/src/binary.rs:73` **fn** `read_tag` — 4-байтовый ASCII-тег (например, `b"head"`, `b"glyf"`)
`crates/engine/font/src/cmap.rs:21` **struct** `Cmap`
`crates/engine/font/src/cmap.rs:31` **fn** `parse`
`crates/engine/font/src/cmap.rs:94` **fn** `glyph_index` — Возвращает glyph index для codepoint, либо `None` если не отображён
`crates/engine/font/src/delta_set_index_map.rs:30` **struct** `DeltaSetIndex` — Распакованный entry: пара индексов для lookup в `ItemVariationStore`
`crates/engine/font/src/delta_set_index_map.rs:36` **struct** `DeltaSetIndexMap`
`crates/engine/font/src/delta_set_index_map.rs:44` **fn** `parse`
`crates/engine/font/src/delta_set_index_map.rs:90` **fn** `get` — Возвращает `(outer, inner)` для glyph_id (или другого входного
`crates/engine/font/src/face.rs:11` **struct** `OffsetTable` — Заголовок TTF/OTF файла. Указывает, сколько таблиц в шрифте
`crates/engine/font/src/face.rs:27` **fn** `read`
`crates/engine/font/src/face.rs:40` **struct** `TableRecord` — Запись в каталоге таблиц: где в файле лежит конкретная таблица
`crates/engine/font/src/face.rs:48` **fn** `read`
`crates/engine/font/src/face.rs:59` **enum** `FontError`
`crates/engine/font/src/face.rs:91` **struct** `Font` — Распарсенный шрифт: каталог таблиц + ссылка на оригинальные байты
`crates/engine/font/src/face.rs:98` **fn** `parse`
`crates/engine/font/src/face.rs:118` **fn** `offset_table`
`crates/engine/font/src/face.rs:122` **fn** `tables`
`crates/engine/font/src/face.rs:128` **fn** `table` — Возвращает байты таблицы по 4-байтовому тегу, либо `None`,
`crates/engine/font/src/face.rs:135` **fn** `head`
`crates/engine/font/src/face.rs:140` **fn** `maxp`
`crates/engine/font/src/face.rs:145` **fn** `cmap`
`crates/engine/font/src/face.rs:150` **fn** `hhea`
`crates/engine/font/src/face.rs:155` **fn** `hmtx`
`crates/engine/font/src/face.rs:162` **fn** `loca`
`crates/engine/font/src/face.rs:169` **fn** `glyf`
`crates/engine/font/src/face.rs:174` **fn** `name`
`crates/engine/font/src/face.rs:179` **fn** `os2`
`crates/engine/font/src/face.rs:191` **fn** `post` — `post` — PostScript Information Table. Содержит italic angle и
`crates/engine/font/src/face.rs:201` **fn** `fvar` — `fvar` (Font Variations) — описание variation axes (wght / wdth / slnt /
`crates/engine/font/src/face.rs:212` **fn** `avar` — `avar` (Axis Variations) — piecewise-linear перенормализация осей из
`crates/engine/font/src/face.rs:226` **fn** `gvar` — `gvar` (Glyph Variations) — per-glyph variation deltas для outline
`crates/engine/font/src/face.rs:238` **fn** `hvar` — `HVAR` (Horizontal Metrics Variations) — variation deltas для
`crates/engine/font/src/face.rs:252` **fn** `advance_width_varied` — Advance width for `glyph_id` with HVAR variation deltas applied
`crates/engine/font/src/face.rs:276` **fn** `vvar` — `VVAR` (Vertical Metrics Variations) — зеркало `HVAR` для
`crates/engine/font/src/face.rs:293` **fn** `mvar` — `MVAR` (Metrics Variations) — variation deltas для глобальных
`crates/engine/font/src/face.rs:302` **fn** `glyph` — Удобная обёртка: glyph_id → outline. `None`, если глиф пустой
`crates/engine/font/src/face.rs:321` **fn** `glyph_resolved` — Возвращает глиф с рекурсивно развёрнутыми composite-компонентами:
`crates/engine/font/src/face.rs:350` **fn** `glyph_resolved_with_coords` — Variable-fonts вариант [`Font::glyph_resolved`]: применяет gvar deltas
`crates/engine/font/src/font_registry.rs:19` **struct** `FontRegistry` — Провайдер шрифтов с поддержкой @font-face: системные шрифты + URL-буферы
`crates/engine/font/src/font_registry.rs:28` **fn** `new`
`crates/engine/font/src/font_registry.rs:38` **fn** `with_dirs` — Registry backed by a custom-dir `SystemFontIndex` — for tests and
`crates/engine/font/src/font_registry.rs:52` **fn** `register_from_bytes` — Регистрирует шрифт из байт-буфера (TrueType / sfnt после декодирования
`crates/engine/font/src/font_registry.rs:88` **fn** `custom_face_count` — Количество зарегистрированных @font-face face-ов. Для тестов
`crates/engine/font/src/font_registry.rs:99` **fn** `resolve_local_bytes` — Resolves a `local()` @font-face source by matching the name against the system
`crates/engine/font/src/font_registry.rs:108` **fn** `face_bytes_for_family` — Возвращает байты первого загруженного face для данной семьи
`crates/engine/font/src/fvar.rs:25` **struct** `VariationAxis` — Одна variation axis. Все значения в native axis units (не CSS-нормализо-
`crates/engine/font/src/fvar.rs:53` **fn** `is_hidden`
`crates/engine/font/src/fvar.rs:60` **fn** `clamp` — Зажать значение в `[min, max]`. Полезно при подаче CSS-уровневого
`crates/engine/font/src/fvar.rs:76` **struct** `NamedInstance` — Одна named instance — фиксированная точка в пространстве variation axes,
`crates/engine/font/src/fvar.rs:95` **struct** `Fvar` — Все axes и instances из `fvar`. Порядок — как в таблице (важно: координаты
`crates/engine/font/src/fvar.rs:101` **fn** `parse`
`crates/engine/font/src/fvar.rs:224` **fn** `axis` — Найти axis по tag-у. Возвращает `None`, если в шрифте нет такой
`crates/engine/font/src/fvar.rs:232` **fn** `is_variable` — `true`, если шрифт имеет хотя бы одну variation axis. Для non-variable
`crates/engine/font/src/fvar.rs:240` **fn** `instance_by_name_id` — Найти named instance с указанным `subfamily_name_id`. Возвращает
`crates/engine/font/src/glyf.rs:25` **struct** `BoundingBox`
`crates/engine/font/src/glyf.rs:33` **struct** `OutlinePoint`
`crates/engine/font/src/glyf.rs:40` **struct** `Contour`
`crates/engine/font/src/glyf.rs:45` **enum** `Outline`
`crates/engine/font/src/glyf.rs:65` **enum** `Anchor` — Как компонент привязывается к parent-у
`crates/engine/font/src/glyf.rs:79` **struct** `CompositeComponent` — Один компонент composite-глифа: ссылка на другой глиф + 2×2 матрица + anchor
`crates/engine/font/src/glyf.rs:86` **struct** `Glyph`
`crates/engine/font/src/glyf.rs:92` **fn** `parse`
`crates/engine/font/src/glyf.rs:286` **struct** `Glyf` — Удобный view над байтами `glyf` для разбора глифа по offset/length из loca
`crates/engine/font/src/glyf.rs:291` **fn** `new`
`crates/engine/font/src/glyf.rs:295` **fn** `glyph_at`
`crates/engine/font/src/gvar.rs:47` **enum** `PointNumbers` — Какие точки glyph-а трогает variation: либо явный список индексов,
`crates/engine/font/src/gvar.rs:59` **struct** `TupleVariation` — Описание одной tuple-variation для glyph-а
`crates/engine/font/src/gvar.rs:79` **struct** `GlyphVariationData` — Полный набор tuple-variations для одного glyph-а
`crates/engine/font/src/gvar.rs:88` **struct** `Gvar` — Распарсенная gvar-таблица. Хранит per-glyph offsets в массив сырых
`crates/engine/font/src/gvar.rs:107` **fn** `parse`
`crates/engine/font/src/gvar.rs:179` **fn** `glyph_variation_data` — Сырой byte-slice glyph-variation-data для одного glyph-а. `None`,
`crates/engine/font/src/gvar.rs:197` **fn** `parse_glyph` — Декодирует `GlyphVariationData` для glyph-а. `None` если у glyph-а
`crates/engine/font/src/gvar.rs:465` **fn** `tuple_axis_scalar` — Per-axis scalar tent-функции для одной оси tuple-variation
`crates/engine/font/src/gvar.rs:512` **fn** `tuple_scalar` — Региональный scalar для всех осей tuple-variation: произведение per-axis
`crates/engine/font/src/head.rs:18` **struct** `Head`
`crates/engine/font/src/head.rs:28` **enum** `IndexToLocFormat`
`crates/engine/font/src/head.rs:36` **fn** `parse`
`crates/engine/font/src/hhea.rs:10` **struct** `Hhea`
`crates/engine/font/src/hhea.rs:19` **fn** `parse`
`crates/engine/font/src/hmtx.rs:12` **struct** `Hmtx`
`crates/engine/font/src/hmtx.rs:19` **fn** `parse`
`crates/engine/font/src/hmtx.rs:35` **fn** `advance_width`
`crates/engine/font/src/hmtx.rs:46` **fn** `left_side_bearing`
`crates/engine/font/src/hvar.rs:26` **struct** `Hvar`
`crates/engine/font/src/hvar.rs:38` **fn** `parse`
`crates/engine/font/src/hvar.rs:72` **fn** `advance_width_index` — `(outer, inner)`-индекс для advance width variations glyph_id
`crates/engine/font/src/hvar.rs:79` **fn** `lsb_index` — Аналогично для LSB. `None`-map → identity-fallback. Caller обычно
`crates/engine/font/src/hvar.rs:83` **fn** `rsb_index`
`crates/engine/font/src/hvar.rs:89` **fn** `has_lsb_variations` — `true`, если HVAR содержит хоть один map для LSB (т.е. шрифт
`crates/engine/font/src/hvar.rs:93` **fn** `has_rsb_variations`
`crates/engine/font/src/item_variation.rs:31` **struct** `RegionAxisCoordinates` — Один axis-сегмент региона: tent-функция со scalar = 1.0 в peak,
`crates/engine/font/src/item_variation.rs:50` **fn** `scalar` — Per-axis scalar для tent-функции в `coord`. Возвращает значение
`crates/engine/font/src/item_variation.rs:92` **struct** `VariationRegion` — Один variation region — кортеж `RegionAxisCoordinates` на каждую ось
`crates/engine/font/src/item_variation.rs:104` **fn** `scalar` — Региональный scalar — произведение per-axis scalars. Region
`crates/engine/font/src/item_variation.rs:120` **struct** `VariationRegionList` — Список всех регионов, на которые могут ссылаться item-variation-data
`crates/engine/font/src/item_variation.rs:134` **struct** `ItemVariationData` — Блок per-item delta-наборов: для `item_count` items, каждый item
`crates/engine/font/src/item_variation.rs:146` **struct** `ItemVariationStore` — Root variation store. `format == 1` для всех современных шрифтов
`crates/engine/font/src/item_variation.rs:155` **fn** `parse` — Parses an `ItemVariationStore` starting at the beginning of `data`
`crates/engine/font/src/item_variation.rs:198` **fn** `evaluate` — Вычисляет суммарный delta для item `(outer, inner)` при текущих
`crates/engine/font/src/item_variation.rs:219` **fn** `is_empty` — `true`, если store не содержит ни регионов, ни data blocks —
`crates/engine/font/src/loca.rs:17` **struct** `Loca`
`crates/engine/font/src/loca.rs:24` **fn** `parse`
`crates/engine/font/src/loca.rs:46` **fn** `glyph_range` — Возвращает `(offset, length)` в байтах внутри `glyf`-таблицы,
`crates/engine/font/src/maxp.rs:9` **struct** `Maxp`
`crates/engine/font/src/maxp.rs:14` **fn** `parse`
`crates/engine/font/src/mvar.rs:29` **struct** `ValueRecord` — Одна запись MVAR: tag метрики + (outer, inner) для lookup в IVS
`crates/engine/font/src/mvar.rs:42` **struct** `Mvar`
`crates/engine/font/src/mvar.rs:50` **fn** `parse`
`crates/engine/font/src/mvar.rs:102` **fn** `lookup` — Lookup `(outer, inner)` для метрики по tag-у. `None`, если запись
`crates/engine/font/src/mvar.rs:114` **fn** `is_sorted_by_tag` — Проверяет, что records отсортированы по tag — инвариант OpenType
`crates/engine/font/src/name.rs:41` **struct** `Name` — Минимальный набор строк, нужных font matcher-у
`crates/engine/font/src/name.rs:55` **fn** `parse`
`crates/engine/font/src/name.rs:85` **fn** `best_family` — «Лучшее» family name: typographic, если есть, иначе обычный family
`crates/engine/font/src/os2.rs:32` **struct** `Os2` — Расширенный набор полей `OS/2`
`crates/engine/font/src/os2.rs:112` **fn** `is_italic` — Italic flag из `fsSelection`
`crates/engine/font/src/os2.rs:117` **fn** `is_oblique` — Oblique flag (OS/2 v4+)
`crates/engine/font/src/os2.rs:123` **fn** `is_bold` — Bold flag из `fsSelection`. Не источник истины для веса —
`crates/engine/font/src/os2.rs:129` **fn** `stretch_percent` — Возвращает stretch в процентах (от 50 до 200)
`crates/engine/font/src/os2.rs:144` **fn** `parse`
`crates/engine/font/src/post.rs:18` **struct** `Post`
`crates/engine/font/src/post.rs:47` **fn** `parse`
`crates/engine/font/src/post.rs:71` **fn** `is_italic` — `true` если italic_angle != 0 (шрифт имеет slant). Удобный
`crates/engine/font/src/rasterizer.rs:20` **struct** `Bitmap`
`crates/engine/font/src/rasterizer.rs:35` **struct** `Rasterizer`
`crates/engine/font/src/rasterizer.rs:41` **fn** `new`
`crates/engine/font/src/rasterizer.rs:49` **fn** `scale`
`crates/engine/font/src/rasterizer.rs:55` **fn** `rasterize` — Растеризует simple-glyph. Возвращает `None` для composite-глифов
`crates/engine/font/src/system_fonts.rs:31` **struct** `SystemFontIndex` — Простой ленивый индекс системных шрифтов
`crates/engine/font/src/system_fonts.rs:44` **fn** `new` — Индекс, который при первом lookup просканирует стандартные пути
`crates/engine/font/src/system_fonts.rs:53` **fn** `with_dirs` — Индекс с явно заданным списком директорий — для тестов и
`crates/engine/font/src/system_fonts.rs:66` **fn** `family_count` — Сколько family-имён зарегистрировано. Для тестов и диагностики;
`crates/engine/font/src/variation.rs:80` **fn** `apply_variations_to_simple_outline` — Применяет набор `TupleVariation` к outline-контурам, имитируя
`crates/engine/font/src/variation_coords.rs:28` **struct** `VariationCoords` — Normalized variation coordinates for a font instance. Stores one f32 per axis
`crates/engine/font/src/variation_coords.rs:33` **fn** `empty` — Creates an empty coordinate vector (no variations applied; uses default
`crates/engine/font/src/variation_coords.rs:45` **fn** `from_css_settings` — Builds normalized coordinates from CSS `font-variation-settings` values
`crates/engine/font/src/variation_coords.rs:92` **fn** `as_slice` — Returns the coordinate vector as a slice
`crates/engine/font/src/variation_coords.rs:97` **fn** `as_mut_slice` — Returns the coordinate vector as a mutable slice (for P4 to update optical sizing)
`crates/engine/font/src/variation_coords.rs:102` **fn** `is_empty` — Returns true if no coordinates are set (default instance)
`crates/engine/font/src/variation_coords.rs:107` **fn** `len` — Returns the number of axes
`crates/engine/font/src/variation_coords.rs:114` **fn** `get_axis_by_tag` — Gets coordinate for a specific axis by tag (for debugging / CSS property hookup)
`crates/engine/font/src/variation_coords.rs:126` **fn** `set_axis_by_tag` — Sets a specific axis coordinate by tag
`crates/engine/font/src/vvar.rs:31` **struct** `Vvar`
`crates/engine/font/src/vvar.rs:45` **fn** `parse`
`crates/engine/font/src/vvar.rs:80` **fn** `advance_height_index` — `(outer, inner)`-индекс для advance height variations glyph_id
`crates/engine/font/src/vvar.rs:87` **fn** `tsb_index` — Аналогично для TSB. `None`-map → identity-fallback. Caller обычно
`crates/engine/font/src/vvar.rs:91` **fn** `bsb_index`
`crates/engine/font/src/vvar.rs:95` **fn** `v_org_index`
`crates/engine/font/src/vvar.rs:99` **fn** `has_tsb_variations`
`crates/engine/font/src/vvar.rs:103` **fn** `has_bsb_variations`
`crates/engine/font/src/vvar.rs:107` **fn** `has_v_org_variations`
`crates/engine/font/src/woff2.rs:19` **fn** `is_woff2` — Returns `true` if `data` begins with the WOFF2 magic signature
`crates/engine/font/src/woff2.rs:24` **fn** `is_woff1` — Returns `true` if `data` begins with the WOFF1 magic signature
`crates/engine/font/src/woff2.rs:480` **fn** `decode_woff2` — Decode WOFF2 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:688` **fn** `decode_woff1` — Decode WOFF1 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:753` **fn** `maybe_decode_font` — If `data` is WOFF2 or WOFF1, decode it and return the raw sfnt bytes

## lumen-html-parser  (43 symbols)

`crates/engine/html-parser/src/picture.rs:56` **struct** `PickedSource` — Финальный URL выбранного источника плюс author-объявленные
`crates/engine/html-parser/src/picture.rs:64` **struct** `PictureParams` — Параметры picker-а
`crates/engine/html-parser/src/picture.rs:90` **fn** `pick_picture_source` — Выбрать источник для `<picture>` элемента. См. модульный заголовок
`crates/engine/html-parser/src/picture.rs:136` **fn** `pick_img_source` — Выбрать источник для одиночного `<img>` элемента (`srcset` + `sizes` +
`crates/engine/html-parser/src/preload_scanner.rs:55` **enum** `PreloadHint` — Один speculative-fetch hint, извлечённый preload-сканером
`crates/engine/html-parser/src/preload_scanner.rs:105` **fn** `scan_preload_hints` — Пробежать по HTML и вернуть все subresource-hint-ы, найденные в
`crates/engine/html-parser/src/push_tokenizer.rs:32` **struct** `PushTokenizer` — Push-режим HTML5 токенизатора. См. module-level docs
`crates/engine/html-parser/src/push_tokenizer.rs:51` **fn** `new` — Создаёт новый `PushTokenizer` в исходном состоянии
`crates/engine/html-parser/src/push_tokenizer.rs:66` **fn** `feed` — Скармливает chunk токенизатору и возвращает токены, ставшие
`crates/engine/html-parser/src/push_tokenizer.rs:87` **fn** `feed_bytes` — Вариант [`PushTokenizer::feed`] для сырых байт из сети
`crates/engine/html-parser/src/push_tokenizer.rs:156` **fn** `end` — Финализирует ввод. Хвост буфера токенизируется как при EOF —
`crates/engine/html-parser/src/push_tokenizer.rs:169` **fn** `pending_len` — Количество ещё не потреблённых байт строкового буфера
`crates/engine/html-parser/src/quirks_mode.rs:18` **fn** `detect_document_mode` — Решение по §13.2.5.1. `public_id`/`system_id` — `None` если в
`crates/engine/html-parser/src/srcset.rs:15` **struct** `SrcsetCandidate` — Один кандидат из `srcset`
`crates/engine/html-parser/src/srcset.rs:23` **enum** `SrcsetDescriptor` — Дескриптор кандидата. По умолчанию `1x` (когда дескриптор
`crates/engine/html-parser/src/srcset.rs:48` **fn** `parse_srcset` — Распарсить значение `srcset` атрибута. Возвращает список кандидатов
`crates/engine/html-parser/src/srcset.rs:172` **fn** `pick_best_for_density` — Выбрать лучший кандидат по DPR для density-descriptors
`crates/engine/html-parser/src/srcset.rs:232` **enum** `SizeLength` — Длина в `sizes`-атрибуте. По HTML5 §4.8.4.4 значение — одиночный
`crates/engine/html-parser/src/srcset.rs:250` **struct** `SizesViewport` — Viewport-параметры для резолва `sizes` в CSS-пиксели. `root_font_size_px`
`crates/engine/html-parser/src/srcset.rs:269` **fn** `resolve` — Резолв длины в CSS-пиксели
`crates/engine/html-parser/src/srcset.rs:287` **enum** `Orientation` — Ориентация viewport-а для media-feature `orientation:`
`crates/engine/html-parser/src/srcset.rs:294` **enum** `ColorScheme` — CSS Media Queries L5 `prefers-color-scheme` значение
`crates/engine/html-parser/src/srcset.rs:306` **enum** `MediaClause` — Одиночный `<media-in-parens>` внутри media-condition (Media Queries L4
`crates/engine/html-parser/src/srcset.rs:360` **enum** `MediaCondition` — Media-condition в `<source media>` / `<img sizes>`-атрибутах
`crates/engine/html-parser/src/srcset.rs:370` **fn** `matches` — Принимает решение, удовлетворяет ли viewport условие
`crates/engine/html-parser/src/srcset.rs:383` **struct** `SourceSize` — Один элемент `sizes`-списка: опциональный media-condition + length
`crates/engine/html-parser/src/srcset.rs:402` **fn** `parse_sizes` — Распарсить значение `sizes`-атрибута. Возвращает список
`crates/engine/html-parser/src/srcset.rs:504` **fn** `parse_media_condition` — Распарсить media-condition. Lenient: `Unsupported` вместо `None` —
`crates/engine/html-parser/src/srcset.rs:697` **fn** `evaluate_sizes` — Вычислить эффективную «source size» в CSS-пикселях по `sizes` и
`crates/engine/html-parser/src/srcset.rs:724` **fn** `pick_best_for_width` — Выбрать лучший кандидат по w-descriptor (HTML5 §4.8.4.3.7)
`crates/engine/html-parser/src/tokenizer.rs:21` **enum** `Token`
`crates/engine/html-parser/src/tokenizer.rs:47` **struct** `Tokenizer`
`crates/engine/html-parser/src/tokenizer.rs:58` **fn** `new`
`crates/engine/html-parser/src/tokenizer.rs:71` **fn** `with_state` — Создаёт tokenizer с заранее заданным `text_only`-состоянием
`crates/engine/html-parser/src/tokenizer.rs:81` **fn** `pos` — Текущая позиция курсора (в байтах от начала `input`). Используется
`crates/engine/html-parser/src/tokenizer.rs:87` **fn** `text_only_state` — Текущее `text_only`-состояние. После исчерпания iterator-а это
`crates/engine/html-parser/src/tree_builder.rs:45` **fn** `parse` — Парсит вход целиком в pull-режиме и возвращает построенный
`crates/engine/html-parser/src/tree_builder.rs:119` **struct** `IncrementalTreeBuilder` — Push-режим tree builder-а: принимает HTML chunk-ами, держит
`crates/engine/html-parser/src/tree_builder.rs:160` **fn** `new` — Создаёт пустой builder в insertion mode `Initial`
`crates/engine/html-parser/src/tree_builder.rs:181` **fn** `feed` — Скармливает chunk push-токенизатору и применяет полученные
`crates/engine/html-parser/src/tree_builder.rs:188` **fn** `feed_bytes` — Вариант [`feed`][Self::feed] для сырых байт
`crates/engine/html-parser/src/tree_builder.rs:195` **fn** `as_doc` — Возвращает ссылку на текущее состояние DOM
`crates/engine/html-parser/src/tree_builder.rs:204` **fn** `finish` — Финализирует ввод. Хвост push-tokenizer-а токенизируется как

## lumen-image  (61 symbols)

`crates/engine/image/src/avif/mod.rs:19` **enum** `AvifError` — Ошибка декодирования AVIF
`crates/engine/image/src/avif/mod.rs:47` **fn** `is_avif` — Проверяет AVIF-сигнатуру по ISOBMFF ftyp-боксу
`crates/engine/image/src/avif/mod.rs:68` **fn** `decode_avif` — Декодирует AVIF-файл в RGBA8 (4 байта на пиксель, row-major)
`crates/engine/image/src/avif/mod.rs:96` **struct** `AvifImageDecoder` — Реализация [`lumen_core::ext::ImageDecoder`] для AVIF
`crates/engine/image/src/decode_cache.rs:17` **type** `ImageHandle` — A thin, reference-counted pointer to a decoded image stored in `ImageDecodeCache`
`crates/engine/image/src/decode_cache.rs:23` **struct** `ImageKey` — Cache key identifying a decoded image
`crates/engine/image/src/decode_cache.rs:27` **fn** `new` — Construct from a URL or hash string
`crates/engine/image/src/decode_cache.rs:52` **struct** `ImageDecodeCache` — LRU decode cache for decoded raster images
`crates/engine/image/src/decode_cache.rs:67` **fn** `new` — Create a new cache with the default 256 MB budget
`crates/engine/image/src/decode_cache.rs:72` **fn** `with_budget` — Create a new cache with a custom memory budget in bytes
`crates/engine/image/src/decode_cache.rs:82` **fn** `used_bytes` — Current memory used by all cached images (bytes)
`crates/engine/image/src/decode_cache.rs:87` **fn** `budget_bytes` — Memory budget (bytes)
`crates/engine/image/src/decode_cache.rs:92` **fn** `len` — Number of cached images
`crates/engine/image/src/decode_cache.rs:97` **fn** `is_empty` — `true` if no images are cached
`crates/engine/image/src/decode_cache.rs:102` **fn** `contains` — `true` if the key is present in the cache
`crates/engine/image/src/decode_cache.rs:109` **fn** `get` — Look up a cached image by key, updating its LRU timestamp
`crates/engine/image/src/decode_cache.rs:125` **fn** `insert` — Insert a decoded image into the cache and return a handle
`crates/engine/image/src/decode_cache.rs:158` **fn** `decode_or_get` — Decode and cache an image, or return the existing cached handle
`crates/engine/image/src/decode_cache.rs:173` **fn** `evict_to_budget` — Evict least-recently-used entries until `used_bytes <= budget_bytes`
`crates/engine/image/src/decode_cache.rs:201` **fn** `remove` — Remove a single cached entry by key
`crates/engine/image/src/decode_cache.rs:211` **fn** `clear` — Evict all cached entries regardless of budget
`crates/engine/image/src/decode_cache.rs:219` **fn** `lru_candidates` — Return LRU candidates sorted from least- to most-recently used
`crates/engine/image/src/decode_cache.rs:234` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting proportionally
`crates/engine/image/src/gif.rs:12` **enum** `GifError` — Ошибки декодирования GIF
`crates/engine/image/src/gif.rs:37` **fn** `is_gif` — Проверяет, является ли начало `bytes` валидной GIF сигнатурой (GIF87a или GIF89a)
`crates/engine/image/src/gif.rs:46` **struct** `AnimatedFrame` — Один кадр анимированного GIF
`crates/engine/image/src/gif.rs:58` **fn** `delay_ms` — Возвращает задержку в миллисекундах
`crates/engine/image/src/gif.rs:66` **enum** `GifLoopCount` — Количество повторений анимации GIF
`crates/engine/image/src/gif.rs:75` **struct** `AnimatedGif` — Анимированный GIF: кадры + размер + метаданные цикличности
`crates/engine/image/src/gif.rs:93` **fn** `frame_index_at` — Возвращает индекс кадра для `elapsed_ms` миллисекунд от начала анимации
`crates/engine/image/src/gif.rs:126` **fn** `frame_at` — Возвращает кадр для `elapsed_ms` миллисекунд от начала анимации
`crates/engine/image/src/gif.rs:140` **fn** `decode_gif` — Декодирует GIF файл и возвращает первый кадр
`crates/engine/image/src/gif.rs:164` **fn** `decode_gif_animated` — Декодирует все кадры GIF и возвращает [`AnimatedGif`]
`crates/engine/image/src/heic.rs:18` **struct** `HeicError` — Error decoding a HEIC/HEIF image
`crates/engine/image/src/heic.rs:33` **fn** `is_heic` — Detects HEIC/HEIF image format
`crates/engine/image/src/heic.rs:66` **fn** `decode_heic` — Stub HEIC/HEIF decoder (Phase 1)
`crates/engine/image/src/jpeg/mod.rs:94` **fn** `decode_jpeg`
`crates/engine/image/src/jpeg/mod.rs:126` **struct** `JpegError` — Ошибка декодирования JPEG (обёртка над zune-jpeg)
`crates/engine/image/src/jxl.rs:16` **struct** `JxlError` — Error decoding a JPEG XL image
`crates/engine/image/src/jxl.rs:32` **fn** `is_jxl` — Detects JPEG XL image format
`crates/engine/image/src/jxl.rs:70` **fn** `decode_jxl` — Stub JPEG XL decoder (Phase 0)
`crates/engine/image/src/lib.rs:31` **fn** `supported_mime_types` — MIME-типы изображений, которые `decode` умеет декодировать
`crates/engine/image/src/lib.rs:45` **fn** `decode` — Декодирует растровое изображение по сигнатуре первых байтов
`crates/engine/image/src/lib.rs:76` **enum** `ImageError` — Ошибка `decode`
`crates/engine/image/src/lib.rs:142` **enum** `IccGamut` — Идентифицированный цветовой охват ICC профиля
`crates/engine/image/src/lib.rs:157` **struct** `IccProfile` — ICC профиль изображения (опциональный)
`crates/engine/image/src/lib.rs:165` **fn** `is_valid` — Проверяет минимальный размер ICC профиля (128 байт)
`crates/engine/image/src/lib.rs:175` **fn** `detect_gamut` — Определяет цветовой охват по сигнатуре пространства данных (bytes 16-19)
`crates/engine/image/src/lib.rs:236` **fn** `correct_rgba_pixels` — Применяет ICC-коррекцию к RGBA8 пикселям in-place
`crates/engine/image/src/lib.rs:313` **struct** `Image` — Декодированное растровое изображение в плотной row-major упаковке
`crates/engine/image/src/lib.rs:325` **fn** `to_rgba8` — Возвращает пиксели в формате RGBA8 (4 байта на пиксель)
`crates/engine/image/src/lib.rs:351` **fn** `resize_bilinear` — Масштабирует `src` до `(dst_w × dst_h)` билинейной интерполяцией
`crates/engine/image/src/lib.rs:403` **fn** `resize_area_avg` — Масштабирует `src` до `(dst_w × dst_h)` усреднением по площади (box filter)
`crates/engine/image/src/lib.rs:462` **enum** `PixelFormat` — Формат пикселя декодированного изображения. Все варианты — 8 бит на канал
`crates/engine/image/src/lib.rs:486` **enum** `DecodeError` — Ошибки декодирования PNG
`crates/engine/image/src/png/mod.rs:54` **fn** `decode_png`
`crates/engine/image/src/png/mod.rs:96` **fn** `encode_png_rgba8` — Кодирует RGBA8 изображение в PNG формат
`crates/engine/image/src/webp/mod.rs:24` **struct** `WebpError` — Ошибка декодирования WebP
`crates/engine/image/src/webp/mod.rs:39` **fn** `is_webp` — Проверяет WebP-сигнатуру без полной валидации
`crates/engine/image/src/webp/mod.rs:52` **fn** `decode_webp` — Декодирует WebP-файл в RGBA8 (4 байта на пиксель, row-major)
`crates/engine/image/src/webp/mod.rs:88` **struct** `WebpImageDecoder` — Реализация [`lumen_core::ext::ImageDecoder`] для WebP

## lumen-js  (208 symbols)

`crates/js/src/audio_bindings.rs:37` **fn** `new_session_seed` — Generate a unique per-session noise seed
`crates/js/src/audio_bindings.rs:46` **fn** `install_audio_bindings` — Install the complete Web Audio API Level 2 into the JS context
`crates/js/src/audio_element.rs:28` **fn** `install_audio_element_bindings` — Install HTMLAudioElement stubs into the JS context
`crates/js/src/background_fetch.rs:22` **fn** `init_background_fetch` — Install the Background Fetch API stub into the JS context
`crates/js/src/background_sync.rs:17` **fn** `init_background_sync` — Install the Background Sync API stub into the JS context
`crates/js/src/badging.rs:12` **fn** `install_badging_bindings` — Install Badging API bindings into the JS context
`crates/js/src/battery_bindings.rs:22` **fn** `install_battery_bindings` — Install Battery Status API disable shim into the JS context
`crates/js/src/bluetooth.rs:5` **fn** `install_bluetooth_bindings`
`crates/js/src/broadcast_channel.rs:61` **struct** `LocalChannel` — A channel instance owned by the current runtime: the receiver half plus its id
`crates/js/src/broadcast_channel.rs:72` **type** `BroadcastRegistry` — All `BroadcastChannel` instances created in this runtime
`crates/js/src/broadcast_channel.rs:80` **fn** `register` — Register a new channel instance for `name` and return its unique id
`crates/js/src/broadcast_channel.rs:100` **fn** `post` — Deliver `json` to every channel named `name` except the sender (`sender_id`)
`crates/js/src/broadcast_channel.rs:119` **fn** `close` — Remove the channel instance `id` from the global hub and this runtime
`crates/js/src/broadcast_channel.rs:135` **fn** `drain` — Drain all pending messages addressed to this runtime's channels
`crates/js/src/broadcast_channel.rs:150` **fn** `install_broadcast_channel_bindings` — Install the `_lumen_bc_*` native bindings and the `BroadcastChannel` JS class
`crates/js/src/canvas2d.rs:66` **fn** `flush_dirty` — Drain dirty canvases and return their current RGBA buffers
`crates/js/src/canvas2d.rs:93` **fn** `install_canvas2d_bindings` — Register the `_lumen_canvas2d_*` native functions on `globals`
`crates/js/src/clipboard.rs:33` **fn** `set_clipboard_provider` — Install the host clipboard provider backing `navigator.clipboard`
`crates/js/src/compute_pressure.rs:8` **fn** `install_compute_pressure_bindings` — Install Compute Pressure API bindings into the JS context
`crates/js/src/contacts.rs:15` **fn** `init_contacts_manager` — Install the Contact Picker API stub into the JS context
`crates/js/src/cookie_banner.rs:30` **fn** `install_cookie_banner_bindings` — Install cookie-banner auto-dismiss shim into the JS context
`crates/js/src/cookie_banner.rs:160` **fn** `install_with_selectors` — Build the `_LUMEN_CONSENT_SELECTORS` global value and inject the shim
`crates/js/src/cookie_store.rs:17` **fn** `init_cookie_store` — Install the Cookie Store API into the JS context
`crates/js/src/credentials.rs:50` **fn** `set_credential_provider` — Install the host credential provider backing `navigator.credentials`
`crates/js/src/credentials.rs:66` **fn** `install_credentials_bindings` — Install the `navigator.credentials` JS shim
`crates/js/src/csp.rs:12` **fn** `install_csp_bindings` — Install CSP JS bindings: `SecurityPolicyViolationEvent` class and
`crates/js/src/css_properties_values_api.rs:14` **struct** `RegisteredPropertiesMap` — Maps property name (e.g. "--my-color") to its definition
`crates/js/src/css_properties_values_api.rs:19` **fn** `new`
`crates/js/src/css_properties_values_api.rs:24` **fn** `register` — Register a custom property definition
`crates/js/src/css_properties_values_api.rs:29` **fn** `get` — Look up a registered property by name
`crates/js/src/css_properties_values_api.rs:34` **fn** `all` — Get all registered properties
`crates/js/src/css_properties_values_api.rs:39` **fn** `clear` — Clear all registrations (for tests)
`crates/js/src/css_properties_values_api.rs:45` **fn** `get_registered_properties` — Get the global registered properties registry, initializing it if necessary
`crates/js/src/css_properties_values_api.rs:51` **struct** `RegisteredProperty` — Definition of a custom CSS property
`crates/js/src/css_properties_values_api.rs:64` **fn** `install_css_properties_values_api` — Install CSS.registerProperty bindings into the JS context
`crates/js/src/device_sensors.rs:8` **fn** `install_device_sensors_bindings`
`crates/js/src/document_pip.rs:8` **fn** `install_document_pip_api` — Install Document Picture-in-Picture API into the JS context
`crates/js/src/dom.rs:101` **enum** `NavigateRequest` — Navigation request emitted by JS (`location.href =`, `location.assign()`,
`crates/js/src/dom.rs:118` **enum** `HistoryUrlUpdate` — Notification emitted by `history.pushState`/`history.replaceState` so the
`crates/js/src/dom.rs:144` **struct** `PopupRequest` — A popup window request emitted by JS `window.open(url, target, features)`
`crates/js/src/dom.rs:162` **enum** `FullscreenRequest` — A fullscreen API request emitted by JS `element.requestFullscreen()` or
`crates/js/src/dom.rs:202` **fn** `install_dom_api` — Install DOM primitives (`_lumen_*`) and the Web API shim into `ctx`
`crates/js/src/dom_parser.rs:34` **fn** `install_dom_parser` — Install DOMParser and XMLSerializer into the JS context
`crates/js/src/element_internals.rs:10` **fn** `install_element_internals_bindings` — Install ElementInternals and CustomStateSet bindings into the JS context
`crates/js/src/esm.rs:25` **type** `SharedPageUrl` — Shared, late-writable page URL used by `LumenResolver` to resolve relative
`crates/js/src/esm.rs:32` **type** `ModuleRegistry` — Shared module source registry: specifier → source code
`crates/js/src/esm.rs:35` **fn** `new_registry` — Creates an empty `ModuleRegistry`
`crates/js/src/esm.rs:44` **struct** `ImportMap` — Import map: specifier mappings for bare specifiers and scoped paths
`crates/js/src/esm.rs:56` **fn** `parse` — Parse an import map from a JSON string
`crates/js/src/esm.rs:92` **fn** `resolve` — Resolve a specifier using this import map
`crates/js/src/esm.rs:135` **struct** `LumenResolver` — URL resolver: normalises module specifiers into canonical keys for the registry
`crates/js/src/esm.rs:145` **fn** `new` — Create a resolver; `page_url` is the initial fallback base (may be empty)
`crates/js/src/esm.rs:154` **fn** `set_import_map` — Set the import map for this resolver
`crates/js/src/esm.rs:168` **fn** `resolve_specifier` — Resolve `name` relative to `base` using simplified URL resolution rules
`crates/js/src/esm.rs:219` **struct** `LumenLoader` — Module loader backed by `ModuleRegistry`
`crates/js/src/esm.rs:225` **fn** `new` — Create a loader backed by `registry`
`crates/js/src/eye_dropper.rs:8` **fn** `install_eye_dropper_bindings`
`crates/js/src/form_validation.rs:9` **fn** `install_form_validation_bindings` — Install Form Constraint Validation API bindings into the JS context
`crates/js/src/gamepad.rs:31` **fn** `install_gamepad_bindings` — Install Gamepad API shim into the JS context
`crates/js/src/generic_sensor.rs:16` **fn** `install_generic_sensor_bindings` — Install Generic Sensor API bindings into the JS context
`crates/js/src/geolocation.rs:25` **struct** `FakeCoords` — Fake geographic coordinates injected into the Geolocation API
`crates/js/src/geolocation.rs:43` **fn** `install_geolocation_bindings` — Install the Geolocation API stub into the JS context
`crates/js/src/heap_snapshot.rs:40` **enum** `HeapSnapshotError` — Error from the heap-snapshot compression layer
`crates/js/src/heap_snapshot.rs:74` **fn** `compress_heap` — Compress a raw heap payload into a [`SuspendedHeap`]
`crates/js/src/heap_snapshot.rs:97` **fn** `decompress_heap` — Inverse of [`compress_heap`]: strip the [`HEAP_MAGIC`] prefix and inflate
`crates/js/src/highlight_api.rs:10` **struct** `HighlightRegistry`
`crates/js/src/highlight_api.rs:15` **fn** `new`
`crates/js/src/highlight_api.rs:19` **fn** `set`
`crates/js/src/highlight_api.rs:23` **fn** `get`
`crates/js/src/highlight_api.rs:27` **fn** `has`
`crates/js/src/highlight_api.rs:31` **fn** `delete`
`crates/js/src/highlight_api.rs:35` **fn** `clear`
`crates/js/src/highlight_api.rs:39` **fn** `all`
`crates/js/src/highlight_api.rs:47` **fn** `get_highlights_registry`
`crates/js/src/highlight_api.rs:52` **struct** `Highlight`
`crates/js/src/highlight_api.rs:58` **fn** `new`
`crates/js/src/highlight_api.rs:66` **fn** `install_highlight_api_bindings`
`crates/js/src/iframe_element.rs:30` **fn** `install_iframe_element_bindings` — Install HTMLIFrameElement stubs into the JS context
`crates/js/src/intl_bindings.rs:42` **fn** `install_intl_bindings` — Install the `Intl` shim into the JS context
`crates/js/src/lib.rs:111` **fn** `deterministic_seed_from_url` — Compute a deterministic u64 seed from a URL for deterministic render mode (8F)
`crates/js/src/lib.rs:125` **struct** `QuickJsRuntime` — QuickJS-based JS runtime via `rquickjs`
`crates/js/src/lib.rs:260` **fn** `new`
`crates/js/src/lib.rs:304` **fn** `register_module_source` — Register an ES module by specifier so it can be `import`-ed by other modules
`crates/js/src/lib.rs:315` **fn** `eval_module` — Evaluate `source` as an ES module (HTML LS §8.1.3 `<script type=module>`)
`crates/js/src/lib.rs:370` **fn** `install_dom` — Install DOM Web API globals (`document`, `window`, `console`, etc.) into
`crates/js/src/lib.rs:948` **fn** `set_cookie_banner_dismiss` — Enable or disable cookie-banner auto-dismiss for subsequent `install_dom` calls
`crates/js/src/lib.rs:957` **fn** `set_deterministic_mode` — Enable deterministic render mode (8F)
`crates/js/src/lib.rs:974` **fn** `freeze_fingerprint` — Freeze fingerprint APIs for canvas / audio / font enumeration (8F.3)
`crates/js/src/lib.rs:1017` **fn** `pump_workers` — Deliver messages posted by worker threads to their `Worker` JS instances
`crates/js/src/lib.rs:1042` **fn** `flush_canvas_updates` — Drain dirty Canvas 2D buffers for upload to the renderer
`crates/js/src/lib.rs:1056` **fn** `pump_broadcast_channels` — Deliver messages posted to this page's `BroadcastChannel` instances
`crates/js/src/lib.rs:1081` **fn** `pump_shared_workers` — Deliver messages posted by `SharedWorker` threads to this page's ports
`crates/js/src/lib.rs:1100` **fn** `take_navigate_request` — Consume any navigation request that JS placed via `location.href =` etc
`crates/js/src/lib.rs:1110` **fn** `take_history_url_updates` — Drain `history.pushState` / `history.replaceState` URL-update notifications
`crates/js/src/lib.rs:1121` **fn** `take_fullscreen_requests` — Drain all fullscreen requests queued by `element.requestFullscreen()` and
`crates/js/src/lib.rs:1129` **fn** `take_view_transition_events` — Drain all View Transition events queued by `document.startViewTransition`
`crates/js/src/lib.rs:1137` **fn** `take_dom_dirty` — Returns `true` if JS mutated the DOM since the last call, clearing the flag
`crates/js/src/lib.rs:1146` **fn** `take_raf_pending` — Returns `true` if `requestAnimationFrame` was called since the last call,
`crates/js/src/lib.rs:1155` **fn** `take_timer_wakeup` — Take the next timer wakeup as Unix epoch ms, clearing the stored value
`crates/js/src/lib.rs:1164` **fn** `update_layout_rects` — Replace the layout bounding-rect table with a fresh snapshot
`crates/js/src/lib.rs:1172` **fn** `update_viewport_size` — Update the viewport dimensions
`crates/js/src/lib.rs:1181` **fn** `take_lazy_image_requests` — Drain lazy image load requests queued by `_lumen_request_lazy_image_load` in JS
`crates/js/src/lib.rs:1193` **fn** `update_scroll_states` — Replace the scroll-state table with a fresh snapshot from the layout tree
`crates/js/src/lib.rs:1202` **fn** `take_scroll_requests` — Drain JS-initiated scroll requests queued by `_lumen_request_scroll`
`crates/js/src/lib.rs:1211` **fn** `take_notification_requests` — Drain all OS notification requests queued by `new Notification(...)` in JS
`crates/js/src/lib.rs:1222` **fn** `take_window_open_requests` — Drain all popup window requests queued by JS `window.open(...)`
`crates/js/src/lib.rs:1231` **fn** `take_console_messages` — Drain all `console.log/warn/error` messages queued since the last call
`crates/js/src/lib.rs:1240` **fn** `update_computed_styles` — Push a fresh snapshot of computed CSS styles into the JS runtime
`crates/js/src/lib.rs:1250` **fn** `set_document_visibility` — Update `document.hidden` / `document.visibilityState` and fire
`crates/js/src/lib.rs:1268` **fn** `notify_dom_content_loaded` — Transition `document.readyState` → `'interactive'` and fire
`crates/js/src/lib.rs:1280` **fn** `notify_window_loaded` — Transition `document.readyState` → `'complete'` and fire
`crates/js/src/media_capabilities.rs:8` **fn** `install_media_capabilities_bindings` — Install Media Capabilities API bindings into the JS context
`crates/js/src/media_devices.rs:26` **fn** `install_media_devices_bindings` — Install MediaDevices API shim into the JS context
`crates/js/src/media_session.rs:36` **fn** `install_media_session_bindings` — Install MediaSession API shim into the JS context
`crates/js/src/media_stream_recording.rs:12` **fn** `init_media_stream_recording` — Install the MediaRecorder API stub into the JS context
`crates/js/src/navigation_api.rs:11` **fn** `install_navigation_api` — Install Navigation API into the JS context
`crates/js/src/navigator_bindings.rs:36` **struct** `NavigatorProfile` — High-entropy `navigator` / `screen` / timezone values exposed to JavaScript
`crates/js/src/navigator_bindings.rs:86` **fn** `set_navigator_profile` — Install a process-wide navigator profile (9F.1). Subsequent calls to the
`crates/js/src/navigator_bindings.rs:93` **fn** `current_navigator_profile` — Return the currently configured profile, or the default if none was set
`crates/js/src/navigator_bindings.rs:111` **fn** `install_navigator_bindings` — Install navigator/screen/timezone normalization shim into the JS context,
`crates/js/src/navigator_bindings.rs:117` **fn** `install_navigator_bindings_with` — Install the navigator shim using an explicit [`NavigatorProfile`], ignoring
`crates/js/src/notifications_bindings.rs:21` **struct** `NotificationRequest` — A notification request queued by `new Notification(...)` in JS
`crates/js/src/notifications_bindings.rs:34` **type** `NotificationQueue` — Shared queue of pending notification requests
`crates/js/src/notifications_bindings.rs:52` **fn** `install_notifications_bindings` — Install Web Notifications API globals into the JS context
`crates/js/src/notifications_bindings.rs:108` **fn** `drain_notifications` — Drain all pending notification requests from the queue
`crates/js/src/offscreen_canvas.rs:33` **struct** `OffscreenCanvas` — Wrapper class for OffscreenCanvas JS object
`crates/js/src/offscreen_canvas.rs:44` **fn** `new` — Create a new OffscreenCanvas with the given dimensions
`crates/js/src/offscreen_canvas.rs:57` **fn** `id` — Get the canvas ID (internal use only)
`crates/js/src/offscreen_canvas.rs:62` **fn** `width` — Get canvas width in CSS pixels
`crates/js/src/offscreen_canvas.rs:67` **fn** `height` — Get canvas height in CSS pixels
`crates/js/src/offscreen_canvas.rs:72` **fn** `transfer_to_image_bitmap` — Transfer pixel buffer to ImageBitmap and clear the canvas
`crates/js/src/offscreen_canvas.rs:113` **fn** `flush_dirty` — Drain dirty offscreen canvases and return their RGBA buffers
`crates/js/src/offscreen_canvas.rs:137` **fn** `install_offscreen_canvas_bindings` — Install OffscreenCanvas bindings and JS shim into the QuickJS runtime
`crates/js/src/paint_worklet.rs:13` **struct** `PaintWorkletRegistry` — Maps worklet name (e.g. "my-paint") to its definition
`crates/js/src/paint_worklet.rs:18` **fn** `new`
`crates/js/src/paint_worklet.rs:23` **fn** `register` — Register a paint worklet definition
`crates/js/src/paint_worklet.rs:28` **fn** `get` — Look up a registered worklet by name
`crates/js/src/paint_worklet.rs:33` **fn** `all` — Get all registered worklets
`crates/js/src/paint_worklet.rs:38` **fn** `clear` — Clear all registrations (for tests)
`crates/js/src/paint_worklet.rs:44` **fn** `get_paint_worklet_registry` — Get the global paint worklet registry, initializing it if necessary
`crates/js/src/paint_worklet.rs:50` **struct** `PaintWorkletDef` — Definition of a registered paint worklet
`crates/js/src/paint_worklet.rs:61` **fn** `install_paint_worklet_api` — Install CSS.paintWorklet bindings into the JS context
`crates/js/src/payment_request.rs:18` **fn** `init_payment_request` — Install the Payment Request API stub into the JS context
`crates/js/src/periodic_sync.rs:19` **fn** `init_periodic_sync` — Install the Periodic Background Sync API stub into the JS context
`crates/js/src/permissions_policy.rs:13` **fn** `install_permissions_policy_bindings` — Install Permissions Policy JS bindings: `document.featurePolicy` and the
`crates/js/src/pointer_lock.rs:30` **fn** `request_pointer_lock` — Request pointer lock for element with given node ID
`crates/js/src/pointer_lock.rs:41` **fn** `exit_pointer_lock` — Exit pointer lock
`crates/js/src/pointer_lock.rs:51` **fn** `set_movement` — Set relative mouse movement delta (called from shell event loop for each mousemove)
`crates/js/src/pointer_lock.rs:62` **fn** `get_lock_state` — Get current pointer lock state: (is_locked, locked_element_nid, movement_x, movement_y)
`crates/js/src/pointer_lock.rs:75` **fn** `is_pointer_locked` — Check if pointer is locked
`crates/js/src/pointer_lock.rs:80` **fn** `get_locked_element_nid` — Get the DOM node ID of the locked element, or None
`crates/js/src/pointer_lock.rs:86` **fn** `take_movement` — Get the current movement delta and reset it to zero
`crates/js/src/presentation_api.rs:19` **fn** `install_presentation_api` — Install the Presentation API bindings into the JS context
`crates/js/src/push_api.rs:18` **fn** `init_push_api` — Install the Push API stub into the JS context
`crates/js/src/reporting_api.rs:13` **fn** `install_reporting_api_bindings` — Install Reporting API bindings into the JS context
`crates/js/src/sanitizer.rs:9` **fn** `install_sanitizer_bindings`
`crates/js/src/scheduler.rs:20` **fn** `install_scheduler_api` — Install the Scheduler API, TaskController, and TaskSignal into the JS context
`crates/js/src/screen_orientation.rs:19` **fn** `install_screen_orientation_bindings` — Install Screen Orientation API shim into the JS context
`crates/js/src/scroll_snap_events.rs:23` **fn** `install_scroll_snap_events_bindings` — Install CSS Scroll Snap L2 events into the JS context
`crates/js/src/serial.rs:7` **fn** `install_serial_bindings` — Install WebSerial API bindings into the JS context
`crates/js/src/shape_detection.rs:8` **fn** `install_shape_detection_bindings`
`crates/js/src/shared_worker.rs:42` **type** `SharedWorkerOutbox` — Outbound queue owned by a single `QuickJsRuntime` (page / context)
`crates/js/src/shared_worker.rs:86` **fn** `connect_shared_worker` — Connect a new client to the shared worker identified by `key`
`crates/js/src/shared_worker.rs:118` **fn** `post_to_shared_worker` — Forward a client `port.postMessage(data)` to the shared-worker thread
`crates/js/src/shared_worker.rs:128` **fn** `close_shared_worker_port` — Notify the shared worker that a client closed its port
`crates/js/src/shared_worker.rs:137` **fn** `drain_messages` — Drain all messages a runtime's shared-worker ports have received
`crates/js/src/shared_worker.rs:147` **fn** `install_shared_worker_bindings` — Install the `_lumen_sw_connect` / `_lumen_sw_post` / `_lumen_sw_close` native
`crates/js/src/speech.rs:84` **fn** `install_speech_bindings` — Install the Web Speech API into `ctx`
`crates/js/src/sri.rs:10` **enum** `SriAlgorithm` — Hash algorithm accepted in the `integrity` attribute
`crates/js/src/sri.rs:17` **struct** `SriToken` — One parsed token from an `integrity` string
`crates/js/src/sri.rs:27` **fn** `parse_integrity_metadata` — Parses a space-separated list of integrity tokens
`crates/js/src/sri.rs:56` **fn** `check_sri` — Returns `true` if `body` passes the SRI check encoded in `integrity`
`crates/js/src/storage_manager.rs:19` **fn** `install_storage_manager_bindings` — Install StorageManager API bindings into the JS context
`crates/js/src/surface_api.rs:29` **fn** `install_surface_api_protection` — Install Layer 1 surface API protection into the JS context
`crates/js/src/temporal_api.rs:36` **fn** `install_temporal_api` — Install the Temporal API shim into the given QuickJS context
`crates/js/src/trusted_types.rs:9` **fn** `install_trusted_types_bindings`
`crates/js/src/typed_om_api.rs:20` **fn** `install_typed_om_api` — Install CSS Typed OM API bindings
`crates/js/src/ua_client_hints.rs:11` **fn** `install_ua_client_hints_bindings` — Install User-Agent Client Hints bindings into the JS context
`crates/js/src/url_pattern.rs:14` **fn** `install_url_pattern_api` — Install URL Pattern API into the JS context
`crates/js/src/video_bindings.rs:27` **fn** `install_video_bindings` — Install HTMLVideoElement stubs into the JS context
`crates/js/src/video_pip.rs:23` **fn** `install_video_pip_api` — Install Video Picture-in-Picture API into the JS context
`crates/js/src/view_transitions.rs:17` **enum** `ViewTransitionEvent` — Events emitted by `document.startViewTransition` and drained by the shell
`crates/js/src/view_transitions.rs:68` **fn** `install_view_transition_bindings` — Register `_lumen_vt_begin` / `_lumen_vt_end` native functions and install
`crates/js/src/virtual_keyboard.rs:15` **fn** `install_virtual_keyboard_bindings` — Install Virtual Keyboard API bindings into the JS context
`crates/js/src/wake_lock.rs:15` **fn** `install_wake_lock_bindings` — Install the Screen Wake Lock API bindings into the JS context
`crates/js/src/web_audio.rs:18` **fn** `install_web_audio_api` — Install the Web Audio API into the JS context
`crates/js/src/web_codecs.rs:10` **fn** `install_web_codecs` — Install Web Codecs API stubs into the JS context
`crates/js/src/web_locks.rs:14` **fn** `install_web_locks_bindings` — Install the Web Locks API bindings into the JS context
`crates/js/src/web_midi.rs:16` **fn** `install_web_midi_api` — Install Web MIDI API bindings into the JS context
`crates/js/src/webassembly.rs:9` **fn** `install_webassembly_bindings` — Install WebAssembly API bindings into the JS context
`crates/js/src/webgl_bindings.rs:25` **fn** `install_webgl_bindings` — Install WebGL fingerprint shim into the JS context
`crates/js/src/webgl_canvas.rs:57` **fn** `install_webgl_canvas` — Install functional WebGL bindings into the JS context
`crates/js/src/webgpu.rs:28` **fn** `install_webgpu_bindings` — Install the WebGPU API bindings into the JS context
`crates/js/src/webhid.rs:5` **fn** `install_webhid_bindings`
`crates/js/src/webrtc_stub.rs:27` **fn** `install_webrtc_bindings` — Install the WebRTC mDNS-only stub into the JS context
`crates/js/src/webtransport.rs:5` **fn** `install_webtransport_bindings`
`crates/js/src/webusb.rs:5` **fn** `install_webusb_bindings`
`crates/js/src/webxr.rs:7` **fn** `install_webxr_bindings` — Install WebXR Device API bindings into the JS context
`crates/js/src/worker.rs:23` **enum** `WorkerInMsg` — Message sent from the main JS thread to a worker thread
`crates/js/src/worker.rs:33` **struct** `WorkerHandle` — Live handle to a spawned worker thread
`crates/js/src/worker.rs:45` **type** `WorkerRegistry` — All live Worker instances for the current page, keyed by worker ID
`crates/js/src/worker.rs:51` **type** `WorkerMessageQueue` — Outbound message queue: messages posted by worker threads to the main thread
`crates/js/src/worker.rs:59` **fn** `spawn_worker` — Spawn a new worker thread that evaluates `script` and waits for messages
`crates/js/src/worker.rs:90` **fn** `post_to_worker` — Send a JSON-serialized message to a live worker thread
`crates/js/src/worker.rs:100` **fn** `terminate_worker` — Terminate a worker and remove it from the registry
`crates/js/src/worker.rs:109` **fn** `drain_messages` — Drain all pending messages sent from worker threads to the main thread
`crates/js/src/worker.rs:118` **fn** `install_worker_bindings` — Install native bindings (`_lumen_create_worker`, `_lumen_worker_post`,
`crates/js/src/xhr.rs:38` **fn** `install_xhr_bindings` — Install the XMLHttpRequest API into the QuickJS context

## lumen-knowledge  (54 symbols)

`crates/knowledge/src/fts.rs:28` **struct** `SearchHit` — Результат полнотекстового поиска
`crates/knowledge/src/fts.rs:43` **struct** `HistoryFts` — FTS5-индекс над `(url, title, text)`. Открывается отдельной БД-файлом
`crates/knowledge/src/fts.rs:54` **fn** `open`
`crates/knowledge/src/fts.rs:60` **fn** `open_in_memory`
`crates/knowledge/src/fts.rs:87` **fn** `index` — Добавить или обновить запись в индексе. `rowid` обычно совпадает
`crates/knowledge/src/fts.rs:111` **fn** `unindex` — Удалить запись по rowid
`crates/knowledge/src/fts.rs:129` **fn** `search` — Полнотекстовый поиск по `text` с ранжированием bm25. `query` —
`crates/knowledge/src/fts.rs:167` **fn** `clear` — Полная очистка индекса
`crates/knowledge/src/history.rs:28` **struct** `HistoryWithFts` — История с интегрированным FTS-индексом. Оборачивает
`crates/knowledge/src/history.rs:36` **fn** `open` — Открыть или создать FTS-индекс истории. Обычно открывается
`crates/knowledge/src/history.rs:42` **fn** `open_in_memory` — Открыть in-memory FTS-индекс (для тестов)
`crates/knowledge/src/history.rs:52` **fn** `index_text` — Индексировать запись истории в FTS. Обычно вызывается после
`crates/knowledge/src/history.rs:58` **fn** `unindex` — Удалить запись из FTS-индекса. Обычно вызывается после
`crates/knowledge/src/history.rs:69` **fn** `search` — Полнотекстовый поиск по истории. Возвращает совпадения,
`crates/knowledge/src/history.rs:75` **fn** `clear` — Очистить весь FTS-индекс. Обычно вызывается при
`crates/knowledge/src/history.rs:85` **fn** `record_visit_with_text` — Записать визит в History и автоматически индексировать текст в FTS
`crates/knowledge/src/history.rs:106` **fn** `delete_with_fts` — Удалить запись из History и автоматически удалить из FTS
`crates/knowledge/src/notes.rs:21` **struct** `Note` — Одна заметка пользователя
`crates/knowledge/src/notes.rs:34` **struct** `NoteSearchHit`
`crates/knowledge/src/notes.rs:41` **struct** `Notes`
`crates/knowledge/src/notes.rs:52` **fn** `open`
`crates/knowledge/src/notes.rs:58` **fn** `open_in_memory`
`crates/knowledge/src/notes.rs:110` **fn** `add` — Создать заметку. Возвращает её id
`crates/knowledge/src/notes.rs:132` **fn** `update` — Обновить selection / context / comment по id. created_at не меняется
`crates/knowledge/src/notes.rs:152` **fn** `delete` — Удалить заметку по id
`crates/knowledge/src/notes.rs:163` **fn** `get` — Получить заметку по id
`crates/knowledge/src/notes.rs:182` **fn** `list_for_url` — Все заметки для конкретного URL (для восстановления highlight-
`crates/knowledge/src/notes.rs:204` **fn** `recent` — Последние N заметок (по убыванию created_at)
`crates/knowledge/src/notes.rs:226` **fn** `search` — Полнотекстовый поиск по selection + comment
`crates/knowledge/src/notes.rs:268` **fn** `count` — Общее число заметок
`crates/knowledge/src/notes.rs:280` **fn** `clear` — Удалить все заметки. Триггеры notes_ad чистят FTS индекс
`crates/knowledge/src/open_tabs.rs:36` **struct** `OpenTabHit` — Результат поиска по открытым вкладкам
`crates/knowledge/src/open_tabs.rs:54` **struct** `OpenTabsIndex` — Живой in-memory FTS5-индекс над открытыми вкладками. Не персистится —
`crates/knowledge/src/open_tabs.rs:67` **fn** `new` — Создать пустой in-memory индекс. По дизайну (§12.4) on-disk варианта
`crates/knowledge/src/open_tabs.rs:88` **fn** `index_tab` — Добавить или обновить вкладку в индексе. `tab_id` — живой shell tab id;
`crates/knowledge/src/open_tabs.rs:112` **fn** `remove_tab` — Убрать вкладку из индекса (при её закрытии). No-op, если вкладки нет
`crates/knowledge/src/open_tabs.rs:129` **fn** `search` — Полнотекстовый поиск по `(url, title, text)` среди открытых вкладок,
`crates/knowledge/src/open_tabs.rs:164` **fn** `count` — Текущее число проиндексированных открытых вкладок
`crates/knowledge/src/open_tabs.rs:176` **fn** `clear` — Очистить весь индекс (например, при выходе или сбросе сессии)
`crates/knowledge/src/read_later.rs:23` **enum** `ReadStatus` — Статус read-later записи
`crates/knowledge/src/read_later.rs:53` **struct** `ReadLaterEntry` — Одна сохранённая страница
`crates/knowledge/src/read_later.rs:69` **struct** `ReadLaterSearchHit`
`crates/knowledge/src/read_later.rs:75` **struct** `ReadLater`
`crates/knowledge/src/read_later.rs:86` **fn** `open`
`crates/knowledge/src/read_later.rs:92` **fn** `open_in_memory`
`crates/knowledge/src/read_later.rs:153` **fn** `save` — Сохранить новую страницу или обновить существующую. Возвращает id
`crates/knowledge/src/read_later.rs:206` **fn** `set_status` — Обновить статус записи (mark read / archive)
`crates/knowledge/src/read_later.rs:220` **fn** `touch` — Обновить last_accessed (вызывается при открытии офлайн-копии)
`crates/knowledge/src/read_later.rs:233` **fn** `get`
`crates/knowledge/src/read_later.rs:252` **fn** `get_by_url`
`crates/knowledge/src/read_later.rs:272` **fn** `list_by_status` — Список записей с указанным статусом, сортировка по saved_at DESC
`crates/knowledge/src/read_later.rs:296` **fn** `search` — Полнотекстовый поиск
`crates/knowledge/src/read_later.rs:346` **fn** `delete`
`crates/knowledge/src/read_later.rs:356` **fn** `count`

## lumen-layout  (477 symbols)

`crates/engine/layout/src/anchor.rs:40` **enum** `AnchorSide` — Which edge or point of an anchor element the `anchor()` function references
`crates/engine/layout/src/anchor.rs:69` **enum** `InsetAreaKeyword` — Single-axis `inset-area` keyword, as defined in §5.2 of the spec
`crates/engine/layout/src/anchor.rs:102` **struct** `AnchorRegistry` — Map from CSS `anchor-name` value (e.g. `"--foo"`) to the border-box [`Rect`]
`crates/engine/layout/src/anchor.rs:109` **struct** `AnchorEntry` — One registered anchor element
`crates/engine/layout/src/anchor.rs:123` **fn** `get` — Look up an anchor by CSS name (e.g. `"--tooltip-anchor"`)
`crates/engine/layout/src/anchor.rs:128` **fn** `is_empty` — True when the registry has no anchors
`crates/engine/layout/src/anchor.rs:154` **fn** `collect_anchors`
`crates/engine/layout/src/anchor.rs:177` **fn** `register_anchor` — Register an element as a named anchor.  Called by P4's CSS wiring when it
`crates/engine/layout/src/anchor.rs:204` **fn** `resolve_anchor_function`
`crates/engine/layout/src/anchor.rs:256` **struct** `AnchoredPosition` — Resolved inset-area position for an anchored element
`crates/engine/layout/src/anchor.rs:285` **fn** `resolve_inset_area`
`crates/engine/layout/src/animation.rs:36` **struct** `AnimatedStyle` — Sparse animated values for one element — scheduler output per node per frame
`crates/engine/layout/src/animation.rs:46` **struct** `AnimationFrame` — Output of `AnimationScheduler::tick` — per-node animated values for one frame
`crates/engine/layout/src/animation.rs:58` **fn** `merge` — Merge `other` into `self`; `other` values take precedence per property
`crates/engine/layout/src/animation.rs:76` **fn** `merge_from` — Extract only compositor-offloadable properties (opacity, transform)
`crates/engine/layout/src/animation.rs:92` **fn** `to_compositor_frame` — Extract only compositor-offloadable properties (opacity, transform)
`crates/engine/layout/src/animation.rs:115` **struct** `CompositorOverride` — Compositor-offloadable overrides for one element
`crates/engine/layout/src/animation.rs:125` **struct** `CompositorAnimFrame` — Per-frame compositor overrides — output of `AnimationFrame::to_compositor_frame`
`crates/engine/layout/src/animation.rs:131` **fn** `is_empty`
`crates/engine/layout/src/animation.rs:135` **fn** `get`
`crates/engine/layout/src/animation.rs:143` **struct** `KeyframeStyle` — Sparse style extracted from one `@keyframes` frame's declarations
`crates/engine/layout/src/animation.rs:152` **fn** `parse_keyframe_style` — Parse the `declarations` of one `@keyframes` frame into a [`KeyframeStyle`]
`crates/engine/layout/src/animation.rs:181` **enum** `AnimValue` — Анимируемое значение. Phase 0: восемь вариантов — Number / Length / Color /
`crates/engine/layout/src/animation.rs:217` **trait** `AnimationInterpolator` — Trait для интерполяции пары computed values
`crates/engine/layout/src/animation.rs:231` **struct** `NoopInterpolator` — Stub-реализация: step-half для любой пары значений
`crates/engine/layout/src/animation.rs:262` **struct** `LinearInterpolator` — Реальная импл §5.2 — linear для Number / Length (same-unit) / Color
`crates/engine/layout/src/animation.rs:746` **struct** `AnimationScheduler` — CSS Animations L1 §3 — scheduler that maps `@keyframes` to interpolated
`crates/engine/layout/src/animation.rs:752` **fn** `new`
`crates/engine/layout/src/animation.rs:762` **fn** `sync` — Register or refresh animations for `node` based on its computed style
`crates/engine/layout/src/animation.rs:783` **fn** `remove_node` — Remove all animation state for `node` (e.g. when the node is removed from the DOM)
`crates/engine/layout/src/animation.rs:793` **fn** `tick` — Compute per-node animated style overrides for the current frame
`crates/engine/layout/src/animation.rs:1094` **struct** `TransitionScheduler` — CSS Transitions L1 §2 — detects property value changes and interpolates
`crates/engine/layout/src/animation.rs:1100` **fn** `new`
`crates/engine/layout/src/animation.rs:1106` **fn** `sync` — Detect value changes between `old` and `new` style for properties listed
`crates/engine/layout/src/animation.rs:1189` **fn** `remove_node` — Remove all transition state for `node` (called when node leaves DOM)
`crates/engine/layout/src/animation.rs:1222` **fn** `tick` — Compute interpolated style overrides for the current frame
`crates/engine/layout/src/box_tree.rs:90` **struct** `ViewBox` — SVG `viewBox="min-x min-y width height"` attribute. Maps SVG user-unit space
`crates/engine/layout/src/box_tree.rs:105` **struct** `PreserveAspectRatio` — SVG `preserveAspectRatio` attribute for aspect-ratio preservation
`crates/engine/layout/src/box_tree.rs:116` **enum** `SvgAlignX` — SVG preserveAspectRatio horizontal alignment
`crates/engine/layout/src/box_tree.rs:127` **enum** `SvgAlignY` — SVG preserveAspectRatio vertical alignment
`crates/engine/layout/src/box_tree.rs:138` **enum** `SvgMeetOrSlice` — SVG preserveAspectRatio meet-or-slice mode
`crates/engine/layout/src/box_tree.rs:148` **enum** `SvgTextAnchor` — SVG `text-anchor` attribute for text horizontal alignment
`crates/engine/layout/src/box_tree.rs:161` **enum** `SvgDominantBaseline` — SVG `dominant-baseline` attribute for text vertical alignment
`crates/engine/layout/src/box_tree.rs:182` **struct** `SvgTransform` — SVG transformation data from the `transform` presentation attribute
`crates/engine/layout/src/box_tree.rs:190` **fn** `identity` — Creates an identity transform (no transformation)
`crates/engine/layout/src/box_tree.rs:195` **fn** `translate` — Creates a translation transform
`crates/engine/layout/src/box_tree.rs:200` **fn** `compose` — Multiplies this transform by another, composing them
`crates/engine/layout/src/box_tree.rs:215` **fn** `transform_point` — Applies this transform to a point (x, y)
`crates/engine/layout/src/box_tree.rs:224` **enum** `SvgShapeKind` — Geometric primitive for an SVG shape element in SVG user units (before viewBox scaling)
`crates/engine/layout/src/box_tree.rs:241` **enum** `FormControlKind` — Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
`crates/engine/layout/src/box_tree.rs:1020` **struct** `ImageRequest` — Запрос на предзагрузку изображения: URL после picking-а по
`crates/engine/layout/src/box_tree.rs:1035` **fn** `collect_image_requests` — Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов
`crates/engine/layout/src/box_tree.rs:1055` **fn** `collect_background_image_requests` — Обходит готовое layout-дерево и возвращает уникальные URL-ы из
`crates/engine/layout/src/box_tree.rs:1155` **struct** `LayoutBox`
`crates/engine/layout/src/box_tree.rs:1182` **struct** `InlineSegment` — Отрезок inline-контента с собственным стилем (до layout)
`crates/engine/layout/src/box_tree.rs:1220` **enum** `PseudoKind` — Marks an inline segment as the target of a CSS structural pseudo-element
`crates/engine/layout/src/box_tree.rs:1238` **struct** `InlineFrag` — Позиционированный текстовый фрагмент в строке (после layout)
`crates/engine/layout/src/box_tree.rs:1270` **enum** `BoxKind`
`crates/engine/layout/src/box_tree.rs:1560` **fn** `layout`
`crates/engine/layout/src/box_tree.rs:1577` **fn** `layout_measured` — Layout without a text measurer. For tests and headless modes; uses `layout_measured_hyp` with `dark_mode=false`
`crates/engine/layout/src/box_tree.rs:1590` **fn** `layout_measured_hyp` — Layout with a real hyphenation provider (for `hyphens: auto`)
`crates/engine/layout/src/box_tree.rs:1617` **fn** `build_iframe_document` — Parse inline HTML from an `<iframe srcdoc="...">` attribute (HTML spec §4.8.5)
`crates/engine/layout/src/box_tree.rs:5707` **fn** `resolve_auto_fill_fit_count` — CSS Grid Layout L3 §9 — Resolve `repeat(auto-fill|auto-fit, <track-list>)` count
`crates/engine/layout/src/box_tree.rs:5892` **fn** `measure_text_w`
`crates/engine/layout/src/box_tree.rs:5911` **fn** `measure_text_w_families` — Как [`measure_text_w`], но учитывает CSS `font-family` каскад
`crates/engine/layout/src/box_tree.rs:6811` **fn** `apply_container_styles` — CSS Container Queries L1: second-pass after layout
`crates/engine/layout/src/color_mix.rs:38` **enum** `MixColorSpace` — CSS Color L5 §10.2 — interpolation color space for `color-mix()`
`crates/engine/layout/src/color_mix.rs:63` **fn** `from_css` — Parse a CSS `color-mix()` interpolation space identifier (case-insensitive)
`crates/engine/layout/src/color_mix.rs:80` **fn** `is_polar` — Returns `true` if this space has a hue (polar) axis
`crates/engine/layout/src/color_mix.rs:96` **fn** `mix_colors` — CSS Color L5 §10.2 — mix two sRGB colors in the given interpolation space
`crates/engine/layout/src/counters.rs:39` **type** `CounterSnapshot` — Per-element counter stacks snapshot
`crates/engine/layout/src/counters.rs:43` **type** `CounterMap` — Maps each element `NodeId` to its counter snapshot (after own reset/increment,
`crates/engine/layout/src/counters.rs:99` **fn** `precompute_counters` — Build a `CounterMap` by walking the DOM in pre-order
`crates/engine/layout/src/counters.rs:163` **fn** `format_counter` — Format a counter integer value according to the given `list-style-type` keyword
`crates/engine/layout/src/counters.rs:230` **enum** `CounterSystem` — Numbering algorithm for a `@counter-style` rule — CSS Counter Styles L3 §4
`crates/engine/layout/src/counters.rs:249` **struct** `RangeBound` — Counter range bound: `None` means ±infinite (CSS Counter Styles L3 §5)
`crates/engine/layout/src/counters.rs:258` **enum** `CounterRange` — Range descriptor value (CSS Counter Styles L3 §5)
`crates/engine/layout/src/counters.rs:267` **struct** `CounterStyleDef` — Parsed `@counter-style` rule — CSS Counter Styles L3 §2
`crates/engine/layout/src/counters.rs:305` **type** `CounterStyleRegistry` — Maps counter style names to their parsed `CounterStyleDef`
`crates/engine/layout/src/counters.rs:308` **fn** `build_counter_style_registry` — Build a `CounterStyleRegistry` from all `@counter-style` rules in a stylesheet
`crates/engine/layout/src/counters.rs:587` **fn** `format_counter_with_registry` — Format a counter value using the registry (custom `@counter-style`) first,
`crates/engine/layout/src/counters.rs:757` **fn** `resolve_counter_value` — CSS Counter Styles L3 §2 — format counter `n` using a resolved `CounterStyleDef`
`crates/engine/layout/src/counters.rs:770` **fn** `build_list_marker_text` — CSS Lists L3 §2.1 — canonical wiring point for `list-style-type` + `@counter-style`
`crates/engine/layout/src/font_palette.rs:26` **struct** `PaletteColorOverride` — Resolved CPAL color override: `(palette_index, color)`
`crates/engine/layout/src/font_palette.rs:44` **fn** `resolve_font_palette_overrides` — Resolves `@font-palette-values` overrides for a given element
`crates/engine/layout/src/font_palette.rs:76` **struct** `ResolvedFontPalette` — Output of [`resolve_font_palette_overrides`]
`crates/engine/layout/src/image_gating.rs:42` **fn** `gate_image_requests` — Returns the set of [`NodeId`]s for `BoxKind::Image` boxes whose bounding
`crates/engine/layout/src/lib.rs:124` **trait** `TextMeasurer`
`crates/engine/layout/src/lib.rs:164` **enum** `ClickableKind` — Classification of an interactive element found during layout-tree traversal
`crates/engine/layout/src/lib.rs:185` **struct** `ClickableElement` — An interactive element with its screen-space bounding rect
`crates/engine/layout/src/lib.rs:206` **fn** `collect_clickable_elements` — Collect all interactive elements from the layout tree in document order
`crates/engine/layout/src/lib.rs:412` **struct** `StickyBox` — Snapshot of a `position: sticky` element captured after normal-flow layout
`crates/engine/layout/src/lib.rs:440` **fn** `collect_sticky_boxes` — Collect all `position: sticky` elements from the layout tree in document order
`crates/engine/layout/src/lib.rs:499` **fn** `compute_sticky_offset` — Compute the visual offset `(dx, dy)` in CSS px to apply to a sticky element
`crates/engine/layout/src/lib.rs:572` **struct** `SnapPoint` — A single snap area inside a [`SnapContainer`]
`crates/engine/layout/src/lib.rs:590` **struct** `SnapContainer` — A scroll container that participates in CSS Scroll Snap L1
`crates/engine/layout/src/lib.rs:615` **fn** `collect_snap_containers` — Collect all scroll containers that participate in CSS Scroll Snap L1
`crates/engine/layout/src/lib.rs:747` **fn** `find_snap_target` — Find the nearest snap target for a scroll gesture
`crates/engine/layout/src/lib.rs:835` **struct** `ScrollContainer` — A scrollable overflow container collected from the layout tree
`crates/engine/layout/src/lib.rs:860` **fn** `collect_scroll_containers` — Collect all `overflow: scroll` / `overflow: auto` containers from the layout tree
`crates/engine/layout/src/lib.rs:929` **fn** `collect_computed_styles` — Walks the layout tree and returns a map of `NodeId index → CSS property map`
`crates/engine/layout/src/lib.rs:955` **fn** `set_scroll_position` — Update the scroll position of a node in the layout tree
`crates/engine/layout/src/masonry.rs:19` **fn** `lay_out_masonry` — Waterfall-grid masonry layout algorithm
`crates/engine/layout/src/mathml.rs:23` **enum** `MathmlElementKind` — Represents the type of MathML element and its visual role
`crates/engine/layout/src/mathml.rs:49` **struct** `MathmlBox` — MathML box: container for mathematical notation
`crates/engine/layout/src/mathml.rs:64` **fn** `new` — Create a new MathML box for a given element type
`crates/engine/layout/src/mathml.rs:75` **fn** `with_denominator` — Set denominator boxes for mfrac elements
`crates/engine/layout/src/mathml.rs:81` **fn** `with_annotation` — Set annotation (exponent/subscript) boxes
`crates/engine/layout/src/mathml.rs:87` **fn** `with_annotation_scale` — Set the scaling factor for annotations (superscript/subscript)
`crates/engine/layout/src/mathml.rs:103` **fn** `collect_mathml_structure` — Collect MathML element structure from a DOM node
`crates/engine/layout/src/mathml.rs:133` **fn** `lay_out_mathml` — Layout algorithm for MathML content
`crates/engine/layout/src/motion_path.rs:22` **struct** `MotionTransform` — Result of resolving a motion offset along an `offset-path`
`crates/engine/layout/src/motion_path.rs:39` **fn** `resolve_motion_transform` — Resolve the motion transform for an element with `offset-path: path(...)`
`crates/engine/layout/src/page.rs:22` **struct** `MarginBoxTextFragment` — Text fragment within a margin-box after layout
`crates/engine/layout/src/page.rs:49` **enum** `MarginBoxPosition` — Position of a margin-box relative to the page box
`crates/engine/layout/src/page.rs:72` **fn** `all` — All 16 margin-box positions in layout order
`crates/engine/layout/src/page.rs:88` **fn** `css_name` — CSS property name for this margin-box in @page rules
`crates/engine/layout/src/page.rs:103` **fn** `is_corner` — Is this a corner box?
`crates/engine/layout/src/page.rs:114` **fn** `is_horizontal_edge` — Is this a horizontal edge box (top or bottom)?
`crates/engine/layout/src/page.rs:119` **fn** `is_vertical_edge` — Is this a vertical edge box (left or right)?
`crates/engine/layout/src/page.rs:129` **struct** `PageProperties` — Computed properties for a page from matching @page rules
`crates/engine/layout/src/page.rs:155` **fn** `default_a4` — Create default page properties (A4 size, 2cm margins)
`crates/engine/layout/src/page.rs:172` **fn** `content_width` — Content box width: page width minus left and right margins
`crates/engine/layout/src/page.rs:177` **fn** `content_height` — Content box height: page height minus top and bottom margins
`crates/engine/layout/src/page.rs:182` **fn** `compute_orientation` — Update orientation based on width/height ratio
`crates/engine/layout/src/page.rs:196` **struct** `MarginBox` — Margin-box with layout information
`crates/engine/layout/src/page.rs:223` **fn** `new` — Create a new margin-box at a given position
`crates/engine/layout/src/page.rs:236` **fn** `with_content` — Assign generated content to this margin-box
`crates/engine/layout/src/page.rs:247` **fn** `layout_text` — Layout text content in this margin-box with word-wrapping
`crates/engine/layout/src/page.rs:352` **struct** `PageBox` — Complete page structure with margin-boxes and page properties
`crates/engine/layout/src/page.rs:365` **fn** `new` — Create a new page with computed properties
`crates/engine/layout/src/page.rs:378` **fn** `apply_margin_box_content` — Apply content functions to margin-boxes and generate text
`crates/engine/layout/src/page.rs:407` **fn** `layout_margin_boxes` — Layout all 16 margin-boxes based on page properties
`crates/engine/layout/src/page.rs:524` **fn** `get_margin_box` — Get a margin-box by position
`crates/engine/layout/src/page.rs:529` **fn** `get_margin_box_mut` — Mutably get a margin-box by position
`crates/engine/layout/src/page.rs:544` **fn** `match_page_rules` — Matches @page rules for a given page number and applies properties
`crates/engine/layout/src/page.rs:614` **fn** `compute_page_properties` — Computes page properties from matching @page rules
`crates/engine/layout/src/page.rs:654` **struct** `PageCounters` — Counter value for page numbering and related counters
`crates/engine/layout/src/page.rs:664` **fn** `new` — Create a new counter set with the page counter initialized to 1 (page 1)
`crates/engine/layout/src/page.rs:672` **fn** `get` — Get the value of a named counter
`crates/engine/layout/src/page.rs:677` **fn** `set` — Set the value of a named counter
`crates/engine/layout/src/page.rs:682` **fn** `increment` — Increment a counter by 1
`crates/engine/layout/src/page.rs:689` **fn** `reset` — Reset a counter to a specified value
`crates/engine/layout/src/page.rs:699` **enum** `ContentFunction` — Represents a content function used in margin-box content generation
`crates/engine/layout/src/page.rs:800` **fn** `resolve_content_function` — Resolves a content function to its text representation
`crates/engine/layout/src/page.rs:831` **fn** `create_page_number_footer` — Common margin-box content preset: page number at bottom center
`crates/engine/layout/src/page.rs:846` **fn** `create_page_number_header` — Common margin-box content preset: page number at top center
`crates/engine/layout/src/page.rs:861` **fn** `create_header_footer` — Common margin-box content preset: custom header and footer
`crates/engine/layout/src/pagination.rs:23` **struct** `PaginationContext` — Parameters for print pagination
`crates/engine/layout/src/pagination.rs:47` **fn** `content_width` — Content box width: page width minus left and right margins
`crates/engine/layout/src/pagination.rs:52` **fn** `content_height` — Content box height: page height minus top and bottom margins
`crates/engine/layout/src/pagination.rs:57` **fn** `content_origin` — Top-left corner of content box within page
`crates/engine/layout/src/pagination.rs:67` **struct** `Page` — A single page with positioned content
`crates/engine/layout/src/pagination.rs:88` **struct** `PageFragment` — A fragment of layout tree content positioned on a page
`crates/engine/layout/src/pagination.rs:112` **fn** `paginate` — Pagination algorithm: split LayoutBox tree into pages
`crates/engine/layout/src/property_trees.rs:39` **struct** `PropertyTreeNodeId` — Идентификатор узла в любом из четырёх деревьев. Уникален в пределах своего
`crates/engine/layout/src/property_trees.rs:45` **fn** `raw`
`crates/engine/layout/src/property_trees.rs:54` **struct** `Mat4` — 4×4 матрица в column-major порядке (как принято в OpenGL / WebGPU)
`crates/engine/layout/src/property_trees.rs:65` **fn** `is_identity`
`crates/engine/layout/src/property_trees.rs:70` **fn** `translation_2d` — 2D translation. Z и W колонки остаются identity
`crates/engine/layout/src/property_trees.rs:78` **fn** `scale_2d` — 2D scale. CSS Transforms L1 §13.4
`crates/engine/layout/src/property_trees.rs:88` **fn** `rotate_2d` — 2D rotation вокруг Z (положительный угол — против часовой стрелки в
`crates/engine/layout/src/property_trees.rs:100` **fn** `skew_x` — `skewX(angle)` — сдвигает X пропорционально Y. CSS Transforms L1 §13.7
`crates/engine/layout/src/property_trees.rs:107` **fn** `skew_y` — `skewY(angle)` — сдвигает Y пропорционально X
`crates/engine/layout/src/property_trees.rs:115` **fn** `from_2d_affine` — 2D affine `matrix(a, b, c, d, e, f)` (CSS Transforms L1 §13.10) →
`crates/engine/layout/src/property_trees.rs:128` **fn** `multiply` — Композиция матриц: `lhs * rhs`. Для column-major OpenGL-конвенции
`crates/engine/layout/src/property_trees.rs:154` **fn** `invert_2d_affine` — Инверсия 2D affine-матрицы. Возвращает `None`, если матрица
`crates/engine/layout/src/property_trees.rs:180` **fn** `transform_point_2d` — Применяет 2D affine часть матрицы к точке `(x, y)`. Z/W колонки
`crates/engine/layout/src/property_trees.rs:204` **fn** `perspective` — CSS `perspective(<length>)` — матрица перспективной проекции с фокусным
`crates/engine/layout/src/property_trees.rs:212` **fn** `translate_3d` — 3D translation. CSS `translate3d(tx, ty, tz)` / `translateZ(tz)`
`crates/engine/layout/src/property_trees.rs:222` **fn** `scale_3d` — 3D scale. CSS `scale3d(sx, sy, sz)` / `scaleZ(sz)`
`crates/engine/layout/src/property_trees.rs:233` **fn** `rotate_x` — Поворот вокруг оси X. CSS `rotateX(theta)`, `theta` в радианах
`crates/engine/layout/src/property_trees.rs:247` **fn** `rotate_y` — Поворот вокруг оси Y. CSS `rotateY(theta)`, `theta` в радианах
`crates/engine/layout/src/property_trees.rs:261` **fn** `rotate_z` — Поворот вокруг оси Z. CSS `rotateZ(theta)` ≡ `rotate(theta)`
`crates/engine/layout/src/property_trees.rs:270` **fn** `rotate_3d` — CSS `rotate3d(x, y, z, theta)` — поворот вокруг произвольной оси
`crates/engine/layout/src/property_trees.rs:302` **fn** `from_3d` — CSS `matrix3d(m11, …, m44)` — 16 значений в column-major порядке
`crates/engine/layout/src/property_trees.rs:312` **fn** `project_point` — Применяет полную 4×4 матрицу к точке `(x, y, z)` и выполняет
`crates/engine/layout/src/property_trees.rs:330` **fn** `project_point_z` — Как [`project_point`](Self::project_point), но возвращает и
`crates/engine/layout/src/property_trees.rs:351` **fn** `transform_z` — Возвращает только трансформированную z-координату точки `(x, y, z)`
`crates/engine/layout/src/property_trees.rs:361` **fn** `is_2d_affine` — `true`, если матрица — чистое 2D affine-преобразование (Z/W-строки
`crates/engine/layout/src/property_trees.rs:385` **struct** `TransformNode` — Узел TransformTree. Хранит локальный transform; accumulated transform
`crates/engine/layout/src/property_trees.rs:395` **struct** `TransformTree` — Дерево transform-преобразований. Корень — identity
`crates/engine/layout/src/property_trees.rs:401` **fn** `empty` — Sprint 0 stub: только root с identity
`crates/engine/layout/src/property_trees.rs:411` **fn** `root`
`crates/engine/layout/src/property_trees.rs:418` **struct** `ScrollNode` — Узел ScrollTree. Хранит scrollable rect и текущий scroll offset
`crates/engine/layout/src/property_trees.rs:431` **struct** `ScrollTree`
`crates/engine/layout/src/property_trees.rs:436` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:448` **fn** `root`
`crates/engine/layout/src/property_trees.rs:456` **struct** `EffectNode` — Узел EffectTree. Хранит opacity / filter / blend-mode — всё, что
`crates/engine/layout/src/property_trees.rs:483` **struct** `EffectTree`
`crates/engine/layout/src/property_trees.rs:488` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:494` **fn** `root`
`crates/engine/layout/src/property_trees.rs:502` **struct** `ClipNode` — Узел ClipTree. Хранит clip rectangle в локальных координатах (т.е
`crates/engine/layout/src/property_trees.rs:511` **struct** `ClipTree`
`crates/engine/layout/src/property_trees.rs:516` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:526` **fn** `root`
`crates/engine/layout/src/property_trees.rs:536` **struct** `PropertyTrees` — 4-deep property trees — единая поверхность, которую layout
`crates/engine/layout/src/property_trees.rs:545` **fn** `empty` — Sprint 0 stub: все 4 дерева — empty roots
`crates/engine/layout/src/property_trees.rs:556` **fn** `build_stub` — Совместимость с Sprint 0: пустые root-only деревья. Используется
`crates/engine/layout/src/property_trees.rs:583` **fn** `build` — Построение property trees из layout-дерева (P1 п.2B)
`crates/engine/layout/src/property_trees.rs:614` **fn** `compute_local_transform` — Вычислить локальную transform-матрицу элемента. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:663` **fn** `forward_box_transform` — Forward-матрица бокса в viewport-координатах. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:729` **fn** `transform_fns_to_matrix` — Build the forward transform matrix from a list of TransformFn with a pivot point
`crates/engine/layout/src/ruby.rs:18` **enum** `RubyPosition` — Ruby annotation position relative to base text
`crates/engine/layout/src/ruby.rs:30` **struct** `RubyBox` — Ruby box: base text with optional annotation
`crates/engine/layout/src/ruby.rs:43` **fn** `new` — Create a new Ruby box with default Over positioning
`crates/engine/layout/src/ruby.rs:56` **fn** `with_position` — Set the ruby text position
`crates/engine/layout/src/ruby.rs:62` **fn** `with_inter_char_spacing` — Set inter-character spacing in em units
`crates/engine/layout/src/ruby.rs:77` **fn** `lay_out_ruby` — Layout algorithm for ruby annotations
`crates/engine/layout/src/scroll_timeline.rs:26` **enum** `ScrollAxis` — Selects which scroll axis drives a timeline
`crates/engine/layout/src/scroll_timeline.rs:40` **struct** `Viewport` — Viewport dimensions used during progress resolution
`crates/engine/layout/src/scroll_timeline.rs:53` **struct** `ScrollTimeline` — Scroll progress timeline (CSS `scroll()` function / named `scroll-timeline`)
`crates/engine/layout/src/scroll_timeline.rs:66` **struct** `ViewTimeline` — View progress timeline (CSS `view()` function / named `view-timeline`)
`crates/engine/layout/src/scroll_timeline.rs:79` **struct** `NamedScrollTimeline` — Named scroll timeline resolved from the layout tree
`crates/engine/layout/src/scroll_timeline.rs:94` **struct** `NamedViewTimeline` — Named view timeline resolved from the layout tree
`crates/engine/layout/src/scroll_timeline.rs:161` **fn** `resolve_scroll_progress` — Resolve the scroll progress fraction `[0.0, 1.0]` for a [`ScrollTimeline`]
`crates/engine/layout/src/scroll_timeline.rs:225` **fn** `resolve_view_progress` — Resolve the view progress fraction `[0.0, 1.0]` for a [`ViewTimeline`]
`crates/engine/layout/src/scroll_timeline.rs:270` **fn** `collect_named_scroll_timelines` — Collect all named scroll timelines defined in the layout tree
`crates/engine/layout/src/scroll_timeline.rs:281` **fn** `collect_named_view_timelines` — Collect all named view timelines defined in the layout tree
`crates/engine/layout/src/selection.rs:16` **fn** `caret_at_point` — Find the caret position (DOM node + UTF-8 byte offset) closest to a pixel point
`crates/engine/layout/src/selection.rs:95` **fn** `selection_rects` — Compute pixel rectangles that cover the selected `range` within the layout tree
`crates/engine/layout/src/selector_query.rs:40` **fn** `find_descendant_by_selector` — Finds the first descendant LayoutBox matching the given selector
`crates/engine/layout/src/selector_query.rs:61` **fn** `find_all_descendants_by_selector` — Finds all descendant LayoutBoxes matching the given selector
`crates/engine/layout/src/selector_query.rs:73` **fn** `style_snapshot` — Returns the computed style snapshot for this box
`crates/engine/layout/src/selector_query.rs:86` **struct** `ComputedStyleSnapshot` — Flat snapshot of the most-queried CSS properties for in-process testing
`crates/engine/layout/src/selector_query.rs:218` **fn** `find_box_by_selector` — Returns a reference to the first `LayoutBox` in document order whose
`crates/engine/layout/src/selector_query.rs:276` **fn** `computed_style_by_selector` — Returns the computed style snapshot of the first matching `LayoutBox`
`crates/engine/layout/src/selector_query.rs:292` **fn** `find_all_by_selector` — Returns references to **all** `LayoutBox`es (in document order) whose
`crates/engine/layout/src/selector_query.rs:333` **fn** `query_all` — Returns all [`NodeId`]s in the document that match `sel`
`crates/engine/layout/src/selector_query.rs:370` **fn** `matches_selector` — Returns `true` if `node` matches **any** selector in `sel`
`crates/engine/layout/src/selector_query.rs:541` **fn** `computed_style_to_map` — Serialises a [`ComputedStyle`] to a CSS property → resolved-value map
`crates/engine/layout/src/snapshot.rs:63` **fn** `serialize_layout_tree` — Корневой entry-point: рекурсивно сериализует всё дерево
`crates/engine/layout/src/stacking.rs:29` **struct** `StackingContextId` — Идентификатор stacking context-а. Монотонно растёт от 0; 0 = root
`crates/engine/layout/src/stacking.rs:35` **fn** `raw`
`crates/engine/layout/src/stacking.rs:48` **enum** `PaintPhase` — CSS 2.1 Appendix E — 7-уровневый порядок отрисовки внутри stacking context
`crates/engine/layout/src/stacking.rs:86` **struct** `StackingContext` — Один stacking context: владелец-box + z-index + ссылки на дочерние
`crates/engine/layout/src/stacking.rs:98` **struct** `StackingTree` — Плоское представление stacking-дерева: вектор `StackingContext` + индексы
`crates/engine/layout/src/stacking.rs:105` **fn** `empty_root` — Дерево с единственным root-контекстом без детей. Используется в
`crates/engine/layout/src/stacking.rs:127` **fn** `build` — Построение stacking-дерева из layout-дерева
`crates/engine/layout/src/stacking.rs:149` **fn** `root`
`crates/engine/layout/src/stacking.rs:181` **fn** `creates_stacking_context` — CSS Positioned Layout L3 §9.10 — создаёт ли элемент собственный
`crates/engine/layout/src/stacking.rs:252` **fn** `box_can_own_stacking_context` — Анонимные / неучаствующие в layout box-ы не имеют DOM-элемента, к
`crates/engine/layout/src/stacking.rs:294` **struct** `PaintOrder` — Painting order — линейная последовательность пар `(StackingContextId,
`crates/engine/layout/src/stacking.rs:314` **fn** `from_tree` — Строит painting order по CSS 2.1 Appendix E + CSS Painting Order L3 §3
`crates/engine/layout/src/stacking.rs:322` **fn** `len`
`crates/engine/layout/src/stacking.rs:326` **fn** `is_empty`
`crates/engine/layout/src/style.rs:29` **enum** `Display`
`crates/engine/layout/src/style.rs:70` **enum** `TextAlign`
`crates/engine/layout/src/style.rs:86` **enum** `TextAlignLast` — CSS Text L3 §7.2 — `text-align-last`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:111` **enum** `Direction` — CSS Writing Modes L3 §2.1 — `direction: ltr | rtl`. Inherited
`crates/engine/layout/src/style.rs:123` **struct** `BoxShadow` — CSS Backgrounds L3 §4.6 — спецификация одной тени бокса
`crates/engine/layout/src/style.rs:137` **struct** `TextShadow` — CSS Text Decoration L3 §4 — спецификация одной тени текста
`crates/engine/layout/src/style.rs:150` **enum** `Cursor` — CSS UI L4 §8.1 — `cursor`. Inherited
`crates/engine/layout/src/style.rs:197` **enum** `TextOverflow` — CSS UI L4 §10.1 — `text-overflow`. Не наследуется
`crates/engine/layout/src/style.rs:212` **enum** `Overflow` — CSS Overflow L3 — `overflow`. Не наследуется
`crates/engine/layout/src/style.rs:229` **enum** `Visibility` — CSS Display L3 §4 — `visibility`. Inherited
`crates/engine/layout/src/style.rs:240` **enum** `WhiteSpace` — CSS Text Module L3 §3.1 — `white-space`. Inherited
`crates/engine/layout/src/style.rs:254` **fn** `preserves_whitespace` — True when whitespace (tabs, newlines) is preserved rather than collapsed
`crates/engine/layout/src/style.rs:259` **fn** `is_nowrap` — True when line wrapping is disabled (lines only break at forced breaks)
`crates/engine/layout/src/style.rs:272` **enum** `TextTransform` — CSS Text Module L3 §3.4 — `text-transform`. Inherited
`crates/engine/layout/src/style.rs:285` **fn** `apply` — Применяет преобразование к строке. Не аллоцирует, если transform = None
`crates/engine/layout/src/style.rs:318` **enum** `FontStyle` — CSS Fonts Module L4: `font-style: normal | italic | oblique`. Inherited
`crates/engine/layout/src/style.rs:333` **enum** `FontVariant` — CSS Fonts L4 §6 — `font-variant` (упрощённый Phase 0). Inherited
`crates/engine/layout/src/style.rs:344` **enum** `FontOpticalSizing` — CSS Fonts L4 §7.12 — `font-optical-sizing`. Inherited
`crates/engine/layout/src/style.rs:367` **struct** `FontStretch` — CSS Fonts Module L4 §2.5 — `font-stretch`. Inherited
`crates/engine/layout/src/style.rs:404` **struct** `FontWeight` — CSS Fonts Module L4 §2.4 — `font-weight`. Inherited
`crates/engine/layout/src/style.rs:410` **fn** `is_bold`
`crates/engine/layout/src/style.rs:426` **struct** `FontVariationSetting` — CSS Fonts L4 §7 — одна запись `font-variation-settings`
`crates/engine/layout/src/style.rs:442` **struct** `TextDecorationLine` — Набор активных линий `text-decoration` для элемента
`crates/engine/layout/src/style.rs:464` **enum** `TextDecorationStyle` — CSS Text Decoration L3 §2.2 — `text-decoration-style`. Стиль штриха
`crates/engine/layout/src/style.rs:477` **fn** `parse` — Парсит одиночный keyword. Возвращает `None` для невалидных и для
`crates/engine/layout/src/style.rs:507` **enum** `TextDecorationThickness` — CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Толщина
`crates/engine/layout/src/style.rs:526` **enum** `TextEmphasisStyle` — CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Форма emphasis-marks
`crates/engine/layout/src/style.rs:541` **enum** `TextEmphasisShape`
`crates/engine/layout/src/style.rs:558` **enum** `TextEmphasisPosition` — CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Сторона
`crates/engine/layout/src/style.rs:567` **fn** `is_over`
`crates/engine/layout/src/style.rs:577` **enum** `TextUnderlinePosition` — CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`
`crates/engine/layout/src/style.rs:596` **enum** `ForcedColorAdjust` — CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:611` **enum** `ColorScheme` — CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:630` **struct** `Color`
`crates/engine/layout/src/style.rs:660` **enum** `ColorSpace` — CSS Color L4 §10 — цветовое пространство для wide-gamut значений
`crates/engine/layout/src/style.rs:670` **struct** `ColorFloat` — Wide-gamut цвет с float-каналами [0..1 для in-gamut, за пределами — out-of-gamut]
`crates/engine/layout/src/style.rs:681` **fn** `to_srgb_color` — Конвертирует в sRGB u8, применяя матрицу цветового пространства и гамму
`crates/engine/layout/src/style.rs:711` **fn** `to_linear_srgb` — Линейные sRGB-каналы [0..1] для прямой передачи в GPU без квантизации
`crates/engine/layout/src/style.rs:778` **enum** `CssColor` — CSS Color L4 §4.2 — типизированное цветовое значение каскада
`crates/engine/layout/src/style.rs:786` **fn** `resolve` — Разрешает значение в sRGB u8 Color. `Wide` конвертируется через матрицу
`crates/engine/layout/src/style.rs:796` **fn** `to_color_opt` — Конвертирует в `Color`, минуя `current_color`. `CurrentColor` → `None`
`crates/engine/layout/src/style.rs:805` **fn** `resolve_linear` — Линейные sRGB-каналы для прямой передачи в GPU
`crates/engine/layout/src/style.rs:830` **enum** `SvgPaint` — SVG Presentation §11.2 — `fill` / `stroke` paint value (`<paint>` type)
`crates/engine/layout/src/style.rs:849` **fn** `resolve` — Resolves the paint value to a concrete `Color`. Returns `None` if paint is `none`
`crates/engine/layout/src/style.rs:861` **enum** `FillRule` — SVG §11.3 — `fill-rule`. Inherited. Initial: `NonZero`
`crates/engine/layout/src/style.rs:872` **enum** `StrokeLinecap` — SVG §11.4 — `stroke-linecap`. Inherited. Initial: `Butt`
`crates/engine/layout/src/style.rs:885` **enum** `StrokeLinejoin` — SVG §11.4 — `stroke-linejoin`. Inherited. Initial: `Miter`
`crates/engine/layout/src/style.rs:897` **enum** `BorderStyle` — Стиль линии CSS border. None = рамка не отображается (как `display: none`)
`crates/engine/layout/src/style.rs:907` **fn** `is_visible`
`crates/engine/layout/src/style.rs:920` **enum** `OutlineStyle` — CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
`crates/engine/layout/src/style.rs:930` **fn** `is_visible`
`crates/engine/layout/src/style.rs:943` **enum** `OutlineColor` — CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
`crates/engine/layout/src/style.rs:954` **enum** `BreakValue` — CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside
`crates/engine/layout/src/style.rs:977` **enum** `BoxSizing` — CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
`crates/engine/layout/src/style.rs:989` **enum** `Position` — CSS Positioned Layout L3 §3 — `position`. Не наследуется
`crates/engine/layout/src/style.rs:999` **fn** `parse`
`crates/engine/layout/src/style.rs:1015` **enum** `FloatSide` — CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
`crates/engine/layout/src/style.rs:1024` **fn** `parse` — Parses `float` keyword value
`crates/engine/layout/src/style.rs:1036` **fn** `is_none` — Returns `true` for `float: none`
`crates/engine/layout/src/style.rs:1044` **enum** `ClearSide` — CSS 2.1 §9.5.2 — `clear`. Не наследуется. Указывает, мимо
`crates/engine/layout/src/style.rs:1054` **fn** `parse` — Parses `clear` keyword value
`crates/engine/layout/src/style.rs:1070` **enum** `Isolation` — CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется
`crates/engine/layout/src/style.rs:1077` **fn** `parse`
`crates/engine/layout/src/style.rs:1091` **enum** `MixBlendMode` — CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется
`crates/engine/layout/src/style.rs:1113` **fn** `parse`
`crates/engine/layout/src/style.rs:1151` **enum** `VerticalAlign` — CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется
`crates/engine/layout/src/style.rs:1172` **fn** `parse_keyword` — Парсит keyword-формы vertical-align. Не покрывает `<length>` /
`crates/engine/layout/src/style.rs:1197` **enum** `TimingFunction` — CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations
`crates/engine/layout/src/style.rs:1235` **struct** `LinearEasingPoint` — CSS Easing L2 §2.4 — одна control-точка функции `linear(...)`
`crates/engine/layout/src/style.rs:1254` **fn** `parse` — Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
`crates/engine/layout/src/style.rs:1321` **fn** `parse_list` — CSS Transitions/Animations L1 — comma-list of timing functions
`crates/engine/layout/src/style.rs:1340` **fn** `progress` — CSS Easing L1 §2 — компьютация eased progress
`crates/engine/layout/src/style.rs:1596` **enum** `StepPosition` — CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`
`crates/engine/layout/src/style.rs:1614` **enum** `IterationCount` — CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
`crates/engine/layout/src/style.rs:1626` **fn** `parse`
`crates/engine/layout/src/style.rs:1639` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1649` **enum** `AnimationDirection` — CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`
`crates/engine/layout/src/style.rs:1662` **fn** `parse`
`crates/engine/layout/src/style.rs:1672` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1684` **enum** `AnimationFillMode` — CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`
`crates/engine/layout/src/style.rs:1697` **fn** `parse`
`crates/engine/layout/src/style.rs:1707` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1717` **enum** `AnimationPlayState` — CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`
`crates/engine/layout/src/style.rs:1726` **fn** `parse`
`crates/engine/layout/src/style.rs:1734` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1752` **enum** `CssWideKeyword` — CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству
`crates/engine/layout/src/style.rs:1762` **fn** `parse_css_wide_keyword` — ASCII case-insensitive проверка значения декларации на CSS-wide keyword
`crates/engine/layout/src/style.rs:1778` **struct** `ComputedStyle`
`crates/engine/layout/src/style.rs:2414` **enum** `Content` — CSS Content L3 — value свойства `content`
`crates/engine/layout/src/style.rs:2427` **enum** `ContentItem`
`crates/engine/layout/src/style.rs:2456` **enum** `ScrollbarWidth` — CSS Scrollbars 1 — `scrollbar-width`. Inherited
`crates/engine/layout/src/style.rs:2467` **fn** `parse`
`crates/engine/layout/src/style.rs:2479` **enum** `ScrollbarGutter` — CSS Overflow L3 — `scrollbar-gutter`
`crates/engine/layout/src/style.rs:2490` **fn** `parse`
`crates/engine/layout/src/style.rs:2509` **enum** `ListStyleType` — CSS Lists L3 §2.1 — markers для list items
`crates/engine/layout/src/style.rs:2536` **fn** `parse`
`crates/engine/layout/src/style.rs:2558` **enum** `ListStylePosition` — CSS Lists L3 §2.3 — `list-style-position`
`crates/engine/layout/src/style.rs:2567` **fn** `parse`
`crates/engine/layout/src/style.rs:2578` **enum** `OverflowWrap` — CSS Text L3 §5.2 — `overflow-wrap`
`crates/engine/layout/src/style.rs:2589` **fn** `parse`
`crates/engine/layout/src/style.rs:2603` **enum** `LineBreak` — CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2614` **enum** `WordBreak` — CSS Text L3 §5.1 — `word-break`
`crates/engine/layout/src/style.rs:2626` **fn** `parse`
`crates/engine/layout/src/style.rs:2639` **enum** `Hyphens` — CSS Text L3 §6 — `hyphens`
`crates/engine/layout/src/style.rs:2652` **fn** `parse`
`crates/engine/layout/src/style.rs:2666` **enum** `TouchAction` — CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2684` **enum** `Appearance` — CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2695` **enum** `PointerEvents` — CSS Pointer Events L1. Default `auto`
`crates/engine/layout/src/style.rs:2709` **fn** `parse`
`crates/engine/layout/src/style.rs:2729` **enum** `Resize` — CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2743` **struct** `ContainFlags` — CSS Containment L3 §3 — `contain` property
`crates/engine/layout/src/style.rs:2760` **enum** `ContentVisibility` — CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`
`crates/engine/layout/src/style.rs:2769` **enum** `ContainerType` — CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:2779` **struct** `ContainerContext` — Resolved container dimensions, passed during style re-computation for container queries
`crates/engine/layout/src/style.rs:2793` **fn** `evaluate_container_condition` — Evaluates a raw @container condition string against a `ContainerContext`
`crates/engine/layout/src/style.rs:2871` **fn** `apply_container_rules` — Applies matching `@container` rules from `sheet` to `style`
`crates/engine/layout/src/style.rs:2916` **enum** `ShapeOutside` — CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2925` **enum** `OffsetRotate` — CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2936` **enum** `PrintColorAdjust` — CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`
`crates/engine/layout/src/style.rs:2944` **enum** `FontSizeAdjust` — CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2953` **enum** `WritingMode` — CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`
`crates/engine/layout/src/style.rs:2970` **enum** `TextOrientation` — CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`
`crates/engine/layout/src/style.rs:2982` **enum** `UserSelect` — CSS UI L4 §6.2 — `user-select`. Inherited
`crates/engine/layout/src/style.rs:2992` **fn** `parse`
`crates/engine/layout/src/style.rs:3006` **enum** `ScrollBehavior` — CSS Overflow L3 — `scroll-behavior`. Inherited
`crates/engine/layout/src/style.rs:3014` **struct** `ScrollSnapType` — CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`
`crates/engine/layout/src/style.rs:3020` **enum** `ScrollSnapAxis`
`crates/engine/layout/src/style.rs:3031` **enum** `ScrollSnapStrictness`
`crates/engine/layout/src/style.rs:3039` **struct** `ScrollSnapAlign` — CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`
`crates/engine/layout/src/style.rs:3045` **enum** `ScrollSnapAlignKeyword`
`crates/engine/layout/src/style.rs:3054` **enum** `ScrollSnapStop`
`crates/engine/layout/src/style.rs:3062` **enum** `OverscrollBehavior` — CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`
`crates/engine/layout/src/style.rs:3070` **fn** `parse`
`crates/engine/layout/src/style.rs:3085` **enum** `ParsedGradient` — CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient
`crates/engine/layout/src/style.rs:3125` **enum** `BackgroundImage` — CSS Backgrounds L3 §3.1 / CSS Images L4 §4 — `background-image` value
`crates/engine/layout/src/style.rs:3153` **enum** `BackgroundRepeat` — CSS Backgrounds L3 §3.4 — `background-repeat`
`crates/engine/layout/src/style.rs:3164` **fn** `parse`
`crates/engine/layout/src/style.rs:3179` **enum** `BackgroundSize` — CSS Backgrounds L3 §3.5 — `background-size`
`crates/engine/layout/src/style.rs:3190` **enum** `BackgroundAttachment` — CSS Backgrounds L3 §3.6 — `background-attachment`
`crates/engine/layout/src/style.rs:3198` **fn** `parse`
`crates/engine/layout/src/style.rs:3219` **enum** `BackgroundOrigin` — CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited
`crates/engine/layout/src/style.rs:3230` **fn** `parse`
`crates/engine/layout/src/style.rs:3253` **enum** `BackgroundClip` — CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited
`crates/engine/layout/src/style.rs:3267` **fn** `parse`
`crates/engine/layout/src/style.rs:3283` **struct** `BackgroundLayer` — CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним)
`crates/engine/layout/src/style.rs:3323` **enum** `ObjectFit` — CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
`crates/engine/layout/src/style.rs:3344` **fn** `parse`
`crates/engine/layout/src/style.rs:3364` **enum** `ImageRendering` — CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
`crates/engine/layout/src/style.rs:3384` **fn** `parse`
`crates/engine/layout/src/style.rs:3408` **enum** `TextWrapMode` — CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited
`crates/engine/layout/src/style.rs:3417` **fn** `parse`
`crates/engine/layout/src/style.rs:3435` **enum** `TextWrapStyle` — CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited
`crates/engine/layout/src/style.rs:3448` **fn** `parse`
`crates/engine/layout/src/style.rs:3464` **enum** `FlexDirection` — CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited
`crates/engine/layout/src/style.rs:3477` **fn** `parse`
`crates/engine/layout/src/style.rs:3493` **enum** `FlexWrap` — CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited
`crates/engine/layout/src/style.rs:3504` **fn** `parse`
`crates/engine/layout/src/style.rs:3519` **enum** `FlexBasis` — CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited
`crates/engine/layout/src/style.rs:3530` **fn** `parse`
`crates/engine/layout/src/style.rs:3544` **struct** `GridRepeat` — CSS Grid Layout L3 §9 — `repeat(auto-fill | auto-fit | <count>, <track-list>)`
`crates/engine/layout/src/style.rs:3553` **enum** `RepeatCount` — Count type for grid-template-columns/rows `repeat()`
`crates/engine/layout/src/style.rs:3566` **enum** `GridTrackSize` — CSS Grid Layout L1 §7.2 — sizing function for a grid track
`crates/engine/layout/src/style.rs:3594` **fn** `resolve_fixed` — Resolve to a concrete pixel size given container width, em, viewport
`crates/engine/layout/src/style.rs:3603` **fn** `is_fr` — True for fractional tracks
`crates/engine/layout/src/style.rs:3608` **fn** `fr` — Extract fr value
`crates/engine/layout/src/style.rs:3613` **fn** `is_subgrid` — True when this track inherits its size from the parent grid (subgrid axis)
`crates/engine/layout/src/style.rs:3658` **fn** `parse_track_list` — Parse a track-list value string into a Vec of GridTrackSize
`crates/engine/layout/src/style.rs:3785` **enum** `GridAutoFlow` — CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited
`crates/engine/layout/src/style.rs:3798` **fn** `parse`
`crates/engine/layout/src/style.rs:3812` **enum** `GridLine` — CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
`crates/engine/layout/src/style.rs:3826` **fn** `parse`
`crates/engine/layout/src/style.rs:3861` **enum** `PositionComponent` — Одна компонента `object-position`. Length-варианты резолвятся в px
`crates/engine/layout/src/style.rs:3874` **fn** `resolve` — Резолв в финальный px-offset относительно левого/верхнего края
`crates/engine/layout/src/style.rs:3885` **struct** `ObjectPosition` — CSS Images L3 §5.5 — `object-position` (две компоненты, x + y)
`crates/engine/layout/src/style.rs:3922` **fn** `parse` — CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
`crates/engine/layout/src/style.rs:4024` **enum** `AlignValue` — CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств
`crates/engine/layout/src/style.rs:4051` **fn** `parse`
`crates/engine/layout/src/style.rs:4073` **enum** `ClipPath` — CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
`crates/engine/layout/src/style.rs:4097` **enum** `TransformStyle` — CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:4106` **enum** `TransformFn` — CSS transform functions — translate/scale/rotate/skew/skewX/skewY/matrix
`crates/engine/layout/src/style.rs:4144` **enum** `FilterFn` — CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:4177` **struct** `GradientStop` — CSS Images L3 §3.4 — единичный `<color-stop>` градиента
`crates/engine/layout/src/style.rs:4187` **fn** `outline_used_width` — CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
`crates/engine/layout/src/style.rs:4198` **fn** `text_rendering_eq` — Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
`crates/engine/layout/src/style.rs:4215` **fn** `root` — Стартовые значения для корня документа
`crates/engine/layout/src/style.rs:4472` **fn** `compute_style` — Computes the `ComputedStyle` for `node` by running the CSS cascade
`crates/engine/layout/src/style.rs:5289` **fn** `compute_pseudo_element_style` — Вычисляет стиль для псевдоэлемента `::before` или `::after` элемента `node`
`crates/engine/layout/src/style.rs:5517` **fn** `validate_against_syntax` — CSS Properties and Values L1 §2 — упрощённая валидация значения
`crates/engine/layout/src/style.rs:7761` **fn** `ua_form_element_colors` — UA stylesheet для HTML form controls (HTML5 §15.5 «Rendering»)
`crates/engine/layout/src/style.rs:7893` **fn** `parse_font_family` — Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
`crates/engine/layout/src/style.rs:7956` **fn** `parse_font_variation_settings` — Парсит CSS `font-variation-settings` (CSS Fonts L4 §7)
`crates/engine/layout/src/style.rs:8048` **fn** `set_cq_context` — Sets the nearest-container size for `cq*` unit resolution during the container re-layout pass
`crates/engine/layout/src/style.rs:8053` **fn** `clear_cq_context` — Clears the `cq*` context after the container re-layout pass completes
`crates/engine/layout/src/style.rs:8077` **fn** `set_interactive_state` — Sets the interactive hover/focus/active state for the next layout pass
`crates/engine/layout/src/style.rs:8088` **fn** `clear_interactive_state` — Clears hover/focus/active state after layout
`crates/engine/layout/src/style.rs:8133` **enum** `LengthOrAuto` — CSS `<length> | auto` — для margin и offset-свойств, где `auto` имеет
`crates/engine/layout/src/style.rs:8141` **fn** `is_auto`
`crates/engine/layout/src/style.rs:8148` **fn** `to_px_opt` — Returns the raw pixel value for `Length::Px` variants; `Auto` and all
`crates/engine/layout/src/style.rs:8158` **fn** `resolve` — Резолвит в пиксели. `Auto` → `None`; нерезолвируемый `%` → `None`
`crates/engine/layout/src/style.rs:8166` **fn** `resolve_or_zero` — Резолвит в пиксели; для `Auto` и нерезолвируемых значений → 0.0
`crates/engine/layout/src/style.rs:8177` **enum** `Length` — Типизированная длина CSS до резолва в пиксели
`crates/engine/layout/src/style.rs:8242` **enum** `CalcNode` — CSS Values L4 §10 — AST `calc()`-выражения. Хранится как двоичное дерево
`crates/engine/layout/src/style.rs:8271` **enum** `MathFn` — CSS Values L4 §10.7-10.9 — научные math-функции. Имена case-insensitive
`crates/engine/layout/src/style.rs:8300` **enum** `RoundStrategy` — CSS Values L4 §10.5.1 — стратегия округления для `round()`
`crates/engine/layout/src/style.rs:8324` **fn** `resolve` — Резолвит выражение в `f32`-пиксели по тем же правилам, что
`crates/engine/layout/src/style.rs:8522` **fn** `resolve` — Возвращает длину в пикселях. `em_basis` — fs, относительно которого
`crates/engine/layout/src/style.rs:8562` **fn** `is_intrinsic` — Returns `true` if this is an intrinsic sizing keyword (min-content,
`crates/engine/layout/src/style.rs:8568` **fn** `resolve_or_zero` — Резолвит с `cb_width` как percent_basis; возвращает 0.0 при неудаче
`crates/engine/layout/src/style.rs:8574` **fn** `px` — Извлекает пиксельное значение для уже-разрешённых `Px`-значений
`crates/engine/layout/src/style.rs:8729` **fn** `parse_length`
`crates/engine/layout/src/style.rs:13467` **fn** `parse_transform_list` — Парсит `<transform-list>` — последовательность `func(args)` через
`crates/engine/layout/src/style.rs:14436` **fn** `parse_grid_template_areas` — CSS Grid L1 §7.3 — parse `grid-template-areas` value
`crates/engine/layout/src/style.rs:14516` **fn** `parse_background_gradient` — CSS Images L3/L4 §3.3/§3.7 — parses color stops from a CSS gradient string
`crates/engine/layout/src/style.rs:14708` **fn** `parse_gradient_stops` — The leading direction / angle / shape argument (e.g. `to right`,
`crates/engine/layout/src/style.rs:15283` **fn** `parse_color`
`crates/engine/layout/src/subgrid.rs:24` **struct** `SubgridContext` — Resolved track sizes and cumulative offsets for one grid axis (columns or rows)
`crates/engine/layout/src/subgrid.rs:35` **fn** `from_parent_tracks` — Build from a slice of parent track sizes and the gap value used between them
`crates/engine/layout/src/subgrid.rs:46` **fn** `total_size` — Total span width/height occupied by all inherited tracks (including inter-track gaps)
`crates/engine/layout/src/subgrid.rs:96` **struct** `SubgridItem` — A grid item that is itself a subgrid container for at least one axis
`crates/engine/layout/src/subgrid.rs:113` **fn** `collect_subgrid_items` — Collect all layout boxes in the tree that are subgrid containers
`crates/engine/layout/src/table.rs:20` **enum** `BorderCollapse` — CSS Tables L2 §17.6 — border-collapse mode for table layout
`crates/engine/layout/src/table.rs:31` **enum** `BorderPrecedence` — CSS Tables L2 §17.6.2 — precedence level used when two borders compete in collapsed mode
`crates/engine/layout/src/table.rs:52` **struct** `CollapsedBorder` — Resolved border description for the collapsed border model (CSS Tables L2 §17.6.2)
`crates/engine/layout/src/table.rs:64` **fn** `resolve_conflict` — Resolves conflict between two competing borders per CSS Tables L2 §17.6.2:
`crates/engine/layout/src/table.rs:81` **struct** `TableContext` — Table layout algorithm context
`crates/engine/layout/src/table.rs:123` **fn** `new` — Create a new empty table context with CSS-initial values
`crates/engine/layout/src/table.rs:138` **fn** `collect_table_structure` — Scan table structure and infer column count, explicit widths, and rowspan occupancy
`crates/engine/layout/src/table.rs:253` **fn** `compute_table_col_widths` — Compute table column widths using the table-layout algorithm
`crates/engine/layout/src/table.rs:288` **fn** `lay_out_table` — Lay out table rows and cells
`crates/engine/layout/src/text_iter.rs:17` **struct** `TextFragment` — A visible text fragment with its absolute screen rectangle
`crates/engine/layout/src/text_iter.rs:37` **fn** `collect_visible_text` — Walk the layout tree and collect all visible text fragments with screen coordinates

## lumen-mcp  (24 symbols)

`crates/mcp/src/protocol.rs:8` **struct** `McpResource` — MCP resource describing a read-only data snapshot
`crates/mcp/src/protocol.rs:21` **struct** `McpTool` — MCP tool describing a callable action
`crates/mcp/src/protocol.rs:32` **struct** `McpRequest` — MCP JSON-RPC запрос
`crates/mcp/src/protocol.rs:47` **fn** `new` — Создать новый MCP запрос
`crates/mcp/src/protocol.rs:57` **fn** `with_id` — Создать запрос с ID для отслеживания ответа
`crates/mcp/src/protocol.rs:65` **struct** `McpResponse` — MCP JSON-RPC ответ
`crates/mcp/src/protocol.rs:80` **fn** `ok` — Создать успешный ответ
`crates/mcp/src/protocol.rs:90` **fn** `err` — Создать ошибку
`crates/mcp/src/protocol.rs:106` **struct** `McpError` — JSON-RPC ошибка
`crates/mcp/src/protocol.rs:118` **enum** `McpMessage` — Размеченное MCP сообщение (запрос или ответ)
`crates/mcp/src/protocol.rs:129` **fn** `from_json` — Распарсить JSON в MCP сообщение
`crates/mcp/src/protocol.rs:137` **fn** `to_json` — Сериализовать MCP сообщение в JSON
`crates/mcp/src/server.rs:15` **struct** `McpServer` — MCP сервер для Lumen браузера
`crates/mcp/src/server.rs:24` **fn** `new` — Создать новый MCP сервер
`crates/mcp/src/server.rs:29` **fn** `run` — Основной цикл сервера: читать запросы и писать ответы
`crates/mcp/src/transport.rs:10` **trait** `Transport` — Абстракция транспорта для MCP сообщений
`crates/mcp/src/transport.rs:22` **struct** `StdioTransport` — Stdio-транспорт (stdin/stdout)
`crates/mcp/src/transport.rs:29` **fn** `new` — Создать новый stdio-транспорт
`crates/mcp/src/transport.rs:69` **struct** `TcpTransport` — TCP-транспорт для `--mcp-port N` режима
`crates/mcp/src/transport.rs:76` **fn** `from_stream` — Создать транспорт поверх уже принятого `TcpStream`
`crates/mcp/src/transport.rs:113` **struct** `VecTransport` — In-memory транспорт для unit-тестов
`crates/mcp/src/transport.rs:122` **fn** `new` — Создать пустой транспорт
`crates/mcp/src/transport.rs:127` **fn** `push_incoming` — Поставить в очередь входящее JSON сообщение
`crates/mcp/src/transport.rs:132` **fn** `take_outgoing` — Забрать все исходящие сообщения (очищает буфер)

## lumen-network  (250 symbols)

`crates/network/src/auth.rs:52` **fn** `get`
`crates/network/src/auth.rs:619` **struct** `StaticCredentialProvider` — Простой credential-провайдер с фиксированной табличкой `(origin, realm) →
`crates/network/src/auth.rs:624` **fn** `new`
`crates/network/src/auth.rs:632` **fn** `with` — Точное совпадение `(origin, realm)`
`crates/network/src/auth.rs:640` **fn** `add` — Зарегистрировать creds после конструирования. `&self` (не `&mut`) —
`crates/network/src/brotli.rs:24` **struct** `BrotliContentDecoder` — `ContentDecoder` для `Content-Encoding: br`. Stateless: один экземпляр
`crates/network/src/cors.rs:35` **enum** `CredentialsMode` — Credentials mode по Fetch §3.1 — определяет, прикладывать ли cookies /
`crates/network/src/cors.rs:50` **fn** `cross_origin_credentials` — Применяются ли credentials для cross-origin запроса в этом режиме?
`crates/network/src/cors.rs:62` **struct** `CorsRequest` — Cross-origin запрос — описание для решения о preflight и сборки CORS-заголовков
`crates/network/src/cors.rs:74` **fn** `is_cors_safelisted_method` — «CORS-safelisted method» (Fetch §4.4.1): GET / HEAD / POST
`crates/network/src/cors.rs:83` **fn** `is_forbidden_request_header` — «forbidden request-header name» (Fetch §4.4.4). UA-controlled заголовки,
`crates/network/src/cors.rs:123` **fn** `is_cors_safelisted_request_header` — «CORS-safelisted request-header» (Fetch §4.4.2). Возвращает true, если
`crates/network/src/cors.rs:151` **fn** `is_cors_safelisted_content_type` — «CORS-safelisted Content-Type» (Fetch §4.4.2): одна из трёх MIME-форм
`crates/network/src/cors.rs:204` **fn** `needs_preflight` — Возвращает true, если запрос требует preflight перед actual request
`crates/network/src/cors.rs:221` **fn** `unsafe_request_header_names` — Имена «unsafe» author-заголовков (lowercased + sorted lexicographically)
`crates/network/src/cors.rs:249` **fn** `build_preflight_headers` — Заголовки OPTIONS preflight-запроса
`crates/network/src/cors.rs:271` **struct** `PreflightResult` — Результат успешного preflight-а. Кешируется по (origin, target_origin,
`crates/network/src/cors.rs:291` **fn** `method_allowed` — Покрывает ли результат preflight-а метод `method` (case-insensitive)?
`crates/network/src/cors.rs:310` **fn** `unmatched_header` — Покрывает ли результат preflight-а все unsafe-заголовки запроса?
`crates/network/src/cors.rs:331` **enum** `CorsError` — Ошибки CORS-валидации (preflight или actual response)
`crates/network/src/cors.rs:393` **fn** `evaluate_preflight_response` — Полный разбор preflight-ответа. Возвращает [`PreflightResult`] для
`crates/network/src/cors.rs:436` **fn** `check_cors_response_headers` — Валидация ACAO + ACAC на **actual response** (не preflight) — Fetch §4.10
`crates/network/src/cors.rs:543` **struct** `PreflightCache` — Кеш preflight-результатов по `(requestor_origin, target_origin,
`crates/network/src/cors.rs:561` **fn** `new`
`crates/network/src/cors.rs:570` **fn** `insert_at` — Добавить результат preflight-а в кеш. `now` — текущее время от UNIX
`crates/network/src/cors.rs:592` **fn** `insert` — То же что [`Self::insert_at`], но с `now = SystemTime::now()`. Для
`crates/network/src/cors.rs:604` **fn** `lookup_at` — Достать НЕИСТЁКШЕЕ entry. Истёкшие удаляются lazy (next-access
`crates/network/src/cors.rs:625` **fn** `lookup`
`crates/network/src/cors.rs:637` **fn** `allows_at` — Возвращает true, если кеш содержит подходящее entry для `req` (метод
`crates/network/src/cors.rs:652` **fn** `allows`
`crates/network/src/cors.rs:657` **fn** `clear` — Полная очистка (для тестов / Profile switching)
`crates/network/src/csp.rs:14` **enum** `HashAlgorithm` — Hash algorithm used in a CSP hash source expression
`crates/network/src/csp.rs:28` **enum** `CspSource` — A single source expression from a CSP directive source list
`crates/network/src/csp.rs:60` **enum** `CspDirective` — A CSP fetch / navigation directive name
`crates/network/src/csp.rs:111` **struct** `CspPolicy` — A parsed Content Security Policy
`crates/network/src/csp.rs:128` **fn** `is_empty` — Returns `true` if no directives or flags are set
`crates/network/src/csp.rs:140` **fn** `effective_sources` — Returns the effective source list for `directive`, falling back to
`crates/network/src/csp.rs:159` **fn** `parse_csp_header` — Parse a `Content-Security-Policy` header value into a [`CspPolicy`]
`crates/network/src/csp.rs:166` **fn** `parse_csp_report_only_header` — Parse a report-only variant of the CSP header
`crates/network/src/dns.rs:22` **struct** `SystemDnsResolver` — DNS-резолвер на основе системного getaddrinfo (через std::net)
`crates/network/src/doh.rs:46` **fn** `encode_query` — Закодировать стандартный DNS query — header + одна question. RD=1
`crates/network/src/doh.rs:100` **fn** `decode_answer_ips` — Распакованный DNS-ответ — без CNAME-цепочек, только IP-адреса из
`crates/network/src/doh.rs:249` **fn** `base64url_encode` — Закодировать байты в base64url **без padding** — RFC 8484 §4.1 явно
`crates/network/src/doh.rs:302` **struct** `DohResolver` — DNS-over-HTTPS резолвер
`crates/network/src/doh.rs:310` **fn** `new` — `endpoint` — URL DoH сервера со схемой `https://`. `transport` —
`crates/network/src/doh.rs:405` **struct** `CachedDnsResolver` — Used to reduce DoH / system DNS lookups when resolving frequently-used hosts
`crates/network/src/doh.rs:413` **fn** `new` — Create a new cached resolver wrapping `inner`
`crates/network/src/dot.rs:62` **fn** `frame_query` — Обернуть DNS message в two-octet length prefix: `[u16 BE len][msg]`
`crates/network/src/dot.rs:77` **fn** `read_framed_message` — Прочитать ОДНО framed DNS message из stream-а: 2 байта BE length,
`crates/network/src/dot.rs:107` **fn** `query_over_stream` — Послать ОДИН DNS query (AAAA или A — определяется `qtype`) по уже
`crates/network/src/dot.rs:140` **struct** `DotResolver` — DNS-over-TLS резолвер
`crates/network/src/dot.rs:149` **fn** `new` — Базовый конструктор. `server_name` — TLS SNI/cert host;
`crates/network/src/dot.rs:159` **fn** `cloudflare` — Cloudflare `1.1.1.1:853` с SNI `one.one.one.one`
`crates/network/src/dot.rs:167` **fn** `google` — Google Public DNS `8.8.8.8:853` с SNI `dns.google`
`crates/network/src/dot.rs:175` **fn** `quad9` — Quad9 `9.9.9.9:853` с SNI `dns.quad9.net`
`crates/network/src/filter/easylist.rs:73` **struct** `EasyListFilter` — EasyList-format `RequestFilter` implementation
`crates/network/src/filter/easylist.rs:91` **fn** `parse` — Parse an EasyList-format text and return a filter
`crates/network/src/filter/easylist.rs:100` **fn** `rule_count` — Number of block rules loaded
`crates/network/src/filter/hosts.rs:28` **struct** `HostsFilter` — Hosts-file `RequestFilter`
`crates/network/src/filter/hosts.rs:34` **fn** `parse` — Parse a hosts-file text and return a filter
`crates/network/src/filter/hosts.rs:73` **fn** `len` — Number of blocked hostnames
`crates/network/src/filter/hosts.rs:78` **fn** `is_empty` — Returns `true` if the block list is empty
`crates/network/src/filter/mod.rs:36` **struct** `CompositeFilter` — Chains multiple [`RequestFilter`] implementations
`crates/network/src/filter/mod.rs:42` **fn** `new` — Create a composite filter from a list of inner filters
`crates/network/src/h2/conn.rs:54` **type** `H2Response` — Decoded HTTP response from an H2 fetch: `(status, headers, body)`
`crates/network/src/h2/conn.rs:103` **struct** `H2Conn` — Stateful HTTP/2 client connection
`crates/network/src/h2/conn.rs:130` **fn** `connect` — Establish an HTTP/2 connection with Chrome-matching SETTINGS
`crates/network/src/h2/conn.rs:139` **fn** `connect_with_profile` — Establish an HTTP/2 connection over `stream` with SETTINGS matching the given profile
`crates/network/src/h2/conn.rs:274` **fn** `fetch` — Perform a single HTTP/2 request and collect the response
`crates/network/src/h2/conn.rs:447` **fn** `send_request` — Send a single HTTP/2 request without waiting for the response
`crates/network/src/h2/conn.rs:495` **fn** `read_response_for_stream` — Read and assemble the complete response for a specific stream ID
`crates/network/src/h2/frame.rs:107` **enum** `FrameError` — Codec-level error. The codec produces only two RFC 9113 §7 error codes on
`crates/network/src/h2/frame.rs:150` **struct** `Priority` — Stream priority block — used by the PRIORITY frame and by HEADERS when the
`crates/network/src/h2/frame.rs:162` **enum** `Frame` — Parsed/encodable HTTP/2 frame (RFC 9113 §6). For padded frames the carried
`crates/network/src/h2/frame.rs:286` **fn** `parse` — Parse one frame from `buf`
`crates/network/src/h2/frame.rs:337` **fn** `encode` — Serialize the frame: append the 9-byte header and payload to `out`
`crates/network/src/h2/hpack.rs:17` **enum** `HpackError` — HPACK codec error. All variants map to `COMPRESSION_ERROR` (0x09) at the
`crates/network/src/h2/hpack.rs:393` **fn** `decode_int` — Decode a variable-length integer with an `n`-bit prefix from `src`
`crates/network/src/h2/hpack.rs:430` **fn** `encode_int` — Encode an integer with an `n`-bit prefix. The `prefix_byte` holds the
`crates/network/src/h2/hpack.rs:450` **fn** `huffman_encode` — Huffman-encode `input`. The result is padded to a byte boundary with
`crates/network/src/h2/hpack.rs:480` **fn** `huffman_decode` — Huffman-decode `input`. Padding bits (EOS prefix, all-ones) are accepted
`crates/network/src/h2/hpack.rs:523` **fn** `decode_string` — Decode a header string (literal or Huffman) from `src`
`crates/network/src/h2/hpack.rs:545` **fn** `encode_string` — Encode a header string. When `use_huffman` is true, the string is
`crates/network/src/h2/hpack.rs:569` **struct** `DynamicTable` — The dynamic table. Entries are added at the front (lowest dynamic index)
`crates/network/src/h2/hpack.rs:581` **fn** `new`
`crates/network/src/h2/hpack.rs:591` **fn** `set_max_size` — Update the maximum size (from a dynamic table size update instruction
`crates/network/src/h2/hpack.rs:597` **fn** `add` — Add a new entry, evicting old ones as needed
`crates/network/src/h2/hpack.rs:611` **fn** `get` — Return `(name, value)` for a 1-based dynamic index (1 = most recent)
`crates/network/src/h2/hpack.rs:617` **fn** `len`
`crates/network/src/h2/hpack.rs:621` **fn** `is_empty`
`crates/network/src/h2/hpack.rs:666` **struct** `HeaderField` — A decoded header field
`crates/network/src/h2/hpack.rs:675` **fn** `new`
`crates/network/src/h2/hpack.rs:683` **fn** `sensitive`
`crates/network/src/h2/hpack.rs:692` **fn** `name_str` — Returns `name` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`)
`crates/network/src/h2/hpack.rs:697` **fn** `value_str` — Returns `value` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`)
`crates/network/src/h2/hpack.rs:705` **struct** `Decoder` — Stateful HPACK decoder. One instance per HTTP/2 connection direction
`crates/network/src/h2/hpack.rs:712` **fn** `new`
`crates/network/src/h2/hpack.rs:721` **fn** `set_proto_max` — Update the protocol-level maximum table size (call when the remote
`crates/network/src/h2/hpack.rs:729` **fn** `decode` — Decode a complete header block fragment into a list of header fields
`crates/network/src/h2/hpack.rs:812` **struct** `Encoder` — Stateful HPACK encoder. One instance per HTTP/2 connection direction
`crates/network/src/h2/hpack.rs:819` **fn** `new`
`crates/network/src/h2/hpack.rs:826` **fn** `with_huffman`
`crates/network/src/h2/hpack.rs:833` **fn** `set_max_size` — Update the maximum dynamic table size. Emits a dynamic table size
`crates/network/src/h2/hpack.rs:844` **fn** `encode` — Encode a list of `(name, value)` pairs into a header block fragment
`crates/network/src/h2/pool.rs:35` **struct** `H2Pool` — A shared pool of HTTP/2 connections, one per origin
`crates/network/src/h2/pool.rs:40` **fn** `new`
`crates/network/src/hsts_preload.rs:23` **struct** `HstsPreloadList` — HSTS Preload List: быстрый поиск по eTLD+1
`crates/network/src/hsts_preload.rs:36` **fn** `load` — Создать preload list из встроенного JSON (Chromium формат)
`crates/network/src/hsts_preload.rs:100` **fn** `is_preloaded` — Проверить, есть ли хост в preload list
`crates/network/src/hsts_preload.rs:128` **fn** `get_preload_list` — Получить глобальный preload list
`crates/network/src/http/client_hints.rs:14` **enum** `ClientHintsProfile` — Client Hints profile — determines which hints to send
`crates/network/src/http/client_hints.rs:23` **fn** `for_http_profile` — Create ClientHintsProfile for the given HTTP profile
`crates/network/src/http/client_hints.rs:40` **fn** `should_send_client_hints` — Determine whether to send Client Hints headers for the given HTTP profile
`crates/network/src/http/client_hints.rs:56` **fn** `client_hints_headers` — Build Client Hints headers for the given UA string (Lumen)
`crates/network/src/http/h2_settings.rs:11` **struct** `H2Settings` — HTTP/2 SETTINGS frame values matching Chrome's configuration
`crates/network/src/http/h2_settings.rs:33` **fn** `for_profile` — Create HTTP/2 SETTINGS for the given profile
`crates/network/src/http/h2_settings.rs:108` **fn** `to_wire_format` — Convert SETTINGS to HTTP/2 wire format: list of (id, value) pairs
`crates/network/src/http/h2_settings.rs:145` **struct** `H2StreamPriority` — HTTP/2 stream priority information for matching Chrome's priority tree
`crates/network/src/http/h2_settings.rs:158` **fn** `default_for_profile` — Create default HTTP/2 stream priority for the root stream
`crates/network/src/http/h2_settings.rs:169` **fn** `to_wire_format` — Convert priority to HTTP/2 wire format (PRIORITY frame payload)
`crates/network/src/http/headers.rs:14` **enum** `HttpProfile` — HTTP profile — determines header order, casing, and HTTP/2 SETTINGS configuration
`crates/network/src/http/headers.rs:53` **struct** `HeaderOrder` — Chrome HTTP/1.1 header order (in request)
`crates/network/src/http/headers.rs:59` **fn** `new` — Create a new header order builder for the given profile
`crates/network/src/http/headers.rs:69` **fn** `add` — Add a header (key, value) to the ordered list
`crates/network/src/http/headers.rs:83` **fn** `to_http_block` — Build the HTTP/1.1 header block string for the request line
`crates/network/src/http/headers.rs:96` **fn** `as_tuples` — Return headers as a list of tuples
`crates/network/src/http/headers.rs:101` **fn** `clear` — Clear all headers
`crates/network/src/http/headers.rs:117` **fn** `build_request_headers` — Build HTTP/1.1 request headers for the given profile
`crates/network/src/http_cache.rs:27` **struct** `CacheControl` — Parsed subset of `Cache-Control` response directives
`crates/network/src/http_cache.rs:42` **fn** `parse` — Parse `Cache-Control` response header value
`crates/network/src/http_cache.rs:62` **fn** `max_age_secs` — Effective freshness lifetime. s-maxage takes precedence over max-age
`crates/network/src/http_cache.rs:89` **struct** `CacheEntry` — A single stored HTTP response (in-memory representation)
`crates/network/src/http_cache.rs:109` **fn** `is_fresh` — True if the entry is fresh and can be served without revalidation
`crates/network/src/http_cache.rs:118` **fn** `conditional_headers` — Build conditional GET headers to revalidate this entry
`crates/network/src/http_cache.rs:137` **struct** `CacheEntrySnapshot` — Owned snapshot of a cache entry returned by `HttpCacheBackend::get`
`crates/network/src/http_cache.rs:160` **trait** `HttpCacheBackend` — Shared interface for HTTP cache backends (in-memory and disk)
`crates/network/src/http_cache.rs:195` **struct** `HttpCache`
`crates/network/src/http_cache.rs:202` **fn** `new` — Create an empty cache with LRU eviction and 50 MB limit
`crates/network/src/http_cache.rs:211` **fn** `len` — Number of entries currently stored
`crates/network/src/http_cache.rs:216` **fn** `is_empty`
`crates/network/src/http_cache.rs:350` **enum** `CacheLookup` — `CacheLookup` is unused externally; we use `get()` which returns `Option<CacheEntrySnapshot>`
`crates/network/src/http_cache.rs:360` **enum** `DiskCacheError` — Error type for [`DiskHttpCache`] operations
`crates/network/src/http_cache.rs:390` **struct** `DiskHttpCache` — SQLite-backed HTTP cache that survives browser restarts (RFC 7234 Phase 1)
`crates/network/src/http_cache.rs:399` **fn** `new` — Open or create a cache database at `path`
`crates/network/src/http_cache.rs:423` **fn** `open_default` — Open or create the default cache database at [`lumen_cache_dir`]`/http_cache.db`
`crates/network/src/http_cache.rs:567` **fn** `lumen_cache_dir` — Returns the Lumen cache directory for the current user
`crates/network/src/lib.rs:1583` **struct** `HttpProxy` — HTTP proxy configuration (RFC 7230 proxy behavior)
`crates/network/src/lib.rs:1595` **fn** `new` — Создать новый прокси без аутентификации
`crates/network/src/lib.rs:1604` **fn** `with_basic_auth` — Создать прокси с базовой аутентификацией (username:password)
`crates/network/src/lib.rs:1647` **struct** `HttpClient` — HTTP/1.1 + HTTPS клиент
`crates/network/src/lib.rs:1686` **fn** `new`
`crates/network/src/lib.rs:1712` **fn** `with_sink` — Подключить EventSink. По умолчанию sink-а нет (события не эмитятся)
`crates/network/src/lib.rs:1723` **fn** `with_filter` — Подключить RequestFilter. По умолчанию фильтра нет — `fetch` всегда
`crates/network/src/lib.rs:1735` **fn** `with_interceptor` — Подключить Service Worker перехватчик fetch-запросов. Проверяется
`crates/network/src/lib.rs:1744` **fn** `with_pool` — Подключить shared `ConnectionPool`. По умолчанию у каждого `HttpClient`
`crates/network/src/lib.rs:1754` **fn** `with_h2_pool` — Подключить shared `H2Pool` (RFC 9113 §9.1.1). По умолчанию HTTP/2
`crates/network/src/lib.rs:1763` **fn** `with_dns_resolver` — Подключить DNS-резолвер. По умолчанию — `SystemDnsResolver` (через
`crates/network/src/lib.rs:1780` **fn** `with_hsts` — Подключить HSTS-store (RFC 6797). По умолчанию — нет:
`crates/network/src/lib.rs:1796` **fn** `with_credentials` — Подключить credential-провайдер для HTTP authentication (RFC 7235 /
`crates/network/src/lib.rs:1807` **fn** `with_tab` — Указать `TabId`, который попадёт в каждое emit-ое событие. В Phase 0
`crates/network/src/lib.rs:1827` **fn** `with_mixed_content_policy` — Подключить mixed-content policy (W3C Mixed Content §5). По умолчанию
`crates/network/src/lib.rs:1851` **fn** `with_content_decoder` — Зарегистрировать `ContentDecoder` для одного encoding. Декодер попадает
`crates/network/src/lib.rs:1897` **fn** `with_cors_cache` — Запросить только диапазон байт ресурса (RFC 7233). Если сервер
`crates/network/src/lib.rs:1909` **fn** `with_cookie_jar` — Attach a cookie store. The provider receives `Cookie:` injection
`crates/network/src/lib.rs:1933` **fn** `with_http_cache` — Подключить HTTP response cache (RFC 7234)
`crates/network/src/lib.rs:1944` **fn** `with_proxy` — Подключить HTTP прокси (RFC 7230). По умолчанию прокси не подключён — запросы
`crates/network/src/lib.rs:1957` **fn** `with_socks5_proxy` — Подключить SOCKS5 прокси (RFC 1928) для туннелирования всех TCP-соединений
`crates/network/src/lib.rs:1968` **fn** `with_fingerprint_profile` — Установить HTTP fingerprinting profile (Standard/Strict/Tor) для Chrome-matching
`crates/network/src/lib.rs:1976` **fn** `fingerprint_profile` — Получить текущий HTTP fingerprinting profile
`crates/network/src/lib.rs:1987` **fn** `with_tls_profile` — Override the TLS fingerprint profile independently of the HTTP profile
`crates/network/src/lib.rs:1993` **fn** `tls_profile` — Получить текущий TLS fingerprinting profile
`crates/network/src/lib.rs:2027` **fn** `fetch_cors` — CORS-enabled fetch для cross-origin subresource (Fetch §3-§4)
`crates/network/src/lib.rs:2075` **fn** `fetch_range`
`crates/network/src/lib.rs:2142` **fn** `fetch_multi_range` — Multi-range запрос (RFC 7233 §4.1). Один request на несколько
`crates/network/src/lib.rs:2228` **fn** `fetch_subresource` — Загрузить подресурс с проверкой mixed-content по подключённой
`crates/network/src/lib.rs:2774` **struct** `InMemoryFetchInterceptor` — In-memory реализация `FetchInterceptor` для тестов без SQLite
`crates/network/src/lib.rs:2780` **fn** `new`
`crates/network/src/lib.rs:2787` **fn** `insert` — Добавить запись: ответ для (origin, url) берётся из кэша без сети
`crates/network/src/mixed_content.rs:33` **enum** `RequestDestination` — Назначение подресурса по Fetch spec §3.2.7 «request destination» —
`crates/network/src/mixed_content.rs:59` **enum** `MixedContentLevel` — Mixed-content уровень для запроса в secure-контексте
`crates/network/src/mixed_content.rs:75` **fn** `is_strict_blocked` — Должны ли мы блокировать запрос по строгому режиму. По умолчанию
`crates/network/src/mixed_content.rs:82` **fn** `is_spec_default_blocked` — Должны ли мы блокировать запрос по spec-default режиму
`crates/network/src/mixed_content.rs:110` **fn** `classify_subresource_request` — Классификация подресурса для secure top-level контекста
`crates/network/src/mixed_content.rs:146` **enum** `MixedContentMode` — Режим enforcement-а для mixed-content в `HttpClient`. Классификатор
`crates/network/src/mixed_content.rs:167` **struct** `MixedContentPolicy` — Связка top-level origin + режим, передаваемая в `HttpClient` через
`crates/network/src/mixed_content.rs:173` **fn** `new`
`crates/network/src/mixed_content.rs:177` **fn** `top_level`
`crates/network/src/mixed_content.rs:181` **fn** `mode`
`crates/network/src/mixed_content.rs:188` **fn** `evaluate` — Возвращает `Some(level)`, если запрос подресурса должен быть
`crates/network/src/mixed_content.rs:209` **fn** `block_reason` — Текстовая причина для `Event::RequestBlocked.reason` — стабильный формат
`crates/network/src/mock.rs:33` **struct** `MockTransport` — Mock HTTP транспорт — перехватывает запросы и возвращает fixture-данные
`crates/network/src/mock.rs:39` **fn** `new` — Создать пустой mock транспорт без зарегистрированных фиксатур
`crates/network/src/mock.rs:53` **fn** `add_fixture` — Зарегистрировать fixture-данные для URL
`crates/network/src/mock.rs:63` **fn** `fixture_count` — Получить текущее количество зарегистрированных фиксатур
`crates/network/src/origin.rs:28` **struct** `Origin` — «Tuple origin» = `(scheme, host, port)`. Сравнение — компонент-к-компоненту,
`crates/network/src/origin.rs:36` **enum** `OriginError` — Ошибки извлечения origin из URL
`crates/network/src/origin.rs:61` **fn** `from_url` — Извлечь tuple origin из `Url`. Возвращает `Err(OriginError::Opaque)`
`crates/network/src/origin.rs:90` **fn** `new` — Конструктор из готовых компонентов (для тестов и внутренних случаев,
`crates/network/src/origin.rs:98` **fn** `scheme`
`crates/network/src/origin.rs:102` **fn** `host`
`crates/network/src/origin.rs:106` **fn** `port`
`crates/network/src/origin.rs:117` **fn** `same_origin` — Same-origin сравнение по HTML LS §7.5 «same origin» для tuple-origin-ов:
`crates/network/src/origin.rs:130` **fn** `is_potentially_trustworthy` — «Potentially trustworthy origin» по W3C Secure Contexts §3.1:
`crates/network/src/origin.rs:145` **fn** `serialize` — Сериализация origin в каноническую форму для заголовков HTTP (`Origin:`,
`crates/network/src/permissions_policy.rs:14` **enum** `PermissionsAllowlist` — The allowlist for a single feature in a [`PermissionsPolicy`]
`crates/network/src/permissions_policy.rs:28` **struct** `PermissionsPolicy` — Parsed representation of a `Permissions-Policy` (or `Feature-Policy`) header
`crates/network/src/permissions_policy.rs:38` **fn** `allows_feature` — Returns `true` if `feature` is allowed for the given `origin`
`crates/network/src/permissions_policy.rs:51` **fn** `features` — Returns all feature names listed in this policy
`crates/network/src/permissions_policy.rs:56` **fn** `allowed_features` — Returns feature names for which the current document origin (`"self"`) is allowed
`crates/network/src/permissions_policy.rs:76` **fn** `parse_permissions_policy_header` — Parse the value of a `Permissions-Policy` header
`crates/network/src/permissions_policy.rs:96` **fn** `parse_feature_policy_header` — Parse the legacy `Feature-Policy` header (space-separated, semicolon-delimited)
`crates/network/src/pool.rs:60` **struct** `ConnectionPool` — Потокобезопасный пул keep-alive соединений. По умолчанию пуст; заполняется
`crates/network/src/pool.rs:65` **fn** `new`
`crates/network/src/pool.rs:109` **fn** `idle_count` — Сколько idle-соединений сейчас в пуле для данного origin-а. Удобно
`crates/network/src/range.rs:32` **enum** `RangeSpec` — Спецификация запрашиваемого диапазона байт (inclusive по обоим концам
`crates/network/src/range.rs:49` **fn** `closed` — Закрытый диапазон `[start; end]` inclusive по обоим концам
`crates/network/src/range.rs:54` **fn** `from` — Открытый диапазон от `start` до конца ресурса
`crates/network/src/range.rs:61` **fn** `suffix` — Suffix-range: последние `length` байт ресурса. RFC 7233 §2.1
`crates/network/src/range.rs:86` **enum** `RangeRequest` — Запрос range-байт, single- или multi-. `Multi(vec)` сериализуется в
`crates/network/src/range.rs:133` **enum** `RangeValidator` — Validator для `If-Range` header (RFC 7233 §3.2). Либо ETag (`"abc"`,
`crates/network/src/range.rs:158` **struct** `ContentRange` — Разобранный `Content-Range: bytes START-END/TOTAL` (RFC 7233 §4.2)
`crates/network/src/range.rs:168` **fn** `parse_content_range` — Парсер `Content-Range: bytes START-END/TOTAL`. Поддерживает обе формы
`crates/network/src/range.rs:189` **struct** `RangeResponse` — Ответ на range-запрос. `status = 206` — Range honored (Content-Range
`crates/network/src/range.rs:199` **struct** `RangePart` — Один part в multipart/byteranges-ответе (или единственный part в случае
`crates/network/src/range.rs:209` **struct** `MultiRangeResponse` — Ответ на multi-range запрос. Caller получает единый список parts,
`crates/network/src/range.rs:223` **fn** `parse_boundary_from_content_type` — Извлечь boundary-токен из значения `Content-Type` (RFC 7231 §3.1.1.1 +
`crates/network/src/range.rs:265` **fn** `parse_multipart_byteranges` — Парсер multipart/byteranges body (RFC 7233 §A + RFC 2046 §5.1.1)
`crates/network/src/socks5.rs:22` **struct** `Socks5Proxy` — SOCKS5 proxy server address and optional credentials
`crates/network/src/socks5.rs:33` **fn** `new` — Create a new SOCKS5 proxy without authentication
`crates/network/src/socks5.rs:42` **fn** `with_auth` — Attach username / password credentials (RFC 1929)
`crates/network/src/socks5.rs:56` **fn** `socks5_connect` — Perform a SOCKS5 handshake on `stream` and request a `CONNECT` to
`crates/network/src/sse.rs:36` **struct** `SseParser` — Incremental `text/event-stream` parser
`crates/network/src/sse.rs:47` **fn** `new`
`crates/network/src/sse.rs:53` **fn** `push_bytes` — Feed a chunk of bytes from the stream; returns any events that
`crates/network/src/sse.rs:175` **fn** `last_event_id` — Current last-event-id (persists across dispatched events, needed for
`crates/network/src/tls/fingerprint.rs:116` **struct** `CertInfo` — X.509 certificate information extracted after a TLS handshake
`crates/network/src/tls/fingerprint.rs:140` **fn** `is_populated` — Return `true` when the cert info was populated (subject_cn is non-empty)
`crates/network/src/tls/fingerprint.rs:147` **fn** `stub_for` — Build a stub `CertInfo` for a given hostname (Phase 0 placeholder)
`crates/network/src/tls/fingerprint.rs:170` **struct** `TlsHandshakeInfo` — TLS handshake parameters extracted from a ClientHello for fingerprinting
`crates/network/src/tls/fingerprint.rs:208` **fn** `ja3_raw_string` — JA3 raw string (pre-MD5 input)
`crates/network/src/tls/fingerprint.rs:240` **fn** `ja4_raw_string` — JA4_r (raw JA4) string — human-readable without SHA256 hashing
`crates/network/src/tls/fingerprint.rs:328` **fn** `is_grease` — Returns `true` if `v` is a GREASE value (RFC 8701)
`crates/network/src/tls/fingerprint.rs:340` **struct** `ChromeJa3Snapshot` — Reference Chrome 130 TLS ClientHello parameters for JA3 snapshot testing
`crates/network/src/tls/fingerprint.rs:404` **struct** `JA4ChromeSnapshot` — Reference Chrome 130 JA4_r parameters for snapshot testing
`crates/network/src/tls/mod.rs:30` **enum** `TlsProfile` — TLS fingerprint profile — controls cipher suites, kx_groups, ALPN, and
`crates/network/src/tls/mod.rs:47` **fn** `http_to_tls_profile` — Map an `HttpProfile` to the corresponding `TlsProfile`
`crates/network/src/tls/mod.rs:64` **fn** `build_client_config` — Build a `ClientConfig` for the given `TlsProfile`
`crates/network/src/webauthn.rs:62` **struct** `VirtualAuthenticator` — In-memory software authenticator: generates and stores ES256 passkeys and
`crates/network/src/webauthn.rs:69` **fn** `new` — Create an empty authenticator with no registered credentials
`crates/network/src/webauthn.rs:74` **fn** `credential_count` — Number of credentials currently registered (test / introspection helper)

## lumen-paint  (277 symbols)

`crates/engine/paint/src/atlas.rs:35` **struct** `AtlasKey` — Композитный ключ glyph-кэша. См. module-level docs
`crates/engine/paint/src/atlas.rs:43` **fn** `new`
`crates/engine/paint/src/atlas.rs:53` **fn** `hash_coords` — Стабильный 64-битный хэш normalized variation coords для cache key
`crates/engine/paint/src/atlas.rs:67` **struct** `GlyphEntry`
`crates/engine/paint/src/atlas.rs:78` **struct** `GlyphAtlas`
`crates/engine/paint/src/atlas.rs:97` **fn** `new`
`crates/engine/paint/src/atlas.rs:112` **fn** `width`
`crates/engine/paint/src/atlas.rs:115` **fn** `height`
`crates/engine/paint/src/atlas.rs:118` **fn** `pixels`
`crates/engine/paint/src/atlas.rs:122` **fn** `dirty`
`crates/engine/paint/src/atlas.rs:125` **fn** `mark_clean`
`crates/engine/paint/src/atlas.rs:129` **fn** `get`
`crates/engine/paint/src/atlas.rs:134` **fn** `access` — Обновляет timestamp доступа для существующей записи
`crates/engine/paint/src/atlas.rs:144` **fn** `get_lru_candidates` — Возвращает список ключей отсортированных по last_accessed (от самого старого к новому)
`crates/engine/paint/src/atlas.rs:154` **fn** `remove_keys` — Удаляет записи с указанными ключами из кэша
`crates/engine/paint/src/atlas.rs:168` **fn** `insert` — Кладёт растеризованный глиф в атлас. Возвращает `None` если место
`crates/engine/paint/src/atlas.rs:232` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting glyphs from the cache
`crates/engine/paint/src/backdrop_cache.rs:49` **struct** `BackdropCache` — Tracks freshness of cached `backdrop-filter` textures
`crates/engine/paint/src/backdrop_cache.rs:64` **fn** `new` — Creates an enabled cache with [`DEFAULT_BUDGET_BYTES`]
`crates/engine/paint/src/backdrop_cache.rs:70` **fn** `with_budget` — Creates an enabled cache with a custom GPU memory budget (bytes)
`crates/engine/paint/src/backdrop_cache.rs:82` **fn** `set_enabled` — Enables or disables the cache. Disabling clears all entries so the
`crates/engine/paint/src/backdrop_cache.rs:91` **fn** `is_enabled` — Whether the cache is currently active
`crates/engine/paint/src/backdrop_cache.rs:101` **fn** `lookup` — Returns `true` (cache HIT) if an entry for `ordinal` exists with a
`crates/engine/paint/src/backdrop_cache.rs:122` **fn** `store` — Records that `ordinal` now holds freshly produced content for
`crates/engine/paint/src/backdrop_cache.rs:142` **fn** `invalidate` — Drops the metadata entry for `ordinal`, if any. Returns `true` if an
`crates/engine/paint/src/backdrop_cache.rs:152` **fn** `clear` — Removes all entries. The renderer drops every backing texture in lockstep
`crates/engine/paint/src/backdrop_cache.rs:163` **fn** `on_memory_pressure` — Responds to a memory-pressure signal. Returns the ordinals whose textures
`crates/engine/paint/src/backdrop_cache.rs:178` **fn** `len` — Number of live cache entries
`crates/engine/paint/src/backdrop_cache.rs:184` **fn** `is_empty` — Whether the cache holds no entries
`crates/engine/paint/src/backdrop_cache.rs:190` **fn** `used_bytes` — Total GPU memory tracked by live entries, in bytes
`crates/engine/paint/src/backdrop_cache.rs:196` **fn** `budget_bytes` — Configured eviction budget, in bytes
`crates/engine/paint/src/backend.rs:38` **enum** `RenderError` — Ошибка рендера — возвращается из [`RenderBackend::render`]
`crates/engine/paint/src/backend.rs:78` **trait** `RenderBackend` — Стабильный интерфейс GPU-рендера для Lumen
`crates/engine/paint/src/backends/compare_backend.rs:35` **struct** `DiffResult` — Результат pixel-diff сравнения двух бэкендов
`crates/engine/paint/src/backends/compare_backend.rs:53` **fn** `diff_percent` — Доля отличающихся пикселей в процентах (0.0 – 100.0)
`crates/engine/paint/src/backends/compare_backend.rs:61` **fn** `is_identical` — `true` если бэкенды дали побитово идентичные результаты
`crates/engine/paint/src/backends/compare_backend.rs:68` **fn** `format` — Форматирует результат в строку для логов
`crates/engine/paint/src/backends/compare_backend.rs:80` **fn** `compute` — Вычисляет DiffResult из двух RGBA8-буферов одинакового размера
`crates/engine/paint/src/backends/compare_backend.rs:129` **struct** `CompareBackend` — Тестовый бэкенд: рендерит двумя бэкендами + вычисляет pixel-diff
`crates/engine/paint/src/backends/compare_backend.rs:145` **fn** `new` — Создаёт CompareBackend из двух headless-бэкендов
`crates/engine/paint/src/backends/compare_backend.rs:153` **fn** `last_diff` — Возвращает результат pixel-diff последнего render-а
`crates/engine/paint/src/backends/compare_backend.rs:158` **fn** `primary` — Предоставляет read-only доступ к первичному бэкенду
`crates/engine/paint/src/backends/compare_backend.rs:163` **fn** `secondary` — Предоставляет read-only доступ к вторичному бэкенду
`crates/engine/paint/src/backends/cpu_backend.rs:31` **struct** `CpuBackend` — Headless CPU-бэкенд на tiny-skia: детерминированный рендер без GPU
`crates/engine/paint/src/backends/cpu_backend.rs:44` **fn** `new` — Создаёт headless CPU-бэкенд с заданным размером поверхности
`crates/engine/paint/src/backends/cpu_backend.rs:49` **fn** `last_image` — Возвращает Image из последнего рендера, если он был выполнен
`crates/engine/paint/src/backends/femtovg_backend.rs:245` **struct** `FemtovgBackend` — femtovg/OpenGL рендер-бэкенд (Phase 2, ADR-010)
`crates/engine/paint/src/backends/femtovg_backend.rs:346` **fn** `new` — Создаёт оконный femtovg-бэкенд из winit-окна
`crates/engine/paint/src/backends/vello_backend.rs:43` **struct** `VelloBackend` — Phase 3 рендер-бэкенд на базе Vello (ADR-010, RB-7 заглушка)
`crates/engine/paint/src/backends/vello_backend.rs:57` **fn** `new` — Создаёт заглушку `VelloBackend` с начальным размером поверхности
`crates/engine/paint/src/backends/wgpu_backend.rs:51` **struct** `WgpuBackend` — wgpu-бэкенд: тонкая обёртка над [`Renderer`], реализующая [`RenderBackend`]
`crates/engine/paint/src/backends/wgpu_backend.rs:62` **fn** `new` — Создаёт оконный бэкенд из winit-окна
`crates/engine/paint/src/backends/wgpu_backend.rs:73` **fn** `new_headless` — Создаёт headless-бэкенд для тестов и `--print-to-pdf`
`crates/engine/paint/src/backends/wgpu_backend.rs:85` **fn** `renderer` — Неизменяемый доступ к внутреннему [`Renderer`]
`crates/engine/paint/src/backends/wgpu_backend.rs:90` **fn** `renderer_mut` — Изменяемый доступ к внутреннему [`Renderer`]
`crates/engine/paint/src/compositor.rs:63` **trait** `Layer` — Один layer: bbox + связь со stacking context-ом + локальный display list
`crates/engine/paint/src/compositor.rs:71` **trait** `LayerTree` — Коллекция layer-ов. Trait-обстракция, чтобы compositor мог принимать
`crates/engine/paint/src/compositor.rs:79` **struct** `BasicLayer` — Sprint 0 / Phase 0 concrete impl. Owned struct без интерлевания —
`crates/engine/paint/src/compositor.rs:100` **struct** `BasicLayerTree` — Sprint 0 / Phase 0 concrete impl. Один display-list = один layer
`crates/engine/paint/src/compositor.rs:108` **fn** `empty` — Пустой tree (нет ни одного layer-а). Полезен как начальное состояние
`crates/engine/paint/src/compositor.rs:117` **fn** `single_layer` — Phase 0: оборачивает весь display-list в один layer на bbox-страницы
`crates/engine/paint/src/compositor.rs:154` **trait** `Compositor` — Compositor: получает обновления сцены через `commit`, отдаёт активную
`crates/engine/paint/src/compositor.rs:187` **struct** `InProcessCompositor` — Single-thread in-process compositor: синхронный swap, без Mutex
`crates/engine/paint/src/compositor.rs:196` **fn** `new`
`crates/engine/paint/src/compositor.rs:331` **struct** `ThreadedCompositor` — Thread-safe compositor: тот же API two-buffer-а, но `commit` и
`crates/engine/paint/src/compositor.rs:338` **fn** `new`
`crates/engine/paint/src/compositor.rs:349` **fn** `handle` — Cheap-clone handle для другого потока: shared доступ к тому же
`crates/engine/paint/src/compositor.rs:434` **struct** `ThreadedCompositorHandle` — Cheap-clone handle на тот же state, что и parent [`ThreadedCompositor`]
`crates/engine/paint/src/compositor.rs:440` **fn** `commit`
`crates/engine/paint/src/compositor.rs:456` **fn** `flush_pending`
`crates/engine/paint/src/compositor.rs:474` **fn** `has_pending`
`crates/engine/paint/src/compositor.rs:483` **fn** `active_tree`
`crates/engine/paint/src/compositor.rs:492` **fn** `active_trees`
`crates/engine/paint/src/compositor.rs:526` **struct** `CompositorThread` — Реальный compositor thread: отдельный OS-поток с vsync tick-loop
`crates/engine/paint/src/compositor.rs:535` **fn** `spawn` — Запускает compositor thread. `handle` — разделяемый доступ к state
`crates/engine/paint/src/compositor.rs:550` **fn** `shutdown` — Запрашивает завершение потока и блокируется до его выхода
`crates/engine/paint/src/display_list.rs:37` **enum** `FilterMode` — CSS Images L3 §4.3 — image-rendering filter mode (scaling algorithm)
`crates/engine/paint/src/display_list.rs:50` **fn** `from_image_rendering` — Преобразует `ImageRendering` в `FilterMode`
`crates/engine/paint/src/display_list.rs:66` **enum** `BlendMode` — CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
`crates/engine/paint/src/display_list.rs:94` **fn** `from_keyword` — Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
`crates/engine/paint/src/display_list.rs:131` **enum** `MaskMode` — CSS Masking L1 §6 — how to derive the mask value from rendered mask-layer pixels
`crates/engine/paint/src/display_list.rs:145` **struct** `CornerRadii` — Corner radii for CSS `border-radius`. Values are in CSS pixels, clamped to ≥ 0
`crates/engine/paint/src/display_list.rs:167` **fn** `all_zero` — Returns `true` if all eight radii are zero (no rounding needed)
`crates/engine/paint/src/display_list.rs:183` **fn** `from_style_and_box` — Builds `CornerRadii` from a `ComputedStyle` and the element's border-box dimensions
`crates/engine/paint/src/display_list.rs:199` **fn** `from_style` — Builds `CornerRadii` from a `ComputedStyle`. `border-radius: N%` values are
`crates/engine/paint/src/display_list.rs:205` **enum** `DisplayCommand`
`crates/engine/paint/src/display_list.rs:672` **type** `DisplayList`
`crates/engine/paint/src/display_list.rs:701` **fn** `fit_image_rect` — CSS Images L3 §5.5 — `object-fit` placement: где располагается
`crates/engine/paint/src/display_list.rs:756` **fn** `fit_image_quad` — Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
`crates/engine/paint/src/display_list.rs:829` **fn** `cull_display_list` — Returns `true` if the display list contains any `backdrop-filter` element
`crates/engine/paint/src/display_list.rs:860` **fn** `contains_backdrop_filter` — Cheap pre-check the renderer uses to decide whether computing a frame
`crates/engine/paint/src/display_list.rs:896` **fn** `hash_display_list` — Computes a content hash over a frame's display list plus the viewport state
`crates/engine/paint/src/display_list.rs:924` **struct** `DiffResult` — Результат сравнения двух display-list-ов
`crates/engine/paint/src/display_list.rs:936` **fn** `identical` — Создаёт DiffResult для идентичных display list-ов
`crates/engine/paint/src/display_list.rs:950` **fn** `changed` — Создаёт DiffResult для изменённых display list-ов с заданным bounding rect
`crates/engine/paint/src/display_list.rs:966` **fn** `diff_display_lists` — Сравнивает два display list-а по Debug hash каждой команды
`crates/engine/paint/src/display_list.rs:1083` **fn** `serialize_display_list`
`crates/engine/paint/src/display_list.rs:1440` **fn** `build_display_list`
`crates/engine/paint/src/display_list.rs:1455` **fn** `build_display_list_with_anim` — Like `build_display_list` but applies compositor animation overrides per node
`crates/engine/paint/src/display_list.rs:1495` **fn** `build_display_list_ordered` — Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E)
`crates/engine/paint/src/display_list.rs:1506` **fn** `build_display_list_ordered_dpr` — Like [`build_display_list_ordered`] but resolves `image-set()` background
`crates/engine/paint/src/display_list.rs:1548` **fn** `build_display_list_ordered_with_anim` — Like [`build_display_list_ordered`] but applies compositor animation overrides per node
`crates/engine/paint/src/display_list.rs:1559` **fn** `build_display_list_ordered_with_anim_dpr` — Like [`build_display_list_ordered_with_anim`] but resolves `image-set()`
`crates/engine/paint/src/display_list.rs:1606` **fn** `build_print_display_list` — Builds a print display list from paginated layout
`crates/engine/paint/src/display_list.rs:1667` **fn** `split_at_page_breaks` — Splits a print display list at `PageBreak` markers
`crates/engine/paint/src/display_list.rs:2406` **fn** `is_image_set` — CSS Images L4 §5 — is `value` an `image-set()` / `-webkit-image-set()` expression?
`crates/engine/paint/src/display_list.rs:2535` **fn** `select_image_set_url` — CSS Images L4 §5 — selects the best `image-set()` candidate URL for `dpr`
`crates/engine/paint/src/display_list.rs:3090` **fn** `point_on_resize_grip` — Возвращает `true`, если точка (`px`, `py`) попадает в resize-grip элемента
`crates/engine/paint/src/fingerprint.rs:21` **struct** `GpuFingerprint` — GPU fingerprint info: normailzed vendor and renderer strings
`crates/engine/paint/src/fingerprint.rs:36` **fn** `from_adapter_info` — Create normalized GPU fingerprint from wgpu adapter info
`crates/engine/paint/src/fingerprint.rs:44` **fn** `vendor` — Vendor string: always "WebKit"
`crates/engine/paint/src/fingerprint.rs:49` **fn** `renderer` — Renderer string: always "Generic GPU"
`crates/engine/paint/src/gap_decorations.rs:18` **struct** `GapDecorationContext` — Parameters for gap rule rendering
`crates/engine/paint/src/gap_decorations.rs:31` **struct** `GapSegment` — One inter-cell gap in a flex, grid, or multicol layout
`crates/engine/paint/src/gap_decorations.rs:58` **fn** `emit_gap_rules` — Emits [`DisplayCommand::DrawBorder`] entries for gap decorations between
`crates/engine/paint/src/glsl.rs:32` **enum** `Val` — Runtime value inside the GLSL interpreter
`crates/engine/paint/src/glsl.rs:49` **fn** `to_float` — Convert any numeric-ish value to a scalar f32
`crates/engine/paint/src/glsl.rs:63` **fn** `to_vec4` — Convert any value to vec4 (broadcasting rules)
`crates/engine/paint/src/glsl.rs:75` **fn** `components` — Number of scalar components
`crates/engine/paint/src/glsl.rs:86` **fn** `get_component` — Read a single float component by index (0-based)
`crates/engine/paint/src/glsl.rs:323` **enum** `GlType` — GLSL type tag (declaration-time)
`crates/engine/paint/src/glsl.rs:394` **struct** `ParsedShader` — A parsed GLSL shader: declaration tables + the `main()` function body
`crates/engine/paint/src/glsl.rs:911` **fn** `parse` — Parse a GLSL ES shader source string
`crates/engine/paint/src/glsl.rs:920` **struct** `ShaderEnv` — Execution environment for a single shader invocation
`crates/engine/paint/src/glsl.rs:938` **fn** `new`
`crates/engine/paint/src/glsl.rs:977` **fn** `exec_main` — Execute the `main()` function of a parsed shader
`crates/engine/paint/src/glsl.rs:1546` **fn** `interp_varyings` — Linearly interpolate a map of varying values given barycentric weights
`crates/engine/paint/src/hit_test.rs:48` **struct** `HitTestResult` — Результат hit-теста
`crates/engine/paint/src/hit_test.rs:77` **fn** `hit_test` — Hit-тест точки в viewport-координатах. `root` — layout-дерево из
`crates/engine/paint/src/layer_cache.rs:21` **struct** `LayerKey` — Layer identification key for cache lookup
`crates/engine/paint/src/layer_cache.rs:31` **fn** `new` — Create a new layer cache key
`crates/engine/paint/src/layer_cache.rs:38` **struct** `LayerEntry` — Metadata for a cached GPU layer texture
`crates/engine/paint/src/layer_cache.rs:54` **struct** `LayerCache` — Layer cache managing GPU memory via LRU eviction
`crates/engine/paint/src/layer_cache.rs:72` **fn** `new` — Create a new layer cache with default 256 MB GPU memory budget
`crates/engine/paint/src/layer_cache.rs:83` **fn** `with_budget` — Create with custom GPU memory budget (in bytes)
`crates/engine/paint/src/layer_cache.rs:94` **fn** `used_bytes` — Get the current GPU memory usage
`crates/engine/paint/src/layer_cache.rs:99` **fn** `budget_bytes` — Get the GPU memory budget
`crates/engine/paint/src/layer_cache.rs:104` **fn** `would_exceed_budget` — Check if adding a layer of given size would exceed budget
`crates/engine/paint/src/layer_cache.rs:111` **fn** `insert` — Insert or update a cached layer
`crates/engine/paint/src/layer_cache.rs:134` **fn** `access` — Mark a cached layer as accessed (used by current render)
`crates/engine/paint/src/layer_cache.rs:144` **fn** `get_lru_candidates` — Get candidates for LRU eviction, sorted from least- to most-recently-used
`crates/engine/paint/src/layer_cache.rs:153` **fn** `remove_keys` — Remove cached layers by key, freeing GPU memory
`crates/engine/paint/src/layer_cache.rs:169` **fn** `clear` — Clear all cached entries (full eviction), including promoted layer registrations
`crates/engine/paint/src/layer_cache.rs:176` **fn** `len` — Get the number of cached layers
`crates/engine/paint/src/layer_cache.rs:181` **fn** `is_empty` — Check if cache is empty
`crates/engine/paint/src/layer_cache.rs:186` **fn** `contains` — Check if a specific layer is in cache
`crates/engine/paint/src/layer_cache.rs:196` **fn** `promote_layer` — Promote a node to its own GPU layer (for `will-change: transform/opacity/filter`)
`crates/engine/paint/src/layer_cache.rs:204` **fn** `is_layer_promoted` — Returns `true` if the given node has a promoted GPU layer
`crates/engine/paint/src/layer_cache.rs:209` **fn** `demote_layer` — Remove the promoted GPU layer for a node, freeing its cache entry
`crates/engine/paint/src/layer_cache.rs:218` **fn** `sync_promoted_layers` — Remove promoted layers for nodes NOT in `current_nodes`
`crates/engine/paint/src/layer_cache.rs:231` **fn** `promoted_count` — Number of nodes currently promoted to their own GPU layer
`crates/engine/paint/src/layer_cache.rs:240` **fn** `on_memory_pressure` — React to an OS memory pressure event by evicting GPU layer textures
`crates/engine/paint/src/lib.rs:89` **struct** `FontMeasurer` — Реализация [`TextMeasurer`] на основе TTF-данных шрифта
`crates/engine/paint/src/lib.rs:99` **fn** `new` — Создаёт измеритель из уже разобранного [`lumen_font::Font`]
`crates/engine/paint/src/lib.rs:190` **struct** `MultiFontMeasurer` — Многошрифтовый измеритель: поддерживает @font-face-загруженные шрифты
`crates/engine/paint/src/lib.rs:199` **fn** `new` — Создаёт измеритель с bundled-шрифтом как fallback
`crates/engine/paint/src/lib.rs:211` **fn** `register_family` — Регистрирует @font-face шрифт под именем `family`
`crates/engine/paint/src/lib.rs:218` **fn** `family_count` — Количество зарегистрированных семей (для тестов)
`crates/engine/paint/src/lib.rs:233` **fn** `resolve_font_stretch` — Resolves `font-stretch` percentage for the first matching family
`crates/engine/paint/src/renderer.rs:1271` **struct** `OffscreenLayer` — GPU-ресурсы одного off-screen opacity layer-а. Создаётся лениво через
`crates/engine/paint/src/renderer.rs:1302` **enum** `SnapshotUploadError` — Ошибка `Renderer::upload_layer_snapshot`
`crates/engine/paint/src/renderer.rs:1331` **enum** `ImageRegisterError` — Ошибка `Renderer::register_image`
`crates/engine/paint/src/renderer.rs:1395` **struct** `Renderer`
`crates/engine/paint/src/renderer.rs:1558` **fn** `new`
`crates/engine/paint/src/renderer.rs:1651` **fn** `new_headless` — Creates a headless `Renderer` for off-screen rendering without a winit window
`crates/engine/paint/src/renderer.rs:3012` **fn** `with_font_provider` — Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
`crates/engine/paint/src/renderer.rs:3020` **fn** `set_font_provider` — Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
`crates/engine/paint/src/renderer.rs:3033` **fn** `preload_fallback_chain` — Эагерно загружает указанные family-имена через текущий `FontProvider`,
`crates/engine/paint/src/renderer.rs:3047` **fn** `gpu_fingerprint` — Returns the normalized GPU fingerprint (vendor/renderer strings)
`crates/engine/paint/src/renderer.rs:3060` **fn** `preload_curated_fallbacks` — Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
`crates/engine/paint/src/renderer.rs:3141` **fn** `register_image` — Регистрирует декодированное изображение в GPU-cache под ключом `src`
`crates/engine/paint/src/renderer.rs:3287` **fn** `unregister_image` — Снимает регистрацию изображения. После этого `DrawImage` для `src`
`crates/engine/paint/src/renderer.rs:3296` **fn** `clear_images` — Снимает регистрацию всех картинок (например, при переходе на новую
`crates/engine/paint/src/renderer.rs:3303` **fn** `has_image` — Зарегистрирована ли картинка с таким `src` (для shell-логирования)
`crates/engine/paint/src/renderer.rs:3321` **fn** `upload_layer_snapshot` — Загружает CPU-пиксели (`Rgba8`, 4 байта/пиксель) как именованный
`crates/engine/paint/src/renderer.rs:3388` **fn** `evict_layer_snapshot` — Удаляет снимок с `id`. GPU-память освобождается при drop-е
`crates/engine/paint/src/renderer.rs:3393` **fn** `clear_layer_snapshots` — Удаляет все снимки (например, при переходе на новую страницу)
`crates/engine/paint/src/renderer.rs:3399` **fn** `has_layer_snapshot` — Зарегистрирован ли снимок с таким `id`
`crates/engine/paint/src/renderer.rs:3404` **fn** `layer_cache` — Получить ссылку на layer cache для статистики / монитора GPU памяти
`crates/engine/paint/src/renderer.rs:3412` **fn** `set_backdrop_cache_enabled` — Enables or disables the `backdrop-filter` result cache (CSS Filter
`crates/engine/paint/src/renderer.rs:3421` **fn** `clear_backdrop_cache` — Drops every cached `backdrop-filter` texture and its metadata. The next
`crates/engine/paint/src/renderer.rs:3428` **fn** `backdrop_cache_len` — Number of live cached `backdrop-filter` textures (for stats / tests)
`crates/engine/paint/src/renderer.rs:3435` **fn** `backdrop_cache_on_memory_pressure` — Forwards a memory-pressure signal to the `backdrop-filter` cache and
`crates/engine/paint/src/renderer.rs:3447` **fn** `atlas_on_memory_pressure` — Forwards a memory-pressure signal to the glyph atlas so it can evict
`crates/engine/paint/src/renderer.rs:3452` **fn** `layer_cache_mut` — Получить мutable ссылку для прямого управления кэшем (advanced usage)
`crates/engine/paint/src/renderer.rs:3458` **fn** `access_layer` — Отметить layer как используемый текущим render pass
`crates/engine/paint/src/renderer.rs:3465` **fn** `cache_layer` — Кэшировать layer слой. Returns `true` if this is a new layer, `false` if updated
`crates/engine/paint/src/renderer.rs:3471` **fn** `return_layer_to_pool` — Return an off-screen layer texture to the pool for recycling (Phase 2 ADR-008)
`crates/engine/paint/src/renderer.rs:3487` **fn** `promote_layer` — Promote a node to its own GPU layer for `will-change: transform/opacity/filter`
`crates/engine/paint/src/renderer.rs:3497` **fn** `is_layer_promoted` — Returns `true` if the given node has a promoted GPU layer
`crates/engine/paint/src/renderer.rs:3502` **fn** `demote_layer` — Remove the promoted GPU layer for a node, freeing its cache entry
`crates/engine/paint/src/renderer.rs:3507` **fn** `clear_layer_cache` — Очистить весь layer cache (полная эвикция) и очистить texture pool
`crates/engine/paint/src/renderer.rs:3513` **fn** `texture_pool_len` — Get the number of free textures in the pool (for diagnostics)
`crates/engine/paint/src/renderer.rs:3518` **fn** `texture_pool_len_for_size` — Get the number of free textures of a specific size (for diagnostics)
`crates/engine/paint/src/renderer.rs:3523` **fn** `clear_texture_pool` — Clear all pooled textures (e.g., when resizing or memory pressure is high)
`crates/engine/paint/src/renderer.rs:3529` **fn** `snapshot_dimensions` — Возвращает `(width, height)` снимка, или `None` если `id` не зарегистрирован
`crates/engine/paint/src/renderer.rs:3535` **fn** `resize` — Resizes the render target. For windowed mode, reconfigures the wgpu surface
`crates/engine/paint/src/renderer.rs:3564` **fn** `set_scale_factor` — Обновить device-pixel-ratio. Вызывается shell-ом по `WindowEvent::ScaleFactorChanged`
`crates/engine/paint/src/renderer.rs:3573` **fn** `scale_factor` — Текущий device-pixel-ratio. Для отладки / тестов (UI обычно его не читает —
`crates/engine/paint/src/renderer.rs:3580` **fn** `viewport_size` — Текущий viewport в **logical** (CSS) пикселях: `physical / scale_factor`
`crates/engine/paint/src/renderer.rs:3765` **fn** `render` — Рендерит две полосы display list-а одним кадром:
`crates/engine/paint/src/renderer.rs:6367` **fn** `render_to_image_cpu` — CPU-based rasterization using tiny-skia (feature="cpu-render" only)
`crates/engine/paint/src/renderer.rs:6393` **fn** `render_tile`
`crates/engine/paint/src/renderer.rs:6432` **fn** `render_to_image` — Renders display commands and returns a CPU `Image` (RGBA8)
`crates/engine/paint/src/renderer.rs:6535` **fn** `render_print_pages` — Renders a print display list into one `Image` per page
`crates/engine/paint/src/scroll_snap.rs:33` **fn** `find_scroll_snap_y` — CSS Scroll Snap L1 — returns the Y scroll offset to snap to, or `None`
`crates/engine/paint/src/scroll_snap.rs:54` **fn** `find_scroll_snap_y_proximity` — CSS Scroll Snap L1 — same as [`find_scroll_snap_y`] but restricts candidates
`crates/engine/paint/src/svg_path.rs:16` **enum** `PathSegment` — One SVG path command (absolute coords, after normalization)
`crates/engine/paint/src/svg_path.rs:36` **fn** `parse_svg_path` — Parses SVG path `d` attribute into absolute-coordinate segments
`crates/engine/paint/src/svg_path.rs:308` **fn** `flatten_path` — Flatten path segments to a list of closed contours
`crates/engine/paint/src/svg_path.rs:552` **fn** `tessellate_polygon` — Tessellate a single closed polygon (no holes) using ear-clipping
`crates/engine/paint/src/svg_path.rs:586` **fn** `tessellate_fill` — Tessellate a path (all contours) into triangles. Multi-contour paths are
`crates/engine/paint/src/svg_path.rs:683` **fn** `tessellate_stroke` — Tessellate stroke outlines for all contours into a flat triangle vertex list
`crates/engine/paint/src/svg_path.rs:787` **enum** `StrokeLinecap` — Stroke caps applied at open sub-path endpoints
`crates/engine/paint/src/svg_path.rs:799` **enum** `StrokeLinejoin` — Join style at connected segment vertices
`crates/engine/paint/src/svg_path.rs:811` **struct** `StrokeParams` — Parameters for advanced stroke tessellation
`crates/engine/paint/src/svg_path.rs:844` **fn** `apply_dash_pattern` — Apply a dash pattern to a list of contours
`crates/engine/paint/src/svg_path.rs:943` **fn** `tessellate_stroke_ex` — Tessellate strokes with full linecap / linejoin / miterlimit / dasharray support
`crates/engine/paint/src/texture_pool.rs:15` **struct** `TextureKey` — Key for a pool entry: texture dimensions
`crates/engine/paint/src/texture_pool.rs:24` **fn** `new` — Create a new texture pool key
`crates/engine/paint/src/texture_pool.rs:34` **struct** `PooledTexture` — A pooled GPU texture resource
`crates/engine/paint/src/texture_pool.rs:53` **struct** `TexturePool` — Texture pool managing free textures for recycling
`crates/engine/paint/src/texture_pool.rs:63` **fn** `new` — Create a new empty texture pool
`crates/engine/paint/src/texture_pool.rs:73` **fn** `acquire` — Try to allocate a texture of the given size from the pool
`crates/engine/paint/src/texture_pool.rs:82` **fn** `release` — Return a texture to the pool for reuse
`crates/engine/paint/src/texture_pool.rs:88` **fn** `clear` — Clear all pooled textures, freeing GPU memory
`crates/engine/paint/src/texture_pool.rs:94` **fn** `len` — Get the number of free textures in the pool (across all sizes)
`crates/engine/paint/src/texture_pool.rs:99` **fn** `is_empty` — Check if the pool is empty
`crates/engine/paint/src/texture_pool.rs:104` **fn** `len_for_size` — Get the number of free textures of a specific size
`crates/engine/paint/src/texture_pool.rs:110` **fn** `pool_size` — Get total tracked pool size (for diagnostics)
`crates/engine/paint/src/texture_pool.rs:115` **fn** `update_size` — Update internal pool size counter (call after creating or destroying a texture)
`crates/engine/paint/src/tile_grid.rs:19` **enum** `TileDirty` — Dirty state of a single tile
`crates/engine/paint/src/tile_grid.rs:31` **struct** `TileGrid` — Tile-grid for dirty-rect tracking
`crates/engine/paint/src/tile_grid.rs:40` **fn** `new` — Create a new grid with all tiles missing (implicitly dirty)
`crates/engine/paint/src/tile_grid.rs:48` **fn** `default_size` — Create a new grid with the default 256 px tile size
`crates/engine/paint/src/tile_grid.rs:53` **fn** `mark_dirty` — Mark a single tile dirty
`crates/engine/paint/src/tile_grid.rs:58` **fn** `mark_clean` — Mark a single tile clean
`crates/engine/paint/src/tile_grid.rs:63` **fn** `is_dirty` — Return `true` if the tile is dirty or has never been rendered
`crates/engine/paint/src/tile_grid.rs:71` **fn** `mark_all_dirty` — Mark all tiles covered by the given page dimensions dirty
`crates/engine/paint/src/tile_grid.rs:84` **fn** `dirty_tiles` — Return all tiles currently marked dirty
`crates/engine/paint/src/tile_grid.rs:107` **fn** `update_from_diff` — Diff `old_dl` against `new_dl` and mark tiles that contain changed
`crates/engine/paint/src/webgl.rs:114` **struct** `SoftwareWebGl` — Pure-Rust software WebGL 1.0 context
`crates/engine/paint/src/webgl.rs:170` **fn** `new` — Create a context with a `width × height` drawing buffer
`crates/engine/paint/src/webgl.rs:197` **fn** `width` — Drawing-buffer width in pixels
`crates/engine/paint/src/webgl.rs:202` **fn** `height` — Drawing-buffer height in pixels
`crates/engine/paint/src/webgl.rs:207` **fn** `pixels` — Borrow the RGBA8 framebuffer (top-left origin, `width*height*4` bytes)
`crates/engine/paint/src/webgl.rs:213` **fn** `pixel` — Read the RGBA pixel at `(x, y)` (top-left origin). Returns
`crates/engine/paint/src/webgl.rs:227` **fn** `viewport` — `gl.viewport(x, y, w, h)`
`crates/engine/paint/src/webgl.rs:232` **fn** `clear_color` — `gl.clearColor(r, g, b, a)`. Components are clamped to `[0, 1]`
`crates/engine/paint/src/webgl.rs:238` **fn** `clear` — `gl.clear(mask)`. Only `COLOR_BUFFER_BIT` has a visible effect; the
`crates/engine/paint/src/webgl.rs:255` **fn** `create_buffer` — `gl.createBuffer()` → opaque buffer id (never 0)
`crates/engine/paint/src/webgl.rs:265` **fn** `bind_buffer` — `gl.bindBuffer(target, buffer)`. `buffer == 0` unbinds. Only
`crates/engine/paint/src/webgl.rs:273` **fn** `buffer_data_f32` — `gl.bufferData(target, data, usage)` for float data. Stores `data`
`crates/engine/paint/src/webgl.rs:280` **fn** `create_shader` — `gl.createShader(kind)` → opaque shader id, or 0 for an unknown kind
`crates/engine/paint/src/webgl.rs:294` **fn** `shader_source` — `gl.shaderSource(shader, source)`
`crates/engine/paint/src/webgl.rs:303` **fn** `compile_shader` — `gl.compileShader(shader)`. Parses the GLSL source into an AST so
`crates/engine/paint/src/webgl.rs:312` **fn** `shader_compiled` — `gl.getShaderParameter(shader, COMPILE_STATUS)` — true once compiled
`crates/engine/paint/src/webgl.rs:317` **fn** `create_program` — `gl.createProgram()` → opaque program id (never 0)
`crates/engine/paint/src/webgl.rs:325` **fn** `attach_shader` — `gl.attachShader(program, shader)`. Slots the shader by its kind
`crates/engine/paint/src/webgl.rs:340` **fn** `link_program` — `gl.linkProgram(program)`. Always marks the program linked
`crates/engine/paint/src/webgl.rs:347` **fn** `program_linked` — `gl.getProgramParameter(program, LINK_STATUS)` — true once linked
`crates/engine/paint/src/webgl.rs:352` **fn** `use_program` — `gl.useProgram(program)`. `program == 0` clears the active program
`crates/engine/paint/src/webgl.rs:358` **fn** `get_attrib_location` — `gl.getAttribLocation(program, name)` → stable location (≥ 0), or -1 if
`crates/engine/paint/src/webgl.rs:375` **fn** `get_uniform_location` — `gl.getUniformLocation(program, name)` → stable location (≥ 0), or -1 if
`crates/engine/paint/src/webgl.rs:391` **fn** `enable_vertex_attrib_array` — `gl.enableVertexAttribArray(index)`
`crates/engine/paint/src/webgl.rs:396` **fn** `disable_vertex_attrib_array` — `gl.disableVertexAttribArray(index)`
`crates/engine/paint/src/webgl.rs:407` **fn** `vertex_attrib_pointer` — `gl.vertexAttribPointer(index, size, type, normalized, stride, offset)`
`crates/engine/paint/src/webgl.rs:422` **fn** `uniform4f` — `gl.uniform4f(location, x, y, z, w)`
`crates/engine/paint/src/webgl.rs:430` **fn** `uniform3f` — `gl.uniform3f(location, x, y, z)`
`crates/engine/paint/src/webgl.rs:437` **fn** `uniform2f` — `gl.uniform2f(location, x, y)`
`crates/engine/paint/src/webgl.rs:444` **fn** `uniform1f` — `gl.uniform1f(location, x)`
`crates/engine/paint/src/webgl.rs:451` **fn** `uniform1i` — `gl.uniform1i(location, v)`. Used to bind sampler2D to a texture unit
`crates/engine/paint/src/webgl.rs:459` **fn** `uniform_matrix4fv` — `gl.uniformMatrix4fv(location, transpose, values)`. Stores a 4×4 float
`crates/engine/paint/src/webgl.rs:468` **fn** `active_texture` — `gl.activeTexture(unit_enum)`. Sets the active texture unit
`crates/engine/paint/src/webgl.rs:473` **fn** `bind_texture` — `gl.bindTexture(target, texture_id)`. Records binding for the active unit
`crates/engine/paint/src/webgl.rs:479` **fn** `tex_image_2d_rgba` — `gl.texImage2D(…, data)`. Averages pixel data to a 1×1 solid colour for
`crates/engine/paint/src/webgl.rs:498` **fn** `draw_arrays` — `gl.drawArrays(mode, first, count)`. Executes vertex and fragment shaders

## lumen-shell  (713 symbols)

`crates/shell/src/address_bar.rs:55` **enum** `OmniboxPrefix` — Префикс @-команды, распознанный в строке ввода
`crates/shell/src/address_bar.rs:66` **fn** `parse_omnibox_prefix` — Разбирает raw ввод → `(OmniboxPrefix, query_str)`
`crates/shell/src/address_bar.rs:79` **enum** `OmniboxSuggestion` — Одна строка autocomplete в dropdown omnibox
`crates/shell/src/address_bar.rs:101` **fn** `commit_value` — Строка, которая будет зафиксирована при выборе этой подсказки
`crates/shell/src/address_bar.rs:109` **fn** `label` — Основной текст строки dropdown
`crates/shell/src/address_bar.rs:121` **fn** `sub_label` — Дополнительный текст под основным label
`crates/shell/src/address_bar.rs:154` **struct** `AddressBarState` — Состояние адресной строки. Хранится в `Lumen` struct наряду с `FindState`
`crates/shell/src/address_bar.rs:169` **fn** `open` — Открыть бар, предзаполнив поле текущим URL страницы
`crates/shell/src/address_bar.rs:177` **fn** `close`
`crates/shell/src/address_bar.rs:185` **fn** `is_open`
`crates/shell/src/address_bar.rs:189` **fn** `input`
`crates/shell/src/address_bar.rs:194` **fn** `suggestions` — Текущий список подсказок (для рендера и клавиатурной навигации)
`crates/shell/src/address_bar.rs:199` **fn** `selected_idx` — Индекс выделенной подсказки. `None` — ни одна не выделена
`crates/shell/src/address_bar.rs:205` **fn** `set_suggestions` — Установить новый список подсказок и сбросить выделение
`crates/shell/src/address_bar.rs:211` **fn** `select_next` — Перейти к следующей (вниз) подсказке
`crates/shell/src/address_bar.rs:222` **fn** `select_prev` — Перейти к предыдущей (вверх) подсказке. `None` если уже на первой
`crates/shell/src/address_bar.rs:230` **fn** `append_str` — Добавить непечатаемые символы (printable chars из keyboard event)
`crates/shell/src/address_bar.rs:244` **fn** `backspace` — Backspace — удалить последний Unicode-символ
`crates/shell/src/address_bar.rs:254` **fn** `commit` — Зафиксировать текущий ввод или выделенную подсказку: закрыть бар и,
`crates/shell/src/address_bar.rs:271` **fn** `take_commit` — Вернуть зафиксированный URL/запрос (если есть) и сбросить его
`crates/shell/src/address_bar.rs:279` **struct** `BarOverlay` — Параметры для сборки overlay display list
`crates/shell/src/address_bar.rs:287` **fn** `build_bar_overlay` — Собирает display list адресной строки. Вызывается каждый кадр, пока
`crates/shell/src/animation_scheduler.rs:49` **struct** `AnimationScheduler` — Планировщик CSS-анимаций. Хранит timing-состояние между кадрами
`crates/shell/src/animation_scheduler.rs:54` **fn** `new`
`crates/shell/src/animation_scheduler.rs:62` **fn** `tick` — Тик планировщика: обходит layout-дерево, для каждой активной анимации
`crates/shell/src/animation_scheduler.rs:75` **fn** `clear` — Удалить все записи для элементов, которых больше нет в дереве
`crates/shell/src/backend_factory.rs:39` **fn** `create_backend` — Создаёт windowed рендер-бэкенд для окна `window`
`crates/shell/src/bidi/protocol.rs:61` **struct** `BidiState` — Connection-level BiDi state
`crates/shell/src/bidi/protocol.rs:103` **fn** `new` — Новое пустое состояние соединения
`crates/shell/src/bidi/protocol.rs:142` **fn** `locale`
`crates/shell/src/bidi/protocol.rs:151` **fn** `timezone`
`crates/shell/src/bidi/protocol.rs:158` **fn** `is_offline`
`crates/shell/src/bidi/protocol.rs:165` **fn** `user_agent_for`
`crates/shell/src/bidi/protocol.rs:178` **fn** `cache_behavior`
`crates/shell/src/bidi/protocol.rs:185` **fn** `intercept_count`
`crates/shell/src/bidi/protocol.rs:197` **fn** `record_response_body`
`crates/shell/src/bidi/protocol.rs:211` **struct** `DispatchResult` — Результат обработки одной команды
`crates/shell/src/bidi/protocol.rs:226` **fn** `dispatch` — Обработать одно BiDi-сообщение, вернуть фреймы для отправки клиенту
`crates/shell/src/bidi/server.rs:14` **fn** `spawn` — Spawn the BiDi server on `127.0.0.1:port`. Non-blocking — runs in a background thread
`crates/shell/src/bidi/transport.rs:18` **fn** `handle` — Handle one accepted TCP stream: WS upgrade → BiDi command loop
`crates/shell/src/click_log.rs:27` **fn** `init` — Вызвать один раз при старте с результатом разбора флага --activity-log
`crates/shell/src/click_log.rs:43` **fn** `is_enabled`
`crates/shell/src/click_log.rs:97` **struct** `ClickInfo` — Клик мышью: window-координаты и что под курсором
`crates/shell/src/click_log.rs:107` **struct** `HitInfo`
`crates/shell/src/click_log.rs:114` **enum** `ClickOutcome`
`crates/shell/src/click_log.rs:123` **fn** `log_click`
`crates/shell/src/click_log.rs:152` **fn** `log_nav` — Навигация на новый URL запущена (navigate_to вызван)
`crates/shell/src/click_log.rs:158` **fn** `log_load_start` — Фоновый поток загрузки страницы стартовал
`crates/shell/src/click_log.rs:165` **fn** `log_load_ok` — Страница загружена и отрисована
`crates/shell/src/click_log.rs:173` **fn** `log_load_err` — Ошибка загрузки
`crates/shell/src/click_log.rs:181` **fn** `log_fragment` — Скроллинг к фрагменту (#id) без перезагрузки страницы
`crates/shell/src/click_log.rs:188` **fn** `log_js_nav` — Навигация из JS (location.href=, history.pushState, window.open …)
`crates/shell/src/click_log.rs:194` **fn** `log_page_ready` — Страница полностью применена (apply_loaded_page завершён)
`crates/shell/src/config.rs:47` **fn** `init_global` — Install the process-global fingerprint profile. Idempotent: the first call
`crates/shell/src/config.rs:53` **fn** `global` — Return the process-global fingerprint profile, or the default if unset
`crates/shell/src/config.rs:63` **struct** `FingerprintProfile` — User-configurable fingerprint identity (9F.1)
`crates/shell/src/config.rs:132` **fn** `effective_tls_profile` — Resolve the effective TLS profile: explicit override, else derived from
`crates/shell/src/config.rs:144` **fn** `navigator_profile` — Build the JS-side [`lumen_js::NavigatorProfile`] from this config
`crates/shell/src/config.rs:168` **fn** `install_navigator` — Install the navigator/screen/timezone values into the process-global JS
`crates/shell/src/config.rs:174` **fn** `apply_http` — Stamp the HTTP and TLS fingerprint onto an [`HttpClient`] builder
`crates/shell/src/config.rs:220` **fn** `effective_socks5_proxy` — Resolve the effective SOCKS5 proxy: explicit override first, then
`crates/shell/src/config.rs:237` **fn** `config_path` — Resolve the platform-specific path to `fingerprint.toml`
`crates/shell/src/config.rs:255` **fn** `load` — Load and parse the fingerprint profile from the default config path
`crates/shell/src/config.rs:267` **fn** `parse` — Parse a flat `key = value` TOML subset into a [`FingerprintProfile`]
`crates/shell/src/deterministic.rs:15` **struct** `DetConfig` — Parsed deterministic-mode configuration from CLI args
`crates/shell/src/deterministic.rs:27` **fn** `extract_deterministic` — Extract all deterministic-mode flags from CLI args
`crates/shell/src/devtools/console_panel.rs:49` **enum** `ConsoleLevel` — Severity level of a console message
`crates/shell/src/devtools/console_panel.rs:94` **struct** `ConsoleMessage` — A single captured console message
`crates/shell/src/devtools/console_panel.rs:107` **struct** `ConsolePanel` — DevTools JS console panel
`crates/shell/src/devtools/console_panel.rs:123` **fn** `new` — Create a new, empty, hidden console panel
`crates/shell/src/devtools/console_panel.rs:135` **fn** `push_batch` — Push a batch of `(level_u8, text)` entries drained from the JS runtime
`crates/shell/src/devtools/console_panel.rs:153` **fn** `clear` — Clear all stored messages and reset scroll
`crates/shell/src/devtools/console_panel.rs:159` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/devtools/console_panel.rs:165` **fn** `len` — Number of stored messages
`crates/shell/src/devtools/console_panel.rs:171` **fn** `is_empty` — `true` when no messages are stored
`crates/shell/src/devtools/console_panel.rs:177` **fn** `scroll_up` — Scroll up by `n` lines (towards older messages)
`crates/shell/src/devtools/console_panel.rs:184` **fn** `scroll_down` — Scroll down by `n` lines (towards newer messages)
`crates/shell/src/devtools/console_panel.rs:196` **fn** `build_console_panel` — Build the viewport-locked console panel overlay
`crates/shell/src/devtools/inspector.rs:54` **struct** `SelectedNode` — A node currently pinned by the inspector, with its computed-style snapshot
`crates/shell/src/devtools/inspector.rs:70` **struct** `DomInspectorPanel` — DevTools DOM inspector panel state
`crates/shell/src/devtools/inspector.rs:82` **fn** `new` — Create a hidden inspector with no hover or selection
`crates/shell/src/devtools/inspector.rs:88` **fn** `toggle` — Toggle inspector activity. Clears hover (but keeps the last selection)
`crates/shell/src/devtools/inspector.rs:97` **fn** `set_hovered` — Update the node under the cursor. Returns `true` when the value changed
`crates/shell/src/devtools/inspector.rs:106` **fn** `select` — Pin a node as the current selection with its computed-style map
`crates/shell/src/devtools/inspector.rs:117` **fn** `scroll_up` — Scroll the property list of the current selection up (towards the top)
`crates/shell/src/devtools/inspector.rs:126` **fn** `scroll_down` — Scroll the property list down (towards the bottom), clamped so the last
`crates/shell/src/devtools/inspector.rs:138` **fn** `find_box` — Find the [`LayoutBox`] for `node` in document order. Returns `None` when the
`crates/shell/src/devtools/inspector.rs:157` **fn** `box_model_rects` — Compute the four box-model rectangles for `lb` in document (page) coordinates
`crates/shell/src/devtools/inspector.rs:212` **fn** `build_box_overlay` — Build the box-model overlay for the hovered box, translated from page
`crates/shell/src/devtools/inspector.rs:245` **fn** `element_label` — Build a human-readable DOM label for `node`, e.g. `div#main.card`, `#text`,
`crates/shell/src/devtools/inspector.rs:276` **fn** `computed_style_map` — Extract a curated computed-style map from a [`LayoutBox`] as ordered
`crates/shell/src/devtools/inspector.rs:373` **fn** `build_inspector_panel` — Build the right-docked computed-style side panel
`crates/shell/src/devtools/network_panel.rs:76` **struct** `NetworkEntry` — A single recorded HTTP request and its lifecycle state
`crates/shell/src/devtools/network_panel.rs:109` **struct** `NetworkLog` — Shared, append-only log of HTTP requests for the network panel
`crates/shell/src/devtools/network_panel.rs:116` **fn** `record_started` — Record a newly started request: appends a pending entry
`crates/shell/src/devtools/network_panel.rs:133` **fn** `record_completed` — Record a completed request: fills the most recent matching pending entry
`crates/shell/src/devtools/network_panel.rs:159` **fn** `record_blocked` — Record a request blocked by the content filter. `reason` is the matched
`crates/shell/src/devtools/network_panel.rs:179` **fn** `record_failed` — Record a network-level failure for a previously started request
`crates/shell/src/devtools/network_panel.rs:206` **fn** `clear` — Clear all recorded requests (call on every top-level navigation)
`crates/shell/src/devtools/network_panel.rs:212` **fn** `len` — Number of recorded requests
`crates/shell/src/devtools/network_panel.rs:218` **fn** `is_empty` — `true` when no requests have been recorded
`crates/shell/src/devtools/network_panel.rs:239` **struct** `NetworkLogSink` — [`EventSink`] wrapper that forwards every event to an inner sink AND records
`crates/shell/src/devtools/network_panel.rs:276` **struct** `NetworkPanel` — DevTools network log panel (§7E.4)
`crates/shell/src/devtools/network_panel.rs:291` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/devtools/network_panel.rs:301` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/devtools/network_panel.rs:307` **fn** `refresh` — Pull the latest entries from the shared [`NetworkLog`] into the panel
`crates/shell/src/devtools/network_panel.rs:314` **fn** `clear_log` — Clear the shared log (call on every top-level navigation)
`crates/shell/src/devtools/network_panel.rs:324` **fn** `len` — Number of entries in the current snapshot
`crates/shell/src/devtools/network_panel.rs:330` **fn** `is_empty` — `true` when the current snapshot has no entries
`crates/shell/src/devtools/network_panel.rs:335` **fn** `scroll_up` — Scroll up by `n` rows (towards older requests)
`crates/shell/src/devtools/network_panel.rs:341` **fn** `scroll_down` — Scroll down by `n` rows (towards newer requests)
`crates/shell/src/devtools/network_panel.rs:353` **fn** `build_network_panel` — Build the viewport-locked network panel overlay
`crates/shell/src/download.rs:45` **struct** `DownloadId` — Opaque identifier for a single download entry
`crates/shell/src/download.rs:50` **enum** `DownloadStatus` — Current state of a download entry
`crates/shell/src/download.rs:71` **struct** `DownloadEntry` — A single download: source URL, destination path, and current status
`crates/shell/src/download.rs:99` **struct** `DownloadManager` — Manages concurrent background downloads and the visibility of the download
`crates/shell/src/download.rs:119` **fn** `new` — Create a new, empty download manager
`crates/shell/src/download.rs:138` **fn** `start_download` — Start a background download of `url` into `dest`
`crates/shell/src/download.rs:173` **fn** `cancel` — Request cancellation of download `id`
`crates/shell/src/download.rs:190` **fn** `open_download` — Open the file in the default OS application
`crates/shell/src/download.rs:203` **fn** `poll` — Drain the internal mpsc channel and update entry statuses
`crates/shell/src/download.rs:234` **fn** `entries` — All entries in insertion order (most recent last)
`crates/shell/src/download.rs:239` **fn** `active_count` — Number of entries whose status is `InProgress` or `Pending`
`crates/shell/src/download.rs:249` **fn** `toggle_visible` — Toggle panel visibility
`crates/shell/src/download.rs:254` **fn** `open` — Show the panel
`crates/shell/src/download.rs:259` **fn** `close` — Hide the panel
`crates/shell/src/download.rs:419` **fn** `build_download_bar` — Build the viewport-locked download panel overlay
`crates/shell/src/extensions/mod.rs:33` **struct** `ContentScript` — A single content-script entry from `manifest.json`
`crates/shell/src/extensions/mod.rs:42` **struct** `ExtensionManifest` — A parsed `manifest.json` for one extension
`crates/shell/src/extensions/mod.rs:69` **struct** `ExtensionRegistry` — Registry of all installed extensions for the current profile
`crates/shell/src/extensions/mod.rs:80` **fn** `extensions_dir` — Return the extensions directory for the current profile
`crates/shell/src/extensions/mod.rs:99` **fn** `load` — Scan the extensions directory and load all valid extensions
`crates/shell/src/extensions/mod.rs:108` **fn** `load_from_dir` — Load extensions from an explicit directory (used in tests)
`crates/shell/src/extensions/mod.rs:135` **fn** `len` — Return the number of loaded extensions
`crates/shell/src/extensions/mod.rs:142` **fn** `is_empty` — Return `true` if no extensions are loaded
`crates/shell/src/extensions/mod.rs:151` **fn** `content_scripts_for_url` — Collect all JS source strings for content scripts that match `page_url`
`crates/shell/src/extensions/mod.rs:316` **fn** `url_matches` — Match `url` against a Chrome-style content-script match pattern
`crates/shell/src/find.rs:29` **struct** `FindState` — Состояние find bar и текущего запроса
`crates/shell/src/find.rs:38` **fn** `is_open`
`crates/shell/src/find.rs:42` **fn** `query`
`crates/shell/src/find.rs:46` **fn** `active_index`
`crates/shell/src/find.rs:50` **fn** `is_regex_mode`
`crates/shell/src/find.rs:54` **fn** `open`
`crates/shell/src/find.rs:58` **fn** `close`
`crates/shell/src/find.rs:64` **fn** `append_str`
`crates/shell/src/find.rs:79` **fn** `backspace`
`crates/shell/src/find.rs:90` **fn** `toggle_regex_mode` — Переключает режим plain-text ↔ regex. Сбрасывает счётчик активного
`crates/shell/src/find.rs:98` **fn** `next` — Циклически переходит к следующему совпадению. `total` — текущее число
`crates/shell/src/find.rs:104` **fn** `prev`
`crates/shell/src/find.rs:115` **struct** `FindMatch` — Найденный матч: bounding box в координатах окна и индекс DrawText-команды
`crates/shell/src/find.rs:128` **fn** `scroll_to_match` — Вычисляет новое значение `scroll_y` так, чтобы `match_rect` попал в
`crates/shell/src/find.rs:152` **fn** `find_matches` — Находит все непересекающиеся вхождения `query` в DrawText-командах `dl`
`crates/shell/src/find.rs:221` **fn** `is_valid_regex_pattern` — Проверяет, является ли `pattern` корректным regex-паттерном
`crates/shell/src/find.rs:238` **fn** `find_matches_regex` — Находит все regex-матчи паттерна `pattern` по [`TextFragment`]-ам
`crates/shell/src/find.rs:314` **struct** `BarOverlay` — Параметры overlay-бара
`crates/shell/src/find.rs:332` **fn** `build_page_with_highlights` — Собирает page-полосу display list-а: исходные команды + highlight-FillRect-ы
`crates/shell/src/find.rs:365` **fn** `build_bar_overlay` — Собирает overlay-полосу: только find-bar (фон + label + input + counter +
`crates/shell/src/find.rs:377` **fn** `build_with_overlay` — Совместимая сборка: page + bar в один list. Только для тестов и dump-режимов
`crates/shell/src/forms.rs:31` **struct** `FormControlState` — Mutable runtime state for a single form control
`crates/shell/src/forms.rs:41` **type** `FormState` — `NodeId` → mutable state map for all form controls on the current page
`crates/shell/src/forms.rs:49` **enum** `FormClickAction` — What the shell should do after a left-click on `node`
`crates/shell/src/forms.rs:63` **fn** `classify_click` — Classify a click on `node` given the current DOM tree
`crates/shell/src/forms.rs:116` **fn** `toggle_details_open` — Toggle the `open` attribute on a `<details>` element in the live DOM
`crates/shell/src/forms.rs:129` **fn** `toggle_checkbox` — Toggle the `checked` attribute on a checkbox input in the live DOM
`crates/shell/src/forms.rs:141` **fn** `set_value` — Set `value` attribute of an input / textarea in the DOM
`crates/shell/src/forms.rs:159` **fn** `find_validation_error` — Depth-first walk: find the first form control that fails HTML5 constraint
`crates/shell/src/forms.rs:170` **fn** `find_control_rect_and_error` — Find rect and error message for a specific invalid control
`crates/shell/src/forms.rs:181` **fn** `find_all_validation_errors` — Collect all form controls that fail HTML5 constraint validation
`crates/shell/src/forms.rs:306` **fn** `find_box_rect` — Find the bounding rect of the LayoutBox for `node`. Returns `None` if the
`crates/shell/src/forms.rs:319` **fn** `find_layout_box` — Find the LayoutBox subtree for `node`. Returns `None` if the node has no box
`crates/shell/src/forms.rs:334` **fn** `collect_modal_dialogs` — Walk `doc` and collect all NodeIds with `data-lumen-modal` attribute
`crates/shell/src/forms.rs:358` **fn** `build_dialog_overlay` — Build a `::backdrop` + translated dialog overlay for a modal `<dialog>`
`crates/shell/src/forms.rs:399` **fn** `build_validation_tooltip` — Build a validation tooltip anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:455` **fn** `collect_form_entries` — Собрать данные формы для submit — DOM-значения, поверх которых наложен
`crates/shell/src/forms.rs:498` **fn** `build_form_submit_event` — Построить параметры отправки формы: `(action, method, body)`
`crates/shell/src/forms.rs:508` **fn** `encode_form_fields` — Encode form fields for submission. Wraps a FormSubmitEvent::Valid variant
`crates/shell/src/forms.rs:521` **fn** `encode_form_fields_multipart` — Encode form fields as `multipart/form-data` (RFC 7578)
`crates/shell/src/forms.rs:533` **fn** `get_form_enctype` — Return the `enctype` attribute of the `<form>` ancestor of `submit_node`,
`crates/shell/src/forms.rs:551` **fn** `build_form_submit`
`crates/shell/src/forms.rs:583` **fn** `make_get_url` — Построить итоговый URL для GET-формы: добавить `?body` к action URL
`crates/shell/src/forms.rs:623` **fn** `build_color_picker` — Build a color-swatch picker anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:660` **fn** `hit_color_swatch` — If viewport-space point `(px, py)` lands on a swatch, return its `[r, g, b]`
`crates/shell/src/forms.rs:681` **fn** `swatch_to_css_color` — Format `[r, g, b]` as CSS `#rrggbb`
`crates/shell/src/forms.rs:691` **struct** `SelectOption` — One entry in a `<select>` dropdown list
`crates/shell/src/forms.rs:714` **fn** `collect_select_options` — Collect all direct `<option>` children of a `<select>` DOM node
`crates/shell/src/forms.rs:751` **fn** `build_select_dropdown` — Build a dropdown overlay anchored below (or above if near the bottom of the
`crates/shell/src/forms.rs:833` **fn** `hit_select_option` — If viewport-space point `(px, py)` lands on an option row, return its index
`crates/shell/src/forms.rs:870` **fn** `apply_select_choice` — Apply the selection of option at `opt_idx` to the `<select>` DOM node:
`crates/shell/src/gc_tick.rs:20` **struct** `GcTick` — Throttled idle GC poller
`crates/shell/src/gc_tick.rs:27` **fn** `new` — Create a new `GcTick`. The first poll fires after [`GC_INTERVAL`] elapses
`crates/shell/src/gc_tick.rs:42` **fn** `poll` — Poll the GC scheduler
`crates/shell/src/hints.rs:18` **struct** `HintItem` — Hint badge for one clickable element
`crates/shell/src/hints.rs:27` **struct** `HintState` — Keyboard hint mode state machine
`crates/shell/src/hints.rs:38` **enum** `HintResult` — Result returned by [`HintState::push_char`]
`crates/shell/src/hints.rs:49` **fn** `is_active` — Whether the hint overlay is currently visible
`crates/shell/src/hints.rs:54` **fn** `open` — Open hint mode with a snapshot of the current page's clickable elements
`crates/shell/src/hints.rs:63` **fn** `close` — Dismiss the overlay without activating anything
`crates/shell/src/hints.rs:71` **fn** `push_char` — Record one typed character and return the resulting state
`crates/shell/src/hints.rs:99` **fn** `typed` — Characters typed so far — used to dim non-matching badges
`crates/shell/src/hints.rs:107` **fn** `items` — Compute viewport-space hint items for the current scroll offsets
`crates/shell/src/hints.rs:172` **fn** `build_hints_overlay` — Build the viewport-locked overlay display list for all active hint badges
`crates/shell/src/input/gesture.rs:36` **enum** `GestureDir` — Six-way gesture direction code
`crates/shell/src/input/gesture.rs:55` **enum** `GestureAction` — Shell action emitted when a completed gesture matches a binding
`crates/shell/src/input/gesture.rs:81` **struct** `GestureMap` — Configurable mapping from [`GestureDir`] to [`GestureAction`]
`crates/shell/src/input/gesture.rs:97` **fn** `empty` — Empty map — no bindings
`crates/shell/src/input/gesture.rs:103` **fn** `bind` — Bind `dir` to `action`, replacing any previous binding
`crates/shell/src/input/gesture.rs:109` **fn** `unbind` — Remove the binding for `dir`
`crates/shell/src/input/gesture.rs:114` **fn** `lookup` — Return the action bound to `dir`, or `None` if unbound
`crates/shell/src/input/gesture.rs:150` **struct** `GestureRecognizer` — State machine for recognizing right-button drag mouse gestures
`crates/shell/src/input/gesture.rs:157` **fn** `new` — Create a recognizer with the default gesture map
`crates/shell/src/input/gesture.rs:163` **fn** `with_map` — Create a recognizer with a custom gesture map
`crates/shell/src/input/gesture.rs:169` **fn** `set_map` — Replace the gesture map at runtime (e.g. from settings)
`crates/shell/src/input/gesture.rs:175` **fn** `map` — Shared reference to the current gesture map
`crates/shell/src/input/gesture.rs:181` **fn** `map_mut` — Mutable reference to the current gesture map
`crates/shell/src/input/gesture.rs:189` **fn** `begin` — Begin tracking a right-button drag from `(x, y)` in CSS pixels
`crates/shell/src/input/gesture.rs:197` **fn** `track` — Update the current drag end-point
`crates/shell/src/input/gesture.rs:211` **fn** `finish` — Finish the drag and return the mapped [`GestureAction`], if any
`crates/shell/src/input/gesture.rs:226` **fn** `cancel` — Cancel the in-progress drag without emitting an action
`crates/shell/src/input/gesture.rs:232` **fn** `is_active` — Returns `true` while a right-button drag is being tracked
`crates/shell/src/input/humanlike.rs:136` **struct** `HumanLikeConfig` — Timing and motion parameters for [`HumanLikeSender`]
`crates/shell/src/input/humanlike.rs:177` **enum** `InputMode` — Controls how injected inputs are delivered to the shell
`crates/shell/src/input/humanlike.rs:202` **struct** `HumanLikeSender` — Wraps [`InputSender`] and injects human-like timing and mouse motion
`crates/shell/src/input/humanlike.rs:216` **fn** `new` — Create a new sender wrapping `inner` with default configuration
`crates/shell/src/input/humanlike.rs:226` **fn** `with_seed` — Create a sender with a fixed PRNG seed for deterministic replay
`crates/shell/src/input/humanlike.rs:235` **fn** `click_at` — Move the cursor along a Bézier arc to `(x, y)`, then dwell, then click
`crates/shell/src/input/humanlike.rs:267` **fn** `type_text` — Type `text` with Gaussian-distributed inter-keystroke delays
`crates/shell/src/input/humanlike.rs:287` **fn** `scroll_to` — Scroll to `(x, y)` immediately (no path animation for scrolls)
`crates/shell/src/input/humanlike.rs:295` **fn** `set_cursor_position` — Override the assumed cursor starting position without moving it
`crates/shell/src/input/mod.rs:40` **enum** `InputCommand` — A single injected input command
`crates/shell/src/input/mod.rs:107` **struct** `InputSender` — Sender side of the input injection channel
`crates/shell/src/input/mod.rs:112` **fn** `click` — Send a synthetic left-click at CSS-pixel coordinates `(x, y)`
`crates/shell/src/input/mod.rs:118` **fn** `mouse_move` — Send a synthetic mouse-move event to CSS-pixel coordinates `(x, y)`
`crates/shell/src/input/mod.rs:124` **fn** `type_text` — Send a synthetic text-typing command
`crates/shell/src/input/mod.rs:130` **fn** `scroll` — Send a synthetic scroll command to position `(x, y)` in CSS pixels
`crates/shell/src/input/mod.rs:140` **fn** `key_down` — Press and release a special key identified by its W3C `KeyboardEvent.code`
`crates/shell/src/input/mod.rs:146` **fn** `enter` — Press Enter in the focused element (submits forms, confirms dialogs)
`crates/shell/src/input/mod.rs:152` **fn** `backspace` — Press Backspace in the focused element (deletes character before cursor)
`crates/shell/src/input/mod.rs:158` **fn** `tab` — Press Tab (move focus to the next focusable element)
`crates/shell/src/input/mod.rs:164` **fn** `escape` — Press Escape (dismiss dialogs, close menus, blur focused element)
`crates/shell/src/input/mod.rs:172` **struct** `InputReceiver` — Receiver side of the input injection channel
`crates/shell/src/input/mod.rs:176` **fn** `drain` — Non-blocking drain: returns all pending commands without blocking
`crates/shell/src/input/mod.rs:185` **fn** `channel` — Create a new input injection channel
`crates/shell/src/input/vim.rs:41` **enum** `VimState` — Which sub-mode the Vim keybinding layer is currently in
`crates/shell/src/input/vim.rs:61` **enum** `VimAction` — Decoded action that the caller should execute in response to a keypress
`crates/shell/src/input/vim.rs:106` **struct** `VimMode` — Vim-mode state machine
`crates/shell/src/input/vim.rs:115` **fn** `new` — Create a new `VimMode` in [`VimState::Normal`]
`crates/shell/src/input/vim.rs:123` **fn** `feed` — Feed one physical key event.  Returns the action to take
`crates/shell/src/links.rs:15` **fn** `find_link_href` — Walk up the ancestor chain from `node_id` to find the nearest `<a>` element
`crates/shell/src/links.rs:43` **fn** `is_navigable_href` — Return true if `href` is a URL scheme the browser should navigate to
`crates/shell/src/links.rs:53` **fn** `fragment_only` — If `href` is a fragment-only reference (starts with `#`), return the
`crates/shell/src/links.rs:60` **fn** `find_element_by_id` — Walk the document tree and return the first element whose `id` attribute
`crates/shell/src/memory_poll.rs:23` **struct** `MemoryPollTick` — Throttled memory pressure poller
`crates/shell/src/memory_poll.rs:36` **fn** `new` — Create a new poller using the given platform source
`crates/shell/src/memory_poll.rs:49` **fn** `tick` — Poll memory pressure and broadcast to `registry` if pressure is Medium or High
`crates/shell/src/memory_poll.rs:66` **fn** `last_level` — Last sampled pressure level.  May be stale by up to [`POLL_INTERVAL`]
`crates/shell/src/memory_poll.rs:75` **fn** `platform_source` — Build the appropriate [`MemoryPressureSource`] for the current platform
`crates/shell/src/momentum_anim.rs:26` **struct** `MomentumAnim` — Velocity-based momentum анимация. Хранится в `Lumen.momentum_anim`
`crates/shell/src/momentum_anim.rs:36` **fn** `new`
`crates/shell/src/momentum_anim.rs:43` **fn** `advance` — Прогнать анимацию до `now_ms`. Возвращает `(Δy, Δx, done)`
`crates/shell/src/notification.rs:18` **fn** `show_os_notification` — Show a desktop notification asynchronously
`crates/shell/src/omnibox/mod.rs:20` **enum** `AliasAction` — Action produced by resolving a raw omnibox input against the alias table
`crates/shell/src/omnibox/mod.rs:39` **fn** `resolve` — Resolve `input` against the alias table and built-in `@` actions
`crates/shell/src/panels/a11y_panel.rs:70` **struct** `A11yPanel` — Accessibility settings panel state
`crates/shell/src/panels/a11y_panel.rs:79` **fn** `new` — Create a new hidden panel with default preferences
`crates/shell/src/panels/a11y_panel.rs:91` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/panels/a11y_panel.rs:96` **fn** `load_draft` — Load current preferences into the draft so edits start from persisted values
`crates/shell/src/panels/a11y_panel.rs:111` **enum** `A11yHit` — Result of a click on (or near) the accessibility panel
`crates/shell/src/panels/a11y_panel.rs:137` **fn** `hit_test` — Classify a click at `(x, y)` CSS px
`crates/shell/src/panels/a11y_panel.rs:226` **fn** `build_a11y_panel` — Build the centred accessibility settings panel overlay
`crates/shell/src/panels/bookmark_panel.rs:95` **struct** `BmEntry` — Lightweight bookmark entry used for panel rendering (loaded from the
`crates/shell/src/panels/bookmark_panel.rs:109` **struct** `BookmarkPanel` — Bookmark manager panel state
`crates/shell/src/panels/bookmark_panel.rs:131` **fn** `new` — Create a new (hidden) panel with an empty bookmark list
`crates/shell/src/panels/bookmark_panel.rs:145` **fn** `toggle` — Flip visibility.  Resets transient state (search focus, drag) when hiding
`crates/shell/src/panels/bookmark_panel.rs:154` **fn** `set_data` — Replace the cached bookmark list and recompute the folder set
`crates/shell/src/panels/bookmark_panel.rs:174` **fn** `visible_entries` — Bookmarks visible under the current folder filter and search query, in
`crates/shell/src/panels/bookmark_panel.rs:191` **fn** `append_search` — Append typed text to the search query (called while `search_active`)
`crates/shell/src/panels/bookmark_panel.rs:197` **fn** `backspace_search` — Delete the last character of the search query
`crates/shell/src/panels/bookmark_panel.rs:203` **fn** `begin_drag` — Begin dragging the bookmark with the given id
`crates/shell/src/panels/bookmark_panel.rs:208` **fn** `take_drag` — Take (and clear) the dragged bookmark id, if a drag is in progress
`crates/shell/src/panels/bookmark_panel.rs:215` **fn** `scroll_by` — Scroll the bookmark list by `dy` CSS px, clamped to `[0, max]` where
`crates/shell/src/panels/bookmark_panel.rs:235` **enum** `BookmarkHit` — Result of a click inside the bookmark panel
`crates/shell/src/panels/bookmark_panel.rs:252` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the panel anchored with its
`crates/shell/src/panels/bookmark_panel.rs:309` **fn** `build_panel` — Build the display list for the panel anchored at `(ax, ay)` (top-left)
`crates/shell/src/panels/cert_panel.rs:58` **struct** `PanelCertData` — Certificate data shown in the panel
`crates/shell/src/panels/cert_panel.rs:81` **fn** `has_data` — Returns `true` if there is meaningful data to display
`crates/shell/src/panels/cert_panel.rs:90` **struct** `CertPanel` — Certificate viewer panel state
`crates/shell/src/panels/cert_panel.rs:101` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/cert_panel.rs:108` **fn** `open` — Open the panel with the given certificate data
`crates/shell/src/panels/cert_panel.rs:115` **fn** `close` — Close the panel
`crates/shell/src/panels/cert_panel.rs:120` **fn** `toggle` — Toggle visibility.  On open: resets scroll to top
`crates/shell/src/panels/cert_panel.rs:129` **fn** `scroll_by` — Scroll the content by `delta` CSS px (positive = down)
`crates/shell/src/panels/cert_panel.rs:137` **fn** `hit_test` — Hit-test a pointer position relative to panel origin
`crates/shell/src/panels/cert_panel.rs:150` **enum** `CertHit` — Result of a pointer hit test on the cert panel
`crates/shell/src/panels/cert_panel.rs:241` **fn** `build_panel` — Append display commands for the cert panel to `buf`
`crates/shell/src/panels/command_palette.rs:83` **enum** `PaletteAction` — A built-in browser action invokable from the palette
`crates/shell/src/panels/command_palette.rs:114` **fn** `label` — Human-readable label shown in the result row
`crates/shell/src/panels/command_palette.rs:133` **fn** `shortcut` — Keyboard-shortcut hint rendered right-aligned in the row (`""` if none)
`crates/shell/src/panels/command_palette.rs:153` **fn** `all` — The full curated command list, in display order (shown first when the
`crates/shell/src/panels/command_palette.rs:177` **enum** `PaletteKind` — What kind of target a palette item represents (drives the row icon and the
`crates/shell/src/panels/command_palette.rs:188` **struct** `PaletteItem` — A single searchable entry in the palette
`crates/shell/src/panels/command_palette.rs:199` **fn** `command` — Build a command item
`crates/shell/src/panels/command_palette.rs:208` **fn** `bookmark` — Build a bookmark item (falls back to the URL when the title is empty)
`crates/shell/src/panels/command_palette.rs:214` **fn** `history` — Build a history item (falls back to the URL when the title is empty)
`crates/shell/src/panels/command_palette.rs:233` **struct** `CommandPalette` — Command palette modal state
`crates/shell/src/panels/command_palette.rs:250` **fn** `new` — Create a hidden palette with the curated command list pre-loaded
`crates/shell/src/panels/command_palette.rs:256` **fn** `open` — Open the palette, resetting the query and selection
`crates/shell/src/panels/command_palette.rs:264` **fn** `close` — Close the palette
`crates/shell/src/panels/command_palette.rs:269` **fn** `toggle` — Toggle visibility; opening resets transient state
`crates/shell/src/panels/command_palette.rs:280` **fn** `set_items` — Replace the item list (commands + bookmarks + history) and clamp the
`crates/shell/src/panels/command_palette.rs:286` **fn** `append` — Append typed text to the query and reset the selection to the top
`crates/shell/src/panels/command_palette.rs:293` **fn** `backspace` — Delete the last character of the query
`crates/shell/src/panels/command_palette.rs:304` **fn** `filtered` — Indices into `items` matching the current query, best match first
`crates/shell/src/panels/command_palette.rs:321` **fn** `select_next` — Move the selection down by one (clamped to the last result)
`crates/shell/src/panels/command_palette.rs:331` **fn** `select_prev` — Move the selection up by one (clamped to the first result)
`crates/shell/src/panels/command_palette.rs:339` **fn** `selected_item` — The currently highlighted item index into `items`, if any result exists
`crates/shell/src/panels/command_palette.rs:380` **fn** `fuzzy_score` — Score `haystack` against `needle` as a case-insensitive subsequence match
`crates/shell/src/panels/command_palette.rs:430` **enum** `PaletteHit` — Result of a click inside the modal palette
`crates/shell/src/panels/command_palette.rs:454` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the modal palette in a
`crates/shell/src/panels/command_palette.rs:477` **fn** `build_panel` — Build the display list for the modal palette over a `viewport_w`×`viewport_h`
`crates/shell/src/panels/focus_panel.rs:72` **struct** `PomodoroTimer` — Wall-clock-driven countdown timer
`crates/shell/src/panels/focus_panel.rs:88` **fn** `new` — Create a running timer of `duration_min` minutes with zero elapsed time
`crates/shell/src/panels/focus_panel.rs:100` **fn** `tick` — Advance the timer to wall-clock `now_ms`.  Adds the delta since the last
`crates/shell/src/panels/focus_panel.rs:111` **fn** `remaining_ms` — Remaining time in milliseconds, clamped to `>= 0`
`crates/shell/src/panels/focus_panel.rs:116` **fn** `progress` — Elapsed fraction in `[0, 1]`.  Returns `1.0` for a zero-length duration
`crates/shell/src/panels/focus_panel.rs:124` **fn** `is_finished` — `true` once the full duration has elapsed
`crates/shell/src/panels/focus_panel.rs:129` **fn** `pause` — Pause counting.  Clears the tick baseline so the paused span is excluded
`crates/shell/src/panels/focus_panel.rs:136` **fn** `resume` — Resume counting.  Clears the tick baseline so the gap before the next
`crates/shell/src/panels/focus_panel.rs:142` **fn** `toggle_pause` — Flip between paused and running
`crates/shell/src/panels/focus_panel.rs:151` **fn** `label` — Remaining time formatted as `MM:SS` (rounded up to whole seconds)
`crates/shell/src/panels/focus_panel.rs:162` **struct** `FocusModePanel` — Focus-mode panel state: the active flag plus the embedded [`PomodoroTimer`]
`crates/shell/src/panels/focus_panel.rs:171` **fn** `new` — Create an inactive panel with a default-length (paused-at-zero) timer
`crates/shell/src/panels/focus_panel.rs:179` **fn** `enter` — Enter focus mode with a fresh `duration_min`-minute timer
`crates/shell/src/panels/focus_panel.rs:185` **fn** `exit` — Leave focus mode (the timer state is kept but no longer ticked)
`crates/shell/src/panels/focus_panel.rs:190` **fn** `toggle` — Toggle focus mode: enter with `duration_min` when off, else exit
`crates/shell/src/panels/focus_panel.rs:199` **fn** `tick` — Advance the embedded timer to `now_ms` when active (no-op otherwise)
`crates/shell/src/panels/focus_panel.rs:216` **enum** `FocusHit` — Result of a click inside the focus widget card
`crates/shell/src/panels/focus_panel.rs:232` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the focus widget card
`crates/shell/src/panels/focus_panel.rs:254` **fn** `build_panel` — Build the display list for the focus widget overlay
`crates/shell/src/panels/history_panel.rs:95` **struct** `HistoryItem` — Lightweight history entry for panel rendering
`crates/shell/src/panels/history_panel.rs:110` **enum** `HistoryRow` — One display row in the scrollable body — either a date-group header or an entry
`crates/shell/src/panels/history_panel.rs:119` **struct** `HistoryPanel` — History panel state
`crates/shell/src/panels/history_panel.rs:149` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/history_panel.rs:154` **fn** `toggle` — Toggle visibility and reset scroll/search when opening
`crates/shell/src/panels/history_panel.rs:163` **fn** `set_items` — Replace the displayed rows (call after data refresh or search)
`crates/shell/src/panels/history_panel.rs:168` **fn** `append_search` — Append a character to the search query
`crates/shell/src/panels/history_panel.rs:173` **fn** `backspace_search` — Delete the last character from the search query
`crates/shell/src/panels/history_panel.rs:178` **fn** `scroll_by` — Scroll by `dy` CSS px (positive = down)
`crates/shell/src/panels/history_panel.rs:184` **fn** `max_scroll` — Maximum scroll offset for the current row set
`crates/shell/src/panels/history_panel.rs:225` **enum** `HistoryHit` — Result of a click inside the history panel
`crates/shell/src/panels/history_panel.rs:245` **fn** `hit_test` — Classify a click at `(mx, my)` in window-space CSS px
`crates/shell/src/panels/history_panel.rs:297` **fn** `build_panel` — Build the panel display list
`crates/shell/src/panels/permission_panel.rs:58` **enum** `PermissionKind` — A single browser permission kind tracked by the panel
`crates/shell/src/panels/permission_panel.rs:79` **fn** `label` — Short display name for the permission row label
`crates/shell/src/panels/permission_panel.rs:89` **fn** `icon` — Emoji icon shown to the left of the label
`crates/shell/src/panels/permission_panel.rs:101` **enum** `PermissionState` — Grant state for a single permission on a single origin
`crates/shell/src/panels/permission_panel.rs:114` **fn** `label` — Label shown on the toggle button
`crates/shell/src/panels/permission_panel.rs:123` **fn** `cycle` — Cycle to the next state: Ask → Allow → Deny → Ask
`crates/shell/src/panels/permission_panel.rs:135` **struct** `PermissionPanel` — Per-site permission popover state (7C.2)
`crates/shell/src/panels/permission_panel.rs:150` **fn** `new` — Create a new hidden panel with no stored permissions
`crates/shell/src/panels/permission_panel.rs:159` **fn** `toggle` — Flip panel visibility
`crates/shell/src/panels/permission_panel.rs:164` **fn** `set_origin` — Update the current origin on navigation (does not clear stored grants)
`crates/shell/src/panels/permission_panel.rs:171` **fn** `state_for` — Return the stored state for `kind` at the current origin
`crates/shell/src/panels/permission_panel.rs:184` **fn** `cycle_permission` — Cycle the state for `kind` at the current origin to the next value
`crates/shell/src/panels/permission_panel.rs:207` **enum** `PermissionHit` — Result of a click inside the permission panel
`crates/shell/src/panels/permission_panel.rs:220` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the permission panel
`crates/shell/src/panels/permission_panel.rs:262` **fn** `build_panel` — Build the display list for the permission floating panel
`crates/shell/src/panels/pip_window.rs:60` **struct** `PipWindow` — Picture-in-picture window state
`crates/shell/src/panels/pip_window.rs:83` **fn** `new` — Create an inactive PiP window positioned at the origin (re-anchored to the
`crates/shell/src/panels/pip_window.rs:97` **fn** `open` — Open the PiP card for a `<video>` source, anchored to the bottom-right of
`crates/shell/src/panels/pip_window.rs:115` **fn** `close` — Close the card (state is retained but no longer drawn)
`crates/shell/src/panels/pip_window.rs:121` **fn** `toggle_play` — Flip the play / pause flag
`crates/shell/src/panels/pip_window.rs:126` **fn** `default_pos` — Default bottom-right anchored top-left corner for a `win_w`×`win_h` window
`crates/shell/src/panels/pip_window.rs:135` **fn** `clamp_to_window` — Clamp the card so it stays fully inside a `win_w`×`win_h` window, leaving
`crates/shell/src/panels/pip_window.rs:143` **fn** `begin_drag` — Begin dragging the card: record the pointer offset from the card origin
`crates/shell/src/panels/pip_window.rs:148` **fn** `dragging` — `true` while a title-bar drag is in progress
`crates/shell/src/panels/pip_window.rs:154` **fn** `drag_to` — Update the card position from the pointer during a drag, clamped to the
`crates/shell/src/panels/pip_window.rs:162` **fn** `end_drag` — End an in-progress drag
`crates/shell/src/panels/pip_window.rs:177` **enum** `PipHit` — Result of a click inside the PiP card
`crates/shell/src/panels/pip_window.rs:193` **fn** `hit_test` — Hit-test a click at window CSS-px `(x, y)` against the PiP card
`crates/shell/src/panels/pip_window.rs:225` **fn** `build_panel` — Build the display list for the PiP overlay.  Empty when inactive
`crates/shell/src/panels/print_panel.rs:76` **enum** `PaperSize` — Paper size for the print job
`crates/shell/src/panels/print_panel.rs:87` **enum** `Orientation` — Page orientation for the print job
`crates/shell/src/panels/print_panel.rs:96` **enum** `MarginPreset` — Margin preset for the print job
`crates/shell/src/panels/print_panel.rs:107` **enum** `ColorMode` — Output colour mode for the print job
`crates/shell/src/panels/print_panel.rs:116` **enum** `PrintField` — Which editable text field currently has keyboard focus in the print panel
`crates/shell/src/panels/print_panel.rs:130` **struct** `PrintPanel` — Print dialog panel state
`crates/shell/src/panels/print_panel.rs:151` **fn** `new` — Create a new hidden panel with default print settings
`crates/shell/src/panels/print_panel.rs:165` **fn** `toggle` — Toggle panel visibility; clears the active editing field on hide
`crates/shell/src/panels/print_panel.rs:173` **fn** `close` — Hide the panel and clear the editing field
`crates/shell/src/panels/print_panel.rs:179` **fn** `push_char` — Append a character to the currently focused text field
`crates/shell/src/panels/print_panel.rs:188` **fn** `pop_char` — Delete the last character from the currently focused text field
`crates/shell/src/panels/print_panel.rs:199` **fn** `margin_px` — Resolve margin values (top/bottom, left/right) in CSS px at 96 DPI
`crates/shell/src/panels/print_panel.rs:218` **enum** `PrintHit` — Result of a click on (or near) the print panel
`crates/shell/src/panels/print_panel.rs:257` **fn** `hit_test` — Classify a click at `(x, y)` CSS px
`crates/shell/src/panels/print_panel.rs:378` **fn** `build_panel` — Build the centred print dialog overlay
`crates/shell/src/panels/privacy_panel.rs:79` **fn** `list_body_height` — Height in CSS px of the scrollable request-list area, given the full window
`crates/shell/src/panels/privacy_panel.rs:88` **struct** `PrivacyPanel` — Privacy network panel (V5). Holds a snapshot of the shared [`NetworkLog`] and
`crates/shell/src/panels/privacy_panel.rs:104` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/panels/privacy_panel.rs:114` **fn** `toggle` — Toggle panel visibility
`crates/shell/src/panels/privacy_panel.rs:120` **fn** `refresh` — Pull the latest entries from the shared [`NetworkLog`] into the snapshot
`crates/shell/src/panels/privacy_panel.rs:129` **fn** `clear_log` — Clear the shared log (call on every top-level navigation). The network
`crates/shell/src/panels/privacy_panel.rs:139` **fn** `len` — Number of entries in the current snapshot
`crates/shell/src/panels/privacy_panel.rs:145` **fn** `is_empty` — `true` when the current snapshot has no entries
`crates/shell/src/panels/privacy_panel.rs:150` **fn** `blocked_count` — Number of blocked requests in the current snapshot
`crates/shell/src/panels/privacy_panel.rs:156` **fn** `allowed_count` — Number of allowed (not blocked) requests in the current snapshot —
`crates/shell/src/panels/privacy_panel.rs:167` **fn** `scroll_down` — Scroll towards older requests by `n` rows
`crates/shell/src/panels/privacy_panel.rs:172` **fn** `scroll_up` — Scroll towards newer requests by `n` rows
`crates/shell/src/panels/privacy_panel.rs:181` **enum** `PrivacyHit` — Result of a click on (or near) the privacy panel
`crates/shell/src/panels/privacy_panel.rs:192` **fn** `hit_test` — Classify a click at `(x, y)` CSS px. `tab_bar_h` is the tab strip height;
`crates/shell/src/panels/privacy_panel.rs:222` **fn** `build_privacy_panel` — Build the right-docked privacy panel overlay
`crates/shell/src/panels/read_later_panel.rs:60` **struct** `ReadLaterPanel` — Read-later panel state
`crates/shell/src/panels/read_later_panel.rs:70` **fn** `new`
`crates/shell/src/panels/read_later_panel.rs:75` **fn** `toggle` — Toggle visibility; resets scroll when opening
`crates/shell/src/panels/read_later_panel.rs:83` **fn** `refresh` — Replace the cached entry list (call after save/delete or on open)
`crates/shell/src/panels/read_later_panel.rs:87` **fn** `scroll_up`
`crates/shell/src/panels/read_later_panel.rs:91` **fn** `scroll_down`
`crates/shell/src/panels/read_later_panel.rs:96` **fn** `max_scroll` — Maximum scroll offset for the current entry count
`crates/shell/src/panels/read_later_panel.rs:107` **enum** `ReadLaterHit` — Result of a click inside or near the panel
`crates/shell/src/panels/read_later_panel.rs:123` **fn** `hit_test` — Classify a click at `(mx, my)` (window-space CSS px)
`crates/shell/src/panels/read_later_panel.rs:160` **fn** `build_panel` — Build the panel display list
`crates/shell/src/panels/read_later_panel.rs:355` **fn** `extract_title_from_html` — Extract the page title from raw HTML bytes
`crates/shell/src/panels/restore_spinner.rs:24` **fn** `build_spinner` — Build spinner overlay if restore has taken longer than THRESHOLD_MS
`crates/shell/src/panels/settings_panel.rs:80` **enum** `SettingsSection` — The four top-level settings sections
`crates/shell/src/panels/settings_panel.rs:102` **fn** `label` — Display label for the tab
`crates/shell/src/panels/settings_panel.rs:116` **enum** `SettingInput` — Which text input currently has keyboard focus
`crates/shell/src/panels/settings_panel.rs:125` **struct** `SettingsPanel` — Settings panel UI state
`crates/shell/src/panels/settings_panel.rs:140` **fn** `new` — Create a new, hidden panel
`crates/shell/src/panels/settings_panel.rs:151` **fn** `open` — Open the panel, loading a fresh snapshot as the working draft
`crates/shell/src/panels/settings_panel.rs:160` **fn** `toggle` — Toggle visibility. When opening, loads `snap` as the draft
`crates/shell/src/panels/settings_panel.rs:169` **fn** `apply_draft` — Clone the current draft for persistence
`crates/shell/src/panels/settings_panel.rs:174` **fn** `append_char` — Append a printable character to the focused text field
`crates/shell/src/panels/settings_panel.rs:183` **fn** `backspace` — Remove the last character from the focused text field
`crates/shell/src/panels/settings_panel.rs:193` **fn** `scroll_by` — Scroll the content area by `dy` CSS px (positive = down)
`crates/shell/src/panels/settings_panel.rs:208` **enum** `SettingsHit` — Result of classifying a click inside the settings panel
`crates/shell/src/panels/settings_panel.rs:237` **fn** `hit_test` — Classify a click at `(mx, my)` in window CSS px. `(px, py)` is the panel
`crates/shell/src/panels/settings_panel.rs:354` **fn** `build_panel` — Append display commands for the settings panel to `list`
`crates/shell/src/panels/shields_panel.rs:58` **struct** `BlockedLog` — Shared accumulator for blocked-request counts, indexed by hostname
`crates/shell/src/panels/shields_panel.rs:69` **fn** `record` — Increment the count for the hostname extracted from `url`
`crates/shell/src/panels/shields_panel.rs:77` **fn** `clear` — Clear all counts (call on every top-level navigation)
`crates/shell/src/panels/shields_panel.rs:83` **fn** `count_for` — Blocked count for a specific hostname (0 if unseen)
`crates/shell/src/panels/shields_panel.rs:96` **struct** `ShieldCountSink` — [`EventSink`] wrapper that forwards every event to an inner sink AND
`crates/shell/src/panels/shields_panel.rs:119` **struct** `ShieldsPanel` — Shields floating panel state (7C.4)
`crates/shell/src/panels/shields_panel.rs:143` **fn** `new` — Create a new hidden panel backed by the given shared `log`
`crates/shell/src/panels/shields_panel.rs:155` **fn** `toggle` — Flip panel visibility
`crates/shell/src/panels/shields_panel.rs:160` **fn** `set_domain` — Update `current_domain` and refresh blocked counts
`crates/shell/src/panels/shields_panel.rs:167` **fn** `refresh` — Pull the latest counts from the shared [`BlockedLog`] into the panel
`crates/shell/src/panels/shields_panel.rs:179` **fn** `clear_log` — Clear the shared blocked log (call on top-level navigation)
`crates/shell/src/panels/shields_panel.rs:188` **fn** `blocked_domain_count` — Blocked-request count for the current domain (from last `refresh`)
`crates/shell/src/panels/shields_panel.rs:193` **fn** `blocked_total_count` — Total blocked-request count for the current page (from last `refresh`)
`crates/shell/src/panels/shields_panel.rs:202` **enum** `ShieldsHit` — Result of a click inside the shields panel
`crates/shell/src/panels/shields_panel.rs:215` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the shields panel
`crates/shell/src/panels/shields_panel.rs:249` **fn** `build_panel` — Build the display list for the shields floating panel
`crates/shell/src/panels/shortcuts_panel.rs:51` **struct** `ShortcutRow` — One entry in the shortcuts list: human label + current binding
`crates/shell/src/panels/shortcuts_panel.rs:64` **fn** `binding_label` — Formatted binding string shown in the key badge (e.g. `"Ctrl+R"`)
`crates/shell/src/panels/shortcuts_panel.rs:80` **fn** `default_rows` — Compile-time default bindings for all displayed commands
`crates/shell/src/panels/shortcuts_panel.rs:129` **enum** `ShortcutsHit` — Hit result from `hit_test`
`crates/shell/src/panels/shortcuts_panel.rs:140` **struct** `ShortcutsPanel` — Keyboard shortcuts panel UI state
`crates/shell/src/panels/shortcuts_panel.rs:156` **fn** `new` — Create a new, hidden panel using compile-time default bindings
`crates/shell/src/panels/shortcuts_panel.rs:168` **fn** `open` — Show the panel
`crates/shell/src/panels/shortcuts_panel.rs:174` **fn** `toggle` — Toggle visibility
`crates/shell/src/panels/shortcuts_panel.rs:179` **fn** `close` — Hide the panel and cancel any pending rebind
`crates/shell/src/panels/shortcuts_panel.rs:185` **fn** `scroll_by` — Scroll the content area by `delta` px (clamped to valid range)
`crates/shell/src/panels/shortcuts_panel.rs:194` **fn** `accept_rebind` — Called when a rebind keypress arrives
`crates/shell/src/panels/shortcuts_panel.rs:210` **fn** `cancel_rebind` — Cancel the current rebind without changing the binding
`crates/shell/src/panels/shortcuts_panel.rs:215` **fn** `hit_test` — Hit-test a click at `(cx, cy)` in panel-local coordinates
`crates/shell/src/panels/shortcuts_panel.rs:232` **fn** `build_panel` — Render the panel into `dl`, anchored at `(ox, oy)` in screen space
`crates/shell/src/panels/sidebar_panel.rs:62` **struct** `SidebarPanel` — Right-docked sidebar web panel state (7D.3)
`crates/shell/src/panels/sidebar_panel.rs:81` **fn** `new` — Create a new hidden sidebar panel with no page loaded
`crates/shell/src/panels/sidebar_panel.rs:93` **fn** `toggle` — Toggle panel visibility.  No-op when no URL has been set
`crates/shell/src/panels/sidebar_panel.rs:103` **fn** `open` — Open the sidebar with `url`.  Clears content if the URL changed
`crates/shell/src/panels/sidebar_panel.rs:116` **fn** `close` — Close the sidebar (hide; URL and content are preserved for re-open)
`crates/shell/src/panels/sidebar_panel.rs:123` **fn** `set_page` — Store a freshly-rendered display list for the sidebar page
`crates/shell/src/panels/sidebar_panel.rs:132` **fn** `max_scroll` — Maximum valid `scroll_y` (0 if content fits in viewport)
`crates/shell/src/panels/sidebar_panel.rs:148` **enum** `SidebarHit` — Result of a click inside the sidebar panel
`crates/shell/src/panels/sidebar_panel.rs:161` **fn** `hit_test` — Hit-test `(x, y)` in CSS px against the sidebar panel
`crates/shell/src/panels/sidebar_panel.rs:198` **fn** `build_panel` — Build the display list for the right-docked sidebar panel
`crates/shell/src/panels/split_view.rs:20` **enum** `SplitFocus` — Which pane receives keyboard and scroll input
`crates/shell/src/panels/split_view.rs:34` **struct** `SplitPane` — Frozen rendering state for the right pane in a split view
`crates/shell/src/panels/split_view.rs:54` **struct** `SplitView` — Active split-view state: two side-by-side `ContentViewport` slots
`crates/shell/src/panels/split_view.rs:63` **fn** `new` — Open split view: right pane shows the given tab's last rendered state
`crates/shell/src/panels/split_view.rs:96` **fn** `build_combined_dl` — Build a combined display list for split-view rendering
`crates/shell/src/panels/split_view.rs:151` **fn** `cursor_in_right` — Return `true` if `window_x` (CSS px) falls inside the right pane
`crates/shell/src/panels/split_view.rs:157` **fn** `right_content_x` — Map a window-space x coord to right-pane content x (accounts for scroll)
`crates/shell/src/panels/split_view.rs:163` **fn** `right_content_y` — Map a window-space y coord to right-pane content y (accounts for scroll)
`crates/shell/src/panels/split_view.rs:168` **fn** `toggle_focus` — Toggle keyboard/scroll focus between left and right pane
`crates/shell/src/panels/split_view.rs:176` **fn** `focus_at` — Transfer focus to whichever pane contains `window_x`
`crates/shell/src/panels/split_view.rs:186` **fn** `scroll_focused_by` — Scroll the focused pane by `dy` CSS px (clamped to content bounds)
`crates/shell/src/panels/tree_tabs.rs:83` **struct** `TreeTabsPanel` — Tree-style tabs panel state
`crates/shell/src/panels/tree_tabs.rs:92` **fn** `new` — Create a new hidden panel with no collapsed subtrees
`crates/shell/src/panels/tree_tabs.rs:97` **fn** `toggle` — Flip visibility. Caller must trigger relayout + redraw
`crates/shell/src/panels/tree_tabs.rs:106` **fn** `toggle_collapsed` — Toggle the collapsed state of the subtree rooted at `tab_id`
`crates/shell/src/panels/tree_tabs.rs:125` **enum** `TreeTabHit` — Result of a click inside the tree tabs panel
`crates/shell/src/panels/tree_tabs.rs:140` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the tree tabs panel
`crates/shell/src/panels/tree_tabs.rs:179` **fn** `build_panel` — Build the display list for the tree-style tabs panel
`crates/shell/src/panels/vertical_tabs.rs:54` **struct** `VerticalTabsPanel` — Vertical tabs panel: list of open tabs rendered as a left-docked sidebar
`crates/shell/src/panels/vertical_tabs.rs:61` **fn** `new` — Create a new (hidden) panel
`crates/shell/src/panels/vertical_tabs.rs:66` **fn** `toggle` — Flip visibility. Caller must trigger relayout + redraw
`crates/shell/src/panels/vertical_tabs.rs:81` **enum** `VTabHit` — Result of a click inside the vertical tab panel area
`crates/shell/src/panels/vertical_tabs.rs:95` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the vertical tabs panel
`crates/shell/src/panels/vertical_tabs.rs:125` **fn** `build_panel` — Build the display list for the vertical tabs panel
`crates/shell/src/panels/workspace_panel.rs:67` **struct** `WsEntry` — Lightweight workspace entry used for panel rendering (loaded from storage on
`crates/shell/src/panels/workspace_panel.rs:80` **struct** `WorkspacePanel` — Workspace switcher panel state
`crates/shell/src/panels/workspace_panel.rs:92` **fn** `new` — Create a new (hidden) panel with an empty workspace list
`crates/shell/src/panels/workspace_panel.rs:102` **fn** `toggle` — Flip visibility.  Caller must trigger redraw (and relayout if changing
`crates/shell/src/panels/workspace_panel.rs:107` **fn** `set_workspaces` — Replace the cached workspace list (call after any storage mutation)
`crates/shell/src/panels/workspace_panel.rs:112` **fn** `set_active` — Mark `id` as the active workspace
`crates/shell/src/panels/workspace_panel.rs:127` **enum** `WorkspaceHit` — Result of a click inside the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:142` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:198` **fn** `build_panel` — Build the display list for the workspace switcher bar
`crates/shell/src/panels/workspace_panel.rs:321` **fn** `parse_ws_color` — Convert a stored CSS colour string (`#RRGGBB`, `#RGB`, or named colour
`crates/shell/src/platform/clipboard.rs:24` **struct** `PlatformClipboard` — Reads and writes the host platform clipboard for `navigator.clipboard`
`crates/shell/src/platform/dark_mode.rs:20` **fn** `theme_prefers_dark` — Maps an OS colour-scheme [`Theme`] to the `prefers-color-scheme: dark`
`crates/shell/src/reader_view.rs:18` **struct** `ArticleContent` — Article content extracted from a raw HTML page
`crates/shell/src/reader_view.rs:37` **fn** `extract_article` — Parse `html` and extract the main article content
`crates/shell/src/reader_view.rs:52` **fn** `build_reader_html` — Wrap an [`ArticleContent`] in the reader template and return a
`crates/shell/src/runtime.rs:39` **enum** `TaskSource` — Источник task-а — HTML §8.1.4.3 «Task sources». Каждому источнику —
`crates/shell/src/runtime.rs:91` **struct** `Task` — Task — отложенное действие, выполняемое за пределами текущего call-stack-а
`crates/shell/src/runtime.rs:97` **fn** `new`
`crates/shell/src/runtime.rs:104` **fn** `source`
`crates/shell/src/runtime.rs:108` **fn** `run`
`crates/shell/src/runtime.rs:122` **struct** `TaskQueue` — Per-source очереди task-ов. Каждый `TaskSource` — отдельная FIFO,
`crates/shell/src/runtime.rs:141` **fn** `new`
`crates/shell/src/runtime.rs:145` **fn** `queue`
`crates/shell/src/runtime.rs:153` **fn** `pop` — Достать task с highest-priority непустой очереди (по
`crates/shell/src/runtime.rs:164` **fn** `len`
`crates/shell/src/runtime.rs:168` **fn** `is_empty`
`crates/shell/src/runtime.rs:174` **fn** `len_of` — Длина очереди конкретного источника — для тестов и метрик
`crates/shell/src/runtime.rs:183` **struct** `Microtask` — Microtask — действие, выполняемое в microtask checkpoint после каждой
`crates/shell/src/runtime.rs:188` **fn** `new`
`crates/shell/src/runtime.rs:194` **fn** `run`
`crates/shell/src/runtime.rs:200` **struct** `MicrotaskQueue`
`crates/shell/src/runtime.rs:205` **fn** `new`
`crates/shell/src/runtime.rs:209` **fn** `queue`
`crates/shell/src/runtime.rs:213` **fn** `pop`
`crates/shell/src/runtime.rs:217` **fn** `len`
`crates/shell/src/runtime.rs:221` **fn** `is_empty`
`crates/shell/src/runtime.rs:229` **type** `AnimationFrameHandle` — Уникальный идентификатор rAF-callback-а, возвращается `request_animation_frame`
`crates/shell/src/runtime.rs:237` **enum** `ObserverKind` — Тип наблюдателя — определяет, в какой стадии rendering steps его callback
`crates/shell/src/runtime.rs:245` **type** `ObserverHandle` — Уникальный handle наблюдателя. `disconnect_observer` снимает регистрацию
`crates/shell/src/runtime.rs:267` **type** `IdleCallbackHandle` — Уникальный идентификатор idle-callback-а — возвращается
`crates/shell/src/runtime.rs:281` **struct** `IdleDeadline` — Аргумент idle-callback-а (W3C `requestIdleCallback` §3 `IdleDeadline`)
`crates/shell/src/runtime.rs:289` **fn** `time_remaining` — Сколько миллисекунд осталось до конца текущего idle-окна. Отрицательные
`crates/shell/src/runtime.rs:300` **fn** `did_timeout` — Был ли callback вызван из-за timeout-параметра запроса (а не реального
`crates/shell/src/runtime.rs:339` **enum** `StepResult` — Результат одной итерации `step()`: запустилась ли task
`crates/shell/src/runtime.rs:349` **struct** `EventLoop` — HTML event loop. Реализует §8.1.4.2 «Processing model» в минимально полезном
`crates/shell/src/runtime.rs:360` **fn** `new`
`crates/shell/src/runtime.rs:368` **fn** `handle` — Дешёвая клон-копия handle-а для постановки task-ов извне и изнутри
`crates/shell/src/runtime.rs:381` **fn** `step` — Один step event-loop-а:
`crates/shell/src/runtime.rs:396` **fn** `perform_microtask_checkpoint` — HTML §8.1.4.4 «Microtask checkpoint». Drain-all: вновь поставленный
`crates/shell/src/runtime.rs:418` **fn** `run_rendering_step` — Rendering opportunity stage — HTML §8.1.5.1 «Run the animation frame
`crates/shell/src/runtime.rs:435` **fn** `pending_tasks` — Сколько task-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:440` **fn** `pending_microtasks` — Сколько microtask-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:446` **fn** `pending_animation_frames` — Сколько rAF-callback-ов сейчас ждёт следующего rendering step
`crates/shell/src/runtime.rs:452` **fn** `pending_idle_callbacks` — Сколько idle-callback-ов сейчас ждёт следующего `run_idle_callbacks`
`crates/shell/src/runtime.rs:474` **fn** `run_idle_callbacks` — W3C `requestIdleCallback` §3 — выполнить ожидающие idle-callback-и
`crates/shell/src/runtime.rs:496` **fn** `active_observers` — Сколько активных наблюдателей указанного типа (для тестов / отладки)
`crates/shell/src/runtime.rs:514` **fn** `deliver_observer_records` — Доставить records всем активным наблюдателям указанного типа
`crates/shell/src/runtime.rs:532` **struct** `EventLoopHandle` — Дёшево клонируемая ссылка на event loop. Closure-ы task-ов / microtask-ов
`crates/shell/src/runtime.rs:537` **fn** `queue_task`
`crates/shell/src/runtime.rs:544` **fn** `queue_microtask`
`crates/shell/src/runtime.rs:553` **fn** `request_animation_frame` — Зарегистрировать rAF-callback. Будет вызван на ближайшем
`crates/shell/src/runtime.rs:572` **fn** `cancel_animation_frame` — Отменить rAF до выполнения. Если handle уже выполнен или неизвестен —
`crates/shell/src/runtime.rs:587` **fn** `request_idle_callback` — Зарегистрировать idle-callback (W3C `requestIdleCallback` §3). Будет
`crates/shell/src/runtime.rs:607` **fn** `cancel_idle_callback` — Отменить idle-callback до выполнения. Неизвестный или уже выполненный
`crates/shell/src/runtime.rs:613` **fn** `register_observer` — Зарегистрировать observer выбранного типа. Callback-ы вызываются при
`crates/shell/src/runtime.rs:630` **fn** `disconnect_observer` — Снять регистрацию наблюдателя. Неизвестный handle — no-op
`crates/shell/src/scroll/decode_gating.rs:22` **fn** `discard_offscreen_images` — Drop CPU-decoded images for all `BoxKind::Image` boxes that are NOT in the
`crates/shell/src/scroll_anim.rs:23` **struct** `ScrollAnim` — Снапшот анимации scroll_y. Хранится в `Lumen.scroll_anim`. Pure-данные —
`crates/shell/src/scroll_anim.rs:36` **fn** `target` — Целевая точка анимации — для аддитивных вызовов
`crates/shell/src/scroll_anim.rs:49` **fn** `sample` — Posizione в момент `now_ms` (CSS px) и флаг завершения
`crates/shell/src/scroll_anim.rs:66` **fn** `ease_out_cubic` — Out-cubic easing: `f(t) = 1 - (1-t)^3`. `f(0)=0`, `f(1)=1`. Параметр
`crates/shell/src/scrollbar.rs:57` **fn** `build_scrollbar_overlay` — Собрать display-command-ы scrollbar-а для подмешивания в overlay
`crates/shell/src/scrollbar.rs:97` **fn** `thumb_geometry` — Pure-fn геометрия thumb-а — `(top, height)` в координатах overlay
`crates/shell/src/scrollbar.rs:119` **enum** `TrackClick` — Результат классификации точки клика по scrollbar-у. `Thumb` — стартуем
`crates/shell/src/scrollbar.rs:132` **fn** `classify_track_click` — Куда попал клик в scrollbar-track: вне / в thumb / выше thumb / ниже thumb
`crates/shell/src/scrollbar.rs:185` **struct** `ScrollDrag` — Снапшот состояния на момент начала drag-а: scroll_y страницы и cursor_y
`crates/shell/src/scrollbar.rs:191` **fn** `new`
`crates/shell/src/scrollbar.rs:199` **fn** `scroll_for` — Желаемый `scroll_y` при текущей позиции курсора. Если scrollbar
`crates/shell/src/session_persist.rs:31` **fn** `open_store` — Open the session store at [`SESSION_DB_PATH`], falling back to an in-memory
`crates/shell/src/session_persist.rs:43` **fn** `active_index` — Index of the tab to make active after restore: the first `is_active` tab, or
`crates/shell/src/source_view.rs:15` **fn** `build_view_source_html` — Wrap `raw` HTML source in a syntax-highlighted page
`crates/shell/src/surface/ctx.rs:22` **struct** `PaintCtx` — Read-only context for [`super::Panel::paint`]
`crates/shell/src/surface/ctx.rs:39` **fn** `new` — Build a paint context with default (non-focused, non-hovered) hints
`crates/shell/src/surface/ctx.rs:56` **struct** `EventCtx` — Side effects a panel may request while handling an event
`crates/shell/src/surface/ctx.rs:69` **fn** `new` — A fresh context with no pending effects
`crates/shell/src/surface/ctx.rs:74` **fn** `dispatch` — Queue a command to be applied after `on_event` returns
`crates/shell/src/surface/ctx.rs:79` **fn** `request_repaint` — Mark this panel dirty so it repaints on the next frame
`crates/shell/src/surface/ctx.rs:84` **fn** `set_cursor` — Ask the shell to show `cursor` while over this panel
`crates/shell/src/surface/ctx.rs:89` **fn** `request_focus` — Ask to capture keyboard focus
`crates/shell/src/surface/ctx.rs:94` **fn** `release_focus` — Ask to release keyboard focus
`crates/shell/src/surface/ctx.rs:101` **fn** `commands` — Commands queued during this event, in dispatch order
`crates/shell/src/surface/ctx.rs:106` **fn** `take_commands` — Take ownership of the queued commands, leaving the context empty
`crates/shell/src/surface/ctx.rs:111` **fn** `wants_repaint` — Whether the panel requested a repaint
`crates/shell/src/surface/ctx.rs:116` **fn** `requested_cursor` — The cursor the panel requested, if any
`crates/shell/src/surface/ctx.rs:122` **fn** `requested_focus_change` — The focus change the panel requested: `Some(true)` to capture focus,
`crates/shell/src/surface/manager.rs:41` **struct** `SlotRect` — Resolved window-space rect for a named docked slot
`crates/shell/src/surface/manager.rs:49` **struct** `LayoutNode` — Informational snapshot of one slot in the docked layout tree
`crates/shell/src/surface/manager.rs:75` **struct** `SurfaceManager` — Single coordinator for all shell UI panels (ADR-009 §SurfaceManager)
`crates/shell/src/surface/manager.rs:87` **fn** `new` — Create an empty manager sized to `(width, height)` CSS px
`crates/shell/src/surface/manager.rs:100` **fn** `register` — Register a panel.  Its rect is computed immediately; `on_mount` is called
`crates/shell/src/surface/manager.rs:113` **fn** `composite` — Composite all visible panels into one `DisplayList` for the renderer
`crates/shell/src/surface/manager.rs:148` **fn** `slot_rect` — Resolved rect for a named docked slot, or `None` if not present
`crates/shell/src/surface/manager.rs:155` **fn** `layout_snapshot` — Snapshot of the docked layout tree (diagnostic / test helper)
`crates/shell/src/surface/manager.rs:170` **fn** `on_resize` — Notify that the window was resized.  All panel rects are recomputed and
`crates/shell/src/surface/manager.rs:186` **fn** `set_visible` — Show or hide a panel by id.  Triggers layout recomputation
`crates/shell/src/surface/manager.rs:195` **fn** `set_theme` — Set the active `Theme` for all subsequent `paint()` calls
`crates/shell/src/surface/manager.rs:200` **fn** `theme` — Active theme
`crates/shell/src/surface/manager.rs:205` **fn** `has_panel` — Whether a panel with `id` is registered
`crates/shell/src/surface/manager.rs:210` **fn** `panel_count` — Number of registered panels
`crates/shell/src/surface/manager.rs:215` **fn** `window_size` — Current window size (CSS px)
`crates/shell/src/surface/manager.rs:220` **fn** `panel_rect` — Rect of a registered panel, or `None` if not found / hidden
`crates/shell/src/surface/manager.rs:232` **fn** `route_mouse_move` — Route a mouse-move event and return the combined response
`crates/shell/src/surface/manager.rs:237` **fn** `route_mouse_down` — Route a mouse-down event
`crates/shell/src/surface/manager.rs:242` **fn** `route_mouse_up` — Route a mouse-up event
`crates/shell/src/surface/manager.rs:247` **fn** `route_click` — Route a click (press + release in the same panel)
`crates/shell/src/surface/manager.rs:252` **fn** `route_scroll` — Route a scroll event
`crates/shell/src/surface/mod.rs:48` **trait** `Panel` — A self-contained shell UI block
`crates/shell/src/surface/theme.rs:21` **struct** `Theme` — All design tokens for one shell appearance
`crates/shell/src/surface/theme.rs:90` **fn** `sand_indigo` — V1 / default: warm sand + indigo (light)
`crates/shell/src/surface/theme.rs:121` **fn** `graphite_amber` — V2 / dark: graphite + amber
`crates/shell/src/surface/theme.rs:152` **fn** `for_dark_mode` — Pick a built-in theme by OS dark-mode preference
`crates/shell/src/surface/types.rs:28` **enum** `Surface` — Where and how a panel appears on screen
`crates/shell/src/surface/types.rs:73` **fn** `is_docked` — `true` for [`Surface::Docked`]
`crates/shell/src/surface/types.rs:78` **fn** `is_overlay` — `true` for floats and modals (anything on the overlay layer)
`crates/shell/src/surface/types.rs:85` **enum** `Corner` — Window corner, used by [`FloatAnchor::Corner`]
`crates/shell/src/surface/types.rs:98` **enum** `FloatAnchor` — Where a [`Surface::Float`] panel is positioned
`crates/shell/src/surface/types.rs:117` **enum** `SizeRule` — How a panel (or slot) describes its desired extent along one axis
`crates/shell/src/surface/types.rs:136` **fn** `resolve` — Resolve a concrete length against the `available` space along the axis
`crates/shell/src/surface/types.rs:146` **fn** `is_flex` — `true` if this rule expands to fill leftover space
`crates/shell/src/surface/types.rs:155` **enum** `MouseButton` — Mouse button identity
`crates/shell/src/surface/types.rs:163` **struct** `ScrollDelta` — Scroll wheel / trackpad delta in CSS px
`crates/shell/src/surface/types.rs:175` **enum** `PanelEvent` — An event delivered to a panel via [`super::Panel::on_event`]
`crates/shell/src/surface/types.rs:206` **enum** `EventResponse` — What a panel returns from [`super::Panel::on_event`]
`crates/shell/src/surface/types.rs:226` **enum** `Command` — State-changing intents a panel can emit
`crates/shell/src/surface/types.rs:258` **enum** `CursorIcon` — Mouse cursor shape requested for a hit target
`crates/shell/src/surface/types.rs:270` **enum** `HitElement` — Semantic identity of the element under the cursor
`crates/shell/src/surface/types.rs:295` **struct** `HitTarget` — Result of [`super::Panel::hit_test`]: what is under a point and how the shell
`crates/shell/src/surface/types.rs:308` **fn** `new` — A minimal hit target for `element` with a default cursor and no tooltip
`crates/shell/src/surface/types.rs:334` **fn** `rect_contains` — `true` if `rect` contains `p` (left/top inclusive, right/bottom exclusive)
`crates/shell/src/tab_lifecycle/manager.rs:14` **type** `TabId` — Opaque tab identifier. Callers create sequential IDs (0, 1, 2, …) or any u64
`crates/shell/src/tab_lifecycle/manager.rs:18` **struct** `TierTransition` — A tier transition that occurred during `tick_idle` or `lru_evict`
`crates/shell/src/tab_lifecycle/manager.rs:35` **struct** `TabLifecycleManager` — Manages lifecycle state for all open tabs
`crates/shell/src/tab_lifecycle/manager.rs:54` **fn** `new` — Create a new manager with the given timeouts and LRU budget
`crates/shell/src/tab_lifecycle/manager.rs:68` **fn** `open_tab` — Open a new tab. The tab starts in Active state and becomes the foreground tab
`crates/shell/src/tab_lifecycle/manager.rs:91` **fn** `activate_tab` — Switch to an existing tab, activating it and sending the previous active tab
`crates/shell/src/tab_lifecycle/manager.rs:136` **fn** `close_tab` — Mark a tab as closed. Advances it to `TabState::Closed` and removes it
`crates/shell/src/tab_lifecycle/manager.rs:157` **fn** `set_pinned` — Pin/unpin a tab. Pinned tabs are never evicted past T1
`crates/shell/src/tab_lifecycle/manager.rs:164` **fn** `tab_state` — Returns the current state of a tab, or `None` if the tab is unknown
`crates/shell/src/tab_lifecycle/manager.rs:169` **fn** `is_active` — Returns `true` if `id` is the foreground (Active) tab
`crates/shell/src/tab_lifecycle/manager.rs:177` **fn** `tick_idle` — Advance all background tabs whose idle timeout has elapsed, and apply
`crates/shell/src/tab_lifecycle/manager.rs:227` **fn** `lru_evict` — Evict least-recently-used background tabs until the number of
`crates/shell/src/tab_lifecycle/manager.rs:283` **fn** `snapshot` — Returns a snapshot of all tab IDs and their current states
`crates/shell/src/tab_lifecycle/restore.rs:22` **struct** `TabMetadata` — Lightweight per-tab identity kept in RAM while a tab is hibernated (T3)
`crates/shell/src/tab_lifecycle/state.rs:10` **enum** `TabState` — Tab lifecycle state (memory tier)
`crates/shell/src/tab_lifecycle/state.rs:34` **enum** `TransitionReason` — Reason for a lifecycle tier transition
`crates/shell/src/tab_lifecycle/state.rs:59` **struct** `TabLifecycle` — Per-tab lifecycle state tracking
`crates/shell/src/tab_lifecycle/state.rs:78` **struct** `TierTimeouts` — User-configurable timeouts for tier transitions
`crates/shell/src/tab_lifecycle/state.rs:101` **enum** `MemoryPressure` — OS memory pressure levels (mirrors `MemoryPressureLevel` from lumen-core)
`crates/shell/src/tab_lifecycle/state.rs:109` **fn** `new` — New tab starts in T0 Active
`crates/shell/src/tab_lifecycle/state.rs:120` **fn** `activate` — Transition to Active (T0), resetting idle counters
`crates/shell/src/tab_lifecycle/state.rs:129` **fn** `hide` — Record the moment the tab was hidden, starting the idle countdown
`crates/shell/src/tab_lifecycle/state.rs:136` **fn** `advance_tier` — Advance to the next tier. Returns `true` if a transition occurred
`crates/shell/src/tab_lifecycle/state.rs:150` **fn** `should_transition_on_idle` — Returns `true` if the idle timeout for the current tier has elapsed
`crates/shell/src/tab_lifecycle/state.rs:167` **fn** `suggested_pressure_state` — If memory pressure justifies an earlier-than-scheduled tier advance, returns
`crates/shell/src/tabs/archive.rs:58` **struct** `ArchivedTab` — A tab that was auto-archived and removed from the visible tab strip
`crates/shell/src/tabs/archive.rs:74` **enum** `ArchiveHit` — Hit result from the archive button or panel
`crates/shell/src/tabs/archive.rs:86` **struct** `TabArchive` — State of the tab archive system
`crates/shell/src/tabs/archive.rs:103` **fn** `new` — Create an empty archive with the panel closed
`crates/shell/src/tabs/archive.rs:108` **fn** `push` — Push a newly-archived tab (prepend — newest entry shown first)
`crates/shell/src/tabs/archive.rs:113` **fn** `take` — Remove and return the archived entry with the given original tab `id`
`crates/shell/src/tabs/archive.rs:119` **fn** `count` — Number of archived entries
`crates/shell/src/tabs/archive.rs:124` **fn** `toggle` — Toggle panel open/closed; resets scroll on open
`crates/shell/src/tabs/archive.rs:132` **fn** `close` — Close panel without clearing entries
`crates/shell/src/tabs/archive.rs:138` **fn** `scroll_up` — Scroll up by one row (clamped at zero)
`crates/shell/src/tabs/archive.rs:144` **fn** `scroll_down` — Scroll down by one row (clamped at last page)
`crates/shell/src/tabs/archive.rs:157` **fn** `archive_btn_x` — Pixel x-coordinate where the archive button begins (right of all tabs)
`crates/shell/src/tabs/archive.rs:177` **fn** `hit_test_button` — Hit-test the archive toolbar button area
`crates/shell/src/tabs/archive.rs:185` **fn** `hit_test_panel` — Hit-test the archive panel when it is open
`crates/shell/src/tabs/archive.rs:238` **fn** `build_button` — Build the archive toolbar button appended to the right of the tab bar
`crates/shell/src/tabs/archive.rs:309` **fn** `build_panel` — Build the drop-down archive panel anchored below the archive button
`crates/shell/src/tabs/containers.rs:44` **enum** `ContainerKind` — Kind of tab container. Drives the border-top colour in the tab strip
`crates/shell/src/tabs/containers.rs:65` **fn** `border_color` — Border-top strip colour, or `None` for [`ContainerKind::None`]
`crates/shell/src/tabs/containers.rs:82` **fn** `name` — Human-readable container name for UI labels
`crates/shell/src/tabs/containers.rs:112` **struct** `ContainerStore` — Origin+container → cookie/storage store id
`crates/shell/src/tabs/containers.rs:122` **fn** `new` — Create an empty store. First minted id will be `0`
`crates/shell/src/tabs/containers.rs:131` **fn** `get_or_create` — Get the store id for `(origin, container)`, allocating a fresh one
`crates/shell/src/tabs/containers.rs:144` **fn** `get` — Look up an existing store id without allocating
`crates/shell/src/tabs/containers.rs:150` **fn** `len` — Number of `(origin, container)` mappings tracked
`crates/shell/src/tabs/containers.rs:156` **fn** `is_empty` — `true` if no mapping has been allocated yet
`crates/shell/src/tabs/strip.rs:56` **struct** `TabEntry` — Metadata for one browser tab
`crates/shell/src/tabs/strip.rs:92` **struct** `TabStrip` — State of the tab strip (tab list + active index)
`crates/shell/src/tabs/strip.rs:103` **fn** `new` — Create the initial tab strip with one blank tab
`crates/shell/src/tabs/strip.rs:119` **fn** `len` — Number of open tabs
`crates/shell/src/tabs/strip.rs:127` **fn** `push_blank` — Append a new blank tab and return its index
`crates/shell/src/tabs/strip.rs:148` **fn** `push_with_opener` — Append a new blank child tab opened by the tab with `opener_id`
`crates/shell/src/tabs/strip.rs:166` **fn** `update_last_activated` — Record `now_ms` as the activation timestamp for the tab at `idx`
`crates/shell/src/tabs/strip.rs:178` **fn** `set_tab_container` — Assign `container` to the tab at `idx`. Out-of-bounds index is a no-op
`crates/shell/src/tabs/strip.rs:186` **fn** `remove` — Remove the tab at `idx`. Returns the new active index (clamped to valid
`crates/shell/src/tabs/strip.rs:198` **fn** `set_active_title` — Update the title of the active tab
`crates/shell/src/tabs/strip.rs:208` **fn** `set_tab_state` — Update the lifecycle state of the tab at `idx`
`crates/shell/src/tabs/strip.rs:219` **enum** `TabHit` — Result of clicking inside the tab bar area
`crates/shell/src/tabs/strip.rs:239` **fn** `hit_test` — Hit-test a click at CSS-px `(x, y)` against the tab bar
`crates/shell/src/tabs/strip.rs:270` **fn** `build_tab_bar` — Build a viewport-locked display list for the tab bar
`crates/shell/src/tabs/strip.rs:376` **fn** `build_tab_tooltip` — Build a small tooltip overlay for a tab with a non-Active tier badge
`crates/shell/src/tabs/tree.rs:22` **fn** `depth_of` — Compute the tree depth of the tab with `id` in the given slice
`crates/shell/src/tabs/tree.rs:38` **fn** `children_of` — Return the IDs of direct children of `parent_id` in strip order
`crates/shell/src/tabs/tree.rs:48` **fn** `subtree_ids` — Collect the IDs of all tabs in the subtree rooted at `root_id` (inclusive)
`crates/shell/src/tabs/tree.rs:63` **struct** `VisibleRow` — A row item produced by [`visible_order`]
`crates/shell/src/tabs/tree.rs:82` **fn** `visible_order` — Build the ordered list of visible tabs for tree-style rendering
`crates/shell/src/zoom.rs:21` **fn** `zoom_in` — Increase zoom by one step, clamped to [`ZOOM_MAX`]
`crates/shell/src/zoom.rs:26` **fn** `zoom_out` — Decrease zoom by one step, clamped to [`ZOOM_MIN`]
`crates/shell/src/zoom.rs:31` **fn** `zoom_reset` — Reset zoom to 100%
`crates/shell/src/zoom.rs:40` **fn** `effective_viewport` — Compute the CSS layout viewport size from the physical window size

## lumen-storage  (452 symbols)

`crates/storage/src/a11y_prefs.rs:38` **enum** `CursorSize` — Accessibility cursor magnification level
`crates/storage/src/a11y_prefs.rs:50` **fn** `as_str` — Serialize to the storage string representation
`crates/storage/src/a11y_prefs.rs:59` **fn** `parse` — Parse from the storage string representation; unknown values → `Normal`
`crates/storage/src/a11y_prefs.rs:72` **struct** `A11yPrefsSnapshot` — All accessibility preferences as a copyable value type
`crates/storage/src/a11y_prefs.rs:105` **struct** `A11yPrefs` — Persistent accessibility preferences store
`crates/storage/src/a11y_prefs.rs:128` **fn** `open` — Open (or create) an on-disk accessibility preferences database
`crates/storage/src/a11y_prefs.rs:134` **fn** `open_in_memory` — Create an in-memory accessibility preferences database (for tests / ephemeral sessions)
`crates/storage/src/a11y_prefs.rs:184` **fn** `font_size_multiplier` — Font-size scale multiplier (e.g. 1.0, 1.25, 1.5)
`crates/storage/src/a11y_prefs.rs:189` **fn** `set_font_size_multiplier` — Set font-size scale multiplier
`crates/storage/src/a11y_prefs.rs:194` **fn** `reduced_motion` — Whether `prefers-reduced-motion` is active
`crates/storage/src/a11y_prefs.rs:199` **fn** `set_reduced_motion` — Set prefers-reduced-motion
`crates/storage/src/a11y_prefs.rs:204` **fn** `forced_colors` — Whether `prefers-forced-colors` is active
`crates/storage/src/a11y_prefs.rs:209` **fn** `set_forced_colors` — Set forced-colors preference
`crates/storage/src/a11y_prefs.rs:214` **fn** `cursor_size` — Cursor magnification level
`crates/storage/src/a11y_prefs.rs:219` **fn** `set_cursor_size` — Set cursor magnification level
`crates/storage/src/a11y_prefs.rs:224` **fn** `snapshot` — Read all preferences into a snapshot value
`crates/storage/src/a11y_prefs.rs:234` **fn** `apply_snapshot` — Persist all fields from a snapshot in one call
`crates/storage/src/autofill.rs:17` **struct** `AutofillEntry`
`crates/storage/src/autofill.rs:25` **struct** `Autofill`
`crates/storage/src/autofill.rs:36` **fn** `open`
`crates/storage/src/autofill.rs:42` **fn** `open_in_memory`
`crates/storage/src/autofill.rs:75` **fn** `record` — Зафиксировать использование значения. Upsert: insert или
`crates/storage/src/autofill.rs:103` **fn** `suggestions` — Получить все сохранённые значения для (origin, field_name),
`crates/storage/src/autofill.rs:131` **fn** `best_for` — Самое популярное значение для поля
`crates/storage/src/autofill.rs:137` **fn** `delete` — Удалить конкретное значение
`crates/storage/src/autofill.rs:151` **fn** `clear_origin` — Удалить все autofill-данные для origin (clear-site-data)
`crates/storage/src/autofill.rs:165` **fn** `clear`
`crates/storage/src/autofill.rs:175` **fn** `count`
`crates/storage/src/bfcache.rs:15` **struct** `BfCacheEntry` — Snapshot of a page suitable for bfcache restoration
`crates/storage/src/bfcache.rs:32` **struct** `BfCache` — In-memory LRU bfcache
`crates/storage/src/bfcache.rs:53` **fn** `new` — Create an empty cache with the given capacity
`crates/storage/src/bfcache.rs:66` **fn** `store` — Store or update an entry
`crates/storage/src/bfcache.rs:84` **fn** `retrieve` — Return a reference to the entry for `url`, or `None` if not cached
`crates/storage/src/bfcache.rs:89` **fn** `remove` — Remove the entry for `url` from the cache
`crates/storage/src/bfcache.rs:95` **fn** `len`
`crates/storage/src/bfcache.rs:99` **fn** `is_empty`
`crates/storage/src/bfcache.rs:103` **fn** `clear`
`crates/storage/src/bookmarks.rs:36` **struct** `Bookmark` — Одна закладка
`crates/storage/src/bookmarks.rs:46` **struct** `Bookmarks`
`crates/storage/src/bookmarks.rs:57` **fn** `open`
`crates/storage/src/bookmarks.rs:63` **fn** `open_in_memory`
`crates/storage/src/bookmarks.rs:103` **fn** `add` — Добавить или обновить закладку. Если url уже существует —
`crates/storage/src/bookmarks.rs:162` **fn** `get` — Получить закладку по url. None если нет
`crates/storage/src/bookmarks.rs:200` **fn** `delete` — Удалить закладку (вместе с тегами благодаря ON DELETE CASCADE)
`crates/storage/src/bookmarks.rs:214` **fn** `list_all` — Все закладки, отсортированные по папке (ASC), затем по created_at DESC
`crates/storage/src/bookmarks.rs:231` **fn** `set_folder` — Переместить закладку в другую папку (DnD reorder в UI-панели)
`crates/storage/src/bookmarks.rs:246` **fn** `list_by_folder` — Список закладок в данной папке (точное совпадение строки)
`crates/storage/src/bookmarks.rs:260` **fn** `list_by_tag` — Список закладок с данным тегом. Сортировка по created_at DESC
`crates/storage/src/bookmarks.rs:277` **fn** `all_tags` — Все уникальные теги в системе (для UI tag-cloud / autocomplete)
`crates/storage/src/bookmarks.rs:296` **fn** `all_folders` — Все уникальные папки
`crates/storage/src/bookmarks.rs:317` **fn** `count` — Общее число закладок
`crates/storage/src/broadcast_channels.rs:24` **struct** `ChannelRegistration`
`crates/storage/src/broadcast_channels.rs:34` **struct** `BroadcastChannels`
`crates/storage/src/broadcast_channels.rs:45` **fn** `open`
`crates/storage/src/broadcast_channels.rs:51` **fn** `open_in_memory`
`crates/storage/src/broadcast_channels.rs:83` **fn** `register` — `new BroadcastChannel(name)` — зарегистрировать. Если уже была
`crates/storage/src/broadcast_channels.rs:113` **fn** `get`
`crates/storage/src/broadcast_channels.rs:129` **fn** `listeners` — Все listeners на конкретном канале origin-а
`crates/storage/src/broadcast_channels.rs:152` **fn** `channels_for_origin` — Все channel-имена, на которые подписан origin (distinct)
`crates/storage/src/broadcast_channels.rs:174` **fn** `unregister` — `channel.close()` — снять регистрацию
`crates/storage/src/broadcast_channels.rs:188` **fn** `unregister_context` — При закрытии вкладки — снять все регистрации этого context-а
`crates/storage/src/broadcast_channels.rs:202` **fn** `count`
`crates/storage/src/browser_settings.rs:41` **struct** `BrowserSettingsSnapshot` — All browser settings in a single value type for easy read/write
`crates/storage/src/browser_settings.rs:76` **struct** `BrowserSettings` — Persistent settings store
`crates/storage/src/browser_settings.rs:99` **fn** `open` — Open (or create) an on-disk settings database
`crates/storage/src/browser_settings.rs:105` **fn** `open_in_memory` — Create an in-memory settings database (for tests / ephemeral sessions)
`crates/storage/src/browser_settings.rs:165` **fn** `homepage` — Homepage / new-tab URL
`crates/storage/src/browser_settings.rs:170` **fn** `set_homepage` — Set homepage URL
`crates/storage/src/browser_settings.rs:175` **fn** `search_engine_id` — ID of the default search engine (`SearchProviderEntry::id`)
`crates/storage/src/browser_settings.rs:180` **fn** `set_search_engine_id` — Set default search engine ID
`crates/storage/src/browser_settings.rs:185` **fn** `shields_enabled` — Whether shields (tracker blocker) are globally enabled
`crates/storage/src/browser_settings.rs:190` **fn** `set_shields_enabled` — Set shields on/off
`crates/storage/src/browser_settings.rs:195` **fn** `fingerprint_mode` — Fingerprint resistance mode: `"standard"`, `"strict"`, or `"off"`
`crates/storage/src/browser_settings.rs:200` **fn** `set_fingerprint_mode` — Set fingerprint resistance mode
`crates/storage/src/browser_settings.rs:205` **fn** `doh_enabled` — Whether DNS-over-HTTPS is enabled
`crates/storage/src/browser_settings.rs:210` **fn** `set_doh_enabled` — Set DNS-over-HTTPS on/off
`crates/storage/src/browser_settings.rs:215` **fn** `font_size` — Base font size in CSS px (e.g. 16.0)
`crates/storage/src/browser_settings.rs:220` **fn** `set_font_size` — Set base font size
`crates/storage/src/browser_settings.rs:225` **fn** `theme` — UI theme: `"dark"`, `"light"`, or `"system"`
`crates/storage/src/browser_settings.rs:230` **fn** `set_theme` — Set UI theme
`crates/storage/src/browser_settings.rs:235` **fn** `download_path` — Absolute path to the default download directory. Empty = OS default
`crates/storage/src/browser_settings.rs:240` **fn** `set_download_path` — Set default download directory path
`crates/storage/src/browser_settings.rs:245` **fn** `snapshot` — Read all settings into a snapshot value
`crates/storage/src/browser_settings.rs:259` **fn** `apply_snapshot` — Persist all fields from a snapshot in one call
`crates/storage/src/cache_storage.rs:19` **struct** `CachedEntry`
`crates/storage/src/cache_storage.rs:30` **struct** `CacheStorage`
`crates/storage/src/cache_storage.rs:41` **fn** `open`
`crates/storage/src/cache_storage.rs:47` **fn** `open_in_memory`
`crates/storage/src/cache_storage.rs:80` **fn** `put` — `cache.put(request, response)` — записать пару
`crates/storage/src/cache_storage.rs:122` **fn** `match_` — `cache.match(request)` — найти ответ. Метод по умолчанию `GET`
`crates/storage/src/cache_storage.rs:146` **fn** `delete` — `cache.delete(request)` — удалить пару. Возвращает true если удалили
`crates/storage/src/cache_storage.rs:168` **fn** `keys` — `cache.keys()` — все entries в одном именованном кэше
`crates/storage/src/cache_storage.rs:193` **fn** `list_cache_names` — `caches.keys()` — список имён всех кэшей origin-а (distinct)
`crates/storage/src/cache_storage.rs:215` **fn** `delete_cache` — `caches.delete(name)` — удалить весь кэш с именем `cache_name`
`crates/storage/src/cache_storage.rs:230` **fn** `clear_origin` — Очистить все entries для origin-а (origin storage clear)
`crates/storage/src/cache_storage.rs:244` **fn** `count`
`crates/storage/src/cached_dns.rs:39` **trait** `Clock` — Источник unix-времени. Дефолт — `SystemTime::now` через
`crates/storage/src/cached_dns.rs:47` **struct** `SystemClock` — Реальные часы через `SystemTime::now()`. При панике (часы до UNIX
`crates/storage/src/cached_dns.rs:63` **struct** `CachedDnsResolver` — Кеширующий DNS-резолвер
`crates/storage/src/cached_dns.rs:74` **fn** `new` — `default_ttl_seconds` — TTL для каждой записи (от `cached_at`)
`crates/storage/src/cached_dns.rs:88` **fn** `with_clock` — То же, что `new`, но с подменяемым clock (тесты)
`crates/storage/src/cookies.rs:28` **enum** `SameSite` — SameSite политика cookie. RFC 6265bis §4.1.2
`crates/storage/src/cookies.rs:59` **struct** `Cookie` — Один cookie с атрибутами. domain хранится lowercase, path — как есть
`crates/storage/src/cookies.rs:72` **struct** `CookieJar` — Cookie jar — обёртка над SQLite-БД cookies
`crates/storage/src/cookies.rs:83` **fn** `open`
`crates/storage/src/cookies.rs:89` **fn** `open_in_memory`
`crates/storage/src/cookies.rs:123` **fn** `set` — Записать (или обновить) cookie. domain нормализуется к lowercase
`crates/storage/src/cookies.rs:155` **fn** `delete` — Удалить конкретный cookie по (domain, path, name, top_level_site)
`crates/storage/src/cookies.rs:183` **fn** `clear_expired` — Удалить все expired cookies (`expires_at < now`). Session cookies
`crates/storage/src/cookies.rs:199` **fn** `clear_session` — Удалить все session cookies (`expires_at IS NULL`). Зовётся при
`crates/storage/src/cookies.rs:217` **fn** `get_for_request` — Получить все cookies, применимые к данному запросу. Фильтрация:
`crates/storage/src/cookies.rs:339` **fn** `parse_set_cookie` — Распарсить значение HTTP-заголовка `Set-Cookie` в `Cookie`. Без PSL
`crates/storage/src/cookies.rs:368` **fn** `parse_set_cookie_with_psl` — Расширенная версия [`parse_set_cookie`] с опциональной проверкой
`crates/storage/src/cookies.rs:554` **struct** `CookieJarProvider` — Implements [`CookieProvider`] using a shared [`CookieJar`]
`crates/storage/src/cookies.rs:561` **fn** `new` — Create a provider backed by the given jar
`crates/storage/src/csp_policies.rs:28` **fn** `parse_csp_header` — Парсит CSP-заголовок в map `directive → sources`
`crates/storage/src/csp_policies.rs:43` **struct** `CspPolicy`
`crates/storage/src/csp_policies.rs:52` **struct** `CspPolicies`
`crates/storage/src/csp_policies.rs:63` **fn** `open`
`crates/storage/src/csp_policies.rs:69` **fn** `open_in_memory`
`crates/storage/src/csp_policies.rs:93` **fn** `store`
`crates/storage/src/csp_policies.rs:110` **fn** `get`
`crates/storage/src/csp_policies.rs:140` **fn** `delete`
`crates/storage/src/csp_policies.rs:153` **fn** `count`
`crates/storage/src/dns_cache.rs:17` **struct** `DnsEntry`
`crates/storage/src/dns_cache.rs:26` **fn** `is_fresh`
`crates/storage/src/dns_cache.rs:31` **struct** `DnsCache`
`crates/storage/src/dns_cache.rs:42` **fn** `open`
`crates/storage/src/dns_cache.rs:48` **fn** `open_in_memory`
`crates/storage/src/dns_cache.rs:78` **fn** `put` — Сохранить DNS-resolve в кэше. Перезаписывает существующую запись
`crates/storage/src/dns_cache.rs:104` **fn** `get` — Получить fresh-запись. Если истекла — `None` (caller идёт в DNS-resolver)
`crates/storage/src/dns_cache.rs:134` **fn** `delete`
`crates/storage/src/dns_cache.rs:147` **fn** `clear_expired`
`crates/storage/src/dns_cache.rs:161` **fn** `clear`
`crates/storage/src/dns_cache.rs:171` **fn** `count`
`crates/storage/src/downloads.rs:16` **enum** `DownloadStatus` — Статус скачивания
`crates/storage/src/downloads.rs:49` **struct** `DownloadEntry` — Одна запись о скачивании
`crates/storage/src/downloads.rs:68` **struct** `Downloads`
`crates/storage/src/downloads.rs:79` **fn** `open`
`crates/storage/src/downloads.rs:85` **fn** `open_in_memory`
`crates/storage/src/downloads.rs:120` **fn** `start` — Создать запись о новом скачивании. Возвращает id
`crates/storage/src/downloads.rs:143` **fn** `update_progress` — Обновить bytes_received (для прогресса)
`crates/storage/src/downloads.rs:157` **fn** `complete` — Зафиксировать успешное завершение
`crates/storage/src/downloads.rs:171` **fn** `cancel` — Зафиксировать отмену пользователем
`crates/storage/src/downloads.rs:185` **fn** `fail` — Зафиксировать ошибку
`crates/storage/src/downloads.rs:198` **fn** `get`
`crates/storage/src/downloads.rs:215` **fn** `list_all` — Все записи в порядке started_at DESC
`crates/storage/src/downloads.rs:238` **fn** `list_by_status` — Только в указанном статусе
`crates/storage/src/downloads.rs:261` **fn** `delete` — Удалить запись (например, после удаления файла или clear-history)
`crates/storage/src/downloads.rs:272` **fn** `clear_completed` — Удалить все завершённые (done/cancelled/failed). Pending не трогаются
`crates/storage/src/downloads.rs:286` **fn** `count`
`crates/storage/src/history.rs:34` **struct** `HistoryEntry` — Запись истории. Возвращается при чтении / поиске
`crates/storage/src/history.rs:45` **struct** `History` — История пользователя
`crates/storage/src/history.rs:56` **fn** `open`
`crates/storage/src/history.rs:62` **fn** `open_in_memory`
`crates/storage/src/history.rs:98` **fn** `record_visit` — Зафиксировать визит. Если url уже встречался — обновляем title /
`crates/storage/src/history.rs:120` **fn** `set_favicon` — Установить favicon-hash для url. Никак не аффектит visit_count
`crates/storage/src/history.rs:134` **fn** `set_text_sha256` — Установить text_sha256 (для дедупликации readability-content)
`crates/storage/src/history.rs:148` **fn** `get` — Найти запись по URL
`crates/storage/src/history.rs:166` **fn** `recent` — Последние N записей (по убыванию visit_date)
`crates/storage/src/history.rs:188` **fn** `most_visited` — Топ-N записей по visit_count. Удобно для new-tab «most visited»
`crates/storage/src/history.rs:212` **fn** `delete` — Удалить запись по url. Никаких ошибок, если url не существует
`crates/storage/src/history.rs:224` **fn** `delete_older_than` — Удалить все записи с `visit_date < before`. Возвращает число
`crates/storage/src/history.rs:239` **fn** `clear` — Полная очистка истории
`crates/storage/src/hsts.rs:19` **struct** `HstsEntry`
`crates/storage/src/hsts.rs:31` **fn** `parse_sts_header` — Парсит Strict-Transport-Security header
`crates/storage/src/hsts.rs:59` **struct** `HstsStore`
`crates/storage/src/hsts.rs:70` **fn** `open`
`crates/storage/src/hsts.rs:76` **fn** `open_in_memory`
`crates/storage/src/hsts.rs:106` **fn** `upsert` — Записать HSTS entry. `host` — lowercase ASCII hostname (без порта)
`crates/storage/src/hsts.rs:146` **fn** `is_https_only` — Проверить, должен ли host обрабатываться как HTTPS-only
`crates/storage/src/hsts.rs:189` **fn** `get`
`crates/storage/src/hsts.rs:212` **fn** `delete`
`crates/storage/src/hsts.rs:223` **fn** `purge_expired` — Удалить все просроченные entries (для GC)
`crates/storage/src/hsts.rs:237` **fn** `count`
`crates/storage/src/http_cache.rs:28` **struct** `CacheControl` — Распарсенные директивы Cache-Control. Из RFC 9111 §5.2 берём только
`crates/storage/src/http_cache.rs:43` **fn** `parse` — Распарсить значение Cache-Control HTTP-заголовка
`crates/storage/src/http_cache.rs:75` **fn** `is_cacheable` — Можно ли вообще хранить ответ в кеше?
`crates/storage/src/http_cache.rs:82` **struct** `CachedResponse` — Кешированная HTTP-запись
`crates/storage/src/http_cache.rs:97` **fn** `is_fresh`
`crates/storage/src/http_cache.rs:105` **struct** `HttpCache`
`crates/storage/src/http_cache.rs:116` **fn** `open`
`crates/storage/src/http_cache.rs:122` **fn** `open_in_memory`
`crates/storage/src/http_cache.rs:157` **fn** `put` — Положить ответ в кеш. Перезаписывает существующую запись с
`crates/storage/src/http_cache.rs:198` **fn** `get` — Получить ответ по URL. Возвращает `Some` даже если запись
`crates/storage/src/http_cache.rs:228` **fn** `get_fresh` — Получить ответ, но только если он свежий (`now < expires_at`)
`crates/storage/src/http_cache.rs:239` **fn** `delete` — Удалить запись
`crates/storage/src/http_cache.rs:253` **fn** `clear_expired` — Удалить expired записи. Возвращает число удалённых строк
`crates/storage/src/http_cache.rs:268` **fn** `clear` — Полная очистка кеша
`crates/storage/src/http_cache.rs:279` **fn** `count` — Общее число записей
`crates/storage/src/indexed_db.rs:38` **fn** `origin_key` — Вычислить безопасный файловый ключ для origin
`crates/storage/src/indexed_db.rs:61` **struct** `IdbStore` — Per-origin persistence для IndexedDB поверх [`StorageBackend`]
`crates/storage/src/indexed_db.rs:72` **fn** `new` — Создать store для конкретного `origin` поверх разделяемого `backend`
`crates/storage/src/indexed_db.rs:85` **fn** `open_or_create` — Открыть или создать выделенный SQLite-файл для IndexedDB
`crates/storage/src/indexed_db.rs:97` **fn** `for_origin` — Открыть или создать IDB-хранилище для `etld_plus_one` в директории `idb_dir`
`crates/storage/src/keyboard_shortcuts.rs:15` **struct** `KeyboardShortcutEntry` — A single keybinding: a command name paired with its modifier + key strings
`crates/storage/src/keyboard_shortcuts.rs:27` **struct** `KeyboardShortcuts` — Persistent store for keyboard shortcut overrides
`crates/storage/src/keyboard_shortcuts.rs:51` **fn** `open` — Open (or create) an on-disk shortcuts database
`crates/storage/src/keyboard_shortcuts.rs:57` **fn** `open_in_memory` — Create an in-memory shortcuts database (for tests / ephemeral sessions)
`crates/storage/src/keyboard_shortcuts.rs:63` **fn** `all` — Return all stored overrides
`crates/storage/src/keyboard_shortcuts.rs:83` **fn** `get` — Return the stored override for `command`, or `None` if using default
`crates/storage/src/keyboard_shortcuts.rs:100` **fn** `set` — Save (or overwrite) a binding override for `command`
`crates/storage/src/keyboard_shortcuts.rs:113` **fn** `remove` — Remove the override for `command` (reverts to compile-time default)
`crates/storage/src/notifications.rs:18` **struct** `Notification`
`crates/storage/src/notifications.rs:34` **struct** `Notifications`
`crates/storage/src/notifications.rs:45` **fn** `open`
`crates/storage/src/notifications.rs:51` **fn** `open_in_memory`
`crates/storage/src/notifications.rs:90` **fn** `show` — Показать notification. Если `tag` непустая и для (origin, tag)
`crates/storage/src/notifications.rs:139` **fn** `mark_dismissed`
`crates/storage/src/notifications.rs:152` **fn** `mark_clicked`
`crates/storage/src/notifications.rs:165` **fn** `get`
`crates/storage/src/notifications.rs:182` **fn** `active` — Активные (не dismissed и не clicked) notifications
`crates/storage/src/notifications.rs:207` **fn** `history` — История всех показанных notifications (включая закрытые)
`crates/storage/src/notifications.rs:229` **fn** `delete`
`crates/storage/src/notifications.rs:239` **fn** `delete_older_than`
`crates/storage/src/notifications.rs:253` **fn** `count`
`crates/storage/src/omnibox_aliases.rs:23` **struct** `OmniboxAlias` — One omnibox bang-alias entry
`crates/storage/src/omnibox_aliases.rs:35` **struct** `OmniboxAliases` — SQLite-backed registry of omnibox bang-aliases
`crates/storage/src/omnibox_aliases.rs:47` **fn** `open` — Open persistent alias store at `path`
`crates/storage/src/omnibox_aliases.rs:54` **fn** `open_in_memory` — Open in-memory store (tests / ephemeral sessions)
`crates/storage/src/omnibox_aliases.rs:97` **fn** `set` — Add or replace an alias.  `trigger` must start with `!`
`crates/storage/src/omnibox_aliases.rs:109` **fn** `get` — Look up an alias by its `trigger` (e.g. `"!g"`)
`crates/storage/src/omnibox_aliases.rs:124` **fn** `list_all` — All aliases ordered by trigger
`crates/storage/src/omnibox_aliases.rs:145` **fn** `delete` — Delete an alias by trigger.  No-op if not found
`crates/storage/src/permissions.rs:20` **enum** `PermissionKind` — Известные типы permissions. Произвольные строки тоже допустимы для
`crates/storage/src/permissions.rs:34` **fn** `as_str`
`crates/storage/src/permissions.rs:47` **fn** `parse`
`crates/storage/src/permissions.rs:63` **enum** `PermissionState` — State permission grant
`crates/storage/src/permissions.rs:91` **struct** `PermissionEntry`
`crates/storage/src/permissions.rs:100` **struct** `Permissions`
`crates/storage/src/permissions.rs:111` **fn** `open`
`crates/storage/src/permissions.rs:117` **fn** `open_in_memory`
`crates/storage/src/permissions.rs:146` **fn** `set` — Поставить state для (origin, kind). Перезаписывает существующий
`crates/storage/src/permissions.rs:170` **fn** `query` — Получить текущий state. Если запись есть, но `expires_at < now` —
`crates/storage/src/permissions.rs:199` **fn** `touch` — Обновить last_used_at — вызывается при фактическом использовании
`crates/storage/src/permissions.rs:213` **fn** `revoke` — Удалить grant (revoke)
`crates/storage/src/permissions.rs:227` **fn** `list_for_origin` — Все permissions для одного origin
`crates/storage/src/permissions.rs:249` **fn** `list_all` — Все записи в БД (для UI permissions-manager)
`crates/storage/src/permissions.rs:271` **fn** `clear_expired` — Удалить все expired grants. Возвращает число удалённых
`crates/storage/src/permissions.rs:286` **fn** `clear_origin` — Удалить все permissions для origin (clear site data)
`crates/storage/src/permissions_policy.rs:26` **enum** `PermissionsAllowlist` — Allowlist для одной feature
`crates/storage/src/permissions_policy.rs:38` **fn** `is_blocked` — `true` если allowlist пуст (`()` или `Origins(vec![])`)
`crates/storage/src/permissions_policy.rs:47` **fn** `allows_self` — `true` если разрешено для текущего origin (`(self)` или `*`)
`crates/storage/src/permissions_policy.rs:59` **fn** `parse_permissions_policy` — Парсит Permissions-Policy header
`crates/storage/src/permissions_policy.rs:129` **struct** `PermissionsPolicy`
`crates/storage/src/permissions_policy.rs:138` **struct** `PermissionsPolicies`
`crates/storage/src/permissions_policy.rs:149` **fn** `open`
`crates/storage/src/permissions_policy.rs:155` **fn** `open_in_memory`
`crates/storage/src/permissions_policy.rs:179` **fn** `store`
`crates/storage/src/permissions_policy.rs:196` **fn** `get`
`crates/storage/src/permissions_policy.rs:226` **fn** `delete`
`crates/storage/src/permissions_policy.rs:239` **fn** `count`
`crates/storage/src/plugins.rs:24` **struct** `PluginManifest`
`crates/storage/src/plugins.rs:37` **struct** `Plugins`
`crates/storage/src/plugins.rs:48` **fn** `open`
`crates/storage/src/plugins.rs:54` **fn** `open_in_memory`
`crates/storage/src/plugins.rs:85` **fn** `install` — Установить плагин. Если name уже есть — Error (UNIQUE constraint)
`crates/storage/src/plugins.rs:108` **fn** `update_manifest` — Обновить версию + capabilities (например, после re-install с новой
`crates/storage/src/plugins.rs:128` **fn** `set_enabled`
`crates/storage/src/plugins.rs:142` **fn** `touch` — Обновить last_used_at (вызывается при каждом invocation плагина)
`crates/storage/src/plugins.rs:155` **fn** `get`
`crates/storage/src/plugins.rs:171` **fn** `get_by_name`
`crates/storage/src/plugins.rs:188` **fn** `list_all` — Все установленные плагины (включая disabled). ORDER BY installed_at ASC
`crates/storage/src/plugins.rs:211` **fn** `list_enabled` — Только enabled-плагины — для runtime-loading
`crates/storage/src/plugins.rs:233` **fn** `uninstall`
`crates/storage/src/plugins.rs:243` **fn** `count`
`crates/storage/src/profiles.rs:25` **struct** `Profile` — Один профиль пользователя
`crates/storage/src/profiles.rs:38` **struct** `ProfileRegistry`
`crates/storage/src/profiles.rs:49` **fn** `open`
`crates/storage/src/profiles.rs:55` **fn** `open_in_memory`
`crates/storage/src/profiles.rs:91` **fn** `create` — Создать новый профиль. Имя должно быть уникальным
`crates/storage/src/profiles.rs:112` **fn** `get` — Получить профиль по id
`crates/storage/src/profiles.rs:134` **fn** `get_by_name` — Получить профиль по имени
`crates/storage/src/profiles.rs:156` **fn** `list_all` — Все профили. Сортировка по created_at ASC (порядок создания)
`crates/storage/src/profiles.rs:181` **fn** `rename` — Переименовать. Имя уникально — конфликт → Error
`crates/storage/src/profiles.rs:195` **fn** `set_settings` — Обновить settings_json
`crates/storage/src/profiles.rs:210` **fn** `delete` — Удалить профиль. Если он был активным — active становится NULL
`crates/storage/src/profiles.rs:224` **fn** `set_active` — Установить активный профиль. `None` → нет активного
`crates/storage/src/profiles.rs:249` **fn** `active` — Получить активный профиль
`crates/storage/src/profiles.rs:273` **fn** `count`
`crates/storage/src/psl.rs:31` **struct** `PslProvider` — Реализация `PublicSuffixList` поверх crate-а `psl` (compiled-in таблица)
`crates/storage/src/psl.rs:35` **fn** `new`
`crates/storage/src/push_subscriptions.rs:20` **struct** `PushSubscription`
`crates/storage/src/push_subscriptions.rs:36` **struct** `PushSubscriptions`
`crates/storage/src/push_subscriptions.rs:47` **fn** `open`
`crates/storage/src/push_subscriptions.rs:53` **fn** `open_in_memory`
`crates/storage/src/push_subscriptions.rs:85` **fn** `subscribe`
`crates/storage/src/push_subscriptions.rs:129` **fn** `get`
`crates/storage/src/push_subscriptions.rs:144` **fn** `get_by_scope`
`crates/storage/src/push_subscriptions.rs:159` **fn** `list_for_origin`
`crates/storage/src/push_subscriptions.rs:180` **fn** `list_all`
`crates/storage/src/push_subscriptions.rs:201` **fn** `unsubscribe`
`crates/storage/src/push_subscriptions.rs:214` **fn** `unsubscribe_origin`
`crates/storage/src/push_subscriptions.rs:228` **fn** `count`
`crates/storage/src/referrer_policy.rs:18` **enum** `ReferrerPolicy`
`crates/storage/src/referrer_policy.rs:43` **fn** `as_str`
`crates/storage/src/referrer_policy.rs:56` **fn** `parse`
`crates/storage/src/referrer_policy.rs:74` **struct** `ReferrerPolicies`
`crates/storage/src/referrer_policy.rs:85` **fn** `open`
`crates/storage/src/referrer_policy.rs:91` **fn** `open_in_memory`
`crates/storage/src/referrer_policy.rs:116` **fn** `set` — Установить policy для origin. Перезаписывает существующую
`crates/storage/src/referrer_policy.rs:135` **fn** `get` — Получить policy для origin. Если нет записи — None
`crates/storage/src/referrer_policy.rs:152` **fn** `get_or_default` — Получить policy с fallback на default (если нет per-origin)
`crates/storage/src/referrer_policy.rs:156` **fn** `delete`
`crates/storage/src/referrer_policy.rs:169` **fn** `list_all`
`crates/storage/src/referrer_policy.rs:193` **fn** `count`
`crates/storage/src/safe_browsing.rs:54` **enum** `ThreatType` — Категория угрозы для записи в Safe Browsing list. Имена совпадают с
`crates/storage/src/safe_browsing.rs:71` **fn** `as_code` — Сериализация в стабильный кодовый идентификатор для БД (lowercase
`crates/storage/src/safe_browsing.rs:84` **fn** `from_code` — Обратный парсинг из кодового id. Неизвестные строки → `Other(s)`,
`crates/storage/src/safe_browsing.rs:112` **fn** `canonical_expression_variants` — Сгенерировать список всех 5×4=20 канонических вариантов `host/path?query`
`crates/storage/src/safe_browsing.rs:131` **fn** `canonical_expression_variants_with_psl` — Версия [`canonical_expression_variants`] с опциональной обрезкой
`crates/storage/src/safe_browsing.rs:266` **fn** `hash_expression` — Хэш канонического expression-а — SHA-256 32 байта. Удобный helper для
`crates/storage/src/safe_browsing.rs:282` **struct** `SafeBrowsingList` — SQLite-backed список Safe Browsing записей
`crates/storage/src/safe_browsing.rs:293` **fn** `open`
`crates/storage/src/safe_browsing.rs:299` **fn** `open_in_memory`
`crates/storage/src/safe_browsing.rs:329` **fn** `add_hash` — Добавить запись по уже-хэшированному значению. `full_hash` обязан
`crates/storage/src/safe_browsing.rs:358` **fn** `add_url` — Удобный wrapper: канонизировать URL → SHA-256 → `add_hash`
`crates/storage/src/safe_browsing.rs:389` **fn** `lookup_hash` — Прямой lookup по полному хэшу (32 байта). Возвращает первое
`crates/storage/src/safe_browsing.rs:415` **fn** `lookup_url` — Главный entry-point фильтрации: проверить URL против всех списков,
`crates/storage/src/safe_browsing.rs:423` **fn** `lookup_url_with_psl` — Версия [`Self::lookup_url`] с опциональной PSL-обрезкой host-suffix
`crates/storage/src/safe_browsing.rs:443` **fn** `clear_list` — Удалить все записи указанного списка. `clear_list("google-v4")` —
`crates/storage/src/safe_browsing.rs:456` **fn** `clear_all` — Удалить все записи во всех списках. Используется при logout/profile
`crates/storage/src/safe_browsing.rs:465` **fn** `count_in` — Сколько записей в конкретном списке
`crates/storage/src/safe_browsing.rs:478` **fn** `count_total` — Сколько всего записей во всех списках
`crates/storage/src/safe_browsing.rs:498` **struct** `SafeBrowsingFilter` — Тонкая обёртка над [`SafeBrowsingList`] для подключения в
`crates/storage/src/safe_browsing.rs:505` **fn** `new`
`crates/storage/src/safe_browsing.rs:513` **fn** `with_psl` — Builder-конструктор с подключённым `PublicSuffixList`. С PSL
`crates/storage/src/search_history.rs:20` **struct** `SearchQuery`
`crates/storage/src/search_history.rs:31` **struct** `SearchHistory`
`crates/storage/src/search_history.rs:42` **fn** `open`
`crates/storage/src/search_history.rs:48` **fn** `open_in_memory`
`crates/storage/src/search_history.rs:80` **fn** `record` — Зафиксировать запрос. Если normalized уже в БД — инкрементит
`crates/storage/src/search_history.rs:104` **fn** `recent` — Последние N запросов по last_used DESC
`crates/storage/src/search_history.rs:126` **fn** `popular` — Самые частые запросы (DESC by frequency, tie-break — last_used DESC)
`crates/storage/src/search_history.rs:149` **fn** `prefix_match` — Запросы, начинающиеся с `prefix` (case-insensitive). Сортировка
`crates/storage/src/search_history.rs:173` **fn** `delete_query`
`crates/storage/src/search_history.rs:186` **fn** `delete_older_than`
`crates/storage/src/search_history.rs:200` **fn** `clear`
`crates/storage/src/search_history.rs:210` **fn** `count`
`crates/storage/src/search_providers.rs:21` **struct** `SearchProviderEntry` — Один поисковый провайдер
`crates/storage/src/search_providers.rs:37` **fn** `build_url` — Подставить query на место `{query}` с URL-encoding по RFC 3986
`crates/storage/src/search_providers.rs:81` **struct** `SearchProviders` — Реестр поисковых провайдеров
`crates/storage/src/search_providers.rs:92` **fn** `open`
`crates/storage/src/search_providers.rs:98` **fn** `open_in_memory`
`crates/storage/src/search_providers.rs:133` **fn** `add` — Добавить провайдера. Имя уникально
`crates/storage/src/search_providers.rs:152` **fn** `get` — Получить провайдера по id
`crates/storage/src/search_providers.rs:169` **fn** `get_by_name`
`crates/storage/src/search_providers.rs:187` **fn** `list_all` — Все провайдеры в порядке создания
`crates/storage/src/search_providers.rs:209` **fn** `delete`
`crates/storage/src/search_providers.rs:221` **fn** `set_default`
`crates/storage/src/search_providers.rs:246` **fn** `default`
`crates/storage/src/search_providers.rs:266` **fn** `count`
`crates/storage/src/service_workers.rs:21` **enum** `UpdateViaCache`
`crates/storage/src/service_workers.rs:32` **fn** `as_str`
`crates/storage/src/service_workers.rs:39` **fn** `parse`
`crates/storage/src/service_workers.rs:50` **struct** `ServiceWorkerRegistration`
`crates/storage/src/service_workers.rs:60` **struct** `ServiceWorkers`
`crates/storage/src/service_workers.rs:71` **fn** `open`
`crates/storage/src/service_workers.rs:77` **fn** `open_in_memory`
`crates/storage/src/service_workers.rs:107` **fn** `register`
`crates/storage/src/service_workers.rs:139` **fn** `touch`
`crates/storage/src/service_workers.rs:152` **fn** `get`
`crates/storage/src/service_workers.rs:169` **fn** `find_for_url` — Найти SW для конкретного URL: scope с самым длинным prefix-match
`crates/storage/src/service_workers.rs:193` **fn** `list_for_origin`
`crates/storage/src/service_workers.rs:214` **fn** `unregister`
`crates/storage/src/service_workers.rs:227` **fn** `unregister_origin`
`crates/storage/src/service_workers.rs:241` **fn** `count`
`crates/storage/src/session_export.rs:26` **struct** `SessionFile` — Portable session file structure
`crates/storage/src/session_export.rs:38` **struct** `ExportedTab` — One tab in a portable session file
`crates/storage/src/session_export.rs:51` **fn** `to_json` — Serialize a [`SessionFile`] to a compact JSON string
`crates/storage/src/session_export.rs:77` **fn** `from_json` — Deserialize a [`SessionFile`] from a JSON string
`crates/storage/src/session_export.rs:139` **fn** `active_tab` — Return the first active tab, or the first tab if none is marked active
`crates/storage/src/session_store.rs:29` **struct** `PersistedTab` — One persisted tab in the saved session
`crates/storage/src/session_store.rs:48` **struct** `SessionStore` — SQLite-backed store holding exactly one session — the tabs open at last close
`crates/storage/src/session_store.rs:60` **fn** `open_in_memory` — Open an in-memory store (data lost when the process exits)
`crates/storage/src/session_store.rs:67` **fn** `open` — Open a persistent on-disk store at `path`
`crates/storage/src/session_store.rs:98` **fn** `save` — Replace the saved session with `tabs`, preserving their order
`crates/storage/src/session_store.rs:130` **fn** `load` — Load all saved tabs in their original left-to-right order
`crates/storage/src/session_store.rs:158` **fn** `clear` — Remove all saved tabs (e.g. user disabled session restore)
`crates/storage/src/session_store.rs:166` **fn** `len` — Number of tabs in the saved session
`crates/storage/src/session_store.rs:175` **fn** `is_empty` — Returns `true` when no session has been saved
`crates/storage/src/site_engagement.rs:22` **struct** `SiteEngagement`
`crates/storage/src/site_engagement.rs:36` **fn** `score` — Engagement score с exponential decay по last_visit. Чем дальше
`crates/storage/src/site_engagement.rs:45` **struct** `SiteEngagementStore`
`crates/storage/src/site_engagement.rs:56` **fn** `open`
`crates/storage/src/site_engagement.rs:62` **fn** `open_in_memory`
`crates/storage/src/site_engagement.rs:91` **fn** `record_visit` — Зафиксировать визит. Инкрементирует visit_count, обновляет last_visit
`crates/storage/src/site_engagement.rs:109` **fn** `add_time` — Добавить time на сайте (foreground seconds)
`crates/storage/src/site_engagement.rs:123` **fn** `get`
`crates/storage/src/site_engagement.rs:142` **fn** `top_by_score` — Топ-N origin-ов по score (decay-нормированному). Алгоритм:
`crates/storage/src/site_engagement.rs:172` **fn** `delete`
`crates/storage/src/site_engagement.rs:185` **fn** `count`
`crates/storage/src/sqlite_store.rs:29` **struct** `SqliteStorage` — Persistent KV-хранилище на SQLite. Создаёт таблицу `kv` при инициализации
`crates/storage/src/sqlite_store.rs:41` **fn** `open` — Открыть БД по пути (файл создаётся при отсутствии)
`crates/storage/src/sqlite_store.rs:49` **fn** `open_in_memory` — Открыть in-memory БД (для тестов и ephemeral session-state)
`crates/storage/src/store.rs:12` **struct** `InMemoryStorage` — In-memory KV-хранилище. Все данные в RAM; `serialize`/`deserialize`
`crates/storage/src/store.rs:77` **fn** `new`
`crates/storage/src/store.rs:82` **fn** `serialize` — Сериализует хранилище в байты (snapshot-формат `LUMEN_KV_V1`)
`crates/storage/src/store.rs:95` **fn** `deserialize` — Десериализует snapshot
`crates/storage/src/store.rs:133` **fn** `save` — Сохраняет snapshot в файл
`crates/storage/src/store.rs:139` **fn** `load` — Загружает snapshot из файла
`crates/storage/src/sw_interceptor.rs:25` **struct** `ServiceWorkerInterceptor` — SQLite-backed SW fetch interceptor
`crates/storage/src/sw_interceptor.rs:31` **fn** `new`
`crates/storage/src/sw_store.rs:25` **struct** `SwStore` — Per-origin persistence SW-регистраций поверх общего [`StorageBackend`]
`crates/storage/src/sw_store.rs:35` **fn** `new` — Создать store для конкретного `origin` поверх разделяемого `backend`
`crates/storage/src/tab_sessions.rs:19` **struct** `TabSession` — Одна вкладка в сохранённой сессии
`crates/storage/src/tab_sessions.rs:40` **struct** `SessionSnapshot` — Снимок сессии — корневая запись для group of tabs
`crates/storage/src/tab_sessions.rs:46` **struct** `TabSessions`
`crates/storage/src/tab_sessions.rs:57` **fn** `open`
`crates/storage/src/tab_sessions.rs:63` **fn** `open_in_memory`
`crates/storage/src/tab_sessions.rs:107` **fn** `create_snapshot` — Создать новый snapshot сессии. Возвращает session_id
`crates/storage/src/tab_sessions.rs:122` **fn** `add_tab` — Добавить вкладку в указанный snapshot
`crates/storage/src/tab_sessions.rs:160` **fn** `update_scroll` — Обновить scroll-позицию (часто меняется)
`crates/storage/src/tab_sessions.rs:174` **fn** `update_form_values` — Обновить form-values (JSON-строка)
`crates/storage/src/tab_sessions.rs:187` **fn** `get_snapshot`
`crates/storage/src/tab_sessions.rs:208` **fn** `list_snapshots` — Все snapshot-ы сессий в порядке created_at DESC (последний — первый)
`crates/storage/src/tab_sessions.rs:236` **fn** `list_tabs` — Все вкладки в snapshot-е
`crates/storage/src/tab_sessions.rs:260` **fn** `delete_snapshot` — Удалить snapshot (cascade удаляет все его вкладки через FK)
`crates/storage/src/tab_sessions.rs:274` **fn** `delete_tab` — Удалить одну вкладку
`crates/storage/src/tab_sessions.rs:285` **fn** `snapshot_count` — Число snapshot-ов
`crates/storage/src/tab_snapshot.rs:75` **struct** `HibernatedTabData` — All data stored on disk for a hibernated tab
`crates/storage/src/tab_snapshot.rs:100` **struct** `TabSnapshotStore` — SQLite-backed store for hibernated tab snapshots
`crates/storage/src/tab_snapshot.rs:112` **fn** `open_in_memory` — Open an in-memory store (data is lost when the process exits)
`crates/storage/src/tab_snapshot.rs:119` **fn** `open` — Open a persistent on-disk store at `path`
`crates/storage/src/tab_snapshot.rs:147` **fn** `store` — Persist a hibernated tab snapshot.  Overwrites any previous entry for
`crates/storage/src/tab_snapshot.rs:171` **fn** `fetch` — Load the hibernated snapshot for `tab_id`
`crates/storage/src/tab_snapshot.rs:202` **fn** `delete` — Remove the snapshot for `tab_id` (called after successful restore)
`crates/storage/src/tab_snapshot.rs:213` **fn** `exists` — Returns `true` if a snapshot exists for `tab_id`
`crates/storage/src/web_manifest.rs:14` **struct** `WebManifest`
`crates/storage/src/web_manifest.rs:25` **struct** `WebManifests`
`crates/storage/src/web_manifest.rs:36` **fn** `open`
`crates/storage/src/web_manifest.rs:42` **fn** `open_in_memory`
`crates/storage/src/web_manifest.rs:69` **fn** `store`
`crates/storage/src/web_manifest.rs:93` **fn** `set_installed`
`crates/storage/src/web_manifest.rs:106` **fn** `get`
`crates/storage/src/web_manifest.rs:130` **fn** `list_installed` — Все установленные PWA (для UI «Installed apps»)
`crates/storage/src/web_manifest.rs:159` **fn** `delete`
`crates/storage/src/web_manifest.rs:172` **fn** `count`
`crates/storage/src/workspaces.rs:18` **struct** `Workspace`
`crates/storage/src/workspaces.rs:32` **struct** `Workspaces`
`crates/storage/src/workspaces.rs:43` **fn** `open`
`crates/storage/src/workspaces.rs:49` **fn** `open_in_memory`
`crates/storage/src/workspaces.rs:81` **fn** `create` — Создать workspace. Position автоматически = MAX(existing)+1
`crates/storage/src/workspaces.rs:109` **fn** `get`
`crates/storage/src/workspaces.rs:124` **fn** `get_by_name`
`crates/storage/src/workspaces.rs:140` **fn** `list_all` — Все workspace-ы в порядке position ASC
`crates/storage/src/workspaces.rs:161` **fn** `rename`
`crates/storage/src/workspaces.rs:174` **fn** `set_color`
`crates/storage/src/workspaces.rs:187` **fn** `set_icon`
`crates/storage/src/workspaces.rs:200` **fn** `set_position`
`crates/storage/src/workspaces.rs:213` **fn** `delete`
`crates/storage/src/workspaces.rs:223` **fn** `count`

---
*Total: 3333 symbols in 20 crates*
