# BUG-1098 — `fetch()` к редиректу на не-HTTP(S) схему детерминированно резолвится `undefined` вместо `TypeError` для первого теста серии

**Статус:** OPEN
**Тип:** дефект реализованного кода — синхронный путь `fetch()` (`_lumen_fetch` в
`crates/js/src/shim/web_api_shim_mid_b2.js`) либо `require_http_scheme`/
redirect-loop в `crates/network` не пробрасывают ошибку схемы в Promise для
первого вызова серии, когда один из последующих редиректов в файле ведёт на
`data:`
**Область:** JS shim (`crates/js/src/shim/web_api_shim_mid_b2.js:980-1237`,
`_lumen_fetch`, синхронная ветка `_lumen_fetch_sync`/`_lumen_response_from_fetch_cache`)
и/или network (`crates/network/src/lib.rs`, redirect-loop вокруг
`require_http_scheme`, `crates/network/src/lib.rs:479-509`)
**Владелец:** P1/P3 (`lumen-js` + `lumen-network`)
**Заведён:** 2026-09-22 (WPT-RUN-7 срез 55, категория `fetch`)

## Симптом

`fetch/api/redirect/redirect-schemes.any.js` гоняет 6 `promise_test`
последовательно (testharness.js — один тест за раз, `forEach` лишь
регистрирует их синхронно), каждый ожидает `TypeError` от
`fetch(".../redirect.py?location=<схема>")`, где сервер отвечает
редиректом на не-HTTP(S) схему (`mailto:`, `data:`, `facetime:`,
`about:blank`, `about:unicorn`, `blob:...`) — по Fetch §4.2 редирект-хоп
обязан провалиться независимо от того, фетчибельна ли эта схема сама по
себе (`data:` фетчибельна как top-level-запрос, но не как redirect-target).

Детерминированно (**3/3** изолированных прогона `run_smoke.py` только этим
файлом, без параллельных категорий) первый тест серии
(`Fetch: handling different schemes in redirects 1` — `tests[0]`, редирект
на `mailto:a@a.com`) падает:

```
FAIL Fetch: handling different schemes in redirects 1 - assert_unreached:
Should have rejected: undefined Reached unreachable code
```

— то есть промис `fetch()` для `mailto:`-редиректа **зарезолвился**
значением `undefined` вместо отклонения `TypeError`.

При этом консоль движка печатает ровно **5** строк
`fetch error: network error: unsupported scheme: <schema>` за прогон —
для `mailto`, `facetime`, `about` (дважды — `about:blank`/`about:unicorn`),
`blob`. Строки для `data` — **нет ни разу**: редирект на `data:,HI`
проходит без диагностики уровня "unsupported scheme", хотя
`require_http_scheme` (`crates/network/src/lib.rs:479-509`) в своей
докстрока explicitly называет `data:` примером bad scheme и утверждает,
что проверяется «на каждом redirect-hop». Похоже, редирект на `data:`
где-то перехватывается отдельной веткой (data: URL — легитимная
top-level-схема для `<img src="data:...">` и т.п.) и декодируется вместо
того, чтобы провалиться как redirect-target — это отдельно от собственно
репортящегося симптома (subtest 1, `mailto`), но обе аномалии наблюдаются
в одном и том же прогоне и, вероятно, связаны общей причиной в
redirect-loop.

Остальные 5 подтестов (`facetime`/`about:blank`/`about:unicorn`/`blob`,
плюс сам факт, что "unsupported scheme" для них корректно логируется)
проходят. `TEST_END: Test OK, expected ERROR. Subtests passed 5/6.
Unexpected 1` — стабильно, каждый раз именно `redirects 1` (`mailto`).

## Прямое измерение

WPT-RUN-7 срез 55, `fetch` (`--update-expected --all --root fetch
--recursive --processes 7 --binary target/dev-release/lumen.exe`): baseline
не зафиксировал этот файл как отклонение (тест PASS в исходном прогоне —
скорее всего, случайность конкретного запуска), но **все три** последующих
`--check` (независимые полные прогоны категории) стабильно репортуют его
как `REGRESSION: /fetch/api/redirect/redirect-schemes.any.html [Fetch:
handling different schemes in redirects 1]: expected PASS, got FAIL`. В
отличие от известного класса флапа [BUG-1022](BUG-1022-OPEN.md) (три
прогона дают три РАЗНЫХ набора регрессий на TIMEOUT-кластере
`fetch/orb/tentative`/`fetch/metadata/generated` — не тронуто этим багом,
см. заметку в `docs/tasks/p2-test-track.md#test-3-срез-55-2026-09-22`),
этот конкретный тест **одинаково** ловится на каждом из трёх `--check` и
трижды воспроизведён изолированно `run_smoke.py` (без соседних категорий,
без `--processes`) — детерминированный дефект, не флап окружения.

## Направление починки (не предписание)

Инструментировать `_lumen_fetch_sync`/redirect-loop логированием per-hop
результата scheme-check specifically для `mailto`- и `data`-веток одного
прогона этого файла: подтвердить (1) действительно ли `data:`-редирект
проходит мимо `require_http_scheme` отдельной веткой (объясняет
пропавшую 6-ю строку лога) и (2) почему промис ИМЕННО первого теста серии
получает `undefined` вместо `TypeError`, а не теста с `data:` (порядок
подтестов у `promise_test` последовательный, `forEach` только
регистрирует). Кандидат: `_lumen_response_from_fetch_cache`
(`web_api_shim_mid_b2.js`) читает состояние общего/синглтон-кэша фетча,
которое к моменту вызова для теста 1 могло быть перезаписано побочным
эффектом обработки другого redirect-hop до того, как `ok`-флаг для теста 1
дошёл до JS — то есть возможен gap в атрибуции результата запросу, а не
в самой scheme-проверке.

## Как проверить фикс

`tests/wpt/.venv/Scripts/python.exe tests/wpt/run_smoke.py --binary
target/dev-release/lumen.exe /fetch/api/redirect/redirect-schemes.any.html`
(без параллельных прогонов) — все 6/6 подтестов должны быть
`Subtests passed 6/6`, включая `redirects 1` (mailto) с корректным
`TypeError`, и лог должен содержать 6 строк
`unsupported scheme: <schema>` (добавляется `data`), не 5.
