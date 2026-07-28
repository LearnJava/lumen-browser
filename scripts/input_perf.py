#!/usr/bin/env python3
"""Латентность ввода и плавность кадра в живом окне Lumen (дорожка PERF, PERF-3).

Меряет две вещи, которые пользователь чувствует напрямую, поверх уже
существующих поверхностей (`LUMEN_FRAME_LOG=1` + MCP `scroll`/`click`/`type`):

  1. input→repaint латентность конец-в-конце — от отправки синтетического
     события (клик/клавиша/щелчок колеса) до прибытия первой кадровой строки
     `[frame] …` в stderr. Строка печатается ПОСЛЕ swap_buffers/рендера, т.е.
     соответствует уже показанному кадру. Харнесс стамповывает каждую строку
     настенным `perf_counter()` при чтении и берёт первый кадр после события.
  2. Плавность кадра как РАСПРЕДЕЛЕНИЕ, не среднее: p50/p95/p99/max времени
     кадра + счётчик дропнутых кадров (тех, что дольше бюджета, по умолчанию
     16.67мс = 60 Гц). Пользователь замечает хвост распределения (p95/p99),
     поэтому scroll_perf.py со средним/медианой недостаточно.

Отличие от scroll_perf.py: тот покрывает только скролл и печатает avg/median/max.
Здесь — три вида ввода (scroll/click/type), перцентильный отчёт, счётчик
дропнутых кадров и per-event латентность; scroll_perf.py оставлен как есть.

Разделение дорожки PERF: тулинг — P2 (этот скрипт), движковые правки по
находкам — P1. Честное ограничение чисто-тулингового пути: латентность меряется
до ПЕРВОЙ кадровой строки после события; она включает задержку flush stderr-пайпа
(мал, eprintln флашит по '\n') и не разбивает hit-test / dispatch / paint по
отдельности — для этого нужен инструментированный трейс в движке (спаны
`input-dispatch`/`hit-test`, дело P1). До тех пор это устойчивый end-to-end
сигнал и ловушка регрессий, как graphic_tests для пикселей.

Требуется живое окно (winit) — в headless-средах без доступа к дисплею окно может
не подняться; тогда используйте --selftest (проверяет чистую статистику без
браузера, входит в ворота).

Примеры:
  python scripts/input_perf.py                       # fixture, все три workload
  python scripts/input_perf.py --workloads scroll,click
  python scripts/input_perf.py --page graphic_tests/1000000-final.html --workloads scroll
  python scripts/input_perf.py --json docs/perf/input-runs/2026-07-18.json
  python scripts/input_perf.py --compare docs/perf/input-runs/2026-07-18.json
  python scripts/input_perf.py --selftest            # статистика без браузера, для ворот
  LUMEN_EXE=path/to/lumen.exe python scripts/input_perf.py --build
"""

from __future__ import annotations

import argparse
import bisect
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
DEFAULT_FIXTURE = REPO_ROOT / "scripts" / "perf-fixtures" / "input.html"
OUT_ROOT = REPO_ROOT / ".tmp" / "input-perf"

# Бюджет одного кадра (мс): 60 Гц. Кадр дольше бюджета считаем «дропнутым»
# (пользователь увидел рывок). Совпадает с бюджетом композитора большинства ОС.
DEFAULT_BUDGET_MS = 1000.0 / 60.0
# Порог регрессии в --compare (как в perf_metrics.py / perf_audit.py).
REGRESSION_PCT = 0.20
# Метрики, по которым считаем регрессию: хвост распределения важнее среднего.
REGRESSION_KEYS = ("frame_p95_ms", "frame_p99_ms", "ack_p95_ms", "latency_p95_ms")

# Извлекают ЗАРЕПОРТЕННОЕ движком время кадра (мс) из строк LUMEN_FRAME_LOG.
# Именно это число (paint/UI-тик), а не интервал прибытия строк, есть реальное
# время кадра — интервал прибытия задан паузой между событиями и бессмыслен.
PAINT_RE = re.compile(r"\[frame\] paint\s+([\d.]+)ms")
TOTAL_RE = re.compile(r"\[frame\] total\s+([\d.]+)ms")


# ── Чистая статистика (тестируется --selftest, без сети/браузера) ─────────────

def percentile(values: list[float], p: float) -> float | None:
    """p-й перцентиль (0..100) методом линейной интерполяции (как numpy 'linear').

    Детерминированная чистая функция — ядро отчёта, поэтому вынесена и покрыта
    --selftest. Для одного элемента возвращает его; для пустого — None.
    """
    if not values:
        return None
    s = sorted(values)
    if len(s) == 1:
        return s[0]
    rank = (p / 100.0) * (len(s) - 1)
    lo = int(rank)
    hi = min(lo + 1, len(s) - 1)
    frac = rank - lo
    return s[lo] + (s[hi] - s[lo]) * frac


def distribution(values: list[float], budget_ms: float, prefix: str) -> dict:
    """Свернуть список времён (мс) в mean/p50/p95/p99/max + счётчик дропов.

    Ключи с префиксом (`frame_`/`latency_`), чтобы кадры и латентность
    ввода жили в одном плоском словаре результата. None-поля для пустого входа.
    """
    n = len(values)
    if n == 0:
        return {
            f"{prefix}n": 0,
            f"{prefix}mean_ms": None, f"{prefix}p50_ms": None,
            f"{prefix}p95_ms": None, f"{prefix}p99_ms": None,
            f"{prefix}max_ms": None, f"{prefix}dropped": None,
            f"{prefix}dropped_pct": None,
        }
    dropped = sum(1 for v in values if v > budget_ms)
    return {
        f"{prefix}n": n,
        f"{prefix}mean_ms": round(sum(values) / n, 2),
        f"{prefix}p50_ms": round(percentile(values, 50), 2),
        f"{prefix}p95_ms": round(percentile(values, 95), 2),
        f"{prefix}p99_ms": round(percentile(values, 99), 2),
        f"{prefix}max_ms": round(max(values), 2),
        f"{prefix}dropped": dropped,
        f"{prefix}dropped_pct": round(dropped / n * 100.0, 1),
    }


def latency_for_events(send_walls: list[float], frame_walls: list[float]) -> list[float]:
    """input→repaint латентность (мс) на каждое событие.

    Для каждого настенного времени отправки события берём ПЕРВЫЙ кадр,
    прибывший строго позже — это и есть первый показанный после ввода кадр.
    События без последующего кадра (окно закрылось / кадр не отрисован)
    отбрасываются. Обе последовательности в секундах perf_counter().
    """
    frames = sorted(frame_walls)
    out: list[float] = []
    for t0 in send_walls:
        idx = bisect.bisect_right(frames, t0)
        if idx < len(frames):
            out.append((frames[idx] - t0) * 1000.0)
    return out


# ── MCP-клиент и читатель кадрового лога ─────────────────────────────────────

def free_port() -> int:
    """Свободный TCP-порт на 127.0.0.1 (тот же приём, что scroll_perf.py)."""
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


class FrameReader:
    """Фоновый поток: читает stderr дочернего процесса, стамповывает `[frame]`.

    Дренаж пайпа обязателен (без него ~4 КБ буфера → ложный дедлок процесса,
    см. reference_scroll_bench_harness). Каждую кадровую строку помечаем
    настенным perf_counter() в момент чтения и складываем в потокобезопасный
    список; сырой поток пишем в log-файл для отладки.
    """

    def __init__(self, stream, log_path: Path) -> None:
        self._stream = stream
        self._log = open(log_path, "w", encoding="utf-8", errors="replace")
        self._lock = threading.Lock()
        # (wall_perf_counter, kind, reported_ms) — kind ∈ {"paint","total"}.
        # wall — момент прибытия строки (для латентности); reported_ms — время
        # кадра, как его измерил движок (для распределения кадров).
        self.frames: list[tuple[float, str, float]] = []
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        for raw in self._stream:
            wall = time.perf_counter()
            self._log.write(raw)
            m = TOTAL_RE.search(raw)
            if m:
                with self._lock:
                    self.frames.append((wall, "total", float(m.group(1))))
                continue
            m = PAINT_RE.search(raw)
            if m:
                with self._lock:
                    self.frames.append((wall, "paint", float(m.group(1))))
        self._log.close()

    def snapshot(self) -> list[tuple[float, str, float]]:
        """Копия накопленных кадров (безопасно во время работы потока)."""
        with self._lock:
            return list(self.frames)


# ── Прогон одного workload ───────────────────────────────────────────────────

def _frame_times(frames: list[tuple[float, str, float]]) -> list[float]:
    """Зарепортенные движком времена кадров (мс) выбранного типа.

    Берём `total` (весь UI-тик layout+paint+swap, как в scroll_perf.py); если
    UI-строк нет (сборка печатает только `paint`) — падаем на `paint`. Это НЕ
    интервал прибытия строк (тот задан паузой между событиями), а время,
    измеренное самим движком в строке `[frame] … XX.XXms`.
    """
    kind = "total" if any(k == "total" for _, k, _ in frames) else "paint"
    return [ms for _, k, ms in frames if k == kind]


def _all_frame_walls(frames: list[tuple[float, str, float]]) -> list[float]:
    """Настенные времена ЛЮБЫХ кадровых строк (для сопоставления с событиями)."""
    return sorted(w for w, _, _ in frames)


def _events_for(kind: str, ticks: int, delta: float) -> list[tuple[str, dict]]:
    """Список (имя-инструмента, аргументы) для одного workload."""
    if kind == "scroll":
        # Вниз и обратно вверх — как scroll_perf.py.
        evs = []
        for direction in (+1, -1):
            for _ in range(ticks):
                evs.append(("scroll", {"target": {"selector": "body"},
                                       "delta": {"x": 0, "y": direction * delta}}))
        return evs
    if kind == "click":
        return [("click", {"target": {"selector": "#btn"}}) for _ in range(ticks * 2)]
    if kind == "type":
        # Посимвольно — одна клавиша на вызов, чтобы снять латентность нажатия.
        text = ("lumen input latency " * 4)[: ticks * 2]
        return [("type", {"target": {"selector": "#inp"}, "text": ch}) for ch in text]
    raise ValueError(f"неизвестный workload: {kind}")


def run_workload(
    client: Client, reader: FrameReader, kind: str,
    ticks: int, delta: float, budget_ms: float, gap_s: float,
) -> dict:
    """Прогнать один вид ввода, вернуть три распределения (мс):

      frame_*   — время кадра, как его меряет движок (плавность, хвост важнее);
      ack_*     — round-trip MCP-вызова = стоимость диспатча события на UI-потоке
                  (плотный сигнал: одно значение на событие, есть всегда);
      latency_* — от отправки события до первой кадровой строки (input→repaint,
                  конец-в-конец; плотно для scroll, разрежённо для click/type,
                  т.к. дискретный ввод коалесит перерисовки — см. docs/perf/input.md).
    """
    frames_before = len(reader.snapshot())
    send_walls: list[float] = []
    ack_ms: list[float] = []
    t_start = time.perf_counter()

    for name, arguments in _events_for(kind, ticks, delta):
        t0 = time.perf_counter()
        send_walls.append(t0)
        client.call(name, arguments)
        ack_ms.append((time.perf_counter() - t0) * 1000.0)
        time.sleep(gap_s)

    # Дать последнему кадру дорисоваться и строке долететь до нас.
    time.sleep(0.3)
    wall = round(time.perf_counter() - t_start, 2)

    frames = reader.snapshot()[frames_before:]
    frame_times = _frame_times(frames)
    latencies = latency_for_events(send_walls, _all_frame_walls(frames))

    rec = {"workload": kind, "events": len(send_walls),
           "wall_s": wall, "frames": len(frames)}
    rec.update(distribution(frame_times, budget_ms, "frame_"))
    rec.update(distribution(ack_ms, budget_ms, "ack_"))
    rec.update(distribution(latencies, budget_ms, "latency_"))
    return rec


# ── Сборка / поиск бинарника ─────────────────────────────────────────────────

def find_exe(cli_exe: str | None, profile: str) -> Path:
    """Найти lumen.exe: --exe > $LUMEN_EXE > target/{profile,dev-release,release}."""
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


# ── Отчёт / сравнение ────────────────────────────────────────────────────────

def _fmt(v: object) -> str:
    return "—" if v is None else str(v)


def summary_md(result: dict) -> str:
    """Markdown-сводка: строка на workload, кадры и латентность рядом."""
    lines = [
        f"# Input-perf: {result['page']}",
        "",
        f"- Бинарь: `{result['exe']}` (живое окно, LUMEN_FRAME_LOG=1)",
        f"- Коммит движка: `{result['commit']}`  ·  бюджет кадра: {result['budget_ms']} мс",
        "- Всё в мс; dropped — кадры длиннее бюджета; ack — round-trip диспатча,"
        " lat(n) — репейнт-латентность (n событий, у click/type мало из-за коалесинга)",
        "",
        "| workload | events | frames | frame p50 | frame p95 | frame p99 | frame max | dropped | ack p95 | lat p95 | lat n |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for r in result["workloads"]:
        lines.append(
            "| " + " | ".join(_fmt(x) for x in (
                r["workload"], r["events"], r["frames"],
                r.get("frame_p50_ms"), r.get("frame_p95_ms"),
                r.get("frame_p99_ms"), r.get("frame_max_ms"),
                f'{_fmt(r.get("frame_dropped"))} ({_fmt(r.get("frame_dropped_pct"))}%)',
                r.get("ack_p95_ms"), r.get("latency_p95_ms"), r.get("latency_n"),
            )) + " |"
        )
    return "\n".join(lines) + "\n"


def compare(result: dict, prev_path: Path) -> str:
    """Дельта хвостовых метрик vs предыдущий прогон (та же машина)."""
    prev = json.loads(prev_path.read_text(encoding="utf-8"))
    prev_by = {r["workload"]: r for r in prev.get("workloads", [])}
    lines = [f"\n## Сравнение с {prev_path.name}", ""]
    for key in REGRESSION_KEYS:
        lines += [f"### {key}", "", "| workload | было | стало | Δ% |", "|---|---|---|---|"]
        for r in result["workloads"]:
            p = prev_by.get(r["workload"])
            if not p:
                continue
            was, now = p.get(key), r.get(key)
            if not isinstance(was, (int, float)) or not isinstance(now, (int, float)):
                continue
            delta = f"{(now - was) / was * 100:+.0f}%" if was else "—"
            mark = " ⚠" if was and (now - was) / was > REGRESSION_PCT else ""
            lines.append(f"| {r['workload']} | {was} | {now} | {delta}{mark} |")
        lines.append("")
    return "\n".join(lines) + "\n"


# ── Самопроверка статистики (без сети/браузера, входит в ворота) ──────────────

def selftest() -> int:
    """Проверить percentile / distribution / latency_for_events. 0 = ок."""
    ok = True

    def check(label: str, got: object, want: object) -> None:
        nonlocal ok
        mark = "ok" if got == want else "FAIL"
        if got != want:
            ok = False
        print(f"  {mark:<4} {label:<28} want={want!r:<10} got={got!r}")

    # percentile: 1..10, линейная интерполяция как numpy 'linear'.
    xs = [float(i) for i in range(1, 11)]  # 1..10
    check("percentile p0", percentile(xs, 0), 1.0)
    check("percentile p100", percentile(xs, 100), 10.0)
    check("percentile p50", percentile(xs, 50), 5.5)          # (5+6)/2
    check("percentile p95", round(percentile(xs, 95), 4), 9.55)
    check("percentile single", percentile([7.0], 95), 7.0)
    check("percentile empty", percentile([], 95), None)

    # distribution: бюджет 16.67 → дропы = кадры длиннее бюджета.
    d = distribution([10.0, 20.0, 30.0, 40.0], DEFAULT_BUDGET_MS, "frame_")
    check("dist n", d["frame_n"], 4)
    check("dist mean", d["frame_mean_ms"], 25.0)
    check("dist dropped", d["frame_dropped"], 3)             # 20,30,40 > 16.67
    check("dist dropped_pct", d["frame_dropped_pct"], 75.0)
    check("dist max", d["frame_max_ms"], 40.0)
    empty = distribution([], DEFAULT_BUDGET_MS, "frame_")
    check("dist empty n", empty["frame_n"], 0)
    check("dist empty p95", empty["frame_p95_ms"], None)

    # latency_for_events: событие @t → первый кадр строго позже.
    # send @1.0,2.0,3.0; кадры @1.05,2.10,2.90 → лат 50мс, 100мс, (нет кадра >3.0)
    lat = latency_for_events([1.0, 2.0, 3.0], [1.05, 2.10, 2.90])
    lat = [round(x, 2) for x in lat]
    check("latency events", lat, [50.0, 100.0])
    check("latency no frames", latency_for_events([1.0], []), [])

    print("SELFTEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


# ── main ─────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--page", default=str(DEFAULT_FIXTURE),
                    help="HTML-файл или URL (по умолчанию scripts/perf-fixtures/input.html)")
    ap.add_argument("--workloads", default="scroll,click,type",
                    help="список через запятую из scroll,click,type")
    ap.add_argument("--ticks", type=int, default=25,
                    help="число событий в одну сторону (scroll); click/type берут 2×")
    ap.add_argument("--delta", type=float, default=300.0, help="CSS-пикселей на щелчок скролла")
    ap.add_argument("--gap-ms", type=float, default=30.0, help="пауза между событиями, мс")
    ap.add_argument("--budget-ms", type=float, default=DEFAULT_BUDGET_MS,
                    help=f"бюджет кадра для счётчика дропов (default {DEFAULT_BUDGET_MS:.2f} = 60Гц)")
    ap.add_argument("--profile", default="dev-release", help="cargo-профиль для --build/поиска exe")
    ap.add_argument("--build", action="store_true", help="пересобрать lumen-shell перед прогоном")
    ap.add_argument("--exe", help="путь к lumen.exe (иначе $LUMEN_EXE / target/*)")
    ap.add_argument("--json", help="куда записать результат (JSON); иначе .tmp/input-perf/*")
    ap.add_argument("--compare", help="результат предыдущего прогона (JSON) для дельта-таблицы")
    ap.add_argument("--selftest", action="store_true",
                    help="проверить статистику без сети/браузера и выйти")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    workloads = [w.strip() for w in args.workloads.split(",") if w.strip()]
    for w in workloads:
        if w not in ("scroll", "click", "type"):
            sys.exit(f"неизвестный workload: {w} (допустимо scroll,click,type)")

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
    env.setdefault("LUMEN_FRAME_LOG", "1")

    print(f"Бинарь {exe}\nСтраница {page_url}\nWorkloads: {', '.join(workloads)}  "
          f"(ticks={args.ticks}, gap={args.gap_ms}мс, бюджет={args.budget_ms:.2f}мс)")

    proc = subprocess.Popen(
        [str(exe), "--mcp-live-port", str(port), "about:blank"],
        cwd=str(REPO_ROOT), env=env,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, bufsize=1, text=True,
        encoding="utf-8", errors="replace",
    )
    reader = FrameReader(proc.stderr, stderr_log)
    result: dict = {
        "date": datetime.now().strftime("%Y%m%d-%H%M%S"),
        "commit": commit, "exe": str(exe), "page": page_url,
        "budget_ms": round(args.budget_ms, 2), "workloads": [],
    }
    try:
        c = Client(port)
        c.call("navigate", {"url": page_url})
        c.call("wait", {"condition": "document_ready", "timeout_ms": 20000})
        time.sleep(1.0)  # дать странице дорисоваться / докачать ресурсы
        for w in workloads:
            print(f"  workload {w} ... ", end="", flush=True)
            rec = run_workload(c, reader, w, args.ticks, args.delta,
                               args.budget_ms, args.gap_ms / 1000.0)
            result["workloads"].append(rec)
            print(f"{rec['frames']} кадров, "
                  f"frame p95={_fmt(rec.get('frame_p95_ms'))}мс, "
                  f"ack p95={_fmt(rec.get('ack_p95_ms'))}мс, "
                  f"lat p95={_fmt(rec.get('latency_p95_ms'))}мс (n={_fmt(rec.get('latency_n'))}), "
                  f"dropped {_fmt(rec.get('frame_dropped'))}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()

    if not result["workloads"]:
        print(f"Кадров не собрано — смотрите {stderr_log}", file=sys.stderr)
        sys.exit(1)

    out_json = Path(args.json) if args.json else OUT_ROOT / f"{result['date']}.json"
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(result, ensure_ascii=False, indent=1), encoding="utf-8")

    md = summary_md(result)
    if args.compare:
        md += compare(result, Path(args.compare))
    print("\n" + md)
    print(f"JSON: {out_json}\nСырой лог кадров: {stderr_log}")


if __name__ == "__main__":
    main()
