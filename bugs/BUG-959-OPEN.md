# BUG-959 — `requestAnimationFrame`/`cancelAnimationFrame` missing entirely on `DedicatedWorkerGlobalScope`

**Статус:** OPEN
**Тип:** дефект реализованного кода — воркер получает собственный набор шимов (`WORKER_TIMERS_SHIM`, `WORKER_NET_SHIM`, `WORKER_OPTIONS_SHIM`, `WORKER_SHIM`, …), и ни один не определяет `requestAnimationFrame`/`cancelAnimationFrame`, хотя страничный `WEB_API_SHIM_MID` их реализует (`crates/js/src/shim/web_api_shim_mid_b.js:786`).
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 37, живая проба через `--mcp-live-port` + собственный http-сервер с корректно подставленным `testharnessreport.js`)
**Область:** js (`crates/js/src/worker.rs`)
**Владелец:** P3.

## Симптом

Внутри dedicated worker вызов `requestAnimationFrame(fn)` бросает
`ReferenceError: requestAnimationFrame is not defined`. Исключение внутри
воркерского обработчика (`self.onmessage`) не долетает никуда — ни один
`[JS error]`/`script error` не появляется в логе процесса, `worker.onerror`
на стороне вызывающего документа не срабатывает (обработчик даже не
установлен тестом) — обработчик просто молча завершается, `postMessage`
обратно на главный поток никогда не происходит.

## Прямое измерение

`grep -n "requestAnimationFrame" crates/js/src/worker.rs` — 0 совпадений
(вся страничная реализация — `web_api_shim_mid_b.js` — воркеру недоступна,
`install_worker_scope_globals_v8`/`WORKER_TIMERS_SHIM`/`WORKER_SHIM` его не
переопределяют).

Живая проба (dev-release, `main` = `657ad9dfa`, `--mcp-live-port`,
собственный http-сервер на `127.0.0.1:8899` с правильно подставленным
`testharnessreport.js` — см. «Метод» ниже):
`/workers/worker-request-animation-frame.html` — `window.__lumen_wpt_results`
остаётся `undefined` через 8 с после навигации (сравни с
`css/... ignored-properties-001.html` того же прогона, где результат
появляется мгновенно) — `promise_test` действительно висит, а не просто
не проверен пробой. В stderr процесса за всё время — ни строки об ошибке.

## Причина

`support/worker-request-animation-frame.js`:

```js
self.onmessage = function(event) {
  requestAnimationFrame(time => {
    postMessage(time);
    self.close();
  });
}
```

`requestAnimationFrame` не определён в `DedicatedWorkerGlobalScope` →
`ReferenceError` бросается синхронно внутри `self.onmessage`, до строки
`postMessage`. Главный документ ждёт `event` от `waitForMessage(worker)`,
которое никогда не резолвится — `promise_test` висит до истечения
таймаута теста.

## Метод (для следующих проб этой же формы)

Прямая проба через `--mcp-live-port` на живой странице, обслуживаемой
голым `python -m http.server`, ложно воспроизводит другой, посторонний
баг: `tests/wpt/resources/testharnessreport.js` содержит формат-токены
`%(output)d`/`%(timeout_multiplier)s`/… (см. предупреждение в самом
файле), которые в реальном прогоне (`wptrunner`/`wptserve`) подставляются
`StaticHandler`, а без него остаются буквальными `%`-последовательностями
→ `SyntaxError: Unexpected token '%'` при загрузке скрипта, из-за чего
любой тест выглядит зависшим независимо от своего кода. Черновой
`.tmp/serve_wpt_like.py` (не закоммичен) отдаёт тот же файл, что и
`environment.py::get_routes` — `executors/message-queue.js`, склеенный с
`testharnessreport.js % {output:0, timeout_multiplier:"1",
explicit_timeout:"false", debug:"false"}` — прежде чем можно доверять
результату живой пробы такого теста.

## Кого это держит

Классифицирует 1 из 42 unclassified id среза 36
(`workers/worker-request-animation-frame.html`). Живым прогоном на
mathml.raw.jsonl (WPT-RUN-5, 2026-08-21) также опровергнуты как гипотезы
ещё 2 unclassified id того же снапшота — `mathml/relations/css-styling/
ignored-properties-001.html` и `html/canvas/element/manual/context-attributes/
canvas-with-padding.html` — оба воспроизведены той же живой пробой (метод
выше) и оба завершились штатно (harness OK / PASS) без единого признака
зависания; TIMEOUT в снимке не находит причины в коде теста, остаются
unclassified без нового маркера (сравни срез 34 `video_crash_empty_src.html`
— тот же паттерн). Третий тест того же семейства,
`mathml/presentation-markup/mrow/legacy-mrow-like-elements-001.html` и
`mathml/presentation-markup/mpadded/mpadded-003.html`, использует
идентичный `setup({explicit_done:true}); window.addEventListener('load',
runTests)` идиому — не проверен живой пробой отдельно (одна и та же
причина TIMEOUT маловероятна: `test`/`assert_true` ловят исключения сами),
задел на следующий срез.

## Направление починки

Добавить `requestAnimationFrame`/`cancelAnimationFrame` в
`WORKER_TIMERS_SHIM` (или отдельный `WORKER_RAF_SHIM`) — тикать через тот
же таймерный насос, что и `setTimeout`, раз в кадр отрисовки страницы
(воркер не имеет собственного растрового кадра, поэтому колбэк логично
привязать к моменту, когда главный документ реально красит следующий
кадр — как уже сделано для `statechange` в `web_audio.rs`, см. CLAUDE.md
«Queue a callback the shim makes on the page's behalf as a task»).
