# BUG-1209 — за HTTP-прокси пул соединений ключуется адресом прокси: CONNECT-туннель к одному хосту переиспользуется для другого (421), HTTP/2 не мультиплексируется

**Статус:** OPEN
**Компонент:** network (`crates/network/src/lib.rs` — `fetch_single`: `PoolKey` из `connect_host/connect_port`
= адрес прокси; `pool.acquire(&key)` / `pool.release(key, conn)`; `h2_pool` выключен при `effective_proxy`)
**Найден:** P2, повторный прогон top100, 2026-09-28

## Симптом

Прогон `perf_audit.py --mode compat` top100 через локальный HTTP-прокси (`proxy = "http://127.0.0.1:8900"`
в `data/fingerprint.toml`, прокси — [`scripts/split_proxy.py`](../scripts/split_proxy.py) с логом каждого
`CONNECT`), dev-release `8bd5c5dd1`, без блокировщика:

- 40 ответов `421 Misdirected Request` на 8 сайтах (twitch, bbc, spotify, zillow, soundcloud, yahoo-jp,
  baidu, tradingview, tumblr); в прогоне 2026-09-23 (другой маршрут, см. журнал) — ни одного.
  На twitch `421` получают `assets.twitch.tv` (4 из 4 скриптов/иконок) и `gql.twitch.tv` — страница пустая.
- Хосты, ответившие Lumen, но ни разу не получившие собственного `CONNECT` в логе прокси:
  twitch — `gql.twitch.tv`; bbc — `www.googletagmanager.com`, `static.chartbeat.com`, `jssdks.mparticle.com`,
  …; soundcloud — `sb.scorecardresearch.com`, `dn0qt3r0xannq.cloudfront.net`, … Именно они и получают `421`.
- Соединений на хост — 10.9 (8944 `CONNECT` на 824 пары сайт/хост); пиковое на хост: nytimes
  `www.nytimes.com` 478, x.com `abs.twimg.com` 454. После PERF-13 без прокси ожидается около одного.

## Причина (по коду)

`PoolKey { host: connect_host, port: connect_port, is_tls: connect_is_tls }`, а за HTTP-прокси это
`(proxy.host, proxy.port, false)`. Установленный `CONNECT host-A` + TLS к host-A уходит в пул под ключом
прокси и следующим `pool.acquire(&key)` достаётся запросу к host-B: запрос с `Host: host-B` идёт по
TLS-сессии host-A. Сервер с другим сертификатом отвечает `421` (RFC 9110 §15.5.20) — это ещё мягкий
исход: общий для нескольких доменов CDN может ответить от имени чужого origin, и cookies/`Authorization`
host-B уходят не туда.

Второе: `h2_pool` за прокси выключен сознательно (комментарий PERF-13: «`key` names the proxy»), поэтому
каждый h2-запрос открывает новый `CONNECT` — прирост PERF-13 за прокси теряется полностью.

## Что сделать

Ключ пула за HTTP-прокси — целевой origin (`host`, `port`, `is_tls`) плюс идентичность прокси, а не
адрес прокси; тогда и `h2_pool` за прокси можно включить. Тест: два последовательных запроса к разным
https-хостам через тестовый CONNECT-прокси — два `CONNECT`, ни один запрос не уходит в чужой туннель.
