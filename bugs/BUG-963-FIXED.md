# BUG-963: `ping` hyperlink-auditing attribute never sends a request

**Статус:** FIXED 2026-09-29 (P6)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js::_lumen_run_activation_behavior`, `crates/js/src/shim/web_api_shim_mid_b3.js::_lumen_fire_hyperlink_ping`, `crates/js/src/svg.rs::SVGAElement`)

## Симптом

Ни `<a ping="...">`/`<area ping="...">` (HTML), ни `<a ping="...">` в SVG-неймспейсе
не отправляют HTTP-запрос по клику — механизм hyperlink auditing (HTML LS
§4.6.9 "Ping") не реализован ни для одной формы. Для `<a>`/`<area>` контент-атрибут
`ping` СОДЕРЖИТСЯ в разметке и даже отражается как IDL-строка
(`_lumen_install_reflection(HTMLAnchorElement.prototype, [['ping', 'ping', 'string'], ...])`,
`crates/js/src/shim/web_api_shim_tail_b.js:1092`/`1104`) — значит `anchor.ping`
читается/пишется, но клик по ссылке не порождает ни одного запроса на URL(ы)
из атрибута. Для SVG `<a>` даже эта отражённая строка отсутствует: SVG-элементы
не получают `HTMLAnchorElement.prototype`, поэтому IDL-геттер/сеттер `ping`
там не существует вовсе.

## Пробой (2026-09-02, живой `--mcp-live-port`)

`tests/wpt/serve_wpt_like.py` (порт 8998) + `target/dev-release/lumen
--mcp-live-port 8999 http://127.0.0.1:8998/svg/linking/scripted/a.ping-functionality.html`.
Браузер завершил `testharness` штатно за ~9с (никакого зависания —
`PROBE harness-complete status=0 tests=3 …:1|…:1|…:1`, все три сабтеста FAIL,
не TIMEOUT):

1. «send ping on click» — `dispatchEvent(new MouseEvent('click', …))` по
   `<a ping="/xhr/resources/delay.py?ms=100">` внутри `<svg>`; `observe_entry`
   (PerformanceObserver на `resource`-записи с собственным 2-секундным
   `Promise.race`-таймаутом) ни разу не видит запись — сервер не получил ни
   одного запроса (`serve_wpt_like.py`'s access log пуст на `delay.py`).
2. «multiple ping URLs» — тот же результат для двух URL через пробел.
3. «ping IDL attribute should be settable» — синхронный `test()`,
   `document.createElementNS(SVG_NS, 'a').ping` бросается сразу на
   `assert_equals(anchor.ping, '…')`, т.к. геттера просто нет на SVG-обёртке.

Ни на одном из трёх сабтестов браузер не хендлит `ping` как сетевой
механизм — grep по `crates/` не находит ни одного `"ping"`/`'ping'` вне
той самой строки reflection-таблицы (`fn.*follow_hyperlink`,
`link_activation.rs`, click-обработчики `<a>` — ни одного упоминания).

## Почему это не объясняет исходный TIMEOUT

Этот id (`/svg/linking/scripted/a.ping-functionality.html`) входил в
40-элементный `residual_ids` снимка WPT-RUN-5 (см. [BUG-961](BUG-961-FIXED.md)
срез 48/50) как TIMEOUT, но живой пробой воспроизвести зависание НЕ
удалось — harness завершается штатно за ~9с, задолго до 10-секундного
бюджета. Это тот же паттерн, что и у `console-log-large-array`/
`canvas-with-padding` (BUG-961): TIMEOUT в реальном корпусном прогоне,
вероятно, — артефакт оркестрации запуска (`mozprocess`/параллельные
процессы), а не зависание внутри самого движка на этом тесте. `ping`
как GAP — реальный, но самостоятельный дефект, найденный попутно.

## Масштаб

`<a ping>`/`<area ping>` (HTML) и SVG `<a ping>` — оба неймспейса,
оба механизма (сетевой запрос по клику + IDL-отражение для SVG).
Затрагивает как минимум `svg/linking/scripted/a.ping-functionality.html`;
не проверено, есть ли отдельные HTML-фокусированные WPT для того же
механизма (`html/semantics/*ping*` не искался в этом срезе).

## Исправление (2026-09-29, P6)

`_lumen_run_activation_behavior`'s ветка `A`/`AREA`
(`web_api_shim_tail_b.js`) теперь зовёт новую
`_lumen_fire_hyperlink_ping(nid, targetHref)` (`web_api_shim_mid_b3.js`)
до вызова `_lumen_navigate_or_fragment` — она не зависит от исхода
навигации. Функция читает `ping`-атрибут, режет по ASCII-пробелам,
резолвит каждый токен относительно `_lumen_document_base_url()` и для
каждого URL заводит независимый `POST` через уже существующий async
fetch-мост (PERF-14: `_lumen_fetch_async_start` + `_lumen_fetch_track`),
с телом `PING` и заголовками `Ping-From`/`Ping-To`. Новой нативной
привязки не потребовалось — `_lumen_fetch_async_start` уже проводит
запрос через `JsFetchProvider::fetch_request`, то есть через тот же
`connect-src` CSP-гейт, что и `fetch()`/`sendBeacon`. Resource Timing
запись (`initiatorType: 'ping'`) пишется в poll-колбэке только после
реального ответа — до этого момента запись была бы с нулевой
длительностью и не прошла бы WPT-проверку `entry.duration > 99`.

`SVGAElement.prototype` (`crates/js/src/svg.rs`) получила
`_lumen_install_reflection(…, [['ping', 'ping', 'string']])` — обычное
DOMString-отражение, как у `HTMLAnchorElement.prototype.ping`; SVG `<a>`
намеренно не получает `HTMLHyperlinkElementUtils`.

Попутно вскрылся и исправлен соседний дефект в той же ветке: `el.href`
для SVG `<a>` — это объект `SVGAnimatedString` (SVGURIReference,
SVG 2 §5.7), а не строка, так что `String(href)` там же ломал и саму
навигацию по клику для SVG-ссылок (не только `ping`). Правка читает
target-href из атрибута (`href` с фолбэком на `xlink:href` — так же,
как это уже делает `SVGAElement.prototype.href`'s собственный геттер)
и резолвит его через `_url_resolve`.

8 новых unit-тестов в `crates/js/tests/cases/link_activation.rs`:
несколько ping-URL на HTML `<a>`, отсутствие `ping`-атрибута → ноль
запросов, IDL round-trip на SVG `<a>`, и клик по SVG `<a>` с фрагментным
`href`, проверяющий и доставку ping, и то, что фрагментная навигация не
уходит в полную перезагрузку. `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` чисто.
