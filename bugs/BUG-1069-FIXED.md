# BUG-1069 — wptrunner-прогон: все `.https.`-тесты падают `ERROR` — тестовый сертификат не покрывает хост `localhost`

**Статус:** FIXED 2026-09-22 (P2)
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

## Починка (2026-09-22)

Тестовый сертификат `tests/wpt/certs/host-cert.pem` перевыпущен (тот же рецепт, что в
`certs/README.md`, только SAN расширен): `subjectAltName=IP:127.0.0.1,DNS:web-platform.test,
DNS:127.0.0.1,DNS:localhost,DNS:*.localhost`. `ca-cert.pem` — копия, как и раньше (см.
`certs/README.md` про то, зачем нужен отдельный файл). Только тулинг, ни одна строка в
`crates/` не менялась.

**Проверено смоуком** (`run_smoke.py`, тот же `dev-release`, без пересборки — Rust не трогался):
- `/fedcm/fedcm-abort.https.html` — было `ERROR`/`certificate not valid for name "localhost"`,
  стало `TIMEOUT` (страница доходит до тела теста; FedCM API не реализован — отдельный,
  ожидаемый недочёт, не эта категория). В логе прогона нет ни одного упоминания
  `TLS`/`certificate`/`invalid peer`.
- `/service-workers/service-worker/activation-after-registration.https.html` — было `ERROR`
  на TLS, стало **unexpected PASS** (harness `OK`, сам тест зелёный): полный TLS-рукопожатие
  до тела теста, тело теста тоже отработало.

Оба подтверждают: TLS-рукопожатие для `https://localhost:PORT/...` теперь проходит проверку
имени, хендшейк для `*.localhost`-поддоменов не проверялся отдельно (сама доступность
`www1.localhost` и т.п. на этой машине упирается в нерезолвящийся DNS — [BUG-1070](BUG-1070-OPEN.md),
не в сертификат).

## Следствие для baseline (не сделано этим заходом)

Починка даёт массовый unexpected-PASS/смену статусов по ~2000 `.ini` во всех уже закрытых
срезами WPT-RUN-7 категориях, где хотя бы часть файлов `.https.` — baseline записывал `ERROR`
как «сегодняшнюю правду движка», а она была лишь артефактом несовпадающего сертификата.
Регенерация всех затронутых категорий — отдельная большая задача (`--update-expected` заново
по каждой, затем `--check`), не входит в этот фикс; она войдёт в срезы WPT-RUN-7 после этого,
начиная с `service-workers` (срез 56 — единственная категория, где влияние измерено на 100%
файлов) как самого быстрого и однозначного подтверждения. `docs/tasks/p2-test-track.md` —
живой список.

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
- `pointerevents` (срез 44 WPT-RUN-7, 2026-09-21): **24 id из 258** — те же `TLS handshake … not valid for name "localhost"` (`coalesced_events_attributes.https.html`, `pointerevent_pointerrawupdate*.https.html`, `pointerlock/*.https.html`, `idlharness.https.window.html` и др.); остальные `ERROR` категории — не сертификат, а [BUG-1065](BUG-1065-OPEN.md)/[BUG-1063](BUG-1063-OPEN.md).
- `workers` (срез 46 WPT-RUN-7, 2026-09-21): **25 id из 337** — 14 `*.any.serviceworker.html` и 11 `*.https.*` (`Worker-creation-happens-in-parallel.https.html`, `same-site-cookies/*.https.window.html`, `postMessage_block.https.html` и др.) — `TLS handshake … not valid for name "localhost"` на `https://localhost:18443/workers/…`; ещё 6 harness-`ERROR` категории (`modules/{dedicated,shared}-worker-import-{csp,referrer}.html`, `semantics/structured-clone/{dedicated,shared}.html`) — по имени не https, причина не разбиралась.
- `editing` (срез 47 WPT-RUN-7, 2026-09-21): **10 id из 700** — `TLS handshake … not valid for name "localhost"` на `https://localhost:18443/editing/…`
  (`edit-context/edit-context-execCommand.tentative.https.html`, `plaintext-only/paste.https.html?white-space=pre` и др.); baseline записан как `ERROR` — нижняя планка.
- `service-workers` (срез 56 WPT-RUN-7, 2026-09-22): **328 id из 328 — вся категория**, без единого исключения. Service workers требуют secure context, поэтому весь вендоренный поднабор (`RUNNABLE_ITEM_TYPES`-отфильтрованный) — `.https.`-only; `--check` дважды подряд дал 0 регрессий на baseline, где каждый файл — `expected: ERROR` с идентичным сообщением `certificate not valid for this hostname`. Крупнейшая на сегодня категория, где эффект бага измерен не частично, а на 100% файлов — см. `docs/tasks/p2-test-track.md#test-3-срез-56-2026-09-22`.
