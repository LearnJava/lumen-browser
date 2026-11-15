# BUG-1817 — `Referer`/`document.referrer` не обновляются после `history.replaceState`

**Статус:** OPEN
**Тип:** дефект реализованного кода — `history.replaceState` URL меняет, но источник referrer-а остаётся снимком URL на момент загрузки страницы.
**Заведён:** 2026-11-15 (P3, срез 1 [BUG-1235](BUG-1235-OPEN.md))
**Область:** shell (`crates/shell/src/resource_base.rs` — `ResourceBase` хранит снимок URL документа, от которого `HttpClient::with_document_context` строит `Referer`; клиент JS `fetch`/XHR — `page_pipeline.rs`). До конкретной строки не локализовано.

## Симптом

`location.href` после `history.replaceState(null, null, "/AAAA…")` меняется штатно (проверено пробой), а исходящий `Referer` сабресурсов (и, соответственно, `document.referrer` у получателя) остаётся от URL, который был на момент загрузки документа.

## Прямое измерение

`referrer-policy/4K` (2026-11-15, dev-release, Windows, `run_report.py --all --root referrer-policy/4K --recursive --processes 7`): 96/108 harness OK, 168/392 сабтестов PASS, 1:45. Из 224 непрошедших **172** — ровно эта форма.

`4K/generic/test-case.sub.js::runLengthTest` делает `history.pushState(null, null, "/")`, затем
`history.replaceState(null, null, "A".repeat(urlLength - location.href.length))` (то есть 128 символов
на origin этой страницы), и ждёт в referrer-е `http://web-platform.test:18300/AAAA…`. Фактически
приходит `http://web-platform.test:18300/referrer-policy/4K/gen/top.http-rp/<policy>/<type>.http.html`
— URL до `replaceState`.

По типам сабресурса (разрез того же прогона): `iframe-tag` 64, `fetch` 24, `script-tag` 24, `xhr` 24,
`worker-classic` 12, `worker-module` 12, `sharedworker-classic` 12.

## Как проверить

`python tests/wpt/run_report.py --binary target/dev-release/lumen.exe --all --root referrer-policy/4K --recursive --processes 7 --log-raw <файл>`;
в `<файл>` у `fetch`/`xhr`-сабтестов с `no-redirect` смотреть `assert_in_array: document.referrer value …`.
После починки — регенерация baseline `referrer-policy` и `mixed-content` (см. пункт 1 [BUG-1235](BUG-1235-OPEN.md)).
