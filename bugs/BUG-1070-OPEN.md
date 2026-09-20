# BUG-1070 — `*.localhost` (`www.localhost`, `www1.localhost`, …) не резолвится на Windows: резолвер целиком полагается на `getaddrinfo`

**Статус:** OPEN
**Тип:** пробел реализации — RFC 6761 §6.3 разрешает приложению самому считать `*.localhost` loopback-адресом, а `SystemDnsResolver` этого не делает.
**Заведён:** 2026-09-20 (P2, WPT-RUN-7 срез 39, `connection-allowlist`; косвенно упомянут в [BUG-1038](BUG-1038-OPEN.md) как «нерезолвящиеся `www*.localhost`»)
**Область:** `crates/network/src/dns.rs::SystemDnsResolver::resolve` (и DoH-путь `doh.rs`, не проверялся).
**Владелец:** P3 (сеть).

## Симптом

```
fetch error: network error: resolve www.localhost:18300: network error: resolve www.localhost:
Этот хост неизвестен. (os error 11001)
```

`ping www.localhost` на этой машине (Windows 10 19045) даёт то же: имя не находится. `localhost`
резолвится через hosts-файл, поддомены — нет (Windows не реализует RFC 6761 §6.3 в резолвере).

Влияет и на реальных пользователей (`dev.app.localhost` — типичный адрес dev-сервера), и на WPT:
`browsers/lumen.py::env_options` (WPT-RUN-10) строит поддомены wptserve как `www.localhost`,
`www1.localhost`, `www2.localhost`, IDN-метки — на Windows-половине корпуса они все не достижимы,
так что подтесты «Navigation to http://www1.localhost:18300 should fail.» в `connection-allowlist`
получают `TIMEOUT`/`NOTRUN` по причине окружения, а не движка. Отсюда же плавающие статусы
(TIMEOUT ↔ NOTRUN) в `navigation-wildcard`/`navigation-response-origin` (срез 39).

## Ожидание

Имена `localhost` и `*.localhost` (регистронезависимо, с хвостовой точкой) резолвятся в `127.0.0.1`/`::1`
внутри движка, без обращения к системному резолверу — как в Chromium и Firefox.
`origin.rs::is_potentially_trustworthy` уже считает `*.localhost` доверенными; резолвер расходится с ним.

## Не проверялось

- Путь DoH (`doh.rs`) и proxy-путь: обходят ли они `SystemDnsResolver`.
- Как это меняет WPT-статусы: на Linux/glibc `*.localhost` резолвится системой, так что цифры двух машин
  расходятся именно этим (см. `browsers/lumen.py::env_options`, «Windows' `*.localhost` resolution is not validated here»).
