#!/usr/bin/env python3
"""Корпус C (COMPAT-CORPUS-2): проверка файлов корпуса и план ночного прогона.

Файлы собирает scripts/corpus_c_build.py (с сетью, раз в квартал); этот скрипт сети не использует.

  python scripts/corpus_c.py check                     # целостность docs/perf/corpus-c*.{txt,tsv}
  python scripts/corpus_c.py plan --date 2026-10-15    # какие сайты идут в эту ночь
  python scripts/corpus_c.py plan --coverage           # неделя вперёд: каждый сайт хотя бы раз
  python scripts/corpus_c.py run --point ru            # план на сегодня + perf_audit --mode compat
  python scripts/corpus_c.py --self-test

Расписание: «топ» — первые CORE сайтов каждого сегмента по позиции (ранг Tranco) — идёт каждую
ночь; остальные ротацией: сайт с номером i идёт в ночь, когда date.toordinal() % 7 == i % 7,
то есть ровно раз за любые 7 подряд ночей. Точка наблюдения (--point ru|abroad) только
подписывает прогон: машина и IP — решение владельца (COMPAT-BIZ), скрипт их не выбирает.
"""
import argparse
import csv
import datetime
import subprocess
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")

REPO = Path(__file__).resolve().parent.parent
PERF = REPO / "docs" / "perf"
TSV = PERF / "corpus-c.tsv"
SEGMENTS = {"ru": 200, "global": 300}
CORE = 100  # сайтов на сегмент в каждой ночи: 100 RU + 100 global = «топ-200»
POINTS = ("ru", "abroad")
COLS = ["slug", "segment", "pos", "weight", "rank", "category", "source", "status", "home", "inner"]


def load(tsv: Path = TSV) -> list[dict]:
    rows = []
    with tsv.open(encoding="utf-8", newline="") as f:
        for r in csv.reader(f, delimiter="\t"):
            if r and not r[0].startswith("#"):
                rows.append(dict(zip(COLS, r)))
    return rows


def corpus_lines(row: dict) -> list[str]:
    """Строки `slug url` для perf_audit.load_corpus: главная и (если есть) внутренняя."""
    out = [f"{row['slug']:<28}{row['home']}"]
    if row["inner"]:
        out.append(f"{row['slug'] + '--inner':<28}{row['inner']}")
    return out


def night_sites(rows: list[dict], day: datetime.date) -> list[dict]:
    """Сайты ночи `day`: топ каждого сегмента + своя седьмая часть остальных."""
    out = []
    for seg in SEGMENTS:
        seg_rows = [r for r in rows if r["segment"] == seg]
        for i, r in enumerate(seg_rows):
            if i < CORE or day.toordinal() % 7 == (i - CORE) % 7:
                out.append(r)
    return out


def check(rows: list[dict]) -> list[str]:
    errs = []
    slugs = set()
    for seg, want in SEGMENTS.items():
        seg_rows = [r for r in rows if r["segment"] == seg]
        if len(seg_rows) != want:
            errs.append(f"сегмент {seg}: {len(seg_rows)} сайтов, нужно {want}")
        ranks = [int(r["rank"]) for r in seg_rows]
        if [r["pos"] for r in seg_rows] != [str(i) for i in range(1, len(seg_rows) + 1)]:
            errs.append(f"сегмент {seg}: позиции не 1..N подряд")
        # ранг 0 — домена нет в Tranco как PLD (поддомены вроде nalog.gov.ru): такие идут в конец
        if ranks != sorted(ranks, key=lambda x: x or 10**9):
            errs.append(f"сегмент {seg}: ранг Tranco не по возрастанию")
        txt = PERF / f"corpus-c-{seg}.txt"
        expect = [ln for r in seg_rows for ln in corpus_lines(r)]
        got = [ln for ln in txt.read_text(encoding="utf-8").splitlines() if ln and not ln.startswith("#")]
        if got != expect:
            errs.append(f"{txt.name} расходится с corpus-c.tsv — пересобери scripts/corpus_c_build.py")
    for r in rows:
        if r["slug"] in slugs:
            errs.append(f"повтор slug {r['slug']}")
        slugs.add(r["slug"])
        if r["category"] in ("", "?"):
            errs.append(f"{r['slug']}: нет категории")
        if r["weight"] not in ("1", "2", "3"):
            errs.append(f"{r['slug']}: вес {r['weight']}")
        if not r["home"].startswith("https://"):
            errs.append(f"{r['slug']}: главная не https")
    return errs


def coverage(rows: list[dict], start: datetime.date) -> tuple[int, int]:
    """(сколько сайтов хоть раз за 7 ночей, всего); топ идёт каждую ночь."""
    seen = set()
    for d in range(7):
        seen.update(r["slug"] for r in night_sites(rows, start + datetime.timedelta(days=d)))
    return len(seen), len(rows)


def write_plan(rows: list[dict], day: datetime.date, point: str) -> Path:
    sites = night_sites(rows, day)
    out = REPO / ".tmp" / "corpus-c" / "night" / f"{day.isoformat()}-{point}.txt"
    out.parent.mkdir(parents=True, exist_ok=True)
    lines = [f"# Корпус C, ночь {day.isoformat()}, точка {point}: {len(sites)} сайтов", ""]
    lines += [ln for r in sites for ln in corpus_lines(r)]
    out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return out


def self_test() -> None:
    def row(seg, i):
        return {"slug": f"{seg}{i}", "segment": seg, "pos": str(i + 1), "weight": "1", "rank": str(i),
                "category": "x", "source": "", "status": "200", "home": f"https://{seg}{i}.example/",
                "inner": ""}
    rows = [row(seg, i) for seg, n in SEGMENTS.items() for i in range(n)]
    start = datetime.date(2026, 10, 1)
    for shift in range(7):  # любое начало недели
        seen, total = coverage(rows, start + datetime.timedelta(days=shift))
        assert seen == total, (shift, seen, total)
    per_night = [len(night_sites(rows, start + datetime.timedelta(days=d))) for d in range(7)]
    assert sum(per_night) == 7 * 2 * CORE + (len(rows) - 2 * CORE), per_night  # остальные ровно раз
    assert all(n >= 2 * CORE for n in per_night)
    core = {r["slug"] for r in rows if int(r["pos"]) <= CORE}
    for d in range(7):
        assert core <= {r["slug"] for r in night_sites(rows, start + datetime.timedelta(days=d))}
    print("self-test OK")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", nargs="?", choices=("check", "plan", "run"))
    ap.add_argument("--date", help="ночь YYYY-MM-DD (по умолчанию сегодня)")
    ap.add_argument("--point", choices=POINTS, default="ru", help="точка наблюдения (подпись прогона)")
    ap.add_argument("--coverage", action="store_true", help="plan: покрытие за 7 ночей")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("extra", nargs="*", help="run: дополнительные аргументы perf_audit.py")
    a = ap.parse_args()
    if a.self_test:
        self_test()
        return 0
    if not a.cmd:
        ap.print_help()
        return 2
    rows = load()
    day = datetime.date.fromisoformat(a.date) if a.date else datetime.date.today()
    if a.cmd == "check":
        errs = check(rows)
        print("\n".join(errs) if errs else f"корпус C в порядке: {len(rows)} сайтов")
        return 1 if errs else 0
    if a.cmd == "plan" and a.coverage:
        seen, total = coverage(rows, day)
        print(f"за 7 ночей с {day}: {seen}/{total} сайтов")
        return 0 if seen == total else 1
    plan = write_plan(rows, day, a.point)
    n = sum(1 for ln in plan.read_text(encoding="utf-8").splitlines() if ln and not ln.startswith("#"))
    print(f"{plan.relative_to(REPO)}: {n} страниц")
    if a.cmd == "plan":
        return 0
    cmd = [sys.executable, str(REPO / "scripts" / "perf_audit.py"), "--mode", "compat",
           "--corpus", str(plan), *a.extra]
    print(" ".join(cmd))
    return subprocess.call(cmd)


if __name__ == "__main__":
    sys.exit(main())
