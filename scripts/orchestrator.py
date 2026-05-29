#!/usr/bin/env python3
"""
Оркестратор задач Lumen.

Автоматический запуск сессий Claude Code для разработчиков P1-P5.
Каждая задача — отдельная сессия с чистым контекстом.

Использование:
    python scripts/orchestrator.py P1                                   # один разработчик
    python scripts/orchestrator.py P1 P2                                # два в параллель
    python scripts/orchestrator.py P1 P2 P3 P4 P5                       # все пятеро
    python scripts/orchestrator.py P1 --max-tasks 3                     # лимит задач
    python scripts/orchestrator.py P1 --new                             # стартовать с нуля
    python scripts/orchestrator.py P1 --model haiku                     # сразу на Haiku (alias)
    python scripts/orchestrator.py P1 --fallback-model haiku            # резерв при лимите
    python scripts/orchestrator.py P1 --model sonnet --fallback-model haiku   # стартуем на Sonnet, резерв Haiku
    python scripts/orchestrator.py --stop P1                            # мягкая остановка
    python scripts/orchestrator.py --stop-all                           # остановить всех
    python scripts/orchestrator.py --status                             # статус всех

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

Выбор модели
------------
По умолчанию `claude` запускается без `--model` — CLI берёт настроенную модель
(обычно Sonnet/Opus). Чтобы стартовать сразу на конкретной модели, можно
использовать короткие алиасы или указать полный ID:

    haiku  → claude-haiku-4-5
    sonnet → claude-sonnet-4-6
    opus   → claude-opus-4-8

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
    4) Ввести имя модели вручную (alias или полное claude-*)

Для unattended-запуска (без интерактивного ввода) задайте:
- CLI: `--fallback-model haiku`
- env: `LUMEN_FALLBACK_MODEL=haiku`

Если и резервная модель упирается в лимит — срабатывает стандартная пауза
5 минут (`wait_for_rate_limit`).

Сбросить fallback можно только перезапуском оркестратора.
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
        r"^##\s+" + re.escape(heading) + r"\s*\n(.*?)(?=\n---|\n##|\Z)",
        content,
        re.DOTALL | re.MULTILINE,
    )
    return m.group(1).strip() if m else ""


def has_tasks(developer: str) -> bool:
    """Проверить, есть ли задачи в STATUS-файле."""
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        log(developer, f"STATUS-файл не найден: {status_file}")
        return False

    content = status_file.read_text(encoding="utf-8")

    # Секция "In progress": непустая и не является заглушкой _(none)_
    in_progress = _extract_section(content, "In progress")
    if in_progress and in_progress != "_(none)_":
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
    """Дописать session_id в уже существующий файл состояния."""
    path = session_state_path(developer)
    if not path.exists():
        return
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
        if "session_id" not in state:
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


def format_event(event: dict) -> list[str]:
    """Превратить JSON-событие stream-json в читаемые строки."""
    lines = []
    ev_type = event.get("type", "")

    # Финальный результат
    if ev_type == "result":
        result_text = event.get("result", "")
        if result_text:
            preview = result_text[:500].replace("\n", " ")
            if len(result_text) > 500:
                preview += "..."
            lines.append(f"  Результат: {preview}")
        return lines

    # Сообщение ассистента — содержит content[] с text и tool_use
    if ev_type == "assistant":
        msg = event.get("message", {})
        for block in msg.get("content", []):
            btype = block.get("type", "")
            if btype == "tool_use":
                lines.append(format_tool_use(block))
            elif btype == "text":
                text = block.get("text", "")
                if text:
                    preview = text[:500].replace("\n", " ")
                    if len(text) > 500:
                        preview += "..."
                    lines.append(f"  {preview}")
        return lines

    return lines


RATE_LIMIT_RE = re.compile(r"resets?\s+(\d{1,2}:\d{2}(?:am|pm)?)", re.IGNORECASE)

# Короткие алиасы. Принимаются в CLI (`--model`, `--fallback-model`),
# env-переменных и в интерактивном prompt. Разворачиваются в полный
# model ID, который и идёт в `claude --model <id>` — в логах видно
# именно полный ID, чтобы пользователь понимал, что запустилось.
MODEL_ALIASES: dict[str, str] = {
    "haiku":  "claude-haiku-4-5",
    "sonnet": "claude-sonnet-4-6",
    "opus":   "claude-opus-4-8",
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
) -> tuple[int, bool, bool]:
    """Запустить claude и показать прогресс. Возвращает (exit_code, rate_limited, auth_error).

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

    def _tracker() -> None:
        while not _stop_tracker.wait(timeout=2.0):
            _seen.update(_snapshot_descendants(process.pid))

    tracker = threading.Thread(target=_tracker, daemon=True)
    tracker.start()

    try:
        rate_limited = False
        auth_error = False
        _session_id_saved = resume_session_id is not None  # уже знаем id при resume
        for line in process.stdout:
            line = line.strip()
            if not line:
                continue

            # Детект rate limit в сыром выводе
            if "hit your limit" in line.lower() or "rate limit" in line.lower():
                rate_limited = True
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
                if "hit your limit" in line.lower() or "rate limit" in line.lower():
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

            for display_line in format_event(event):
                log(developer, display_line)

        # Проверить stderr тоже — rate limit / auth error может быть там
        stderr_output = process.stderr.read()
        if stderr_output:
            sl = stderr_output.lower()
            if "hit your limit" in sl or "rate limit" in sl:
                rate_limited = True
                match = RATE_LIMIT_RE.search(stderr_output)
                if match:
                    log(developer, f"  Rate limit до {match.group(1)}")
                else:
                    log(developer, "  Rate limit обнаружен")
            elif "403" in stderr_output and ("forbidden" in sl or "authenticate" in sl):
                auth_error = True
                log(developer, "  Auth error (403) в stderr")

        process.wait()
        return process.returncode, rate_limited, auth_error
    finally:
        _stop_tracker.set()
        tracker.join(timeout=3.0)

        with _process_lock:
            if _active_process is process:
                _active_process = None

        killed = _kill_pids(_seen)
        if killed > 0:
            log(developer, f"  Завершено {killed} дочерних процессов после сессии")


def wait_for_rate_limit(developer: str):
    """Подождать 5 минут при rate limit."""
    wait_minutes = 5
    log(developer, f"Rate limit — пауза {wait_minutes} мин...")
    set_jobstatus(developer, "rate limit",
                  f"ждёт до {(datetime.now() + timedelta(minutes=wait_minutes)).strftime('%H:%M')}")
    time.sleep(wait_minutes * 60)
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


def run_task_loop(
    developer: str,
    max_tasks: int = 0,
    fallback_preset: str | None = None,
    force_new: bool = False,
    initial_model: str | None = None,
):
    """Цикл задач для одного разработчика.

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

    # --- Принудительный старт с чистого листа ---
    if force_new:
        if session_state_path(developer).exists():
            log(developer, "Флаг --new: удаляю сохранённое состояние сессии, не возобновляю.")
            clear_session_state(developer)
        else:
            log(developer, "Флаг --new: сохранённого состояния нет, стартую с нуля.")

    # --- Восстановление после краша ---
    existing = load_session_state(developer)
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

            resume_prompt = (
                f"Сессия разработчика {developer} была прервана (crash/закрытие терминала/отключение питания). "
                f"Выполни: git status, затем прочитай STATUS-{developer}.md. "
                f"На основе истории диалога выше и текущего состояния git определи, "
                f"что уже сделано, и продолжи задачу с места остановки. "
                f"Когда задача завершена — вызови /lumen-task-finish."
            )
            try:
                exit_code, rate_limited, auth_error = run_claude(
                    developer, resume_prompt, task_count,
                    resume_session_id=session_id,
                    model=fallback_model or initial_model,
                )
            except FileNotFoundError:
                log(developer, "claude не найден в PATH.")
                clear_session_state(developer)
                return
            except Exception as e:
                log(developer, f"Ошибка запуска: {e}")
                clear_session_state(developer)
                return

            if exit_code == 0:
                clear_session_state(developer)
                log(developer, f"Задача #{task_count} (возобновлённая) завершена.")
            elif rate_limited:
                task_count -= 1
                if fallback_model is None:
                    fallback_model = resolve_fallback_model(developer, fallback_preset)
                    announce_fallback(developer, "лимит во время возобновления сессии", fallback_model)
                    set_jobstatus(developer, "fallback model", fallback_model)
                else:
                    log(developer, f"Резервная модель {fallback_model} тоже исчерпана.")
                    # Оставить файл состояния — попробуем снова после паузы
                    wait_for_rate_limit(developer)
            elif auth_error:
                log(developer, "Auth error при возобновлении. Пауза 60 сек...")
                time.sleep(60)
                task_count -= 1
            else:
                log(developer, f"Возобновление не удалось (код {exit_code}). Продолжаю обычным режимом.")
                clear_session_state(developer)
                task_count -= 1
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

        # Записать состояние ДО запуска — чтобы не потерять при краше
        save_session_state(developer, task_count)

        prompt = (
            f"Ты разработчик {developer}. "
            f"Прочитай STATUS-{developer}.md. "
            f"Если есть 'In progress' — продолжи эту задачу. "
            f"Если нет — возьми первую задачу из 'Next'. "
            f"Когда задача завершена — вызови /lumen-task-finish."
        )

        log(developer, "Запуск claude...")
        try:
            exit_code, rate_limited, auth_error = run_claude(
                developer, prompt, task_number=task_count,
                model=fallback_model or initial_model,
            )
        except FileNotFoundError:
            log(developer, "claude не найден в PATH.")
            clear_session_state(developer)
            break
        except Exception as e:
            log(developer, f"Ошибка запуска: {e}")
            clear_session_state(developer)
            break

        if rate_limited and exit_code != 0:
            task_count -= 1  # Не считать неудачную попытку как задачу
            if fallback_model is None:
                # Первый rate limit — спрашиваем модель и повторяем без паузы
                fallback_model = resolve_fallback_model(developer, fallback_preset)
                announce_fallback(developer, f"задача #{task_count + 1}", fallback_model)
                set_jobstatus(developer, "fallback model", fallback_model)
            else:
                # И резервная модель уже исчерпана — стандартная пауза 5 минут
                log(developer, f"Резервная модель {fallback_model} тоже исчерпана.")
                # Оставить файл состояния с session_id — пригодится при возобновлении после паузы
                wait_for_rate_limit(developer)
        elif auth_error and exit_code != 0:
            task_count -= 1
            clear_session_state(developer)
            log(developer, "Auth error (403). Пауза 60 сек перед повтором...")
            time.sleep(60)
        elif exit_code != 0:
            task_count -= 1  # Не считать ошибку как задачу
            clear_session_state(developer)
            log(developer, f"Claude завершился с кодом {exit_code}.")
            log(developer, "Пауза 30 секунд перед повтором...")
            time.sleep(30)
        else:
            clear_session_state(developer)
            log(developer, f"Задача #{task_count} завершена.")

    set_jobstatus(developer, "остановлен", f"выполнено задач: {task_count}")
    log(developer, f"Цикл завершён. Выполнено задач: {task_count}.")


def create_stop_file(developers: list[str]):
    """Создать стоп-файлы."""
    for dev in developers:
        sf = stop_file_path(dev)
        sf.touch()
        print(f"{dev} будет остановлен после текущей задачи. ({sf})")


def main():
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

    developers = args.developers

    # Приоритет: CLI > env > None. Алиасы (opus/sonnet/haiku) сразу
    # разворачиваются в полный model ID — дальше по коду уже только full ID.
    initial_model = resolve_model_alias(args.model) or resolve_model_alias(
        os.environ.get(INITIAL_MODEL_ENV)
    )
    fallback_model_preset = resolve_model_alias(args.fallback_model)

    if len(developers) == 1:
        # Один разработчик — в текущем окне
        run_task_loop(
            developers[0], args.max_tasks, fallback_model_preset, args.new, initial_model,
        )
    else:
        # Несколько — каждый в отдельном окне консоли.
        # В дочерние окна передаём уже развёрнутые полные ID — повторного резолва не нужно.
        script = Path(__file__).resolve()
        max_arg = f" --max-tasks {args.max_tasks}" if args.max_tasks > 0 else ""
        fb_arg = f" --fallback-model {fallback_model_preset}" if fallback_model_preset else ""
        new_arg = " --new" if args.new else ""
        model_arg = f" --model {initial_model}" if initial_model else ""

        for dev in developers:
            cmd = f'python "{script}" {dev}{max_arg}{fb_arg}{new_arg}{model_arg}'
            title = f"Lumen {dev}"

            if os.name == "nt":
                # Windows: start открывает новое окно с заголовком
                subprocess.Popen(
                    f'start "{title}" cmd /k {cmd}',
                    shell=True,
                    cwd=PROJECT_DIR,
                )
            else:
                # Linux/macOS fallback
                subprocess.Popen(
                    ["bash", "-c", f"{cmd}; exec bash"],
                    cwd=PROJECT_DIR,
                )

            print(f"Запущен {dev} в отдельном окне.")

        print()
        print("Для остановки: python scripts/orchestrator.py --stop P1")
        print("Остановить всех: python scripts/orchestrator.py --stop-all")


if __name__ == "__main__":
    main()
