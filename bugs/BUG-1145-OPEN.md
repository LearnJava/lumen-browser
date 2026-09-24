# BUG-1145 — MCP `eval`: таймаут движкового потока отдаётся как «JS context not available»

**Статус:** OPEN
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
