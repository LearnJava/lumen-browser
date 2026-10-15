#!/usr/bin/env python3
"""Свод A/B 2×2 идентичности (COMPAT-AB-IDENTITY): стек (Lumen / Chrome) × UA (с токеном Lumen / без).

Ячейки приходят из двух плеч: `compat_ab_chrome.py` (`--variant plain|lumen`) и `compat_ab_lumen.py`
(тот же формат записей). Здесь они сводятся в таблицу и в два числа для решений ADR-034:

  * **токен** — сайт заблокирован с токеном `Lumen/x.y` и пущен без него, в плече Lumen (это тот браузер,
    который поедет к пользователю). Порог Р1: переход на `chrome_lumen`, если таких ≤ 0,5 % корпуса и 0 среди
    must-work (вес 3 в `docs/perf/corpus-c.tsv`);
  * **стек** — сайт пущен Chrome и заблокирован Lumen при одинаковой идентичности `plain`. Порог Р4: проект
    TLS/H2 открывать, если стеку приписаны отказы ≥ 3 % корпуса или хотя бы один must-work.

«Заблокирован» = статус из `compat_ab_chrome.BLOCKED` (`SITE_REFUSED`, `WALL_SOFT`, `NET_FAIL`, `HTTP_ERROR`).
Сайт, у которого во всех ячейках `NET_FAIL`/`TIMEOUT` (мёртв с этой машины), в знаменатель не входит.
Одиночное расхождение живого сайта — шум IP и антибота, поэтому расхождения перепроверяют: все файлы с той же
парой (сайт, вариант) считаются голосами, побеждает исход большинства.

    python scripts/compat_ab_matrix.py corpus --out docs/perf/corpus-ab.txt
    python scripts/compat_ab_matrix.py merge --chrome a.json --chrome recheck.json --lumen b.json --md out.md
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compat_ab_chrome import BLOCKED  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
PERF = REPO / "docs" / "perf"
DEAD = {"NET_FAIL", "TIMEOUT"}
TOKEN_THRESHOLD = 0.5  # %, Р1
STACK_THRESHOLD = 3.0  # %, Р4
ARMS = ("chrome", "lumen")
VARIANTS = ("plain", "lumen")

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")


# ── корпус ───────────────────────────────────────────────────────────────────

def read_pairs(path: Path) -> list[tuple[str, str]]:
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            slug, url = line.split(None, 1)
            out.append((slug, url.strip()))
    return out


def read_weights(tsv: Path) -> dict[str, int]:
    """slug → вес (3 = must-work) из `docs/perf/corpus-c.tsv`."""
    weights = {}
    for line in tsv.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            cols = line.split("\t")
            weights[cols[0]] = int(cols[3])
    return weights


def build_corpus() -> list[tuple[str, str, str]]:
    """(slug, url, источник): главные страницы корпуса C, стенд G, 6 сайтов BUG-1113, малый корпус perf_audit.

    Одинаковый URL берётся один раз (первый источник побеждает); slug стенда — с префиксом `stand-`."""
    rows: list[tuple[str, str, str]] = []
    for line in (PERF / "corpus-c.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            cols = line.split("\t")
            rows.append((cols[0], cols[8], "corpus-c"))
    for line in (PERF / "corpus-captcha.tsv").read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            cols = line.split("\t")
            rows.append(("stand-" + cols[0], cols[3], "stand-g"))
    for name, src in (("ab-bug1113.txt", "bug1113"), ("corpus.txt", "corpus")):
        rows += [(s, u, src) for s, u in read_pairs(PERF / name)]
    seen_url, seen_slug, out = set(), set(), []
    for slug, url, src in rows:
        if url in seen_url or slug in seen_slug:
            continue
        seen_url.add(url)
        seen_slug.add(slug)
        out.append((slug, url, src))
    return out


# ── свод ─────────────────────────────────────────────────────────────────────

def load_cells(files: dict[str, list[Path]]) -> dict[tuple[str, str, str], dict]:
    """(плечо, вариант, slug) → запись. Пара снята несколько раз (основной прогон + перепроверки) —
    берётся исход большинства (блокировка / не блокировка), при равенстве — поздняя запись."""
    runs: dict[tuple[str, str, str], list[dict]] = {}
    for arm, paths in files.items():
        for path in paths:
            for rec in json.loads(path.read_text(encoding="utf-8"))["results"]:
                runs.setdefault((arm, rec["variant"], rec["slug"]), []).append(rec)
    cells = {}
    for key, recs in runs.items():
        votes = sum(1 for r in recs if r["status"] in BLOCKED)
        want_blocked = votes * 2 > len(recs) if votes * 2 != len(recs) else recs[-1]["status"] in BLOCKED
        cells[key] = next(r for r in reversed(recs) if (r["status"] in BLOCKED) == want_blocked)
        cells[key] = dict(cells[key], runs=len(recs), blocked_runs=votes)
    return cells


def blocked(rec: dict | None) -> bool | None:
    """None — ячейки нет или сайт не ответил осмысленно (мёртв/таймаут)."""
    if rec is None:
        return None
    if rec["status"] in DEAD:
        return None
    return rec["status"] in BLOCKED


TLS_CAUSES = ("antibot", "tls")


def stack_cause(rec: dict) -> str:
    """Почему Lumen не пущен там, где Chrome пущен: trust (нет корня в хранилище), dns, tls (рукопожатие оборвано),
    antibot (отказ по статусу/вендору), jschl (JS-челлендж Cloudflare), soft (заглушка в тексте исходника), other."""
    err = (rec.get("net_error") or "").lower()
    if "unknownissuer" in err or "untrusted certificate" in err:
        return "trust"
    if err.startswith("dns:"):
        return "dns"
    if err.startswith("tls:"):
        return "tls"
    if any(str(sig).startswith("cloudflare") for sig in (rec.get("wall") or {}).get("signals", [])):
        return "jschl"  # челлендж Cloudflare проходит JS, а плечо Lumen его не исполняет
    if rec["status"] in ("SITE_REFUSED", "HTTP_ERROR"):
        return "antibot"
    return "soft" if rec["status"] == "WALL_SOFT" else "other"


def analyse(cells: dict, weights: dict[str, int]) -> dict:
    slugs = list(dict.fromkeys(k[2] for k in cells))
    complete, dead = [], []
    for s in slugs:
        recs = [cells.get((a, v, s)) for a in ARMS for v in VARIANTS]
        if any(r is None for r in recs):
            continue  # ячейка не снята: в свод не идёт
        if all(r["status"] in DEAD for r in recs):
            dead.append(s)
        else:
            complete.append(s)

    def blk(arm: str, var: str, s: str) -> bool:
        return cells[(arm, var, s)]["status"] in BLOCKED

    out = {"measured": complete, "dead": dead, "total": len(slugs)}
    out["token"] = {a: [s for s in complete if blk(a, "lumen", s) and not blk(a, "plain", s)] for a in ARMS}
    out["token_reverse"] = {a: [s for s in complete if blk(a, "plain", s) and not blk(a, "lumen", s)] for a in ARMS}
    out["stack"] = [s for s in complete if blk("lumen", "plain", s) and not blk("chrome", "plain", s)]
    out["stack_reverse"] = [s for s in complete if blk("chrome", "plain", s) and not blk("lumen", "plain", s)]
    out["both_block"] = [s for s in complete if blk("lumen", "plain", s) and blk("chrome", "plain", s)]
    out["cells"] = {
        (a, v): {st: sum(1 for s in complete if cells[(a, v, s)]["status"] == st) for st in sorted({cells[(a, v, s)]["status"] for s in complete})}
        for a in ARMS for v in VARIANTS
    }
    n = max(len(complete), 1)
    must = [s for s in complete if weights.get(s) == 3]
    out["must_total"] = len(must)
    out["token_pct"] = round(100 * len(out["token"]["lumen"]) / n, 2)
    out["stack_pct"] = round(100 * len(out["stack"]) / n, 2)
    out["token_must"] = [s for s in out["token"]["lumen"] if weights.get(s) == 3]
    out["stack_must"] = [s for s in out["stack"] if weights.get(s) == 3]
    out["stack_cause"] = {s: stack_cause(cells[("lumen", "plain", s)]) for s in out["stack"]}
    # Р4 про отпечаток TLS/H2 (rustls): корневые сертификаты, DNS и заглушки «обновите браузер» в исходнике без JS
    # к нему не относятся — у них свои починки (OS-доверие, резолвер, JS-плечо).
    out["stack_tls"] = [s for s in out["stack"] if out["stack_cause"][s] in TLS_CAUSES]
    out["stack_tls_pct"] = round(100 * len(out["stack_tls"]) / n, 2)
    out["stack_tls_must"] = [s for s in out["stack_tls"] if weights.get(s) == 3]
    out["verdict_r1"] = "переход на chrome_lumen допустим" if out["token_pct"] <= TOKEN_THRESHOLD and not out["token_must"] else "оставить chrome_unbranded"
    out["verdict_r4"] = "проект TLS/H2 открывать" if out["stack_tls_pct"] >= STACK_THRESHOLD or out["stack_tls_must"] else "TLS/H2 не трогать"
    return out


def wall_note(cells: dict, arm: str, var: str, slug: str) -> str:
    r = cells[(arm, var, slug)]
    w = r.get("wall") or {}
    vendor = w.get("vendor") or w.get("kind") or ""
    return f"{r['status']}({r.get('http_status')}{'/' + vendor if vendor and w.get('kind') != 'none' else ''})"


def render_md(res: dict, cells: dict, weights: dict[str, int], meta: str) -> str:
    n = len(res["measured"])
    out = ["# A/B 2×2 идентичности: стек × токен", "", meta, ""]
    out += [f"Сайтов в своде: {n} (из {res['total']}; мёртвых с этой машины: {len(res['dead'])}), must-work (вес 3): {res['must_total']}.", ""]
    statuses = sorted({st for c in res["cells"].values() for st in c})
    out += ["| Ячейка | " + " | ".join(statuses) + " | заблокировано |", "|---|" + "---|" * (len(statuses) + 1)]
    for a in ARMS:
        for v in VARIANTS:
            c = res["cells"][(a, v)]
            b = sum(c.get(st, 0) for st in BLOCKED)
            out.append(f"| {a} / {v} | " + " | ".join(str(c.get(st, 0)) for st in statuses) + f" | {b} ({100 * b / max(n, 1):.1f} %) |")

    def listing(title: str, slugs: list[str], arm_a: str, var_a: str, arm_b: str, var_b: str) -> list[str]:
        if not slugs:
            return [f"**{title}:** нет.", ""]
        rows = [f"**{title}:** {len(slugs)}", "", f"| Сайт | вес | {arm_a}/{var_a} | {arm_b}/{var_b} |", "|---|---|---|---|"]
        for s in slugs:
            rows.append(f"| {s} | {weights.get(s, '—')} | {wall_note(cells, arm_a, var_a, s)} | {wall_note(cells, arm_b, var_b, s)} |")
        return rows + [""]

    out += ["", f"## Токен `Lumen/x.y` (плечо Lumen): {len(res['token']['lumen'])} из {n} = {res['token_pct']} % (порог Р1 ≤ {TOKEN_THRESHOLD} %, must-work: {len(res['token_must'])})", ""]
    out += listing("пущен без токена, заблокирован с ним (Lumen)", res["token"]["lumen"], "lumen", "plain", "lumen", "lumen")
    out += listing("заблокирован без токена, пущен с ним (Lumen) — шум", res["token_reverse"]["lumen"], "lumen", "plain", "lumen", "lumen")
    out += listing("то же для Chrome: пущен без токена, заблокирован с ним", res["token"]["chrome"], "chrome", "plain", "chrome", "lumen")
    out += listing("Chrome: заблокирован без токена, пущен с ним — шум", res["token_reverse"]["chrome"], "chrome", "plain", "chrome", "lumen")
    out += [f"## Стек (plain): Chrome пустил, Lumen заблокирован: {len(res['stack'])} из {n} = {res['stack_pct']} % (порог Р4 ≥ {STACK_THRESHOLD} %, must-work: {len(res['stack_must'])})", ""]
    out += listing("Chrome пустил, Lumen заблокирован", res["stack"], "chrome", "plain", "lumen", "plain")
    causes = {}
    for s, c in res["stack_cause"].items():
        causes.setdefault(c, []).append(s)
    out += ["Причины (`stack_cause`): " + "; ".join(f"{c} — {len(v)} ({', '.join(v)})" for c, v in sorted(causes.items())), "",
            f"К отпечатку TLS/H2 относятся причины {'/'.join(TLS_CAUSES)}: {len(res['stack_tls'])} из {n} = {res['stack_tls_pct']} %, "
            f"must-work: {len(res['stack_tls_must'])} ({', '.join(res['stack_tls_must']) or '—'}).", ""]
    out += listing("Lumen пустил, Chrome заблокирован (Chrome исполняет JS-стены, Lumen — нет)", res["stack_reverse"], "chrome", "plain", "lumen", "plain")
    out += [f"Заблокированы обоими (plain): {len(res['both_block'])} — {', '.join(res['both_block']) or '—'}", ""]
    out += ["## Вердикты", "", f"- Р1/Р2: {res['verdict_r1']}.", f"- Р4: {res['verdict_r4']}.", ""]
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("corpus", help="собрать docs/perf/corpus-ab.txt")
    c.add_argument("--out", type=Path, default=PERF / "corpus-ab.txt")
    m = sub.add_parser("merge", help="свести ячейки в таблицу")
    m.add_argument("--chrome", type=Path, action="append", default=[], help="results.json плеча Chrome (повторяемый)")
    m.add_argument("--lumen", type=Path, action="append", default=[], help="results.json плеча Lumen (повторяемый)")
    m.add_argument("--weights", type=Path, default=PERF / "corpus-c.tsv")
    m.add_argument("--md", type=Path, help="записать отчёт сюда")
    m.add_argument("--meta", default="", help="строка про условия прогона (маршрут, дата, версии)")
    args = ap.parse_args()

    if args.cmd == "corpus":
        rows = build_corpus()
        head = ["# Корпус A/B 2×2 идентичности (COMPAT-AB-IDENTITY): `python scripts/compat_ab_matrix.py corpus`.",
                "# Источники по порядку: главные страницы docs/perf/corpus-c.tsv (500, вес в нём же), стенд G docs/perf/corpus-captcha.tsv",
                "# (slug `stand-…`), docs/perf/ab-bug1113.txt, docs/perf/corpus.txt; одинаковый URL — один раз. Формат `slug url`."]
        args.out.write_text("\n".join(head + [f"{s:<28} {u}" for s, u, _ in rows]) + "\n", encoding="utf-8", newline="\n")
        from collections import Counter
        print(f"{len(rows)} сайтов -> {args.out}", dict(Counter(src for *_, src in rows)))
        return 0

    cells = load_cells({"chrome": args.chrome, "lumen": args.lumen})
    weights = read_weights(args.weights)
    res = analyse(cells, weights)
    md = render_md(res, cells, weights, args.meta)
    if args.md:
        args.md.write_text(md, encoding="utf-8", newline="\n")
    print(md)
    return 0


if __name__ == "__main__":
    sys.exit(main())
