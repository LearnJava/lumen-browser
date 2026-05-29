---
name: Orchestrator on Linux limitations
description: Ограничения скрипта orchestrator.py на Linux — нет трекинга дочерних процессов, нет автоматического открытия окон
type: project
originSessionId: cf185255-a95d-44bf-8cb6-1040eef6b3af
---
## Какие части не работают на Linux

### 1. Трекинг дочерних процессов (дочерний код)

На Windows скрипт отслеживает и убивает зависшие дочерние процессы claude через Win32 Toolhelp API:
```python
if os.name == "nt":
    # Полная система через CreateToolhelp32Snapshot, Process32First, Process32Next
    def _snapshot_descendants(root_pid: int) -> set:
        # ... Win32 код ...
else:
    def _snapshot_descendants(root_pid: int) -> set:
        return set()  # На Linux не реализовано — всегда пусто
```

**На Linux:** Если claude зависнет или будет убит неправильно, его дочерние процессы могут остаться висеть в памяти. Пришлось бы убивать вручную через `pkill -f claude`.

### 2. Автоматическое открытие окон (запуск нескольких разработчиков)

На Windows при запуске `python orchestrator.py P1 P2 P3 P4` автоматически открываются 4 окна cmd с заголовками "Lumen P1", "Lumen P2" и т.д.:
```python
if os.name == "nt":
    subprocess.Popen(
        f'start "{title}" cmd /k {cmd}',
        shell=True,
        cwd=PROJECT_DIR,
    )
else:
    subprocess.Popen(
        ["bash", "-c", f"{cmd}; exec bash"],  # На Linux просто в текущем bash
        cwd=PROJECT_DIR,
    )
```

**На Linux:** Скрипт запускает всех разработчиков в фоне (background processes), но **новые окна терминала не открываются**.

## Как использовать на Linux

### Один разработчик (просто)
```bash
python scripts/orchestrator.py P1
# Работает как есть, в текущем терминале
```

### Несколько разработчиков (вручную открывай окна)
1. Открой 4 отдельных терминала
2. В первом: `python scripts/orchestrator.py P1`
3. Во втором: `python scripts/orchestrator.py P2`
4. В третьем: `python scripts/orchestrator.py P3`
5. В четвёртом: `python scripts/orchestrator.py P4`

Или используй `tmux` / `screen` для создания виртуальных терминалов:
```bash
tmux new-session -d -s p1 'python scripts/orchestrator.py P1'
tmux new-session -d -s p2 'python scripts/orchestrator.py P2'
tmux new-session -d -s p3 'python scripts/orchestrator.py P3'
tmux new-session -d -s p4 'python scripts/orchestrator.py P4'

# Просмотр статуса
python scripts/orchestrator.py --status

# Подключение к сессии P1
tmux attach-session -t p1

# Остановка P1 после текущей задачи
python scripts/orchestrator.py --stop P1
```

## Что работает везде (Windows + Linux)

- ✅ Основной цикл обработки задач
- ✅ Чтение/запись STATUS файлов
- ✅ Обработка Ctrl+C и сигналов (SIGINT, SIGTERM)
- ✅ Восстановление после краша через session_id
- ✅ Мягкая остановка (--stop)
- ✅ Статус (--status)
- ✅ Rate limit и auth error обработка

## Улучшение на Linux (TODO)

Если потребуется:
1. Добавить `subprocess` трекинг через `/proc/<pid>/stat` (Linux-specific)
2. Или просто документировать, что на Linux нужно вручную открывать окна
3. Добавить поддержку `xterm -e` или `gnome-terminal` для автоматического открытия (GNOME)

На данный момент скрипт разработан **для Windows и работает там в полную мощь**.
