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
зелёные. Не сделано (на момент 4a): подключение к
`HttpClient::webtransport_sessions` и к JS (`createBidirectionalStream()`
пока по-прежнему реджектится, `ReadableStream`/`WritableStream`-обёртка для
bidi отдельно) — срез 4b; приём входящих
(`incomingBidirectionalStreams`/`incomingUnidirectionalStreams`) и
датаграммы (RFC 9221) остаются отдельными под-срезами. Статус остаётся
`planned`.

**Срез 4c — done (2026-09-20, P1) — bidi `readable` (приём входящих байт
на стриме, который открыли мы).** `_lumen_webtransport_read_stream(handle,
streamId)` (`webtransport_read_bidi_stream` в trait `JsFetchProvider`,
`h3_webtransport_read_stream_on_driver` в `client_transport.rs`) — каждый
вызов дренирует одним non-blocking проходом всё, что уже стоит в очереди
сокета сессии (`RequestDriver::poll_incoming_nonblocking`, новый метод:
`DatagramEventLoop::poll_nonblocking`/`ConnectionDriver::wait_nonblocking`
читают с нулевым read-timeout, в отличие от `wait()`, который блокируется
до ближайшего QUIC-дедлайна — тот может быть секундами в будущем, а сессия
между чтениями больше никем не дренится), затем отдаёт то, что стало
читаемым на `streamId`. Попутно найден и починен латентный баг:
`RequestDispatch::on_stream_frame_with_sink` гейтит входящий STREAM-фрейм
по `mux.is_active(stream_id)` — стримы, которые WebTransport открывает
напрямую через `streams_mut()` (`h3_webtransport_open_bidi_stream_on_driver`),
мультиплексору неизвестны, так что любой ответ пира на такой стрим валил
бы **весь** datagram ingest ошибкой `MuxError::UnknownStream` (до этого
среза непротестировано против живого пира). `RequestDispatch::register_foreign_stream`
регистрирует такой id как легитимный вне мультиплексора — входящий STREAM
на нём реассемблируется в `StreamManager`, но не порождает `H3Response`;
`h3_webtransport_open_bidi_stream_on_driver` теперь зовёт его сама. Шим:
`openBidiStreamReadable` — `pull()`-based `ReadableStream`, опрашивает
нативный биндинг синхронно (без `setTimeout`, пока не пусто и не
`finished`), при пустом непоследнем ответе планирует retry через
`setTimeout(0)`. 6 новых тестов `lumen-network` (`request_dispatch`:
foreign-stream reassembly/mux-isolation; `event_loop`: `poll_nonblocking`;
`request_driver`: `poll_incoming_nonblocking` composes с обычным
request/response на одном соединении), 2 новых теста `lumen-js`
(`_lumen_webtransport_read_stream` unsupported-with-no-provider,
end-to-end byte delivery + close). `cargo clippy -p lumen-network`,
`cargo clippy -p lumen-js --features v8-backend` (`-D warnings`) зелёные.
Не сделано: incoming (peer-initiated) uni/bidi-стримы — другой механизм
(обнаружение нового id, открытого пиром, а не чтение с уже открытого нами)
— срез 4d; датаграммы (срез 4/datagrams в декомпозиции ниже); lifecycle
(срез 5).

**Срез 4d — S — done (2026-09-20, P1) — incoming (peer-initiated)
unidirectional streams (bidi incoming remains a follow-up slice).**
`RequestDispatch::on_stream_frame_with_sink`'s `mux.is_active`/`foreign_streams`
gate (срез 4c's fix already exempted streams *we* register ahead of time)
now also auto-registers a never-seen **server-initiated** stream id
(`stream::is_server_initiated`, RFC 9000 §2.1: ids `4n+1`/`4n+3`) the moment
its first STREAM frame arrives — the only shape of server-initiated stream a
WebTransport client ever sees (HTTP/3 server push is not implemented) — and
queues it once in a new `discovered_server_streams` drain
(`RequestDispatch::take_discovered_server_streams`). New transport primitive
`h3_webtransport_poll_new_peer_streams_on_driver` (`client_transport.rs`)
wraps `poll_incoming_nonblocking` + that drain; `parse_webtransport_uni_header`
parses the `0x54`+session-id header (draft-ietf-webtrans-http3 §4.2) off the
accumulated bytes, `None` while incomplete (the caller retries next poll).
`HttpClient::webtransport_poll_incoming_uni_streams(handle)`
(`crates/network/src/lib.rs`) orchestrates per session: classifies each newly
discovered id by parity (`is_unidirectional`; a discovered bidi id is left
alone — that's a later slice), accumulates header bytes in
`WebTransportSession::pending_peer_uni_headers` across polls until
`parse_webtransport_uni_header` succeeds, then stashes the header's leftover
application bytes in `peer_uni_stream_leftover` and reports the id ready.
`webtransport_read_incoming_uni_stream(handle, stream_id)` prepends that
leftover on its first call, then behaves like `webtransport_read_bidi_stream`.
Two new `JsFetchProvider` methods (default "unsupported", same pattern as
every other WebTransport extension point) plus two new natives,
`_lumen_webtransport_poll_incoming_uni_streams`/
`_lumen_webtransport_read_incoming_uni_stream`. Shim:
`incomingUnidirectionalStreams` is no longer a permanently-empty
`ReadableStream` — `openIncomingUnidirectionalStreams` polls discovery in the
same `setTimeout(0)` loop shape as `openBidiStreamReadable`, waiting (not
erroring) while the session has no live handle yet, and stopping for good on
`close()`/a failed `ready` (`_readyFailed`); each discovered id is wrapped by
`openIncomingUniStreamReadable` (read-only — a WebTransport incoming uni
stream carries no writable half by definition, RFC 9000 §2.1) and enqueued
into the outer stream. 3 new tests `lumen-network::h3::request_dispatch`
(auto-registration/discovery-once/ordering), 5 new tests
`lumen-network::h3::client_transport` (poll/classify/header-parse, including
a truncated-header case), 2306+3 tests `lumen-network`; 6 new tests
`lumen-js` (native unsupported/success + one end-to-end discovery→nested-
readable→bytes test), 3972+6 tests `lumen-js --features v8-backend`. `cargo
clippy -p lumen-core -p lumen-network -p lumen-js --all-targets --features
lumen-js/v8-backend -D warnings` and `cargo check --workspace` green. Not
done: incoming bidirectional streams (their `readable` needs the same
discovery+header-parse machinery this slice built, but their `writable` also
needs a `SendStream` registered for a peer-picked id — deferred, no
dedicated sub-slice number assigned yet), datagrams (срез 4/datagrams
below), lifecycle (срез 5).

**Срез 4e — done (2026-09-20, P1) — incoming bidirectional streams.**
`HttpClient::classify_discovered_peer_streams` (`crates/network/src/lib.rs`,
factored out of срез 4d's `webtransport_poll_incoming_uni_streams` so both
poll methods share one discovery drain) now files a discovered bidirectional
id (RFC 9000 §2.1) into its own `pending_peer_bidi_headers` map instead of
leaving it unclassified, and immediately registers its send half via a new
transport primitive, `h3_webtransport_open_incoming_bidi_send_on_driver`
(`client_transport.rs`) — idempotent `open_send_stream`, no header of ours to
write since the peer's own `WEBTRANSPORT_STREAM` frame already established
the direction. `webtransport_poll_incoming_bidi_streams`/
`webtransport_read_incoming_bidi_stream` mirror the uni pair exactly
(`parse_webtransport_uni_header` is reused unchanged — it never validated the
type byte, only decoded two varints, so it works for both stream kinds); the
write half needed no new primitive at all — `webtransport_write_uni_stream`/
`webtransport_close_uni_stream`/`webtransport_abort_uni_stream` were already
generic over any already-open `stream_id`, and the discovery step above is
what makes a peer-picked id "already open" for them. Two new
`JsFetchProvider` methods (default "unsupported", same pattern as every other
extension point) and two new natives
(`_lumen_webtransport_poll_incoming_bidi_streams`/
`_lumen_webtransport_read_incoming_bidi_stream`). Shim:
`incomingBidirectionalStreams` is no longer a permanently-empty
`ReadableStream` — `openIncomingBidirectionalStreams` mirrors
`openIncomingUnidirectionalStreams`'s discovery-loop shape but wraps each
discovered id as a full `WebTransportBidirectionalStream`
(`openIncomingBidiStreamReadable` for `readable`, the existing
`openUniStreamWritable` for `writable` — unmodified, since a `SendStream`
never knew its own direction, so the same function that backs a self-opened
stream's write half backs a peer-opened one's too). 2 new tests
`client_transport` (write only succeeds after registration; a second,
idempotent registration does not reset bytes already queued), 2314+2 tests
`lumen-network`; 5 new tests `lumen-js` (both new natives'
unsupported/success plus one end-to-end discovery→readable-bytes+writable-write
test), 3978+5 tests `lumen-js --features v8-backend`. `cargo clippy -p
lumen-core -p lumen-network -p lumen-js --all-targets --features
lumen-js/v8-backend -D warnings` and `cargo check --workspace` green. Not
done: datagrams (срез 4/datagrams below), lifecycle `closed`/`close(info)`
on the session (срез 5).

**Срез 4b — done (2026-09-20, P1) — подключение bidi-стрима к сессии и к
JS.** `HttpClient::webtransport_open_bidi_stream(handle)`
(`crates/network/src/lib.rs`) зеркалит `webtransport_open_uni_stream`:
per-сессии счётчик `next_bidi_stream_number` (отдельное id-пространство от
uni, RFC 9000 §2.1), `peer_initial_max_stream_data_bidi` берётся из
`ClientConnectConfig::initial_max_stream_data_bidi_remote` (аналог
`initial_max_stream_data_uni` у uni-стрима). Новый нативный биндинг
`_lumen_webtransport_open_bidi_stream(handle)`
(`crates/js/src/webtransport.rs`) и
`JsFetchProvider::webtransport_open_bidi_stream` (default — «не
поддерживается», как остальные точки расширения). `createBidirectionalStream()`
больше не реджектится безусловно: при отсутствии `_handle` — тот же
синхронный reject, что и у `createUnidirectionalStream()`; иначе открывает
реальный QUIC bidi-стрим и резолвит `WebTransportBidirectionalStream`, чей
`writable` переиспользует `openUniStreamWritable` без изменений (`SendStream`
не знает своего направления, write/close/abort идентичны uni-стриму) — тест
`create_bidirectional_stream_write_reaches_the_native_with_the_right_ids`
закрепляет составную цепочку. `readable` — пока `emptyReadableStream()`:
входящие байты WebTransport-стрима (uni или bidi) в JS не доходят ни в одном
из направлений — это остаётся вместе с приёмом входящих стримов, следующий
под-срез. 4 новых юнит-теста `lumen-js` (unsupported/success для нативного
биндинга, reject-before-ready, write end-to-end для bidi) — 30/30
`webtransport`-тестов `lumen-js --features v8-backend` зелёные, 17/17
webtransport-тестов `lumen-network` не затронуты и зелёные, `clippy -p
lumen-network -p lumen-core -p lumen-js --all-targets --features
lumen-js/v8-backend -D warnings` и `cargo check` тех же крейтов зелёные.

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

**Срез datagrams-a — done (2026-09-20, P1) — транспортный примитив: QUIC DATAGRAM frame + transport parameter.**
`h3/datagram.rs` (несмотря на имя) — это UDP-датаграммная коалесация пакетов
(RFC 9000 §12.2/§14.1), не сам QUIC DATAGRAM (RFC 9221); этого типа фрейма не
было в `quic_frame.rs` вообще. Новый `Frame::Datagram(Vec<u8>)`: парсит обе
формы (`0x30` без Length — до конца пакета; `0x31` с Length — коалесируемая),
кодирует всегда во `0x31` (тот же выбор, что у STREAM). `PacketType::permits`
разрешает его только в 0-RTT/1-RTT (RFC 9221 §4). `SendPriority::of` даёт ему
приоритет STREAM. `QuicConnection::dispatch_frame` кладёт принятый DATAGRAM в
`effects.deferred` — оттуда он доходит до `RequestTurn::route_deferred`'s
`residual` (не запрашивается пумпом, значит не `is_request_frame`), но пока
никто это не читает: маршрутизация к конкретной WebTransport-сессии по quarter
stream id (RFC 9297 §2.1) — отдельный под-срез. Новый транспортный параметр
`max_datagram_frame_size` (id `0x20`, RFC 9221 §3) в `transport_params.rs`:
`None` по умолчанию (расширение не поддерживается), клиент рекламирует
`Some(65535)` через новое `ClientConnectConfig::max_datagram_frame_size` —
намеренно завышенный потолок, реальным ограничителем остаётся подтверждённый
path MTU, которого этот параметр не видит. 12 новых тестов `quic_frame`, 2
новых `transport_params`, 2326 тестов `lumen-network`, `clippy -p
lumen-network --all-targets -D warnings` и `cargo check --workspace` зелёные.
Не сделано: приём/отправка настоящих датаграмм (нужен quarter-stream-id
энкодинг сессии + маршрутизация в `HttpClient::webtransport_sessions`),
`_lumen_webtransport_send_datagram`/`_lumen_webtransport_poll_incoming_datagrams`,
шим `WebTransportDatagramDuplexStream.readable`/`.writable` (сейчас
permanently-empty/permanently-reject) — следующий под-срез.

**Срез datagrams-b — done (2026-09-20, P1) — приём/отправка датаграмм и
шим.** `RequestDriver` (`crates/network/src/h3/request_driver.rs`) больше не
роняет `Frame::Datagram` на пол: `poll_incoming_nonblocking` фильтрует
`ingest.residual` и копит каждый payload в новом накопителе
(`RequestDriver::take_datagrams`) — раньше `route_deferred`'s `residual`
не читал вообще никто. Новые транспортные примитивы
`client_transport.rs::h3_webtransport_send_datagram_on_driver`/
`h3_webtransport_poll_incoming_datagrams_on_driver`: отправка кодирует
RFC 9297 §2.1 quarter stream id (`session_id / 4` — session id всегда
client-initiated bidi, `4n`, деление точное) как первый varint payload'а и
кладёт `Frame::Datagram` прямо в `ConnectionSendState::enqueue`
(Application Data) — у датаграммы, в отличие от стрима, нет своего QUIC
stream id, маршрутизировать через `streams_mut()` нечем; приём дренирует
`take_datagrams()` и оставляет только те, чей quarter id совпал с сессией
(один `RequestDriver` держит ровно одну WT-сессию, так что в реальности
несовпадений не бывает, но проверка не убрана — код не полагается на это
как на инвариант против недобросовестного пира). `HttpClient::webtransport_send_datagram`/
`webtransport_poll_incoming_datagrams` (`lib.rs`) и два новых метода
`JsFetchProvider` (default «не поддерживается», `ext.rs`) — тот же паттерн,
что и у прочих точек расширения WebTransport. Новые нативные биндинги
`_lumen_webtransport_send_datagram(handle, bytes)`/
`_lumen_webtransport_poll_incoming_datagrams(handle)` (последний отдаёт
JSON-массив массивов байт — на один poll может прийти больше одной
датаграммы). Шим: `WebTransportDatagramDuplexStream` больше не
permanently-empty/permanently-reject — `writable` (`openDatagramWritable`)
лениво ждёт `session._handle`, как и `datagrams` сконструирован в
`WebTransport`; `readable` (`openDatagramReadable`) — тот же
discovery-loop, что у `incomingUnidirectionalStreams`, но кладёт в очередь
целый `Uint8Array` на каждую датаграмму, а не вложенный `ReadableStream`
(датаграмма RFC 9221 доставляется целиком или не доставляется вовсе, в
отличие от байт-ориентированного стрима). Мёртвые `emptyReadableStream`/
`rejectingWritableStream` (единственные потребители — старый `datagrams`)
удалены. Файл `crates/js/src/webtransport.rs` был на пределе конвенции в
2000 строк ещё до этого среза — тестовый модуль (`mod tests_v8`, ~1090
строк) вынесен в `crates/js/src/webtransport/tests.rs` через `#[path]`,
тот же приём, что `css-parser/src/parser.rs`'s `parser/tests/*.rs`. 3 новых
теста `request_driver`, 6 новых `client_transport` (2314+2 датаграммных
округляются до 2320+), `clippy -p lumen-network --all-targets -D warnings`
зелёный; 7 новых тестов `lumen-js --features v8-backend` (47/47
`webtransport`-тестов зелёные), `clippy -p lumen-core -p lumen-network -p
lumen-js --all-targets --features lumen-js/v8-backend -D warnings` и
`cargo check --workspace` зелёные. Не сделано: lifecycle `closed`/
`close(info)` на сессии (срез 5) — последний оставшийся под-срез задачи.

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
- [x] Bidi streams — открытие + write/close/abort до JS (срезы 4a-4b, 2026-09-20).
- [x] Bidi readable — приём входящих байт на стриме, который открыли мы (срез 4c, 2026-09-20).
- [x] Incoming unidirectional streams — обнаружение + header-парсинг + `incomingUnidirectionalStreams` (срез 4d, 2026-09-20).
- [x] Incoming bidirectional streams — та же машинерия обнаружения + регистрация send-half под id, который выбрал пир (срез 4e, 2026-09-20).
- [x] Datagrams — приём и отправка через RFC 9297 §2.1 quarter stream id, `datagrams.readable`/`.writable` живые (срез datagrams-b, 2026-09-20).
- [ ] Lifecycle (срез 5) — остаётся.
- [x] `CAPABILITIES.md` — WebTransport 🟡 (`ready` живой, uni+bidi write+bidi read+incoming uni+incoming bidi+datagrams живые, lifecycle ещё стаб).
