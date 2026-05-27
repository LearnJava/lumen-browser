# SYMBOLS

Auto-generated public API index. Regenerate: `python scripts/gen_symbols.py`

**Usage:** grep for a symbol → get `file:line` → `Read file offset=N limit=30`.

## lumen-canvas  (24 symbols)

`crates/engine/canvas/src/color.rs:3` **struct** `CanvasColor` — RGBA color used by the Canvas 2D API
`crates/engine/canvas/src/color.rs:11` **fn** `rgba`
`crates/engine/canvas/src/color.rs:16` **fn** `with_alpha_mult` — Multiply `self.a` by `alpha` (0.0–1.0)
`crates/engine/canvas/src/color.rs:25` **fn** `from_css_str` — Parse a CSS color string.  Supports:
`crates/engine/canvas/src/lib.rs:27` **struct** `Context2D` — HTML Canvas 2D rendering context
`crates/engine/canvas/src/lib.rs:47` **fn** `new` — Create a new context with a transparent black buffer
`crates/engine/canvas/src/lib.rs:63` **fn** `width`
`crates/engine/canvas/src/lib.rs:64` **fn** `height`
`crates/engine/canvas/src/lib.rs:67` **fn** `pixels` — Raw RGBA8 pixel data
`crates/engine/canvas/src/lib.rs:70` **fn** `resize` — Resize the canvas (clears the buffer)
`crates/engine/canvas/src/lib.rs:82` **fn** `clear_rect` — `clearRect(x, y, w, h)` — erase region to transparent black
`crates/engine/canvas/src/lib.rs:97` **fn** `fill_rect` — `fillRect(x, y, w, h)` — fill region with current `fillStyle`
`crates/engine/canvas/src/lib.rs:103` **fn** `stroke_rect` — `strokeRect(x, y, w, h)` — stroke the outline of a rectangle
`crates/engine/canvas/src/lib.rs:117` **fn** `begin_path` — `beginPath()` — discard current path
`crates/engine/canvas/src/lib.rs:123` **fn** `move_to` — `moveTo(x, y)` — start a new sub-path
`crates/engine/canvas/src/lib.rs:132` **fn** `line_to` — `lineTo(x, y)` — add a line segment
`crates/engine/canvas/src/lib.rs:143` **fn** `close_path` — `closePath()` — add a line back to the sub-path start
`crates/engine/canvas/src/lib.rs:153` **fn** `arc` — `arc(cx, cy, r, start_angle, end_angle[, anticlockwise])` — add an arc
`crates/engine/canvas/src/lib.rs:173` **fn** `fill` — `fill()` — fill the current path with `fillStyle`
`crates/engine/canvas/src/lib.rs:180` **fn** `stroke` — `stroke()` — stroke the current path with `strokeStyle`
`crates/engine/canvas/src/path.rs:3` **enum** `PathSegment` — A single segment in a 2D path
`crates/engine/canvas/src/path.rs:11` **type** `PathCommand` — Alias kept for API symmetry with the HTML spec (`PathCommand` = verb)
`crates/engine/canvas/src/rasterize.rs:4` **fn** `fill_path` — Fill `path` using the even-odd scanline algorithm
`crates/engine/canvas/src/rasterize.rs:41` **fn** `stroke_path` — Stroke `path` by drawing each line segment as a thick rectangle

## lumen-core  (137 symbols)

`crates/core/src/capability.rs:7` **enum** `Capability`
`crates/core/src/capability.rs:27` **struct** `CapabilityToken`
`crates/core/src/error.rs:7` **enum** `Error`
`crates/core/src/error.rs:35` **type** `Result`
`crates/core/src/event.rs:9` **struct** `TabId`
`crates/core/src/event.rs:14` **enum** `SubresourceKind` — Тип subresource-ресурса, найденного preload-сканером
`crates/core/src/event.rs:29` **enum** `FetchPriority` — Приоритет выборки subresource-а. Отражает HTML Living Standard §17.2.3
`crates/core/src/event.rs:41` **fn** `for_kind` — Приоритет по типу subresource (Fetch Standard §2.2)
`crates/core/src/event.rs:53` **enum** `Event`
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
`crates/core/src/ext.rs:263` **trait** `EncodingDetector` — Определение кодировки HTML-документа. Для кириллицы критично уметь
`crates/core/src/ext.rs:273` **enum** `FontStyle` — Начертание face-а: `font-style` из CSS Fonts L4. Phase 0 — три
`crates/core/src/ext.rs:282` **fn** `parse_keyword` — Парсит CSS-ключевое слово `normal | italic | oblique` (case-insensitive)
`crates/core/src/ext.rs:303` **struct** `FaceRecord` — Метаданные одного face-а в индексе шрифтов
`crates/core/src/ext.rs:339` **trait** `FontProvider` — Источник системных шрифтов. Реализация — в `lumen-font::system_fonts`
`crates/core/src/ext.rs:393` **fn** `match_face` — CSS Fonts L4 §5.2 алгоритм матчинга — извлечён из trait-а в свободную
`crates/core/src/ext.rs:626` **trait** `JsRuntime` — JavaScript runtime — исполнение JS-кода (HTML inline scripts, `eval`,
`crates/core/src/ext.rs:647` **enum** `JsValue` — Простые JSON-совместимые типы для передачи через trait-границу
`crates/core/src/ext.rs:660` **fn** `object` — Хелпер: построить object из key-value пар
`crates/core/src/ext.rs:670` **enum** `JsError` — Ошибка исполнения JavaScript: либо syntax error (parse), либо runtime
`crates/core/src/ext.rs:691` **type** `JsResult`
`crates/core/src/ext.rs:696` **struct** `NullJsRuntime` — Null implementation — всегда возвращает `JsError::NotImplemented`
`crates/core/src/ext.rs:734` **trait** `UnicodeProvider` — Unicode-таблицы: line break (UAX #14), grapheme/word segmentation
`crates/core/src/ext.rs:759` **struct** `NullUnicodeProvider` — Null-реализация `UnicodeProvider` — все методы возвращают пустые векторы
`crates/core/src/ext.rs:787` **trait** `IdnaProvider` — IDN (Internationalized Domain Names) полный UTS #46. Свой Punycode-encoder
`crates/core/src/ext.rs:797` **struct** `NullIdnaProvider` — Null-реализация `IdnaProvider` — все методы возвращают `None`. Потребитель
`crates/core/src/ext.rs:822` **trait** `PublicSuffixList` — Public Suffix List — отделение публичных суффиксов от регистрируемых
`crates/core/src/ext.rs:843` **struct** `NullPublicSuffixList` — Null-реализация `PublicSuffixList` — все запросы возвращают `None`/`false`
`crates/core/src/ext.rs:869` **trait** `ContentDecoder` — HTTP `Content-Encoding` декодер. Один экземпляр trait-а = один кодек
`crates/core/src/ext.rs:884` **struct** `UnsupportedContentDecoder` — Stub-реализация `ContentDecoder` для encoding-а, на который нет
`crates/core/src/ext.rs:915` **trait** `FontFormat` — Декодер альтернативных файловых форматов шрифта (WOFF2, WOFF) в raw
`crates/core/src/ext.rs:933` **struct** `NullFontFormat` — Null-реализация `FontFormat` — `can_decode` всегда `false`,
`crates/core/src/ext.rs:958` **trait** `ImageDecoder` — Plug-in декодер растровых изображений для форматов, не встроенных в
`crates/core/src/ext.rs:985` **trait** `SpellChecker` — Spell checker — проверка орфографии для form field / contenteditable
`crates/core/src/ext.rs:999` **struct** `NullSpellChecker` — Null-реализация `SpellChecker` — `check` всегда возвращает `true`, чтобы
`crates/core/src/ext.rs:1016` **trait** `HyphenationProvider` — Hyphenation — поиск позиций мягких переносов для CSS `hyphens: auto`
`crates/core/src/ext.rs:1027` **struct** `NullHyphenationProvider` — Null-реализация `HyphenationProvider` — никаких переносов не предлагается
`crates/core/src/ext.rs:1044` **enum** `WsMessage` — Сообщение, полученное от WebSocket-сервера (RFC 6455 §5.6)
`crates/core/src/ext.rs:1060` **trait** `WebSocketSession` — Открытое WebSocket-соединение. Объект владеет TCP/TLS-стримом
`crates/core/src/ext.rs:1076` **trait** `WebSocketProvider` — Фабрика WebSocket-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1094` **struct** `SseEvent` — Полностью разобранное SSE-событие (HTML Living Standard §9.2.6)
`crates/core/src/ext.rs:1110` **trait** `SseSession` — Открытое SSE-соединение (EventSource). Блокирующий интерфейс
`crates/core/src/ext.rs:1127` **trait** `SseProvider` — Фабрика SSE-соединений. Реализуется `lumen-network::HttpClient`
`crates/core/src/ext.rs:1154` **trait** `FetchInterceptor` — Перехватчик fetch-запросов уровня Service Worker
`crates/core/src/ext.rs:1166` **struct** `JsFetchResult` — Full HTTP response for a synchronous JS `fetch()` call
`crates/core/src/ext.rs:1184` **trait** `JsFetchProvider` — Synchronous HTTP fetch bridge for the JS runtime
`crates/core/src/ext.rs:1195` **enum** `JsWsEvent` — A single queued event from a WebSocket connection, ready for delivery to JS
`crates/core/src/ext.rs:1225` **trait** `JsWebSocketSession` — A live WebSocket connection from the JS runtime's perspective
`crates/core/src/ext.rs:1240` **trait** `JsWebSocketProvider` — Factory that opens WebSocket connections for the JS runtime
`crates/core/src/form.rs:15` **struct** `FormEntry` — Запись формы — пара (name, value) с опциональным filename (для multipart)
`crates/core/src/form.rs:21` **enum** `FormValue`
`crates/core/src/form.rs:33` **fn** `text`
`crates/core/src/form.rs:40` **fn** `file`
`crates/core/src/form.rs:62` **fn** `encode_form_urlencoded` — Сериализует form-set как `application/x-www-form-urlencoded`
`crates/core/src/form.rs:97` **fn** `decode_form_value` — Decode urlencoded form value: `+` → пробел; `%HH` → байт. Не-валидные
`crates/core/src/form.rs:129` **fn** `encode_form_multipart` — Сериализует form-set как `multipart/form-data` (RFC 7578)
`crates/core/src/geom.rs:7` **struct** `Point`
`crates/core/src/geom.rs:21` **struct** `Size`
`crates/core/src/geom.rs:38` **struct** `Rect`
`crates/core/src/geom.rs:71` **fn** `origin`
`crates/core/src/geom.rs:78` **fn** `size`
`crates/core/src/geom.rs:85` **fn** `right`
`crates/core/src/geom.rs:89` **fn** `bottom`
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

## lumen-css-parser  (48 symbols)

`crates/engine/css-parser/src/parser.rs:38` **enum** `SimpleSelector`
`crates/engine/css-parser/src/parser.rs:50` **struct** `AttrSelector`
`crates/engine/css-parser/src/parser.rs:61` **enum** `AttrOp`
`crates/engine/css-parser/src/parser.rs:77` **enum** `PseudoClass`
`crates/engine/css-parser/src/parser.rs:317` **enum** `DirArg` — Аргумент `:dir(...)` pseudo-class (CSS Selectors L4 §13.2)
`crates/engine/css-parser/src/parser.rs:328` **struct** `RelativeSelector` — Один элемент relative-selector-list-а из `:has()`. `combinator` — если
`crates/engine/css-parser/src/parser.rs:341` **struct** `NthSpec` — Формула `an+b` из CSS Selectors §6.6.5.1. Элемент с 1-based индексом `i`
`crates/engine/css-parser/src/parser.rs:351` **fn** `matches` — Возвращает true, если элемент с 1-based индексом `index` матчит формулу
`crates/engine/css-parser/src/parser.rs:370` **struct** `CompoundSelector`
`crates/engine/css-parser/src/parser.rs:375` **enum** `Combinator`
`crates/engine/css-parser/src/parser.rs:387` **struct** `ComplexSelector`
`crates/engine/css-parser/src/parser.rs:401` **fn** `specificity` — Specificity по CSS Selectors Level 3 §16:
`crates/engine/css-parser/src/parser.rs:478` **struct** `Specificity`
`crates/engine/css-parser/src/parser.rs:497` **struct** `Declaration`
`crates/engine/css-parser/src/parser.rs:506` **struct** `Rule`
`crates/engine/css-parser/src/parser.rs:517` **struct** `PropertyRule` — CSS Properties and Values L1 §1.1 — регистрация custom property через
`crates/engine/css-parser/src/parser.rs:525` **struct** `Stylesheet`
`crates/engine/css-parser/src/parser.rs:593` **struct** `ContainerRule` — `@container <name>? <condition> { rules }` — CSS Containment L3 §3
`crates/engine/css-parser/src/parser.rs:606` **struct** `CounterStyleRule` — `@counter-style <name> { ... }` — CSS Counter Styles L3 §2
`crates/engine/css-parser/src/parser.rs:615` **struct** `PageRule` — `@page <selector>? { decls }` — CSS Paged Media L3 §3
`crates/engine/css-parser/src/parser.rs:626` **struct** `ScopeRule` — `@scope (<root>) [to (<limit>)] { rules }` — CSS Cascade L6
`crates/engine/css-parser/src/parser.rs:639` **struct** `StartingStyleRule` — `@starting-style { rules }` — CSS Transitions L2 §3.4. Контейнер
`crates/engine/css-parser/src/parser.rs:645` **struct** `KeyframesRule` — `@keyframes name { offset { decls } ... }` — CSS Animations L1 §3
`crates/engine/css-parser/src/parser.rs:654` **struct** `Keyframe`
`crates/engine/css-parser/src/parser.rs:663` **struct** `SupportsRule` — `@supports <condition> { rules }` блок — CSS Conditional Rules L3 §2
`crates/engine/css-parser/src/parser.rs:680` **enum** `SupportsCondition` — Условие в `@supports (...)`. Грамматика:
`crates/engine/css-parser/src/parser.rs:703` **fn** `evaluate` — Вычислить условие: вернуть `true`, если потребитель поддерживает
`crates/engine/css-parser/src/parser.rs:718` **struct** `LayerRule` — `@layer name { rules }` блок
`crates/engine/css-parser/src/parser.rs:728` **struct** `ImportRule` — `@import` декларация. Per CSS Cascade L4 §6.5 + Media Queries L4:
`crates/engine/css-parser/src/parser.rs:742` **struct** `FontFaceRule` — `@font-face { font-family: ...; src: url(...) format(...); ... }`
`crates/engine/css-parser/src/parser.rs:767` **struct** `FontFaceSource`
`crates/engine/css-parser/src/parser.rs:776` **enum** `FontFaceSourceKind`
`crates/engine/css-parser/src/parser.rs:785` **struct** `MediaRule` — Группа CSS-правил, вложенных в `@media`-блок
`crates/engine/css-parser/src/parser.rs:793` **struct** `MediaQuery` — Media query — OR-список AND-clauses (Media Queries L4 §3). Пустой
`crates/engine/css-parser/src/parser.rs:807` **struct** `MediaQueryClause` — Одна clause в media query — AND-список feature/media-type условий
`crates/engine/css-parser/src/parser.rs:819` **enum** `MediaCondition`
`crates/engine/css-parser/src/parser.rs:832` **enum** `MediaFeature`
`crates/engine/css-parser/src/parser.rs:854` **enum** `MediaOrientation`
`crates/engine/css-parser/src/parser.rs:860` **enum** `ColorScheme`
`crates/engine/css-parser/src/parser.rs:869` **struct** `MediaContext` — Контекст, против которого матчатся media queries. Заполняется
`crates/engine/css-parser/src/parser.rs:894` **fn** `matches` — Пустой query (= `@media all`) — true. Иначе хотя бы одна
`crates/engine/css-parser/src/parser.rs:909` **fn** `matches` — Per Media Queries L4 §3.2: пустая `conditions` — clause invalid
`crates/engine/css-parser/src/parser.rs:926` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:936` **fn** `matches`
`crates/engine/css-parser/src/parser.rs:973` **fn** `parse`
`crates/engine/css-parser/src/parser.rs:981` **fn** `parse_inline_style` — Парсит содержимое HTML-атрибута `style="..."` — declaration-list без
`crates/engine/css-parser/src/parser.rs:1138` **fn** `parse_supports_condition` — Парсит `@supports`-условие из строки между `@supports` и `{`
`crates/engine/css-parser/src/parser.rs:1312` **fn** `parse_media_query` — Распарсить media query из строки между `@media` и `{`. Принимает

## lumen-devtools  (8 symbols)

`crates/devtools/src/cdp.rs:18` **fn** `dispatch` — Обработать одно CDP сообщение, вернуть JSON-строку для отправки клиенту
`crates/devtools/src/server.rs:11` **struct** `DevToolsServer` — Фоновый DevTools сервер. Живёт пока не дропнется (join handle отсоединён)
`crates/devtools/src/server.rs:19` **fn** `spawn` — Запустить сервер на `127.0.0.1:port`. Не блокирует — поток в фоне
`crates/devtools/src/server.rs:28` **fn** `port`
`crates/devtools/src/ws.rs:12` **enum** `WsError`
`crates/devtools/src/ws.rs:42` **fn** `upgrade` — Прочитать HTTP Upgrade запрос, проверить заголовки, отправить 101
`crates/devtools/src/ws.rs:104` **fn** `read_text_frame` — Прочитать один WebSocket фрейм (RFC 6455 §5.2)
`crates/devtools/src/ws.rs:125` **fn** `write_text_frame` — Отправить text фрейм (server→client, без маски)

## lumen-dom  (54 symbols)

`crates/engine/dom/src/lib.rs:9` **struct** `NodeId`
`crates/engine/dom/src/lib.rs:12` **fn** `index`
`crates/engine/dom/src/lib.rs:16` **fn** `from_index`
`crates/engine/dom/src/lib.rs:22` **enum** `Namespace`
`crates/engine/dom/src/lib.rs:32` **struct** `QualName`
`crates/engine/dom/src/lib.rs:38` **fn** `html`
`crates/engine/dom/src/lib.rs:47` **struct** `Attribute`
`crates/engine/dom/src/lib.rs:57` **enum** `ShadowRootMode` — Shadow root mode per Shadow DOM spec §4.2
`crates/engine/dom/src/lib.rs:72` **enum** `NodeData`
`crates/engine/dom/src/lib.rs:105` **struct** `Node`
`crates/engine/dom/src/lib.rs:112` **fn** `element_name`
`crates/engine/dom/src/lib.rs:121` **fn** `get_attr` — Возвращает значение атрибута по имени (ASCII case-insensitive). На
`crates/engine/dom/src/lib.rs:137` **fn** `sandbox_flags` — Sandbox-ограничения для `<iframe sandbox="...">` по HTML LS §7.6.5
`crates/engine/dom/src/lib.rs:149` **fn** `input_type` — HTML5 form input type для `<input type="...">`. Возвращает None
`crates/engine/dom/src/lib.rs:164` **enum** `InputType` — HTML5 form input types (HTML Standard §4.10.5). Спека определяет
`crates/engine/dom/src/lib.rs:216` **fn** `parse` — Распарсить значение `type`-атрибута. Case-insensitive по
`crates/engine/dom/src/lib.rs:245` **fn** `as_str`
`crates/engine/dom/src/lib.rs:276` **fn** `is_textual` — Текстовая семантика — поле с буквенным контентом, на котором
`crates/engine/dom/src/lib.rs:286` **fn** `is_button_like` — Кнопочная семантика — submit/reset/button/image, рендерится
`crates/engine/dom/src/lib.rs:296` **struct** `FormInfo` — Данные `<form>` элемента — URL назначения, метод и число полей ввода
`crates/engine/dom/src/lib.rs:314` **enum** `DocumentMode` — Парсинг-режим документа по HTML5 §13.2.6.2 «The insertion mode»
`crates/engine/dom/src/lib.rs:330` **struct** `Document`
`crates/engine/dom/src/lib.rs:356` **fn** `new`
`crates/engine/dom/src/lib.rs:372` **fn** `root`
`crates/engine/dom/src/lib.rs:380` **fn** `mode` — Текущий парсинг-режим. Tree builder выставляет его при
`crates/engine/dom/src/lib.rs:386` **fn** `set_mode` — Установить режим. Использует tree builder при инициализации
`crates/engine/dom/src/lib.rs:401` **fn** `target` — Текущий target — id из URL fragment (без ведущего `#`), к которому
`crates/engine/dom/src/lib.rs:408` **fn** `set_target` — Установить current target (id без `#`). `None` — нет fragment-а в URL
`crates/engine/dom/src/lib.rs:420` **fn** `attach_shadow` — Attach a shadow root to `host` and return its `NodeId`
`crates/engine/dom/src/lib.rs:427` **fn** `shadow_root_of` — Return the shadow root attached to `host`, or `None` if not a shadow host
`crates/engine/dom/src/lib.rs:432` **fn** `is_shadow_host` — Whether `id` is a shadow host (has an attached shadow root)
`crates/engine/dom/src/lib.rs:436` **fn** `get`
`crates/engine/dom/src/lib.rs:440` **fn** `get_mut`
`crates/engine/dom/src/lib.rs:444` **fn** `len`
`crates/engine/dom/src/lib.rs:448` **fn** `is_empty`
`crates/engine/dom/src/lib.rs:460` **fn** `base_href` — HTML5 §4.2.3 — найти первый `<base href="...">` в документе и
`crates/engine/dom/src/lib.rs:472` **fn** `find_first_element` — Найти первый элемент, удовлетворяющий предикату. Pre-order обход
`crates/engine/dom/src/lib.rs:498` **fn** `create_element`
`crates/engine/dom/src/lib.rs:505` **fn** `create_text`
`crates/engine/dom/src/lib.rs:509` **fn** `create_comment`
`crates/engine/dom/src/lib.rs:519` **fn** `create_fragment` — Allocate a `DocumentFragment` node in the arena
`crates/engine/dom/src/lib.rs:527` **fn** `set_template_content` — Register `fragment` as the content container for `template`
`crates/engine/dom/src/lib.rs:533` **fn** `template_content` — Return the content `DocumentFragment` for a `<template>` element, or
`crates/engine/dom/src/lib.rs:537` **fn** `create_doctype`
`crates/engine/dom/src/lib.rs:551` **fn** `append_child` — Append `child` as the last child of `parent`. If `child` already has a parent, it is detached first
`crates/engine/dom/src/lib.rs:559` **fn** `detach` — Remove `node` from its current parent. The node itself stays in the arena and can be re-attached
`crates/engine/dom/src/lib.rs:662` **fn** `check_form_gate` — Гейт отправки форм по sandbox-флагу HTML §7.6.5
`crates/engine/dom/src/lib.rs:683` **fn** `find_ancestor_form` — Найти ближайший предок `<form>` для узла `node`
`crates/engine/dom/src/lib.rs:706` **fn** `collect_dom_form_fields` — Собрать имена и значения submittable-контролов формы из DOM-атрибутов
`crates/engine/dom/src/lib.rs:800` **struct** `AnchorInfo` — Информация об якорной ссылке (`<a href>`), найденной в документе
`crates/engine/dom/src/lib.rs:833` **struct** `FlatTree` — Pre-computed composed tree (flat tree) for Shadow DOM layout traversal
`crates/engine/dom/src/lib.rs:843` **fn** `children_of` — Composed-tree children of `id`
`crates/engine/dom/src/lib.rs:858` **fn** `build_flat_tree` — Build the composed (flat) tree for the document
`crates/engine/dom/src/lib.rs:953` **fn** `check_navigation_gate` — Гейт навигации по sandbox-флагу HTML §7.6.5

## lumen-encoding  (11 symbols)

`crates/engine/encoding/src/decoder.rs:14` **fn** `decode` — Декодирует байты в строку. Алиас для [`decode_to_string`], короткий и
`crates/engine/encoding/src/decoder.rs:21` **fn** `decode_to_string` — То же, что [`decode`], но с явным именем — для случаев, когда из
`crates/engine/encoding/src/detect.rs:16` **fn** `detect` — Главная точка входа. Возвращает кодировку, в которой следует декодировать
`crates/engine/encoding/src/detect.rs:89` **fn** `sniff_meta_charset` — Ищет `<meta charset>` или `<meta http-equiv="Content-Type" content="...; charset=X">`
`crates/engine/encoding/src/ext_impl.rs:17` **struct** `HeuristicDetector` — Детектор кодировок по умолчанию
`crates/engine/encoding/src/lib.rs:39` **enum** `Encoding` — Поддерживаемые в Phase 0 кодировки
`crates/engine/encoding/src/lib.rs:57` **fn** `name` — Стабильное имя кодировки. Используется в API детектора
`crates/engine/encoding/src/lib.rs:77` **fn** `from_label` — Парсит label кодировки (case-insensitive, с алиасами)
`crates/engine/encoding/src/unicode_provider.rs:23` **struct** `Icu4xUnicodeProvider` — ICU4x-провайдер Unicode-операций
`crates/engine/encoding/src/unicode_provider.rs:31` **fn** `new` — Создаёт провайдер с auto-режимом (LSTM/dictionary для CJK/Thai/etc)
`crates/engine/encoding/src/unicode_provider.rs:40` **fn** `new_latin` — Облегчённая версия — только Latin + UAX #14 rules, без LSTM

## lumen-font  (159 symbols)

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
`crates/engine/font/src/face.rs:241` **fn** `hvar` — `HVAR` (Horizontal Metrics Variations) — variation deltas для
`crates/engine/font/src/face.rs:252` **fn** `vvar` — `VVAR` (Vertical Metrics Variations) — зеркало `HVAR` для
`crates/engine/font/src/face.rs:269` **fn** `mvar` — `MVAR` (Metrics Variations) — variation deltas для глобальных
`crates/engine/font/src/face.rs:278` **fn** `glyph` — Удобная обёртка: glyph_id → outline. `None`, если глиф пустой
`crates/engine/font/src/face.rs:297` **fn** `glyph_resolved` — Возвращает глиф с рекурсивно развёрнутыми composite-компонентами:
`crates/engine/font/src/face.rs:326` **fn** `glyph_resolved_with_coords` — Variable-fonts вариант [`Font::glyph_resolved`]: применяет gvar deltas
`crates/engine/font/src/font_registry.rs:19` **struct** `FontRegistry` — Провайдер шрифтов с поддержкой @font-face: системные шрифты + URL-буферы
`crates/engine/font/src/font_registry.rs:28` **fn** `new`
`crates/engine/font/src/font_registry.rs:42` **fn** `register_from_bytes` — Регистрирует шрифт из байт-буфера (TrueType / sfnt после декодирования
`crates/engine/font/src/font_registry.rs:77` **fn** `custom_face_count` — Количество зарегистрированных @font-face face-ов. Для тестов
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
`crates/engine/font/src/os2.rs:106` **fn** `is_italic` — Italic flag из `fsSelection`
`crates/engine/font/src/os2.rs:111` **fn** `is_oblique` — Oblique flag (OS/2 v4+)
`crates/engine/font/src/os2.rs:117` **fn** `is_bold` — Bold flag из `fsSelection`. Не источник истины для веса —
`crates/engine/font/src/os2.rs:121` **fn** `parse`
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
`crates/engine/font/src/woff2.rs:449` **fn** `decode_woff2` — Decode WOFF2 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:646` **fn** `decode_woff1` — Decode WOFF1 bytes into a raw sfnt byte vector
`crates/engine/font/src/woff2.rs:711` **fn** `maybe_decode_font` — If `data` is WOFF2 or WOFF1, decode it and return the raw sfnt bytes

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
`crates/engine/html-parser/src/tree_builder.rs:44` **fn** `parse` — Парсит вход целиком в pull-режиме и возвращает построенный
`crates/engine/html-parser/src/tree_builder.rs:105` **struct** `IncrementalTreeBuilder` — Push-режим tree builder-а: принимает HTML chunk-ами, держит
`crates/engine/html-parser/src/tree_builder.rs:141` **fn** `new` — Создаёт пустой builder в insertion mode `Initial`
`crates/engine/html-parser/src/tree_builder.rs:161` **fn** `feed` — Скармливает chunk push-токенизатору и применяет полученные
`crates/engine/html-parser/src/tree_builder.rs:168` **fn** `feed_bytes` — Вариант [`feed`][Self::feed] для сырых байт
`crates/engine/html-parser/src/tree_builder.rs:175` **fn** `as_doc` — Возвращает ссылку на текущее состояние DOM
`crates/engine/html-parser/src/tree_builder.rs:184` **fn** `finish` — Финализирует ввод. Хвост push-tokenizer-а токенизируется как

## lumen-image  (16 symbols)

`crates/engine/image/src/jpeg/mod.rs:8` **fn** `decode_jpeg`
`crates/engine/image/src/jpeg/mod.rs:39` **struct** `JpegError` — Ошибка декодирования JPEG (обёртка над zune-jpeg)
`crates/engine/image/src/lib.rs:23` **fn** `supported_mime_types` — MIME-типы изображений, которые `decode` умеет декодировать
`crates/engine/image/src/lib.rs:34` **fn** `decode` — Декодирует растровое изображение по сигнатуре первых байтов
`crates/engine/image/src/lib.rs:52` **enum** `ImageError` — Ошибка `decode`
`crates/engine/image/src/lib.rs:88` **struct** `Image` — Декодированное растровое изображение в плотной row-major упаковке
`crates/engine/image/src/lib.rs:98` **fn** `to_rgba8` — Возвращает пиксели в формате RGBA8 (4 байта на пиксель)
`crates/engine/image/src/lib.rs:124` **fn** `resize_bilinear` — Масштабирует `src` до `(dst_w × dst_h)` билинейной интерполяцией
`crates/engine/image/src/lib.rs:176` **fn** `resize_area_avg` — Масштабирует `src` до `(dst_w × dst_h)` усреднением по площади (box filter)
`crates/engine/image/src/lib.rs:235` **enum** `PixelFormat` — Формат пикселя декодированного изображения. Все варианты — 8 бит на канал
`crates/engine/image/src/lib.rs:259` **enum** `DecodeError` — Ошибки декодирования PNG
`crates/engine/image/src/png/mod.rs:8` **fn** `decode_png`
`crates/engine/image/src/webp/mod.rs:24` **struct** `WebpError` — Ошибка декодирования WebP
`crates/engine/image/src/webp/mod.rs:39` **fn** `is_webp` — Проверяет WebP-сигнатуру без полной валидации
`crates/engine/image/src/webp/mod.rs:52` **fn** `decode_webp` — Декодирует WebP-файл в RGBA8 (4 байта на пиксель, row-major)
`crates/engine/image/src/webp/mod.rs:88` **struct** `WebpImageDecoder` — Реализация [`lumen_core::ext::ImageDecoder`] для WebP

## lumen-js  (12 symbols)

`crates/js/src/dom.rs:96` **enum** `NavigateRequest` — Navigation request emitted by JS (`location.href =`, `location.assign()`,
`crates/js/src/dom.rs:130` **fn** `install_dom_api` — Install DOM primitives (`_lumen_*`) and the Web API shim into `ctx`
`crates/js/src/lib.rs:19` **struct** `QuickJsRuntime` — QuickJS-based JS runtime via `rquickjs`
`crates/js/src/lib.rs:62` **fn** `new`
`crates/js/src/lib.rs:90` **fn** `install_dom` — Install DOM Web API globals (`document`, `window`, `console`, etc.) into
`crates/js/src/lib.rs:125` **fn** `take_navigate_request` — Consume any navigation request that JS placed via `location.href =` etc
`crates/js/src/lib.rs:133` **fn** `take_dom_dirty` — Returns `true` if JS mutated the DOM since the last call, clearing the flag
`crates/js/src/lib.rs:142` **fn** `take_raf_pending` — Returns `true` if `requestAnimationFrame` was called since the last call,
`crates/js/src/lib.rs:151` **fn** `take_timer_wakeup` — Take the next timer wakeup as Unix epoch ms, clearing the stored value
`crates/js/src/lib.rs:160` **fn** `update_layout_rects` — Replace the layout bounding-rect table with a fresh snapshot
`crates/js/src/lib.rs:168` **fn** `update_viewport_size` — Update the viewport dimensions
`crates/js/src/lib.rs:177` **fn** `take_lazy_image_requests` — Drain lazy image load requests queued by `_lumen_request_lazy_image_load` in JS

## lumen-knowledge  (37 symbols)

`crates/knowledge/src/fts.rs:28` **struct** `SearchHit` — Результат полнотекстового поиска
`crates/knowledge/src/fts.rs:43` **struct** `HistoryFts` — FTS5-индекс над `(url, title, text)`. Открывается отдельной БД-файлом
`crates/knowledge/src/fts.rs:54` **fn** `open`
`crates/knowledge/src/fts.rs:60` **fn** `open_in_memory`
`crates/knowledge/src/fts.rs:87` **fn** `index` — Добавить или обновить запись в индексе. `rowid` обычно совпадает
`crates/knowledge/src/fts.rs:111` **fn** `unindex` — Удалить запись по rowid
`crates/knowledge/src/fts.rs:129` **fn** `search` — Полнотекстовый поиск по `text` с ранжированием bm25. `query` —
`crates/knowledge/src/fts.rs:167` **fn** `clear` — Полная очистка индекса
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

## lumen-layout  (296 symbols)

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
`crates/engine/layout/src/animation.rs:743` **struct** `AnimationScheduler` — CSS Animations L1 §3 — scheduler that maps `@keyframes` to interpolated
`crates/engine/layout/src/animation.rs:749` **fn** `new`
`crates/engine/layout/src/animation.rs:759` **fn** `sync` — Register or refresh animations for `node` based on its computed style
`crates/engine/layout/src/animation.rs:780` **fn** `remove_node` — Remove all animation state for `node` (e.g. when the node is removed from the DOM)
`crates/engine/layout/src/animation.rs:790` **fn** `tick` — Compute per-node animated style overrides for the current frame
`crates/engine/layout/src/animation.rs:1081` **struct** `TransitionScheduler` — CSS Transitions L1 §2 — detects property value changes and interpolates
`crates/engine/layout/src/animation.rs:1087` **fn** `new`
`crates/engine/layout/src/animation.rs:1093` **fn** `sync` — Detect value changes between `old` and `new` style for properties listed
`crates/engine/layout/src/animation.rs:1162` **fn** `remove_node` — Remove all transition state for `node` (called when node leaves DOM)
`crates/engine/layout/src/animation.rs:1168` **fn** `tick` — Compute interpolated style overrides for the current frame
`crates/engine/layout/src/box_tree.rs:55` **enum** `FormControlKind` — Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
`crates/engine/layout/src/box_tree.rs:88` **struct** `ImageRequest` — Запрос на предзагрузку изображения: URL после picking-а по
`crates/engine/layout/src/box_tree.rs:103` **fn** `collect_image_requests` — Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов
`crates/engine/layout/src/box_tree.rs:123` **fn** `collect_background_image_requests` — Обходит готовое layout-дерево и возвращает уникальные URL-ы из
`crates/engine/layout/src/box_tree.rs:223` **struct** `LayoutBox`
`crates/engine/layout/src/box_tree.rs:235` **struct** `InlineSegment` — Отрезок inline-контента с собственным стилем (до layout)
`crates/engine/layout/src/box_tree.rs:266` **enum** `PseudoKind` — Marks an inline segment as the target of a CSS structural pseudo-element
`crates/engine/layout/src/box_tree.rs:284` **struct** `InlineFrag` — Позиционированный текстовый фрагмент в строке (после layout)
`crates/engine/layout/src/box_tree.rs:309` **enum** `BoxKind`
`crates/engine/layout/src/box_tree.rs:382` **fn** `layout`
`crates/engine/layout/src/box_tree.rs:396` **fn** `layout_measured`
`crates/engine/layout/src/box_tree.rs:407` **fn** `layout_measured_hyp` — Layout with a real hyphenation provider (for `hyphens: auto`)
`crates/engine/layout/src/box_tree.rs:4228` **fn** `apply_container_styles` — CSS Container Queries L1: second-pass after layout
`crates/engine/layout/src/counters.rs:33` **type** `CounterSnapshot` — Per-element counter stacks snapshot
`crates/engine/layout/src/counters.rs:37` **type** `CounterMap` — Maps each element `NodeId` to its counter snapshot (after own reset/increment,
`crates/engine/layout/src/counters.rs:90` **fn** `precompute_counters` — Build a `CounterMap` by walking the DOM in pre-order
`crates/engine/layout/src/counters.rs:152` **fn** `format_counter` — Format a counter integer value according to the given `list-style-type` keyword
`crates/engine/layout/src/lib.rs:74` **trait** `TextMeasurer` — Интерфейс измерения ширины символов для line wrapping
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
`crates/engine/layout/src/property_trees.rs:200` **struct** `TransformNode` — Узел TransformTree. Хранит локальный transform; accumulated transform
`crates/engine/layout/src/property_trees.rs:210` **struct** `TransformTree` — Дерево transform-преобразований. Корень — identity
`crates/engine/layout/src/property_trees.rs:216` **fn** `empty` — Sprint 0 stub: только root с identity
`crates/engine/layout/src/property_trees.rs:226` **fn** `root`
`crates/engine/layout/src/property_trees.rs:233` **struct** `ScrollNode` — Узел ScrollTree. Хранит scrollable rect и текущий scroll offset
`crates/engine/layout/src/property_trees.rs:246` **struct** `ScrollTree`
`crates/engine/layout/src/property_trees.rs:251` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:263` **fn** `root`
`crates/engine/layout/src/property_trees.rs:271` **struct** `EffectNode` — Узел EffectTree. Хранит opacity / filter / blend-mode — всё, что
`crates/engine/layout/src/property_trees.rs:298` **struct** `EffectTree`
`crates/engine/layout/src/property_trees.rs:303` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:309` **fn** `root`
`crates/engine/layout/src/property_trees.rs:317` **struct** `ClipNode` — Узел ClipTree. Хранит clip rectangle в локальных координатах (т.е
`crates/engine/layout/src/property_trees.rs:326` **struct** `ClipTree`
`crates/engine/layout/src/property_trees.rs:331` **fn** `empty`
`crates/engine/layout/src/property_trees.rs:341` **fn** `root`
`crates/engine/layout/src/property_trees.rs:351` **struct** `PropertyTrees` — 4-deep property trees — единая поверхность, которую layout
`crates/engine/layout/src/property_trees.rs:360` **fn** `empty` — Sprint 0 stub: все 4 дерева — empty roots
`crates/engine/layout/src/property_trees.rs:371` **fn** `build_stub` — Совместимость с Sprint 0: пустые root-only деревья. Используется
`crates/engine/layout/src/property_trees.rs:398` **fn** `build` — Построение property trees из layout-дерева (P1 п.2B)
`crates/engine/layout/src/property_trees.rs:429` **fn** `compute_local_transform` — Вычислить локальную transform-матрицу элемента. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:468` **fn** `forward_box_transform` — Forward-матрица бокса в viewport-координатах. CSS Transforms L1 §13:
`crates/engine/layout/src/property_trees.rs:524` **fn** `transform_fns_to_matrix` — Build the forward transform matrix from a list of TransformFn with a pivot point
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
`crates/engine/layout/src/stacking.rs:246` **fn** `box_can_own_stacking_context` — Анонимные / неучаствующие в layout box-ы не имеют DOM-элемента, к
`crates/engine/layout/src/stacking.rs:288` **struct** `PaintOrder` — Painting order — линейная последовательность пар `(StackingContextId,
`crates/engine/layout/src/stacking.rs:308` **fn** `from_tree` — Строит painting order по CSS 2.1 Appendix E + CSS Painting Order L3 §3
`crates/engine/layout/src/stacking.rs:316` **fn** `len`
`crates/engine/layout/src/stacking.rs:320` **fn** `is_empty`
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
`crates/engine/layout/src/style.rs:354` **struct** `FontStretch` — CSS Fonts Module L4 §2.5 — `font-stretch`. Inherited
`crates/engine/layout/src/style.rs:391` **struct** `FontWeight` — CSS Fonts Module L4 §2.4 — `font-weight`. Inherited
`crates/engine/layout/src/style.rs:397` **fn** `is_bold`
`crates/engine/layout/src/style.rs:413` **struct** `FontVariationSetting` — CSS Fonts L4 §7 — одна запись `font-variation-settings`
`crates/engine/layout/src/style.rs:429` **struct** `TextDecorationLine` — Набор активных линий `text-decoration` для элемента
`crates/engine/layout/src/style.rs:451` **enum** `TextDecorationStyle` — CSS Text Decoration L3 §2.2 — `text-decoration-style`. Стиль штриха
`crates/engine/layout/src/style.rs:464` **fn** `parse` — Парсит одиночный keyword. Возвращает `None` для невалидных и для
`crates/engine/layout/src/style.rs:494` **enum** `TextDecorationThickness` — CSS Text Decoration L3 §2.3 — `text-decoration-thickness`. Толщина
`crates/engine/layout/src/style.rs:513` **enum** `TextEmphasisStyle` — CSS Text Decoration L4 §5.3 — `text-emphasis-style`. Форма emphasis-marks
`crates/engine/layout/src/style.rs:528` **enum** `TextEmphasisShape`
`crates/engine/layout/src/style.rs:545` **enum** `TextEmphasisPosition` — CSS Text Decoration L4 §5.5 — `text-emphasis-position`. Сторона
`crates/engine/layout/src/style.rs:554` **fn** `is_over`
`crates/engine/layout/src/style.rs:564` **enum** `TextUnderlinePosition` — CSS Text Decoration L3 §6.1 / L4 §5.1 — `text-underline-position`
`crates/engine/layout/src/style.rs:583` **enum** `ForcedColorAdjust` — CSS Color Adjustment L1 §4 — `forced-color-adjust`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:598` **enum** `ColorScheme` — CSS Color Adjustment L1 §3 — `color-scheme`. Inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:617` **struct** `Color`
`crates/engine/layout/src/style.rs:647` **enum** `ColorSpace` — CSS Color L4 §10 — цветовое пространство для wide-gamut значений
`crates/engine/layout/src/style.rs:657` **struct** `ColorFloat` — Wide-gamut цвет с float-каналами [0..1 для in-gamut, за пределами — out-of-gamut]
`crates/engine/layout/src/style.rs:668` **fn** `to_srgb_color` — Конвертирует в sRGB u8, применяя матрицу цветового пространства и гамму
`crates/engine/layout/src/style.rs:698` **fn** `to_linear_srgb` — Линейные sRGB-каналы [0..1] для прямой передачи в GPU без квантизации
`crates/engine/layout/src/style.rs:765` **enum** `CssColor` — CSS Color L4 §4.2 — типизированное цветовое значение каскада
`crates/engine/layout/src/style.rs:773` **fn** `resolve` — Разрешает значение в sRGB u8 Color. `Wide` конвертируется через матрицу
`crates/engine/layout/src/style.rs:783` **fn** `to_color_opt` — Конвертирует в `Color`, минуя `current_color`. `CurrentColor` → `None`
`crates/engine/layout/src/style.rs:792` **fn** `resolve_linear` — Линейные sRGB-каналы для прямой передачи в GPU
`crates/engine/layout/src/style.rs:816` **enum** `BorderStyle` — Стиль линии CSS border. None = рамка не отображается (как `display: none`)
`crates/engine/layout/src/style.rs:826` **fn** `is_visible`
`crates/engine/layout/src/style.rs:839` **enum** `OutlineStyle` — CSS Basic UI L4 §5.3 — `outline-style`. Включает все `<border-style>`
`crates/engine/layout/src/style.rs:849` **fn** `is_visible`
`crates/engine/layout/src/style.rs:862` **enum** `OutlineColor` — CSS Basic UI L4 §5.4 — `outline-color`. Помимо явного цвета поддерживает
`crates/engine/layout/src/style.rs:873` **enum** `BreakValue` — CSS Fragmentation L3 §3.1 — break-before / break-after / break-inside
`crates/engine/layout/src/style.rs:896` **enum** `BoxSizing` — CSS `box-sizing`. Определяет, что именно задаёт `width` / `height`:
`crates/engine/layout/src/style.rs:908` **enum** `Position` — CSS Positioned Layout L3 §3 — `position`. Не наследуется
`crates/engine/layout/src/style.rs:918` **fn** `parse`
`crates/engine/layout/src/style.rs:934` **enum** `FloatSide` — CSS 2.1 §9.5.1 — `float`. Не наследуется. `Left`/`Right` выводят
`crates/engine/layout/src/style.rs:943` **fn** `parse` — Parses `float` keyword value
`crates/engine/layout/src/style.rs:955` **fn** `is_none` — Returns `true` for `float: none`
`crates/engine/layout/src/style.rs:963` **enum** `ClearSide` — CSS 2.1 §9.5.2 — `clear`. Не наследуется. Указывает, мимо
`crates/engine/layout/src/style.rs:973` **fn** `parse` — Parses `clear` keyword value
`crates/engine/layout/src/style.rs:989` **enum** `Isolation` — CSS Compositing & Blending L1 §2.1 — `isolation`. Не наследуется
`crates/engine/layout/src/style.rs:996` **fn** `parse`
`crates/engine/layout/src/style.rs:1010` **enum** `MixBlendMode` — CSS Compositing & Blending L1 §3.1 — `mix-blend-mode`. Не наследуется
`crates/engine/layout/src/style.rs:1032` **fn** `parse`
`crates/engine/layout/src/style.rs:1070` **enum** `VerticalAlign` — CSS Inline Layout / CSS 2.1 §10.8.1 — `vertical-align`. Не наследуется
`crates/engine/layout/src/style.rs:1091` **fn** `parse_keyword` — Парсит keyword-формы vertical-align. Не покрывает `<length>` /
`crates/engine/layout/src/style.rs:1116` **enum** `TimingFunction` — CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations
`crates/engine/layout/src/style.rs:1154` **struct** `LinearEasingPoint` — CSS Easing L2 §2.4 — одна control-точка функции `linear(...)`
`crates/engine/layout/src/style.rs:1173` **fn** `parse` — Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
`crates/engine/layout/src/style.rs:1240` **fn** `parse_list` — CSS Transitions/Animations L1 — comma-list of timing functions
`crates/engine/layout/src/style.rs:1259` **fn** `progress` — CSS Easing L1 §2 — компьютация eased progress
`crates/engine/layout/src/style.rs:1515` **enum** `StepPosition` — CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`
`crates/engine/layout/src/style.rs:1533` **enum** `IterationCount` — CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
`crates/engine/layout/src/style.rs:1545` **fn** `parse`
`crates/engine/layout/src/style.rs:1558` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1568` **enum** `AnimationDirection` — CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`
`crates/engine/layout/src/style.rs:1581` **fn** `parse`
`crates/engine/layout/src/style.rs:1591` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1603` **enum** `AnimationFillMode` — CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`
`crates/engine/layout/src/style.rs:1616` **fn** `parse`
`crates/engine/layout/src/style.rs:1626` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1636` **enum** `AnimationPlayState` — CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`
`crates/engine/layout/src/style.rs:1645` **fn** `parse`
`crates/engine/layout/src/style.rs:1653` **fn** `parse_list`
`crates/engine/layout/src/style.rs:1671` **enum** `CssWideKeyword` — CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству
`crates/engine/layout/src/style.rs:1681` **fn** `parse_css_wide_keyword` — ASCII case-insensitive проверка значения декларации на CSS-wide keyword
`crates/engine/layout/src/style.rs:1697` **struct** `ComputedStyle`
`crates/engine/layout/src/style.rs:2247` **enum** `Content` — CSS Content L3 — value свойства `content`
`crates/engine/layout/src/style.rs:2260` **enum** `ContentItem`
`crates/engine/layout/src/style.rs:2289` **enum** `ScrollbarWidth` — CSS Scrollbars 1 — `scrollbar-width`. Inherited
`crates/engine/layout/src/style.rs:2300` **fn** `parse`
`crates/engine/layout/src/style.rs:2312` **enum** `ScrollbarGutter` — CSS Overflow L3 — `scrollbar-gutter`
`crates/engine/layout/src/style.rs:2323` **fn** `parse`
`crates/engine/layout/src/style.rs:2342` **enum** `ListStyleType` — CSS Lists L3 §2.1 — markers для list items
`crates/engine/layout/src/style.rs:2369` **fn** `parse`
`crates/engine/layout/src/style.rs:2389` **enum** `ListStylePosition` — CSS Lists L3 §2.3 — `list-style-position`
`crates/engine/layout/src/style.rs:2398` **fn** `parse`
`crates/engine/layout/src/style.rs:2409` **enum** `OverflowWrap` — CSS Text L3 §5.2 — `overflow-wrap`
`crates/engine/layout/src/style.rs:2420` **fn** `parse`
`crates/engine/layout/src/style.rs:2434` **enum** `LineBreak` — CSS Text L3 §5.2 — `line-break`. Inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2445` **enum** `WordBreak` — CSS Text L3 §5.1 — `word-break`
`crates/engine/layout/src/style.rs:2457` **fn** `parse`
`crates/engine/layout/src/style.rs:2470` **enum** `Hyphens` — CSS Text L3 §6 — `hyphens`
`crates/engine/layout/src/style.rs:2483` **fn** `parse`
`crates/engine/layout/src/style.rs:2497` **enum** `TouchAction` — CSS Pointer Events L3 / Touch Events — `touch-action`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2515` **enum** `Appearance` — CSS Basic UI L4 §5 — `appearance`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2526` **enum** `PointerEvents` — CSS Pointer Events L1. Default `auto`
`crates/engine/layout/src/style.rs:2540` **fn** `parse`
`crates/engine/layout/src/style.rs:2560` **enum** `Resize` — CSS Basic UI L4 §6 — `resize`. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2574` **struct** `ContainFlags` — CSS Containment L3 §3 — `contain` property
`crates/engine/layout/src/style.rs:2591` **enum** `ContentVisibility` — CSS Containment L3 §4 — `content-visibility`. NOT inherited. Initial: `Visible`
`crates/engine/layout/src/style.rs:2600` **enum** `ContainerType` — CSS Container Queries L1 §3.1 — `container-type`. NOT inherited. Initial: `Normal`
`crates/engine/layout/src/style.rs:2610` **struct** `ContainerContext` — Resolved container dimensions, passed during style re-computation for container queries
`crates/engine/layout/src/style.rs:2624` **fn** `evaluate_container_condition` — Evaluates a raw @container condition string against a `ContainerContext`
`crates/engine/layout/src/style.rs:2702` **fn** `apply_container_rules` — Applies matching `@container` rules from `sheet` to `style`
`crates/engine/layout/src/style.rs:2747` **enum** `ShapeOutside` — CSS Shapes L1 §3 — `shape-outside` value. NOT inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2756` **enum** `OffsetRotate` — CSS Motion Path L1 §3 — `offset-rotate`. NOT inherited. Initial: `Auto`
`crates/engine/layout/src/style.rs:2767` **enum** `PrintColorAdjust` — CSS Color Adjustment L1 §5 — `print-color-adjust`. NOT inherited. Initial: `Economy`
`crates/engine/layout/src/style.rs:2775` **enum** `FontSizeAdjust` — CSS Fonts L5 §4 — `font-size-adjust`. Inherited. Initial: `None`
`crates/engine/layout/src/style.rs:2784` **enum** `WritingMode` — CSS Writing Modes L3 §2.1 — `writing-mode`. Inherited. Initial: `HorizontalTb`
`crates/engine/layout/src/style.rs:2801` **enum** `TextOrientation` — CSS Writing Modes L3 §6.5 — `text-orientation`. Inherited. Initial: `Mixed`
`crates/engine/layout/src/style.rs:2813` **enum** `UserSelect` — CSS UI L4 §6.2 — `user-select`. Inherited
`crates/engine/layout/src/style.rs:2823` **fn** `parse`
`crates/engine/layout/src/style.rs:2837` **enum** `ScrollBehavior` — CSS Overflow L3 — `scroll-behavior`. Inherited
`crates/engine/layout/src/style.rs:2845` **struct** `ScrollSnapType` — CSS Scroll Snap L1 §3.1 — `scroll-snap-type: none | <axis> [mandatory | proximity]`
`crates/engine/layout/src/style.rs:2851` **enum** `ScrollSnapAxis`
`crates/engine/layout/src/style.rs:2862` **enum** `ScrollSnapStrictness`
`crates/engine/layout/src/style.rs:2870` **struct** `ScrollSnapAlign` — CSS Scroll Snap L1 §6.1 — `scroll-snap-align: none | <axis-keyword>{1,2}`
`crates/engine/layout/src/style.rs:2876` **enum** `ScrollSnapAlignKeyword`
`crates/engine/layout/src/style.rs:2885` **enum** `ScrollSnapStop`
`crates/engine/layout/src/style.rs:2893` **enum** `OverscrollBehavior` — CSS Overscroll Behavior L1 §2 — `overscroll-behavior: auto | contain | none`
`crates/engine/layout/src/style.rs:2901` **fn** `parse`
`crates/engine/layout/src/style.rs:2916` **enum** `ParsedGradient` — CSS Images L3/L4 §3.3/§3.7 — parsed linear / radial / conic gradient
`crates/engine/layout/src/style.rs:2956` **enum** `BackgroundImage` — CSS Backgrounds L3 §3.1 — `background-image` value
`crates/engine/layout/src/style.rs:2968` **enum** `BackgroundRepeat` — CSS Backgrounds L3 §3.4 — `background-repeat`
`crates/engine/layout/src/style.rs:2979` **fn** `parse`
`crates/engine/layout/src/style.rs:2994` **enum** `BackgroundSize` — CSS Backgrounds L3 §3.5 — `background-size`
`crates/engine/layout/src/style.rs:3005` **enum** `BackgroundAttachment` — CSS Backgrounds L3 §3.6 — `background-attachment`
`crates/engine/layout/src/style.rs:3013` **fn** `parse`
`crates/engine/layout/src/style.rs:3034` **enum** `BackgroundOrigin` — CSS Backgrounds L3 §3.7 — `background-origin`. Non-inherited
`crates/engine/layout/src/style.rs:3045` **fn** `parse`
`crates/engine/layout/src/style.rs:3068` **enum** `BackgroundClip` — CSS Backgrounds L3 §3.8 — `background-clip`. Non-inherited
`crates/engine/layout/src/style.rs:3082` **fn** `parse`
`crates/engine/layout/src/style.rs:3098` **struct** `BackgroundLayer` — CSS Backgrounds L3 §3 — один фоновый слой. Первый в Vec = верхний (рисуется последним)
`crates/engine/layout/src/style.rs:3133` **enum** `ObjectFit` — CSS Images L3 §5.5 — `object-fit`. Применяется к replaced elements
`crates/engine/layout/src/style.rs:3154` **fn** `parse`
`crates/engine/layout/src/style.rs:3174` **enum** `ImageRendering` — CSS Images L3 §6.1 — `image-rendering`. Hint для движка о том, как
`crates/engine/layout/src/style.rs:3194` **fn** `parse`
`crates/engine/layout/src/style.rs:3218` **enum** `TextWrapMode` — CSS Text Module Level 4 §6.4.1 — `text-wrap-mode`. Inherited
`crates/engine/layout/src/style.rs:3227` **fn** `parse`
`crates/engine/layout/src/style.rs:3245` **enum** `TextWrapStyle` — CSS Text Module Level 4 §6.4.2 — `text-wrap-style`. Inherited
`crates/engine/layout/src/style.rs:3258` **fn** `parse`
`crates/engine/layout/src/style.rs:3274` **enum** `FlexDirection` — CSS Flexbox L1 §5.1 — `flex-direction`. Non-inherited
`crates/engine/layout/src/style.rs:3287` **fn** `parse`
`crates/engine/layout/src/style.rs:3303` **enum** `FlexWrap` — CSS Flexbox L1 §5.2 — `flex-wrap`. Non-inherited
`crates/engine/layout/src/style.rs:3314` **fn** `parse`
`crates/engine/layout/src/style.rs:3329` **enum** `FlexBasis` — CSS Flexbox L1 §7.3 — `flex-basis`. Non-inherited
`crates/engine/layout/src/style.rs:3340` **fn** `parse`
`crates/engine/layout/src/style.rs:3354` **enum** `GridTrackSize` — CSS Grid Layout L1 §7.2 — sizing function for a grid track
`crates/engine/layout/src/style.rs:3373` **fn** `resolve_fixed` — Resolve to a concrete pixel size given container width, em, viewport
`crates/engine/layout/src/style.rs:3382` **fn** `is_fr` — True for fractional tracks
`crates/engine/layout/src/style.rs:3387` **fn** `fr` — Extract fr value
`crates/engine/layout/src/style.rs:3425` **fn** `parse_track_list` — Parse a track-list value string into a Vec of GridTrackSize
`crates/engine/layout/src/style.rs:3492` **enum** `GridAutoFlow` — CSS Grid Layout L1 §8.5 — `grid-auto-flow`. Non-inherited
`crates/engine/layout/src/style.rs:3505` **fn** `parse`
`crates/engine/layout/src/style.rs:3519` **enum** `GridLine` — CSS Grid Layout L1 §8.3 — a grid-line reference for grid-column-start,
`crates/engine/layout/src/style.rs:3533` **fn** `parse`
`crates/engine/layout/src/style.rs:3568` **enum** `PositionComponent` — Одна компонента `object-position`. Length-варианты резолвятся в px
`crates/engine/layout/src/style.rs:3581` **fn** `resolve` — Резолв в финальный px-offset относительно левого/верхнего края
`crates/engine/layout/src/style.rs:3592` **struct** `ObjectPosition` — CSS Images L3 §5.5 — `object-position` (две компоненты, x + y)
`crates/engine/layout/src/style.rs:3629` **fn** `parse` — CSS Values L4 §9.4 — `<position>` для object-position. Phase 0
`crates/engine/layout/src/style.rs:3731` **enum** `AlignValue` — CSS Box Alignment L3 §6.1 — значения для align-/justify- свойств
`crates/engine/layout/src/style.rs:3758` **fn** `parse`
`crates/engine/layout/src/style.rs:3780` **enum** `ClipPath` — CSS Masking L1 §3.5 — basic-shapes для `clip-path`. Phase 0
`crates/engine/layout/src/style.rs:3803` **enum** `TransformFn` — CSS Transforms L1 §11 — функции `transform`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:3821` **enum** `FilterFn` — CSS Filter Effects L1 §3 — функции `filter`. Phase 0 поддерживает
`crates/engine/layout/src/style.rs:3854` **struct** `GradientStop` — CSS Images L3 §3.4 — единичный `<color-stop>` градиента
`crates/engine/layout/src/style.rs:3864` **fn** `outline_used_width` — CSS 2.1 §17.6.1 / Basic UI L4 §5.2 — **used** value `outline-width`
`crates/engine/layout/src/style.rs:3875` **fn** `text_rendering_eq` — Два стиля рендерят текст одинаково (цвет, размер, интерлиньяж, начертание,
`crates/engine/layout/src/style.rs:3892` **fn** `root` — Стартовые значения для корня документа
`crates/engine/layout/src/style.rs:4108` **fn** `compute_style`
`crates/engine/layout/src/style.rs:4724` **fn** `compute_pseudo_element_style` — Вычисляет стиль для псевдоэлемента `::before` или `::after` элемента `node`
`crates/engine/layout/src/style.rs:4911` **fn** `validate_against_syntax` — CSS Properties and Values L1 §2 — упрощённая валидация значения
`crates/engine/layout/src/style.rs:7313` **fn** `parse_font_family` — Парсит `font-family: a, "b c", d` в Vec<String>. Запятые разделяют
`crates/engine/layout/src/style.rs:7376` **fn** `parse_font_variation_settings` — Парсит CSS `font-variation-settings` (CSS Fonts L4 §7)
`crates/engine/layout/src/style.rs:7461` **fn** `set_cq_context` — Sets the nearest-container size for `cq*` unit resolution during the container re-layout pass
`crates/engine/layout/src/style.rs:7466` **fn** `clear_cq_context` — Clears the `cq*` context after the container re-layout pass completes
`crates/engine/layout/src/style.rs:7474` **enum** `LengthOrAuto` — CSS `<length> | auto` — для margin и offset-свойств, где `auto` имеет
`crates/engine/layout/src/style.rs:7482` **fn** `is_auto`
`crates/engine/layout/src/style.rs:7489` **fn** `to_px_opt` — Returns the raw pixel value for `Length::Px` variants; `Auto` and all
`crates/engine/layout/src/style.rs:7499` **fn** `resolve` — Резолвит в пиксели. `Auto` → `None`; нерезолвируемый `%` → `None`
`crates/engine/layout/src/style.rs:7507` **fn** `resolve_or_zero` — Резолвит в пиксели; для `Auto` и нерезолвируемых значений → 0.0
`crates/engine/layout/src/style.rs:7518` **enum** `Length` — Типизированная длина CSS до резолва в пиксели
`crates/engine/layout/src/style.rs:7583` **enum** `CalcNode` — CSS Values L4 §10 — AST `calc()`-выражения. Хранится как двоичное дерево
`crates/engine/layout/src/style.rs:7612` **enum** `MathFn` — CSS Values L4 §10.7-10.9 — научные math-функции. Имена case-insensitive
`crates/engine/layout/src/style.rs:7641` **enum** `RoundStrategy` — CSS Values L4 §10.5.1 — стратегия округления для `round()`
`crates/engine/layout/src/style.rs:7665` **fn** `resolve` — Резолвит выражение в `f32`-пиксели по тем же правилам, что
`crates/engine/layout/src/style.rs:7863` **fn** `resolve` — Возвращает длину в пикселях. `em_basis` — fs, относительно которого
`crates/engine/layout/src/style.rs:7903` **fn** `is_intrinsic` — Returns `true` if this is an intrinsic sizing keyword (min-content,
`crates/engine/layout/src/style.rs:7909` **fn** `resolve_or_zero` — Резолвит с `cb_width` как percent_basis; возвращает 0.0 при неудаче
`crates/engine/layout/src/style.rs:7915` **fn** `px` — Извлекает пиксельное значение для уже-разрешённых `Px`-значений
`crates/engine/layout/src/style.rs:8070` **fn** `parse_length`
`crates/engine/layout/src/style.rs:12551` **fn** `parse_transform_list` — Парсит `<transform-list>` — последовательность `func(args)` через
`crates/engine/layout/src/style.rs:13370` **fn** `parse_grid_template_areas` — CSS Grid L1 §7.3 — parse `grid-template-areas` value
`crates/engine/layout/src/style.rs:13450` **fn** `parse_background_gradient` — CSS Images L3/L4 §3.3/§3.7 — parses color stops from a CSS gradient string
`crates/engine/layout/src/style.rs:13642` **fn** `parse_gradient_stops` — The leading direction / angle / shape argument (e.g. `to right`,
`crates/engine/layout/src/style.rs:14216` **fn** `parse_color`

## lumen-network  (164 symbols)

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
`crates/network/src/dns.rs:22` **struct** `SystemDnsResolver` — DNS-резолвер на основе системного getaddrinfo (через std::net)
`crates/network/src/doh.rs:46` **fn** `encode_query` — Закодировать стандартный DNS query — header + одна question. RD=1
`crates/network/src/doh.rs:100` **fn** `decode_answer_ips` — Распакованный DNS-ответ — без CNAME-цепочек, только IP-адреса из
`crates/network/src/doh.rs:249` **fn** `base64url_encode` — Закодировать байты в base64url **без padding** — RFC 8484 §4.1 явно
`crates/network/src/doh.rs:302` **struct** `DohResolver` — DNS-over-HTTPS резолвер
`crates/network/src/doh.rs:310` **fn** `new` — `endpoint` — URL DoH сервера со схемой `https://`. `transport` —
`crates/network/src/dot.rs:62` **fn** `frame_query` — Обернуть DNS message в two-octet length prefix: `[u16 BE len][msg]`
`crates/network/src/dot.rs:77` **fn** `read_framed_message` — Прочитать ОДНО framed DNS message из stream-а: 2 байта BE length,
`crates/network/src/dot.rs:107` **fn** `query_over_stream` — Послать ОДИН DNS query (AAAA или A — определяется `qtype`) по уже
`crates/network/src/dot.rs:140` **struct** `DotResolver` — DNS-over-TLS резолвер
`crates/network/src/dot.rs:149` **fn** `new` — Базовый конструктор. `server_name` — TLS SNI/cert host;
`crates/network/src/dot.rs:159` **fn** `cloudflare` — Cloudflare `1.1.1.1:853` с SNI `one.one.one.one`
`crates/network/src/dot.rs:167` **fn** `google` — Google Public DNS `8.8.8.8:853` с SNI `dns.google`
`crates/network/src/dot.rs:175` **fn** `quad9` — Quad9 `9.9.9.9:853` с SNI `dns.quad9.net`
`crates/network/src/h2/conn.rs:51` **type** `H2Response` — Decoded HTTP response from an H2 fetch: `(status, headers, body)`
`crates/network/src/h2/conn.rs:71` **struct** `H2Conn` — Stateful HTTP/2 client connection
`crates/network/src/h2/conn.rs:95` **fn** `connect` — Establish an HTTP/2 connection over `stream`
`crates/network/src/h2/conn.rs:218` **fn** `fetch` — Perform a single HTTP/2 request and collect the response
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
`crates/network/src/http_cache.rs:23` **struct** `CacheControl` — Parsed subset of `Cache-Control` response directives
`crates/network/src/http_cache.rs:38` **fn** `parse` — Parse `Cache-Control` response header value
`crates/network/src/http_cache.rs:58` **fn** `max_age_secs` — Effective freshness lifetime. s-maxage takes precedence over max-age
`crates/network/src/http_cache.rs:85` **struct** `CacheEntry` — A single stored HTTP response
`crates/network/src/http_cache.rs:105` **fn** `is_fresh` — True if the entry is fresh and can be served without revalidation
`crates/network/src/http_cache.rs:114` **fn** `conditional_headers` — Build conditional GET headers to revalidate this entry
`crates/network/src/http_cache.rs:139` **struct** `HttpCache` — Thread-safe in-memory HTTP response cache (RFC 7234)
`crates/network/src/http_cache.rs:145` **fn** `new` — Create an empty cache
`crates/network/src/http_cache.rs:157` **fn** `lookup` — Look up a cached response for `url`
`crates/network/src/http_cache.rs:179` **fn** `get` — Get the cache entry for `url` if it exists (fresh or stale)
`crates/network/src/http_cache.rs:197` **fn** `store` — Store a successful (2xx) response in the cache
`crates/network/src/http_cache.rs:253` **fn** `revalidate` — Update an existing entry after a 304 Not Modified response
`crates/network/src/http_cache.rs:281` **fn** `len` — Number of entries currently stored
`crates/network/src/http_cache.rs:286` **fn** `is_empty`
`crates/network/src/http_cache.rs:301` **struct** `CacheEntrySnapshot` — Owned snapshot of a cache entry returned by `HttpCache::get`
`crates/network/src/http_cache.rs:315` **enum** `CacheLookup` — `CacheLookup` is unused externally; we use `get()` which returns `Option<CacheEntrySnapshot>`
`crates/network/src/lib.rs:1233` **struct** `HttpClient` — HTTP/1.1 + HTTPS клиент
`crates/network/src/lib.rs:1251` **fn** `new`
`crates/network/src/lib.rs:1271` **fn** `with_sink` — Подключить EventSink. По умолчанию sink-а нет (события не эмитятся)
`crates/network/src/lib.rs:1282` **fn** `with_filter` — Подключить RequestFilter. По умолчанию фильтра нет — `fetch` всегда
`crates/network/src/lib.rs:1294` **fn** `with_interceptor` — Подключить Service Worker перехватчик fetch-запросов. Проверяется
`crates/network/src/lib.rs:1303` **fn** `with_pool` — Подключить shared `ConnectionPool`. По умолчанию у каждого `HttpClient`
`crates/network/src/lib.rs:1313` **fn** `with_h2_pool` — Подключить shared `H2Pool` (RFC 9113 §9.1.1). По умолчанию HTTP/2
`crates/network/src/lib.rs:1322` **fn** `with_dns_resolver` — Подключить DNS-резолвер. По умолчанию — `SystemDnsResolver` (через
`crates/network/src/lib.rs:1339` **fn** `with_hsts` — Подключить HSTS-store (RFC 6797). По умолчанию — нет:
`crates/network/src/lib.rs:1355` **fn** `with_credentials` — Подключить credential-провайдер для HTTP authentication (RFC 7235 /
`crates/network/src/lib.rs:1366` **fn** `with_tab` — Указать `TabId`, который попадёт в каждое emit-ое событие. В Phase 0
`crates/network/src/lib.rs:1386` **fn** `with_mixed_content_policy` — Подключить mixed-content policy (W3C Mixed Content §5). По умолчанию
`crates/network/src/lib.rs:1410` **fn** `with_content_decoder` — Зарегистрировать `ContentDecoder` для одного encoding. Декодер попадает
`crates/network/src/lib.rs:1456` **fn** `with_cors_cache` — Запросить только диапазон байт ресурса (RFC 7233). Если сервер
`crates/network/src/lib.rs:1475` **fn** `with_http_cache` — Подключить HTTP response cache (RFC 7234)
`crates/network/src/lib.rs:1510` **fn** `fetch_cors` — CORS-enabled fetch для cross-origin subresource (Fetch §3-§4)
`crates/network/src/lib.rs:1552` **fn** `fetch_range`
`crates/network/src/lib.rs:1613` **fn** `fetch_multi_range` — Multi-range запрос (RFC 7233 §4.1). Один request на несколько
`crates/network/src/lib.rs:1693` **fn** `fetch_subresource` — Загрузить подресурс с проверкой mixed-content по подключённой
`crates/network/src/lib.rs:2066` **struct** `InMemoryFetchInterceptor` — In-memory реализация `FetchInterceptor` для тестов без SQLite
`crates/network/src/lib.rs:2072` **fn** `new`
`crates/network/src/lib.rs:2079` **fn** `insert` — Добавить запись: ответ для (origin, url) берётся из кэша без сети
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
`crates/network/src/sse.rs:36` **struct** `SseParser` — Incremental `text/event-stream` parser
`crates/network/src/sse.rs:47` **fn** `new`
`crates/network/src/sse.rs:53` **fn** `push_bytes` — Feed a chunk of bytes from the stream; returns any events that
`crates/network/src/sse.rs:175` **fn** `last_event_id` — Current last-event-id (persists across dispatched events, needed for

## lumen-paint  (77 symbols)

`crates/engine/paint/src/atlas.rs:35` **struct** `AtlasKey` — Композитный ключ glyph-кэша. См. module-level docs
`crates/engine/paint/src/atlas.rs:43` **fn** `new`
`crates/engine/paint/src/atlas.rs:53` **fn** `hash_coords` — Стабильный 64-битный хэш normalized variation coords для cache key
`crates/engine/paint/src/atlas.rs:67` **struct** `GlyphEntry`
`crates/engine/paint/src/atlas.rs:76` **struct** `GlyphAtlas`
`crates/engine/paint/src/atlas.rs:92` **fn** `new`
`crates/engine/paint/src/atlas.rs:106` **fn** `width`
`crates/engine/paint/src/atlas.rs:109` **fn** `height`
`crates/engine/paint/src/atlas.rs:112` **fn** `pixels`
`crates/engine/paint/src/atlas.rs:116` **fn** `dirty`
`crates/engine/paint/src/atlas.rs:119` **fn** `mark_clean`
`crates/engine/paint/src/atlas.rs:123` **fn** `get`
`crates/engine/paint/src/atlas.rs:130` **fn** `insert` — Кладёт растеризованный глиф в атлас. Возвращает `None` если место
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
`crates/engine/paint/src/display_list.rs:31` **enum** `BlendMode` — CSS Compositing & Blending L1 §5 — blend mode. Phase 0 содержит только
`crates/engine/paint/src/display_list.rs:59` **fn** `from_keyword` — Парсит CSS-keyword `mix-blend-mode` / `background-blend-mode` (CSS
`crates/engine/paint/src/display_list.rs:95` **struct** `CornerRadii` — Corner radii for CSS `border-radius`. Values are in CSS pixels, clamped to ≥ 0
`crates/engine/paint/src/display_list.rs:117` **fn** `all_zero` — Returns `true` if all eight radii are zero (no rounding needed)
`crates/engine/paint/src/display_list.rs:133` **fn** `from_style_and_box` — Builds `CornerRadii` from a `ComputedStyle` and the element's border-box dimensions
`crates/engine/paint/src/display_list.rs:149` **fn** `from_style` — Builds `CornerRadii` from a `ComputedStyle`. `border-radius: N%` values are
`crates/engine/paint/src/display_list.rs:155` **enum** `DisplayCommand`
`crates/engine/paint/src/display_list.rs:475` **type** `DisplayList`
`crates/engine/paint/src/display_list.rs:504` **fn** `fit_image_rect` — CSS Images L3 §5.5 — `object-fit` placement: где располагается
`crates/engine/paint/src/display_list.rs:559` **fn** `fit_image_quad` — Финальный GPU-quad для `<img>`: пересечение «полного» placement-rect
`crates/engine/paint/src/display_list.rs:617` **fn** `serialize_display_list`
`crates/engine/paint/src/display_list.rs:923` **fn** `build_display_list`
`crates/engine/paint/src/display_list.rs:938` **fn** `build_display_list_with_anim` — Like `build_display_list` but applies compositor animation overrides per node
`crates/engine/paint/src/display_list.rs:978` **fn** `build_display_list_ordered` — Билдер display list-а, **уважающий painting order** (CSS 2.1 Appendix E)
`crates/engine/paint/src/display_list.rs:1019` **fn** `build_display_list_ordered_with_anim` — Like [`build_display_list_ordered`] but applies compositor animation overrides per node
`crates/engine/paint/src/hit_test.rs:48` **struct** `HitTestResult` — Результат hit-теста
`crates/engine/paint/src/hit_test.rs:71` **fn** `hit_test` — Hit-тест точки в viewport-координатах. `root` — layout-дерево из
`crates/engine/paint/src/lib.rs:47` **struct** `FontMeasurer` — Реализация [`TextMeasurer`] на основе TTF-данных шрифта
`crates/engine/paint/src/lib.rs:57` **fn** `new` — Создаёт измеритель из уже разобранного [`lumen_font::Font`]
`crates/engine/paint/src/renderer.rs:1106` **enum** `SnapshotUploadError` — Ошибка `Renderer::upload_layer_snapshot`
`crates/engine/paint/src/renderer.rs:1135` **enum** `ImageRegisterError` — Ошибка `Renderer::register_image`
`crates/engine/paint/src/renderer.rs:1199` **struct** `Renderer`
`crates/engine/paint/src/renderer.rs:1292` **fn** `new`
`crates/engine/paint/src/renderer.rs:2377` **fn** `with_font_provider` — Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
`crates/engine/paint/src/renderer.rs:2385` **fn** `set_font_provider` — Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
`crates/engine/paint/src/renderer.rs:2398` **fn** `preload_fallback_chain` — Эагерно загружает указанные family-имена через текущий `FontProvider`,
`crates/engine/paint/src/renderer.rs:2417` **fn** `preload_curated_fallbacks` — Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
`crates/engine/paint/src/renderer.rs:2494` **fn** `register_image` — Регистрирует декодированное изображение в GPU-cache под ключом `src`
`crates/engine/paint/src/renderer.rs:2631` **fn** `unregister_image` — Снимает регистрацию изображения. После этого `DrawImage` для `src`
`crates/engine/paint/src/renderer.rs:2640` **fn** `clear_images` — Снимает регистрацию всех картинок (например, при переходе на новую
`crates/engine/paint/src/renderer.rs:2647` **fn** `has_image` — Зарегистрирована ли картинка с таким `src` (для shell-логирования)
`crates/engine/paint/src/renderer.rs:2665` **fn** `upload_layer_snapshot` — Загружает CPU-пиксели (`Rgba8`, 4 байта/пиксель) как именованный
`crates/engine/paint/src/renderer.rs:2732` **fn** `evict_layer_snapshot` — Удаляет снимок с `id`. GPU-память освобождается при drop-е
`crates/engine/paint/src/renderer.rs:2737` **fn** `clear_layer_snapshots` — Удаляет все снимки (например, при переходе на новую страницу)
`crates/engine/paint/src/renderer.rs:2743` **fn** `has_layer_snapshot` — Зарегистрирован ли снимок с таким `id`
`crates/engine/paint/src/renderer.rs:2749` **fn** `snapshot_dimensions` — Возвращает `(width, height)` снимка, или `None` если `id` не зарегистрирован
`crates/engine/paint/src/renderer.rs:2753` **fn** `resize`
`crates/engine/paint/src/renderer.rs:2769` **fn** `set_scale_factor` — Обновить device-pixel-ratio. Вызывается shell-ом по `WindowEvent::ScaleFactorChanged`
`crates/engine/paint/src/renderer.rs:2778` **fn** `scale_factor` — Текущий device-pixel-ratio. Для отладки / тестов (UI обычно его не читает —
`crates/engine/paint/src/renderer.rs:2785` **fn** `viewport_size` — Текущий viewport в **logical** (CSS) пикселях: `physical / scale_factor`
`crates/engine/paint/src/renderer.rs:2904` **fn** `render` — Рендерит две полосы display list-а одним кадром:
`crates/engine/paint/src/scroll_snap.rs:33` **fn** `find_scroll_snap_y` — CSS Scroll Snap L1 — returns the Y scroll offset to snap to, or `None`
`crates/engine/paint/src/scroll_snap.rs:54` **fn** `find_scroll_snap_y_proximity` — CSS Scroll Snap L1 — same as [`find_scroll_snap_y`] but restricts candidates

## lumen-shell  (105 symbols)

`crates/shell/src/animation_scheduler.rs:49` **struct** `AnimationScheduler` — Планировщик CSS-анимаций. Хранит timing-состояние между кадрами
`crates/shell/src/animation_scheduler.rs:54` **fn** `new`
`crates/shell/src/animation_scheduler.rs:62` **fn** `tick` — Тик планировщика: обходит layout-дерево, для каждой активной анимации
`crates/shell/src/animation_scheduler.rs:75` **fn** `clear` — Удалить все записи для элементов, которых больше нет в дереве
`crates/shell/src/find.rs:26` **struct** `FindState` — Состояние find bar и текущего запроса
`crates/shell/src/find.rs:33` **fn** `is_open`
`crates/shell/src/find.rs:37` **fn** `query`
`crates/shell/src/find.rs:41` **fn** `active_index`
`crates/shell/src/find.rs:45` **fn** `open`
`crates/shell/src/find.rs:49` **fn** `close`
`crates/shell/src/find.rs:55` **fn** `append_str`
`crates/shell/src/find.rs:70` **fn** `backspace`
`crates/shell/src/find.rs:82` **fn** `next` — Циклически переходит к следующему совпадению. `total` — текущее число
`crates/shell/src/find.rs:88` **fn** `prev`
`crates/shell/src/find.rs:99` **struct** `FindMatch` — Найденный матч: bounding box в координатах окна и индекс DrawText-команды
`crates/shell/src/find.rs:124` **fn** `scroll_to_match` — Вычисляет новое значение `scroll_y` так, чтобы `match_rect` попал в
`crates/shell/src/find.rs:149` **fn** `find_matches` — Находит все непересекающиеся вхождения `query` в DrawText-командах
`crates/shell/src/find.rs:218` **struct** `BarOverlay` — Параметры overlay-бара. `window_size` — текущий размер окна в логических
`crates/shell/src/find.rs:235` **fn** `build_page_with_highlights` — Собирает page-полосу display list-а: исходные команды + highlight-FillRect-ы
`crates/shell/src/find.rs:268` **fn** `build_bar_overlay` — Собирает overlay-полосу: только find-bar (фон + label + input + counter)
`crates/shell/src/find.rs:282` **fn** `build_with_overlay` — Совместимая сборка: page + bar в один list. Используется тестами и
`crates/shell/src/forms.rs:30` **struct** `FormControlState` — Mutable runtime state for a single form control
`crates/shell/src/forms.rs:40` **type** `FormState` — `NodeId` → mutable state map for all form controls on the current page
`crates/shell/src/forms.rs:48` **enum** `FormClickAction` — What the shell should do after a left-click on `node`
`crates/shell/src/forms.rs:57` **fn** `classify_click` — Classify a click on `node` given the current DOM tree
`crates/shell/src/forms.rs:92` **fn** `toggle_checkbox` — Toggle the `checked` attribute on a checkbox input in the live DOM
`crates/shell/src/forms.rs:104` **fn** `set_value` — Set `value` attribute of an input / textarea in the DOM
`crates/shell/src/forms.rs:121` **fn** `find_validation_error` — Depth-first walk: find the first form control that fails HTML5 constraint
`crates/shell/src/forms.rs:171` **fn** `find_box_rect` — Find the bounding rect of the LayoutBox for `node`. Returns `None` if the
`crates/shell/src/forms.rs:193` **fn** `build_validation_tooltip` — Build a validation tooltip anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:248` **fn** `collect_form_entries` — Собрать данные формы для submit — DOM-значения, поверх которых наложен
`crates/shell/src/forms.rs:283` **fn** `build_form_submit` — Построить параметры отправки формы: `(action, method, body)`
`crates/shell/src/forms.rs:304` **fn** `make_get_url` — Построить итоговый URL для GET-формы: добавить `?body` к action URL
`crates/shell/src/forms.rs:344` **fn** `build_color_picker` — Build a color-swatch picker anchored below `anchor` (document coordinates)
`crates/shell/src/forms.rs:381` **fn** `hit_color_swatch` — If viewport-space point `(px, py)` lands on a swatch, return its `[r, g, b]`
`crates/shell/src/forms.rs:402` **fn** `swatch_to_css_color` — Format `[r, g, b]` as CSS `#rrggbb`
`crates/shell/src/links.rs:15` **fn** `find_link_href` — Walk up the ancestor chain from `node_id` to find the nearest `<a>` element
`crates/shell/src/links.rs:43` **fn** `is_navigable_href` — Return true if `href` is a URL scheme the browser should navigate to
`crates/shell/src/links.rs:53` **fn** `fragment_only` — If `href` is a fragment-only reference (starts with `#`), return the
`crates/shell/src/links.rs:60` **fn** `find_element_by_id` — Walk the document tree and return the first element whose `id` attribute
`crates/shell/src/momentum_anim.rs:26` **struct** `MomentumAnim` — Velocity-based momentum анимация. Хранится в `Lumen.momentum_anim`
`crates/shell/src/momentum_anim.rs:36` **fn** `new`
`crates/shell/src/momentum_anim.rs:43` **fn** `advance` — Прогнать анимацию до `now_ms`. Возвращает `(Δy, Δx, done)`
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
`crates/shell/src/runtime.rs:244` **type** `ObserverHandle` — Уникальный handle наблюдателя. `disconnect_observer` снимает регистрацию
`crates/shell/src/runtime.rs:266` **type** `IdleCallbackHandle` — Уникальный идентификатор idle-callback-а — возвращается
`crates/shell/src/runtime.rs:280` **struct** `IdleDeadline` — Аргумент idle-callback-а (W3C `requestIdleCallback` §3 `IdleDeadline`)
`crates/shell/src/runtime.rs:288` **fn** `time_remaining` — Сколько миллисекунд осталось до конца текущего idle-окна. Отрицательные
`crates/shell/src/runtime.rs:299` **fn** `did_timeout` — Был ли callback вызван из-за timeout-параметра запроса (а не реального
`crates/shell/src/runtime.rs:338` **enum** `StepResult` — Результат одной итерации `step()`: запустилась ли task
`crates/shell/src/runtime.rs:348` **struct** `EventLoop` — HTML event loop. Реализует §8.1.4.2 «Processing model» в минимально полезном
`crates/shell/src/runtime.rs:359` **fn** `new`
`crates/shell/src/runtime.rs:367` **fn** `handle` — Дешёвая клон-копия handle-а для постановки task-ов извне и изнутри
`crates/shell/src/runtime.rs:380` **fn** `step` — Один step event-loop-а:
`crates/shell/src/runtime.rs:395` **fn** `perform_microtask_checkpoint` — HTML §8.1.4.4 «Microtask checkpoint». Drain-all: вновь поставленный
`crates/shell/src/runtime.rs:417` **fn** `run_rendering_step` — Rendering opportunity stage — HTML §8.1.5.1 «Run the animation frame
`crates/shell/src/runtime.rs:434` **fn** `pending_tasks` — Сколько task-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:439` **fn** `pending_microtasks` — Сколько microtask-ов сейчас в очереди (для тестов / отладки)
`crates/shell/src/runtime.rs:445` **fn** `pending_animation_frames` — Сколько rAF-callback-ов сейчас ждёт следующего rendering step
`crates/shell/src/runtime.rs:451` **fn** `pending_idle_callbacks` — Сколько idle-callback-ов сейчас ждёт следующего `run_idle_callbacks`
`crates/shell/src/runtime.rs:473` **fn** `run_idle_callbacks` — W3C `requestIdleCallback` §3 — выполнить ожидающие idle-callback-и
`crates/shell/src/runtime.rs:495` **fn** `active_observers` — Сколько активных наблюдателей указанного типа (для тестов / отладки)
`crates/shell/src/runtime.rs:513` **fn** `deliver_observer_records` — Доставить records всем активным наблюдателям указанного типа
`crates/shell/src/runtime.rs:531` **struct** `EventLoopHandle` — Дёшево клонируемая ссылка на event loop. Closure-ы task-ов / microtask-ов
`crates/shell/src/runtime.rs:536` **fn** `queue_task`
`crates/shell/src/runtime.rs:543` **fn** `queue_microtask`
`crates/shell/src/runtime.rs:552` **fn** `request_animation_frame` — Зарегистрировать rAF-callback. Будет вызван на ближайшем
`crates/shell/src/runtime.rs:571` **fn** `cancel_animation_frame` — Отменить rAF до выполнения. Если handle уже выполнен или неизвестен —
`crates/shell/src/runtime.rs:586` **fn** `request_idle_callback` — Зарегистрировать idle-callback (W3C `requestIdleCallback` §3). Будет
`crates/shell/src/runtime.rs:606` **fn** `cancel_idle_callback` — Отменить idle-callback до выполнения. Неизвестный или уже выполненный
`crates/shell/src/runtime.rs:612` **fn** `register_observer` — Зарегистрировать observer выбранного типа. Callback-ы вызываются при
`crates/shell/src/runtime.rs:629` **fn** `disconnect_observer` — Снять регистрацию наблюдателя. Неизвестный handle — no-op
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

## lumen-storage  (369 symbols)

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
`crates/storage/src/bookmarks.rs:212` **fn** `list_by_folder` — Список закладок в данной папке (точное совпадение строки)
`crates/storage/src/bookmarks.rs:226` **fn** `list_by_tag` — Список закладок с данным тегом. Сортировка по created_at DESC
`crates/storage/src/bookmarks.rs:243` **fn** `all_tags` — Все уникальные теги в системе (для UI tag-cloud / autocomplete)
`crates/storage/src/bookmarks.rs:262` **fn** `all_folders` — Все уникальные папки
`crates/storage/src/bookmarks.rs:283` **fn** `count` — Общее число закладок
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
*Total: 1560 symbols in 16 crates*
