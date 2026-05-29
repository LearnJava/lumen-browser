---
name: Task orchestrator for parallel developers
description: Автозапуск Claude Code сессий P1–P5 параллельно. Флаги --model/--fallback-model (с alias opus/sonnet/haiku) и --new. Восстановление после краша через --resume
type: project
originSessionId: cf185255-a95d-44bf-8cb6-1040eef6b3af
---
## Назначение

`scripts/orchestrator.py` — управляет параллельными Claude Code сессиями для разработчиков P1–P5. Каждый разработчик работает в отдельной сессии, берёт задачи из `STATUS-PN.md`, выполняет их последовательно.

## Использование

```bash
# Один разработчик в текущем окне
python scripts/orchestrator.py P1

# Несколько в параллель (каждый в отдельном окне)
python scripts/orchestrator.py P1 P2 P3 P4

# С лимитом задач
python scripts/orchestrator.py P1 P2 --max-tasks 5

# Сразу на конкретной модели (alias или полный ID)
python scripts/orchestrator.py P1 --model haiku
LUMEN_MODEL=haiku python scripts/orchestrator.py P1 P2

# Резервная модель при rate limit (без интерактивного prompt)
python scripts/orchestrator.py P1 --fallback-model haiku
LUMEN_FALLBACK_MODEL=haiku python scripts/orchestrator.py P1 P2

# Принудительный старт с чистого листа (без --resume старой сессии)
python scripts/orchestrator.py P1 --new

# Статус всех разработчиков
python scripts/orchestrator.py --status

# Мягкая остановка P1 (доработает текущую задачу, потом стоп)
python scripts/orchestrator.py --stop P1

# Остановить всех
python scripts/orchestrator.py --stop-all
```

Алиасы моделей: `haiku → claude-haiku-4-5`, `sonnet → claude-sonnet-4-6`, `opus → claude-opus-4-7`. Разворачиваются при разборе CLI; в логах и в `claude --model <id>` идёт полный ID.

## Жизненный цикл задачи

1. Если задан `--new` — удалить `.session-PN.json`, не возобновлять старую сессию
2. Иначе: если есть `.session-PN.json` с `session_id` — возобновить через `claude --resume <id>`
3. Оркестратор проверяет `STATUS-PN.md`
4. Если есть "In progress" — продолжает её; иначе берёт первую из "Next"
5. Запускает Claude: `claude -p "Ты разработчик PN. Прочитай STATUS-PN.md..." [--model <id>]`
   - `--model` добавляется, если задан `--model`/`LUMEN_MODEL` или активирован fallback
   - Приоритет: fallback_model > initial_model > дефолт CLI
6. Claude читает статус, работает над задачей, вызывает `/lumen-task-finish`
7. Если exit code = 0 → удаляет `.session-PN.json`, готов к следующей
8. Если rate limit основной модели → интерактивный выбор резервной (или preset) → повтор БЕЗ паузы
9. Если rate limit резервной модели → пауза 5 мин, потом повтор
10. Если auth 403 → пауза 60 сек, повтор
11. Если прочая ошибка → пауза 30 сек, повтор

## Восстановление после краша

При запуске сессии оркестратор сохраняет состояние в `scripts/.session-PN.json`:
```json
{
  "developer": "P1",
  "task_number": 3,
  "started": "2026-05-28T14:32:01",
  "session_id": "uuid-here"
}
```

Если Claude упадёт (Ctrl+C, закрытие терминала, краш):
- При следующем запуске оркестратор находит `.session-PN.json`
- Запускает `claude --resume <session_id>` с промптом о восстановлении
- Claude видит полную историю диалога и git status
- Продолжает задачу с места остановки

**Если session_id не успел сохраниться** → файл удаляется, задача начинается заново.

## Служебные файлы

- `scripts/.stop-PN` — флаг мягкой остановки (создаётся `--stop`, удаляется оркестратором)
- `scripts/.jobstatus-PN` — текущий статус (обновляется в реальном времени)
- `scripts/.session-PN.json` — состояние прерванной сессии (для восстановления)

Все добавлены в `.gitignore`.

## Интеграция с `--dangerously-skip-permissions`

Оркестратор запускает Claude с флагом `--dangerously-skip-permissions`, поэтому Bash-команды в сессии выполняются без permission prompts. Это необходимо для полной автоматизации без блокировки на интерактивных вопросах.

## Особенности Windows

- На Windows каждый разработчик открывается в отдельном окне cmd (командная строка)
- Заголовок окна: "Lumen P1", "Lumen P2" и т.д.
- На Linux/macOS fallback: bash -c с exec bash для интерактивного сеанса
- Трекинг дочерних процессов через Win32 Toolhelp API (убивает зависшие процессы после сессии)

## Ограничения

- Один разработчик = одна сессия. Две параллельные сессии одного разработчика = конфликты.
- Цикл проверяет стоп-файл только **между задачами**, текущая задача всегда доработает.
- При rate limit резервной модели пауза ровно 5 минут (hardcoded), никакой парсинг "resets at 3:45pm".

## Выбор модели

**Стартовая модель** (`--model` / `LUMEN_MODEL`) используется с первого вызова claude и при возобновлении через `--resume`. Если не задана — CLI берёт дефолтную модель.

**Fallback модель** (`--fallback-model` / `LUMEN_FALLBACK_MODEL`) активируется ТОЛЬКО при первом rate limit. До этого момента игнорируется. После активации переопределяет стартовую до конца жизни процесса.

При rate limit основной модели и отсутствии preset — интерактивное меню:

```
1) haiku   (по умолчанию)
2) sonnet
3) opus
4) Ввести имя вручную (alias или полное claude-*)
```

Для unattended-запуска (без меню):
- CLI: `python scripts/orchestrator.py P1 --fallback-model haiku`
- env: `LUMEN_FALLBACK_MODEL=haiku`

При параллельном запуске `--model` и `--fallback-model` пробрасываются в каждое дочернее окно с уже развёрнутым полным ID. Env-переменные наследуются автоматически.

Выбранная fallback-модель запоминается до конца жизни процесса. **Не сохраняется** между перезапусками оркестратора — после рестарта восстановление через `--resume` идёт через стартовую модель (`--model` / `LUMEN_MODEL`) или дефолт CLI.

## Логирование

Каждая строка в консоли содержит метку времени и номер разработчика:
```
[14:32:01] [P1] Старт. Проект: D:\RustProjects\lumen-browser
[14:32:01] [P1] === Задача #1 ===
[14:45:18] [P1] Найдена прерванная сессия #1 (начата 2026-05-28T14:32:01)
[14:45:18] [P1]   session_id: 7fa2c8d9-4a1b...
[14:45:18] [P1] Возобновляю через --resume...
```

**Разработчик использует эти логи, чтобы отследить что делает оркестратор и каждый разработчик.**
