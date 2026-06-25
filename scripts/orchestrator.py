#!/usr/bin/env python3
"""
Оркестратор задач Lumen.

Автоматический запуск сессий Claude Code для разработчиков P1-P5.
Каждая задача — отдельная сессия с чистым контекстом.

Использование:
    python scripts/orchestrator.py                                      # БЕЗ аргументов — пошаговый мастер
    python scripts/orchestrator.py P1                                   # один разработчик
    python scripts/orchestrator.py P1 P2                                # два в параллель
    python scripts/orchestrator.py P1 P2 P3 P4 P5                       # все пятеро
    python scripts/orchestrator.py P1 --max-tasks 3                     # лимит задач
    python scripts/orchestrator.py P1 --new                             # стартовать с нуля
    python scripts/orchestrator.py P1 --model haiku                     # сразу на Haiku (alias)
    python scripts/orchestrator.py P1 --fallback-model haiku            # резерв при лимите
    python scripts/orchestrator.py P1 --model sonnet --fallback-model haiku   # стартуем на Sonnet, резерв Haiku
    python scripts/orchestrator.py P5 --max-tasks 1                     # P5: один прогон ревизии
    python scripts/orchestrator.py --stop P1                            # мягкая остановка
    python scripts/orchestrator.py --stop-all                           # остановить всех
    python scripts/orchestrator.py --status                             # статус всех

P5 (роль здоровья кода) — особый случай
---------------------------------------
Задачи P5 рекуррентные: health-свип не уходит из секции «Next» STATUS-P5.md,
поэтому `has_tasks("P5")` всегда возвращает True. Запуск без лимита
(`orchestrator.py P5`) будет гонять ревизию по кругу бесконечно. Всегда
ограничивай: `--max-tasks 1` (один прогон) или останавливай `--stop P5`.
У фич-сессий P1–P4 «Next» со временем пустеет — там лимит не обязателен.

Восстановление после краша
---------------------------
При старте каждой задачи оркестратор пишет файл состояния:
    scripts/.session-PN.json  →  { task_number, started, session_id }

session_id захватывается из первого stream-json события и дописывается в файл сразу.
При нормальном завершении задачи файл удаляется.

Если оркестратор упал (Ctrl+C, закрытие терминала, отключение питания):
- при следующем запуске он найдёт .session-PN.json
- если в нём есть session_id — запустит claude --resume <id> с recovery-промптом
- Claude увидит полную историю диалога и состояние git, продолжит с места остановки
- если session_id нет (сессия не успела стартовать) — сбросит файл, начнёт заново

Принудительный старт с нуля: ключ `--new` удаляет .session-PN.json до старта и
отключает попытку возобновления. Полезен, если прошлая сессия зависла, контекст
больше не актуален, или надо начать новую задачу с чистым диалогом.

Файлы .session-*.json добавлены в .gitignore.

Возобновление при ошибках во время работы
------------------------------------------
Rate limit, auth error (403) и прочие ненулевые коды выхода НЕ бросают
задачу: оркестратор сохраняет session_id, ждёт (или переключается на
резервную модель) и возобновляет ТУ ЖЕ сессию через `claude --resume` —
контекст диалога не теряется. Новая сессия стартует только если старая
не успела получить session_id, либо возобновление стабильно падает
(3 ошибки подряд без rate limit / 403).

Выбор модели
------------
По умолчанию `claude` запускается без `--model` — CLI берёт настроенную модель
(обычно Sonnet/Opus). Чтобы стартовать сразу на конкретной модели, можно
использовать короткие алиасы или указать полный ID:

    haiku  → claude-haiku-4-5
    sonnet → claude-sonnet-4-6
    opus   → claude-opus-4-8
    fable  → claude-fable-5

- CLI:  `--model haiku`        (или полный `--model claude-haiku-4-5`)
- env:  `LUMEN_MODEL=haiku`

Алиасы разворачиваются при разборе командной строки — в логах и в вызовах
`claude --model <id>` идёт уже полный ID, чтобы пользователь видел, что
именно запустилось. Приоритет: CLI > env. Заданная модель используется
во всех вызовах `claude`, пока не сработает fallback при rate limit
(см. ниже) — тогда fallback её переопределяет до конца жизни процесса
оркестратора.

Fallback на резервную модель при rate limit
-------------------------------------------
По умолчанию `claude` запускается без `--model` (CLI берёт настроенную модель,
обычно Sonnet/Opus). При первом детекте rate limit оркестратор:
- НЕ ставит паузу 5 минут;
- предлагает интерактивный выбор модели для переключения (или берёт
  заранее заданную через `--fallback-model` / `LUMEN_FALLBACK_MODEL`);
- запоминает выбор для всех последующих вызовов этого разработчика
  в рамках текущего процесса оркестратора;
- печатает заметный баннер в лог, чтобы пользователь видел переключение.

Меню выбора (вызывается при отсутствии заранее заданной модели):

    1) haiku   — самая быстрая, отдельные щедрые лимиты (по умолч.)
    2) sonnet  — баланс скорости и качества
    3) opus    — мощная, обычно общие лимиты с Sonnet
    4) fable   — новейшая, максимальное качество
    5) Ввести имя модели вручную (alias или полное claude-*)

Для unattended-запуска (без интерактивного ввода) задайте:
- CLI: `--fallback-model haiku`
- env: `LUMEN_FALLBACK_MODEL=haiku`

Если и резервная модель упирается в лимит — срабатывает стандартная пауза
5 минут (`wait_for_rate_limit`).

Сбросить fallback можно только перезапуском оркестратора.

Делегирование в Laguna M.1 (флаг --laguna)
------------------------------------------
Опционально задачи можно отдавать модели Laguna M.1 (poolside), а не Claude:

    --laguna assist  — Claude остаётся водителем (STATUS/правки/cargo/git/
                       /lumen-task-finish), но написание кода делегирует Laguna
                       через .tmp/laguna.py (к промпту добавляется LAGUNA_ASSIST_NOTE).
    --laguna solo    — Claude НЕ участвует: run_laguna_solo() ведёт агент-петлю
                       поверх Laguna в собственном worktree по текстовому tool-
                       протоколу READ/LIST/WRITE/BASH/DONE; перед коммитом — ворота
                       cargo check + clippy -D warnings + test по затронутым crates.

Нужен POOLSIDE_API_KEY (env или .tmp/poolside.env) и пакет openai (ленивый импорт).
ПРИВАТНОСТЬ: всё, что уходит в Laguna, публикуется на серверах poolside; solo шлёт
исходники (READ). Полная документация всех нюансов — scripts/README.md, раздел
«Делегирование в Laguna M.1».
"""

import argparse
import atexit
import json
import os
import signal
import subprocess
import sys
import re
import threading
import time
import traceback
from datetime import datetime, timedelta
from pathlib import Path

# Корень проекта — два уровня вверх от scripts/
PROJECT_DIR = Path(__file__).resolve().parent.parent
SCRIPTS_DIR = Path(__file__).resolve().parent

# --- Завершение дочерних процессов при выходе ---

_active_process: subprocess.Popen | None = None
_process_lock = threading.Lock()


def _cleanup() -> None:
    """Завершить активный подпроцесс claude при выходе оркестратора."""
    with _process_lock:
        proc = _active_process
    if proc is not None and proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


def _sighandler(sig, frame) -> None:
    _cleanup()
    sys.exit(0)


atexit.register(_cleanup)
signal.signal(signal.SIGINT, _sighandler)
signal.signal(signal.SIGTERM, _sighandler)

if os.name == "nt":
    import ctypes

    @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_uint)
    def _win_ctrl_handler(ctrl_type: int) -> bool:
        # Срабатывает на Ctrl+C (0), Ctrl+Break (1) и закрытие окна (2)
        _cleanup()
        return False  # Передать управление стандартному обработчику

    ctypes.windll.kernel32.SetConsoleCtrlHandler(_win_ctrl_handler, True)


# --- Трекинг дочерних процессов (Windows) ---

if os.name == "nt":
    _k32 = ctypes.windll.kernel32
    _k32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
    _k32.CreateToolhelp32Snapshot.argtypes = [ctypes.c_uint32, ctypes.c_uint32]
    _k32.Process32First.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    _k32.Process32Next.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    _k32.CloseHandle.argtypes = [ctypes.c_void_p]
    _k32.OpenProcess.restype = ctypes.c_void_p
    _k32.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    _INVALID_HANDLE = ctypes.c_void_p(-1).value

    class _PROCESSENTRY32(ctypes.Structure):
        _fields_ = [
            ("dwSize",              ctypes.c_ulong),
            ("cntUsage",            ctypes.c_ulong),
            ("th32ProcessID",       ctypes.c_ulong),
            ("th32DefaultHeapID",   ctypes.c_size_t),
            ("th32ModuleID",        ctypes.c_ulong),
            ("cntThreads",          ctypes.c_ulong),
            ("th32ParentProcessID", ctypes.c_ulong),
            ("pcPriClassBase",      ctypes.c_long),
            ("dwFlags",             ctypes.c_ulong),
            ("szExeFile",           ctypes.c_char * 260),
        ]

    def _snapshot_descendants(root_pid: int) -> set:
        """Вернуть PID всех живых потомков root_pid через Win32 Toolhelp snapshot."""
        TH32CS_SNAPPROCESS = 0x00000002
        snap = _k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
        if snap is None or snap == _INVALID_HANDLE:
            return set()
        parent_to_children: dict = {}
        entry = _PROCESSENTRY32()
        entry.dwSize = ctypes.sizeof(_PROCESSENTRY32)
        try:
            ok = _k32.Process32First(snap, ctypes.byref(entry))
            while ok:
                parent_to_children.setdefault(
                    entry.th32ParentProcessID, []
                ).append(entry.th32ProcessID)
                ok = _k32.Process32Next(snap, ctypes.byref(entry))
        finally:
            _k32.CloseHandle(snap)
        result: set = set()
        queue = [root_pid]
        while queue:
            pid = queue.pop()
            for child in parent_to_children.get(pid, []):
                result.add(child)
                queue.append(child)
        return result

    def _kill_pids(pids: set) -> int:
        """Завершить процессы по PID. Возвращает количество убитых."""
        PROCESS_TERMINATE = 0x0001
        killed = 0
        for pid in pids:
            handle = _k32.OpenProcess(PROCESS_TERMINATE, False, pid)
            if handle:
                if _k32.TerminateProcess(handle, 1):
                    killed += 1
                _k32.CloseHandle(handle)
        return killed

else:
    def _snapshot_descendants(root_pid: int) -> set:  # type: ignore[misc]
        return set()

    def _kill_pids(pids: set) -> int:  # type: ignore[misc]
        return 0


def log(developer: str, message: str):
    ts = datetime.now().strftime("%H:%M:%S")
    print(f"[{ts}] [{developer}] {message}", flush=True)


def _extract_section(content: str, heading: str) -> str:
    """Вернуть текст секции от ## heading до следующего ## или --- или конца файла."""
    m = re.search(
        r"^##\s+" + re.escape(heading) + r"\s*\n(.*?)(?=\n---|\n##(?!#)|\Z)",
        content,
        re.DOTALL | re.MULTILINE,
    )
    return m.group(1).strip() if m else ""


def has_tasks(developer: str) -> bool:
    """Проверить, есть ли задачи в STATUS-файле.

    Поддерживает два формата STATUS-PN.md:

    1. Новый (P1/P3/P4) — голые строки-указатели `<источник>:NN`, по одной
       на задачу: `ROADMAP.md:92`, `BUGS.md:133`, `CSS-SPECS.md:221`,
       либо код-якорь `crates/.../ruby.rs:76`. Без заголовков и таблиц.
       Любая такая строка = открытая задача.
    2. Старый (P5 — рекуррентная ревизия; P2 — резерв) — секции
       `## In progress` / `## Next` с таблицами, чекбоксами или
       подзаголовками `### N.`.
    """
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        log(developer, f"STATUS-файл не найден: {status_file}")
        return False

    content = status_file.read_text(encoding="utf-8")

    # Новый формат: строка-указатель `<источник>:NN` (источник — путь к файлу
    # без пробелов, NN — номер строки). Игнорируем заголовки/цитаты/прозу.
    for line in content.splitlines():
        s = line.strip()
        if not s or s.startswith(("#", ">", "-", "_", "*")):
            continue
        if re.match(r"^\S+:\d+$", s):
            return True

    # Секция "In progress": непустая и не италик-заглушка вида
    # _(none)_, _(нет)_, _(none — роль-резерв)_ и т.п.
    in_progress = _extract_section(content, "In progress")
    if in_progress and not re.fullmatch(r"_\(.*\)_", in_progress, re.DOTALL):
        return True

    # Секция "Next": содержит строки таблицы | N |, чекбоксы - [ или заголовки ### N.
    next_section = _extract_section(content, "Next")
    if (
        re.search(r"\|\s*\d+\s*\|", next_section)
        or re.search(r"- \[", next_section)
        or re.search(r"^###\s+\d+\.", next_section, re.MULTILINE)
    ):
        return True

    return False


# --- Worklog + STATUS-маркировка наработок Laguna (solo) ---
#
# В solo-режиме Laguna коммитит в своём worktree, но НЕ вливает в main. Чтобы
# Claude-разработчик потом нашёл, проверил и влил эти наработки, оркестратор:
#   1) метит строку-указатель в STATUS-PN.md как `<указатель> | <ветка> | LagunaM1`
#      (третий параметр-маркёр = «сделано Laguna, ждёт проверки и влития»);
#   2) пишет читаемый отчёт в scripts/laguna-worklog.md (worktree/ветка/commit/
#      описание задачи).
# После проверки + merge Claude удаляет строку из worklog и снимает указатель из
# STATUS-PN.md (см. LAGUNA_REVIEW_NOTE в task_prompt).

LAGUNA_WORKLOG = SCRIPTS_DIR / "laguna-worklog.md"
LAGUNA_STATUS_MARKER = "LagunaM1"
_WORKLOG_HEADER = (
    "# Laguna solo worklog\n\n"
    "Наработки Laguna M.1 в solo-режиме оркестратора, ожидающие проверки и "
    "влития в main Claude-сессией разработчика. Одна строка таблицы = одна "
    "задача. После проверки + merge строку удаляет Claude (и снимает маркёр "
    "`LagunaM1` со строки-указателя в STATUS-PN.md).\n\n"
    "| Время | Dev | Задача | Указатель | Ветка | Worktree | Commit |\n"
    "|---|---|---|---|---|---|---|\n"
)


def _pointer_is_actionable(pointer: str) -> bool:
    """Указатель ведёт на реальную открытую работу?

    Защита от протухших STATUS-PN.md: указатель на строку ROADMAP.md со
    статусом `done` означает уже выполненную задачу — Laguna там нечего
    реализовывать, она лишь впустую гоняет READ/grep до лимита итераций.
    Такие (и снятые `cancelled`/`dropped`) указатели пропускаем.

    Для не-ROADMAP источников (BUGS.md, CSS-SPECS.md, код file:line) считаем
    указатель actionable — у них своя семантика статуса, не ломаем её.
    """
    m = re.match(r"^(\S+):(\d+)$", pointer)
    if not m:
        return True
    if not m.group(1).endswith("ROADMAP.md"):
        return True
    src = PROJECT_DIR / m.group(1)
    ln = int(m.group(2))
    try:
        lines = src.read_text(encoding="utf-8").splitlines()
    except OSError:
        return True
    if not (1 <= ln <= len(lines)):
        return True
    cols = lines[ln - 1].split("|")
    # Формат строки ROADMAP: `| id | phase | <пусто> | status | ...`.
    # status — 4-й столбец между разделителями (индекс 4 после split по '|').
    if len(cols) < 5:
        return True
    status = cols[4].strip().lower()
    return status not in {"done", "cancelled", "dropped", "wontfix"}


def pick_laguna_task(developer: str) -> tuple[int, str] | None:
    """Выбрать первую НЕпомеченную actionable строку-указатель из STATUS-PN.md.

    Возвращает (номер строки 1-based, текст указателя `<источник>:NN`) или
    None, если непомеченных actionable указателей нет. Пропускаются:
    - уже помеченные строки (`<указатель> | <ветка> | LagunaM1`) — сделаны
      Laguna и ждут влития, повторно их брать нельзя;
    - указатели на строки ROADMAP.md со статусом `done`/`cancelled`/… —
      задача уже закрыта, Laguna там зациклится на разведке (см.
      `_pointer_is_actionable`).
    """
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        return None
    for idx, line in enumerate(status_file.read_text(encoding="utf-8").splitlines(), 1):
        s = line.strip()
        if re.match(r"^\S+:\d+$", s) and _pointer_is_actionable(s):
            return idx, s
    return None


def resolve_pointer_desc(pointer: str) -> str:
    """Прочитать строку `<источник>:NN` и вернуть текст задачи для лога.

    `<источник>` — путь к файлу относительно корня проекта (ROADMAP.md,
    BUGS.md, CSS-SPECS.md или код-якорь file.rs), NN — 1-based номер строки.
    Возвращает обрезанный до 200 симв. текст этой строки; при сбое — сам
    указатель.
    """
    m = re.match(r"^(\S+):(\d+)$", pointer)
    if not m:
        return pointer
    src = PROJECT_DIR / m.group(1)
    ln = int(m.group(2))
    try:
        lines = src.read_text(encoding="utf-8").splitlines()
        if 1 <= ln <= len(lines):
            return lines[ln - 1].strip()[:200] or pointer
    except OSError:
        pass
    return pointer


def mark_laguna_task(developer: str, pointer: str, branch: str) -> None:
    """Пометить строку-указатель в STATUS-PN.md как сделанную Laguna.

    Переписывает первую строку, чей текст равен `pointer`, в формат
    `<указатель> | <ветка> | LagunaM1`. Это сигнал Claude-разработчику:
    наработку надо проверить и влить в main перед своей задачей. Если строка
    не найдена (формат изменился) — STATUS не трогается.
    """
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        return
    lines = status_file.read_text(encoding="utf-8").splitlines()
    for i, line in enumerate(lines):
        if line.strip() == pointer:
            lines[i] = f"{pointer} | {branch} | {LAGUNA_STATUS_MARKER}"
            break
    status_file.write_text("\n".join(lines) + "\n", encoding="utf-8")


def append_worklog(
    developer: str, desc: str, pointer: str, branch: str, worktree: str, commit: str
) -> None:
    """Добавить строку о завершённой Laguna-задаче в scripts/laguna-worklog.md.

    Создаёт файл с заголовком при первом вызове. `|` в описании экранируется,
    чтобы не разрушить markdown-таблицу.
    """
    if not LAGUNA_WORKLOG.exists():
        LAGUNA_WORKLOG.write_text(_WORKLOG_HEADER, encoding="utf-8")
    ts = datetime.now().strftime("%Y-%m-%d %H:%M")
    safe_desc = desc.replace("|", "\\|")
    row = f"| {ts} | {developer} | {safe_desc} | {pointer} | {branch} | {worktree} | {commit} |\n"
    with LAGUNA_WORKLOG.open("a", encoding="utf-8") as f:
        f.write(row)


def stop_file_path(developer: str) -> Path:
    return SCRIPTS_DIR / f".stop-{developer}"


def jobstatus_path(developer: str) -> Path:
    return SCRIPTS_DIR / f".jobstatus-{developer}"


def session_state_path(developer: str) -> Path:
    return SCRIPTS_DIR / f".session-{developer}.json"


def save_session_state(developer: str, task_number: int, session_id: str | None = None) -> None:
    """Сохранить состояние сессии перед запуском claude."""
    state: dict = {
        "developer": developer,
        "task_number": task_number,
        "started": datetime.now().isoformat(),
    }
    if session_id:
        state["session_id"] = session_id
    session_state_path(developer).write_text(
        json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8"
    )


def update_session_id(developer: str, session_id: str) -> None:
    """Записать актуальный session_id в файл состояния.

    Перезаписывает старое значение: `claude --resume` порождает НОВУЮ
    сессию с новым id, и для повторного возобновления после следующей
    ошибки нужен именно последний id, а не исходный.
    """
    path = session_state_path(developer)
    if not path.exists():
        return
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
        if state.get("session_id") != session_id:
            state["session_id"] = session_id
            path.write_text(json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8")
    except (json.JSONDecodeError, OSError):
        pass


def load_session_state(developer: str) -> dict | None:
    """Загрузить сохранённое состояние сессии, если есть."""
    path = session_state_path(developer)
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None


def clear_session_state(developer: str) -> None:
    """Удалить файл состояния после успешного завершения задачи."""
    path = session_state_path(developer)
    if path.exists():
        path.unlink()


def set_jobstatus(developer: str, status: str, detail: str = ""):
    """Записать текущий статус разработчика в файл."""
    path = jobstatus_path(developer)
    ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    lines = [f"status: {status}", f"updated: {ts}"]
    if detail:
        lines.append(f"detail: {detail}")
    path.write_text("\n".join(lines), encoding="utf-8")


def show_status():
    """Показать статус всех разработчиков."""
    print("Статус разработчиков:")
    print("-" * 50)
    for dev in ["P1", "P2", "P3", "P4", "P5"]:
        path = jobstatus_path(dev)
        if not path.exists():
            print(f"  {dev}: не запущен")
        else:
            content = path.read_text(encoding="utf-8")
            status = ""
            updated = ""
            detail = ""
            for line in content.splitlines():
                if line.startswith("status:"):
                    status = line.split(":", 1)[1].strip()
                elif line.startswith("updated:"):
                    updated = line.split(":", 1)[1].strip()
                elif line.startswith("detail:"):
                    detail = line.split(":", 1)[1].strip()
            info = f"{status}"
            if detail:
                info += f" | {detail}"
            if updated:
                info += f" (обн. {updated})"
            print(f"  {dev}: {info}")
    print("-" * 50)


def format_tool_use(block: dict) -> str:
    """Форматировать один tool_use блок."""
    tool = block.get("name", "?")
    inp = block.get("input", {})
    if tool == "Bash":
        cmd = inp.get("command", "")
        preview = cmd[:500].replace("\n", " ")
        return f"  $ {preview}"
    elif tool == "Read":
        return f"  Читает: {inp.get('file_path', '?')}"
    elif tool == "Edit":
        return f"  Редактирует: {inp.get('file_path', '?')}"
    elif tool == "Write":
        return f"  Пишет: {inp.get('file_path', '?')}"
    elif tool == "Grep":
        return f"  Ищет: {inp.get('pattern', '?')}"
    elif tool == "Glob":
        return f"  Glob: {inp.get('pattern', '?')}"
    elif tool == "Skill":
        return f"  Skill: {inp.get('skill', '?')}"
    elif tool == "Agent":
        return f"  Agent: {inp.get('description', '?')}"
    else:
        return f"  Инструмент: {tool}"


def format_event(event: dict, last_text: list[str] | None = None) -> list[str]:
    """Превратить JSON-событие stream-json в читаемые строки.

    `last_text` — однозначный мутируемый контейнер `[str]` для дедупа: в нём
    хранится текст последнего напечатанного assistant-сообщения. Финальное
    событие `result` дословно повторяет это сообщение (оно уже выведено в
    реальном времени как `assistant`-событие), поэтому при совпадении его
    текст не печатается повторно — остаётся только маркер завершения.
    """
    lines = []
    ev_type = event.get("type", "")

    # Финальный результат
    if ev_type == "result":
        result_text = event.get("result", "")
        # Дедуп: result дублирует последнее assistant-сообщение — печатаем
        # только маркер, сам текст уже был в логе.
        if last_text is not None and result_text.strip() == last_text[0].strip():
            lines.append("  ✓ Сессия завершена")
            return lines
        if result_text:
            lines.append("  Результат:")
            for part in result_text.splitlines():
                lines.append(f"    {part}")
        return lines

    # Сообщение ассистента — содержит content[] с text и tool_use
    if ev_type == "assistant":
        msg = event.get("message", {})
        msg_texts = []
        for block in msg.get("content", []):
            btype = block.get("type", "")
            if btype == "tool_use":
                lines.append(format_tool_use(block))
            elif btype == "text":
                text = block.get("text", "")
                if text:
                    msg_texts.append(text)
                    for part in text.splitlines():
                        lines.append(f"  {part}")
        # Запомнить текст этого сообщения для последующего дедупа result
        if last_text is not None and msg_texts:
            last_text[0] = "\n".join(msg_texts)
        return lines

    return lines


RATE_LIMIT_RE = re.compile(r"resets?\s+(\d{1,2}:\d{2}(?:\s*[ap]m)?)", re.IGNORECASE)
# Фраза "hit your limit" не покрывает "hit your session limit" — используем широкий паттерн
RATE_LIMIT_TEXT_RE = re.compile(r"hit your\b.*\blimit|rate.?limit|session limit", re.IGNORECASE)

# Короткие алиасы. Принимаются в CLI (`--model`, `--fallback-model`),
# env-переменных и в интерактивном prompt. Разворачиваются в полный
# model ID, который и идёт в `claude --model <id>` — в логах видно
# именно полный ID, чтобы пользователь понимал, что запустилось.
MODEL_ALIASES: dict[str, str] = {
    "haiku":  "claude-haiku-4-5",
    "sonnet": "claude-sonnet-4-6",
    "opus":   "claude-opus-4-8",
    "fable":  "claude-fable-5",
}


def resolve_model_alias(name: str | None) -> str | None:
    """Развернуть короткий alias в полный model ID.

    `haiku` → `claude-haiku-4-5`, и т.п. Если значение не alias —
    возвращается как есть (можно указать любую custom-модель).
    None / пустая строка → None.
    """
    if not name:
        return None
    key = name.strip().lower()
    if not key:
        return None
    return MODEL_ALIASES.get(key, name.strip())


# Предопределённые варианты для меню выбора резервной модели.
# Первый элемент — значение по умолчанию (если пользователь нажал Enter).
# Формат: (alias, описание). Полный model ID берётся из MODEL_ALIASES.
PREDEFINED_FALLBACKS: list[tuple[str, str]] = [
    ("haiku",  "самая быстрая, отдельные щедрые лимиты (рекомендуется)"),
    ("sonnet", "баланс скорости и качества"),
    ("opus",   "мощная, обычно общие лимиты с Sonnet"),
    ("fable",  "новейшая, максимальное качество"),
]

# Имя env-переменной для unattended-запуска (отключает интерактивный prompt).
FALLBACK_MODEL_ENV = "LUMEN_FALLBACK_MODEL"

# Имя env-переменной для задания модели «с самого начала» (до первого rate limit).
INITIAL_MODEL_ENV = "LUMEN_MODEL"


def choose_fallback_model(developer: str) -> str:
    """Интерактивно спросить пользователя, на какую модель переключиться.

    Возвращает полный model ID (alias уже развёрнут).
    Пустой ввод → первый вариант (haiku → claude-haiku-4-5).
    При EOF/Ctrl+C → также первый вариант, чтобы не блокировать цикл.
    """
    border = "=" * 60
    default_alias = PREDEFINED_FALLBACKS[0][0]
    default_model = MODEL_ALIASES[default_alias]
    print(border, flush=True)
    print(f"[{developer}] Выберите резервную модель для переключения:", flush=True)
    for i, (alias, desc) in enumerate(PREDEFINED_FALLBACKS, 1):
        print(f"  {i}) {alias:<8} — {desc}", flush=True)
    print(f"  {len(PREDEFINED_FALLBACKS) + 1}) Ввести имя модели вручную (alias или полное claude-*)", flush=True)
    print(border, flush=True)
    print(
        f"  (для unattended-режима задайте --fallback-model или {FALLBACK_MODEL_ENV})",
        flush=True,
    )

    while True:
        try:
            raw = input(f"[{developer}] Выбор [1 = {default_alias}]: ").strip()
        except (EOFError, KeyboardInterrupt):
            print(f"\n[{developer}] Ввод прерван — использую {default_model}", flush=True)
            return default_model

        if not raw:
            return default_model

        if raw.isdigit():
            idx = int(raw)
            if 1 <= idx <= len(PREDEFINED_FALLBACKS):
                alias = PREDEFINED_FALLBACKS[idx - 1][0]
                return MODEL_ALIASES[alias]
            if idx == len(PREDEFINED_FALLBACKS) + 1:
                try:
                    custom = input(f"[{developer}] Имя модели (alias или полное claude-*): ").strip()
                except (EOFError, KeyboardInterrupt):
                    print(f"\n[{developer}] Ввод прерван — использую {default_model}", flush=True)
                    return default_model
                resolved = resolve_model_alias(custom)
                if resolved:
                    return resolved
                print("Пустое имя — повторите выбор.", flush=True)
                continue
            print("Неверный номер — повторите выбор.", flush=True)
            continue

        # Допускаем прямой ввод alias или полного имени вместо номера
        resolved = resolve_model_alias(raw)
        if resolved:
            return resolved


def resolve_fallback_model(developer: str, preset: str | None) -> str:
    """Вернуть полный ID резервной модели: из preset, env, или интерактивного выбора."""
    if preset:
        return preset  # preset уже развёрнут в main() через resolve_model_alias
    env_resolved = resolve_model_alias(os.environ.get(FALLBACK_MODEL_ENV))
    if env_resolved:
        return env_resolved
    return choose_fallback_model(developer)


def run_claude(
    developer: str,
    prompt: str,
    task_number: int = 0,
    resume_session_id: str | None = None,
    model: str | None = None,
) -> tuple[int, bool, bool, str | None]:
    """Запустить claude и показать прогресс. Возвращает (exit_code, rate_limited, auth_error, reset_time).

    reset_time — строка вида "6:50pm" из сообщения "resets 6:50pm", или None.
    При resume_session_id использует --resume <id> для продолжения прерванной сессии.
    При model передаёт `--model <id>` в CLI (используется для fallback на Haiku).
    """
    global _active_process

    cmd = [
        "claude",
        "--dangerously-skip-permissions",
        "--verbose",
        "--output-format", "stream-json",
    ]
    if model:
        cmd += ["--model", model]
    if resume_session_id:
        cmd += ["--resume", resume_session_id, "-p", prompt]
    else:
        cmd += ["-p", prompt]

    if model:
        log(developer, f"  Модель: {model}")

    process = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=PROJECT_DIR,
        text=True,
        encoding="utf-8",
        errors="replace",
    )

    with _process_lock:
        _active_process = process

    # Накапливаем PID потомков пока claude жив — после выхода они могут стать сиротами
    _seen: set = set()
    _stop_tracker = threading.Event()
    # Текст последнего assistant-сообщения — для дедупа финального result-события
    _last_text: list[str] = [""]

    def _tracker() -> None:
        while not _stop_tracker.wait(timeout=2.0):
            _seen.update(_snapshot_descendants(process.pid))

    tracker = threading.Thread(target=_tracker, daemon=True)
    tracker.start()

    try:
        rate_limited = False
        auth_error = False
        reset_time: str | None = None
        # Даже при resume id захватываем заново: claude --resume порождает
        # НОВУЮ сессию с новым id, его и нужно хранить для следующего resume.
        _session_id_saved = False
        for line in process.stdout:
            line = line.strip()
            if not line:
                continue

            # Детект rate limit в сыром выводе
            if RATE_LIMIT_TEXT_RE.search(line):
                rate_limited = True
                m = RATE_LIMIT_RE.search(line)
                if m and reset_time is None:
                    reset_time = m.group(1)
                log(developer, f"  Rate limit: {line[:500]}")
                continue

            # Детект 403 / auth error в сыром выводе
            if "403" in line and ("forbidden" in line.lower() or "authenticate" in line.lower()):
                auth_error = True
                log(developer, f"  Auth error (403): {line[:500]}")
                continue

            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                # Не-JSON строка — может быть сообщение от CLI
                if RATE_LIMIT_TEXT_RE.search(line):
                    rate_limited = True
                    log(developer, f"  Rate limit: {line[:500]}")
                elif "403" in line and ("forbidden" in line.lower() or "authenticate" in line.lower()):
                    auth_error = True
                    log(developer, f"  Auth error (403): {line[:500]}")
                continue

            # Захватить session_id из первого события, где он есть
            if not _session_id_saved and task_number > 0:
                sid = event.get("session_id", "")
                if sid:
                    update_session_id(developer, sid)
                    _session_id_saved = True

            # Детект rate limit в JSON-событии
            if event.get("type") == "rate_limit_event":
                info = event.get("rate_limit_info", {})
                if info.get("status", "").startswith("blocked"):
                    rate_limited = True
                    log(developer, "  Rate limit (blocked)")
            # Детект через текст ассистента ("You've hit your session limit …")
            if not rate_limited and event.get("type") == "assistant":
                for block in event.get("message", {}).get("content", []):
                    if block.get("type") == "text":
                        text = block.get("text", "")
                        if RATE_LIMIT_TEXT_RE.search(text):
                            rate_limited = True
                            m = RATE_LIMIT_RE.search(text)
                            if m and reset_time is None:
                                reset_time = m.group(1)
                            break

            for display_line in format_event(event, _last_text):
                log(developer, display_line)

        # Проверить stderr тоже — rate limit / auth error может быть там
        stderr_output = process.stderr.read()
        if stderr_output:
            sl = stderr_output.lower()
            if RATE_LIMIT_TEXT_RE.search(stderr_output):
                rate_limited = True
                match = RATE_LIMIT_RE.search(stderr_output)
                if match:
                    if reset_time is None:
                        reset_time = match.group(1)
                    log(developer, f"  Rate limit до {match.group(1)}")
                else:
                    log(developer, "  Rate limit обнаружен")
            elif "403" in stderr_output and ("forbidden" in sl or "authenticate" in sl):
                auth_error = True
                log(developer, "  Auth error (403) в stderr")

        process.wait()
        return process.returncode, rate_limited, auth_error, reset_time
    finally:
        _stop_tracker.set()
        tracker.join(timeout=3.0)

        with _process_lock:
            if _active_process is process:
                _active_process = None

        killed = _kill_pids(_seen)
        if killed > 0:
            log(developer, f"  Завершено {killed} дочерних процессов после сессии")


def wait_for_rate_limit(developer: str, reset_time_str: str | None = None):
    """Подождать до сброса лимита.

    Если передан reset_time_str (например "6:50pm"), ждёт до этого времени +1 мин запас.
    Иначе — фиксированные 5 минут.
    """
    wait_seconds = 5 * 60  # fallback
    reset_label = (datetime.now() + timedelta(seconds=wait_seconds)).strftime("%H:%M")

    if reset_time_str:
        try:
            # Парсим "6:50pm", "18:50", "6:50 pm" и т.п.
            t_str = reset_time_str.replace(" ", "").lower()
            fmt = "%I:%M%p" if ("am" in t_str or "pm" in t_str) else "%H:%M"
            parsed = datetime.strptime(t_str, fmt)
            now = datetime.now()
            reset_dt = now.replace(hour=parsed.hour, minute=parsed.minute, second=0, microsecond=0)
            if reset_dt <= now:
                reset_dt += timedelta(days=1)
            wait_seconds = max(60, int((reset_dt - now).total_seconds()) + 60)
            reset_label = reset_dt.strftime("%H:%M")
        except ValueError:
            pass  # не распарсилось — используем fallback 5 мин

    log(developer, f"Rate limit — пауза до {reset_label} ({wait_seconds // 60} мин {wait_seconds % 60} сек)...")
    set_jobstatus(developer, "rate limit", f"ждёт до {reset_label}")
    time.sleep(wait_seconds)
    log(developer, "Пауза завершена, продолжаю.")


def announce_fallback(developer: str, reason: str, model: str) -> None:
    """Печатает заметный баннер о переключении на резервную модель."""
    border = "=" * 60
    log(developer, "")
    log(developer, border)
    log(developer, "  RATE LIMIT основной модели")
    log(developer, f"  Причина: {reason}")
    log(developer, f"  Переключаюсь на резервную модель: {model}")
    log(developer, f"  Следующий вызов claude пойдёт через {model} БЕЗ паузы.")
    log(developer, "  Сбросить fallback можно только перезапуском оркестратора.")
    log(developer, border)
    log(developer, "")


# =====================================================================
# Делегирование в Laguna M.1 (poolside)
# =====================================================================
#
# Два режима, выбираются флагом `--laguna {assist,solo}`:
#
#   assist — Claude остаётся водителем (читает STATUS, правит файлы,
#            гоняет cargo/git, зовёт /lumen-task-finish), но САМО написание
#            кода делегирует Laguna через `python .tmp/laguna.py`. Это лишь
#            добавка к промпту — никакого нового исполнения.
#
#   solo   — оркестратор сам становится агент-харнессом: ведёт многоходовый
#            диалог с Laguna по текстовому tool-протоколу (READ/LIST/WRITE/
#            BASH/DONE), применяет правки, гоняет `cargo check`, при ошибке
#            возвращает их Laguna, на успехе коммитит. Claude не участвует.
#
# ПРИВАТНОСТЬ: всё, что уходит в Laguna (включая содержимое исходников при
# READ в solo-режиме), публикуется на серверах poolside. Режимы включаются
# только явным флагом. См. memory feedback_laguna_delegation_triggers.

LAGUNA_BASE_URL = "https://inference.poolside.ai/v1"
LAGUNA_MODEL = "poolside/laguna-m.1"
LAGUNA_ENV = PROJECT_DIR / ".tmp" / "poolside.env"
LAGUNA_MAX_ITERS = 24
# Таймаут одного запроса к Laguna, сек. Дефолт OpenAI SDK = 600с (10 мин): при
# залипшем streaming-соединении это значит 10 минут простоя до ошибки. Режем до
# 120с — застрявший поток падает быстро и цикл повторяет запрос. Override: env.
LAGUNA_TIMEOUT = float(os.environ.get("LAGUNA_TIMEOUT", "120"))
# Keep-alive пула httpx. Протухшее переиспользуемое TLS-соединение — типичная
# причина `[SSL: DECRYPTION_FAILED_OR_BAD_RECORD_MAC]` после паузы между
# итерациями. LAGUNA_KEEPALIVE=0 → новое соединение на каждый запрос (Connection:
# close): чуть медленнее на рукопожатии, но без stale-connection SSL-сбоев.
LAGUNA_KEEPALIVE = os.environ.get("LAGUNA_KEEPALIVE", "1").lower() not in ("0", "false", "no")
# Потолок подряд идущих сетевых/SSL-сбоев на одной итерации. Раньше except в
# solo-цикле делал `sleep; continue` без инкремента `it` и без пересоздания
# клиента — отравленное keepalive-соединение бралось из пула снова, и тот же
# запрос вечно падал с `[SSL: DECRYPTION_FAILED_OR_BAD_RECORD_MAC]`. Теперь
# после стольких сбоев подряд solo-сессия сохраняется и завершается.
LAGUNA_MAX_NET_FAILS = int(os.environ.get("LAGUNA_MAX_NET_FAILS", "5"))


def laguna_debug(developer: str, message: str) -> None:
    """Дописать строку в scripts/laguna-debug-PN.log — полный лог обмена с Laguna.

    Отдельно от консольного `log()`: сюда идут размеры запроса/ответа, тайминг,
    счётчик chunk'ов и полный трейсбэк сетевых сбоев — чтобы постфактум понять,
    уходит ли запрос и приходит ли ответ, и на чём именно рвётся соединение.
    """
    try:
        path = SCRIPTS_DIR / f"laguna-debug-{developer}.log"
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        with path.open("a", encoding="utf-8") as f:
            f.write(f"[{ts}] {message}\n")
    except OSError:
        pass

# Добавка к промпту Claude в режиме assist.
LAGUNA_ASSIST_NOTE = (
    " РЕЖИМ LAGUNA(assist): код НЕ пиши сам — делегируй написание Laguna M.1. "
    "Сформулируй точное ТЗ (нужные сигнатуры, путь к файлу, ограничения) и вызови "
    "`python .tmp/laguna.py \"<ТЗ>\"` (добавь --think для сложной логики). Получив код — "
    "САМ проверь его: cargo check/clippy/test -p <crate>; при ошибках отправь на доработку "
    "тем же вызовом с описанием ошибки. Затем интегрируй по правилам Lumen и закоммить. "
    "Финальная сборка, тесты и ревью — всегда твои, наружу шли только не-секретный код."
)


def _load_poolside_key() -> str | None:
    """Достать POOLSIDE_API_KEY из env или .tmp/poolside.env (как laguna.py)."""
    if os.environ.get("POOLSIDE_API_KEY"):
        return os.environ["POOLSIDE_API_KEY"]
    if LAGUNA_ENV.exists():
        for line in LAGUNA_ENV.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("export "):
                line = line[len("export "):]
            if "=" in line and not line.startswith("#"):
                k, v = line.split("=", 1)
                if k.strip() == "POOLSIDE_API_KEY":
                    return v.strip().strip('"').strip("'")
    return None


class LagunaClient:
    """Тонкий клиент к Laguna M.1 поверх OpenAI SDK (многоходовый, in-process).

    Сеть тут работает (оркестратор живёт в реальном cmd-окне, а не в песочнице
    Claude Code). enable_thinking=false по умолчанию — иначе reasoning-модель
    тратит минуты на thinking перед ответом.
    """

    def __init__(self, developer: str = "P?") -> None:
        from openai import OpenAI  # ленивый импорт: нужен только в laguna-режимах
        import httpx

        key = _load_poolside_key()
        if not key:
            raise RuntimeError(
                "POOLSIDE_API_KEY не найден (ни в env, ни в .tmp/poolside.env)"
            )
        self.developer = developer
        # Свой httpx-клиент: явный таймаут + управляемый keep-alive (см. константы).
        if LAGUNA_KEEPALIVE:
            http = httpx.Client(timeout=LAGUNA_TIMEOUT)
        else:
            http = httpx.Client(
                timeout=LAGUNA_TIMEOUT,
                limits=httpx.Limits(max_keepalive_connections=0, max_connections=10),
                headers={"Connection": "close"},
            )
        self.client = OpenAI(
            api_key=key, base_url=LAGUNA_BASE_URL, http_client=http, max_retries=0,
        )
        laguna_debug(
            developer,
            f"LagunaClient init: base={LAGUNA_BASE_URL} model={LAGUNA_MODEL} "
            f"timeout={LAGUNA_TIMEOUT}s keepalive={LAGUNA_KEEPALIVE} key=...{key[-4:]}",
        )

    def chat(self, messages: list[dict], think: bool = False, max_tokens: int = 8000) -> str:
        """Один проход диалога. Возвращает собранный content (без reasoning).

        Пишет в laguna-debug-PN.log размер запроса, тайминг, число chunk'ов и
        размер ответа — чтобы видеть, реально ли идёт обмен с Laguna. Сетевые
        исключения НЕ глушит: их с полным трейсбэком ловит вызывающий цикл.
        """
        req_chars = sum(len(str(m.get("content", ""))) for m in messages)
        laguna_debug(
            self.developer,
            f"→ chat: msgs={len(messages)} req_chars={req_chars} "
            f"think={think} max_tokens={max_tokens}",
        )
        t0 = time.monotonic()
        resp = self.client.chat.completions.create(
            model=LAGUNA_MODEL,
            messages=messages,
            stream=True,
            max_completion_tokens=max_tokens,
            temperature=0.2,
            extra_body={"chat_template_kwargs": {"enable_thinking": think}},
        )
        parts: list[str] = []
        n_chunks = 0
        for chunk in resp:
            n_chunks += 1
            if chunk.choices and chunk.choices[0].delta.content is not None:
                parts.append(chunk.choices[0].delta.content)
        out = "".join(parts)
        dt = time.monotonic() - t0
        laguna_debug(
            self.developer,
            f"← chat OK: {dt:.1f}s chunks={n_chunks} resp_chars={len(out)}"
            + ("  (ПУСТОЙ ответ!)" if not out else ""),
        )
        return out

    def close(self) -> None:
        """Закрыть нижележащий httpx-клиент (сбросить весь пул соединений).

        Нужно при сетевом/SSL-сбое: отравленное keepalive-соединение остаётся в
        пуле и переиспользуется на следующем запросе, давая тот же
        `[SSL: DECRYPTION_FAILED_OR_BAD_RECORD_MAC]`. После close() вызывающий
        код поднимает свежий LagunaClient с чистым пулом.
        """
        try:
            self.client.close()
        except Exception:  # noqa: BLE001 — закрытие best-effort, ошибки не важны
            pass


# --- Текстовый tool-протокол для solo-режима ---

LAGUNA_CMD_RE = re.compile(r"^(READ|LIST|WRITE|BASH|DONE)\b(.*)$")
# Что Laguna разрешено запускать через BASH: только чтение + сборка/тесты.
LAGUNA_BASH_ALLOW = re.compile(
    r"^\s*(cargo\s+(check|test|clippy|build|fmt)|grep|rg|ls|git\s+(status|diff|log|branch))\b"
)
# Явный чёрный список разрушительного.
LAGUNA_BASH_DENY = re.compile(
    r"(\brm\b|\bdel\b|>>?|\bpush\b|reset\s+--hard|checkout|--force|\brebase\b|clean\s+-|:\s*>)",
    re.IGNORECASE,
)


def _safe_path(rel: str, base: Path) -> Path | None:
    """Разрешить путь только внутри `base` (рабочая граница — worktree)."""
    try:
        p = (base / rel.strip()).resolve()
    except (OSError, ValueError):
        return None
    if p != base and base not in p.parents:
        return None
    return p


def parse_laguna_commands(text: str) -> list[tuple[str, str, str | None]]:
    """Разобрать ответ Laguna в список (cmd, arg, body).

    WRITE забирает следующий за ним fenced-блок ```...``` как body (полное
    новое содержимое файла). Остальные команды — однострочные, body=None.
    """
    lines = text.splitlines()
    cmds: list[tuple[str, str, str | None]] = []
    i = 0
    while i < len(lines):
        m = LAGUNA_CMD_RE.match(lines[i].strip())
        if not m:
            i += 1
            continue
        cmd, arg = m.group(1), m.group(2).strip()
        if cmd == "WRITE":
            j = i + 1
            while j < len(lines) and not lines[j].lstrip().startswith("```"):
                j += 1
            if j < len(lines):
                k = j + 1
                buf: list[str] = []
                while k < len(lines) and not lines[k].lstrip().startswith("```"):
                    buf.append(lines[k])
                    k += 1
                cmds.append((cmd, arg, "\n".join(buf)))
                i = k + 1
                continue
        cmds.append((cmd, arg, None))
        i += 1
    return cmds


def _run_shell(cmd: str, base: Path, timeout: int = 900) -> tuple[int, str]:
    """Низкоуровневый запуск команды в `base`. Возвращает (returncode, output)."""
    try:
        r = subprocess.run(
            cmd, shell=True, cwd=base, capture_output=True,
            text=True, encoding="utf-8", errors="replace", timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return 124, f"(таймаут {timeout}с)"
    return r.returncode, (r.stdout or "") + (r.stderr or "")


def _laguna_do_read(arg: str, base: Path) -> str:
    p = _safe_path(arg, base)
    if p is None or not p.is_file():
        return f"READ {arg}: файл не найден или вне worktree."
    text = p.read_text(encoding="utf-8", errors="replace")
    if len(text) > 16000:
        text = text[:16000] + "\n…(обрезано)…"
    return f"=== READ {arg} ===\n{text}"


def _laguna_do_list(arg: str, base: Path) -> str:
    pattern = arg or "*"
    try:
        hits = sorted(str(p.relative_to(base)) for p in base.glob(pattern))
    except (ValueError, OSError) as e:
        return f"LIST {arg}: ошибка ({e})"
    return f"=== LIST {arg} ({len(hits)}) ===\n" + "\n".join(hits[:200])


def _laguna_do_write(developer: str, arg: str, body: str | None, written: set[str], base: Path) -> str:
    if body is None:
        return f"WRITE {arg}: нет fenced-блока с содержимым — пропущено."
    p = _safe_path(arg, base)
    if p is None:
        return f"WRITE {arg}: путь вне worktree — отказано."
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(body, encoding="utf-8")
    written.add(str(p.relative_to(base)).replace("\\", "/"))
    log(developer, f"  Laguna записал: {arg} ({len(body)} симв.)")
    return f"WRITE {arg}: ок ({len(body)} симв.)."


def _laguna_do_bash(developer: str, arg: str, base: Path) -> str:
    if LAGUNA_BASH_DENY.search(arg) or not LAGUNA_BASH_ALLOW.match(arg):
        return f"BASH {arg}: команда запрещена (разрешены cargo/grep/rg/ls/git status|diff|log)."
    log(developer, f"  $ {arg[:200]}")
    rc, out = _run_shell(arg, base, timeout=600)
    if len(out) > 12000:
        out = out[:12000] + "\n…(обрезано)…"
    return f"=== BASH {arg} (exit {rc}) ===\n{out}"


def _crate_for_path(rel: str, base: Path) -> str | None:
    """Имя crate (`name` из ближайшего `[package]` Cargo.toml) для файла."""
    p = _safe_path(rel, base)
    if p is None:
        return None
    d = p if p.is_dir() else p.parent
    while True:
        cargo = d / "Cargo.toml"
        if cargo.is_file():
            txt = cargo.read_text(encoding="utf-8", errors="replace")
            if "[package]" in txt:
                m = re.search(r'(?m)^\s*name\s*=\s*"([^"]+)"', txt)
                if m:
                    return m.group(1)
        if d == base or base not in d.parents:
            return None
        d = d.parent


def _run_solo_gates(developer: str, base: Path, written: set[str]) -> tuple[bool, str]:
    """Жёсткие ворота перед коммитом: check + clippy -D warnings + test по
    затронутым crates (правила Lumen). Если crate не определён — общий
    `cargo check`. Возвращает (ok, склеенный вывод неудачных/всех прогонов).
    """
    crates = sorted({c for rel in written if (c := _crate_for_path(rel, base))})
    if not crates:
        rc, out = _run_shell("cargo check", base)
        return rc == 0, f"=== cargo check (exit {rc}) ===\n{out[:12000]}"

    log(developer, f"  Ворота для crates: {', '.join(crates)}")
    outputs: list[str] = []
    for c in crates:
        for cmd in (
            f"cargo check -p {c}",
            f"cargo clippy -p {c} --all-targets -- -D warnings",
            f"cargo test -p {c}",
        ):
            log(developer, f"  ворота: {cmd}")
            rc, out = _run_shell(cmd, base)
            outputs.append(f"=== {cmd} (exit {rc}) ===\n{out[:8000]}")
            if rc != 0:
                return False, "\n\n".join(outputs)
    return True, "\n\n".join(outputs)


def _create_solo_worktree(developer: str, task_number: int) -> tuple[Path | None, str | None, str]:
    """Создать изолированный worktree+ветку от main для solo-задачи.

    Возвращает (path, branch, err). При ошибке path/branch = None, err — текст.
    """
    num = developer[1:] if developer.startswith("P") else developer
    stamp = datetime.now().strftime("%H%M%S")
    branch = f"p{num}-laguna-t{task_number}-{stamp}"
    wt = PROJECT_DIR / ".claude" / "worktrees" / f"{developer.lower()}-laguna-{stamp}"
    rc, out = _run_shell(
        f'git worktree add "{wt}" -b {branch} main', PROJECT_DIR, timeout=120
    )
    if rc != 0:
        return None, None, out
    return wt, branch, ""


def _laguna_commit(developer: str, msg: str, paths: set[str], base: Path) -> None:
    """Закоммитить написанные Laguna файлы в worktree (ветка уже своя)."""
    if paths:
        _run_shell("git add " + " ".join(f'"{p}"' for p in paths), base, timeout=120)
    full = (
        msg
        + "\n\nНаписано Laguna M.1 (poolside) через solo-оркестратор; "
        "ворота: cargo check + clippy -D warnings + test пройдены.\n\n"
        "Co-Authored-By: Laguna M.1 (poolside) <noreply@poolside.ai>\n"
    )
    # Сообщение через временный файл, чтобы не воевать с экранированием.
    msg_file = base / ".laguna-commit-msg.txt"
    msg_file.write_text(full, encoding="utf-8")
    _run_shell('git commit -F ".laguna-commit-msg.txt"', base, timeout=120)
    msg_file.unlink(missing_ok=True)


def laguna_solo_system_prompt(developer: str) -> str:
    return (
        f"Ты автономный разработчик {developer} в проекте Lumen — браузерный движок на Rust. "
        "Работаешь в изолированном worktree (своя ветка). Прямого доступа к файлам у тебя НЕТ — "
        "взаимодействуй ТОЛЬКО командами, каждая с НОВОЙ строки, в начале строки:\n"
        "  READ <путь>            — прислать содержимое файла (путь относительно корня worktree)\n"
        "  LIST <glob>            — список файлов по маске (напр. crates/**/*.rs)\n"
        "  WRITE <путь>           — СЛЕДУЮЩИМ идёт один ```-блок с ПОЛНЫМ новым содержимым файла\n"
        "  BASH <команда>         — только cargo check|test|clippy|build, grep, rg, ls, git status|diff|log\n"
        "  DONE <текст коммита>   — задача готова; я прогоню ворота и закоммичу\n\n"
        "В одном ответе можно несколько команд. После WRITE я применю файл. "
        "ПЕРЕД WRITE всегда сделай READ изменяемого файла — WRITE перезаписывает файл целиком, "
        "частичный текст уничтожит остальное. Соблюдай стиль Lumen: edition 2024, без unwrap/panic "
        "в проде, /// doc-комменты на всех pub. "
        "ВОРОТА на DONE (обязаны пройти, иначе DONE отклонён): по каждому затронутому crate "
        "`cargo check -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`, "
        "`cargo test -p <crate>`. Прогоняй их сам через BASH до DONE и чини ошибки/варнинги. "
        "Не пиши прозу вне команд — она игнорируется. "
        f"Начни с: READ STATUS-{developer}.md"
    )


def laguna_session_path(developer: str) -> Path:
    """Путь к файлу персиста solo-сессии Laguna (для восстановления после краша)."""
    return SCRIPTS_DIR / f".laguna-session-{developer}.json"


def save_laguna_session(developer: str, state: dict) -> None:
    """Сохранить состояние solo-сессии: messages + worktree + ветка + written.

    Зачем: API Laguna (OpenAI-совместимый chat completions) — stateless,
    серверного `--resume` нет (см. docs/orchestrator README). Единственный
    способ возобновить прерванный диалог — переиграть сохранённый `messages`.
    Ключ POOLSIDE_API_KEY СЮДА НЕ пишется — он перечитывается из env/.tmp при
    каждом запуске (это секрет, не часть состояния сессии).
    """
    try:
        laguna_session_path(developer).write_text(
            json.dumps(state, ensure_ascii=False), encoding="utf-8"
        )
    except OSError as e:
        log(developer, f"  (не удалось сохранить laguna-сессию: {e})")


def load_laguna_session(developer: str) -> dict | None:
    """Загрузить сохранённое состояние solo-сессии, если есть."""
    path = laguna_session_path(developer)
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None


def clear_laguna_session(developer: str) -> None:
    """Удалить файл персиста solo-сессии (на успехе / max-iters / --new)."""
    path = laguna_session_path(developer)
    if path.exists():
        path.unlink()


def _laguna_initial_messages(developer: str, pointer: str, desc: str) -> list[dict]:
    """Стартовый диалог для свежей solo-сессии.

    Задачу выбирает оркестратор (`pick_laguna_task`) и передаёт явным
    указателем `pointer` (`<источник>:NN`) — Laguna не выбирает сама, чтобы
    оркестратор точно знал, какую строку STATUS пометить `LagunaM1` на успехе.
    """
    return [
        {"role": "system", "content": laguna_solo_system_prompt(developer)},
        {
            "role": "user",
            "content": (
                f"Твоя задача — указатель `{pointer}` из STATUS-{developer}.md "
                "(формат `<файл>:<номер_строки>`). Сначала READ этот источник, "
                f"найди на строке {pointer.split(':')[-1]} формулировку задачи и "
                "реализуй её end-to-end. "
                f"Краткое описание задачи: {desc}\n"
                "Когда ворота (check + clippy -D warnings + test) зелёные — "
                "DONE с осмысленным текстом коммита."
            ),
        },
    ]


def run_laguna_solo(developer: str, task_number: int, think: bool = False) -> bool:
    """Solo-режим: оркестратор ведёт агент-петлю поверх Laguna без Claude.

    Работает в изолированном worktree (своя ветка p<N>-laguna-*). Задачу
    выбирает САМ оркестратор — первый непомеченный указатель из STATUS-PN.md
    (`pick_laguna_task`). Перед коммитом — жёсткие ворота: cargo check +
    clippy -D warnings + test по затронутым crates. Возвращает True при
    успешном завершении (ворота зелёные + коммит).

    На успехе worktree/ветка ОСТАЮТСЯ для ревью, и оркестратор оставляет два
    следа для Claude-разработчика, который позже проверит и вольёт работу:
      - помечает строку-указатель в STATUS-PN.md как
        `<указатель> | <ветка> | LagunaM1` (`mark_laguna_task`);
      - дописывает отчёт (worktree/ветка/commit/описание) в
        scripts/laguna-worklog.md (`append_worklog`).
    Само влитие в main/доксинк/чистку делает Claude (см. laguna_review_note).

    Восстановление после краша. API Laguna stateless (серверного resume нет),
    поэтому состояние диалога персистится локально в `.laguna-session-PN.json`
    после каждой итерации (messages + worktree + ветка + written; ключ НЕ
    сохраняется). Если при старте найден файл и его worktree цел — диалог
    переигрывается из сохранённого messages в ТОМ ЖЕ worktree, продолжая с
    места обрыва. Файл удаляется на успехе, на исчерпании итераций и по `--new`
    (последнее — в run_task_loop). Если worktree пропал — старт заново.
    """
    try:
        client = LagunaClient(developer)
    except (RuntimeError, ImportError) as e:
        log(developer, f"Laguna недоступна: {e}")
        return False

    # --- Возобновление прерванной solo-сессии (если есть и worktree цел) ---
    work_dir: Path | None = None
    branch: str | None = None
    written: set[str] = set()
    messages: list[dict] = []
    start_iter = 1
    pointer: str | None = None  # строка-указатель STATUS, над которой работаем
    pointer_desc: str = ""

    saved = load_laguna_session(developer)
    if saved:
        wt = Path(saved.get("worktree", ""))
        if wt.is_dir() and saved.get("messages"):
            work_dir = wt
            branch = saved.get("branch")
            written = set(saved.get("written", []))
            messages = saved["messages"]
            task_number = saved.get("task_number", task_number)
            pointer = saved.get("pointer")
            pointer_desc = saved.get("pointer_desc", "")
            start_iter = int(saved.get("iter", 0)) + 1
            if start_iter > LAGUNA_MAX_ITERS:
                # Прошлая сессия упёрлась в лимит итераций — резюмировать нечего.
                log(developer, "Сохранённая solo-сессия исчерпала лимит итераций — старт заново.")
                clear_laguna_session(developer)
                work_dir = None
            else:
                log(developer, f"Возобновляю прерванную solo-сессию: задача #{task_number}, итер. {start_iter}")
                log(developer, f"  Worktree: {work_dir}  (ветка {branch})")
                log(developer, f"  Диалог восстановлен: {len(messages)} сообщений, файлов записано: {len(written)}")
        else:
            log(developer, "Найдено состояние solo-сессии, но worktree отсутствует/пуст — старт заново.")
            clear_laguna_session(developer)

    if work_dir is None:
        # Оркестратор сам выбирает задачу — первый непомеченный указатель из
        # STATUS-PN.md. Это нужно, чтобы на успехе пометить именно её LagunaM1.
        picked = pick_laguna_task(developer)
        if picked is None:
            log(developer, f"В STATUS-{developer}.md нет непомеченных задач-указателей — solo нечего делать.")
            return False
        _, pointer = picked
        pointer_desc = resolve_pointer_desc(pointer)
        log(developer, f"  Задача (указатель): {pointer} — {pointer_desc}")

        work_dir, branch, err = _create_solo_worktree(developer, task_number)
        if work_dir is None:
            log(developer, f"Не удалось создать worktree для Laguna: {err}")
            return False
        log(developer, f"  Worktree: {work_dir}  (ветка {branch})")
        messages = _laguna_initial_messages(developer, pointer, pointer_desc)
        start_iter = 1

    def _persist(cur_iter: int) -> None:
        save_laguna_session(developer, {
            "developer": developer,
            "task_number": task_number,
            "iter": cur_iter,
            "worktree": str(work_dir),
            "branch": branch,
            "pointer": pointer,
            "pointer_desc": pointer_desc,
            "written": sorted(written),
            "messages": messages,
        })

    # Зафиксировать worktree/диалог сразу — чтобы краш на первой же итерации
    # был восстановим (worktree уже записан, его подхватит следующий запуск).
    _persist(start_iter - 1)

    it = start_iter
    net_fails = 0  # подряд идущие сетевые/SSL-сбои на текущей итерации
    while it <= LAGUNA_MAX_ITERS:
        log(developer, f"Laguna solo: итерация {it}/{LAGUNA_MAX_ITERS}...")
        set_jobstatus(developer, "laguna-solo", f"задача #{task_number}, итер. {it}")
        try:
            reply = client.chat(messages, think=think)
        except Exception as e:  # сетевые/SDK сбои — пересоздать клиент и повторить (без +it)
            net_fails += 1
            # Полная диагностика в laguna-debug-PN.log: тип, цепочка причин
            # (там обычно настоящий SSL-объект) и трейсбэк. В консоль — кратко.
            cause = e.__cause__ or e.__context__
            chain: list[str] = []
            depth = 0
            while cause and depth < 6:
                chain.append(f"{type(cause).__name__}: {cause}")
                cause = cause.__cause__ or cause.__context__
                depth += 1
            laguna_debug(
                developer,
                f"✗ chat FAIL (итер. {it}, сбой {net_fails}/{LAGUNA_MAX_NET_FAILS}): "
                f"{type(e).__name__}: {e}\n"
                f"  причины: {' <- '.join(chain) or '—'}\n"
                f"{traceback.format_exc()}",
            )
            # Сбросить пул соединений и поднять свежий клиент: отравленное
            # keepalive-соединение (источник SSL bad-MAC) иначе берётся снова.
            client.close()
            try:
                client = LagunaClient(developer)
            except (RuntimeError, ImportError) as re_err:
                laguna_debug(developer, f"✗ не удалось пересоздать LagunaClient: {re_err}")
            if net_fails >= LAGUNA_MAX_NET_FAILS:
                # Подряд слишком много сбоев — сеть/эндпоинт недоступны. Не
                # крутиться вечно: сохранить диалог (резюмируемо) и выйти.
                _persist(it - 1)
                log(developer,
                    f"  Laguna API: {net_fails} сетевых сбоев подряд — останов solo "
                    f"(сессия сохранена, резюмируется позже). "
                    f"Детали → scripts/laguna-debug-{developer}.log")
                return False
            log(developer,
                f"  Ошибка Laguna API: {type(e).__name__}: {e}. "
                f"Сбой {net_fails}/{LAGUNA_MAX_NET_FAILS}, пересоздал клиент, пауза 20с. "
                f"(детали → scripts/laguna-debug-{developer}.log)")
            time.sleep(20)
            continue

        net_fails = 0  # успешный обмен — сбросить счётчик сбоев
        messages.append({"role": "assistant", "content": reply})
        cmds = parse_laguna_commands(reply)
        if not cmds:
            messages.append({
                "role": "user",
                "content": "Не нашёл ни одной команды (READ/LIST/WRITE/BASH/DONE). Повтори в формате протокола.",
            })
            _persist(it)
            it += 1
            continue

        feedback: list[str] = []
        commit_msg: str | None = None
        for cmd, arg, body in cmds:
            if cmd == "READ":
                feedback.append(_laguna_do_read(arg, work_dir))
            elif cmd == "LIST":
                feedback.append(_laguna_do_list(arg, work_dir))
            elif cmd == "WRITE":
                feedback.append(_laguna_do_write(developer, arg, body, written, work_dir))
            elif cmd == "BASH":
                feedback.append(_laguna_do_bash(developer, arg, work_dir))
            elif cmd == "DONE":
                commit_msg = arg or f"{developer}: задача #{task_number} (Laguna)"

        if commit_msg is not None:
            log(developer, "  Laguna заявил DONE — прогоняю ворота (check + clippy -D + test)...")
            ok, gate_out = _run_solo_gates(developer, work_dir, written)
            if ok:
                _laguna_commit(developer, commit_msg, written, work_dir)
                # Хеш только что созданного коммита — для отчёта в worklog.
                rc_h, head = _run_shell("git rev-parse --short HEAD", work_dir, timeout=30)
                commit_hash = head.strip().splitlines()[-1] if rc_h == 0 and head.strip() else "?"
                # Пометить строку STATUS как сделанную Laguna + записать отчёт.
                if pointer:
                    mark_laguna_task(developer, pointer, branch or "?")
                    append_worklog(
                        developer, pointer_desc, pointer, branch or "?",
                        str(work_dir), commit_hash,
                    )
                    log(developer, f"  STATUS-{developer}.md: указатель помечен `{LAGUNA_STATUS_MARKER}`.")
                    log(developer, f"  Отчёт дописан в {LAGUNA_WORKLOG}")
                clear_laguna_session(developer)
                log(developer, "Laguna solo: ворота пройдены, закоммичено.")
                log(developer, f"  Готово к ревью/влитию Claude: ветка {branch} @ {work_dir} (commit {commit_hash})")
                return True
            feedback.append(
                "DONE ОТКЛОНЁН — ворота не прошли:\n" + gate_out[:14000]
                + "\nИсправь ошибки/варнинги и снова DONE."
            )

        joined = "\n\n".join(feedback)
        if len(joined) > 24000:
            joined = joined[:24000] + "\n…(обрезано)…"
        messages.append({"role": "user", "content": joined})
        _persist(it)
        it += 1

    # Исчерпан лимит итераций — терминальная неудача (не краш). Снимаем персист,
    # чтобы повторный запуск НЕ возобновлял заведомо застрявший диалог, а начал
    # новую попытку. Worktree оставляем для ручного ревью.
    clear_laguna_session(developer)
    log(developer, f"Laguna solo: исчерпан лимит итераций ({LAGUNA_MAX_ITERS}). Останов.")
    log(developer, f"  Worktree с незавершённой работой оставлен: {work_dir}")
    log(developer, f"  Убрать: git worktree remove {work_dir} --force && git branch -D {branch}")
    return False


def laguna_review_note(developer: str) -> str:
    """Пред-шаг для Claude-разработчика: проверить и влить наработки Laguna.

    В solo-режиме оркестратор метит сделанные Laguna строки STATUS маркёром
    `LagunaM1` и пишет отчёт в scripts/laguna-worklog.md, но НЕ вливает их в
    main. Перед своей задачей разработчик обязан разобраться с этими
    наработками: проверить ворота и влить (или отклонить), затем почистить
    worklog и STATUS.
    """
    return (
        f" ПЕРЕД своей задачей проверь наработки Laguna. Открой STATUS-{developer}.md и "
        "найди строки с третьим параметром-маркёром `LagunaM1` (формат "
        "`<указатель> | <ветка> | LagunaM1`). Для КАЖДОЙ такой строки: "
        "(1) посмотри scripts/laguna-worklog.md — там worktree, ветка, commit и описание; "
        "(2) прогони ворота на её ветке (cargo check + clippy -D warnings + test по затронутым crate); "
        "(3) при ЗЕЛЁНЫХ воротах влей ветку в main (git merge --no-ff), удали ветку и worktree; "
        "(4) после влития удали строку этой задачи из scripts/laguna-worklog.md и удали её "
        "строку-указатель из STATUS-{0}.md; (5) git push origin main. "
        "Если ворота КРАСНЫЕ — НЕ вливай: оставь строку и worklog как есть, сообщи об этом. "
        "Только разобравшись со всеми наработками Laguna, приступай к своей задаче. "
    ).format(developer)


def task_prompt(developer: str, laguna_mode: str | None = None) -> str:
    """Стандартный промпт для старта задачи с чистым диалогом."""
    note = LAGUNA_ASSIST_NOTE if laguna_mode == "assist" else ""
    # Claude-режимы (None/assist) сначала проверяют и вливают наработки Laguna.
    # В solo Claude не участвует — пред-шаг не нужен.
    review = laguna_review_note(developer) if laguna_mode != "solo" else ""
    if developer == "P3":
        return (
            "Ты разработчик P3 (только баг-фиксы)." + review +
            " Прочитай STATUS-P3.md. "
            "Если есть 'In progress' — продолжи ЭТОТ ОДИН баг. "
            "Если нет — возьми ПЕРВЫЙ баг из 'Next' (только один, не больше). "
            "Когда баг исправлен — вызови /lumen-task-finish. "
            "ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
            "Не бери следующий баг. Один баг = одна сессия." + note
        )
    return (
        f"Ты разработчик {developer}." + review +
        f" Прочитай STATUS-{developer}.md. "
        f"Если есть 'In progress' — продолжи эту задачу. "
        f"Если нет — возьми первую задачу из 'Next'. "
        f"Когда задача завершена — вызови /lumen-task-finish." + note
    )


def resume_after_error_prompt(developer: str, laguna_mode: str | None = None) -> str:
    """Промпт для возобновления сессии, прерванной ошибкой (rate limit / 403 / сбой CLI)."""
    note = LAGUNA_ASSIST_NOTE if laguna_mode == "assist" else ""
    base = (
        "Сессия была прервана ошибкой (rate limit / auth error / сбой CLI). "
        "Выполни git status, сверься с историей диалога выше и продолжи текущую "
        "задачу с места остановки. Когда задача завершена — вызови /lumen-task-finish."
    )
    if developer == "P3":
        return base + (
            " ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
            "Не бери следующий баг. Один баг = одна сессия."
        ) + note
    return base + note


def run_task_loop(
    developer: str,
    max_tasks: int = 0,
    fallback_preset: str | None = None,
    force_new: bool = False,
    initial_model: str | None = None,
    laguna_mode: str | None = None,
):
    """Цикл задач для одного разработчика.

    laguna_mode — режим делегирования в Laguna M.1:
      None     — обычный Claude-цикл;
      "assist" — Claude-водитель, но код пишет через .tmp/laguna.py (добавка к промпту);
      "solo"   — Claude не используется: задачу гонит run_laguna_solo() поверх Laguna.

    Любая ошибка внутри задачи (rate limit, 403, ненулевой код выхода)
    не бросает задачу: сессия возобновляется через `claude --resume`
    после паузы или переключения на резервную модель (см. attempt_task).

    fallback_preset — если задан, при первом rate limit переключение
    произойдёт молча на указанную модель (без интерактивного prompt).
    Если None — пользователю будет показано меню выбора.

    force_new — если True, удалить сохранённое состояние сессии
    (`.session-PN.json`) и стартовать с чистого листа, не пытаясь
    возобновить прерванную сессию через `claude --resume`.

    initial_model — модель, на которой стартовать с самого первого вызова
    (передаётся в `claude --model <id>`). Если None — CLI использует свою
    дефолтную модель. При rate limit активный модельный fallback (через
    `fallback_preset` или интерактивный выбор) переопределяет initial_model.
    """
    stop_file = stop_file_path(developer)
    task_count = 0
    fallback_model: str | None = None  # выставляется при первом rate limit

    log(developer, f"Старт. Проект: {PROJECT_DIR}")
    if initial_model:
        log(developer, f"Стартовая модель: {initial_model}")
    if laguna_mode:
        log(developer, f"Режим Laguna: {laguna_mode}")
        if laguna_mode == "solo":
            log(developer, "  ВНИМАНИЕ: solo шлёт исходники в Laguna (poolside) — всё публикуется на их серверах.")

    def attempt_task(task_number: int, prompt: str, resume_id: str | None) -> bool:
        """Выполнить задачу #task_number с повторами до успеха.

        Rate limit, auth error (403) и прочие ненулевые коды выхода не
        бросают задачу: оркестратор ждёт (или переключается на резервную
        модель) и возобновляет ТУ ЖЕ сессию через `claude --resume`,
        сохраняя контекст диалога. Возвращает True при успешном завершении
        задачи; False — при фатальной ошибке запуска claude (бинарь не
        найден и т.п.); состояние сессии при этом сохраняется, чтобы
        следующий запуск оркестратора возобновил её.
        """
        nonlocal fallback_model
        generic_failures = 0  # подряд идущие ошибки без rate limit / 403

        while True:
            try:
                exit_code, rate_limited, auth_error, reset_time = run_claude(
                    developer, prompt, task_number,
                    resume_session_id=resume_id,
                    model=fallback_model or initial_model,
                )
            except FileNotFoundError:
                log(developer, "claude не найден в PATH.")
                return False
            except Exception as e:
                log(developer, f"Ошибка запуска: {e}")
                return False

            if exit_code == 0:
                clear_session_state(developer)
                return True

            # Сессия прервана ошибкой. Берём последний session_id из файла
            # состояния, чтобы возобновить именно её, а не начинать заново.
            state = load_session_state(developer)
            saved_id = state.get("session_id") if state else None
            if saved_id:
                resume_id = saved_id
                prompt = resume_after_error_prompt(developer, laguna_mode)

            if rate_limited:
                generic_failures = 0
                if fallback_model is None:
                    # Первый rate limit — выбираем резервную модель и
                    # возобновляем сессию сразу, без паузы.
                    fallback_model = resolve_fallback_model(developer, fallback_preset)
                    announce_fallback(developer, f"задача #{task_number}", fallback_model)
                else:
                    log(developer, f"Резервная модель {fallback_model} тоже исчерпана.")
                    wait_for_rate_limit(developer, reset_time)
            elif auth_error:
                generic_failures = 0
                log(developer, "Auth error (403). Пауза 60 сек перед возобновлением...")
                set_jobstatus(developer, "auth error", f"задача #{task_number}")
                time.sleep(60)
            else:
                generic_failures += 1
                log(developer, f"Claude завершился с кодом {exit_code}.")
                if generic_failures >= 3 and resume_id:
                    # Возобновление стабильно падает — сессия, видимо,
                    # повреждена. Сбрасываем её и начинаем задачу заново.
                    log(developer, "3 ошибки подряд — сбрасываю сессию, начинаю задачу заново.")
                    clear_session_state(developer)
                    save_session_state(developer, task_number)
                    resume_id = None
                    prompt = task_prompt(developer, laguna_mode)
                    generic_failures = 0
                log(developer, "Пауза 30 секунд перед повтором...")
                time.sleep(30)

            if resume_id:
                log(developer, f"Возобновляю сессию через --resume {resume_id[:16]}...")
            set_jobstatus(developer, "работает", f"задача #{task_number} (повтор)")

    # --- Принудительный старт с чистого листа ---
    if force_new:
        if laguna_mode == "solo":
            if laguna_session_path(developer).exists():
                log(developer, "Флаг --new: удаляю сохранённую solo-сессию Laguna, начинаю заново.")
                clear_laguna_session(developer)
            else:
                log(developer, "Флаг --new: сохранённой solo-сессии нет, стартую с нуля.")
        elif session_state_path(developer).exists():
            log(developer, "Флаг --new: удаляю сохранённое состояние сессии, не возобновляю.")
            clear_session_state(developer)
        else:
            log(developer, "Флаг --new: сохранённого состояния нет, стартую с нуля.")

    # --- Восстановление после краша (только для Claude-режимов; solo резюмируется
    #     внутри run_laguna_solo через .laguna-session-PN.json, не здесь) ---
    existing = load_session_state(developer) if laguna_mode != "solo" else None
    if existing:
        task_number = existing.get("task_number", 1)
        session_id = existing.get("session_id")
        started = existing.get("started", "?")
        if session_id:
            log(developer, f"Найдена прерванная сессия #{task_number} (начата {started})")
            log(developer, f"  session_id: {session_id[:16]}...")
            log(developer, "Возобновляю через --resume...")

            task_count = task_number
            set_jobstatus(developer, "возобновление", f"задача #{task_count}")

            one_bug_note = (
                " ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
                "Не бери следующий баг. Один баг = одна сессия."
                if developer == "P3" else ""
            )
            resume_prompt = (
                f"Сессия разработчика {developer} была прервана (crash/закрытие терминала/отключение питания). "
                f"Выполни: git status, затем прочитай STATUS-{developer}.md. "
                f"На основе истории диалога выше и текущего состояния git определи, "
                f"что уже сделано, и продолжи задачу с места остановки. "
                f"Когда задача завершена — вызови /lumen-task-finish.{one_bug_note}"
            )
            if not attempt_task(task_count, resume_prompt, session_id):
                set_jobstatus(developer, "остановлен", "ошибка запуска claude")
                return
            log(developer, f"Задача #{task_count} (возобновлённая) завершена.")
        else:
            log(developer, f"Найдено состояние задачи #{task_number} без session_id (сессия не стартовала). Сбрасываю.")
            clear_session_state(developer)

    log(developer, f"Стоп-файл: {stop_file}")
    log(developer, "")
    set_jobstatus(developer, "запущен")

    while True:
        # Проверка стоп-файла
        if stop_file.exists():
            log(developer, "Найден стоп-файл. Останавливаюсь.")
            stop_file.unlink()
            break

        # Проверка лимита задач
        if max_tasks > 0 and task_count >= max_tasks:
            log(developer, f"Достигнут лимит задач ({max_tasks}). Останавливаюсь.")
            break

        # Проверка наличия задач
        if not has_tasks(developer):
            log(developer, "Нет задач. Останавливаюсь.")
            break

        task_count += 1
        log(developer, f"=== Задача #{task_count} ===")
        set_jobstatus(developer, "работает", f"задача #{task_count}")

        if laguna_mode == "solo":
            # Solo: оркестратор сам агент поверх Laguna, без claude и без
            # .session-файла (нечего возобновлять через --resume).
            log(developer, "Запуск Laguna (solo)...")
            if not run_laguna_solo(developer, task_count):
                task_count -= 1  # задача не завершилась — останов цикла
                break
            log(developer, f"Задача #{task_count} завершена (Laguna solo).")
            continue

        # Записать состояние ДО запуска — чтобы не потерять при краше
        save_session_state(developer, task_count)

        log(developer, "Запуск claude...")
        if not attempt_task(task_count, task_prompt(developer, laguna_mode), None):
            task_count -= 1  # запуск claude не состоялся
            break
        log(developer, f"Задача #{task_count} завершена.")

    set_jobstatus(developer, "остановлен", f"выполнено задач: {task_count}")
    log(developer, f"Цикл завершён. Выполнено задач: {task_count}.")


def create_stop_file(developers: list[str]):
    """Создать стоп-файлы."""
    for dev in developers:
        sf = stop_file_path(dev)
        sf.touch()
        print(f"{dev} будет остановлен после текущей задачи. ({sf})")


def dispatch_run(
    developers: list[str],
    max_tasks: int,
    fallback_preset: str | None,
    force_new: bool,
    initial_model: str | None,
    laguna_mode: str | None,
) -> None:
    """Запустить разработчиков: одного — в текущем окне, нескольких — по окнам.

    Модели уже развёрнуты в полный ID. Общая точка входа для CLI (`main`) и
    интерактивного мастера (`run_wizard`).
    """
    if len(developers) == 1:
        run_task_loop(
            developers[0], max_tasks, fallback_preset, force_new, initial_model, laguna_mode,
        )
        return

    # Несколько — каждый в отдельном окне консоли. В дочерние окна передаём
    # уже развёрнутые полные ID — повторного резолва не нужно.
    script = Path(__file__).resolve()
    max_arg = f" --max-tasks {max_tasks}" if max_tasks > 0 else ""
    fb_arg = f" --fallback-model {fallback_preset}" if fallback_preset else ""
    new_arg = " --new" if force_new else ""
    model_arg = f" --model {initial_model}" if initial_model else ""
    laguna_arg = f" --laguna {laguna_mode}" if laguna_mode else ""

    for dev in developers:
        cmd = f'python "{script}" {dev}{max_arg}{fb_arg}{new_arg}{model_arg}{laguna_arg}'
        title = f"Lumen {dev}"
        if os.name == "nt":
            subprocess.Popen(f'start "{title}" cmd /k {cmd}', shell=True, cwd=PROJECT_DIR)
        else:
            subprocess.Popen(["bash", "-c", f"{cmd}; exec bash"], cwd=PROJECT_DIR)
        print(f"Запущен {dev} в отдельном окне.")

    print()
    print("Для остановки: python scripts/orchestrator.py --stop P1")
    print("Остановить всех: python scripts/orchestrator.py --stop-all")


# =====================================================================
# Интерактивный мастер (запуск без аргументов)
# =====================================================================

def _wiz_menu(title: str, options: list[tuple[str, str]], default: int = 1) -> int:
    """Показать нумерованное меню, вернуть выбранный 1-based индекс."""
    print()
    print(title)
    for i, (label, hint) in enumerate(options, 1):
        line = f"  {i}) {label}"
        if hint:
            line += f"  — {hint}"
        print(line)
    while True:
        raw = input(f"Выбор [{default}]: ").strip()
        if not raw:
            return default
        if raw.isdigit() and 1 <= int(raw) <= len(options):
            return int(raw)
        print(f"  Введите число 1..{len(options)}.")


def _wiz_yes_no(prompt: str, default: bool = False) -> bool:
    d = "Y/n" if default else "y/N"
    while True:
        raw = input(f"{prompt} [{d}]: ").strip().lower()
        if not raw:
            return default
        if raw in ("y", "yes", "д", "да"):
            return True
        if raw in ("n", "no", "н", "нет"):
            return False
        print("  Ответьте y/n.")


def _wiz_int(prompt: str, default: int) -> int:
    while True:
        raw = input(f"{prompt} [{default}]: ").strip()
        if not raw:
            return default
        if raw.isdigit():
            return int(raw)
        print("  Введите целое число.")


def _wiz_model(prompt: str, allow_default: bool = True) -> str | None:
    """Выбор модели через меню. Возвращает полный model ID или None (дефолт CLI)."""
    opts: list[tuple[str, str]] = []
    if allow_default:
        opts.append(("(по умолчанию CLI)", "не передавать --model"))
    opts += [
        ("haiku", MODEL_ALIASES["haiku"]),
        ("sonnet", MODEL_ALIASES["sonnet"]),
        ("opus", MODEL_ALIASES["opus"]),
        ("fable", MODEL_ALIASES["fable"]),
        ("ввести вручную", "alias или полный claude-*"),
    ]
    idx = _wiz_menu(prompt, opts, default=1)
    label = opts[idx - 1][0]
    if allow_default and idx == 1:
        return None
    if label == "ввести вручную":
        return resolve_model_alias(input("  Имя модели: ").strip())
    return MODEL_ALIASES[label]


# Роли разработчиков — подсказки в мастере.
_WIZ_ROLES: dict[str, str] = {
    "P1": "фичи (любая подсистема)",
    "P2": "резерв (задач обычно нет)",
    "P3": "только баг-фиксы",
    "P4": "только CSS-свойства",
    "P5": "здоровье кода (ставь лимит 1)",
}


def _wiz_developers(prompt: str = "Каких разработчиков запустить?") -> list[str]:
    print()
    print(prompt + " (P1–P5)")
    for d, h in _WIZ_ROLES.items():
        print(f"  {d} — {h}")
    while True:
        raw = input("Список (напр. '1 3 4' или 'P1 P3'): ").strip()
        if not raw:
            print("  Нужен хотя бы один.")
            continue
        toks = raw.replace(",", " ").upper().split()
        devs: list[str] = []
        ok = True
        for t in toks:
            if t.isdigit():
                t = "P" + t
            if t in _WIZ_ROLES:
                if t not in devs:
                    devs.append(t)
            else:
                print(f"  Неизвестно: {t}")
                ok = False
                break
        if ok and devs:
            return devs


def _wiz_equiv_cmd(developers, max_tasks, fallback_preset, force_new, initial_model, laguna_mode) -> str:
    """Эквивалентная CLI-команда для показа в сводке (учит флагам)."""
    parts = [f"python scripts/orchestrator.py {' '.join(developers)}"]
    if max_tasks > 0:
        parts.append(f"--max-tasks {max_tasks}")
    if initial_model:
        parts.append(f"--model {initial_model}")
    if fallback_preset:
        parts.append(f"--fallback-model {fallback_preset}")
    if laguna_mode:
        parts.append(f"--laguna {laguna_mode}")
    if force_new:
        parts.append("--new")
    return " ".join(parts)


def run_wizard() -> None:
    """Пошаговый мастер: вызывается при запуске без аргументов."""
    print("=" * 60)
    print("  Оркестратор Lumen — интерактивный запуск")
    print("  (для неинтерактивного режима см. --help)")
    print("=" * 60)
    try:
        action = _wiz_menu("Что сделать?", [
            ("Запустить разработчиков", ""),
            ("Показать статус", ""),
            ("Остановить разработчика(ов)", ""),
            ("Остановить всех", ""),
            ("Выход", ""),
        ], default=1)

        if action == 2:
            show_status()
            return
        if action == 3:
            create_stop_file(_wiz_developers("Кого остановить?"))
            return
        if action == 4:
            create_stop_file(["P1", "P2", "P3", "P4", "P5"])
            return
        if action == 5:
            print("Выход.")
            return

        # action 1 — сбор параметров запуска
        developers = _wiz_developers()

        mode_idx = _wiz_menu("Режим выполнения:", [
            ("Claude (обычный)", "сессии Claude Code"),
            ("Laguna assist", "Claude-водитель, код пишет Laguna"),
            ("Laguna solo", "без Claude, оркестратор-агент поверх Laguna"),
        ], default=1)
        laguna_mode = {1: None, 2: "assist", 3: "solo"}[mode_idx]
        if laguna_mode:
            print("  (нужны POOLSIDE_API_KEY в env/.tmp/poolside.env и пакет openai)")
            if laguna_mode == "solo":
                print("  ВНИМАНИЕ: solo шлёт исходники в poolside — всё публикуется.")

        # Модель и резерв — только для режимов с Claude (solo их игнорирует).
        initial_model: str | None = None
        fallback_preset: str | None = None
        if laguna_mode != "solo":
            initial_model = _wiz_model("Стартовая модель Claude:", allow_default=True)
            if _wiz_yes_no("Задать резервную модель при rate limit?", default=False):
                fallback_preset = _wiz_model("Резервная модель:", allow_default=False)

        # Лимит задач.
        recommend_one = "P5" in developers or laguna_mode == "solo"
        if "P5" in developers:
            print("\nP5 — ревизия рекуррентна, без лимита крутится бесконечно. Рекомендуется 1.")
        elif laguna_mode == "solo":
            print("\nsolo — финиш по правилам делается вручную. Рекомендуется 1.")
        else:
            print("\nЛимит задач на разработчика (0 = без лимита).")
        max_tasks = _wiz_int("Макс. задач", 1 if recommend_one else 0)

        force_new = _wiz_yes_no(
            "Старт с чистого листа (--new, не возобновлять прерванную сессию)?", default=False
        )

        # Сводка + эквивалентная команда.
        cmd = _wiz_equiv_cmd(developers, max_tasks, fallback_preset, force_new, initial_model, laguna_mode)
        print()
        print("-" * 60)
        print(f"  Разработчики : {' '.join(developers)}")
        print(f"  Режим        : {laguna_mode or 'Claude'}")
        if laguna_mode != "solo":
            print(f"  Модель       : {initial_model or '(дефолт CLI)'}")
            print(f"  Резерв       : {fallback_preset or '(нет)'}")
        print(f"  Лимит задач  : {max_tasks or 'без лимита'}")
        print(f"  --new        : {'да' if force_new else 'нет'}")
        print(f"  Эквивалент   : {cmd}")
        print("-" * 60)
        if not _wiz_yes_no("Запустить?", default=True):
            print("Отменено.")
            return

        dispatch_run(developers, max_tasks, fallback_preset, force_new, initial_model, laguna_mode)
    except (EOFError, KeyboardInterrupt):
        print()
        print("Прервано. Ничего не запущено.")


def main():
    # Запуск без аргументов вообще — пошаговый мастер.
    if len(sys.argv) == 1:
        run_wizard()
        return

    parser = argparse.ArgumentParser(
        description="Оркестратор задач Lumen — автозапуск сессий Claude Code."
    )
    parser.add_argument(
        "developers",
        nargs="*",
        choices=["P1", "P2", "P3", "P4", "P5"],
        metavar="DEV",
        help="Разработчики для запуска: P1 P2 P3 P4 P5",
    )
    parser.add_argument(
        "--max-tasks",
        type=int,
        default=0,
        help="Максимум задач на разработчика (0 = без ограничения)",
    )
    # Динамически собираем подсказку с алиасами для --model / --fallback-model.
    # ASCII-стрелка `->` вместо `→`, чтобы --help не падал на Windows cp1251.
    alias_help = ", ".join(f"{a} -> {full}" for a, full in MODEL_ALIASES.items())

    parser.add_argument(
        "--model",
        type=str,
        default=None,
        metavar="MODEL",
        help=(
            "Модель для первого и всех последующих вызовов claude "
            "(передаётся как `claude --model <id>`). "
            f"Алиасы: {alias_help}. "
            "Можно указать любой другой полный ID. "
            f"Также через env {INITIAL_MODEL_ENV}. "
            "При rate limit активный fallback переопределит эту модель."
        ),
    )
    parser.add_argument(
        "--fallback-model",
        type=str,
        default=None,
        metavar="MODEL",
        help=(
            "Резервная модель при rate limit основной "
            "(unattended-режим, отключает интерактивный prompt). "
            f"Алиасы: {alias_help}. "
            f"Также через env {FALLBACK_MODEL_ENV}."
        ),
    )
    parser.add_argument(
        "--new",
        action="store_true",
        help=(
            "Стартовать с чистого листа: удалить сохранённое состояние "
            "сессии (.session-PN.json) и не пытаться возобновить через "
            "claude --resume. Удобно, если прошлая сессия зависла или "
            "её контекст больше не релевантен."
        ),
    )
    parser.add_argument(
        "--laguna",
        type=str,
        default=None,
        choices=["assist", "solo"],
        help=(
            "Делегировать задачи в Laguna M.1 (poolside). "
            "assist — Claude остаётся водителем, но код пишет через .tmp/laguna.py; "
            "solo — оркестратор сам ведёт агент-петлю поверх Laguna без Claude "
            "(шлёт исходники в poolside — всё публикуется). Требует .tmp/poolside.env."
        ),
    )
    parser.add_argument(
        "--stop",
        nargs="+",
        choices=["P1", "P2", "P3", "P4", "P5"],
        metavar="DEV",
        help="Остановить разработчика после текущей задачи",
    )
    parser.add_argument(
        "--stop-all",
        action="store_true",
        help="Остановить всех разработчиков",
    )
    parser.add_argument(
        "--status",
        action="store_true",
        help="Показать статус всех разработчиков",
    )

    args = parser.parse_args()

    # Режим статуса
    if args.status:
        show_status()
        return

    # Режим остановки
    if args.stop_all:
        create_stop_file(["P1", "P2", "P3", "P4", "P5"])
        return

    if args.stop:
        create_stop_file(args.stop)
        return

    # Режим запуска
    if not args.developers:
        parser.print_help()
        sys.exit(1)

    # Приоритет: CLI > env > None. Алиасы (opus/sonnet/haiku) сразу
    # разворачиваются в полный model ID — дальше по коду уже только full ID.
    initial_model = resolve_model_alias(args.model) or resolve_model_alias(
        os.environ.get(INITIAL_MODEL_ENV)
    )
    fallback_model_preset = resolve_model_alias(args.fallback_model)

    dispatch_run(
        args.developers, args.max_tasks, fallback_model_preset,
        args.new, initial_model, args.laguna,
    )


if __name__ == "__main__":
    main()
