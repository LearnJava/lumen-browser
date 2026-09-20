# Задача: WebTransport (поверх HTTP/3)

**Developer:** P1
**Ветка:** `p1-webtransport`
**Размер:** M (срез 1 сделан 2026-09-19; блокер снят — см. зависимость; срезы 2–5 остаются)
**Крейты:** `lumen-js`, `lumen-network`

## Goal
Реализовать W3C WebTransport (§3–5): `new WebTransport(url)` над HTTP/3 (Extended CONNECT,
RFC 9220) с datagrams и uni/bidirectional QUIC-стримами, `ready`/`closed` промисами.

## ⚠️ Зависимость
**Снята 2026-09-19 (P1) — P3-h3 закрыт (`done`).** `crates/network/src/h3/` теперь
включает живой UDP-транспорт и QUIC connection loop (`client_bootstrap.rs::connect_client`,
`udp.rs::DatagramTransport`/`UdpDatagram`), не только чистые кодеки. Остаётся не готово
именно то, что WebTransport использует напрямую: Extended CONNECT (RFC 9220,
`:protocol = webtransport`) нет вообще (только settings-половина,
`SETTINGS_ENABLE_CONNECT_PROTOCOL`), и QUIC DATAGRAM (RFC 9221, `h3/datagram.rs`)
остаётся чистым кодеком без сетевого IO. Срез 1 (этот файл) не требует ни того, ни
другого — только срезы 2–4.

## Current state (сверено с кодом 2026-09-19)
- `crates/js/src/webtransport.rs` — **срез 1 сделан**: spec-корректный каркас классов
  (`WebTransport`/`WebTransportError`/`WebTransportDatagramDuplexStream`/
  `WebTransportBidirectionalStream`), URL-валидация (§5.1), один нативный биндинг
  `_lumen_webtransport_open` (пока всегда «нет сессии»). Файл создан заново — не
  копия удалённого S12b-21 стаба.
- `crates/network/src/h3/` — живой QUIC-транспорт есть (сокет + handshake + connection
  loop), Extended CONNECT нет, datagram flush в сеть нет.
- `crates/network/src/h3/datagram.rs` — QUIC DATAGRAM frame кодек (RFC 9221), pure-codec,
  не привязан к сокету.

## Entry points
- `crates/js/src/webtransport.rs` — `install_webtransport_v8` (точка расширения для срезов 2–5).
- `crates/network/src/h3/mod.rs` — публичная поверхность HTTP/3 (кодеки + транспорт).
- `crates/network/src/h3/client_bootstrap.rs`, `udp.rs` — живой QUIC connection loop, отправная точка для среза 2.
- `crates/network/src/h3/datagram.rs` — QUIC DATAGRAM кодек (для среза 4).
- `crates/network/src/h3/settings.rs::enable_connect_protocol` — settings-половина Extended CONNECT, срезу 2 нужна вторая половина (сам CONNECT-стрим).

## Срезы (декомпозиция)
### Срез 0 — done — Дождаться живого QUIC/H3 IO от P3-h3
**2026-09-19: закрыт** — P3-h3 в статусе `done`, живой UDP-транспорт и QUIC handshake есть.

### Срез 1 — S — done — Каркас JS-API без «phase-0-stub» текстов
**2026-09-19: сделан.** Классы (`WebTransport`, `WebTransportBidirectionalStream`,
`WebTransportDatagramDuplexStream`, `WebTransportError` с `source`/`streamErrorCode`),
методы дергают нативный биндинг `_lumen_webtransport_open`, который пока возвращает
«not connected» (i32-сентинел, BUG-457) — форма API уже спек-корректна, замена
биндинга на реальный не потребует переписывать этот файл.

### Срез 2 — M — Extended CONNECT над H3 (RFC 9220), gated by QUIC
Нативный биндинг: открыть WT-сессию через `:protocol = webtransport` CONNECT-стрим
на живом H3-соединении. Резолв `ready` при 2xx, reject при отказе. **Требует срез 0.**

**Срез 2a — done (2026-09-20, P1) — транспортный примитив.** `:protocol`
псевдо-заголовок (RFC 9220 §3) в `crates/network/src/h3` — `ClientRequest::protocol`,
`build_request_fields`/`encode_request`. `open_extended_connect`/`extended_connect_head`
пробрасываются сквозной цепочкой `RequestDispatch → RequestPump → RequestTurn →
RequestDriver` (тот же паттерн, что и у `send_request`); `extended_connect_head` читает
финальную голову ответа **до** FIN — `RequestMux::peek_final_head`/`ClientExchange::final_head`
— то, чего обычный запрос не требовал (успешная Extended CONNECT-сессия намеренно никогда
не FIN'ится). `h3_extended_connect_on_driver` (`client_transport.rs`) — генерик по
`DatagramTransport` (как и `h3_exchange`), драйвит `transmit`/`poll` до финальной головы
turn-budget'ом. Новые ошибки `ConnectFetchError::ExtendedConnect{Dispatch,Driver,Incomplete}`.
2286/2286 тестов `lumen-network`, `clippy -p lumen-network --all-targets -D warnings` и
`cargo check --workspace` зелёные.

**Срез 2b — done (2026-09-20, P1) — доводка до `ready`.** `JsFetchProvider` (lumen-core)
получил `webtransport_connect(url) -> Result<JsWebTransportSession>` с default-реализацией
«не поддерживается» (тестовые двойники не ломаются); `lumen-network::HttpClient`
перекрывает его — парсит URL, зовёт `h3::client_transport::h3_connect` (свежий QUIC-коннект,
резолвер клиента, не через Alt-Svc/`http3_enabled` — WebTransport QUIC-native по определению,
не апгрейд HTTP-запроса), затем `h3_extended_connect_on_driver` (срез 2a) с
`protocol = b"webtransport"`; 2xx → `Ok(JsWebTransportSession{handle,status})`, иначе/на любую
ошибку `Err`. Подтверждённый driver не дропается — оседает в новом
`HttpClient::webtransport_sessions` (`HashMap<i32, (RequestDriver<UdpDatagram>, stream_id)>`)
под свежим handle, для среза 3 (streams). `crates/js/src/webtransport.rs`'s
`_lumen_webtransport_open` теперь принимает `fetch_provider` (как `install_dom`'s остальные
сетевые биндинги) и возвращает JSON-строку (`{"ok":true,"status":N}` / `{"ok":false,"message":…}`)
— шим `WebTransport`'s `setTimeout`-колбэк резолвит `ready` на `ok:true`, иначе реджектит
`ready`/`closed` `WebTransportError`-ом. `closed` намеренно не settl'ится на успехе — lifecycle
(срез 5) ещё не подключён. 3964+151 тестов `lumen-js --features v8-backend` и 2297 тестов
`lumen-network` зелёные, `clippy -p lumen-core -p lumen-network -p lumen-js --all-targets -D
warnings` и `cargo check --workspace` зелёные. Handle пока не покидает Rust — срез 3
(uni/bidi streams) первый, кому он понадобится в JS.

### Срез 3 — M — Uni/Bidirectional streams
`createUnidirectionalStream`/`createBidirectionalStream` → реальные QUIC-стримы,
обёрнутые в WHATWG ReadableStream/WritableStream (переиспользовать stream-инфраструктуру
из `dom.rs`). **Требует срез 2.**

**Срез 3a-3d — done (2026-09-20, P1) — uni-стримы end-to-end.** Открытие
(`h3_webtransport_open_uni_stream_on_driver`), запись
(`h3_webtransport_write_stream_on_driver`) и close/abort
(`h3_webtransport_close_uni_stream_on_driver`/`h3_webtransport_reset_uni_stream_on_driver`,
срез 3d) — `createUnidirectionalStream()` резолвит настоящий `WritableStream`
чей `write`/`close`/`abort` все доходят до реального QUIC uni-стрима. Полная
хронология срезов 3a/3b/3c/3d — см. `ROADMAP.md`'s `P3-webtransport` строку.

**Срез 4a — done (2026-09-20, P1) — транспортный примитив для bidi-стримов.**
`crates/network/src/h3/client_transport.rs::h3_webtransport_open_bidi_stream_on_driver` —
зеркало среза 3a для client-initiated bidirectional QUIC-стримов (RFC 9000
§2.1: младшие два бита `0b00`, n-й стрим — `4n`, отдельное от uni-счётчика
id-пространство). В отличие от uni-стрима (у него есть нативный QUIC
stream-type байт), bidi-стрим такого поля не имеет — HTTP/3 кодирует
направление фреймом `WEBTRANSPORT_STREAM` (тип `0x41`, draft-ietf-webtrans-http3
§4.3) с session id как payload, первым на стриме; дальше сырые
WebTransport-байты без дальнейшего HTTP/3-фрейминга. Send-половина identична
uni-стриму (`SendStream` не знает про направление), поэтому
`h3_webtransport_write_stream_on_driver`/`close_uni_stream_on_driver`/
`reset_uni_stream_on_driver` переиспользованы без изменений — тест
`webtransport_bidi_stream_write_close_and_reset_reuse_the_uni_primitives`
это закрепляет. Recv-половина (`readable` у `WebTransportBidirectionalStream`)
не требует отдельного «открытия» — `StreamManager` создаёт `RecvStream`
лениво на первый входящий фрейм от пира, тем же путём, что и у обычных
запросов. `WebTransportStreamError::StreamsExhausted`'s текст обобщён (был
жёстко про unidirectional, теперь про оба направления — использован и здесь).
5 новых юнит-тестов (id=0/4, длина заголовка при session-id=0/100, overflow,
переиспользование write/close), 2314 тестов `lumen-network`,
`clippy -p lumen-network --all-targets -D warnings` и `cargo check --workspace`
зелёные. Не сделано: подключение к `HttpClient::webtransport_sessions` и к JS
(`createBidirectionalStream()` пока по-прежнему реджектится,
`ReadableStream`/`WritableStream`-обёртка для bidi отдельно) — срез 4b;
приём входящих (`incomingBidirectionalStreams`/`incomingUnidirectionalStreams`)
и датаграммы (RFC 9221) остаются отдельными под-срезами. Статус остаётся
`planned`.

**Срез 3a — done (2026-09-20, P1) — транспортный примитив для uni-стримов.**
`crates/network/src/h3/client_transport.rs::h3_webtransport_open_uni_stream_on_driver` —
аллоцирует client-initiated unidirectional QUIC stream id (RFC 9000 §2.1: младшие
два бита `0b10`, n-й стрим — `4n + 2`; отдельное от `RequestMux`-счётчика
пространство id, координация не нужна — весь этот драйвер посвящён одной
WT-сессии), пишет заголовок WT-стрима (varint stream type `0x54`, затем varint
session id = id Extended CONNECT-стрима — draft-ietf-webtrans-http3 §4.2, сверено
по дататрекеру IETF) через существующую цепочку аксессоров
`driver.turn_mut().pump_mut().dispatch_mut().streams_mut()` (никаких новых полей/методов
в `RequestDispatch`/`RequestPump`/`RequestTurn`/`RequestDriver` не потребовалось — вся
цепочка уже была `pub`), один `transmit()` без ожидания ответа (uni-стрим не отвечает).
`WebTransportStreamError` — переполнение id (`2^60`, чисто теоретическое, как у
`RequestMux::OpenError::StreamsExhausted`), ошибка кодирования varint, ошибка
драйвера. 4 новых юнит-теста на `MockDatagramTransport` (id=2/6, длина заголовка
для session-id=0 и session-id=100 — проверяет переключение длины varint), 2301
тестов `lumen-network`, `clippy -p lumen-network --all-targets -D warnings` зелёный.
Не сделано: подключение к `HttpClient::webtransport_sessions` (нужен per-сессии
счётчик `next_uni_stream_number`) и к JS (`_lumen_webtransport_open` пока не
возвращает `handle` наружу в JS — без него `createUnidirectionalStream()` нечем
адресовать сессию; `WritableStream`-обёртка с маршалингом `Uint8Array`→`Vec<u8>`)
— срез 3b. Bidi-стримы и приём входящих стримов — отдельные под-срезы (эта функция
даёт только client-initiated uni, самый простой случай: отдельное от bidi id-пространство,
не требует координации с `RequestMux`).

### Срез 4 — S — Datagrams
`datagrams.readable/writable` поверх `h3/datagram.rs` (QUIC DATAGRAM). **Требует срез 2.**

### Срез 5 — S — Lifecycle: closed/close(info)/сессионные коды ошибок
Корректный `closed` промис, `close({closeCode, reason})`, RFC 9114/9220 error mapping.

## Tests
- Юнит (`lumen-js`): наличие классов, `new WebTransport('https://…')` не бросает синхронно,
  типы `ready`/`closed`/`datagrams` (можно до QUIC — проверяют форму API).
- Integration (после QUIC): сессия к mock-H3-серверу, echo bidi-стрим, datagram round-trip.
- Пока QUIC нет — тесты фиксируют «reject с корректным `WebTransportError`», не «throw».

## Definition of done
- [x] Зависимость P3-h3 → живой QUIC IO явно отражена (срез 0), задача не стартует раньше.
  **2026-09-19: P3-h3 закрыт (`done`)** — блокер снят, срез 1 сделан этой ревизией.
- [x] Шим переписан в spec-форму классов (срез 1) — уже можно без QUIC.
  Новый `crates/js/src/webtransport.rs` (не копия удалённого S12b-21 стаба):
  `WebTransport`/`WebTransportError`/`WebTransportDatagramDuplexStream`/
  `WebTransportBidirectionalStream`, URL-валидация, один нативный биндинг
  `_lumen_webtransport_open` (i32-сентинел, BUG-457), `install_v8!` подключён.
  7 юнит-тестов зелёные.
- [x] Extended CONNECT доходит до живого `ready` (срезы 2a/2b, см. таблицу выше).
- [x] Uni streams end-to-end: open/write/close/abort (срезы 3a-3d, 2026-09-20).
- [ ] Bidi streams, datagrams, lifecycle — остаток среза 3, срезы 4–5.
- [x] `CAPABILITIES.md` — WebTransport 🟡 (`ready` живой, streams/datagrams/lifecycle ещё стабы).
