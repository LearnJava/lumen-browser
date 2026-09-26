# BUG-1177 — HTTP/2: запросы из очереди общего соединения теряются с `connection closing` вместо повтора

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, по ходу закрытия [BUG-493](BUG-493-FIXED.md); видимое окно `--maximized`,
`LUMEN_NO_ADBLOCK=1`).
**Область:** network (`crates/network/src/h2/mux.rs:395-398` — `fail_queued` отвечает
`MuxError::retryable("connection closing")`; `crates/network/src/lib.rs:1699-1723` — повтор
делается один раз и только в ветке пула).

## Симптом

bbc.com, 1 прогон из 3. К `static.files.bbci.co.uk` одним пакетом уходят 11 запросов по общему
HTTP/2-соединению (PERF-13). Соединение рвётся на стороне ОС:

```
✗ …/favicon-32x32.png (read: H2 I/O: Программа на вашем хост-компьютере разорвала установленное подключение. (os error 10053))
✗ …/_next/static/chunks/073u89hsadvk0.js (read: H2 connection unusable: connection closing)
✗ …/_next/static/chunks/0_a80-qmpyc3s.js (read: H2 connection unusable: connection closing)
[JS error] script load failed: …/073u89hsadvk0.js: TypeError: fetch: network error for …
[JS error] script load failed: …/0_a80-qmpyc3s.js: TypeError: fetch: network error for …
```

Остальные 8 запросов того же пакета — `200`. Два чанка Next.js потеряны; `curl` на те же URL
сразу после — `200 text/javascript`. Прогоны 2 и 3 — ни одного `✗`.

## Что известно

- `connection closing` — ответ `fail_queued`: запрос стоял в очереди мультиплексора (ещё не
  отправлен), соединение закрылось. Такой запрос сервер точно не видел, он помечен `retryable`.
- Повтор в `fetch_single` — **один** (`retried`), и только когда соединение взято из пула
  (`Acquire::Mux`). Запрос, который сам открыл соединение (ветка `conn.is_h2` после
  `Acquire::Connect`, `lib.rs:1852-1868`), повтора не получает вовсе: `Err(e) => Err(e.error)`
  без проверки `e.retryable`. Какая из двух веток сработала здесь, по логу не видно.
- Обрыв `os error 10053` — окружение (VPN), но потерю *невыполненных* запросов при обрыве
  Chrome не допускает: они уходят на новое соединение.

## Что сделать

1. Воспроизвести детерминированно: локальный H2-сервер, который после N-го потока закрывает
   TCP, при `SETTINGS_MAX_CONCURRENT_STREAMS` меньше числа запросов (чтобы часть стояла в очереди).
2. `retryable`-ошибку повторять на новом соединении в обеих ветках, в том числе для
   запроса-открывателя.

Критерий: на стенде п. 1 все запросы получают ответ; на bbc нет `connection closing`.

## Второй сайт (2026-09-26, P6): cnbc

При закрытии [BUG-648](BUG-648-FIXED.md) (видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`)
две `rel=preload`-загрузки cnbc (`static-redesign.cnbcfm.com/dist/main-….js`,
`…/92419-….js`) упали с `read: H2 connection lost: … H2 I/O: peer closed connection without
sending TLS close_notify`. Повтора на новом соединении не было, и шим сообщил `link hint fetch
failed`. Позже те же URL пришли `200`, уже через `<script src>`, так что страница собралась
(2650 узлов). Лишний запрос и событие `error` на `<link rel=preload>` при этом остались.
Триггер другой (обрыв TLS, а не `connection closing` из очереди), но вывод тот же: запрос,
потерянный из-за обрыва общего соединения, не повторяется.
