#!/usr/bin/env python3
"""User-centric перф-метрики на корпусе реальных сайтов (дорожка PERF, PERF-2).

Извлекает метрики «по образцу» FCP / LCP / TTI + Total Blocking Time из
таймлайна одной навигации, который уже собирает `lumen --trace-nav`
(PERF-1, Chrome Trace Event Format). Никакого нового кода в движке — харнесс
прогоняет headless `--trace-nav` по каждому сайту корпуса и раскладывает
готовые спаны в метрики, ведёт журнал «метрика ↔ коммит» (модель
docs/build-speed.md) и ловит перф-регрессии так же, как graphic_tests ловят
пиксельные.

Метрики (все в мс от старта навигации; извлекаются из спанов PERF-1
`navigation → fetch-document → parse-html → run-scripts → layout → paint`
+ инстант `first-paint`):

  ttfb_ms       — конец спана `fetch-document` (документ получен целиком;
                  прокси Time To First Byte — сеть не отдаёт под-тайминги)
  html_parse_ms — длительность `parse-html`
  script_ms     — длительность `run-scripts` (исполнение JS на UI-потоке)
  layout_ms     — длительность `layout` (каскад + бокс-дерево)
  paint_ms      — длительность `paint` (CPU-растеризация — известный тормоз)
  fcp_ms        — инстант `first-paint` (прокси First Contentful Paint)
  lcp_ms        — конец спана `paint` (прокси Largest Contentful Paint)
  tti_ms        — max(fcp, конец `run-scripts`) (прокси Time To Interactive)
  tbt_ms        — Σ max(0, task_ms − 50) по main-thread script-задачам
                  (прокси Total Blocking Time)
  nav_ms        — длительность корневого спана `navigation` (полная загрузка)
  doc_bytes / total_bytes / resources — детерминированные байтовые/ресурсные
                  счётчики (устойчивый сигнал регрессии, не зависит от сети)

ВАЖНО про headless-путь: `--trace-nav` красит страницу ОДИН раз в конце (тот же
CPU-путь, что `--screenshot`), поэтому fcp/lcp/tti схлопываются к моменту
завершения единственного paint — распределить их как в живом инкрементальном
рендере headless-путь не может (то же ограничение, что зафиксировано в
docs/perf/journal.md: headless paint ≠ живое окно). Реально различимые и
действенные числа этого пути — ttfb / html_parse / script(tbt) / layout / paint
и байтовые счётчики; fcp/lcp/tti даются «по образцу» для полноты и как якорь
общего времени. Живые FCP/LCP/TTI появятся, когда PerformanceObserver
paint-timing будет дописан в движке (JS-каркас частично готов: performance.now,
long-animation-frames, soft-navigation в crates/js).

Сравнивать числа можно только между прогонами ОДНОЙ машины; ±20% на сетевых
фазах — шум, >20% на той же машине — находка (порог как в perf_audit.py).

Примеры:
  python scripts/perf_metrics.py                       # весь корпус, 1 прогон
  python scripts/perf_metrics.py --only lenta --repeat 3
  python scripts/perf_metrics.py samples/page.html     # разовый URL/файл
  python scripts/perf_metrics.py --compare docs/perf/metrics-runs/2026-07-18.json
  python scripts/perf_metrics.py --selftest            # проверка извлечения без сети
  LUMEN_EXE=path/to/lumen.exe python scripts/perf_metrics.py
"""

from __future__ import annotations

import argparse
import json
import os
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
OUT_ROOT = REPO_ROOT / ".tmp" / "perf-metrics"

# Порядок метрик в сводке/сравнении (ключ → заголовок колонки)
METRIC_COLUMNS = [
    ("ttfb_ms", "ttfb"),
    ("fcp_ms", "fcp"),
    ("lcp_ms", "lcp"),
    ("tti_ms", "tti"),
    ("tbt_ms", "tbt"),
    ("script_ms", "script"),
    ("layout_ms", "layout"),
    ("paint_ms", "paint"),
    ("nav_ms", "nav"),
]
# Метрика, по которой считаем регрессию в --compare (общее время загрузки)
REGRESSION_KEY = "nav_ms"
# Порог блокирующей задачи для TBT (мс): вклад = max(0, task − 50), как в Lighthouse
BLOCKING_THRESHOLD_MS = 50.0


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


# ── Извлечение метрик из Chrome-trace (чистая функция, тестируется --selftest) ──

def _by_name(events: list[dict], name: str) -> list[dict]:
    """Все complete-спаны (ph='X') с заданным именем."""
    return [e for e in events if e.get("ph") == "X" and e.get("name") == name]


def _span_end(events: list[dict], name: str) -> float | None:
    """Конец (ts+dur, мкс) последнего спана с этим именем, либо None."""
    spans = _by_name(events, name)
    if not spans:
        return None
    return max(e["ts"] + e.get("dur", 0.0) for e in spans)


def _span_dur(events: list[dict], name: str) -> float | None:
    """Суммарная длительность (мкс) всех спанов с этим именем, либо None."""
    spans = _by_name(events, name)
    if not spans:
        return None
    return sum(e.get("dur", 0.0) for e in spans)


def metrics_from_trace(trace: dict) -> dict:
    """Разложить Chrome-trace одной навигации в user-centric метрики (мс).

    Чистая функция над разобранным JSON `--trace-nav` — вся логика извлечения
    здесь, чтобы её можно было проверить на синтетическом трейсе (--selftest)
    без запуска браузера и без сети.
    """
    events = trace.get("traceEvents", [])
    # Origin навигации: старт корневого спана (trace::enable зовётся прямо
    # перед ним, так что ts≈0, но не завязываемся на это).
    nav_spans = _by_name(events, "navigation")
    origin = min((e["ts"] for e in nav_spans), default=0.0)
    if not nav_spans:
        # Битый/пустой трейс без корневого спана — origin по минимальному ts
        origin = min((e["ts"] for e in events), default=0.0)

    def rel_ms(ts_us: float | None) -> float | None:
        """Смещение от origin в мс (округл. до 0.1)."""
        return None if ts_us is None else round((ts_us - origin) / 1000.0, 1)

    def dur_ms(dur_us: float | None) -> float | None:
        """Длительность в мс (округл. до 0.1)."""
        return None if dur_us is None else round(dur_us / 1000.0, 1)

    # first-paint — инстант (ph='i')
    fp_instants = [e for e in events if e.get("ph") == "i" and e.get("name") == "first-paint"]
    fcp_us = min((e["ts"] for e in fp_instants), default=None)

    ttfb = rel_ms(_span_end(events, "fetch-document"))
    fcp = rel_ms(fcp_us)
    lcp = rel_ms(_span_end(events, "paint"))
    script_end = _span_end(events, "run-scripts")
    # TTI ≈ момент, когда и первый paint случился, и JS-задачи на UI-потоке
    # отработали. В headless-пути обе точки — конец единственного paint.
    tti_candidates = [v for v in (fcp_us, script_end) if v is not None]
    tti = rel_ms(max(tti_candidates)) if tti_candidates else None

    # Total Blocking Time: сумма «сверх 50 мс» по main-thread script-задачам.
    # Единственный такой спан пути — `run-scripts`; берём все на всякий случай.
    tbt_us = sum(
        max(0.0, e.get("dur", 0.0) - BLOCKING_THRESHOLD_MS * 1000.0)
        for e in _by_name(events, "run-scripts")
    )
    tbt = round(tbt_us / 1000.0, 1) if _by_name(events, "run-scripts") else None

    # Байтовые/ресурсные счётчики — детерминированный, не сетевой сигнал.
    net_names = ("GET ", "img ", "css ", "script ")
    resource_spans = [
        e for e in events
        if e.get("ph") == "X" and any(str(e.get("name", "")).startswith(p) for p in net_names)
    ]
    total_bytes = sum(
        int(e.get("args", {}).get("size", 0)) for e in resource_spans
    )
    doc_span = next((e for e in resource_spans if str(e["name"]).startswith("GET ")), None)
    doc_bytes = int(doc_span["args"].get("size", 0)) if doc_span and "args" in doc_span else None

    return {
        "ttfb_ms": ttfb,
        "html_parse_ms": dur_ms(_span_dur(events, "parse-html")),
        "script_ms": dur_ms(_span_dur(events, "run-scripts")),
        "layout_ms": dur_ms(_span_dur(events, "layout")),
        "paint_ms": dur_ms(_span_dur(events, "paint")),
        "fcp_ms": fcp,
        "lcp_ms": lcp,
        "tti_ms": tti,
        "tbt_ms": tbt,
        "nav_ms": dur_ms(_span_dur(events, "navigation")),
        "doc_bytes": doc_bytes,
        "total_bytes": total_bytes or None,
        "resources": len(resource_spans) or None,
    }


def _median(values: list[float]) -> float:
    """Медиана непустого списка (без numpy)."""
    s = sorted(values)
    n = len(s)
    mid = n // 2
    return s[mid] if n % 2 else (s[mid - 1] + s[mid]) / 2.0


def merge_repeats(runs: list[dict]) -> dict:
    """Схлопнуть N прогонов одного сайта в медиану по каждой метрике.

    Медиана устойчивее среднего к разовому сетевому выбросу (протокол
    docs/build-speed.md: несколько прогонов, брать медиану).
    """
    if len(runs) == 1:
        return runs[0]
    merged: dict = {}
    keys = set().union(*(r.keys() for r in runs))
    for k in keys:
        vals = [r[k] for r in runs if isinstance(r.get(k), (int, float))]
        merged[k] = round(_median(vals), 1) if vals else None
    return merged


# ── Прогон одного сайта ──────────────────────────────────────────────────────

def run_trace(exe: Path, url: str, out_json: Path, timeout: int) -> tuple[dict | None, str]:
    """Прогнать `lumen --trace-nav`; вернуть (разобранный trace | None, диагностика)."""
    if out_json.exists():
        out_json.unlink()
    try:
        proc = subprocess.run(
            [str(exe), "--trace-nav", str(out_json), url],
            capture_output=True,
            timeout=timeout,
            cwd=str(REPO_ROOT),
        )
    except subprocess.TimeoutExpired:
        return None, "TIMEOUT"
    if not out_json.exists():
        stderr = proc.stderr.decode("utf-8", errors="replace").strip().splitlines()
        return None, (stderr[-1][:120] if stderr else f"rc={proc.returncode}, трейс не записан")
    try:
        return json.loads(out_json.read_text(encoding="utf-8")), ""
    except (OSError, json.JSONDecodeError) as e:
        return None, f"битый трейс: {e}"


def measure_site(
    exe: Path, slug: str, url: str, out_dir: Path, timeout: int, repeat: int
) -> dict:
    """repeat прогонов одного сайта → медианные метрики + статус."""
    rec: dict = {"slug": slug, "url": url}
    runs: list[dict] = []
    note = ""
    for i in range(repeat):
        trace, err = run_trace(exe, url, out_dir / f"{slug}.{i}.json", timeout)
        if trace is None:
            note = err
            continue
        runs.append(metrics_from_trace(trace))
    if not runs:
        rec["status"] = "TIMEOUT" if note == "TIMEOUT" else "FAIL"
        rec["error"] = note
        return rec
    rec["status"] = "OK"
    rec["samples"] = len(runs)
    rec.update(merge_repeats(runs))
    return rec


# ── Сводка / сравнение ───────────────────────────────────────────────────────

def _fmt(v: object) -> str:
    """Ячейка таблицы: число как есть, None → «—»."""
    return "—" if v is None else str(v)


def summary_md(results: list[dict], exe: Path, commit: str, repeat: int) -> str:
    """Markdown-сводка прогона (метрики в мс)."""
    header = " | ".join(title for _, title in METRIC_COLUMNS)
    sep = "|".join(["---"] * (len(METRIC_COLUMNS) + 2))
    lines = [
        f"# Перф-метрики: {len(results)} сайтов (медиана из {repeat})",
        "",
        f"- Бинарь: `{exe}` (headless --trace-nav, CPU-путь)",
        f"- Коммит движка: `{commit}`",
        "- Все значения в мс от старта навигации; см. шапку scripts/perf_metrics.py",
        "",
        f"| slug | статус | {header} |",
        f"|{sep}|",
    ]
    for r in results:
        cells = " | ".join(_fmt(r.get(key)) for key, _ in METRIC_COLUMNS)
        lines.append(f"| {r['slug']} | {r['status']} | {cells} |")
    return "\n".join(lines) + "\n"


def compare(results: list[dict], prev_path: Path) -> str:
    """Дельта ключевой метрики (nav_ms) vs предыдущий прогон (та же машина)."""
    prev = {r["slug"]: r for r in json.loads(prev_path.read_text(encoding="utf-8"))["results"]}
    lines = [
        f"\n## Сравнение с {prev_path.name} (метрика {REGRESSION_KEY})",
        "",
        "| slug | было | стало | Δ% |",
        "|---|---|---|---|",
    ]
    for r in results:
        p = prev.get(r["slug"])
        if not p:
            continue
        was, now = p.get(REGRESSION_KEY), r.get(REGRESSION_KEY)
        if not isinstance(was, (int, float)) or not isinstance(now, (int, float)):
            continue
        delta = f"{(now - was) / was * 100:+.0f}%" if was else "—"
        mark = " ⚠" if was and (now - was) / was > 0.20 else ""
        lines.append(f"| {r['slug']} | {was} | {now} | {delta}{mark} |")
    return "\n".join(lines) + "\n"


# ── Самопроверка извлечения (без сети/браузера) ──────────────────────────────

def _synthetic_trace() -> dict:
    """Правдоподобный трейс одной навигации для проверки metrics_from_trace."""
    def ev(name, cat, ph, ts, dur=0.0, size=None, tid=0):
        e = {"name": name, "cat": cat, "ph": ph, "ts": ts, "tid": tid, "pid": 1}
        if ph == "X":
            e["dur"] = dur
        if size is not None:
            e["args"] = {"size": size}
        return e

    # ts/dur в мкс. navigation 0..300мс; document получен к 50мс; run-scripts
    # 120мс (блокирующая задача); layout 20мс; paint 100мс; first-paint @280мс.
    return {
        "traceEvents": [
            ev("navigation", "nav", "X", 0, 300_000),
            ev("fetch-document", "net", "X", 0, 50_000),
            ev("GET https://x/", "net", "X", 5_000, 45_000, size=12000),
            ev("parse-html", "parse", "X", 50_000, 10_000),
            ev("run-scripts", "script", "X", 60_000, 120_000),
            ev("script https://x/a.js", "net", "X", 55_000, 4_000, size=8000, tid=2),
            ev("layout", "layout", "X", 180_000, 20_000),
            ev("paint", "paint", "X", 200_000, 100_000),
            ev("first-paint", "paint", "i", 280_000),
        ]
    }


def selftest() -> int:
    """Проверить metrics_from_trace на синтетическом трейсе. 0 = ок."""
    m = metrics_from_trace(_synthetic_trace())
    expected = {
        "ttfb_ms": 50.0,        # конец fetch-document
        "html_parse_ms": 10.0,
        "script_ms": 120.0,
        "layout_ms": 20.0,
        "paint_ms": 100.0,
        "fcp_ms": 280.0,        # инстант first-paint
        "lcp_ms": 300.0,        # конец paint (200+100)
        "tti_ms": 280.0,        # max(fcp=280, конец run-scripts=180)
        "tbt_ms": 70.0,         # 120 − 50
        "nav_ms": 300.0,
        "doc_bytes": 12000,
        "total_bytes": 20000,   # 12000 + 8000
        "resources": 2,         # GET + script
    }
    ok = True
    for k, want in expected.items():
        got = m.get(k)
        mark = "ok" if got == want else "FAIL"
        if got != want:
            ok = False
        print(f"  {mark:<4} {k:<14} want={want!r:<10} got={got!r}")
    # Пустой трейс не должен падать
    empty = metrics_from_trace({"traceEvents": []})
    if any(v is not None for v in empty.values()):
        print("  FAIL empty-trace -> все метрики должны быть None:", empty)
        ok = False
    else:
        print("  ok   empty-trace -> все None")
    print("SELFTEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("target", nargs="?", help="разовый URL/файл вместо корпуса")
    ap.add_argument("--corpus", default=str(DEFAULT_CORPUS), help="файл корпуса (slug url)")
    ap.add_argument("--only", action="append", default=[], help="фильтр по подстроке slug (повторяемый)")
    ap.add_argument("--exe", help="путь к lumen.exe (иначе $LUMEN_EXE / target/*)")
    ap.add_argument("--timeout", type=int, default=240, help="таймаут одной навигации, с (default 240)")
    ap.add_argument("--repeat", type=int, default=1, help="прогонов на сайт, берётся медиана (default 1)")
    ap.add_argument("--compare", help="results.json предыдущего прогона для дельта-таблицы")
    ap.add_argument("--selftest", action="store_true", help="проверить извлечение метрик без сети и выйти")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    exe = find_exe(args.exe)
    if args.target:
        sites = [("target", args.target)]
    else:
        sites = load_corpus(Path(args.corpus), args.only)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = OUT_ROOT / stamp
    out_dir.mkdir(parents=True, exist_ok=True)
    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, cwd=str(REPO_ROOT)
    ).stdout.strip()

    print(f"Метрики {len(sites)} сайтов, бинарь {exe}, repeat={args.repeat}, таймаут {args.timeout}с")
    print(f"Результаты: {out_dir}")
    results = []

    def flush() -> None:
        """Промежуточное сохранение — падение на N-м сайте не теряет предыдущие."""
        (out_dir / "results.json").write_text(
            json.dumps(
                {"date": stamp, "commit": commit, "exe": str(exe),
                 "repeat": args.repeat, "timeout_s": args.timeout, "results": results},
                ensure_ascii=False,
                indent=1,
            ),
            encoding="utf-8",
        )

    for i, (slug, url) in enumerate(sites, 1):
        print(f"[{i}/{len(sites)}] {slug} {url} ... ", end="", flush=True)
        t0 = time.monotonic()
        rec = measure_site(exe, slug, url, out_dir, args.timeout, args.repeat)
        wall = round(time.monotonic() - t0, 1)
        if rec["status"] == "OK":
            note = " ".join(
                f"{title}={_fmt(rec.get(key))}" for key, title in METRIC_COLUMNS
            )
        else:
            note = rec.get("error", "")
        print(f"{rec['status']} ({wall}s) {note}")
        results.append(rec)
        flush()

    md = summary_md(results, exe, commit, args.repeat)
    if args.compare:
        md += compare(results, Path(args.compare))
    (out_dir / "summary.md").write_text(md, encoding="utf-8")
    print("\n" + md)
    failed = [r["slug"] for r in results if r["status"] != "OK"]
    print(f"Готово: {len(results) - len(failed)}/{len(results)} OK"
          + (f", проблемы: {', '.join(failed)}" if failed else ""))


if __name__ == "__main__":
    main()
