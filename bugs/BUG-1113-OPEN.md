# BUG-1113 — Chrome-профиль HTTP-заголовков выдаёт не-браузер: `DNT: 1` и UA `Chrome/130` → антибот 403/401/RST

**Статус:** OPEN
**Заведён:** 2026-09-23 (P2, прогон top100-foreign против видимого Chrome 153,
[журнал](../docs/perf/journal.md) §2026-09-23 top100 split).
**Область:** network (`crates/network/src/http/headers.rs::build_request_headers`
— ветка `HttpProfile::Chrome | Strict`, строки ~177-178; `crates/network/src/http/mod.rs:39`
— `CHROME_USER_AGENT`).

## Симптом

В прогоне 100 сайтов пять сайтов отказали Lumen, но открылись у Chrome 153 в том же
маршруте (трафик обоих мимо VPN-туннеля, через один и тот же локальный прокси):

| Сайт | Lumen | Chrome 153 (видимое окно) |
|---|---|---|
| zillow.com | `← 403` → SITE_REFUSED | 1326 узлов DOM, 189 ресурсов |
| reuters.com | `← 401` → SITE_REFUSED | 2434 узла, 65 ресурсов |
| accuweather.com | `← 403` → SITE_REFUSED | 420 узлов, 50 ресурсов |
| adobe.com | `✗ H2 RST_STREAM on stream 1: error_code=0x2` → NET_FAIL | 1135 узлов, 74 ресурса |
| washingtonpost.com | `✗ H2 RST_STREAM on stream 1: error_code=0x2` → NET_FAIL | (Chrome: таймаут замера) |

Логи: `.tmp/perf-audit/20260923-210653/live.stderr.<slug>.0.log` в worktree аудита.

## Корень (установлен опытом, не по коду)

Матрица «TLS-отпечаток × набор заголовков», по 2 повтора на ячейку, тот же маршрут
(`.tmp/tlslab/antibot_matrix.py`). «Chrome TLS» — `curl_cffi impersonate=chrome`,
«простой TLS» — `httpx` h2 поверх OpenSSL (отпечаток не-браузерный, как у rustls):

| | Chrome TLS + заголовки Lumen | Chrome TLS + заголовки Chrome | простой TLS + заголовки Lumen | простой TLS + заголовки Chrome |
|---|---|---|---|---|
| zillow | 403 | **200** | 403 | **200** |
| reuters | 401 | **200** | 401 | **200** |
| accuweather | 200 | 200 | 403 | **200** |
| adobe | 200 | 200 | RST_STREAM | **200** |
| washingtonpost | 200 | 200 | RST_STREAM | **200** |

Решают **заголовки**, не TLS: с заголовками Chrome проходят все пять даже на
не-браузерном TLS. Бисект по одному заголовку от набора Lumen
(`.tmp/tlslab/header_bisect.py`, простой TLS):

| Вариант (от заголовков Lumen) | zillow | reuters | accuweather | adobe | wapo |
|---|---|---|---|---|---|
| как есть | 403 | 401 | 403 | RST | RST |
| **UA → Chrome/153** | 200 | 200 | 200 | 200 | 200 |
| **без `DNT`** | 403 | 401 | 200 | 200 | 200 |
| + `sec-ch-ua` (130) | 403 | 401 | 200 | 200 | 200 |
| без `Cache-Control` / + `Upgrade-Insecure-Requests`+`Sec-Fetch-User` / + `zstd` | без изменений ||||

Две независимые причины:

1. **`DNT: 1`.** Комментарий в `headers.rs` («Chrome sends by default») неверен:
   Chrome по умолчанию DNT **не шлёт** (эхо-сервер, headless Chrome 153 — заголовка нет).
   `DNT: 1` при UA Chrome — несовместимая комбинация, по ней Akamai режет
   accuweather/adobe/washingtonpost.
2. **UA `Chrome/130` отстаёт на 23 мажорные версии** от текущего стабильного Chrome (153).
   zillow и reuters отказывают только по нему; при `Chrome/153` проходят все пять.
   Константа захардкожена и не обновляется.

## Что сделать

- Убрать `DNT` из `HttpProfile::Chrome` (оставить только там, где он есть у эмулируемого
  браузера; `Strict` — отдельное решение, у него свой сигнал `Sec-GPC`).
- Поднять `CHROME_USER_AGENT` (и `Accept`, и `Edg/130` в Edge-профиле) до текущей
  стабильной версии; завести способ не отставать (тест на «возраст» константы или
  общий источник версии с `sec-ch-ua`).
- Перепроверить после фикса тем же `header_bisect.py` / прогоном пяти сайтов.

## Не входит

ebay.com и stackoverflow.com отдают 403 даже Chrome-TLS + Chrome-заголовкам: это
JS-челлендж Akamai / Cloudflare, для прохождения которого нужно отрендерить тело
403-ответа — отдельный дефект [BUG-1114](BUG-1114-FIXED.md).

## Ещё сайт: khanacademy (2026-09-25, P6, перемер при BUG-1120)

khanacademy отдаёт в `__KA_DATA__` флаг `KA-is-unsupported-browser: true` и рисует баннер
«Unsupported browser / Upgrade your browser» вместо главной. Флаг ставит сервер по UA:
Chrome 153 (видимое окно, `--user-agent=…`) с `Chrome/130.0.0.0` получает `true`,
с `Chrome/140.0.0.0` — `false`, со своим UA — `false`. Lumen шлёт `Chrome/130` из
`CHROME_USER_AGENT`. Критерий фикса дополняется: на khanacademy `KA-is-unsupported-browser` — `false`.
