# Задача: WebTransport (поверх HTTP/3)

**Developer:** P1
**Ветка:** `p1-webtransport`
**Размер:** M (крупная; фактически заблокирована — см. зависимость)
**Крейты:** `lumen-js`, `lumen-network`

## Goal
Реализовать W3C WebTransport (§3–5): `new WebTransport(url)` над HTTP/3 (Extended CONNECT,
RFC 9220) с datagrams и uni/bidirectional QUIC-стримами, `ready`/`closed` промисами.

## ⚠️ Зависимость
**Блокируется задачей P3-h3 (HTTP/3), которая СЕЙЧАС active.** WebTransport требует
живого QUIC/HTTP-3 соединения. На 2026-07-05 HTTP/3 в Lumen — это большой набор
**чистых кодеков без сетевого IO** (см. `crates/network/src/h3/mod.rs:1-189`, срезы 1–41):
varint/frame/qpack/quic_frame/packet/пакетная защита/TLS 1.3 key schedule и т.д.
**Живого QUIC-эндпоинта (сокет + handshake + connection loop) ещё нет.** До появления
рабочего QUIC-транспорта WebTransport реализовать нельзя — только каркас JS-API.

## Current state (сверено с кодом 2026-07-05)
- `crates/js/src/webtransport.rs:1-108` — **чистая заглушка Phase 0**: все операции
  reject/throw `WebTransportError('… no QUIC …', 'phase-0-stub')`.
  - `createBidirectionalStream()` / `createUnidirectionalStream()` → `Promise.reject`
    (`webtransport.rs:61-78`).
  - `get ready` → reject (`:80`), `get closed` → resolve immediately (`:86`),
    `datagrams.readable/writable.read/write` → throw (`:24-34`).
  - Стейт-машина только декоративная (`state = 'connecting'`, `:50`).
- `crates/network/src/h3/` — 41 срез pure-кодеков; **нет** живого QUIC connection loop,
  нет сокета, нет Extended CONNECT, нет datagram flush в сеть.
- `crates/network/src/h3/datagram.rs` — есть (QUIC DATAGRAM frame кодек, RFC 9221),
  но это тоже pure-codec, не привязан к сокету.

## Entry points
- `crates/js/src/webtransport.rs:5` — `install_webtransport_bindings` (точка замены шима).
- `crates/network/src/h3/mod.rs` — публичная поверхность HTTP/3 (кодеки).
- `crates/network/src/h3/datagram.rs` — QUIC DATAGRAM кодек (для WT datagrams).
- Будущий QUIC connection loop (пока не существует) — главный blocker.

## Срезы (декомпозиция)
### Срез 0 — БЛОКЕР — Дождаться живого QUIC/H3 IO от P3-h3
Пока `h3/` = только кодеки, срезы 2–5 невозможны. Зафиксировать зависимость в ROADMAP,
не начинать реализацию транспорта.

### Срез 1 — S — Каркас JS-API без «phase-0-stub» текстов
Переписать шим `webtransport.rs` в правильную структуру классов
(`WebTransport`, `WebTransportBidirectionalStream`, `WebTransportDatagramDuplexStream`,
`WebTransportError` с `source`/`streamErrorCode`), где методы дергают нативные биндинги.
Пока QUIC нет — биндинги возвращают «not connected», но форма API уже спек-корректна.

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
- [ ] Зависимость P3-h3 → живой QUIC IO явно отражена (срез 0), задача не стартует раньше.
- [ ] Шим переписан в spec-форму классов (срез 1) — уже можно без QUIC.
- [ ] (после QUIC) Extended CONNECT, uni/bidi streams, datagrams, lifecycle.
- [ ] `CAPABILITIES.md` — WebTransport 🟡 (каркас) / ✅ (после полной реализации).
