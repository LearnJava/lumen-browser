# BUG-1152 — все подресурсы уходят с `Sec-Fetch-Dest: document`, `Sec-Fetch-Mode: navigate`, `Sec-Fetch-Site: none`

**Статус:** OPEN
**Заведён:** 2026-09-24 (P6, при закрытии [BUG-1116](BUG-1116-FIXED.md) — сравнение заголовков на стенде)
**Область:** network (`crates/network/src/http/headers.rs::build_request_headers` — навигационный
набор `Sec-Fetch-*` вшит в профиль и не зависит от `RequestDestination`)

## Симптом

Стенд `.tmp/seqlab` пишет заголовки каждого запроса. Chrome 153:
`/async.js` → `script / no-cors / same-origin`, `/bg.png` → `image / no-cors / same-origin`,
`fetch('/api')` → `empty / cors / same-origin`. Lumen: у **каждого** запроса, включая скрипты,
картинки и `fetch()`, — `document / navigate / none`, как у перехода по адресной строке.

## Почему это важно

- Серверы и WAF по Fetch Metadata (`Sec-Fetch-Dest`/`Mode`/`Site`) решают, отдавать ли ресурс:
  «навигация» за картинкой или `fetch()` на API — типичный признак бота, запрос может получить
  403 или HTML-заглушку вместо ресурса.
- Отпечаток: сочетание Chrome-UA и навигационных заголовков на подресурсах выделяет Lumen из
  массы Chrome, против цели маскировки профиля.

Смежно с [BUG-1021](BUG-1021-OPEN.md) (подресурсы без `Origin`/`Sec-Fetch-Mode: cors`) — там про
CORS-режим шрифтов, здесь про весь набор Fetch Metadata на всех подресурсах.

## Что сделать

`build_request_headers` получает destination/mode/site запроса (Fetch Metadata §2):
`Sec-Fetch-Dest` из `RequestDestination` (`script`/`style`/`image`/`font`/`empty`…),
`Sec-Fetch-Mode` (`no-cors` для элементов, `cors` для `fetch()`/`@font-face`), `Sec-Fetch-Site`
по сравнению origin-ов инициатора и цели; `Sec-Fetch-User` только у навигаций пользователя.
Критерий: заголовки на стенде совпадают с Chrome 153 по всем 16 запросам.
