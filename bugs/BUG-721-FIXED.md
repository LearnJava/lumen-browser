# BUG-721: `fetch()` отдавал тело чужого ответа — все тела до 64 КиБ читались из общего слота `FetchCache`

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` — `Response._fromFetchCache`, `Response.prototype._consumeBody`)
**Найден:** P3, при разборе [BUG-703](BUG-703-OPEN.md), 2026-08-09

## Симптом

На странице с несколькими параллельными `fetch()` ответы перемешиваются:
`resp.text()` возвращает тело того запроса, который завершился последним,
а не своё. Статус, `url` и заголовки при этом свои — расходится только тело,
поэтому отказ выглядит как «сервер отдал не то» и не ловится ни одним
обычным каналом.

Замер на `https://www.tbank.ru/` (лог живой пробы, 25 с после `document_ready`):
20+ разных URL получили одно и то же тело в 1447 байт — JSON-конфиг
cookie-consent, — в том числе webpack-чанк
`tramvai-web-performance-rum.<hash>.chunk.js` и все микроблоки `boxy/mm/*`;
`/sw.js` получил 135 731-байтовый бандл cookie-consent. Тела больше 64 КиБ
(`form-debit-card.client.js` — 1 980 593 байта и т.п.) приходили правильные.

## Механизм

`Response._fromFetchCache` устроен верно по замыслу: `_lumen_stream_alloc()`
копирует тело из единственного глобального слота `FetchCache` в персональный
слот ответа, чтобы следующий `fetch()` не мог его затереть. Ломает всё
**eager pull** в конструкторе `ReadableStream` (`dom.rs`, «Eagerly fill: call
pull once after start»):

1. `_fromFetchCache` создаёт `r.body = new ReadableStream({pull: …})`;
2. конструктор тут же зовёт `pull` один раз — для тела не длиннее
   `_RS_CHUNK` (64 КиБ) в очередь потока уходит **всё** тело;
3. `pull` видит `pos >= totalLen` и вызывает `freeHandle()`:
   `_lumen_stream_free(handle)` + `r._stream_handle = 0`;
4. всё это — синхронно внутри `_fromFetchCache`, то есть ещё до того, как
   `fetch()` зарезолвил промис;
5. позже `_consumeBody()` видит `_stream_handle === 0`, считает ответ
   «legacy-вызовом без слота» и читает `_lumen_fetch_body_length()` /
   `_lumen_fetch_body_chunk()` — **общий слот `FetchCache`**, в котором к
   этому моменту лежит тело другого запроса.

Отсюда и порог: тела длиннее 64 КиБ требуют нескольких `pull`, слот доживает
до `_consumeBody` и читается правильно. Пустой ответ (слот не выделяется,
`_lumen_stream_alloc()` → 0) тоже уходил в общий слот и получал чужое тело
вместо пустого.

## Фикс

`_fromFetchCache` помечает ответ флагом `_from_fetch_cache`. `_consumeBody`,
не найдя персонального слота у такого ответа, собирает тело из очереди его
собственного `ReadableStream` (`body._rs_ctrl._queue`) — там к этому моменту
лежат ровно все байты, потому что слот освобождается именно в тот момент,
когда последний байт попал в очередь. Ветка общего слота остаётся только для
настоящих legacy-вызовов, у которых `_body === null` и потока нет.

Тест: `dom::tests::v8_whatwg_streams::fetch_cache_response_reads_own_stream_queue_not_global_slot`
(ответ с `_from_fetch_cache`, `_stream_handle = 0` и двумя чанками в очереди
обязан дать их конкатенацию, а не пустое тело из общего слота).
`cargo test -p lumen-js --features v8-backend --lib dom::` — 1218/1218.

## Проверка на живой странице

`https://www.tbank.ru/`: до фикса — 19 `Uncaught SyntaxError: Unexpected
token ':'` из `_lumen_script_execute_classic` (JSON вместо JS) и
`ChunkLoadError: Loading chunk tramvai-web-performance-rum failed.
(missing: null)` в телеметрии приложения; после — 0 `SyntaxError`, каждый URL
получает своё тело, `ChunkLoadError` исчез. Рендер главной страницы этим
не чинится — остаток см. [BUG-703](BUG-703-OPEN.md).
