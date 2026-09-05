#!/usr/bin/env python3
"""Перф-аудит корпуса реальных сайтов (дорожка PERF, ROADMAP.md, AUDIT-1).

Живой прогон с GUI-окном (`--mcp-live-port`) — три РЕЖИМА (`--mode`), каждый
измеряет своё, не смешивать в одном выводе (BUG-992 — «сегодняшний прогон
это B, а читается как A»):

  --mode stability (B, default) — один процесс на весь корпус: ровно то, что
    ловит накопление RAM, утечки, деградацию и падения между сайтами. Каждый
    сайт — НОВАЯ вкладка (MCP `new_tab`), вкладки не закрываются (намеренно).
    Прогонять партиями по 15-20 сайтов (`--only`) — иначе к сороковому сайту
    таблица меряет усталость процесса, а не сайты.
  --mode compat (A) — свежий процесс lumen НА КАЖДЫЙ сайт: числа сопоставимы
    между сайтами, потому что ни один не наследует RAM/вкладки предыдущего.
    Дороже по времени (запуск процесса на сайт), зато честная кросс-сайтовая
    метрика (старт, готовность, память, JS-ошибки, рендер).
  --mode session (C) — одна сессия: `--session-tabs` вкладок (default 20),
    затем скролл/назад-вперёд/попап (через `eval`: history.back/forward(),
    window.open())/проверка фрейма/приближённое закрытие. Ограничение: MCP
    (`crates/mcp/src/server.rs`) не даёт `switch_tab`/`close_tab` — только
    последняя открытая вкладка управляема, «закрытие» это навигация НА
    about:blank последней вкладки, а не закрытие всех N.

На сайт собираются: время до document_ready, RAM тек/пик, JS-ошибки консоли,
CPU-скриншот (resource://screenshot). Статус — по единой таксономии
(`classify_status`): OK / DEGRADED / BROKEN_RENDER / TIMEOUT / HUNG / DEAD,
плюс внешние SITE_REFUSED (401/403) / NET_FAIL (сеть). BROKEN_RENDER —
подозрение (health-log эвристика ИЛИ повтор кадра в прогоне), точность
ограничена BUG-993 (эвристика и путь снимка расходятся в обе стороны).

Режим --phases — headless-разложение по фазам, точечный разбор ОДНОГО сайта
(не заменяет ни один из трёх режимов выше), три замера одним бинарём:

  1. --dump-source      -> t_source      (сеть + декодирование + парсинг HTML)
  2. --dump-layout      -> t_layout      (+ каскад + layout + JS; LUMEN_PROFILE_TREE=1)
  3. --screenshot       -> t_screenshot  (+ растеризация/paint, CPU-путь)

Производные фазы (приближение — каждая стадия повторяет предыдущие):
  net_parse    = t_source
  style_layout = t_layout - t_source
  paint        = t_screenshot - t_layout

Результат: .tmp/perf-audit/<timestamp>/ (results.json, summary.md, логи,
скриншоты — НЕ коммитятся). Протокол анализа и заведения багов — skill
/lumen-perf-audit (.claude/skills/lumen-perf-audit/SKILL.md).

Примеры:
  python scripts/perf_audit.py                          # режим B (stability), весь корпус
  python scripts/perf_audit.py --mode compat --only lenta,rbc  # режим A, пара сайтов
  python scripts/perf_audit.py --mode session --session-tabs 10
  python scripts/perf_audit.py --phases --only lenta    # headless-разложение по фазам
  python scripts/perf_audit.py --compare docs/perf/runs/2026-07-17.json
  LUMEN_EXE=path/to/lumen.exe python scripts/perf_audit.py
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import socket
import struct
import subprocess
import sys
import threading
import time
from datetime import datetime
from pathlib import Path

# Windows-консоль по умолчанию cp1251 — не переваривает Δ/⚠ в сводке
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = REPO_ROOT / "docs" / "perf" / "corpus.txt"
OUT_ROOT = REPO_ROOT / ".tmp" / "perf-audit"
# Паттерны строк stderr, которые считаем сигналом проблемы (без учёта регистра)
ERROR_RE = re.compile(r"error|panic|failed|не распознан|unsupported", re.IGNORECASE)
# Строки верхнего уровня дерева LUMEN_PROFILE_TREE=1 (без начального отступа)
PROFILE_LINE_RE = re.compile(r"^\S.*\d+(?:\.\d+)?\s*ms", re.MULTILINE)
# Сообщения консоли сетевой природы (ресурс не загрузился) — не JS-ошибки страницы
NET_CONSOLE_RE = re.compile(
    r"failed to load resource|net::|err_[a-z_]+|failed to fetch|blocked by", re.IGNORECASE
)
# Главный документ в сетевом логе печатается «← <статус> <url>», сбои — «✗ <url> (stage: reason)»
LOG_STATUS_RE = re.compile(r"←\s+(\d{3})\s+(\S+)")

_SIG_NORM_RE = re.compile(r"0x[0-9a-f]+|\d+(?:\.\d+)?")


def error_sig(line: str) -> str:
    """Нормализованная сигнатура строки (регистр/цифры/хеши не различаем)."""
    return " ".join(_SIG_NORM_RE.sub("N", line.lower()).split())


def count_sigs(lines: list[str]) -> dict[str, dict]:
    """Сигнатура -> {count, sample}: настоящее число повторений без потери текста."""
    out: dict[str, dict] = {}
    for ln in lines:
        sig = error_sig(ln)
        slot = out.setdefault(sig, {"count": 0, "sample": ln})
        slot["count"] += 1
    return out


def main_http_status(text: str, url: str) -> int | None:
    """HTTP-статус главного документа по сетевому логу (← N url).

    Ищем последний статус именно URL сайта (с учётом http→https и конечного
    слэша); при редиректе на другой хост — по его пути; иначе последний статус.
    """
    pairs = [(int(m.group(1)), m.group(2).rstrip("/")) for m in LOG_STATUS_RE.finditer(text)]
    if not pairs:
        return None
    norm = url.rstrip("/")
    norm_https = norm.replace("http://", "https://", 1)
    for code, target in reversed(pairs):
        if target == norm or target == norm_https:
            return code
    host = norm.split("://", 1)[-1].split("/", 1)[0]
    for code, target in reversed(pairs):
        if target.startswith("http") and host in target:
            return code
    return pairs[-1][0]


def network_failures(text: str, url: str) -> list[str]:
    """Сетевые сбои главного документа: ✗-строки, чей хост совпадает с сайтом.

    Ресурсы с других хостов (CDN, реклама) в расчёт не берём — они не объясняют
    «страница так и не стала готовой».
    """
    ref_host = url.split("://", 1)[-1].split("/", 1)[0].lower()
    out = []
    for ln in text.splitlines():
        ln = ln.strip()
        if not ln.startswith("✗") or "://" not in ln:
            continue
        host = ln.split("://", 1)[-1].split("(", 1)[0].split("/", 1)[0].lower().split(":", 1)[0]
        if host == ref_host and ln not in out:
            out.append(ln)
    return out


def read_health_events(path: Path, pos: int) -> tuple[list[dict], int]:
    """Новые строки `health.log` (JSONL) с позиции pos; битые строки пропускаются."""
    try:
        with path.open("rb") as f:
            f.seek(pos)
            chunk = f.read()
    except OSError:
        return [], pos
    events = []
    for ln in chunk.decode("utf-8", errors="replace").splitlines():
        ln = ln.strip()
        if not ln:
            continue
        try:
            events.append(json.loads(ln))
        except json.JSONDecodeError:
            continue
    return events, pos + len(chunk)


def classify_status(
    *,
    dead: bool,
    hung: bool,
    http_status: int | None,
    net_failure: bool,
    ready: bool,
    broken_render: bool,
    have_png: bool,
    js_error_count: int,
) -> str:
    """Единая таксономия исхода сайта (AUDIT-1) для всех трёх режимов прогона.

    Успех определяется как навигация + готовность + отрисованный контент +
    отсутствие JS-ошибок, а не бинарным document_ready:

    - DEAD/HUNG — окно упало / перестало качать очередь сообщений необратимо.
    - SITE_REFUSED/NET_FAIL — сайт/сеть не пустили (401/403, DNS/TLS/соединение)
      — не отказ движка, вынесены за пределы шестёрки исходов.
    - TIMEOUT — не дошли до document_ready и это не отказ сайта/сети.
    - BROKEN_RENDER — готовность есть, но признаки говорят «ничего не
      нарисовано» (health-log эвристика ИЛИ дублирующийся кадр в прогоне).
      Достоверность этого исхода ограничена BUG-993 (эвристика и путь снимка
      противоречат друг другу в обе стороны) — читать как «подозрение», не факт.
    - DEGRADED — готовность и хоть какой-то кадр есть, но верификация кадра
      недоступна (нет PNG) или страница кричит JS-ошибками в консоль.
    - OK — готовность, кадр, ни одной консольной JS-ошибки.
    """
    if dead:
        return "DEAD"
    if hung:
        return "HUNG"
    if http_status in (401, 403):
        return "SITE_REFUSED"
    if not ready:
        return "NET_FAIL" if net_failure else "TIMEOUT"
    if broken_render:
        return "BROKEN_RENDER"
    if not have_png or js_error_count > 0:
        return "DEGRADED"
    return "OK"


def find_exe(cli_exe: str | None) -> Path:
    """Найти lumen.exe: --exe > $LUMEN_EXE > target/{dev-release,release,debug}."""
    candidates = []
    if cli_exe:
        candidates.append(Path(cli_exe))
    if os.environ.get("LUMEN_EXE"):
        candidates.append(Path(os.environ["LUMEN_EXE"]))
    # target/ обычно живёт в корневом клоне, не в worktree — проверяем оба
    for root in (REPO_ROOT, REPO_ROOT.parent.parent.parent):
        for profile in ("dev-release", "release", "debug"):
            candidates.append(root / "target" / profile / "lumen.exe")
    for c in candidates:
        if c.is_file():
            return c
    sys.exit(
        "lumen.exe не найден. Соберите: cargo build -p lumen-shell --profile dev-release\n"
        "или укажите путь через --exe / $LUMEN_EXE.\nПроверены: "
        + ", ".join(str(c) for c in candidates)
    )


def load_corpus(path: Path, only: list[str]) -> list[tuple[str, str]]:
    """Прочитать корпус (строки `slug url`), отфильтровать по --only подстрокам."""
    sites = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        slug, url = line.split(None, 1)
        if only and not any(o in slug for o in only):
            continue
        sites.append((slug, url.strip()))
    if not sites:
        sys.exit(f"Корпус пуст (файл {path}, фильтр --only {only})")
    return sites


def _win_proc_stats(popen: subprocess.Popen) -> dict:
    """Пиковая рабочая память и CPU-время завершившегося процесса (WinAPI, без зависимостей).

    Работает, пока жив handle Popen (до GC объекта). На не-Windows возвращает {}.
    """
    if sys.platform != "win32":
        return {}
    import ctypes
    from ctypes import wintypes

    class PMC(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    stats: dict = {}
    try:
        handle = wintypes.HANDLE(int(popen._handle))  # noqa: SLF001 — публичного API у Popen нет
        pmc = PMC()
        pmc.cb = ctypes.sizeof(pmc)
        if ctypes.WinDLL("psapi").GetProcessMemoryInfo(handle, ctypes.byref(pmc), pmc.cb):
            stats["peak_mb"] = round(pmc.PeakWorkingSetSize / 1048576, 1)
            stats["cur_mb"] = round(pmc.WorkingSetSize / 1048576, 1)  # для живого процесса
        times = (wintypes.FILETIME * 4)()
        if ctypes.WinDLL("kernel32").GetProcessTimes(
            handle, ctypes.byref(times[0]), ctypes.byref(times[1]),
            ctypes.byref(times[2]), ctypes.byref(times[3]),
        ):
            def ft_s(ft: wintypes.FILETIME) -> float:
                return ((ft.dwHighDateTime << 32) | ft.dwLowDateTime) / 1e7
            stats["cpu_s"] = round(ft_s(times[2]) + ft_s(times[3]), 2)  # kernel + user
    except (OSError, AttributeError, ValueError):
        pass
    return stats


def run_stage(
    exe: Path, args: list[str], log_path: Path, timeout: int, extra_env: dict | None = None
) -> dict:
    """Запустить один headless-прогон lumen; вернуть тайминг + RAM/CPU + диагностику."""
    env = os.environ.copy()
    env.update(extra_env or {})
    t0 = time.monotonic()
    timed_out = False
    proc = subprocess.Popen(
        [str(exe), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        cwd=str(REPO_ROOT),
    )
    try:
        stdout, stderr = proc.communicate(timeout=timeout)
        rc = proc.returncode
    except subprocess.TimeoutExpired:
        timed_out = True
        rc = None
        proc.kill()
        stdout, stderr = proc.communicate()
    wall = round(time.monotonic() - t0, 2)
    proc_stats = _win_proc_stats(proc)

    stderr_text = stderr.decode("utf-8", errors="replace")
    log_path.write_bytes(stderr)
    error_lines = []
    for ln in stderr_text.splitlines():
        if ERROR_RE.search(ln) and ln.strip() not in error_lines:
            error_lines.append(ln.strip())
    return {
        "wall_s": wall,
        "rc": rc,
        "timed_out": timed_out,
        "stdout_bytes": len(stdout),
        "error_lines": error_lines,  # полный список, без среза (BUG-992)
        "error_sigs": count_sigs(error_lines),
        "stderr_text": stderr_text,
        **proc_stats,  # peak_mb / cpu_s (Windows)
    }


def png_size(path: Path) -> tuple[int, int] | None:
    """Ширина/высота PNG из IHDR (без зависимостей)."""
    try:
        with path.open("rb") as f:
            head = f.read(24)
        if len(head) == 24 and head[:8] == b"\x89PNG\r\n\x1a\n":
            w, h = struct.unpack(">II", head[16:24])
            return w, h
    except OSError:
        pass
    return None


def audit_site(exe: Path, slug: str, url: str, out_dir: Path, timeout: int) -> dict:
    """Три замера одного сайта; вернуть запись results.json."""
    rec: dict = {"slug": slug, "url": url}

    keys = ("wall_s", "rc", "timed_out", "stdout_bytes", "error_lines", "error_sigs", "peak_mb", "cpu_s")

    src = run_stage(exe, ["--dump-source", url], out_dir / f"{slug}.source.stderr.log", timeout)
    rec["source"] = {k: src[k] for k in keys if k in src}
    # HTTP-статус главного документа из сетевого лога («← 403 https://…»)
    statuses = re.findall(r"←\s*(\d{3})\s", src["stderr_text"])
    rec["http_status"] = int(statuses[-1]) if statuses else None

    lay = run_stage(
        exe,
        ["--dump-layout", url],
        out_dir / f"{slug}.layout.stderr.log",
        timeout,
        extra_env={"LUMEN_PROFILE_TREE": "1"},
    )
    rec["layout"] = {k: lay[k] for k in keys if k in lay}
    # Топ-строки профиля каскада/layout — подсказка «куда смотреть», не точная разбивка
    rec["layout"]["profile_top"] = PROFILE_LINE_RE.findall(lay["stderr_text"])[:12]

    png = out_dir / f"{slug}.png"
    shot = run_stage(exe, ["--screenshot", str(png), url], out_dir / f"{slug}.screenshot.stderr.log", timeout)
    rec["screenshot"] = {k: shot[k] for k in keys if k in shot}
    rec["screenshot"]["png_size"] = png_size(png)

    # Производные фазы (валидны только когда все стадии завершились сами)
    if not (src["timed_out"] or lay["timed_out"] or shot["timed_out"]):
        rec["phases"] = {
            "net_parse_s": src["wall_s"],
            "style_layout_s": round(max(0.0, lay["wall_s"] - src["wall_s"]), 2),
            "paint_s": round(max(0.0, shot["wall_s"] - lay["wall_s"]), 2),
        }
    ok = (
        not shot["timed_out"]
        and shot["rc"] == 0
        and rec["screenshot"]["png_size"] is not None
    )
    rec["status"] = "OK" if ok else ("TIMEOUT" if shot["timed_out"] else "FAIL")
    # «Сайт не пустил» (401/403) и сетевые сбои — отдельные исходы, не отказы движка
    if rec["http_status"] in (401, 403):
        rec["status"] = "SITE_REFUSED"
    elif rec["status"] == "TIMEOUT" and network_failures(
        src["stderr_text"] + lay["stderr_text"], url
    ):
        rec["status"] = "NET_FAIL"
    return rec


# ── Живой режим (GUI-окно, один процесс на весь корпус) ──────────────────────

def _free_port() -> int:
    """Свободный локальный TCP-порт (bind-and-release)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def _wait_for_mcp_token(stderr_log: Path, timeout_s: float = 20.0) -> str:
    """Poll `stderr_log` for the ADR-024 §Access model (DEVX-15) token line.

    The child's stderr is redirected straight to `stderr_log` (see
    `LiveBrowser._spawn`), so polling the file is enough.
    """
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            with open(stderr_log, encoding="utf-8", errors="replace") as fh:
                for line in fh:
                    if line.startswith("[mcp] token: "):
                        return line.strip()[len("[mcp] token: "):]
        except OSError:
            pass
        time.sleep(0.1)
    raise RuntimeError(f"lumen --mcp-live-port token not found in {stderr_log}")


class Mcp:
    """Line-delimited JSON-RPC клиент к `--mcp-live-port` (паттерн scripts/scroll_perf.py)."""

    def __init__(self, port: int, timeout: float, stderr_log: Path) -> None:
        last: Exception | None = None
        for _ in range(300):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError as e:
                last = e
                time.sleep(0.1)
        else:
            raise RuntimeError(f"MCP-порт {port} не поднялся: {last}")
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock.settimeout(timeout + 30)  # дольше самого длинного wait
        self._reader = self.sock.makefile("r", encoding="utf-8", newline="\n")
        self._id = 0
        # ADR-024 §Access model (DEVX-15): mandatory first call.
        token = _wait_for_mcp_token(stderr_log)
        self.call("initialize", {"token": token})

    def call(self, method: str, params: dict) -> dict:
        """Один RPC; RuntimeError при error-ответе, OSError при мёртвом сокете."""
        self._id += 1
        req = json.dumps({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params})
        self.sock.sendall((req + "\n").encode("utf-8"))
        line = self._reader.readline()
        if not line:
            raise OSError("MCP-соединение закрыто (окно упало?)")
        resp = json.loads(line)
        if resp.get("error") is not None:
            raise RuntimeError(f"{method}: {resp['error']}")
        return resp.get("result") or {}

    def tool(self, name: str, arguments: dict) -> dict:
        return self.call("tools/call", {"name": name, "arguments": arguments})

    def resource(self, uri: str) -> list[dict]:
        return self.call("resources/read", {"uri": uri}).get("contents") or []


class HungMonitor(threading.Thread):
    """Фоновый опрос IsHungAppWindow (WinAPI): та самая эвристика, по которой
    Windows пишет «Не отвечает» (окно не качает сообщения >5 с). Копит
    per-site сумму и максимальную серию зависания."""

    def __init__(self) -> None:
        super().__init__(daemon=True)
        self._lock = threading.Lock()
        self._pid: int | None = None
        self._hwnd = None
        self._stop = False
        self._total = 0.0
        self._streak = 0.0
        self._max_streak = 0.0
        if sys.platform == "win32":
            self.start()

    def watch_pid(self, pid: int) -> None:
        """Начать следить за окном процесса pid (после спавна/рестарта)."""
        with self._lock:
            self._pid = pid
            self._hwnd = None

    def begin_site(self) -> None:
        """Сбросить счётчики перед очередным сайтом."""
        with self._lock:
            self._total = self._streak = self._max_streak = 0.0

    def site_stats(self) -> dict:
        """Метрики зависания текущего сайта (сумма/максимальная серия, с)."""
        with self._lock:
            if self._total == 0.0:
                return {}
            return {
                "hung_total_s": round(self._total, 1),
                "hung_max_streak_s": round(max(self._max_streak, self._streak), 1),
            }

    def stop(self) -> None:
        self._stop = True

    def _find_hwnd(self, pid: int):
        """Главное видимое top-level окно процесса pid."""
        import ctypes
        from ctypes import wintypes
        user32 = ctypes.WinDLL("user32")
        found = []

        @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        def cb(hwnd, _lp):
            wnd_pid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wnd_pid))
            if wnd_pid.value == pid and user32.IsWindowVisible(hwnd):
                found.append(hwnd)
                return False
            return True

        user32.EnumWindows(cb, 0)
        return found[0] if found else None

    def run(self) -> None:
        import ctypes
        user32 = ctypes.WinDLL("user32")
        step = 0.5
        while not self._stop:
            time.sleep(step)
            with self._lock:
                pid, hwnd = self._pid, self._hwnd
            if pid is None:
                continue
            if hwnd is None:
                hwnd = self._find_hwnd(pid)
                if hwnd is None:
                    continue
                with self._lock:
                    self._hwnd = hwnd
            hung = bool(user32.IsHungAppWindow(hwnd))
            with self._lock:
                if hung:
                    self._total += step
                    self._streak += step
                else:
                    self._max_streak = max(self._max_streak, self._streak)
                    self._streak = 0.0


class RamMonitor(threading.Thread):
    """Фоновый замер рабочего набора и CPU живого процесса (Windows, шаг 1 с).

    Счётчики процесса — накопительные за жизнь: однажды достигнутый пик рабочего
    набора не падает, процессорное время только растёт. Здесь считается честное
    на сайт: peak_mb — максимум мгновенного рабочего набора внутри окна сайта,
    cpu_s — дельта процессорного времени от старта сайта (BUG-992).
    """

    def __init__(self) -> None:
        super().__init__(daemon=True)
        self._lock = threading.Lock()
        self._popen: subprocess.Popen | None = None
        self._stop = False
        self._cur_mb = 0.0
        self._peak_mb = 0.0
        self._cpu_start: float | None = None
        self._cpu_end: float | None = None
        if sys.platform == "win32":
            self.start()

    def watch(self, popen: subprocess.Popen) -> None:
        with self._lock:
            self._popen = popen

    def begin_site(self) -> None:
        """Сбросить per-site счётчики и зафиксировать стартовую точку CPU."""
        with self._lock:
            st = _win_proc_stats(self._popen) if self._popen else {}
            self._cur_mb = st.get("cur_mb", 0.0)
            self._peak_mb = self._cur_mb
            self._cpu_start = st.get("cpu_s")
            self._cpu_end = self._cpu_start

    def site_stats(self) -> dict:
        with self._lock:
            d = {"cur_mb": round(self._cur_mb, 1), "peak_mb": round(max(self._peak_mb, self._cur_mb), 1)}
            if self._cpu_start is not None and self._cpu_end is not None:
                d["cpu_s"] = round(max(0.0, self._cpu_end - self._cpu_start), 2)
            return d

    def stop(self) -> None:
        self._stop = True

    def run(self) -> None:
        step = 1.0
        while not self._stop:
            time.sleep(step)
            with self._lock:
                popen = self._popen
            if popen is None or popen.poll() is not None:
                continue
            st = _win_proc_stats(popen)
            if not st:
                continue
            with self._lock:
                self._cur_mb = st.get("cur_mb", 0.0)
                self._peak_mb = max(self._peak_mb, self._cur_mb)
                cpu = st.get("cpu_s")
                if cpu is not None:
                    if self._cpu_start is None:
                        self._cpu_start = cpu
                    self._cpu_end = cpu


class LiveBrowser:
    """Одно GUI-окно lumen на весь прогон + перезапуск при смерти."""

    def __init__(self, exe: Path, out_dir: Path, timeout: float) -> None:
        self.exe, self.out_dir, self.timeout = exe, out_dir, timeout
        self.restarts = 0
        self.hung = HungMonitor()
        self.ram = RamMonitor()
        self._spawn()

    def _spawn(self) -> None:
        port = _free_port()
        self.log_path = self.out_dir / f"live.stderr.{self.restarts}.log"
        log = self.log_path.open("wb")
        env = os.environ.copy()
        env["LUMEN_HEALTH_LOG"] = "1"
        self.proc = subprocess.Popen(
            [str(self.exe), "--mcp-live-port", str(port), "--maximized", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=log, cwd=str(REPO_ROOT), env=env,
        )
        self.mcp = Mcp(port, self.timeout, self.log_path)
        self.hung.watch_pid(self.proc.pid)
        self.ram.watch(self.proc)
        # BUG-991: the engine names its journal after its own pid and only ever
        # appends to it, so each restart gets a fresh file instead of the next
        # process truncating the previous one's records away — track that path
        # per-process rather than assuming a single fixed `health.log`.
        self.health_log_path = REPO_ROOT / f"health.{self.proc.pid}.log"
        self.health_log_pos = 0

    def health_events_since(self, pos: int) -> tuple[list[dict], int]:
        """Новые записи health.<pid>.log (broken_render/panic/…) с позиции pos."""
        return read_health_events(self.health_log_path, pos)

    def stderr_since(self, pos: int) -> str:
        """Сырой фрагмент stderr с позиции pos: сетевой лог (← status, ✗ сбой), сообщения движка."""
        try:
            with self.log_path.open("rb") as f:
                f.seek(pos)
                return f.read().decode("utf-8", errors="replace")
        except OSError:
            return ""

    def stderr_errors_since(self, pos: int) -> tuple[list[str], dict, int]:
        """Ошибко-подобные строки stderr с позиции pos: полный список без среза,
        сигнатуры с числом повторений и новая позиция (per-site атрибуция)."""
        try:
            with self.log_path.open("rb") as f:
                f.seek(pos)
                chunk = f.read()
        except OSError:
            return [], {}, pos
        text = chunk.decode("utf-8", errors="replace")
        raw = [ln.strip() for ln in text.splitlines() if ERROR_RE.search(ln)]
        unique = list(dict.fromkeys(raw))
        return unique, count_sigs(raw), pos + len(chunk)

    def restart(self) -> None:
        """Убить зависшее/мёртвое окно и поднять новое (сам факт — находка)."""
        self.restarts += 1
        try:
            self.proc.kill()
            self.proc.wait(timeout=10)
        except OSError:
            pass
        self._spawn()

    def close(self) -> None:
        self.hung.stop()
        self.ram.stop()
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            self.proc.kill()


def audit_site_live(
    br: LiveBrowser,
    slug: str,
    url: str,
    out_dir: Path,
    timeout: int,
    dwell: float,
    scroll_ticks: int,
    png_hash_counts: dict[str, int] | None = None,
) -> dict:
    """Один сайт в живом окне: навигация → готовность → скролл → скриншот → консоль → RAM.

    Итоговый `status` — по единой таксономии AUDIT-1 (`classify_status`), а не
    бинарному document_ready.
    """
    rec: dict = {"slug": slug, "url": url, "restarted": False}
    log_pos = br.log_path.stat().st_size if br.log_path.exists() else 0
    health_pos = br.health_log_pos
    ready = False
    net_failure = False
    br.hung.begin_site()
    br.ram.begin_site()
    t0 = time.monotonic()
    try:
        # Новая вкладка на сайт (MCP-инструмент new_tab: open_new_tab +
        # navigate_to в шелле). Фолбэк на navigate в текущей вкладке — только
        # для старых бинарей без new_tab.
        try:
            br.mcp.tool("new_tab", {"url": url})
            rec["own_tab"] = True
        except RuntimeError as e:
            rec["own_tab"] = False
            rec["tab_error"] = str(e)[:120]
            br.mcp.tool("navigate", {"url": url})
        rec["nav_ack_s"] = round(time.monotonic() - t0, 2)
        br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": timeout * 1000})
        rec["ready_s"] = round(time.monotonic() - t0, 2)
        ready = True
    except RuntimeError as e:  # error-ответ (чаще всего таймаут wait)
        rec["error"] = str(e)[:200]
        if "own_tab" not in rec or "command timed out" in rec.get("tab_error", "") + str(e):
            # Даже new_tab/navigate не прошли — UI-поток мёртв необратимо
            # (как для пользователя): перезапускаем окно, это находка.
            rec["status"] = "HUNG"
            rec.update(br.hung.site_stats())
            br.restart()
            rec["restarted"] = True
            rec["stderr_errors"] = []
            return rec
        # До document_ready не дошли: отделить «сайт не пустил» (401/403) и
        # сетевые сбои от настоящих отказов движка (BUG-992).
        chunk = br.stderr_since(log_pos)
        rec["net_failures"] = network_failures(chunk, url)
        rec["http_status"] = main_http_status(chunk, url)
        net_failure = bool(rec["net_failures"])
    except (OSError, json.JSONDecodeError, socket.timeout) as e:  # окно умерло/зависло
        rec["status"] = "DEAD"
        rec["error"] = str(e)[:200]
        br.restart()
        rec["restarted"] = True
        return rec

    try:
        # network_idle добираем неблокирующе — не у всех сайтов сеть затихает
        try:
            br.mcp.tool("wait", {"condition": "network_idle", "timeout_ms": 5000})
            rec["network_idle"] = True
        except RuntimeError:
            rec["network_idle"] = False

        time.sleep(dwell)  # пользователь смотрит на отрисованный сайт
        for direction in (+1, -1):  # визуальная проверка скролла
            for _ in range(scroll_ticks):
                # MCP parse_target больше не принимает {"css": …} (2026-08-17) —
                # только селектор строкой, иначе скролл падает и теряется хвост
                br.mcp.tool("scroll", {"target": {"selector": "body"}, "delta": {"x": 0, "y": direction * 600}})
                time.sleep(0.15)

        contents = br.mcp.resource("resource://screenshot")
        if contents and contents[0].get("data"):
            png = out_dir / f"{slug}.png"
            png_bytes = base64.b64decode(contents[0]["data"])
            png.write_bytes(png_bytes)
            rec["png_size"] = png_size(png)
            if png_hash_counts is not None:
                h = hashlib.md5(png_bytes).hexdigest()  # noqa: S324 — дедуп кадров, не крипто
                png_hash_counts[h] = png_hash_counts.get(h, 0) + 1
                rec["png_hash"] = h

        console = br.mcp.resource("resource://console")
        entries = json.loads(console[0].get("text", "[]")) if console else []
        errors = [e.get("message", "") for e in entries if e.get("level") == "Error"]
        rec["console_total"] = len(entries)
        net_errs = [m for m in errors if NET_CONSOLE_RE.search(m)]
        js_errs = [m for m in errors if not NET_CONSOLE_RE.search(m)]
        # Полные списки + сигнатуры с числом повторений: обрезка на 8 искажала
        # частоты, по которым ранжируются баги (BUG-992)
        rec["console_errors"] = js_errs
        rec["console_network_errors"] = net_errs
        rec["console_error_sigs"] = count_sigs(js_errs)
    except (OSError, RuntimeError, json.JSONDecodeError, socket.timeout) as e:
        rec.setdefault("error", str(e)[:200])
        if br.proc.poll() is not None or isinstance(e, OSError):
            br.restart()
            rec["restarted"] = True
    chunk = br.stderr_since(log_pos)
    rec.setdefault("net_failures", network_failures(chunk, url))
    net_failure = net_failure or bool(rec["net_failures"])
    rec["http_status"] = main_http_status(chunk, url)
    rec["stderr_errors"], rec["stderr_error_sigs"], _ = br.stderr_errors_since(log_pos)
    rec.update(br.ram.site_stats())
    rec.update(br.hung.site_stats())
    # Накопительные итоги процесса — явными именами, не как «на сайт» (BUG-992)
    proc_total = _win_proc_stats(br.proc)
    if proc_total:
        rec["proc_peak_mb_total"] = proc_total.get("peak_mb")
        rec["proc_cpu_s_total"] = proc_total.get("cpu_s")

    health_events, br.health_log_pos = br.health_events_since(health_pos)
    rec["broken_render_signal"] = any(e.get("kind") == "broken_render" for e in health_events)
    rec["suspected_duplicate_frame"] = bool(
        png_hash_counts is not None and rec.get("png_hash") and png_hash_counts[rec["png_hash"]] > 1
    )
    js_error_count = len(rec.get("console_errors") or [])
    rec["status"] = classify_status(
        dead=False,
        hung=False,
        http_status=rec.get("http_status"),
        net_failure=net_failure,
        ready=ready,
        broken_render=rec["broken_render_signal"] or rec["suspected_duplicate_frame"],
        have_png=bool(rec.get("png_size")),
        js_error_count=js_error_count,
    )
    if rec["status"] == "DEGRADED" and not rec.get("png_size"):
        rec.setdefault("no_png_reason", rec.get("error", "resource://screenshot не вернул кадр"))
    return rec


def summary_md_live(results: list[dict], exe: Path, commit: str, restarts: int) -> str:
    """Markdown-сводка живого прогона."""
    lines = [
        f"# Перф-аудит (живое окно): {len(results)} сайтов, перезапусков: {restarts}",
        "",
        f"- Бинарь: `{exe}` (GUI, один процесс, дефолтный рендер-бэкенд)",
        f"- Коммит движка: `{commit}`",
        "",
        "| slug | статус | готовность, с | RAM тек, МБ | RAM пик, МБ | CPU сайт, с | не отвечает, с | JS-ошибки | первая ошибка |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for r in results:
        all_errs = (r.get("console_errors") or []) + (r.get("stderr_errors") or [])
        err = all_errs[0][:60] if all_errs else r.get("error", "")
        restarted = " ↻" if r["restarted"] else ""
        if r.get("own_tab") is False:
            restarted += " (без вкладки)"
        lines.append(
            f"| {r['slug']} | {r['status']}{restarted} | {r.get('ready_s', '—')} "
            f"| {r.get('cur_mb', '—')} | {r.get('peak_mb', '—')} | {r.get('cpu_s', '—')} "
            f"| {r.get('hung_total_s', '')} | {len(all_errs) or ''} | {err} |"
        )
    lines += [
        "",
        "Статусы (AUDIT-1): OK — готовность + кадр + ноль JS-ошибок консоли; DEGRADED — готовность есть,",
        "но кадр не подтверждён или есть JS-ошибки; BROKEN_RENDER — готовность есть, но признаки говорят",
        "«ничего не нарисовано» (эвристика health-log ИЛИ повтор кадра в прогоне — читать как подозрение,",
        "точность ограничена BUG-993); TIMEOUT — не дошли до готовности не по вине сайта/сети;",
        "SITE_REFUSED — главный документ вернул 401/403; NET_FAIL — сетевой сбой главного документа",
        "(DNS/TLS/соединение/блок); HUNG/DEAD — окно зависло/упало необратимо.",
        "peak_mb/cpu_s — на сайт (максимум/дельта замеров); накопительные итоги процесса —",
        "proc_peak_mb_total/proc_cpu_s_total в results.json.",
    ]
    return "\n".join(lines) + "\n"



def dominant_phase(rec: dict) -> str:
    """Название самой дорогой фазы записи (для сводки)."""
    ph = rec.get("phases")
    if not ph:
        return "-"
    return max(ph, key=ph.get).removesuffix("_s")


def summary_md(results: list[dict], exe: Path, commit: str) -> str:
    """Markdown-сводка прогона."""
    lines = [
        f"# Перф-аудит: {len(results)} сайтов",
        "",
        f"- Бинарь: `{exe}`",
        f"- Коммит движка: `{commit}`",
        "",
        "| slug | статус | HTTP | source, с | layout, с | screenshot, с | RAM пик, МБ | CPU, с | доминирует | ошибки |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in results:
        errs = r["screenshot"]["error_lines"] or r["layout"]["error_lines"]
        err_note = errs[0][:60] if errs else ""
        dom = dominant_phase(r) if r["status"] == "OK" else "-"
        lines.append(
            f"| {r['slug']} | {r['status']} | {r.get('http_status') or '—'} "
            f"| {r['source']['wall_s']} | {r['layout']['wall_s']} "
            f"| {r['screenshot']['wall_s']} | {r['screenshot'].get('peak_mb', '—')} "
            f"| {r['screenshot'].get('cpu_s', '—')} | {dom} | {err_note} |"
        )
    return "\n".join(lines) + "\n"


def compare(results: list[dict], prev_path: Path) -> str:
    """Дельта t_screenshot vs предыдущий прогон (тот же корпус, та же машина)."""
    prev = {r["slug"]: r for r in json.loads(prev_path.read_text(encoding="utf-8"))["results"]}
    lines = [f"\n## Сравнение с {prev_path.name}", "", "| slug | было, с | стало, с | Δ% |", "|---|---|---|---|"]
    for r in results:
        p = prev.get(r["slug"])
        if not p:
            continue
        def total(rec: dict) -> float | None:
            return rec.get("ready_s") if "ready_s" in rec else rec.get("screenshot", {}).get("wall_s")

        was, now = total(p), total(r)
        if was is None or now is None:
            continue
        delta = f"{(now - was) / was * 100:+.0f}%" if was else "—"
        mark = " ⚠" if was and (now - was) / was > 0.20 else ""
        lines.append(f"| {r['slug']} | {was} | {now} | {delta}{mark} |")
    return "\n".join(lines) + "\n"


def iter_stability(exe: Path, sites: list, out_dir: Path, timeout: int, dwell: float, scroll_ticks: int, stats: dict):
    """Режим B (стабильность): один процесс на весь корпус — ровно то, что
    ловит накопление, утечки, деградацию и падения между сайтами. Прогонять
    корпус партиями по 15-20 сайтов (`--only`) — так удобнее читать таблицу
    и падение на N-м сайте не теряет разбор предыдущей партии."""
    png_hash_counts: dict[str, int] = {}
    br = LiveBrowser(exe, out_dir, timeout)
    try:
        for slug, url in sites:
            yield audit_site_live(br, slug, url, out_dir, timeout, dwell, scroll_ticks, png_hash_counts)
    finally:
        stats["restarts"] = br.restarts
        br.close()


def iter_compat(exe: Path, sites: list, out_dir: Path, timeout: int, dwell: float, scroll_ticks: int, stats: dict):
    """Режим A (совместимость): свежий процесс на каждый сайт — числа
    сопоставимы между сайтами, потому что ни один не наследует RAM/вкладки
    предыдущего (в отличие от режима B, где это накопление — сама суть)."""
    png_hash_counts: dict[str, int] = {}
    total_restarts = 0
    for slug, url in sites:
        br = LiveBrowser(exe, out_dir, timeout)
        try:
            yield audit_site_live(br, slug, url, out_dir, timeout, dwell, scroll_ticks, png_hash_counts)
        finally:
            total_restarts += br.restarts
            br.close()
    stats["restarts"] = total_restarts


def iter_session(exe: Path, sites: list, out_dir: Path, timeout: int, dwell: float, scroll_ticks: int, tab_count: int, stats: dict):
    """Режим C (сессия): одна сессия — N вкладок, скролл, назад/вперёд и попап
    (через `eval`: `history.back/forward()`, `window.open()`), проверка
    фрейма, приближённое закрытие.

    Ограничение тулинга: MCP (`crates/mcp/src/server.rs`) не даёт `switch_tab`
    ни `close_tab` — только последняя открытая вкладка активна и управляема.
    «Закрытие» поэтому реализовано как навигация последней активной вкладки на
    `about:blank`; остальные N-1 вкладок сессии программно не закрываются —
    задокументированный пробел, не тихая недоработка (см. `note` записи
    `_session_close_last_tab`).
    """
    png_hash_counts: dict[str, int] = {}
    picked = sites[:tab_count] if tab_count else sites
    br = LiveBrowser(exe, out_dir, timeout)
    try:
        for slug, url in picked:
            rec = audit_site_live(br, slug, url, out_dir, timeout, dwell, scroll_ticks, png_hash_counts)
            rec["session_step"] = "tab_open"
            yield rec

        nav_a = picked[0][1] if picked else "about:blank"
        nav_b = picked[min(1, len(picked) - 1)][1] if picked else "about:blank"
        # history.back()/forward() идут через JS (eval), а не через шелл-навигацию —
        # готовность после них не всегда даёт тот же document_ready-переход, что
        # обычная навигация (известный класс гэпов вокруг history.*, см.
        # docs/engine-gaps.md), поэтому таймаут короткий и неуспех не считается
        # фатальным для остальных шагов сессии — settle-пауза ниже гарантирует,
        # что к следующему шагу (frame) JS-контекст стабилен независимо от исхода.
        back_fwd = {"slug": "_session_back_forward", "url": nav_b, "restarted": False, "session_step": "back_forward"}
        t0 = time.monotonic()
        try:
            br.mcp.tool("navigate", {"url": nav_a})
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": timeout * 1000})
            br.mcp.tool("navigate", {"url": nav_b})
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": timeout * 1000})
        except RuntimeError as e:
            back_fwd["setup_error"] = str(e)[:200]
        try:
            br.mcp.tool("eval", {"code": "history.back()"})
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": 5000})
            back_fwd["back_ok"] = True
        except RuntimeError as e:
            back_fwd["back_ok"] = False
            back_fwd["error"] = str(e)[:200]
        time.sleep(1.0)
        try:
            br.mcp.tool("eval", {"code": "history.forward()"})
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": 5000})
            back_fwd["forward_ok"] = True
        except RuntimeError as e:
            back_fwd["forward_ok"] = False
            back_fwd.setdefault("error", str(e)[:200])
        time.sleep(1.0)  # settle перед следующим шагом (frame), см. комментарий выше
        back_fwd["elapsed_s"] = round(time.monotonic() - t0, 2)
        back_fwd["status"] = "OK" if back_fwd.get("back_ok") and back_fwd.get("forward_ok") else "DEGRADED"
        yield back_fwd

        popup = {"slug": "_session_popup", "url": "", "restarted": False, "session_step": "popup"}
        try:
            r = br.mcp.tool("eval", {"code": "typeof window.open('about:blank') !== 'undefined'"})
            popup["opened"] = bool(r.get("result"))
            popup["status"] = "OK" if popup["opened"] else "DEGRADED"
        except RuntimeError as e:
            popup["opened"] = False
            popup["error"] = str(e)[:200]
            popup["status"] = "DEGRADED"
        yield popup

        frame = {"slug": "_session_frame", "url": "", "restarted": False, "session_step": "frame"}
        try:
            r = br.mcp.tool("eval", {"code": "document.querySelectorAll('iframe').length"})
            frame["iframe_count"] = r.get("result")
            frame["status"] = "OK"
        except RuntimeError as e:
            frame["iframe_count"] = None
            frame["error"] = str(e)[:200]
            frame["status"] = "DEGRADED"
        yield frame

        close_rec = {"slug": "_session_close_last_tab", "url": "about:blank", "restarted": False, "session_step": "close"}
        before = br.ram.site_stats()
        try:
            br.mcp.tool("navigate", {"url": "about:blank"})
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": 10000})
            close_rec["status"] = "OK"
        except RuntimeError as e:
            close_rec["status"] = "DEGRADED"
            close_rec["error"] = str(e)[:200]
        time.sleep(1.0)
        after = br.ram.site_stats()
        close_rec["cur_mb_before"] = before.get("cur_mb")
        close_rec["cur_mb_after"] = after.get("cur_mb")
        close_rec["note"] = (
            f"{len(picked)} вкладок открыто за сессию; программно закрыта (навигацией на about:blank) "
            "только последняя активная — MCP не даёт switch_tab/close_tab, остальные остаются открытыми "
            "(известное ограничение тулинга)"
        )
        yield close_rec
    finally:
        stats["restarts"] = br.restarts
        br.close()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--corpus", default=str(DEFAULT_CORPUS), help="файл корпуса (slug url)")
    ap.add_argument("--only", action="append", default=[], help="фильтр по подстроке slug (повторяемый)")
    ap.add_argument("--exe", help="путь к lumen.exe (иначе $LUMEN_EXE / target/*)")
    ap.add_argument("--timeout", type=int, default=240, help="таймаут одной стадии/навигации, с (default 240)")
    ap.add_argument("--compare", help="results.json предыдущего прогона для дельта-таблицы")
    ap.add_argument("--phases", action="store_true", help="headless-разложение по фазам вместо живого окна (точечный разбор одного сайта)")
    ap.add_argument(
        "--mode", choices=("stability", "compat", "session"), default="stability",
        help="режим живого прогона (AUDIT-1): stability=B (один процесс на корпус, default), "
             "compat=A (свежий процесс на сайт, числа сопоставимы), session=C (одна сессия: вкладки/скролл/назад-вперёд/попап/фрейм)",
    )
    ap.add_argument("--session-tabs", type=int, default=20, help="--mode session: сколько сайтов открыть вкладками (default 20)")
    ap.add_argument("--dwell", type=float, default=3.0, help="live: секунд показывать каждый сайт (default 3)")
    ap.add_argument("--scroll-ticks", type=int, default=4, help="live: щелчков скролла вниз/вверх (default 4)")
    args = ap.parse_args()

    exe = find_exe(args.exe)
    sites = load_corpus(Path(args.corpus), args.only)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = OUT_ROOT / stamp
    out_dir.mkdir(parents=True, exist_ok=True)
    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, cwd=str(REPO_ROOT)
    ).stdout.strip()

    mode = "phases" if args.phases else args.mode
    print(f"Аудит {len(sites)} сайтов ({mode}), бинарь {exe}, таймаут {args.timeout}с")
    print(f"Результаты: {out_dir}")
    results: list[dict] = []
    stats = {"restarts": 0}

    def flush_results() -> None:
        """Промежуточное сохранение — падение на N-м сайте не теряет предыдущие."""
        (out_dir / "results.json").write_text(
            json.dumps(
                {"date": stamp, "commit": commit, "exe": str(exe), "mode": mode,
                 "timeout_s": args.timeout, "results": results},
                ensure_ascii=False,
                indent=1,
            ),
            encoding="utf-8",
        )

    if args.phases:
        for i, (slug, url) in enumerate(sites, 1):
            print(f"[{i}/{len(sites)}] {slug} {url} ... ", end="", flush=True)
            rec = audit_site(exe, slug, url, out_dir, args.timeout)
            note = f"total={rec['screenshot']['wall_s']}s dominant={dominant_phase(rec)}"
            results.append(rec)
            print(f"{rec['status']} {note}")
            flush_results()
    else:
        if mode == "stability":
            gen = iter_stability(exe, sites, out_dir, args.timeout, args.dwell, args.scroll_ticks, stats)
        elif mode == "compat":
            gen = iter_compat(exe, sites, out_dir, args.timeout, args.dwell, args.scroll_ticks, stats)
        else:
            gen = iter_session(exe, sites, out_dir, args.timeout, args.dwell, args.scroll_ticks, args.session_tabs, stats)
        # Закрытие процесса(-ов) идёт в finally самого генератора (LiveBrowser
        # держится внутри iter_*) — не нужен отдельный try/finally здесь.
        for i, rec in enumerate(gen, 1):
            n_err = len(rec.get("console_errors", [])) + len(rec.get("stderr_errors", []))
            hung = f" hung={rec['hung_total_s']}s" if rec.get("hung_total_s") else ""
            note = (f"ready={rec.get('ready_s', '—')}s ram={rec.get('cur_mb', '—')}MB{hung} "
                    f"err={n_err}"
                    + (" RESTARTED" if rec.get("restarted") else ""))
            results.append(rec)
            print(f"[{i}] {rec['slug']} {rec.get('url', '')} -> {rec['status']} {note}")
            flush_results()

    md = (summary_md(results, exe, commit) if args.phases
          else summary_md_live(results, exe, commit, stats["restarts"]))
    if args.compare:
        md += compare(results, Path(args.compare))
    (out_dir / "summary.md").write_text(md, encoding="utf-8")
    print("\n" + md)
    outside = ("SITE_REFUSED", "NET_FAIL")
    healthy = ("OK", "DEGRADED") if not args.phases else ("OK", "OK_NO_PNG")
    ok_slugs = [r["slug"] for r in results if r["status"] in healthy]
    refused = [r["slug"] for r in results if r["status"] in outside]
    problems = [r["slug"] for r in results if r["status"] not in (*healthy, *outside)]
    print(
        f"Готово: {len(ok_slugs)}/{len(results)} дошли до document_ready"
        + (f", закрыты сайтом/сетью ({', '.join(refused)})" if refused else "")
        + (f", проблемы движка ({', '.join(problems)})" if problems else "")
    )


if __name__ == "__main__":
    main()
