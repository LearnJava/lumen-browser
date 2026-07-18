#!/usr/bin/env python3
"""Session-health report over `health.log` (дорожка PERF, PERF-6).

Читает локальный приватный «журнал здоровья сессии», который движок пишет под
флагом `--health-log` / `--activity-log` / `LUMEN_HEALTH_LOG=1`
(`crates/shell/src/health_log.rs`), и превращает поток проблем в
приоритизацию P3-багфиксов ПО ЧАСТОТЕ ВСТРЕЧАЕМОСТИ в реальном браузинге —
а не по случайным находкам. Аналог того, как graphic_tests ловят пиксельные
регрессии, только здесь единица — «сколько раз это укусило пользователя».

Никакого нового кода в движке для самого отчёта: движок эмитит сырые записи
JSON Lines, скрипт агрегирует. Каждая строка `health.log` — самодостаточный
JSON-объект с полем `kind`:

  panic          — паника Rust где угодно в процессе (message + backtrace +
                   страница, открытая в момент падения)
  console_error  — вызов `console.error(...)` самой страницы (сайт багует)
  load_error     — навигация, которая вообще не загрузилась (сеть/TLS/декод)
  broken_render  — страница загрузилась, но НИЧЕГО не отрисовала при
                   содержательном DOM (эвристика белого экрана)

Приоритизация: проблемы группируются в «сигнатуры» (kind + хост + нормализ.
текст) и ранжируются по числу повторов; отдельно даётся сводка «самые
проблемные хосты». Паника весит больше console.error — краш важнее.

Приватность: всё остаётся на машине (принцип privacy.md). Скрипт только
читает локальный файл, ничего не отправляет.

Примеры:
  python scripts/health_report.py                    # отчёт по ./health.log
  python scripts/health_report.py path/to/health.log
  python scripts/health_report.py --top 20
  python scripts/health_report.py --json             # машинный вывод
  python scripts/health_report.py --kind panic       # только паники
  python scripts/health_report.py --selftest         # проверка без браузера (в ворота)
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from urllib.parse import urlparse

# Веса вклада каждого типа проблемы в «оценку проблемности хоста».
# Краш важнее ошибки консоли, белый экран — тоже серьёзно.
KIND_WEIGHT = {
    "panic": 10,
    "broken_render": 5,
    "load_error": 3,
    "console_error": 1,
}

# Типы записей, которые считаются проблемами (session_start и пр. игнорируются).
PROBLEM_KINDS = set(KIND_WEIGHT)


def host_of(url: str) -> str:
    """Хост из URL для группировки. file:// → имя файла; мусор → '(unknown)'.

    Порт отбрасывается, регистр нормализуется — https://Ex.com:443/a и
    http://ex.com/b группируются в один хост `ex.com`.
    """
    if not url:
        return "(unknown)"
    parsed = urlparse(url)
    if parsed.scheme in ("http", "https") and parsed.hostname:
        return parsed.hostname.lower()
    if parsed.scheme == "file":
        # Локальный файл — берём имя файла как «хост», чтобы группировать по странице.
        tail = parsed.path.rsplit("/", 1)[-1]
        return f"file:{tail}" if tail else "file:(root)"
    # Не-URL источник (about:blank, startup, произвольная строка описания).
    return url.split("/", 1)[0][:60] or "(unknown)"


def normalize_detail(kind: str, detail: str) -> str:
    """Свернуть деталь в стабильную сигнатуру, чтобы близкие ошибки группировались.

    Цифры → `#`, кавычки-в-URL и длинные пути схлопываются: 'line 42' и
    'line 88' становятся одной сигнатурой 'line #'. Пустая деталь → '(none)'.
    """
    if not detail:
        return "(none)"
    s = detail.strip()
    # Схлопнуть URL целиком (они шумят и различаются по query).
    s = re.sub(r"https?://\S+", "<url>", s)
    s = re.sub(r"file://\S+", "<file>", s)
    # Числа → '#'. Шестнадцатеричные адреса бэктрейса тоже.
    s = re.sub(r"0x[0-9a-fA-F]+", "<addr>", s)
    s = re.sub(r"\d+", "#", s)
    s = re.sub(r"\s+", " ", s)
    # Для паники берём только первую строку message (без бэктрейса — он в отдельном поле).
    if kind == "panic":
        s = s.split("\\n", 1)[0]
    return s[:160]


def parse_health_log(text: str) -> list[dict]:
    """Разобрать содержимое health.log (JSON Lines). Битые строки пропускаются."""
    records: list[dict] = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict) and "kind" in obj:
            records.append(obj)
    return records


def aggregate(records: list[dict], kind_filter: str | None = None) -> dict:
    """Свести записи в сигнатуры и хостовую сводку."""
    kind_counts: Counter[str] = Counter()
    host_score: defaultdict[str, float] = defaultdict(float)
    host_kind: defaultdict[str, Counter] = defaultdict(Counter)
    # Сигнатура: (kind, host, normalized_detail) -> [count, sample_detail, sample_url]
    sig: dict[tuple, dict] = {}

    for rec in records:
        kind = rec.get("kind", "")
        if kind not in PROBLEM_KINDS:
            continue
        if kind_filter and kind != kind_filter:
            continue
        url = rec.get("url", "") or ""
        host = host_of(url)
        detail = rec.get("detail", "") or ""
        kind_counts[kind] += 1
        host_score[host] += KIND_WEIGHT.get(kind, 1)
        host_kind[host][kind] += 1

        key = (kind, host, normalize_detail(kind, detail))
        entry = sig.get(key)
        if entry is None:
            sig[key] = {
                "kind": kind,
                "host": host,
                "signature": key[2],
                "count": 1,
                "sample_detail": detail,
                "sample_url": url,
                "weight": KIND_WEIGHT.get(kind, 1),
            }
        else:
            entry["count"] += 1

    signatures = sorted(
        sig.values(),
        key=lambda e: (e["count"] * e["weight"], e["count"]),
        reverse=True,
    )
    hosts = sorted(
        (
            {
                "host": h,
                "score": host_score[h],
                "kinds": dict(host_kind[h]),
                "total": sum(host_kind[h].values()),
            }
            for h in host_score
        ),
        key=lambda e: (e["score"], e["total"]),
        reverse=True,
    )
    return {
        "total_problems": sum(kind_counts.values()),
        "by_kind": dict(kind_counts),
        "hosts": hosts,
        "signatures": signatures,
    }


def format_report(agg: dict, top: int) -> str:
    lines: list[str] = []
    total = agg["total_problems"]
    lines.append(f"=== Lumen session-health report — {total} problem event(s) ===")
    if total == 0:
        lines.append("No problems recorded. Clean session.")
        return "\n".join(lines)

    lines.append("")
    lines.append("By kind:")
    for kind in sorted(agg["by_kind"], key=lambda k: -agg["by_kind"][k]):
        lines.append(f"  {kind:<14} {agg['by_kind'][kind]}")

    lines.append("")
    lines.append(f"Most problematic hosts (weighted, top {top}):")
    for h in agg["hosts"][:top]:
        kinds = ", ".join(f"{k}×{v}" for k, v in sorted(h["kinds"].items()))
        lines.append(f"  {h['score']:>6.0f}  {h['host']:<32} {kinds}")

    lines.append("")
    lines.append(f"Top recurring issues (fix these first, top {top}):")
    for i, s in enumerate(agg["signatures"][:top], 1):
        lines.append(
            f"  {i:>2}. [{s['kind']}] {s['host']}  ×{s['count']}"
        )
        detail = (s["sample_detail"] or "").replace("\n", " ")
        if len(detail) > 120:
            detail = detail[:117] + "…"
        if detail:
            lines.append(f"      {detail}")
    return "\n".join(lines)


def run_selftest() -> int:
    """Проверка парсинга/агрегации на синтетическом журнале без браузера."""
    sample = "\n".join(
        [
            '{"kind":"session_start","time":"00:00:00.000","ts_ms":0}',
            '{"kind":"console_error","url":"https://a.com/x","detail":"TypeError at line 42","ts_ms":1}',
            '{"kind":"console_error","url":"https://a.com/y","detail":"TypeError at line 88","ts_ms":2}',
            '{"kind":"console_error","url":"http://a.com:80/z","detail":"other error","ts_ms":3}',
            '{"kind":"broken_render","url":"https://b.com/","detail":"30 DOM nodes but nothing painted","dom_nodes":30,"layout_boxes":3,"rendered_units":0,"ts_ms":4}',
            '{"kind":"panic","url":"https://b.com/","detail":"index out of bounds","location":"foo.rs:10:5","ts_ms":5}',
            "not json — must be skipped",
            '{"no_kind":true}',
        ]
    )
    records = parse_health_log(sample)
    assert len(records) == 6, f"expected 6 parsed records, got {len(records)}"

    agg = aggregate(records)
    assert agg["total_problems"] == 5, agg["total_problems"]
    assert agg["by_kind"]["console_error"] == 3, agg["by_kind"]
    assert agg["by_kind"]["panic"] == 1

    # a.com has 3 console errors (weight 1 each) = score 3;
    # b.com has 1 broken_render (5) + 1 panic (10) = score 15 → ranked first.
    assert agg["hosts"][0]["host"] == "b.com", agg["hosts"][0]
    assert agg["hosts"][0]["score"] == 15, agg["hosts"][0]

    # The two "TypeError at line N" errors collapse into ONE signature (line #).
    a_sigs = [s for s in agg["signatures"] if s["host"] == "a.com" and s["kind"] == "console_error"]
    line_sig = [s for s in a_sigs if "line #" in s["signature"]]
    assert line_sig and line_sig[0]["count"] == 2, a_sigs

    # host_of normalizes port and scheme.
    assert host_of("https://Ex.com:443/a") == "ex.com"
    assert host_of("http://ex.com/b") == "ex.com"
    assert host_of("file:///C:/tmp/page.html") == "file:page.html"
    assert host_of("") == "(unknown)"

    # kind filter narrows the aggregation.
    only_panic = aggregate(records, kind_filter="panic")
    assert only_panic["total_problems"] == 1, only_panic

    print("health_report selftest: OK")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description="Session-health report over health.log (PERF-6).")
    ap.add_argument("logfile", nargs="?", default="health.log", help="path to health.log (default: ./health.log)")
    ap.add_argument("--top", type=int, default=15, help="how many hosts/issues to list (default 15)")
    ap.add_argument("--kind", choices=sorted(PROBLEM_KINDS), help="restrict report to one problem kind")
    ap.add_argument("--json", action="store_true", help="emit machine-readable JSON instead of text")
    ap.add_argument("--selftest", action="store_true", help="run offline self-test and exit")
    args = ap.parse_args(argv)

    if args.selftest:
        return run_selftest()

    try:
        with open(args.logfile, "r", encoding="utf-8") as f:
            text = f.read()
    except OSError as e:
        print(f"cannot read {args.logfile}: {e}", file=sys.stderr)
        print("(run the browser with --health-log to produce one)", file=sys.stderr)
        return 1

    records = parse_health_log(text)
    agg = aggregate(records, kind_filter=args.kind)

    if args.json:
        print(json.dumps(agg, ensure_ascii=False, indent=2))
    else:
        print(format_report(agg, args.top))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
