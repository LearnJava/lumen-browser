# BUG-1151 — `<link rel=preload as=fetch>` не отдаётся странице: `fetch()` на тот же URL идёт вторым запросом

**Статус:** FIXED 2026-09-30
**Заведён:** 2026-09-24 (P6, при закрытии [BUG-1116](BUG-1116-FIXED.md))
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b3.js` — `fetch()`, флаг `usePreloaded`
в `_lumen_fetch_async_start` сейчас выставляется только для внутренних загрузок элементов с
`_lumenInitiatorType`)

## Симптом

Стенд `.tmp/seqlab` (сервер отвечает через 700 мс, пишет каждый запрос), страница с
`<link rel=preload as=fetch href=/api crossorigin>` и скриптом `fetch('/api')`: у Lumen `/api`
запрашивается дважды (подсказка + `fetch()`), у Chrome 153 — один раз.

## Корень

После BUG-1116 байты подсказки лежат в `PREFETCH_CACHE`, но обычный `fetch()` страницы в него не
смотрит — намеренно: у `fetch()` свой mode/credentials/заголовки, а слот ключуется только URL,
и отдать `fetch()` ответ, полученный с другим режимом, нельзя.

## Что сделать

HTML LS §4.6.7 «consume a preloaded resource»: ключ записи — URL + destination + mode +
credentials mode. Хранить в слоте `as` и crossorigin-режим подсказки; `fetch()` берёт запись,
только если метод GET, нет тела, нет авторских заголовков и `(mode, credentials)` совпадают.
Запись потребляется один раз. Критерий: на стенде `/api` запрашивается один раз.

## Исправление (P3, 2026-09-30)

Шим ведёт реестр `_lumen_fetch_hints` (URL → режим credentials запроса подсказки): `_lumen_link_preload` пишет туда `as=fetch` с атрибутом `crossorigin` (без него подсказка `no-cors`, cors-`fetch()` её не берёт). `_lumen_fetch` через `_lumen_fetch_take_hint` берёт запись, только если GET, нет тела, нет авторских заголовков, mode=`cors`, credentials и cache совпадают; запись тратится в любом случае. Совпадение едет в натив третьим сегментом `load` (`mode|destination|preloaded`), `_lumen_fetch_async_start` тогда зовёт `fetch_preloaded` (успешный прогрев; 4xx/5xx слот не хранит как успех). Тесты — `dom::tests::v8_formdata::*preload*`.

**Остаток:** динамически вставленный `<link>` регистрирует подсказку синхронно, но байты кладёт через `setTimeout(0)`; `fetch()` сразу после `appendChild` не найдёт слот и уйдёт в сеть.
