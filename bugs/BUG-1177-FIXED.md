# BUG-1177 — HTTP/2: запросы из очереди общего соединения теряются с `connection closing` вместо повтора

**Статус:** FIXED 2026-09-28 (P3)
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

## Исправление (2026-09-28, P3)

Мультиплексор помечал потерянные запросы правильно (`fail_queued` и `run` → `retryable`), терял их
`fetch_single`: повтор был один и только в ветке пула, а запрос, открывший соединение
(`Acquire::Connect` → `r.fulfill(mux)`), при ошибке возвращал её без проверки `retryable`.

- `h2::pool::send_resending` — один путь отправки для обеих веток: запрос из пула и запрос на
  только что открытом соединении (`opened`). Ошибка с `retryable` → `evict` мёртвого соединения и
  повтор на новом (запросы, потерянные одним обрывом, сходятся на одном новом рукопожатии через
  резервацию пула). Бюджет — `H2_RESEND_LIMIT = 2` на запрос, как `kMaxRetryAttempts` в
  Chromium `HttpNetworkTransaction`.
- `fetch_single`: H2-часть — цикл вокруг `send_resending`; открытое соединение идёт в следующий
  виток как `opened`, а не отправляется отдельным кодом без повтора.
- Не повторяется, как и раньше: неидемпотентный запрос и запрос, на который ответ уже начался
  (`response_started`) — это совпадает с Chrome.

**Тесты** (`h2::mux::tests`, h2c-стенд `serve_then_drop`):
`request_that_opened_a_dying_connection_is_resent`, `requests_lost_with_a_dying_connection_all_get_answers`
(6 запросов, 2 потока одновременно, обрыв после 4 ответов → все 6 получают ответ, 2 соединения),
`resend_budget_is_bounded`. С отключённым повтором падают все три; 40 прогонов подряд — зелёные.
Стенд закрывает сокет через `shutdown(Write)` + дочитывание: закрытие с непрочитанными кадрами
шлёт RST, и Windows выбрасывает у клиента ещё не прочитанные ответы (первая версия теста флакала 3/20).

**Живая проверка** (`--dump-layout`, dev-release, `LUMEN_NO_ADBLOCK=1`, чистый `data/`, VPN,
3 прогона): bbc — ни одного `✗`, `connection closing` нет. cnbc — `connection closing` нет, но:
прогон 3 — главный документ оборвался уже после начала ответа (не повторяется и в Chrome);
прогон 1 — соединение замолчало без FIN/RST и 34 потока ждали по 60 с → [BUG-1205](BUG-1205-FIXED.md).
