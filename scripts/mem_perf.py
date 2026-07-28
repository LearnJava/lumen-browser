#!/usr/bin/env python3
"""Память Lumen как ВРЕМЕННОЙ РЯД, а не разовый дамп (дорожка PERF, PERF-5).

Разовый `LUMEN_MEM_REPORT` (временная инструментация из BUG-272) отвечает на
вопрос «сколько сейчас», но не на два вопроса, которые пользователь чувствует в
длинной сессии: выходит ли память на ПЛАТО после открытия N вкладок и не ТЕЧЁТ ли
она со временем. Этот харнесс превращает уже существующие поверхности в
постоянную метрику — БЕЗ нового кода в движке, как PERF-2/3/4:

  • плотный ряд RSS процесса (WinAPI `GetProcessMemoryInfo`, тот же приём, что в
    perf_audit.py) — снимается харнессом каждые `--sample-ms`, поэтому виден и
    per-tab прирост, и наклон на idle;
  • разбивка «куда ушла память» из строк `MEM_REPORT` (Rust-структуры: dl-cache,
    image/prefetch-кэши, web-fonts, GIF, femtovg raw_images) + `js_malloc`
    (куча V8 = C-heap). Неатрибуцированный остаток `rss − rust − js` —
    это GPU-драйвер / фрагментация / прочий C-heap (см. reference_memory_diagnosis_toolkit);
  • GPU-память процесса (best-effort, `--gpu`, Windows `Get-Counter`) — на
    интегрированной графике это системная RAM, поэтому она уже сидит в RSS:
    показываем отдельным сигналом, НЕ вычитаем (иначе двойной учёт).

Две фазы прогона:
  ramp  — открыть N вкладок (MCP `new_tab`) на фикстуру, снять прирост RSS на
          вкладку и наклон RSS/вкладку (растёт линейно = per-tab-утечка);
  hold  — простоять idle `--hold-s` секунд, снять наклон RSS во времени
          (`hold_slope_mb_per_min`) — детектор утечки длинной сессии.

Разделение дорожки PERF: тулинг — P2 (этот скрипт), движковые правки по находкам
(например постоянный counting-allocator для точного Rust-heap, или spans утечки) —
P1/P3. Честное ограничение чисто-тулингового пути: Rust-heap считается как СУММА
известных хранилищ из `MEM_REPORT`, а не через инструментированный аллокатор,
поэтому «неатрибуцированный» бакет включает и невидимую Rust-фрагментацию.

Требуется живое окно (winit); в headless-средах без дисплея окно может не
подняться — тогда используйте `--selftest` (проверяет чистую статистику без
браузера, входит в ворота).

Примеры:
  python scripts/mem_perf.py                             # фикстура, 6 вкладок + hold
  python scripts/mem_perf.py --tabs 10 --hold-s 60
  python scripts/mem_perf.py --page https://example.com --tabs 4
  python scripts/mem_perf.py --gpu                       # + GPU-counter (Windows, медленно)
  python scripts/mem_perf.py --json docs/perf/memory-runs/2026-07-18.json
  python scripts/mem_perf.py --compare docs/perf/memory-runs/2026-07-18.json
  python scripts/mem_perf.py --selftest                  # статистика без браузера, для ворот
  LUMEN_EXE=path/to/lumen.exe python scripts/mem_perf.py --build
"""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime
from pathlib import Path

# Windows-консоль по умолчанию cp1251 — переключаем на UTF-8, чтобы русские
# строки и Δ/⚠ в сводке не роняли скрипт.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_FIXTURE = REPO_ROOT / "scripts" / "perf-fixtures" / "mem.html"
OUT_ROOT = REPO_ROOT / ".tmp" / "mem-perf"

# Порог регрессии в --compare (как в perf_metrics.py / input_perf.py / perf_audit.py).
REGRESSION_PCT = 0.20
# Метрики, по которым считаем регрессию: рост памяти = хуже.
REGRESSION_KEYS = ("plateau_rss_mb", "per_tab_mb", "hold_slope_mb_per_min", "unattributed_mb")
# По умолчанию: наклон RSS на idle больше этого (МБ/мин) помечаем как подозрение на утечку.
DEFAULT_LEAK_MB_PER_MIN = 5.0

# ── Разбор строки MEM_REPORT ──────────────────────────────────────────────────
# Формат (crates/shell/src/main.rs, ветка about_to_wait под LUMEN_MEM_REPORT):
#   MEM_REPORT dl_cmds=.. prev_styles=.. dl_cache=X.XMB img_cache=N/X.XMB
#   prefetch=N/X.XMB web_fonts=N/X.XMB gifs=N/X.XMB js_malloc=X.XMB js_used=X.XMB
#   femtovg: raw_images=N (X.X MB), ...           ← только на femtovg-бэкенде
# Каждый под-паттерн опционален: под wgpu-бэкендом femtovg-хвоста нет, а старые
# бинари могут не печатать часть полей — отсутствующее считаем нулём.
_MB_FIELDS = {
    "dl_cache_mb": re.compile(r"dl_cache=([\d.]+)MB"),
    "img_cache_mb": re.compile(r"img_cache=\d+/([\d.]+)MB"),
    "prefetch_mb": re.compile(r"prefetch=\d+/([\d.]+)MB"),
    "web_fonts_mb": re.compile(r"web_fonts=\d+/([\d.]+)MB"),
    "gifs_mb": re.compile(r"gifs=\d+/([\d.]+)MB"),
    # Ведущий минус: движок печатает -1/1e6 = «-0.0MB» как sentinel «JS-heap
    # недоступен» (drain_query_js вернул None в дефолтном flag-off режиме MT).
    "js_malloc_mb": re.compile(r"js_malloc=(-?[\d.]+)MB"),
    "js_used_mb": re.compile(r"js_used=(-?[\d.]+)MB"),
    "femtovg_raw_mb": re.compile(r"raw_images=\d+ \(([\d.]+) MB\)"),
}
# Слагаемые Rust-heap (известные Rust-структуры движка).
_RUST_KEYS = ("dl_cache_mb", "img_cache_mb", "prefetch_mb", "web_fonts_mb", "gifs_mb", "femtovg_raw_mb")


# ── Чистая статистика (тестируется --selftest, без сети/браузера) ──────────────

def median(values: list[float]) -> float | None:
    """Медиана (для плато = стационарного значения ряда). None для пустого."""
    if not values:
        return None
    s = sorted(values)
    n = len(s)
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2.0


def least_squares_slope(xs: list[float], ys: list[float]) -> float | None:
    """Наклон прямой МНК (единиц y на единицу x). None если < 2 точек или x без разброса.

    Ядро leak-детектора: наклон ряда RSS во времени (idle) и по номеру вкладки
    (ramp). Детерминированная чистая функция — покрыта --selftest.
    """
    n = len(xs)
    if n < 2 or len(ys) != n:
        return None
    mx = sum(xs) / n
    my = sum(ys) / n
    denom = sum((x - mx) ** 2 for x in xs)
    if denom == 0:
        return None
    num = sum((xs[i] - mx) * (ys[i] - my) for i in range(n))
    return num / denom


def parse_mem_report(line: str) -> dict | None:
    """Строку `MEM_REPORT …` → {поле_mb: float}. None если это не строка отчёта.

    Отсутствующие поля (другой бэкенд / старый бинарь) не попадают в словарь —
    их подставляет нулём attribute(). Числа уже в МБ (движок делит на 1e6).
    """
    if "MEM_REPORT" not in line:
        return None
    out: dict[str, float | None] = {}
    for key, rx in _MB_FIELDS.items():
        m = rx.search(line)
        if not m:
            continue
        raw = m.group(1)
        # JS-поля: движок печатает -1/1e6 (≈ «-0.0MB») как sentinel «недоступно».
        # После форматирования {:.1} и минус, и ноль теряются во float, поэтому
        # ловим sentinel по ЗНАКУ в сырой строке — память неотрицательна.
        if raw.startswith("-"):
            out[key] = None
        else:
            out[key] = float(raw)
    return out


def attribute(sample: dict, rss_mb: float | None) -> dict:
    """Разложить один замер на Rust-heap / C-heap(JS) / неатрибуцированный остаток.

    rust_known = сумма известных Rust-структур из MEM_REPORT;
    cheap_js   = js_malloc (арена V8 живёт в C-heap); None если движок вернул
                 sentinel (отрицательное значение = JS-heap не запрошен в
                 дефолтном flag-off режиме MT) — тогда JS не вычитается и его
                 доля честно уходит в «неатрибуцировано»;
    unattributed = max(0, rss − rust_known − cheap_js) — GPU-драйвер /
                   фрагментация / прочий C-heap (см. reference_memory_diagnosis_toolkit).
    Без rss (нет WinAPI) unattributed = None.
    """
    rust = round(sum(sample.get(k) or 0.0 for k in _RUST_KEYS), 2)
    raw_js = sample.get("js_malloc_mb")
    raw_used = sample.get("js_used_mb")
    cheap = round(raw_js, 2) if isinstance(raw_js, (int, float)) else None
    used = round(raw_used, 2) if isinstance(raw_used, (int, float)) else None
    out = {
        "rust_known_mb": rust,
        "cheap_js_mb": cheap,
        "js_used_mb": used,
    }
    if rss_mb is not None:
        out["rss_mb"] = round(rss_mb, 1)
        out["unattributed_mb"] = round(max(0.0, rss_mb - rust - (cheap or 0.0)), 1)
    else:
        out["rss_mb"] = None
        out["unattributed_mb"] = None
    return out


# ── Замер памяти процесса (WinAPI, без зависимостей) ──────────────────────────

def proc_rss_mb(popen: subprocess.Popen) -> float | None:
    """Текущий working set (RSS) живого процесса в МБ. None вне Windows / при ошибке.

    Тот же приём, что _win_proc_stats() в perf_audit.py, но для ЖИВОГО процесса
    (WorkingSetSize, а не Peak): вызывается многократно, строит ряд.
    """
    if sys.platform != "win32":
        return None
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

    try:
        handle = wintypes.HANDLE(int(popen._handle))  # noqa: SLF001 — публичного API у Popen нет
        pmc = PMC()
        pmc.cb = ctypes.sizeof(pmc)
        if ctypes.WinDLL("psapi").GetProcessMemoryInfo(handle, ctypes.byref(pmc), pmc.cb):
            return pmc.WorkingSetSize / 1048576.0
    except (OSError, AttributeError, ValueError):
        pass
    return None


def gpu_local_mb(pid: int) -> float | None:
    """GPU Process Memory «Local Usage» процесса (МБ) через PowerShell. None если недоступно.

    Best-effort и МЕДЛЕННО (Get-Counter спавнит PowerShell ~0.3-1 с), поэтому
    снимается только в контрольных точках (--gpu). На интегрированной графике это
    системная RAM — сигнал справочный, в неатрибуцированный остаток не вычитается.
    """
    if sys.platform != "win32":
        return None
    ps = (
        f"$c = Get-Counter '\\GPU Process Memory(pid_{pid}*)\\Local Usage' "
        f"-ErrorAction SilentlyContinue; "
        f"if ($c) {{ ($c.CounterSamples | Measure-Object -Property CookedValue -Sum).Sum }}"
    )
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps],
            capture_output=True, text=True, timeout=15,
        ).stdout.strip()
        return round(float(out) / 1048576.0, 1) if out else None
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None


# ── MCP-клиент и читатель MEM_REPORT ──────────────────────────────────────────

def free_port() -> int:
    """Свободный TCP-порт на 127.0.0.1 (тот же приём, что input_perf.py)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Client:
    """Line-delimited JSON-RPC клиент к `--mcp-live-port` (протокол run.py --live)."""

    def __init__(self, port: int) -> None:
        last: Exception | None = None
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
                break
            except OSError as e:
                last = e
                time.sleep(0.1)
        else:
            raise RuntimeError(f"MCP-порт {port} не поднялся: {last}")
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self.sock.settimeout(60)
        self._reader = self.sock.makefile("r", encoding="utf-8", newline="\n")
        self._id = 0

    def call(self, name: str, arguments: dict) -> dict:
        self._id += 1
        req = json.dumps({"jsonrpc": "2.0", "id": self._id,
                          "method": "tools/call",
                          "params": {"name": name, "arguments": arguments}})
        self.sock.sendall((req + "\n").encode("utf-8"))
        line = self._reader.readline()
        if not line:
            raise RuntimeError("MCP-соединение закрыто (окно упало?)")
        resp = json.loads(line)
        if resp.get("error") is not None:
            raise RuntimeError(f"{name}: {resp['error']}")
        return resp.get("result") or {}


class MemReader:
    """Фоновый поток: дренирует stderr дочернего процесса, стамповывает `MEM_REPORT`.

    Дренаж пайпа обязателен (без него ~4 КБ буфера → ложный дедлок процесса,
    см. reference_scroll_bench_harness). Каждую строку отчёта помечаем настенным
    perf_counter() и разбираем в разбивку; сырой поток пишем в log для отладки.
    """

    def __init__(self, stream, log_path: Path) -> None:
        self._stream = stream
        self._log = open(log_path, "w", encoding="utf-8", errors="replace")
        self._lock = threading.Lock()
        # (wall_perf_counter, parsed_mb_dict).
        self.reports: list[tuple[float, dict]] = []
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        for raw in self._stream:
            self._log.write(raw)
            parsed = parse_mem_report(raw)
            if parsed is not None:
                with self._lock:
                    self.reports.append((time.perf_counter(), parsed))
        self._log.close()

    def latest(self) -> tuple[float, dict] | None:
        """Последний разобранный MEM_REPORT (или None, если ещё не было)."""
        with self._lock:
            return self.reports[-1] if self.reports else None


# ── Сборка / поиск бинарника ──────────────────────────────────────────────────

def find_exe(cli_exe: str | None, profile: str) -> Path:
    """Найти lumen.exe: --exe > $LUMEN_EXE > target/{profile,dev-release,release,debug}."""
    candidates = []
    if cli_exe:
        candidates.append(Path(cli_exe))
    if os.environ.get("LUMEN_EXE"):
        candidates.append(Path(os.environ["LUMEN_EXE"]))
    # target/ обычно в корневом клоне, не в worktree — проверяем оба.
    for root in (REPO_ROOT, REPO_ROOT.parent.parent.parent):
        for prof in (profile, "dev-release", "release", "debug"):
            candidates.append(root / "target" / prof / "lumen.exe")
    for c in candidates:
        if c.is_file():
            return c
    sys.exit(
        "lumen.exe не найден. Соберите: cargo build -p lumen-shell --profile "
        + profile + "\nили укажите путь через --exe / $LUMEN_EXE.\nПроверены: "
        + ", ".join(str(c) for c in candidates)
    )


def to_url(page: str) -> str:
    """HTML-файл → file://-URL; URL/about: — как есть."""
    if page.startswith(("http://", "https://", "file://", "about:")):
        return page
    return "file:///" + str(Path(page).resolve()).replace("\\", "/")


# ── Прогон ────────────────────────────────────────────────────────────────────

def sample_series(
    proc: subprocess.Popen, phase: str, duration_s: float, sample_ms: float,
    series: list[dict], t_start: float, tab_i: int,
) -> None:
    """Снимать RSS каждые sample_ms в течение duration_s, дописывая в series.

    Каждая точка: {t_s, phase, tab, rss_mb}. Настенное t_s — от общего начала
    прогона, чтобы наклоны ramp/hold считались в одной оси времени.
    """
    end = time.perf_counter() + duration_s
    while time.perf_counter() < end:
        rss = proc_rss_mb(proc)
        if rss is not None:
            series.append({
                "t_s": round(time.perf_counter() - t_start, 2),
                "phase": phase, "tab": tab_i, "rss_mb": round(rss, 1),
            })
        time.sleep(sample_ms / 1000.0)


def run_session(
    client: Client, reader: MemReader, proc: subprocess.Popen,
    page_url: str, tabs: int, tab_dwell_s: float, hold_s: float,
    sample_ms: float, leak_mb_per_min: float, want_gpu: bool,
) -> dict:
    """Прогнать ramp (N вкладок) + hold (idle), вернуть ряд и агрегаты."""
    t_start = time.perf_counter()
    series: list[dict] = []
    per_tab_rss: list[dict] = []  # RSS сразу после каждой вкладки

    # Первая вкладка уже есть (about:blank) — навигируем её на фикстуру как tab 0.
    client.call("navigate", {"url": page_url})
    client.call("wait", {"condition": "document_ready", "timeout_ms": 20000})
    sample_series(proc, "ramp", tab_dwell_s, sample_ms, series, t_start, 0)
    rss0 = proc_rss_mb(proc)
    if rss0 is not None:
        per_tab_rss.append({"tab": 0, "rss_mb": round(rss0, 1)})

    # Остальные вкладки — new_tab (вкладка становится активной).
    for i in range(1, tabs):
        client.call("new_tab", {"url": page_url})
        client.call("wait", {"condition": "document_ready", "timeout_ms": 20000})
        sample_series(proc, "ramp", tab_dwell_s, sample_ms, series, t_start, i)
        rss = proc_rss_mb(proc)
        if rss is not None:
            per_tab_rss.append({"tab": i, "rss_mb": round(rss, 1)})
        print(f"  вкладка {i + 1}/{tabs} открыта, RSS={rss and round(rss, 1)}МБ")

    rss_after_ramp = proc_rss_mb(proc)

    # Idle-hold — детектор утечки длинной сессии.
    print(f"  hold {hold_s:.0f}с (idle, детектор утечки) ...")
    sample_series(proc, "hold", hold_s, sample_ms, series, t_start, tabs - 1)

    rss_end = proc_rss_mb(proc)
    gpu_mb = gpu_local_mb(proc.pid) if want_gpu else None

    # ── Агрегаты ──
    rss_start = per_tab_rss[0]["rss_mb"] if per_tab_rss else None
    per_tab_mb = None
    if rss_start is not None and rss_after_ramp is not None and tabs > 0:
        per_tab_mb = round((rss_after_ramp - rss_start) / tabs, 2)
    # Наклон RSS по номеру вкладки (линейный рост = per-tab-утечка).
    ramp_slope = None
    if len(per_tab_rss) >= 2:
        ramp_slope = least_squares_slope(
            [p["tab"] for p in per_tab_rss], [p["rss_mb"] for p in per_tab_rss]
        )
        ramp_slope = round(ramp_slope, 2) if ramp_slope is not None else None

    hold_pts = [p for p in series if p["phase"] == "hold"]
    plateau_rss = None
    hold_slope_min = None
    if hold_pts:
        # Плато = медиана RSS в последней трети hold (после переходных процессов).
        tail = hold_pts[len(hold_pts) * 2 // 3:] or hold_pts
        plateau_rss = median([p["rss_mb"] for p in tail])
        s = least_squares_slope([p["t_s"] for p in hold_pts], [p["rss_mb"] for p in hold_pts])
        if s is not None:
            hold_slope_min = round(s * 60.0, 2)  # МБ/с → МБ/мин

    # Разбивка на конец из последнего MEM_REPORT.
    latest = reader.latest()
    breakdown = attribute(latest[1] if latest else {}, rss_end)
    if gpu_mb is not None:
        breakdown["gpu_mb"] = gpu_mb

    return {
        "tabs": tabs,
        "samples": len(series),
        "mem_reports": len(reader.reports),
        "rss_start_mb": rss_start,
        "rss_after_ramp_mb": round(rss_after_ramp, 1) if rss_after_ramp is not None else None,
        "rss_end_mb": round(rss_end, 1) if rss_end is not None else None,
        "per_tab_mb": per_tab_mb,
        "ramp_slope_mb_per_tab": ramp_slope,
        "plateau_rss_mb": round(plateau_rss, 1) if plateau_rss is not None else None,
        "hold_slope_mb_per_min": hold_slope_min,
        "leak_suspected": (hold_slope_min is not None and hold_slope_min > leak_mb_per_min),
        "leak_threshold_mb_per_min": leak_mb_per_min,
        "breakdown": breakdown,
        "per_tab_rss": per_tab_rss,
        "series": series,
    }


# ── Отчёт / сравнение ─────────────────────────────────────────────────────────

def _fmt(v: object) -> str:
    return "—" if v is None else str(v)


def summary_md(result: dict) -> str:
    """Markdown-сводка: агрегаты памяти + разбивка на конец прогона."""
    r = result["result"]
    b = r["breakdown"]
    leak = " ⚠ подозрение на утечку" if r.get("leak_suspected") else ""
    lines = [
        f"# Mem-perf: {result['page']}",
        "",
        f"- Бинарь: `{result['exe']}` (живое окно, LUMEN_MEM_REPORT)",
        f"- Коммит движка: `{result['commit']}`  ·  вкладок: {r['tabs']}  ·  "
        f"замеров RSS: {r['samples']}  ·  MEM_REPORT: {r['mem_reports']}",
        "",
        "## Ряд RSS (МБ)",
        "",
        "| старт | после ramp | конец | на вкладку | наклон/вкладку | плато (hold) | наклон hold (МБ/мин) |",
        "|---|---|---|---|---|---|---|",
        "| " + " | ".join(_fmt(x) for x in (
            r["rss_start_mb"], r["rss_after_ramp_mb"], r["rss_end_mb"],
            r["per_tab_mb"], r["ramp_slope_mb_per_tab"],
            r["plateau_rss_mb"], f'{_fmt(r["hold_slope_mb_per_min"])}{leak}',
        )) + " |",
        "",
        "## Разбивка на конец (МБ)",
        "",
        "| RSS | Rust-heap (известн.) | C-heap JS (js_malloc) | js_used | неатрибуцировано | GPU (best-effort) |",
        "|---|---|---|---|---|---|",
        "| " + " | ".join(_fmt(x) for x in (
            b.get("rss_mb"), b.get("rust_known_mb"), b.get("cheap_js_mb"),
            b.get("js_used_mb"), b.get("unattributed_mb"), b.get("gpu_mb"),
        )) + " |",
        "",
        "> «Неатрибуцировано» = RSS − Rust-heap − js_malloc: GPU-драйвер, "
        "фрагментация Rust-кучи, прочий C-heap. GPU показан отдельно и НЕ вычтен "
        "(на интегрированной графике он уже в RSS). C-heap JS = «—», когда движок "
        "вернул sentinel (JS-heap не запрошен в дефолтном flag-off режиме MT) — "
        "тогда доля V8 уходит в «неатрибуцировано». Точный Rust-heap ждёт "
        "постоянного counting-allocator в движке (задача P1/P3).",
    ]
    return "\n".join(lines) + "\n"


def compare(result: dict, prev_path: Path) -> str:
    """Дельта ключевых метрик vs предыдущий прогон (та же машина). Рост = хуже."""
    prev = json.loads(prev_path.read_text(encoding="utf-8"))
    # Плоский вид: агрегаты верхнего уровня + разбивка (unattributed_mb и т.п.
    # живут внутри "breakdown") в одном словаре, чтобы REGRESSION_KEYS доставали
    # любую метрику независимо от вложенности.
    pr = {**prev.get("result", {}), **prev.get("result", {}).get("breakdown", {})}
    cur = {**result["result"], **result["result"].get("breakdown", {})}
    lines = [f"\n## Сравнение с {prev_path.name}", "",
             "| метрика | было | стало | Δ% |", "|---|---|---|---|"]
    for key in REGRESSION_KEYS:
        was, now = pr.get(key), cur.get(key)
        if not isinstance(was, (int, float)) or not isinstance(now, (int, float)):
            lines.append(f"| {key} | {_fmt(was)} | {_fmt(now)} | — |")
            continue
        delta = f"{(now - was) / was * 100:+.0f}%" if was else "—"
        mark = " ⚠" if was and (now - was) / was > REGRESSION_PCT else ""
        lines.append(f"| {key} | {was} | {now} | {delta}{mark} |")
    return "\n".join(lines) + "\n"


# ── Самопроверка статистики (без сети/браузера, входит в ворота) ──────────────

def selftest() -> int:
    """Проверить median / least_squares_slope / parse_mem_report / attribute. 0 = ок."""
    ok = True

    def check(label: str, got: object, want: object) -> None:
        nonlocal ok
        mark = "ok" if got == want else "FAIL"
        if got != want:
            ok = False
        print(f"  {mark:<4} {label:<32} want={want!r:<14} got={got!r}")

    # median.
    check("median odd", median([3.0, 1.0, 2.0]), 2.0)
    check("median even", median([1.0, 2.0, 3.0, 4.0]), 2.5)
    check("median single", median([7.0]), 7.0)
    check("median empty", median([]), None)

    # least_squares_slope: y = 2x + 5 → наклон 2.
    xs = [0.0, 1.0, 2.0, 3.0]
    ys = [5.0, 7.0, 9.0, 11.0]
    check("slope linear", round(least_squares_slope(xs, ys), 6), 2.0)
    check("slope flat", least_squares_slope([0.0, 1.0, 2.0], [4.0, 4.0, 4.0]), 0.0)
    check("slope single", least_squares_slope([1.0], [1.0]), None)
    check("slope no x spread", least_squares_slope([2.0, 2.0], [1.0, 3.0]), None)

    # parse_mem_report: полная строка (femtovg-хвост) и не-отчёт.
    line = ("MEM_REPORT dl_cmds=100 prev_styles=50 dl_cache=1.2MB img_cache=3/4.5MB "
            "prefetch=2/1.1MB web_fonts=1/0.3MB gifs=0/0.0MB js_malloc=12.0MB js_used=8.0MB "
            "femtovg: raw_images=2 (0.5 MB), gpu_images=4, layer_pool=1")
    p = parse_mem_report(line)
    check("parse dl_cache", p["dl_cache_mb"], 1.2)
    check("parse img_cache", p["img_cache_mb"], 4.5)
    check("parse js_malloc", p["js_malloc_mb"], 12.0)
    check("parse femtovg raw", p["femtovg_raw_mb"], 0.5)
    check("parse non-report", parse_mem_report("some other line"), None)
    # wgpu-бэкенд: femtovg-хвоста нет — поле просто отсутствует.
    p2 = parse_mem_report("MEM_REPORT dl_cmds=1 prev_styles=1 dl_cache=0.5MB "
                          "img_cache=0/0.0MB prefetch=0/0.0MB web_fonts=0/0.0MB "
                          "gifs=0/0.0MB js_malloc=3.0MB js_used=2.0MB")
    check("parse no femtovg", "femtovg_raw_mb" in p2, False)
    # sentinel «-0.0MB» (JS-heap недоступен) распознаётся regex как -0.0.
    ps = parse_mem_report("MEM_REPORT dl_cache=0.4MB img_cache=0/0.0MB prefetch=0/0.0MB "
                          "web_fonts=0/0.0MB gifs=0/0.0MB js_malloc=-0.0MB js_used=-0.0MB")
    check("parse js sentinel", ps["js_malloc_mb"], None)

    # attribute: rust = 1.2+4.5+1.1+0.3+0.0+0.5 = 7.6; cheap = 12.0;
    # rss=100 → unattributed = 100 − 7.6 − 12.0 = 80.4.
    a = attribute(p, 100.0)
    check("attribute rust", a["rust_known_mb"], 7.6)
    check("attribute cheap", a["cheap_js_mb"], 12.0)
    check("attribute unattributed", a["unattributed_mb"], 80.4)
    check("attribute clamp", attribute(p, 5.0)["unattributed_mb"], 0.0)  # rss < sum → 0
    # sentinel: cheap_js = None, JS не вычитается → unattributed = rss − rust.
    asn = attribute(ps, 100.0)
    check("attribute sentinel cheap", asn["cheap_js_mb"], None)
    check("attribute sentinel unattr", asn["unattributed_mb"], 99.6)  # 100 − 0.4
    check("attribute no rss", attribute(p, None)["unattributed_mb"], None)

    print("SELFTEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


# ── main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--page", default=str(DEFAULT_FIXTURE),
                    help="HTML-файл или URL (по умолчанию scripts/perf-fixtures/mem.html)")
    ap.add_argument("--tabs", type=int, default=6, help="сколько вкладок открыть в фазе ramp")
    ap.add_argument("--tab-dwell-s", type=float, default=2.0, help="пауза на вкладку (сек)")
    ap.add_argument("--hold-s", type=float, default=30.0, help="длительность idle-hold (сек)")
    ap.add_argument("--sample-ms", type=float, default=500.0, help="период замера RSS (мс)")
    ap.add_argument("--leak-mb-per-min", type=float, default=DEFAULT_LEAK_MB_PER_MIN,
                    help=f"порог наклона hold для флага утечки (default {DEFAULT_LEAK_MB_PER_MIN} МБ/мин)")
    ap.add_argument("--gpu", action="store_true",
                    help="снять GPU Process Memory (Windows, медленно, best-effort)")
    ap.add_argument("--profile", default="dev-release", help="cargo-профиль для --build/поиска exe")
    ap.add_argument("--build", action="store_true", help="пересобрать lumen-shell перед прогоном")
    ap.add_argument("--exe", help="путь к lumen.exe (иначе $LUMEN_EXE / target/*)")
    ap.add_argument("--json", help="куда записать результат (JSON); иначе .tmp/mem-perf/*")
    ap.add_argument("--compare", help="результат предыдущего прогона (JSON) для дельта-таблицы")
    ap.add_argument("--selftest", action="store_true",
                    help="проверить статистику без сети/браузера и выйти")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    if args.tabs < 1:
        sys.exit("--tabs должно быть ≥ 1")

    if args.build:
        rc = subprocess.call(
            ["cargo", "build", "-p", "lumen-shell", "--profile", args.profile], cwd=str(REPO_ROOT)
        )
        if rc != 0:
            sys.exit(rc)

    exe = find_exe(args.exe, args.profile)
    page_url = to_url(args.page)
    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, cwd=str(REPO_ROOT)
    ).stdout.strip()

    OUT_ROOT.mkdir(parents=True, exist_ok=True)
    stderr_log = OUT_ROOT / "stderr.log"
    port = free_port()
    env = dict(os.environ)
    env.setdefault("LUMEN_MEM_REPORT", "1")

    print(f"Бинарь {exe}\nСтраница {page_url}\n"
          f"Вкладок: {args.tabs}  dwell={args.tab_dwell_s}с  hold={args.hold_s}с  "
          f"замер каждые {args.sample_ms:.0f}мс  GPU={'да' if args.gpu else 'нет'}")

    proc = subprocess.Popen(
        [str(exe), "--mcp-live-port", str(port), "about:blank"],
        cwd=str(REPO_ROOT), env=env,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, bufsize=1, text=True,
        encoding="utf-8", errors="replace",
    )
    reader = MemReader(proc.stderr, stderr_log)
    result: dict = {
        "date": datetime.now().strftime("%Y%m%d-%H%M%S"),
        "commit": commit, "exe": str(exe), "page": page_url, "result": {},
    }
    try:
        c = Client(port)
        result["result"] = run_session(
            c, reader, proc, page_url, args.tabs, args.tab_dwell_s,
            args.hold_s, args.sample_ms, args.leak_mb_per_min, args.gpu,
        )
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()

    if not result["result"].get("samples"):
        print(f"Замеров RSS не собрано (не Windows или окно упало) — смотрите {stderr_log}",
              file=sys.stderr)
        sys.exit(1)

    out_json = Path(args.json) if args.json else OUT_ROOT / f"{result['date']}.json"
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")

    md = summary_md(result)
    if args.compare:
        md += compare(result, Path(args.compare))
    print("\n" + md)
    print(f"JSON: {out_json}\nСырой лог MEM_REPORT: {stderr_log}")


if __name__ == "__main__":
    main()
