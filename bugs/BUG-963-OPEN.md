# BUG-963: `ping` hyperlink-auditing attribute never sends a request

**Статус:** OPEN
**Компонент:** js (нет ни одного call-site — механизм отсутствует целиком)

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
