#!/usr/bin/env python3
"""Перф-аудит корпуса реальных сайтов (дорожка PERF, ROADMAP.md).

Режим по умолчанию — ЖИВОЙ: один запуск lumen.exe с GUI-окном
(`--mcp-live-port`), каждый сайт корпуса открывается в НОВОЙ вкладке
(MCP-инструмент `new_tab` — вкладка становится активной; к концу
прогона в окне 14 вкладок, как в реальной сессии) — видно, как рендерится
каждый сайт (визуальный анализ), а RAM одного процесса замеряется
кумулятивно по мере накопления вкладок. На сайт собираются:
время до document_ready, RAM тек/пик, JS-ошибки консоли, CPU-скриншот
(resource://screenshot). Зависший сайт фиксируется как TIMEOUT, мёртвое
окно — как DEAD с перезапуском браузера (сам перезапуск — находка).

Режим --phases — headless-разложение по фазам: три замера на сайт
одним бинарём:

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
  python scripts/perf_audit.py                          # живой прогон корпуса с GUI
  python scripts/perf_audit.py --dwell 8                # дольше показывать каждый сайт
  python scripts/perf_audit.py --phases --only lenta    # headless-разложение по фазам
  python scripts/perf_audit.py --compare docs/perf/runs/2026-07-17.json
  LUMEN_EXE=path/to/lumen.exe python scripts/perf_audit.py
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import socket
import struct
import subprocess
import sys
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
        "error_lines": error_lines[:8],
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

    keys = ("wall_s", "rc", "timed_out", "stdout_bytes", "error_lines", "peak_mb", "cpu_s")

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
    return rec


# ── Живой режим (GUI-окно, один процесс на весь корпус) ──────────────────────

def _free_port() -> int:
    """Свободный локальный TCP-порт (bind-and-release)."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Mcp:
    """Line-delimited JSON-RPC клиент к `--mcp-live-port` (паттерн scripts/scroll_perf.py)."""

    def __init__(self, port: int, timeout: float) -> None:
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


class LiveBrowser:
    """Одно GUI-окно lumen на весь прогон + перезапуск при смерти."""

    def __init__(self, exe: Path, out_dir: Path, timeout: float) -> None:
        self.exe, self.out_dir, self.timeout = exe, out_dir, timeout
        self.restarts = 0
        self._spawn()

    def _spawn(self) -> None:
        port = _free_port()
        self.log_path = self.out_dir / f"live.stderr.{self.restarts}.log"
        log = self.log_path.open("wb")
        self.proc = subprocess.Popen(
            [str(self.exe), "--mcp-live-port", str(port), "about:blank"],
            stdout=subprocess.DEVNULL, stderr=log, cwd=str(REPO_ROOT),
        )
        self.mcp = Mcp(port, self.timeout)

    def stderr_errors_since(self, pos: int) -> tuple[list[str], int]:
        """Ошибко-подобные строки stderr с позиции pos (per-site атрибуция) + новая позиция."""
        try:
            with self.log_path.open("rb") as f:
                f.seek(pos)
                chunk = f.read()
        except OSError:
            return [], pos
        lines = []
        for ln in chunk.decode("utf-8", errors="replace").splitlines():
            if ERROR_RE.search(ln) and ln.strip() not in lines:
                lines.append(ln.strip())
        return lines[:8], pos + len(chunk)

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
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            self.proc.kill()


def audit_site_live(
    br: LiveBrowser, slug: str, url: str, out_dir: Path, timeout: int, dwell: float, scroll_ticks: int
) -> dict:
    """Один сайт в живом окне: навигация → готовность → скролл → скриншот → консоль → RAM."""
    rec: dict = {"slug": slug, "url": url, "restarted": False}
    log_pos = br.log_path.stat().st_size if br.log_path.exists() else 0
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
        rec["status"] = "OK"
    except RuntimeError as e:  # error-ответ (чаще всего таймаут wait)
        rec["status"] = "TIMEOUT"
        rec["error"] = str(e)[:200]
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
                br.mcp.tool("scroll", {"target": {"css": "body"}, "delta": {"x": 0, "y": direction * 600}})
                time.sleep(0.15)

        contents = br.mcp.resource("resource://screenshot")
        if contents and contents[0].get("data"):
            png = out_dir / f"{slug}.png"
            png.write_bytes(base64.b64decode(contents[0]["data"]))
            rec["png_size"] = png_size(png)

        console = br.mcp.resource("resource://console")
        entries = json.loads(console[0].get("text", "[]")) if console else []
        errors = [e.get("message", "") for e in entries if e.get("level") == "Error"]
        rec["console_total"] = len(entries)
        rec["console_errors"] = errors[:8]

        rec.update(_win_proc_stats(br.proc))
    except (OSError, RuntimeError, json.JSONDecodeError, socket.timeout) as e:
        rec.setdefault("error", str(e)[:200])
        if br.proc.poll() is not None or isinstance(e, OSError):
            br.restart()
            rec["restarted"] = True
    rec["stderr_errors"], _ = br.stderr_errors_since(log_pos)
    return rec


def summary_md_live(results: list[dict], exe: Path, commit: str, restarts: int) -> str:
    """Markdown-сводка живого прогона."""
    lines = [
        f"# Перф-аудит (живое окно): {len(results)} сайтов, перезапусков: {restarts}",
        "",
        f"- Бинарь: `{exe}` (GUI, один процесс, дефолтный рендер-бэкенд)",
        f"- Коммит движка: `{commit}`",
        "",
        "| slug | статус | готовность, с | RAM тек, МБ | RAM пик, МБ | JS-ошибки | первая ошибка |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in results:
        all_errs = (r.get("console_errors") or []) + (r.get("stderr_errors") or [])
        err = all_errs[0][:60] if all_errs else r.get("error", "")
        restarted = " ↻" if r["restarted"] else ""
        if r.get("own_tab") is False:
            restarted += " (без вкладки)"
        lines.append(
            f"| {r['slug']} | {r['status']}{restarted} | {r.get('ready_s', '—')} "
            f"| {r.get('cur_mb', '—')} | {r.get('peak_mb', '—')} "
            f"| {len(all_errs) or ''} | {err} |"
        )
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


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--corpus", default=str(DEFAULT_CORPUS), help="файл корпуса (slug url)")
    ap.add_argument("--only", action="append", default=[], help="фильтр по подстроке slug (повторяемый)")
    ap.add_argument("--exe", help="путь к lumen.exe (иначе $LUMEN_EXE / target/*)")
    ap.add_argument("--timeout", type=int, default=240, help="таймаут одной стадии/навигации, с (default 240)")
    ap.add_argument("--compare", help="results.json предыдущего прогона для дельта-таблицы")
    ap.add_argument("--phases", action="store_true", help="headless-разложение по фазам вместо живого окна")
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

    mode = "phases" if args.phases else "live"
    print(f"Аудит {len(sites)} сайтов ({mode}), бинарь {exe}, таймаут {args.timeout}с")
    print(f"Результаты: {out_dir}")
    results = []
    br = None if args.phases else LiveBrowser(exe, out_dir, args.timeout)

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

    try:
        for i, (slug, url) in enumerate(sites, 1):
            print(f"[{i}/{len(sites)}] {slug} {url} ... ", end="", flush=True)
            if args.phases:
                rec = audit_site(exe, slug, url, out_dir, args.timeout)
                note = f"total={rec['screenshot']['wall_s']}s dominant={dominant_phase(rec)}"
            else:
                rec = audit_site_live(br, slug, url, out_dir, args.timeout, args.dwell, args.scroll_ticks)
                n_err = len(rec.get('console_errors', [])) + len(rec.get('stderr_errors', []))
                note = (f"ready={rec.get('ready_s', '—')}s ram={rec.get('cur_mb', '—')}MB "
                        f"err={n_err}"
                        + (" RESTARTED" if rec["restarted"] else ""))
            results.append(rec)
            print(f"{rec['status']} {note}")
            flush_results()
    finally:
        if br is not None:
            br.close()

    md = (summary_md(results, exe, commit) if args.phases
          else summary_md_live(results, exe, commit, br.restarts))
    if args.compare:
        md += compare(results, Path(args.compare))
    (out_dir / "summary.md").write_text(md, encoding="utf-8")
    print("\n" + md)
    failed = [r["slug"] for r in results if r["status"] != "OK"]
    print(f"Готово: {len(results) - len(failed)}/{len(results)} OK" + (f", проблемы: {', '.join(failed)}" if failed else ""))


if __name__ == "__main__":
    main()
