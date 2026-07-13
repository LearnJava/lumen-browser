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
    python scripts/orchestrator.py P1 --coders team --max-tasks 1       # цикл: Claude-брифы → кодеры → Claude-ревью
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

Классификация ошибок: причина берётся из терминального result-события
(api_error_status), текстовые регэкспы применяются только к не-JSON
строкам — иначе служебное rate_limit_event со status=allowed давало
ложный «rate limit». 403 может означать и обрыв сети: перед повтором
оркестратор проверяет доступность api.anthropic.com (TCP-проба) и при
обрыве ждёт восстановления сети (проба раз в 30 сек), а не считает
модель исчерпанной. Дочерние процессы упавшей сессии добиваются всегда
(защита от зомби cargo/lumen); возобновлённая сессия перезапускает
прерванные команды сама.

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

Делегирование кодерам Kilo (флаг --coders)
------------------------------------------
Опционально задачи можно отдавать бесплатным моделям через Kilo Gateway
(выбраны бенчем 2026-07-02, см. .tmp/kilo_bench.py):

    --coders assist  — Claude остаётся водителем (STATUS/правки/cargo/git/
                       /lumen-task-finish), но написание кода делегирует
                       Step 3.7 Flash через .tmp/kilo_client.py (к промпту
                       добавляется CODERS_ASSIST_NOTE).
    --coders solo    — Claude НЕ участвует: run_coder_solo() ведёт агент-петлю
                       в собственном worktree по текстовому tool-протоколу
                       READ/LIST/WRITE/BASH/DONE. Два независимых кодера
                       чередуются по задачам (Step 3.7 Flash ↔ Nemotron 3
                       Ultra); перед коммитом — ворота cargo check + clippy
                       -D warnings + test по затронутым crates; после коммита
                       nano-omni сравнивает скриншоты с main (визуальная
                       приёмка, вердикт в worklog). Финальное ревью, одобрение
                       и влитие в main — за Claude-сессией разработчика.
    --coders team    — полный конвейер одного цикла:
                       (1) Claude-сессия «бригадир» берёт две первые задачи из
                           STATUS, выделяет мелкие срезы и пишет по брифу на
                           кодера в .tmp/briefs/ (+ манифест);
                       (2) кодеры выполняют брифы (задача A — Step 3.7 Flash,
                           задача B — Nemotron 3 Ultra) с воротами cargo и
                           визуальной приёмкой nano-omni;
                       (3) Claude-сессия «ревьюер» смотрит диффы, одобряет или
                           отклоняет, вливает одобренное в main и чистит следы;
                       все сессии завершаются, цикл повторяется, пока есть
                       задачи. --max-tasks считает ЦИКЛЫ (1 цикл = до 2 задач).
                       Модели фаз настраиваются раздельно: --prep-model —
                       бригадир (по умолчанию fable: качество брифов решает
                       успех кодеров), --review-model — ревьюер (по умолчанию
                       --model / дефолт CLI; слабые модели не рекомендуются —
                       ревью — самый ответственный этап конвейера).

Нужен KILO_API_KEY (env или .tmp/kilo.env); SDK не нужен (urllib).
ПРИВАТНОСТЬ: free-эндпоинты Kilo/NVIDIA — trial, запросы логируются на их
стороне; solo шлёт исходники (READ) и скриншоты. Полная документация —
scripts/README.md, раздел «Делегирование кодерам Kilo».
"""

import argparse
import atexit
import json
import os
import signal
import socket
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
        """Вернуть PID всех живых потомков root_pid (POSIX: Linux/macOS/BSD).

        На Linux строит карту parent→children из /proc/<pid>/stat; при
        отсутствии /proc (macOS/BSD) — из вывода `ps -eo pid=,ppid=`. Затем
        обходит дерево от root_pid вширь. Пустое множество — если ни один
        источник недоступен (тогда осиротевшие потомки не добиваются, как
        было раньше на всех не-Windows).
        """
        parent_to_children: dict[int, list[int]] = {}
        proc_dir = Path("/proc")
        if proc_dir.is_dir():
            for entry in proc_dir.iterdir():
                if not entry.name.isdigit():
                    continue
                try:
                    stat = (entry / "stat").read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue  # процесс уже умер между iterdir и read
                # Формат stat: "pid (comm) state ppid ...". comm может
                # содержать пробелы и ')', поэтому режем хвост после ПОСЛЕДНЕЙ ')'.
                rparen = stat.rfind(")")
                if rparen == -1:
                    continue
                fields = stat[rparen + 2:].split()
                if len(fields) < 2:
                    continue
                try:
                    pid = int(entry.name)
                    ppid = int(fields[1])
                except ValueError:
                    continue
                parent_to_children.setdefault(ppid, []).append(pid)
        else:
            try:
                out = subprocess.run(
                    ["ps", "-eo", "pid=,ppid="],
                    capture_output=True, text=True, timeout=5,
                ).stdout
            except (OSError, subprocess.SubprocessError):
                return set()
            for line in out.splitlines():
                parts = line.split()
                if len(parts) != 2:
                    continue
                try:
                    pid, ppid = int(parts[0]), int(parts[1])
                except ValueError:
                    continue
                parent_to_children.setdefault(ppid, []).append(pid)

        result: set = set()
        queue = [root_pid]
        while queue:
            pid = queue.pop()
            for child in parent_to_children.get(pid, []):
                if child not in result:
                    result.add(child)
                    queue.append(child)
        return result

    def _kill_pids(pids: set) -> int:  # type: ignore[misc]
        """Завершить процессы по PID через SIGKILL. Возвращает число убитых."""
        killed = 0
        for pid in pids:
            try:
                os.kill(pid, signal.SIGKILL)
                killed += 1
            except OSError:
                pass  # процесс уже завершился или чужой — пропускаем
        return killed


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


# --- Worklog + STATUS-маркировка наработок кодеров (solo) ---
#
# В solo-режиме кодер (Step 3.7 Flash / Nemotron 3 Ultra) коммитит в своём
# worktree, но НЕ вливает в main. Чтобы Claude-разработчик потом нашёл,
# отревьюил и влил эти наработки, оркестратор:
#   1) метит строку-указатель в STATUS-PN.md как `<указатель> | <ветка> | <маркёр>`
#      (маркёр кодера, напр. `Step37` = «сделано кодером, ждёт ревью и влития»);
#   2) пишет читаемый отчёт в scripts/coders-worklog.md (кодер/worktree/ветка/
#      commit/вердикт визуальной приёмки).
# После ревью + merge Claude удаляет строку из worklog и снимает указатель из
# STATUS-PN.md (см. coders_review_note в task_prompt).

CODERS_WORKLOG = SCRIPTS_DIR / "coders-worklog.md"
_WORKLOG_HEADER = (
    "# Coders solo worklog\n\n"
    "Наработки кодеров Kilo (Step 3.7 Flash / Nemotron 3 Ultra) в solo-режиме "
    "оркестратора, ожидающие ревью и влития в main Claude-сессией "
    "разработчика. Одна строка таблицы = одна задача. После ревью + merge "
    "строку удаляет Claude (и снимает маркёр со строки-указателя в "
    "STATUS-PN.md). Vision — вердикт визуальной приёмки nano-omni, полный "
    "отчёт в scripts/vision-reports/<ветка>.md.\n\n"
    "| Время | Dev | Кодер | Задача | Указатель | Ветка | Worktree | Commit | Vision |\n"
    "|---|---|---|---|---|---|---|---|---|\n"
)


def _pointer_is_actionable(pointer: str) -> bool:
    """Указатель ведёт на реальную открытую работу?

    Защита от протухших STATUS-PN.md: указатель на строку ROADMAP.md со
    статусом `done` означает уже выполненную задачу — кодеру там нечего
    реализовывать, он лишь впустую гоняет READ/grep до лимита итераций.
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


def pick_coder_task(developer: str) -> tuple[int, str] | None:
    """Выбрать первую НЕпомеченную actionable строку-указатель из STATUS-PN.md.

    Возвращает (номер строки 1-based, текст указателя `<источник>:NN`) или
    None, если непомеченных actionable указателей нет. Пропускаются:
    - уже помеченные строки (`<указатель> | <ветка> | <маркёр>`) — сделаны
      кодером и ждут влития, повторно их брать нельзя;
    - указатели на строки ROADMAP.md со статусом `done`/`cancelled`/… —
      задача уже закрыта, кодер там зациклится на разведке (см.
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


def mark_coder_task(developer: str, pointer: str, branch: str, marker: str) -> None:
    """Пометить строку-указатель в STATUS-PN.md как сделанную кодером.

    Переписывает первую строку, чей текст равен `pointer`, в формат
    `<указатель> | <ветка> | <маркёр>` (маркёр кодера, напр. `Step37`). Это
    сигнал Claude-разработчику: наработку надо отревьюить и влить в main
    перед своей задачей. Если строка не найдена (формат изменился) — STATUS
    не трогается.
    """
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        return
    lines = status_file.read_text(encoding="utf-8").splitlines()
    for i, line in enumerate(lines):
        if line.strip() == pointer:
            lines[i] = f"{pointer} | {branch} | {marker}"
            break
    status_file.write_text("\n".join(lines) + "\n", encoding="utf-8")


def append_worklog(
    developer: str, coder: str, desc: str, pointer: str,
    branch: str, worktree: str, commit: str, vision: str,
) -> None:
    """Добавить строку о завершённой кодером задаче в scripts/coders-worklog.md.

    Создаёт файл с заголовком при первом вызове. `|` в описании и вердикте
    экранируется, чтобы не разрушить markdown-таблицу.
    """
    if not CODERS_WORKLOG.exists():
        CODERS_WORKLOG.write_text(_WORKLOG_HEADER, encoding="utf-8")
    ts = datetime.now().strftime("%Y-%m-%d %H:%M")
    safe_desc = desc.replace("|", "\\|")
    safe_vision = vision.replace("|", "\\|")
    row = (
        f"| {ts} | {developer} | {coder} | {safe_desc} | {pointer} "
        f"| {branch} | {worktree} | {commit} | {safe_vision} |\n"
    )
    with CODERS_WORKLOG.open("a", encoding="utf-8") as f:
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

# Модель Claude-бригадира (фаза 1 team-цикла) по умолчанию. Качество брифов —
# главный фактор успеха слабых кодеров (урок прогона 2026-07-07), поэтому
# дефолт — самая сильная модель. Переопределяется флагом --prep-model.
DEFAULT_TEAM_PREP_MODEL = MODEL_ALIASES["fable"]


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

            # Сначала пробуем JSON: текстовые регэкспы по сырой JSON-строке
            # давали ложные срабатывания — служебное событие rate_limit_event
            # со status="allowed" содержит подстроку "rate_limit", но лимит
            # НЕ исчерпан. JSON-события анализируем только структурно.
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                # Не-JSON строка — сообщение от CLI, здесь текстовый детект уместен
                if RATE_LIMIT_TEXT_RE.search(line):
                    rate_limited = True
                    m = RATE_LIMIT_RE.search(line)
                    if m and reset_time is None:
                        reset_time = m.group(1)
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

            # Детект rate limit в JSON-событии (только status=blocked;
            # status=allowed — обычный служебный отчёт, не ошибка)
            if event.get("type") == "rate_limit_event":
                info = event.get("rate_limit_info", {})
                if info.get("status", "").startswith("blocked"):
                    rate_limited = True
                    log(developer, "  Rate limit (blocked)")
            # Детект через текст СИНТЕТИЧЕСКОГО ассистент-сообщения
            # ("You've hit your session limit …"). Обычные сообщения модели
            # не проверяем: разработчик, работающий над кодом про rate limit,
            # упоминает эти слова в тексте — это не ошибка CLI.
            if event.get("type") == "assistant":
                msg = event.get("message", {})
                if msg.get("model") == "<synthetic>":
                    for block in msg.get("content", []):
                        if block.get("type") == "text":
                            text = block.get("text", "")
                            if RATE_LIMIT_TEXT_RE.search(text):
                                rate_limited = True
                                m = RATE_LIMIT_RE.search(text)
                                if m and reset_time is None:
                                    reset_time = m.group(1)
                                break
            # Терминальное result-событие с ошибкой: api_error_status —
            # самый достоверный признак причины смерти сессии.
            if event.get("type") == "result" and event.get("is_error"):
                api_status = event.get("api_error_status")
                rtext = str(event.get("result", ""))
                rl = rtext.lower()
                if api_status == 403 or (
                    "403" in rtext and ("forbidden" in rl or "authenticate" in rl)
                ):
                    auth_error = True
                    # Свежий терминальный 403 достовернее накопленного флага:
                    # обрыв сети тоже даёт 403, это не исчерпание лимита.
                    rate_limited = False
                    log(developer, "  Result: API 403 — auth error или обрыв сети")
                elif api_status == 429 or RATE_LIMIT_TEXT_RE.search(rtext):
                    rate_limited = True
                    m = RATE_LIMIT_RE.search(rtext)
                    if m and reset_time is None:
                        reset_time = m.group(1)
                    log(developer, "  Result: rate limit")

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


def wait_for_network(developer: str, host: str = "api.anthropic.com", port: int = 443) -> bool:
    """Если сети нет — ждать её восстановления (TCP-проба host:port раз в 30 сек).

    Возвращает True, если пришлось ждать (сеть была недоступна и вернулась),
    False — если сеть доступна с первой пробы (значит 403 пришёл не из-за
    обрыва сети, а из-за реальной проблемы авторизации/лимитов доступа).
    """
    waited = False
    while True:
        try:
            with socket.create_connection((host, port), timeout=5):
                pass
            if waited:
                log(developer, "Сеть восстановлена.")
            return waited
        except OSError:
            if not waited:
                waited = True
                log(developer, f"Сети нет ({host}:{port} недоступен) — жду восстановления, проба раз в 30 сек...")
                set_jobstatus(developer, "нет сети", "жду восстановления")
            time.sleep(30)


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
# Делегирование кодерам Kilo Gateway (Step 3.7 Flash + Nemotron 3 Ultra;
# визуальная приёмка — Nemotron 3 Nano Omni)
# =====================================================================
#
# Два режима, выбираются флагом `--coders {assist,solo}`:
#
#   assist — Claude остаётся водителем (читает STATUS, правит файлы,
#            гоняет cargo/git, зовёт /lumen-task-finish), но САМО написание
#            кода делегирует Step 3.7 Flash через `python .tmp/kilo_client.py`.
#            Это лишь добавка к промпту — никакого нового исполнения.
#
#   solo   — оркестратор сам агент-харнесс: ведёт многоходовый диалог с
#            кодером по текстовому tool-протоколу (READ/LIST/WRITE/BASH/DONE),
#            применяет правки, гоняет ворота cargo, на успехе коммитит.
#            Два независимых кодера чередуются по задачам (нечётная — Step 3.7
#            Flash, чётная — Nemotron 3 Ultra). После коммита nano-omni
#            сравнивает скриншоты worktree и main (визуальная приёмка,
#            вердикт в worklog). Claude не участвует — финальное ревью,
#            одобрение и merge делает Claude-сессия разработчика позже.
#
# Выбор моделей — бенч 2026-07-02 (.tmp/kilo_bench.py, память
# reference_kilo_free_models_bench): step-3.7-flash — быстрые «руки»
# (полный модуль с одного патча), nemotron-3-ultra — качество + честный
# фидбэк-цикл, nemotron-3-nano-omni — «глаза» (мультимодальный, точно
# описывает скриншоты; для правок кода непригоден). Laguna M.1
# дисквалифицирована (пустые ответы, удаляет тесты, нестабильный эндпоинт) —
# прежняя интеграция poolside удалена.
#
# ПРИВАТНОСТЬ: free-эндпоинты Kilo/NVIDIA — trial, запросы ЛОГИРУЮТСЯ на их
# стороне; solo шлёт содержимое исходников (READ) и скриншоты. Режимы
# включаются только явным флагом. Конфиденциальное не слать.

KILO_API_URL = "https://api.kilo.ai/api/gateway/chat/completions"
KILO_ENV = PROJECT_DIR / ".tmp" / "kilo.env"

# Реестр кодеров: ключ → model id (Kilo Gateway), человекочитаемый ярлык и
# маркёр для STATUS-PN.md. Ключ также идёт в имя ветки/worktree.
CODERS: dict[str, dict[str, str]] = {
    "step37": {
        "id": "stepfun/step-3.7-flash:free",
        "label": "Step 3.7 Flash",
        "marker": "Step37",
    },
    "nemotron": {
        "id": "nvidia/nemotron-3-ultra-550b-a55b:free",
        "label": "Nemotron 3 Ultra",
        "marker": "Nemotron3U",
    },
}
# Порядок чередования кодеров по задачам в solo-режиме.
CODER_ROTATION = ["step37", "nemotron"]
# Маркёры STATUS всех кодеров — для ревью-подсказки Claude.
CODER_MARKERS = [c["marker"] for c in CODERS.values()]
# «Глаза»: мультимодальная модель визуальной приёмки скриншотов.
VISION_MODEL_ID = "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free"

CODER_MAX_ITERS = 24
# Таймаут одного HTTP-запроса к Kilo, сек (free-tier бывает медленным).
KILO_TIMEOUT = float(os.environ.get("KILO_TIMEOUT", "300"))
# Потолок подряд идущих сетевых сбоев на одной итерации solo-петли.
CODER_MAX_NET_FAILS = int(os.environ.get("CODER_MAX_NET_FAILS", "5"))
# Лимиты докачки ответа (finish_reason == "length"): кодерам хватает 4;
# nano-omni склонен зацикливаться на continuation — ему 2 (память
# reference_kilo_free_models_bench: «убивать после 2-3 докачек»).
CODER_MAX_CONTINUATIONS = 4
VISION_MAX_CONTINUATIONS = 2


def coder_debug(developer: str, message: str) -> None:
    """Дописать строку в scripts/coder-debug-PN.log — полный лог обмена с Kilo.

    Отдельно от консольного `log()`: сюда идут размеры запроса/ответа, тайминг
    и детали троттлинга/сетевых сбоев — чтобы постфактум понять, уходит ли
    запрос и приходит ли ответ, и на чём именно рвётся обмен.
    """
    try:
        path = SCRIPTS_DIR / f"coder-debug-{developer}.log"
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        with path.open("a", encoding="utf-8") as f:
            f.write(f"[{ts}] {message}\n")
    except OSError:
        pass


# Добавка к промпту Claude в режиме assist.
CODERS_ASSIST_NOTE = (
    " РЕЖИМ КОДЕРОВ(assist): код НЕ пиши сам — делегируй написание Step 3.7 Flash. "
    "Собери точное ТЗ (сигнатуры, путь к файлу, ограничения) в messages-JSON в файле "
    "внутри .tmp/, затем вызови `source .tmp/kilo.env && python .tmp/kilo_client.py "
    "<messages.json> <ответ.txt>`. Для ревью чужим взглядом — тот же вызов с "
    "KILO_MODEL='nvidia/nemotron-3-ultra-550b-a55b:free'. Получив код — САМ проверь "
    "его: cargo check/clippy/test -p <crate>; при ошибках отправь на доработку тем же "
    "вызовом с текстом ошибки. Затем интегрируй по правилам Lumen и закоммить. "
    "Финальная сборка, тесты и ревью — всегда твои. Эндпоинты trial и логируются — "
    "секреты и конфиденциальное не слать."
)


def _load_kilo_key() -> str | None:
    """Достать KILO_API_KEY из env или .tmp/kilo.env (формат `export KILO_API_KEY=...`)."""
    if os.environ.get("KILO_API_KEY"):
        return os.environ["KILO_API_KEY"]
    if KILO_ENV.exists():
        for line in KILO_ENV.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("export "):
                line = line[len("export "):]
            if "=" in line and not line.startswith("#"):
                k, v = line.split("=", 1)
                if k.strip() == "KILO_API_KEY":
                    return v.strip().strip('"').strip("'")
    return None


class KiloClient:
    """Тонкий клиент Kilo Gateway (OpenAI-совместимый chat completions, urllib).

    Без SDK-зависимостей — та же форма запроса, что в `.tmp/kilo_client.py`.
    `reasoning.effort=low` по умолчанию (иначе reasoning-модели жгут токены на
    раздумья); модели, не принимающие параметр `reasoning`, получают повтор
    без него (детект по 400). Прогрессивный бэкофф на 403/429/5xx — free-tier
    троттлит общий ключ. Докачка длинных ответов — в `chat()`.
    """

    def __init__(self, developer: str, model: str) -> None:
        key = _load_kilo_key()
        if not key:
            raise RuntimeError("KILO_API_KEY не найден (ни в env, ни в .tmp/kilo.env)")
        self.developer = developer
        self.model = model
        self.key = key
        coder_debug(
            developer,
            f"KiloClient init: model={model} timeout={KILO_TIMEOUT}s key=...{key[-4:]}",
        )

    def _call_once(self, messages: list[dict], max_tokens: int, effort: str) -> tuple[str, str]:
        """Один HTTP-запрос с ретраями троттлинга. Возвращает (content, finish_reason)."""
        import urllib.error
        import urllib.request

        body: dict = {
            "model": self.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": 0.2,
            "reasoning": {"effort": effort},
        }
        backoff = 5.0
        drop_reasoning = False
        last_err: Exception | None = None
        for attempt in range(1, 9):
            if drop_reasoning:
                body.pop("reasoning", None)
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
            req = urllib.request.Request(
                KILO_API_URL, data=data, method="POST",
                headers={
                    "Authorization": f"Bearer {self.key}",
                    "Content-Type": "application/json",
                },
            )
            t0 = time.monotonic()
            try:
                with urllib.request.urlopen(req, timeout=KILO_TIMEOUT) as r:
                    d = json.load(r)
                ch = d["choices"][0]
                content = ch["message"].get("content") or ""
                finish = ch.get("finish_reason") or "stop"
                coder_debug(
                    self.developer,
                    f"← OK {time.monotonic() - t0:.1f}s resp_chars={len(content)} "
                    f"finish={finish}" + ("  (ПУСТОЙ ответ!)" if not content else ""),
                )
                return content, finish
            except urllib.error.HTTPError as e:
                payload = e.read()[:400].decode("utf-8", "replace")
                # Некоторые модели не принимают объект reasoning — раз убрать.
                if e.code == 400 and "reasoning" in payload.lower() and not drop_reasoning:
                    drop_reasoning = True
                    coder_debug(self.developer, "400 про reasoning — повтор без параметра")
                    continue
                if e.code in (403, 429, 500, 502, 503):
                    coder_debug(
                        self.developer,
                        f"троттлинг {e.code} (попытка {attempt}) — пауза {backoff:.0f}с: {payload[:200]}",
                    )
                    time.sleep(backoff)
                    backoff = min(backoff * 2, 180)
                    last_err = e
                    continue
                coder_debug(self.developer, f"HTTP {e.code}: {payload}")
                raise
            except Exception as e:  # сетевые сбои — ретрай с бэкоффом
                coder_debug(self.developer, f"[net] {type(e).__name__}: {e} — пауза {backoff:.0f}с")
                time.sleep(backoff)
                backoff = min(backoff * 2, 180)
                last_err = e
        raise RuntimeError(f"Kilo: исчерпаны ретраи ({last_err})")

    def chat(
        self,
        messages: list[dict],
        max_tokens: int = 16000,
        effort: str = "low",
        max_continuations: int = CODER_MAX_CONTINUATIONS,
    ) -> str:
        """Полный ответ с докачкой по finish_reason == "length".

        Докачка НЕ мутирует переданный `messages` (работает на временной
        копии), чтобы continuation-куски не попадали в персист диалога
        solo-петли.
        """
        req_chars = sum(len(str(m.get("content", ""))) for m in messages)
        coder_debug(
            self.developer,
            f"→ chat: model={self.model} msgs={len(messages)} req_chars={req_chars}",
        )
        content, finish = self._call_once(messages, max_tokens, effort)
        parts = [content]
        tmp = list(messages)
        cont = 0
        while finish == "length" and cont < max_continuations:
            cont += 1
            coder_debug(self.developer, f"finish=length — докачка {cont}/{max_continuations}")
            tmp = tmp + [
                {"role": "assistant", "content": content},
                {"role": "user", "content": "continue exactly where you stopped, no repetition"},
            ]
            content, finish = self._call_once(tmp, max_tokens, effort)
            parts.append(content)
        return "".join(parts)


# --- Текстовый tool-протокол для solo-режима ---

CODER_CMD_RE = re.compile(r"^(READ|LIST|WRITE|BASH|DONE)\b(.*)$")
# Что кодеру разрешено запускать через BASH: только чтение + сборка/тесты.
CODER_BASH_ALLOW = re.compile(
    r"^\s*(cargo\s+(check|test|clippy|build|fmt)|grep|rg|ls|git\s+(status|diff|log|branch))\b"
)
# Явный чёрный список разрушительного.
CODER_BASH_DENY = re.compile(
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


def parse_coder_commands(text: str) -> list[tuple[str, str, str | None]]:
    """Разобрать ответ кодера в список (cmd, arg, body).

    WRITE забирает следующий за ним fenced-блок ```...``` как body (полное
    новое содержимое файла). Остальные команды — однострочные, body=None.
    """
    lines = text.splitlines()
    cmds: list[tuple[str, str, str | None]] = []
    i = 0
    while i < len(lines):
        m = CODER_CMD_RE.match(lines[i].strip())
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


def _coder_do_read(arg: str, base: Path) -> str:
    p = _safe_path(arg, base)
    if p is None or not p.is_file():
        return f"READ {arg}: файл не найден или вне worktree."
    text = p.read_text(encoding="utf-8", errors="replace")
    if len(text) > 16000:
        text = text[:16000] + "\n…(обрезано)…"
    return f"=== READ {arg} ===\n{text}"


def _coder_do_list(arg: str, base: Path) -> str:
    pattern = arg or "*"
    try:
        hits = sorted(str(p.relative_to(base)) for p in base.glob(pattern))
    except (ValueError, OSError) as e:
        return f"LIST {arg}: ошибка ({e})"
    return f"=== LIST {arg} ({len(hits)}) ===\n" + "\n".join(hits[:200])


def _coder_do_write(developer: str, arg: str, body: str | None, written: set[str], base: Path) -> str:
    if body is None:
        return f"WRITE {arg}: нет fenced-блока с содержимым — пропущено."
    p = _safe_path(arg, base)
    if p is None:
        return f"WRITE {arg}: путь вне worktree — отказано."
    rel_norm = str(p.relative_to(base)).replace("\\", "/")
    # Тестовые страницы неприкосновенны (правило Lumen): бенч показал, что
    # free-модели «чинят» провалы удалением/правкой тестов.
    if rel_norm.startswith("graphic_tests/"):
        return f"WRITE {arg}: страницы graphic_tests/ неприкосновенны — чини код движка, а не тест."
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(body, encoding="utf-8")
    written.add(rel_norm)
    log(developer, f"  Кодер записал: {arg} ({len(body)} симв.)")
    return f"WRITE {arg}: ок ({len(body)} симв.)."


def _coder_do_bash(developer: str, arg: str, base: Path) -> str:
    if CODER_BASH_DENY.search(arg) or not CODER_BASH_ALLOW.match(arg):
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


def _create_solo_worktree(
    developer: str, task_number: int, coder_key: str
) -> tuple[Path | None, str | None, str]:
    """Создать изолированный worktree+ветку от main для solo-задачи кодера.

    Возвращает (path, branch, err). При ошибке path/branch = None, err — текст.
    """
    num = developer[1:] if developer.startswith("P") else developer
    stamp = datetime.now().strftime("%H%M%S")
    branch = f"p{num}-{coder_key}-t{task_number}-{stamp}"
    wt = PROJECT_DIR / ".claude" / "worktrees" / f"{developer.lower()}-{coder_key}-{stamp}"
    rc, out = _run_shell(
        f'git worktree add "{wt}" -b {branch} main', PROJECT_DIR, timeout=120
    )
    if rc != 0:
        return None, None, out
    return wt, branch, ""


def _coder_commit(developer: str, label: str, msg: str, paths: set[str], base: Path) -> None:
    """Закоммитить написанные кодером файлы в worktree (ветка уже своя)."""
    if paths:
        _run_shell("git add " + " ".join(f'"{p}"' for p in paths), base, timeout=120)
    full = (
        msg
        + f"\n\nНаписано {label} (Kilo Gateway) через solo-оркестратор; "
        "ворота: cargo check + clippy -D warnings + test пройдены.\n\n"
        f"Co-Authored-By: {label} <noreply@kilo.ai>\n"
    )
    # Сообщение через временный файл, чтобы не воевать с экранированием.
    msg_file = base / ".coder-commit-msg.txt"
    msg_file.write_text(full, encoding="utf-8")
    _run_shell('git commit -F ".coder-commit-msg.txt"', base, timeout=120)
    msg_file.unlink(missing_ok=True)


def coder_solo_system_prompt(developer: str) -> str:
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
        "Существующие тесты НЕ удалять и НЕ ослаблять: провал теста чинится кодом движка, а не "
        "правкой теста; страницы graphic_tests/ и пороги тестов неприкосновенны. "
        "ВОРОТА на DONE (обязаны пройти, иначе DONE отклонён): по каждому затронутому crate "
        "`cargo check -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`, "
        "`cargo test -p <crate>`. Прогоняй их сам через BASH до DONE и чини ошибки/варнинги. "
        "Не пиши прозу вне команд — она игнорируется. "
        f"Начни с: READ STATUS-{developer}.md"
    )


def coder_session_path(developer: str) -> Path:
    """Путь к файлу персиста solo-сессии кодера (для восстановления после краша)."""
    return SCRIPTS_DIR / f".coder-session-{developer}.json"


def save_coder_session(developer: str, state: dict) -> None:
    """Сохранить состояние solo-сессии: messages + worktree + ветка + written + кодер.

    Зачем: API Kilo (OpenAI-совместимый chat completions) — stateless,
    серверного `--resume` нет. Единственный способ возобновить прерванный
    диалог — переиграть сохранённый `messages`. Ключ KILO_API_KEY СЮДА НЕ
    пишется — он перечитывается из env/.tmp при каждом запуске (это секрет,
    не часть состояния сессии).
    """
    try:
        coder_session_path(developer).write_text(
            json.dumps(state, ensure_ascii=False), encoding="utf-8"
        )
    except OSError as e:
        log(developer, f"  (не удалось сохранить coder-сессию: {e})")


def load_coder_session(developer: str) -> dict | None:
    """Загрузить сохранённое состояние solo-сессии, если есть."""
    path = coder_session_path(developer)
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None


def clear_coder_session(developer: str) -> None:
    """Удалить файл персиста solo-сессии (на успехе / max-iters / --new)."""
    path = coder_session_path(developer)
    if path.exists():
        path.unlink()


def _coder_initial_messages(developer: str, pointer: str, desc: str) -> list[dict]:
    """Стартовый диалог для свежей solo-сессии.

    Задачу выбирает оркестратор (`pick_coder_task`) и передаёт явным
    указателем `pointer` (`<источник>:NN`) — кодер не выбирает сам, чтобы
    оркестратор точно знал, какую строку STATUS пометить маркёром на успехе.
    """
    return [
        {"role": "system", "content": coder_solo_system_prompt(developer)},
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


def _coder_brief_messages(developer: str, pointer: str, brief: str) -> list[dict]:
    """Стартовый диалог по брифу от Claude-бригадира (team-режим).

    Бриф самодостаточен (точные файлы/строки/сигнатуры/критерии готовности) —
    кодер не имеет доступа к диалогу бригадира. Скоуп ограничен брифом.
    """
    if len(brief) > 24000:
        brief = brief[:24000] + "\n…(бриф обрезан)…"
    return [
        {"role": "system", "content": coder_solo_system_prompt(developer)},
        {
            "role": "user",
            "content": (
                f"Твоя задача — указатель `{pointer}` из STATUS-{developer}.md. "
                "Ведущий разработчик подготовил тебе подробный бриф (ниже). "
                "Следуй ему ТОЧНО; за пределы брифа не выходи.\n\n"
                "=== БРИФ ===\n" + brief + "\n=== КОНЕЦ БРИФА ===\n\n"
                "Начни с READ файлов, названных в брифе. Когда критерии готовности "
                "выполнены и ворота (check + clippy -D warnings + test) зелёные — "
                "DONE с осмысленным текстом коммита."
            ),
        },
    ]


# --- Визуальная приёмка (nano-omni) ---
#
# После зелёных ворот и коммита кодера оркестратор снимает headless
# CPU-скриншоты (`lumen --screenshot`) эталонных страниц из worktree кодера
# и из main (PROJECT_DIR) и отдаёт пары изображений nano-omni на описание
# различий с вердиктом. Вердикт СОВЕЩАТЕЛЬНЫЙ: он пишется в worklog и в отчёт
# scripts/vision-reports/<ветка>.md, но коммит не откатывает — финальное
# решение принимает Claude-ревью.

VISION_PAGES = ["samples/page.html", "graphic_tests/1000000-final.html"]
VISION_REPORTS_DIR = SCRIPTS_DIR / "vision-reports"
VISION_VERDICT_RE = re.compile(r"^VERDICT:\s*(PASS|WARN|FAIL)\b(.*)$", re.MULTILINE)
# Кэш эталонных скриншотов main на время жизни процесса: page → путь PNG.
_vision_baselines: dict[str, Path] = {}


def _screenshot_page(base: Path, page: str, out: Path) -> str | None:
    """Снять headless CPU-скриншот страницы `page` движком из дерева `base`.

    Возвращает None при успехе, иначе текст ошибки (хвост вывода cargo).
    Первый вызов в свежем worktree собирает lumen-shell — это долго,
    таймаут щедрый.
    """
    out.parent.mkdir(parents=True, exist_ok=True)
    out.unlink(missing_ok=True)
    rc, log_out = _run_shell(
        f'cargo run -p lumen-shell -- --screenshot "{out}" "{page}"',
        base, timeout=2400,
    )
    if rc != 0 or not out.is_file():
        return (log_out or f"exit {rc}")[-1500:]
    return None


def _vision_baseline(developer: str, page: str) -> Path | None:
    """Эталонный скриншот страницы из main (кэшируется на процесс)."""
    cached = _vision_baselines.get(page)
    if cached and cached.is_file():
        return cached
    safe = re.sub(r"[^a-z0-9]+", "-", page.lower()).strip("-")
    out = PROJECT_DIR / ".tmp" / f"vision-baseline-{safe}.png"
    err = _screenshot_page(PROJECT_DIR, page, out)
    if err:
        tail = err.splitlines()[-1] if err.splitlines() else err
        log(developer, f"  vision: эталон main для {page} не снят: {tail}")
        return None
    _vision_baselines[page] = out
    return out


def _b64_png(path: Path) -> str:
    """PNG → base64-строка для data-URL в мультимодальном сообщении."""
    import base64
    return base64.b64encode(path.read_bytes()).decode("ascii")


def _write_vision_report(branch: str, verdict: str, reply: str, errors: list[str]) -> None:
    """Сохранить полный отчёт визуальной приёмки в scripts/vision-reports/<ветка>.md."""
    try:
        VISION_REPORTS_DIR.mkdir(parents=True, exist_ok=True)
        ts = datetime.now().strftime("%Y-%m-%d %H:%M")
        body = [
            f"# Визуальная приёмка `{branch}`",
            "",
            f"- Время: {ts}",
            f"- Модель: {VISION_MODEL_ID}",
            f"- Вердикт: **{verdict}**",
            "",
        ]
        if errors:
            body += ["## Проблемы шага", ""] + [f"- {e}" for e in errors] + [""]
        if reply:
            body += ["## Ответ nano-omni", "", reply, ""]
        (VISION_REPORTS_DIR / f"{branch}.md").write_text("\n".join(body), encoding="utf-8")
    except OSError:
        pass


def run_vision_acceptance(
    developer: str, work_dir: Path, branch: str, written: set[str]
) -> str:
    """Визуальная приёмка наработки кодера силами nano-omni («глаза»).

    Сравнивает скриншоты VISION_PAGES из worktree кодера и из main; пары
    изображений уходят nano-omni с просьбой описать различия и дать вердикт:
    PASS (различий нет/косметика), WARN (подозрительно), FAIL (явная
    поломка). Полный отчёт → scripts/vision-reports/<ветка>.md. Возвращает
    короткий вердикт для worklog. Любой сбой шага НЕ фатален — вернётся
    `SKIP (...)`, финальное решение всё равно за Claude-ревью.
    """
    if not any(p.endswith(".rs") or p.startswith("crates/") for p in written):
        return "N/A (не затронут код движка)"

    pairs: list[tuple[str, Path, Path]] = []  # (page, baseline, new)
    errors: list[str] = []
    for i, page in enumerate(VISION_PAGES):
        base_png = _vision_baseline(developer, page)
        if base_png is None:
            errors.append(f"{page}: эталон main не снят")
            continue
        new_png = work_dir / ".tmp" / f"vision-new-{i}.png"
        err = _screenshot_page(work_dir, page, new_png)
        if err:
            tail = err.splitlines()[-1] if err.splitlines() else err
            errors.append(f"{page}: скриншот worktree не снят: {tail}")
            continue
        pairs.append((page, base_png, new_png))

    if not pairs:
        verdict = "SKIP (скриншоты не сняты)"
        _write_vision_report(branch, verdict, "", errors)
        return verdict

    content: list[dict] = [{
        "type": "text",
        "text": (
            "Ты визуальный контролёр браузерного движка Lumen. Ниже пары "
            "скриншотов одних и тех же страниц: BASELINE (main, до правок) и "
            "NEW (после правок кодера). Задача кодера могла ЗАКОННО изменить "
            "рендеринг. Опиши по-русски все видимые различия каждой пары "
            "(если различий нет — так и скажи). Ищи признаки ПОЛОМКИ: "
            "исчезнувшие блоки, пустые области, налезание текста, сломанная "
            "разметка, потерянные цвета/рамки. Последней строкой дай вердикт "
            "строго в формате `VERDICT: PASS` (различий нет или косметика), "
            "`VERDICT: WARN <причина>` (подозрительно) или "
            "`VERDICT: FAIL <причина>` (явная поломка). "
            f"Страницы по порядку: {', '.join(p for p, _, _ in pairs)}."
        ),
    }]
    for page, base_png, new_png in pairs:
        content.append({"type": "text", "text": f"BASELINE {page}:"})
        content.append({
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64," + _b64_png(base_png)},
        })
        content.append({"type": "text", "text": f"NEW {page}:"})
        content.append({
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64," + _b64_png(new_png)},
        })

    log(developer, f"  vision: {len(pairs)} пар скриншотов → nano-omni...")
    try:
        client = KiloClient(developer, VISION_MODEL_ID)
        reply = client.chat(
            [{"role": "user", "content": content}],
            max_tokens=4000,
            max_continuations=VISION_MAX_CONTINUATIONS,
        )
    except Exception as e:  # noqa: BLE001 — приёмка совещательная, solo не роняем
        verdict = f"SKIP (vision недоступен: {type(e).__name__})"
        _write_vision_report(branch, verdict, "", errors + [f"{type(e).__name__}: {e}"])
        return verdict

    m = None
    for m in VISION_VERDICT_RE.finditer(reply):
        pass  # интересует последняя строка VERDICT в ответе
    if m:
        tail = m.group(2).strip()
        verdict = m.group(1) + (f" {tail}" if tail else "")
    else:
        verdict = "UNPARSED (нет строки VERDICT)"
    _write_vision_report(branch, verdict, reply, errors)
    return verdict


def run_coder_solo(
    developer: str,
    task_number: int,
    coder_key: str,
    pointer: str | None = None,
    brief: str | None = None,
) -> dict:
    """Solo-режим: оркестратор ведёт агент-петлю поверх кодера Kilo без Claude.

    Работает в изолированном worktree (своя ветка p<N>-<кодер>-*). Задачу
    выбирает САМ оркестратор — первый непомеченный указатель из STATUS-PN.md
    (`pick_coder_task`) — либо её задаёт вызывающий код явными `pointer` +
    `brief` (team-режим: бриф подготовлен Claude-бригадиром и целиком уходит
    кодеру стартовым сообщением). Перед коммитом — жёсткие ворота: cargo
    check + clippy -D warnings + test по затронутым crates; DONE без единого
    WRITE отклоняется (урок прогона 2026-07-07: step37 заявил DONE после
    чистой разведки, и ворота прошли на нетронутом дереве). После коммита —
    визуальная приёмка nano-omni (`run_vision_acceptance`, совещательная).

    Возвращает dict результата для worklog/ревью:
    `{ok, coder, pointer, branch, worktree, commit, vision}` — ok=True только
    при зелёных воротах + коммите.

    На успехе worktree/ветка ОСТАЮТСЯ для ревью, и оркестратор оставляет два
    следа для Claude-разработчика, который позже отревьюит и вольёт работу:
      - помечает строку-указатель в STATUS-PN.md как
        `<указатель> | <ветка> | <маркёр кодера>` (`mark_coder_task`);
      - дописывает отчёт (кодер/worktree/ветка/commit/vision) в
        scripts/coders-worklog.md (`append_worklog`).
    Само влитие в main/доксинк/чистку делает Claude (см. coders_review_note).

    Восстановление после краша. API Kilo stateless (серверного resume нет),
    поэтому состояние диалога персистится локально в `.coder-session-PN.json`
    после каждой итерации (messages + worktree + ветка + written + кодер;
    ключ НЕ сохраняется). Если при старте найден файл и его worktree цел —
    диалог переигрывается из сохранённого messages в ТОМ ЖЕ worktree и с ТЕМ
    ЖЕ кодером (переданный coder_key игнорируется), продолжая с места обрыва.
    Файл удаляется на успехе, на исчерпании итераций и по `--new` (последнее —
    в run_task_loop). Если worktree пропал — старт заново.
    """
    # --- Возобновление прерванной solo-сессии (если есть и worktree цел) ---
    work_dir: Path | None = None
    branch: str | None = None
    written: set[str] = set()
    messages: list[dict] = []
    start_iter = 1
    # pointer (параметр) — строка-указатель STATUS, над которой работаем;
    # None → задачу выберет pick_coder_task ниже.
    pointer_desc: str = ""

    saved = load_coder_session(developer)
    if saved:
        wt = Path(saved.get("worktree", ""))
        if wt.is_dir() and saved.get("messages"):
            work_dir = wt
            branch = saved.get("branch")
            written = set(saved.get("written", []))
            messages = saved["messages"]
            task_number = saved.get("task_number", task_number)
            coder_key = saved.get("coder", coder_key)
            pointer = saved.get("pointer")
            pointer_desc = saved.get("pointer_desc", "")
            start_iter = int(saved.get("iter", 0)) + 1
            if start_iter > CODER_MAX_ITERS:
                # Прошлая сессия упёрлась в лимит итераций — резюмировать нечего.
                log(developer, "Сохранённая solo-сессия исчерпала лимит итераций — старт заново.")
                clear_coder_session(developer)
                work_dir = None
            else:
                log(developer, f"Возобновляю прерванную solo-сессию: задача #{task_number}, итер. {start_iter}")
                log(developer, f"  Кодер: {CODERS.get(coder_key, {}).get('label', coder_key)}")
                log(developer, f"  Worktree: {work_dir}  (ветка {branch})")
                log(developer, f"  Диалог восстановлен: {len(messages)} сообщений, файлов записано: {len(written)}")
        else:
            log(developer, "Найдено состояние solo-сессии, но worktree отсутствует/пуст — старт заново.")
            clear_coder_session(developer)

    def _result(ok: bool, commit: str | None = None, vision: str | None = None) -> dict:
        """Снимок результата попытки (для solo-цикла и team-ревью)."""
        return {
            "ok": ok,
            "coder": CODERS.get(coder_key, {}).get("label", coder_key),
            "pointer": pointer,
            "branch": branch,
            "worktree": str(work_dir) if work_dir else None,
            "commit": commit,
            "vision": vision,
        }

    if coder_key not in CODERS:
        log(developer, f"Неизвестный кодер `{coder_key}` — останов.")
        return _result(False)
    coder = CODERS[coder_key]

    try:
        client = KiloClient(developer, coder["id"])
    except RuntimeError as e:
        log(developer, f"Kilo недоступен: {e}")
        return _result(False)

    if work_dir is None:
        if pointer is None:
            # Оркестратор сам выбирает задачу — первый непомеченный указатель
            # из STATUS-PN.md, чтобы на успехе пометить именно её маркёром.
            picked = pick_coder_task(developer)
            if picked is None:
                log(developer, f"В STATUS-{developer}.md нет непомеченных задач-указателей — solo нечего делать.")
                return _result(False)
            _, pointer = picked
        pointer_desc = resolve_pointer_desc(pointer)
        log(developer, f"  Задача (указатель): {pointer} — {pointer_desc}")
        log(developer, f"  Кодер: {coder['label']}" + ("  (по брифу)" if brief else ""))

        work_dir, branch, err = _create_solo_worktree(developer, task_number, coder_key)
        if work_dir is None:
            log(developer, f"Не удалось создать worktree для кодера: {err}")
            return _result(False)
        log(developer, f"  Worktree: {work_dir}  (ветка {branch})")
        if brief:
            messages = _coder_brief_messages(developer, pointer, brief)
        else:
            messages = _coder_initial_messages(developer, pointer, pointer_desc)
        start_iter = 1

    def _persist(cur_iter: int) -> None:
        save_coder_session(developer, {
            "developer": developer,
            "task_number": task_number,
            "coder": coder_key,
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
    net_fails = 0  # подряд идущие сетевые сбои на текущей итерации
    while it <= CODER_MAX_ITERS:
        log(developer, f"{coder['label']} solo: итерация {it}/{CODER_MAX_ITERS}...")
        set_jobstatus(developer, "coder-solo", f"{coder_key}, задача #{task_number}, итер. {it}")
        try:
            reply = client.chat(messages)
        except Exception as e:  # исчерпаны ретраи клиента — сеть/эндпоинт лежат
            net_fails += 1
            coder_debug(
                developer,
                f"✗ chat FAIL (итер. {it}, сбой {net_fails}/{CODER_MAX_NET_FAILS}): "
                f"{type(e).__name__}: {e}\n{traceback.format_exc()}",
            )
            if net_fails >= CODER_MAX_NET_FAILS:
                # Подряд слишком много сбоев — не крутиться вечно: сохранить
                # диалог (резюмируемо) и выйти.
                _persist(it - 1)
                log(developer,
                    f"  Kilo API: {net_fails} сбоев подряд — останов solo "
                    f"(сессия сохранена, резюмируется позже). "
                    f"Детали → scripts/coder-debug-{developer}.log")
                return _result(False)
            log(developer,
                f"  Ошибка Kilo API: {type(e).__name__}: {e}. "
                f"Сбой {net_fails}/{CODER_MAX_NET_FAILS}, пауза 30с. "
                f"(детали → scripts/coder-debug-{developer}.log)")
            time.sleep(30)
            continue

        net_fails = 0  # успешный обмен — сбросить счётчик сбоев
        messages.append({"role": "assistant", "content": reply})
        cmds = parse_coder_commands(reply)
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
                feedback.append(_coder_do_read(arg, work_dir))
            elif cmd == "LIST":
                feedback.append(_coder_do_list(arg, work_dir))
            elif cmd == "WRITE":
                feedback.append(_coder_do_write(developer, arg, body, written, work_dir))
            elif cmd == "BASH":
                feedback.append(_coder_do_bash(developer, arg, work_dir))
            elif cmd == "DONE":
                commit_msg = arg or f"{developer}: задача #{task_number} ({coder['label']})"

        if commit_msg is not None and not written:
            # Работа без единого WRITE не принимается: иначе ворота проходят
            # на нетронутом дереве и «успех» оставляет пустую ветку.
            feedback.append(
                "DONE ОТКЛОНЁН — не записано ни одного файла (ни одного WRITE). "
                "Задача без изменений кода не принимается: внеси правки и снова DONE."
            )
            commit_msg = None

        if commit_msg is not None:
            log(developer, f"  {coder['label']} заявил DONE — прогоняю ворота (check + clippy -D + test)...")
            ok, gate_out = _run_solo_gates(developer, work_dir, written)
            if ok:
                _coder_commit(developer, coder["label"], commit_msg, written, work_dir)
                # Хеш только что созданного коммита — для отчёта в worklog.
                rc_h, head = _run_shell("git rev-parse --short HEAD", work_dir, timeout=30)
                commit_hash = head.strip().splitlines()[-1] if rc_h == 0 and head.strip() else "?"
                # Визуальная приёмка nano-omni (совещательная, после коммита).
                set_jobstatus(developer, "coder-vision", f"{coder_key}, задача #{task_number}")
                vision = run_vision_acceptance(developer, work_dir, branch or "?", written)
                log(developer, f"  Визуальная приёмка nano-omni: {vision}")
                # Пометить строку STATUS как сделанную кодером + записать отчёт.
                if pointer:
                    mark_coder_task(developer, pointer, branch or "?", coder["marker"])
                    append_worklog(
                        developer, coder["label"], pointer_desc, pointer,
                        branch or "?", str(work_dir), commit_hash, vision,
                    )
                    log(developer, f"  STATUS-{developer}.md: указатель помечен `{coder['marker']}`.")
                    log(developer, f"  Отчёт дописан в {CODERS_WORKLOG}")
                clear_coder_session(developer)
                log(developer, f"{coder['label']} solo: ворота пройдены, закоммичено.")
                log(developer, f"  Готово к ревью/влитию Claude: ветка {branch} @ {work_dir} (commit {commit_hash})")
                return _result(True, commit=commit_hash, vision=vision)
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
    clear_coder_session(developer)
    log(developer, f"{coder['label']} solo: исчерпан лимит итераций ({CODER_MAX_ITERS}). Останов.")
    log(developer, f"  Worktree с незавершённой работой оставлен: {work_dir}")
    log(developer, f"  Убрать: git worktree remove {work_dir} --force && git branch -D {branch}")
    return _result(False)


def coders_review_note(developer: str) -> str:
    """Пред-шаг для Claude-разработчика: отревьюить и влить наработки кодеров.

    В solo-режиме оркестратор метит сделанные кодерами строки STATUS маркёрами
    (`Step37`/`Nemotron3U`) и пишет отчёт в scripts/coders-worklog.md, но НЕ
    вливает их в main. Перед своей задачей разработчик обязан разобраться с
    этими наработками: отревьюить дифф, проверить ворота и влить (или
    отклонить), затем почистить worklog, STATUS и vision-отчёты.
    """
    markers = "`" + "`/`".join(CODER_MARKERS) + "`"
    return (
        f" ПЕРЕД своей задачей проверь наработки кодеров. Открой STATUS-{developer}.md и "
        f"найди строки с третьим параметром-маркёром {markers} (формат "
        "`<указатель> | <ветка> | <маркёр>`). Для КАЖДОЙ такой строки: "
        "(1) посмотри scripts/coders-worklog.md — там кодер, worktree, ветка, commit и "
        "вердикт визуальной приёмки nano-omni (полный отчёт — scripts/vision-reports/<ветка>.md); "
        "(2) сделай РЕВЬЮ диффа ветки (git log -p main..<ветка>): корректность, стиль Lumen, "
        "doc-комменты, тесты НЕ удалены и НЕ ослаблены; "
        "(3) прогони ворота на её ветке (cargo check + clippy -D warnings + test по затронутым crate); "
        "(4) при зелёных воротах и вменяемом ревью влей ветку в main (git merge --no-ff), "
        "сделай доксинк, удали ветку и worktree; "
        f"(5) удали строку этой задачи из scripts/coders-worklog.md, её строку-указатель из "
        f"STATUS-{developer}.md и отчёт из scripts/vision-reports/; (6) git push origin main. "
        "Если ворота красные или ревью выявило проблемы — НЕ вливай: оставь строку и worklog "
        "как есть, сообщи об этом. Только разобравшись со всеми наработками кодеров, "
        "приступай к своей задаче. "
    )


# --- Режим team: Claude-бригадир → два кодера → Claude-ревью ---
#
# Полный конвейер одного цикла (`--coders team`):
#   1. Claude-сессия «бригадир» (team_prep_prompt): берёт две первые
#      непомеченные задачи из STATUS-PN.md, для крупных выделяет мелкий срез,
#      пишет самодостаточные брифы в .tmp/briefs/ + манифест JSON. Код не
#      трогает. Сессия завершается.
#   2. Оркестратор гонит кодеров по брифам (run_coder_solo с pointer+brief):
#      задача A — Step 3.7 Flash, задача B — Nemotron 3 Ultra,
#      последовательно (общий троттлинг ключа). Ворота + vision как в solo.
#   3. Claude-сессия «ревьюер» (team_review_prompt): ревью диффов, ворота,
#      одобряет → merge --no-ff в main + доксинк + чистка STATUS/worklog/
#      vision/веток/worktree; отклоняет → сносит ветку без влития и пишет
#      причину. Пустые провальные ветки сносит. Push. Сессия завершается.
#   4. Цикл повторяется, пока есть задачи; --max-tasks считает циклы.
#
# Урок прогона 2026-07-07: кодерам нельзя давать сырой указатель на большую
# roadmap-задачу — они тонут в разведке (24 итерации grep без единого WRITE).
# Бриф от Claude с точными файлами/строками — обязательный вход.

BRIEFS_DIR = PROJECT_DIR / ".tmp" / "briefs"


def team_manifest_path(developer: str) -> Path:
    """Путь к JSON-манифесту брифов, который пишет Claude-бригадир в фазе 1."""
    return BRIEFS_DIR / f"{developer}-manifest.json"


def team_prep_prompt(developer: str) -> str:
    """Промпт фазы 1 team-цикла: Claude-бригадир готовит брифы двум кодерам."""
    manifest = f".tmp/briefs/{developer}-manifest.json"
    return (
        f"Ты ведущий разработчик {developer} проекта Lumen. Этап: подготовка задач для двух "
        "слабых AI-кодеров (Step 3.7 Flash и Nemotron 3 Ultra). САМ код движка НЕ пиши, "
        f"ничего не коммить, STATUS/ROADMAP не менять. Прочитай STATUS-{developer}.md и возьми "
        "ДВЕ первые строки-указателя формата `<файл>:NN` БЕЗ суффикса ` | ` (помеченные уже в "
        "работе). Если непомеченных меньше двух — возьми сколько есть. Для каждой изучи "
        "источник и код; если задача крупная или многосрезовая — выдели ОДИН маленький "
        "самостоятельный срез (размер XS/S, один заход слабой модели). Напиши для каждой "
        f"самодостаточный бриф в файл .tmp/briefs/{developer}-task-a.md (вторая — "
        f"{developer}-task-b.md) по правилам разбивки для слабых моделей: точные пути файлов и "
        "номера строк, полные сигнатуры, короткие выдержки существующего кода, что строго НЕ "
        "трогать (тесты не ослаблять, graphic_tests/ неприкосновенны), стиль Lumen (edition "
        "2024, без unwrap/panic в проде, /// на всех pub), критерии готовности (какие cargo-"
        "команды по каким crate должны позеленеть). Кодер НЕ видит этот диалог — бриф должен "
        f"быть полным сам по себе. Затем запиши манифест {manifest} строго вида: "
        '{"tasks": [{"pointer": "<файл>:NN", "brief": ".tmp/briefs/' + developer + '-task-a.md", '
        '"slice": "краткое имя среза"}, ...]}. Если непомеченных задач нет вообще — запиши '
        '{"tasks": []}. После записи манифеста сразу заверши сессию.'
    )


def load_team_manifest(developer: str) -> list[dict]:
    """Прочитать манифест фазы 1. Возвращает до двух валидных задач.

    Валидная запись — dict с непустыми `pointer` и `brief`. Битый JSON или
    отсутствие файла → пустой список (цикл останавливается с сообщением).
    """
    path = team_manifest_path(developer)
    if not path.exists():
        return []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return []
    tasks = data.get("tasks", []) if isinstance(data, dict) else []
    out: list[dict] = []
    for t in tasks[:2]:
        if isinstance(t, dict) and t.get("pointer") and t.get("brief"):
            out.append(t)
    return out


def team_review_prompt(developer: str, results: list[dict]) -> str:
    """Промпт фазы 3 team-цикла: Claude-ревьюер разбирает наработки кодеров.

    `results` — список dict из run_coder_solo. Успешные ветки ревьюятся и
    вливаются (или мотивированно отклоняются), провальные пустые — сносятся.
    """
    lines = []
    for r in results:
        status = (
            "ворота пройдены, закоммичено"
            if r.get("ok")
            else "ПРОВАЛ (лимит итераций / ошибка) — ветка скорее всего пустая"
        )
        lines.append(
            f"- кодер {r.get('coder')}; указатель {r.get('pointer')}; ветка {r.get('branch')}; "
            f"worktree {r.get('worktree')}; commit {r.get('commit') or '—'}; "
            f"vision: {r.get('vision') or '—'}; статус: {status}"
        )
    listing = "\n".join(lines) if lines else "(пусто)"
    return (
        f"Ты ведущий разработчик {developer} проекта Lumen. Этап: ревью наработок AI-кодеров "
        f"этого цикла. Новые задачи НЕ бери. Результаты:\n{listing}\n\n"
        "Для КАЖДОЙ успешной ветки: (1) посмотри дифф `git log -p main..<ветка>`, сверь с "
        "брифом в .tmp/briefs/ и вердиктом vision (scripts/vision-reports/<ветка>.md, если "
        "есть); проверь корректность, стиль Lumen, doc-комменты, что тесты не удалены и не "
        "ослаблены; (2) прогони ворота в её worktree: cargo check + clippy --all-targets "
        "-D warnings + test по затронутым crate; (3) ОДОБРЯЕШЬ → влей в main "
        "(git merge --no-ff), сделай доксинк по матрице CLAUDE.md, удали строку-указатель "
        f"задачи из STATUS-{developer}.md, её строку из scripts/coders-worklog.md и отчёт из "
        "scripts/vision-reports/, удали ветку и worktree; НЕ одобряешь → НЕ вливай: верни "
        f"строку-указатель в STATUS-{developer}.md к исходному виду `<файл>:NN` (сними "
        "` | <ветка> | <маркёр>`), удали строку worklog, ветку и worktree, и в финальном "
        "ответе объясни причину отказа. Для каждой ПРОВАЛЬНОЙ ветки: просто удали ветку и "
        "worktree (git worktree remove --force + git branch -D; коммитов там нет). "
        f"В конце: если было влито — git push origin main; удали файлы .tmp/briefs/{developer}-*; "
        "коротко подведи итог цикла (что влито, что отклонено и почему) и заверши сессию."
    )


# Дисциплина прогонов cargo (добавка к промптам сессий). Родилась из аудита
# лога 2026-07-08: сессии перезапускали clippy/test ради другого grep-фильтра,
# гоняли ворота в фоне (пустой буферизованный output → минуты поллинга +
# повторный прогон) и дублировали per-crate ворота перед /lumen-task-finish.
# Полные правила — docs/commands.md «Gate discipline».
GATE_DISCIPLINE_NOTE = (
    " Дисциплина прогонов: пока пишешь код — только cargo check -p; "
    "clippy -p и точечные тесты (cargo test -p <crate> -- <module>) — ОДИН раз "
    "перед коммитом; полные ворота (workspace clippy + scoped-test) гоняет "
    "/lumen-task-finish один раз, СИНХРОННО (foreground Bash, timeout 600000), "
    "не в фоне. Вывод cargo пиши в .tmp/<имя>.log и фильтруй grep-ом по файлу — "
    "НЕ перезапускай cargo ради другого фильтра вывода."
)


def task_prompt(developer: str, coders_mode: str | None = None) -> str:
    """Стандартный промпт для старта задачи с чистым диалогом."""
    note = CODERS_ASSIST_NOTE if coders_mode == "assist" else ""
    # Claude-режимы (None/assist) сначала проверяют и вливают наработки кодеров.
    # В solo Claude не участвует — пред-шаг не нужен.
    review = coders_review_note(developer) if coders_mode != "solo" else ""
    if developer == "P3":
        return (
            "Ты разработчик P3 (только баг-фиксы)." + review +
            " Прочитай STATUS-P3.md. "
            "Если есть 'In progress' — продолжи ЭТОТ ОДИН баг. "
            "Если нет — возьми ПЕРВЫЙ баг из 'Next' (только один, не больше). "
            "Когда баг исправлен — вызови /lumen-task-finish. "
            "ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
            "Не бери следующий баг. Один баг = одна сессия." +
            GATE_DISCIPLINE_NOTE + note
        )
    return (
        f"Ты разработчик {developer}." + review +
        f" Прочитай STATUS-{developer}.md. "
        f"Если есть 'In progress' — продолжи эту задачу. "
        f"Если нет — возьми первую задачу из 'Next'. "
        f"Когда задача завершена — вызови /lumen-task-finish." +
        GATE_DISCIPLINE_NOTE + note
    )


def resume_after_error_prompt(developer: str, coders_mode: str | None = None) -> str:
    """Промпт для возобновления сессии, прерванной ошибкой (rate limit / 403 / сбой CLI)."""
    note = CODERS_ASSIST_NOTE if coders_mode == "assist" else ""
    base = (
        "Сессия была прервана ошибкой (rate limit / auth error / сбой CLI). "
        "Выполни git status, сверься с историей диалога выше и продолжи текущую "
        "задачу с места остановки. Когда задача завершена — вызови /lumen-task-finish."
        + GATE_DISCIPLINE_NOTE
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
    coders_mode: str | None = None,
    prep_model: str | None = None,
    review_model: str | None = None,
):
    """Цикл задач для одного разработчика.

    coders_mode — режим делегирования кодерам Kilo:
      None     — обычный Claude-цикл;
      "assist" — Claude-водитель, но код пишет Step 3.7 Flash через
                 .tmp/kilo_client.py (добавка к промпту);
      "solo"   — Claude не используется: задачи гонит run_coder_solo(),
                 кодеры чередуются (Step 3.7 Flash ↔ Nemotron 3 Ultra);
      "team"   — цикл из трёх фаз: Claude-бригадир пишет брифы →
                 кодеры выполняют их → Claude-ревьюер вливает одобренное
                 (max_tasks считает циклы, 1 цикл = до 2 задач).

    prep_model / review_model — модели Claude для фаз 1 и 3 team-цикла
    (полные ID, алиасы развёрнуты в main). prep_model=None → fable
    (DEFAULT_TEAM_PREP_MODEL), review_model=None → initial_model / дефолт CLI.
    Активный fallback при rate limit переопределяет обе.

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
    if coders_mode:
        log(developer, f"Режим кодеров: {coders_mode}")
        if coders_mode == "solo":
            log(developer, "  ВНИМАНИЕ: solo шлёт исходники и скриншоты на trial-эндпоинты Kilo/NVIDIA (логируются).")

    def attempt_task(
        task_number: int, prompt: str, resume_id: str | None,
        model_override: str | None = None,
    ) -> bool:
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
                    model=fallback_model or model_override or initial_model,
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
                prompt = resume_after_error_prompt(developer, coders_mode)

            # auth_error проверяется ПЕРВЫМ: терминальный 403 (обрыв сети
            # тоже даёт 403) достовернее флага rate limit, который мог
            # накопиться от текстовых срабатываний ранее в сессии.
            if auth_error:
                generic_failures = 0
                set_jobstatus(developer, "auth error", f"задача #{task_number}")
                if wait_for_network(developer):
                    log(developer, "Причиной 403 был обрыв сети — возобновляю сразу.")
                else:
                    log(developer, "Сеть доступна — 403 похож на реальный auth error. Пауза 60 сек...")
                    time.sleep(60)
            elif rate_limited:
                generic_failures = 0
                if fallback_model is None:
                    # Первый rate limit — выбираем резервную модель и
                    # возобновляем сессию сразу, без паузы.
                    fallback_model = resolve_fallback_model(developer, fallback_preset)
                    announce_fallback(developer, f"задача #{task_number}", fallback_model)
                else:
                    log(developer, f"Резервная модель {fallback_model} тоже исчерпана.")
                    wait_for_rate_limit(developer, reset_time)
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
                    prompt = task_prompt(developer, coders_mode)
                    generic_failures = 0
                log(developer, "Пауза 30 секунд перед повтором...")
                time.sleep(30)

            if resume_id:
                log(developer, f"Возобновляю сессию через --resume {resume_id[:16]}...")
            set_jobstatus(developer, "работает", f"задача #{task_number} (повтор)")

    # --- Принудительный старт с чистого листа ---
    if force_new:
        if coders_mode == "solo":
            if coder_session_path(developer).exists():
                log(developer, "Флаг --new: удаляю сохранённую solo-сессию кодера, начинаю заново.")
                clear_coder_session(developer)
            else:
                log(developer, "Флаг --new: сохранённой solo-сессии нет, стартую с нуля.")
        elif session_state_path(developer).exists():
            log(developer, "Флаг --new: удаляю сохранённое состояние сессии, не возобновляю.")
            clear_session_state(developer)
        else:
            log(developer, "Флаг --new: сохранённого состояния нет, стартую с нуля.")

    # --- Восстановление после краша (только для Claude-режимов; solo резюмируется
    #     внутри run_coder_solo через .coder-session-PN.json, не здесь) ---
    existing = load_session_state(developer) if coders_mode != "solo" else None
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

        if coders_mode == "team":
            # Team-цикл: Claude-брифы → кодеры → Claude-ревью и merge.
            # task_count здесь = номер ЦИКЛА (1 цикл = до 2 задач).
            team_prep = prep_model or DEFAULT_TEAM_PREP_MODEL
            log(developer, f"Team-цикл #{task_count}: фаза 1 — Claude готовит брифы (модель {team_prep})...")
            try:
                team_manifest_path(developer).unlink(missing_ok=True)
            except OSError:
                pass
            save_session_state(developer, task_count)
            if not attempt_task(task_count, team_prep_prompt(developer), None,
                                model_override=team_prep):
                task_count -= 1
                break
            team_tasks = load_team_manifest(developer)
            if not team_tasks:
                log(developer, "Манифест брифов пуст/не создан — непомеченных задач нет. Останов.")
                task_count -= 1
                break
            results: list[dict] = []
            for i, t in enumerate(team_tasks):
                coder_key = CODER_ROTATION[i % len(CODER_ROTATION)]
                brief_path = PROJECT_DIR / t["brief"]
                try:
                    brief_text = brief_path.read_text(encoding="utf-8")
                except OSError as e:
                    log(developer, f"Бриф {t['brief']} не читается ({e}) — задача пропущена.")
                    continue
                log(developer,
                    f"Team-цикл #{task_count}: фаза 2 — кодер {CODERS[coder_key]['label']} "
                    f"по брифу {t['brief']} ({t.get('slice', t['pointer'])})...")
                results.append(run_coder_solo(
                    developer, task_count, coder_key,
                    pointer=t["pointer"], brief=brief_text,
                ))
            if not results:
                log(developer, "Ни один бриф не ушёл кодерам — останов.")
                task_count -= 1
                break
            ok_n = sum(1 for r in results if r.get("ok"))
            log(developer,
                f"Team-цикл #{task_count}: фаза 3 — Claude ревьюит "
                f"({ok_n}/{len(results)} веток с коммитами, "
                f"модель {review_model or initial_model or 'дефолт CLI'})...")
            save_session_state(developer, task_count)
            if not attempt_task(task_count, team_review_prompt(developer, results), None,
                                model_override=review_model):
                task_count -= 1
                break
            log(developer, f"Team-цикл #{task_count} завершён (роздано задач: {len(results)}).")
            continue

        if coders_mode == "solo":
            # Solo: оркестратор сам агент поверх кодера Kilo, без claude и без
            # .session-файла (нечего возобновлять через --resume). Кодеры
            # чередуются по задачам; при resume сохранённая сессия сама
            # восстановит своего кодера.
            coder_key = CODER_ROTATION[(task_count - 1) % len(CODER_ROTATION)]
            log(developer, f"Запуск кодера {CODERS[coder_key]['label']} (solo)...")
            if not run_coder_solo(developer, task_count, coder_key).get("ok"):
                task_count -= 1  # задача не завершилась — останов цикла
                break
            log(developer, f"Задача #{task_count} завершена (кодер solo).")
            continue

        # Записать состояние ДО запуска — чтобы не потерять при краше
        save_session_state(developer, task_count)

        log(developer, "Запуск claude...")
        if not attempt_task(task_count, task_prompt(developer, coders_mode), None):
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


def _linux_terminal_argv(title: str, inner_cmd: str) -> list[str] | None:
    """Подобрать argv для запуска inner_cmd в новом окне терминала (Linux/BSD).

    Возвращает готовый argv для первого установленного эмулятора терминала
    (konsole, gnome-terminal, alacritty, kitty, xfce4-terminal, foot, xterm,
    x-terminal-emulator) или None, если ни один не найден либо нет графической
    сессии (нет ни DISPLAY, ни WAYLAND_DISPLAY). Окно держится открытым после
    завершения задачи (`exec bash`), чтобы можно было прочитать вывод.
    """
    import shutil

    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        return None
    hold = f"{inner_cmd}; exec bash"
    # (бинарь, построитель argv). Порядок = приоритет; первый найденный побеждает.
    builders: list[tuple[str, "callable"]] = [
        ("konsole",             lambda b: [b, "-p", f"tabtitle={title}", "-e", "bash", "-c", hold]),
        ("gnome-terminal",      lambda b: [b, "--title", title, "--", "bash", "-c", hold]),
        ("alacritty",           lambda b: [b, "-t", title, "-e", "bash", "-c", hold]),
        ("kitty",               lambda b: [b, "--title", title, "bash", "-c", hold]),
        ("xfce4-terminal",      lambda b: [b, f"--title={title}", "-x", "bash", "-c", hold]),
        ("foot",                lambda b: [b, "-T", title, "bash", "-c", hold]),
        ("xterm",               lambda b: [b, "-T", title, "-e", "bash", "-c", hold]),
        ("x-terminal-emulator", lambda b: [b, "-T", title, "-e", "bash", "-c", hold]),
    ]
    for name, build in builders:
        path = shutil.which(name)
        if path:
            return build(path)
    return None


def _spawn_dev_window(dev: str, cmd: str) -> None:
    """Запустить команду разработчика в отдельном окне/сессии (кроссплатформенно).

    Windows — новое окно cmd. Linux/BSD с графической сессией — новое окно
    терминала (см. `_linux_terminal_argv`). Без графической сессии, но с tmux —
    detached-сессия `lumen-<dev>` (подключиться: `tmux attach -t lumen-<dev>`).
    Иначе (нет ни того, ни другого) — фоновый процесс в текущем терминале, вывод
    нескольких разработчиков перемешается.
    """
    title = f"Lumen {dev}"
    if os.name == "nt":
        subprocess.Popen(f'start "{title}" cmd /k {cmd}', shell=True, cwd=PROJECT_DIR)
        print(f"Запущен {dev} в отдельном окне.")
        return

    argv = _linux_terminal_argv(title, cmd)
    if argv is not None:
        # start_new_session — окно живёт независимо от процесса оркестратора.
        subprocess.Popen(argv, cwd=PROJECT_DIR, start_new_session=True)
        print(f"Запущен {dev} в отдельном окне терминала ({Path(argv[0]).name}).")
        return

    import shutil

    tmux = shutil.which("tmux")
    if tmux:
        session = f"lumen-{dev}"
        subprocess.Popen(
            [tmux, "new-session", "-d", "-s", session, cmd],
            cwd=PROJECT_DIR, start_new_session=True,
        )
        print(f"Запущен {dev} в tmux-сессии '{session}' "
              f"(подключение: tmux attach -t {session}).")
        return

    subprocess.Popen(
        ["bash", "-c", f"{cmd}; exec bash"], cwd=PROJECT_DIR, start_new_session=True,
    )
    print(f"Запущен {dev} в фоне (нет граф. сессии и tmux — вывод "
          f"нескольких разработчиков перемешается; для изоляции установите tmux).")


def dispatch_run(
    developers: list[str],
    max_tasks: int,
    fallback_preset: str | None,
    force_new: bool,
    initial_model: str | None,
    coders_mode: str | None,
    prep_model: str | None = None,
    review_model: str | None = None,
) -> None:
    """Запустить разработчиков: одного — в текущем окне, нескольких — по окнам.

    Модели уже развёрнуты в полный ID. Общая точка входа для CLI (`main`) и
    интерактивного мастера (`run_wizard`).
    """
    if coders_mode and len(developers) > 1:
        # Троттлинг free-tier Kilo общий на ключ: параллельные окна душат
        # друг друга 429 (см. память reference_kilo_free_models_bench).
        print("ВНИМАНИЕ: несколько разработчиков в режиме кодеров делят один "
              "KILO_API_KEY — параллельные запросы троттлятся (429). "
              "Рекомендуется запускать по одному.")
    if len(developers) == 1:
        run_task_loop(
            developers[0], max_tasks, fallback_preset, force_new, initial_model,
            coders_mode, prep_model, review_model,
        )
        return

    # Несколько — каждый в отдельном окне консоли. В дочерние окна передаём
    # уже развёрнутые полные ID — повторного резолва не нужно.
    script = Path(__file__).resolve()
    max_arg = f" --max-tasks {max_tasks}" if max_tasks > 0 else ""
    fb_arg = f" --fallback-model {fallback_preset}" if fallback_preset else ""
    new_arg = " --new" if force_new else ""
    model_arg = f" --model {initial_model}" if initial_model else ""
    coders_arg = f" --coders {coders_mode}" if coders_mode else ""
    prep_arg = f" --prep-model {prep_model}" if prep_model else ""
    review_arg = f" --review-model {review_model}" if review_model else ""

    for dev in developers:
        cmd = (f'python "{script}" {dev}{max_arg}{fb_arg}{new_arg}'
               f'{model_arg}{coders_arg}{prep_arg}{review_arg}')
        _spawn_dev_window(dev, cmd)

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


def _wiz_equiv_cmd(developers, max_tasks, fallback_preset, force_new, initial_model,
                   coders_mode, prep_model=None, review_model=None) -> str:
    """Эквивалентная CLI-команда для показа в сводке (учит флагам)."""
    parts = [f"python scripts/orchestrator.py {' '.join(developers)}"]
    if max_tasks > 0:
        parts.append(f"--max-tasks {max_tasks}")
    if initial_model:
        parts.append(f"--model {initial_model}")
    if fallback_preset:
        parts.append(f"--fallback-model {fallback_preset}")
    if coders_mode:
        parts.append(f"--coders {coders_mode}")
    if prep_model:
        parts.append(f"--prep-model {prep_model}")
    if review_model:
        parts.append(f"--review-model {review_model}")
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
            ("Кодеры assist", "Claude-водитель, код пишет Step 3.7 Flash"),
            ("Кодеры solo", "без Claude: step37/nemotron чередуются, приёмка nano-omni"),
            ("Кодеры team", "цикл: Claude-брифы -> кодеры -> Claude-ревью и merge"),
        ], default=1)
        coders_mode = {1: None, 2: "assist", 3: "solo", 4: "team"}[mode_idx]
        if coders_mode:
            print("  (нужен KILO_API_KEY в env/.tmp/kilo.env)")
            if coders_mode == "solo":
                print("  ВНИМАНИЕ: solo шлёт исходники и скриншоты на trial-эндпоинты Kilo/NVIDIA.")

        # Модель и резерв — только для режимов с Claude (solo их игнорирует).
        initial_model: str | None = None
        fallback_preset: str | None = None
        prep_model: str | None = None
        review_model: str | None = None
        if coders_mode != "solo":
            initial_model = _wiz_model("Стартовая модель Claude:", allow_default=True)
            if _wiz_yes_no("Задать резервную модель при rate limit?", default=False):
                fallback_preset = _wiz_model("Резервная модель:", allow_default=False)
        if coders_mode == "team":
            print(f"  Бригадир (брифы) по умолчанию: {DEFAULT_TEAM_PREP_MODEL}")
            if _wiz_yes_no("Переопределить модель бригадира?", default=False):
                prep_model = _wiz_model("Модель бригадира (фаза 1):", allow_default=False)
            print("  Ревьюер по умолчанию: стартовая модель / дефолт CLI (слабые модели не рекомендуются).")
            if _wiz_yes_no("Задать отдельную модель ревьюера?", default=False):
                review_model = _wiz_model("Модель ревьюера (фаза 3):", allow_default=False)

        # Лимит задач.
        recommend_one = "P5" in developers or coders_mode in ("solo", "team")
        if "P5" in developers:
            print("\nP5 — ревизия рекуррентна, без лимита крутится бесконечно. Рекомендуется 1.")
        elif coders_mode == "solo":
            print("\nsolo — финиш по правилам делается вручную. Рекомендуется 1.")
        elif coders_mode == "team":
            print("\nteam — лимит считает ЦИКЛЫ (1 цикл = до 2 задач + ревью). Рекомендуется 1.")
        else:
            print("\nЛимит задач на разработчика (0 = без лимита).")
        max_tasks = _wiz_int("Макс. задач", 1 if recommend_one else 0)

        force_new = _wiz_yes_no(
            "Старт с чистого листа (--new, не возобновлять прерванную сессию)?", default=False
        )

        # Сводка + эквивалентная команда.
        cmd = _wiz_equiv_cmd(developers, max_tasks, fallback_preset, force_new,
                             initial_model, coders_mode, prep_model, review_model)
        print()
        print("-" * 60)
        print(f"  Разработчики : {' '.join(developers)}")
        print(f"  Режим        : {coders_mode or 'Claude'}")
        if coders_mode != "solo":
            print(f"  Модель       : {initial_model or '(дефолт CLI)'}")
            print(f"  Резерв       : {fallback_preset or '(нет)'}")
        if coders_mode == "team":
            print(f"  Бригадир     : {prep_model or DEFAULT_TEAM_PREP_MODEL}")
            print(f"  Ревьюер      : {review_model or initial_model or '(дефолт CLI)'}")
        print(f"  Лимит задач  : {max_tasks or 'без лимита'}")
        print(f"  --new        : {'да' if force_new else 'нет'}")
        print(f"  Эквивалент   : {cmd}")
        print("-" * 60)
        if not _wiz_yes_no("Запустить?", default=True):
            print("Отменено.")
            return

        dispatch_run(developers, max_tasks, fallback_preset, force_new, initial_model,
                     coders_mode, prep_model, review_model)
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
        "--coders",
        type=str,
        default=None,
        choices=["assist", "solo", "team"],
        help=(
            "Делегировать задачи кодерам Kilo Gateway. "
            "assist — Claude остаётся водителем, но код пишет Step 3.7 Flash "
            "через .tmp/kilo_client.py; solo — оркестратор сам ведёт агент-петлю "
            "поверх кодеров без Claude (Step 3.7 Flash и Nemotron 3 Ultra "
            "чередуются по задачам, визуальная приёмка — nano-omni); "
            "team — полный цикл: Claude готовит брифы двум кодерам, кодеры "
            "пишут, Claude ревьюит и вливает одобренное в main (--max-tasks "
            "считает циклы, 1 цикл = до 2 задач). Исходники и скриншоты уходят "
            "на trial-эндпоинты. Требует KILO_API_KEY (env или .tmp/kilo.env)."
        ),
    )
    parser.add_argument(
        "--prep-model",
        type=str,
        default=None,
        metavar="MODEL",
        help=(
            "Только для --coders team: модель Claude-бригадира (фаза 1, брифы). "
            f"Алиасы: {alias_help}. По умолчанию fable "
            "(качество брифов решает успех слабых кодеров)."
        ),
    )
    parser.add_argument(
        "--review-model",
        type=str,
        default=None,
        metavar="MODEL",
        help=(
            "Только для --coders team: модель Claude-ревьюера (фаза 3, ревью и merge). "
            f"Алиасы: {alias_help}. По умолчанию --model / дефолт CLI. "
            "Слабые модели (haiku) не рекомендуются — ревью самый ответственный этап."
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

    # `choices` намеренно не задан на позиционном `developers` (nargs="*"):
    # argparse в связке nargs="*" + choices ломается с "invalid choice: []",
    # если позиционных аргументов ноль, а есть любой другой флаг (--stop,
    # --status, --stop-all). Валидируем значения вручную.
    valid_devs = {"P1", "P2", "P3", "P4", "P5"}
    invalid = [d for d in args.developers if d not in valid_devs]
    if invalid:
        parser.error(
            f"argument DEV: invalid choice: {invalid[0]!r} "
            f"(choose from {', '.join(sorted(valid_devs))})"
        )

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
        args.new, initial_model, args.coders,
        resolve_model_alias(args.prep_model), resolve_model_alias(args.review_model),
    )


if __name__ == "__main__":
    main()
