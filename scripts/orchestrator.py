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
        r"^##\s+" + re.escape(heading) + r"\s*\n(.*?)(?=\n---|\n##(?!#)|\Z)",
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


def task_prompt(developer: str) -> str:
    """Стандартный промпт для старта задачи с чистым диалогом."""
    if developer == "P3":
        return (
            "Ты разработчик P3 (только баг-фиксы). "
            "Прочитай STATUS-P3.md. "
            "Если есть 'In progress' — продолжи ЭТОТ ОДИН баг. "
            "Если нет — возьми ПЕРВЫЙ баг из 'Next' (только один, не больше). "
            "Когда баг исправлен — вызови /lumen-task-finish. "
            "ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
            "Не бери следующий баг. Один баг = одна сессия."
        )
    return (
        f"Ты разработчик {developer}. "
        f"Прочитай STATUS-{developer}.md. "
        f"Если есть 'In progress' — продолжи эту задачу. "
        f"Если нет — возьми первую задачу из 'Next'. "
        f"Когда задача завершена — вызови /lumen-task-finish."
    )


def resume_after_error_prompt(developer: str) -> str:
    """Промпт для возобновления сессии, прерванной ошибкой (rate limit / 403 / сбой CLI)."""
    base = (
        "Сессия была прервана ошибкой (rate limit / auth error / сбой CLI). "
        "Выполни git status, сверься с историей диалога выше и продолжи текущую "
        "задачу с места остановки. Когда задача завершена — вызови /lumen-task-finish."
    )
    if developer == "P3":
        return base + (
            " ВАЖНО: после /lumen-task-finish немедленно заверши сессию. "
            "Не бери следующий баг. Один баг = одна сессия."
        )
    return base


def run_task_loop(
    developer: str,
    max_tasks: int = 0,
    fallback_preset: str | None = None,
    force_new: bool = False,
    initial_model: str | None = None,
):
    """Цикл задач для одного разработчика.

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
                prompt = resume_after_error_prompt(developer)

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
                    prompt = task_prompt(developer)
                    generic_failures = 0
                log(developer, "Пауза 30 секунд перед повтором...")
                time.sleep(30)

            if resume_id:
                log(developer, f"Возобновляю сессию через --resume {resume_id[:16]}...")
            set_jobstatus(developer, "работает", f"задача #{task_number} (повтор)")

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

        # Записать состояние ДО запуска — чтобы не потерять при краше
        save_session_state(developer, task_count)

        log(developer, "Запуск claude...")
        if not attempt_task(task_count, task_prompt(developer), None):
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
