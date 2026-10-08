# BUG-1145 — MCP `eval`: таймаут движкового потока отдаётся как «JS context not available»

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/engine_thread.rs:64` `QUERY_TIMEOUT = 5s`, `:293` `recv_timeout → None`; `crates/shell/src/app/about_to_wait.rs:928-943` — `None` → «JS context not available»)

## Симптом

При разборе 48 сайтов `eval` не отвечал на cnbc, gemini, udemy, imgur, github, soundcloud,
w3schools — только на страницах, где движковый поток долго занят. `route_query_js` возвращает `None`
и когда нет `js_ctx`, и когда `EngineThread::query` не дождался ответа за `QUERY_TIMEOUT = 5 с`
(BUG-935 ввёл этот таймаут вместо вечного `recv()`); `about_to_wait.rs` превращает оба случая в одну
строку. На простых страницах `eval` работает. Минимального репро нет: нужна страница, которая
держит движковый поток дольше 5 с.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

Минимальной страницы нет. Воспроизводится на cnbc.com (3 из 3) через `.tmp/compat/probe.py lumen cnbc --expr document.title --wait 8` в worktree аудита.

**Результат:** Lumen: `Eval error: JS context not available` через 32–68 с после `ready`, паники в stderr нет. Chrome (CDP): ответ сразу.

## Что сделать

Различать два случая в ответе: «контекста нет» и «движковый поток занят, запрос не выполнен
за N с» (с N и с тем, что именно занимает поток, если известно). Для автоматизации — дать `eval`
возможность ждать дольше (параметр таймаута запроса) вместо фиксированных 5 с. Критерий: на cnbc
ответ либо приходит, либо называет таймаут, а не отсутствие контекста.

## Ещё два сайта (2026-09-25, P6): imdb, twitch

Опрос `eval` каждые 4–10 с по ходу загрузки (видимое окно, `LUMEN_NO_ADBLOCK=1`): imdb — 3 из 3
прогонов, после коммита настоящего документа 2–3 опроса подряд `JS context not available`
(49–91 с), затем снова ответ (104 с: 4720 узлов); twitch — один такой опрос на 78 с между
ответами на 43 и 103 с. Контекст не пропадал — поток был занят, как и описано выше. Отдельно:
на странице **без** `<script>` ответ тот же, но причина другая — рантайма нет вовсе
([BUG-1178](BUG-1178-FIXED.md)). Найдено по ходу закрытия [BUG-493](BUG-493-FIXED.md).

## Исправление (2026-09-28, P6)

**Причина в двух слоях.** (1) `AutomationCommand::Eval` шёл через `route_query_js` →
`EngineThread::query`, который на таймауте `QUERY_TIMEOUT = 5 с` отдаёт `None` — тот же `None`,
что и «контекста нет». (2) Сам UI-поток ждал ответа эти 5 с, то есть `eval` на занятом потоке
ещё и замораживал окно.

**Что сделано.**
- `eval` с движковым потоком уходит обычным `Task` (по порядку за уже поставленными заданиями) и
  ждёт в `Lumen::pending_evals`, не блокируя UI-поток; приход ответа будит цикл
  (`LoadEvent::AutomationWake`), срок вкладывается в `ControlFlow::WaitUntil`
  (`crates/shell/src/app/about_to_wait.rs`, `PendingEval`).
- Три разных ответа: итог; `engine thread busy: eval not run within N s (running task from
  <file:line> for M s); retry or pass a larger timeout_ms`; `JS context not available` — только
  когда рантайма действительно нет (с пометкой `: page is still loading`, пока навигация его не
  установила). Остановленный поток — `engine thread stopped before running eval`.
- `EngineThread::busy()` — что поток исполняет и сколько (`EngineWork`: layout / readback /
  task с call site'ом постановщика из `#[track_caller]`, BUG-935 S23).
- Срок — из запроса: `AutomationCommand::Eval(js, Option<u64>)`, `BrowserSession::eval_with_timeout`,
  MCP `eval` принимает `timeout_ms` (умолчание — 5000, потолок — 600000).

**Проверка.** Юнит-тесты `tests::automation_eval` (решение `PendingEval::poll`) и
`engine_thread::tests::busy_names_running_task_and_clears_when_idle`; MCP —
`tool_eval_timeout_ms_reaches_session`. Живьём (`--maximized`, `LUMEN_NO_ADBLOCK=1`, опрос `eval`
каждые 4 с), cnbc.com: 4–68 с — `JS context not available: page is still loading` (рантайм страницы
ещё не установлен); 72 с — `engine thread busy: eval not run within 5.0 s (running task from
crates/shell/src/app/about_to_wait.rs:261:13 for 4.0 s)` (пачка `tick_timers`/`pump_*`, т.е. JS
таймеров страницы); тот же запрос с `timeout_ms: 30000` на 83 с — ответ через 17.7 с (2630 узлов).

**Что осталось за рамками (→ [BUG-935](BUG-935-OPEN.md)).** Срок `eval` считается с момента, когда
UI-поток разобрал очередь автоматизации, а до неё `about_to_wait` проходит цепочку других
блокирующих `route_query_js` по 5 с каждый. Стенд `setTimeout(() => while(Date.now()-t<40000){})`:
`eval` с умолчанием ответил результатом через 39 с — UI-поток добрался до команды только к концу
цикла. Это та же блокировка UI-потока на `query`, что ведёт BUG-935.
