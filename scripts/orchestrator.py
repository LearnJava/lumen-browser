#!/usr/bin/env python3
"""
Оркестратор задач Lumen.

Автоматический запуск сессий Claude Code для разработчиков P1-P4.
Каждая задача — отдельная сессия с чистым контекстом.

Использование:
    python scripts/orchestrator.py P1              # один разработчик
    python scripts/orchestrator.py P1 P2           # два в параллель
    python scripts/orchestrator.py P1 P2 P3 P4    # все четверо
    python scripts/orchestrator.py P1 --max-tasks 3
    python scripts/orchestrator.py --stop P1       # мягкая остановка
    python scripts/orchestrator.py --stop-all      # остановить всех
    python scripts/orchestrator.py --status        # статус всех
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


def has_tasks(developer: str) -> bool:
    """Проверить, есть ли задачи в STATUS-файле."""
    status_file = PROJECT_DIR / f"STATUS-{developer}.md"
    if not status_file.exists():
        log(developer, f"STATUS-файл не найден: {status_file}")
        return False

    content = status_file.read_text(encoding="utf-8")
    if re.search(r"In progress:", content):
        return True
    if re.search(r"Next:", content) and re.search(r"- \[", content):
        return True
    return False


def stop_file_path(developer: str) -> Path:
    return SCRIPTS_DIR / f".stop-{developer}"


def jobstatus_path(developer: str) -> Path:
    return SCRIPTS_DIR / f".jobstatus-{developer}"


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
    for dev in ["P1", "P2", "P3", "P4"]:
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
        preview = cmd[:120].replace("\n", " ")
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
            preview = result_text[:200].replace("\n", " ")
            if len(result_text) > 200:
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
                    preview = text[:200].replace("\n", " ")
                    if len(text) > 200:
                        preview += "..."
                    lines.append(f"  {preview}")
        return lines

    return lines


RATE_LIMIT_RE = re.compile(r"resets?\s+(\d{1,2}:\d{2}(?:am|pm)?)", re.IGNORECASE)


def run_claude(developer: str, prompt: str) -> tuple[int, bool]:
    """Запустить claude и показать прогресс. Возвращает (exit_code, rate_limited)."""
    global _active_process

    process = subprocess.Popen(
        [
            "claude", "-p", prompt,
            "--dangerously-skip-permissions",
            "--verbose",
            "--output-format", "stream-json",
        ],
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
        for line in process.stdout:
            line = line.strip()
            if not line:
                continue

            # Детект rate limit в сыром выводе
            if "hit your limit" in line.lower() or "rate limit" in line.lower():
                rate_limited = True
                log(developer, f"  Rate limit: {line[:120]}")
                continue

            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                # Не-JSON строка — может быть сообщение от CLI
                if "hit your limit" in line.lower() or "rate limit" in line.lower():
                    rate_limited = True
                    log(developer, f"  Rate limit: {line[:120]}")
                continue

            # Детект rate limit в JSON-событии
            if event.get("type") == "rate_limit_event":
                info = event.get("rate_limit_info", {})
                if info.get("status", "").startswith("blocked"):
                    rate_limited = True
                    log(developer, "  Rate limit (blocked)")

            for display_line in format_event(event):
                log(developer, display_line)

        # Проверить stderr тоже — rate limit может быть там
        stderr_output = process.stderr.read()
        if stderr_output and ("hit your limit" in stderr_output.lower()
                              or "rate limit" in stderr_output.lower()):
            rate_limited = True
            # Попробовать извлечь время сброса
            match = RATE_LIMIT_RE.search(stderr_output)
            if match:
                log(developer, f"  Rate limit до {match.group(1)}")
            else:
                log(developer, f"  Rate limit обнаружен")

        process.wait()
        return process.returncode, rate_limited
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


def run_task_loop(developer: str, max_tasks: int = 0):
    """Цикл задач для одного разработчика."""
    stop_file = stop_file_path(developer)
    task_count = 0

    log(developer, f"Старт. Проект: {PROJECT_DIR}")
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

        prompt = (
            f"Ты разработчик {developer}. "
            f"Прочитай STATUS-{developer}.md. "
            f"Если есть 'In progress' — продолжи эту задачу. "
            f"Если нет — возьми первую задачу из 'Next'. "
            f"Когда задача завершена — вызови /lumen-task-finish."
        )

        log(developer, "Запуск claude...")
        try:
            exit_code, rate_limited = run_claude(developer, prompt)
        except FileNotFoundError:
            log(developer, "claude не найден в PATH.")
            break
        except Exception as e:
            log(developer, f"Ошибка запуска: {e}")
            break

        if rate_limited and exit_code != 0:
            task_count -= 1  # Не считать неудачную попытку как задачу
            wait_for_rate_limit(developer)
        elif exit_code != 0:
            task_count -= 1  # Не считать ошибку как задачу
            log(developer, f"Claude завершился с кодом {exit_code}.")
            log(developer, "Пауза 30 секунд перед повтором...")
            time.sleep(30)
        else:
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
        choices=["P1", "P2", "P3", "P4"],
        metavar="DEV",
        help="Разработчики для запуска: P1 P2 P3 P4",
    )
    parser.add_argument(
        "--max-tasks",
        type=int,
        default=0,
        help="Максимум задач на разработчика (0 = без ограничения)",
    )
    parser.add_argument(
        "--stop",
        nargs="+",
        choices=["P1", "P2", "P3", "P4"],
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
        create_stop_file(["P1", "P2", "P3", "P4"])
        return

    if args.stop:
        create_stop_file(args.stop)
        return

    # Режим запуска
    if not args.developers:
        parser.print_help()
        sys.exit(1)

    developers = args.developers

    if len(developers) == 1:
        # Один разработчик — в текущем окне
        run_task_loop(developers[0], args.max_tasks)
    else:
        # Несколько — каждый в отдельном окне консоли
        script = Path(__file__).resolve()
        max_arg = f" --max-tasks {args.max_tasks}" if args.max_tasks > 0 else ""

        for dev in developers:
            cmd = f'python "{script}" {dev}{max_arg}'
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
        print(f"Для остановки: python scripts/orchestrator.py --stop P1")
        print(f"Остановить всех: python scripts/orchestrator.py --stop-all")


if __name__ == "__main__":
    main()
