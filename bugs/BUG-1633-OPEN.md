# BUG-1633 — `--mcp-live-port`: `wait` и `eval` не отвечают даже на простой странице

**Статус:** OPEN
**Заведён:** 2026-10-18 (Chat, при разборе [BUG-1239](BUG-1239-OPEN.md))
**Область:** shell / автоматизация живого окна (`--mcp-live-port`, SDC-2) — не локализовано

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
