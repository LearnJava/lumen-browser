---
name: Orchestrator on Linux support
description: orchestrator.py полностью работает на Linux — трекинг дочерних процессов через /proc и открытие окон терминала реализованы (ветка p1-orchestrator-linux)
type: project
originSessionId: cf185255-a95d-44bf-8cb6-1040eef6b3af
---
## Статус: работает на Linux в полную силу (с 2026-07-13)

Ранее две части были Windows-only заглушками; обе реализованы в ветке
`p1-orchestrator-linux` (merge в main 2026-07-13).

### 1. Трекинг и убийство дочерних процессов — РЕАЛИЗОВАНО

`_snapshot_descendants` / `_kill_pids` (ветка `else` в `scripts/orchestrator.py`):
- Linux: карта parent→children из `/proc/<pid>/stat` (ppid — поле после
  последней `)`; comm может содержать пробелы и скобки), обход дерева вширь.
- macOS/BSD (нет `/proc`): та же карта из `ps -eo pid=,ppid=`.
- `_kill_pids` шлёт `os.kill(pid, SIGKILL)`.

Зомби `claude`/`cargo`/`lumen` после сессии добиваются как на Windows.
`pkill -f claude` вручную больше не нужен.

### 2. Открытие окон терминала — РЕАЛИЗОВАНО

`_spawn_dev_window(dev, cmd)` + `_linux_terminal_argv(title, inner)`:
- Есть граф. сессия (`DISPLAY` или `WAYLAND_DISPLAY`): первый найденный из
  konsole → gnome-terminal → alacritty → kitty → xfce4-terminal → foot →
  xterm → x-terminal-emulator. Окно держится открытым (`exec bash`).
- Нет граф. сессии, но есть `tmux`: detached-сессия `lumen-<dev>`
  (подключение `tmux attach -t lumen-P1`).
- Иначе: фоновый процесс (вывод перемешивается — как было раньше).

Все дочерние окна/сессии запускаются с `start_new_session=True` — живут
независимо от процесса-родителя.

## Что было кроссплатформенным и раньше

Основной цикл, STATUS I/O, SIGINT/SIGTERM, восстановление по session_id,
`--stop`/`--status`, rate-limit/auth/network обработка, режимы `--coders`.

## Проверено (CachyOS, Wayland, konsole+alacritty)

py_compile OK; `_snapshot_descendants` находит форкнутые sleep-и и
`_kill_pids` их добивает; `_linux_terminal_argv` выбирает konsole и
возвращает None без DISPLAY/WAYLAND; реальное окно konsole открывается.
