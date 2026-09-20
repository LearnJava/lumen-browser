# BUG-1069 — wptrunner-прогон: все `.https.`-тесты падают `ERROR` — тестовый сертификат не покрывает хост `localhost`

**Статус:** OPEN
**Тип:** дефект тулинга (`tests/wpt/certs/`, `tools/wptrunner/wptrunner/browsers/lumen.py::env_options`), не движка.
**Заведён:** 2026-09-20 (P2, WPT-RUN-7 срез 39, `connection-allowlist`)
**Область:** `tests/wpt/certs/host-cert.pem` (SAN `IP:127.0.0.1, DNS:web-platform.test, DNS:127.0.0.1`) против
`browser_host = "localhost"` (WPT-RUN-10, 2026-09-04).
**Владелец:** P2 (тулинг WPT).

## Симптом

Каждая навигация на `https://localhost:18443/...` кончается TLS-ошибкой ещё до отправки запроса:

```
navigate: navigation failed: network error: TLS handshake: invalid peer certificate:
certificate not valid for name "localhost"; certificate is only valid for IpAddress(127.0.0.1),
DnsName("web-platform.test") or DnsName("127.0.0.1")
```

Доверие к корню при этом есть (`LUMEN_EXTRA_CA_CERT`, BUG-785) — отказ именно по имени. Файл
целиком получает `ERROR`, ни один подтест не стартует.

**Измерено (`connection-allowlist`, 2026-09-20):** 27 из 27 файлов с `expected: ERROR` — `.https.`-тесты,
и в логе прогона ровно 27 уникальных `https://localhost:18443/...` с этой ошибкой.

**Оценка масштаба (не проверена по каждому файлу):** в `tests/wpt/metadata/` 1992 из 2024 `.ini`
с `https` в имени имеют файловый `expected: ERROR` — то есть в baseline записаны как «сегодняшняя
правда движка» тесты, которые на самом деле не дошли до движка вообще.

## Почему раньше не всплыло

`tests/wpt/certs/README.md` объясняет расхождение SAN и `browser_host` как «moot — сертификат
отвергается раньше проверки имени» (написано, пока не было BUG-785 и корень не был доверенным). С BUG-785
это утверждение устарело: корень доверенный, и проверка имени — единственное, что осталось.

## Ожидание

`.https.`-тесты доходят до тела теста. Вероятная починка — только тулинг: перевыпустить локальный
самоподписанный сертификат с SAN `localhost`, `*.localhost` (поддомены `www`, `www1`, `www2`, IDN-метки
строятся wptserve префиксом на `browser_host`) и `127.0.0.1`, обновить `README.md`. Rust-часть не нужна.

## Следствие для baseline

Починка даст массовый unexpected-PASS/смену статусов по ~2000 `.ini`: baseline надо будет
регенерировать по всем затронутым категориям. Это отдельная большая задача, а не правка одного
файла — заводится как таковая при взятии бага.

## Не проверялось

- Что после смены SAN тесты действительно доходят до утверждений (нужна проба на одном
  `.https.`-файле: `run_smoke.py <id>` с новым `--host-cert-path`).
- Что все 1992 файла упираются именно в имя, а не в другую причину (проверены `connection-allowlist` — 27 файлов
  и `signed-exchange` — 27 файлов, срез 40 WPT-RUN-7, 2026-09-20; во втором случае `.https.` в имени только у 8
  из 27, у остальных 19 страница `http://`, а `.sxg` грузится в `iframe` по `https://localhost:18443/…` и падает
  той же TLS-ошибкой — так что оценка по имени файла занижает охват. Соответствие файл→строка лога там
  подтверждено по счёту (18 загрузок `.sxg` + 4 из `service-workers/` + 6 `.https.`-страниц), не по каждому файлу;
  и `fedcm` — 81 файл из 81, срез 41 WPT-RUN-7, 2026-09-20: 81 уникальный `https://localhost:18443/…` в логе,
  все с этой ошибкой, других причин нет — категория целиком `expected: ERROR`, ни один подтест не стартует;
  и `shared-storage` — 88 `.https.`-файлов из 90, срез 42, 2026-09-20: 264 строки `ExecutorException` = 88×3, все
  с этой ошибкой;
  и `websockets` — срез 43 WPT-RUN-7, 2026-09-20: в логе 430 уникальных `https://localhost:19000/…` (весь класс `?wpt_flags=h2`, HTTP/2-сервер wptserve)
  и 56 `https://localhost:18443/…`, все с этой ошибкой; harness-`ERROR` из-за неё — ~224 секций из 333, остальные 108 — не сертификат, а [BUG-1071](BUG-1071-OPEN.md)).
- Пробный перевыпуск сертификата (SAN + `localhost`, `*.localhost`; срез 41→42, 2026-09-20, не закоммичен):
  на `fedcm` TLS-ошибка исчезает (0 из 81), файлы доходят до страницы и дают `TIMEOUT` на
  `testharnessreport.js` вместо `ERROR` — то есть после починки baseline сдвинется `ERROR → TIMEOUT` (или лучше),
  а не останется прежним. Не проверено на других категориях.
- **Второй артефакт `browser_host = "localhost"`, не связанный с сертификатом:** `http://localhost` в движке —
  secure context (`crates/network/src/origin.rs::is_potentially_trustworthy`, как в спеке), поэтому тесты, которые
  ждут *insecure* context на `.http.`-странице (`shared-storage/insecure-context.tentative.http.html`,
  `…-writable-insecure-context…http.sub.html`), получают `FAIL` независимо от сертификата — верный ответ движка на
  неверный хост. Починка сертификата этого не лечит.
- `*.localhost` в SAN не решает нерезолвимость самих поддоменов на Windows — см. [BUG-1070](BUG-1070-OPEN.md).
