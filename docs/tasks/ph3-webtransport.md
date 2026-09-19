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

### Срез 3 — M — Uni/Bidirectional streams
`createUnidirectionalStream`/`createBidirectionalStream` → реальные QUIC-стримы,
обёрнутые в WHATWG ReadableStream/WritableStream (переиспользовать stream-инфраструктуру
из `dom.rs`). **Требует срез 2.**

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
- [ ] (после QUIC) Extended CONNECT, uni/bidi streams, datagrams, lifecycle.
  Остаётся срезам 2–5 — см. таблицу выше.
- [x] `CAPABILITIES.md` — WebTransport 🟡 (каркас).
