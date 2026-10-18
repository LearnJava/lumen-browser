# BUG-1633 — `--mcp-live-port`: `wait` и `eval` не отвечают даже на простой странице

**Статус:** FIXED 2026-10-18 (P6)
**Заведён:** 2026-10-18 (Chat, при разборе [BUG-1239](BUG-1239-OPEN.md))
**Область:** shell / поток браузера (`browser_thread.rs`, ADR-032) — `about_to_wait` не вызывался

## Симптом

`lumen.exe --maximized --mcp-live-port <N> "data:text/html,<p>hi</p>"`, клиент — `Client` из
`scripts/scroll_perf.py`:

- подключение и `initialize` с токеном проходят;
- `tools/call wait {condition: document_ready, timeout_ms: 10000}` → через 12 с
  `Wait error: automation command timed out`;
- `tools/call eval {code: "1+1"}` → ответа нет 20 с (сокет закрыт по таймауту клиента);
- на `https://www.youtube.com/` — то же: `eval` с `timeout_ms: 5000` не отвечает 30 с, повторный
  `initialize` на новом соединении тоже не отвечает.

Одинаково на `target/release/lumen.exe` (2026-10-18 13:40) и `target/dev-release/lumen.exe` (11:46).
В момент замера было открыто ещё одно окно Lumen (YouTube, без `--mcp-live-port`) — проверить и без него.
Страница в окне при этом отрисована (`[bench] first non-empty frame` в логе), то есть команды
автоматизации не доходят до engine-потока или ответ не возвращается.

## Почему важно

Без живого `eval` не диагностируются сайты (BUG-1239 — YouTube), не работают `scripts/scroll_perf.py`,
`scripts/offscreen_level_probe.py`, `graphic_tests/run.py --live`, `scripts/perf_audit.py`.

## Что сделать

1. Воспроизвести на свежей сборке `main` без других окон Lumen. Если уже не воспроизводится —
   найти коммит-исправление и закрыть со ссылкой.
2. Иначе — проследить команду от `automation_server`/MCP-live до engine-потока и обратно
   (очередь команд, пробуждение event loop, `about_to_wait`), найти, где она застревает.
3. Регрессионный тест: живое окно + `eval "1+1"` отвечает быстрее 5 с.

**Готово, когда:** `python .tmp/mcp_sanity.py release` (или аналог в `scripts/`) печатает
`wait ok` и `eval ok`; `scripts/scroll_perf.py` проходит.

## Причина и исправление

Воспроизводится не на простой странице, а там, где страница гонит перерисовки без перерыва
(youtube: SMIL-анимация, ~400 кадров/с `delta Identical`). На простой странице на свежих
сборках `wait`/`eval` отвечают сразу (1,2 с / 0,05 с) — первоначальный замер, видимо, попал на
окно youtube рядом.

Цикл потока браузера (`run_browser_loop`) после первого сообщения разбирал всё, что лежит в
канале, `while let Ok(m) = rx.try_recv()`. Каждый обработчик `RedrawRequested` просит следующий
кадр, и главный поток шлёт следующее сообщение раньше, чем канал опустеет, — внутренний цикл не
заканчивался, `on_about_to_wait` не вызывался вовсе. А в нём — команды автоматизации
(`automation_rx`), сроки `wait`/`eval` и таймеры. Отсюда `automation command timed out` у `wait`
(водящий срок драйвера 12 с) и молчащий `eval`. Признак: с `LUMEN_NO_BROWSER_THREAD=1` те же
вызовы отвечают нормально (winit зовёт `about_to_wait` после каждой пачки событий).

Исправление:
- `browser_thread.rs`: `drain_batch` — не больше `MAX_BATCH` (64) сообщений подряд, дальше
  `on_about_to_wait`; остаток канала разбирается следующим оборотом (как в winit). Два юнит-теста.
- `app/about_to_wait.rs`: срок `PendingWait` теперь вкладывается в `ControlFlow` (как у
  `pending_evals`, BUG-1145) — на тихой странице `wait` отвечает «wait timeout» вовремя.
- `scripts/scroll_perf.py`: `target` как строка-селектор (формат инструмента `scroll` изменился),
  разбор лога без строки `[frame] paint`.

Проверено на `dev-release` с `--proxy http://127.0.0.1:12334` на youtube: `wait` 10,9 с с
ответом `wait timeout` (раньше 12 с `automation command timed out`), `eval` отвечает за 0,01 с;
простая страница — `wait ok` / `eval ok`; `scripts/scroll_perf.py` проходит.

Остаток — сама перерисовка без остановки: [BUG-1637](BUG-1637-OPEN.md).
