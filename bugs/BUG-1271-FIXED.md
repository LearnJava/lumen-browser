# BUG-1271 — wptrunner-прогон: https на поддоменах (`www1.localhost` и др.) падает `certificate not valid for this hostname` — wildcard `*.localhost` не принимается rustls-webpki

**Статус:** FIXED 2026-10-05 (P2)
**Заведён:** 2026-10-05 (P2, WPT-RUN-9 — полный прогон корпуса, наблюдение из `.tmp/wpt-run9.log`)
**Область:** `tests/wpt/certs/host-cert.pem` (SAN) — тулинг WPT, Rust не тронут.
**Владелец:** P2.

## Симптом

После [BUG-1069](BUG-1069-FIXED.md) (SAN дополнен `localhost`, `*.localhost`) и [BUG-1070](BUG-1070-FIXED.md)
(`*.localhost` резолвится в loopback внутри движка) поддомены wptserve стали достижимы, но любое https-обращение
к ним кончается TLS-ошибкой:

```
https://www1.localhost:18443/... (tls: TLS handshake: certificate not valid for this hostname)
```

В первых 19 из 211 единиц прогона WPT-RUN-9 (`referrer-policy`, `mixed-content`, `fetch`,
`content-security-policy`, `websockets`) — 2 311 таких строк в логах, 654 пары «шард × тест»; хосты
`www1.localhost:18443` (2 006), `www1.localhost:20443` (184), `www.localhost:20443` (101), `www.localhost:20444` (16),
`www.localhost.:20443` (4). Затронуты в первую очередь cross-origin-подтесты `referrer-policy/gen/**` и
`mixed-content/**` (общий хелпер `common/security-features/subresource/*.py` с `www1`).

## Причина

`rustls-webpki` (0.103, `subject_name/dns_name.rs`) вслед за NSS требует **не менее двух меток после `*`**:
`*.localhost` — одна метка, такой wildcard для сопоставления не используется вовсе. Поэтому SAN `DNS:*.localhost`,
добавленный в BUG-1069, ничего не покрывал: проверка того фикса шла только на `https://localhost:…`, поддомены
отдельно не проверялись (это прямо записано в `BUG-1069-FIXED.md`, «хендшейк для `*.localhost`-поддоменов не
проверялся отдельно»). Тогда же поддомены и не резолвились (BUG-1070), так что TLS-отказ был не виден за DNS-отказом.

## Исправление

Сертификат перевыпущен тем же рецептом (`tests/wpt/certs/README.md`), SAN дополнен **явным списком** всех поддоменов,
которые строит wptserve (`tools/serve/serve.py::_subdomains` — `www`, `www1`, `www2`, IDN `天気の良い日`/`élève` в
punycode, все одно- и двухуровневые сочетания: 30 имён) с суффиксом `.localhost`. `*.localhost` оставлен —
безвреден и пригодится, если клиент когда-нибудь станет мягче. `ca-cert.pem` — копия, как и раньше.
Не покрыты: `nonexistent.localhost` (так и задумано wptserve — `not_subdomains`), хвостовая точка
(`www.localhost.` — 4 строки, webpki имени с точкой не сопоставляет независимо от SAN) и альтернативный домен
`not-web-platform.test` (известный пробел WPT-RUN-10).

## Проверка

Проба на бинаре прогона WPT-RUN-9 (`d913a39b9`), отдельные порты (+4000, `LUMEN_WPT_SERVER_CONFIG`), параллельно
с идущим прогоном:

| тест | в прогоне WPT-RUN-9 (старый cert) | с новым cert |
|---|---|---|
| `/referrer-policy/gen/iframe.meta/same-origin/iframe-tag.http.html` | OK, 5/10 | OK, **9/10** |
| `/referrer-policy/gen/srcdoc.meta/no-referrer/img-tag.http.html` | TIMEOUT, 0/12 | TIMEOUT, 0/12 |

В логе пробы 0 строк `not valid for this hostname`, 21 обращение к `https://www1.localhost:22443` проходит
рукопожатие. `img-tag` остаётся TIMEOUT по другой причине (BMP-ответ хелпера — [BUG-1097](BUG-1097-FIXED.md)
закрыт, причина таймаута не разбиралась).

## Последствия

- Цифра WPT-RUN-9 (запущена 2026-10-05 12:25 со старым сертификатом из слота `p2-work`) занижена на https-поддоменах;
  прогон не перезапускается — шарды, отработавшие до замены, несут старое состояние. Перепрогнать затронутые
  категории (`referrer-policy`, `mixed-content`, `fetch`, `content-security-policy`, `websockets`, `service-workers`,
  `cookies`, …) после основного прогона: `run_corpus.py --resume` их не повторит, нужен отдельный запуск по этим корням.
- baseline `.ini` категорий с https-поддоменами (WPT-RUN-7) снят на старом сертификате — те же категории,
  что требовали перегенерации после BUG-1069, частично потребуют её снова.
