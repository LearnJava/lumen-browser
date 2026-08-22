# BUG-844 — EventSource: поток, законченный телом ответа (`Content-Length`, а не закрытием сокета), не считается законченным — ни `error`, ни переподключения; а на переподключениях, которые всё же происходят, не стреляет `open`

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером, есть маркер `eventsource-no-reconnect`)
**Область:** `crates/network/src/sse.rs:393` (переподключение — только по завершению чтения соединения), `crates/js/src/dom.rs:7521` (`_lumen_sse_pump_one`, ветка `ev.t === 'open'`), `crates/js/src/dom.rs:7538` (ветка `close` — переподключение шима)
**Владелец:** P1/P3 (`lumen-network` + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Ответ вида «HTTP/1.1 200, `Content-Type: text/event-stream`,
`Content-Length: N`, соединение keep-alive» — именно так отвечает **любой**
`.py`-обработчик под `wptserve` (`eventsource/resources/message.py`,
`last-event-id.py`):

```js
var es = new EventSource('resources/message.py?message=retry%3A3000%0Adata%3Ax');
es.onopen = () => console.log('open');       // приходит ровно один раз
// поток кончился — но readyState остаётся 1 (OPEN) навсегда,
// `error` не приходит, переподключения нет
```

А на пути, где поток заканчивается закрытием сокета, переподключение есть, но
`open` больше не стреляет ни разу.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 11 тиков). Пробный сервер
записывает каждое соединение, поэтому «переподключения не было» доказано
независимо от страницы:

| вариант | ожидалось | получено |
|---|---|---|
| `sse-lengthed` (форма `wptserve`) | `sse-open` ×2 | `sse-open` ×1, `sse-message data=lengthed`, `sse-lengthed-checked readyState=1`; сервер видит **одно** соединение за 6 с |
| `sse-reconnect-onopen` (поток закрыт сокетом) | `sse-open n=2 at≈400` | `sse-open n=1 at=32`, `sse-opens-total 1`; сервер видит **14** соединений с шагом ≈400 мс |

То есть обе половины контракта расходятся с наблюдаемой реальностью в разные
стороны: там, где переподключение обязано быть, его нет; там, где оно есть,
о нём не сообщают.

Контроль: доставка сообщений, `lastEventId` и отправка `Last-Event-ID` на
переподключении работают (`sse-basic`, `sse-id-reconnect` — заголовок
`Last-Event-ID: 42` виден на сервере), `retry:` без пробела после двоеточия
парсится верно (шаг 400 мс из `retry:400`).

## Причина (локализована чтением кода)

`crates/network/src/sse.rs:393` переподключается после того, как **чтение
соединения завершилось**. Для HTTP/1.0-ответа это закрытие сокета, а для
keep-alive-ответа с `Content-Length` конец тела не завершает соединение —
цикл продолжает ждать байты, которых не будет. Спека (HTML LS §9.2.5
«reestablish the connection») говорит про конец *потока*, а не сокета.

Второй фасет — в шиме:

```js
// dom.rs:7521
if (ev.t === 'open') {
    if (es._readyState === 2) { continue; }
    es._readyState = 1;
    _lumen_sse_fire(es, 'open', new Event('open', …));
```

сам по себе он корректен, но после переподключения из ветки `close`
(`dom.rs:7538`) новое соединение открывается вызовом `_lumen_sse_connect`
напрямую, и события `open` в очередь не попадает — измерение показывает 14
соединений и один `open`.

## Масштаб

Маркер `eventsource-no-reconnect` в `tests/wpt/timeout_audit.py` — **8 id**
остатка снимка WPT-RUN-5, то есть весь `eventsource`-остаток:
`format-field-retry`, `format-field-retry-bogus`, `format-field-id`,
`format-field-id-2`, `format-data-before-final-empty-line`,
`format-field-id-null.window`, `eventsource-reconnect.window`,
`eventsource-close.window`. Каждый из них ждёт либо второго `open`, либо
второго сообщения — то есть переподключения, которого под `wptserve` не
происходит вовсе.

## Направление починки (не предписание)

Считать поток законченным по концу тела ответа (в том числе при keep-alive),
а не только по закрытию сокета, и после переподключения класть в очередь
событие `open` — тем же путём, каким его кладёт первое соединение.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   sse-lengthed --variant sse-reconnect-onopen` — ожидаются несколько
   соединений в `sse connects` для первого и `sse-open n=2` для второго.
2. WPT: `run_report.py --all --root eventsource` — семейства
   `format-field-retry*`/`format-field-id*` должны перестать быть TIMEOUT.
