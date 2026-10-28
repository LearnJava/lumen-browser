#!/usr/bin/env python3
"""Кластеризация прогона compat_crawl: превращает results.jsonl в очередь починок (SITES-TRIAGE).

Для каждого сайта, который не прошёл (статус Lumen не OK или упала хотя бы одна проверка),
назначается ПЕРВИЧНЫЙ класс по приоритету:

    не наш (антибот/гео/отказ) → TIMEOUT/HUNG → NET_FAIL → blank:near-empty → BROKEN_RENDER
    → pixels (только differing) → a11y → прочее

Внутри класса сайты группируются по причине (сигнатура исключения, «нет API», тип сетевой
ошибки, строка stderr, «без улик»). Вес сайта — колонка `вес` корпуса C; сайт из
`--user-slugs` получает вес 5. Вес кластера — сумма весов.

    python scripts/compat_triage.py --results A.jsonl [--results B.jsonl] \
        --corpus docs/perf/corpus-c.tsv --out report.md [--user-results U.jsonl]
"""
from __future__ import annotations

import argparse
import csv
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

USER_WEIGHT = 5

CLASS_ORDER = ["не наш", "TIMEOUT/HUNG", "NET_FAIL", "blank:near-empty", "BROKEN_RENDER",
               "pixels", "a11y", "прочее"]

NO_API_RE = re.compile(
    r"([\w.$]+ is not (?:a function|defined|a constructor)|"
    r"cannot read propert(?:y|ies) of (?:undefined|null) \(reading '[^']+'\))", re.IGNORECASE)
NOISE_STDERR = ("→", "←")


def load_weights(corpus: Path) -> dict[str, dict]:
    """slug -> {weight, segment, url} из corpus-c.tsv."""
    out: dict[str, dict] = {}
    with corpus.open(encoding="utf-8", newline="") as f:
        for row in csv.reader(f, delimiter="\t"):
            if not row or row[0].startswith("#") or len(row) < 4:
                continue
            try:
                weight = int(row[3])
            except ValueError:
                continue
            out[row[0]] = {"weight": weight, "segment": row[1],
                           "url": row[8] if len(row) > 8 else ""}
    return out


def load_records(paths: list[Path]) -> dict[str, dict]:
    """Читает results.jsonl; при повторе slug побеждает последний файл."""
    out: dict[str, dict] = {}
    for p in paths:
        with p.open(encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                rec = json.loads(line)
                out[rec["slug"]] = rec
    return out


def last_lumen(rec: dict) -> dict:
    runs = rec.get("runs") or []
    return (runs[-1].get("lumen") or {}) if runs else {}


def check(rec: dict, name: str) -> dict:
    return (rec.get("checks") or {}).get(name) or {}


def failed_checks(rec: dict) -> list[str]:
    return [n for n, c in (rec.get("checks") or {}).items() if isinstance(c, dict) and c.get("ok") is False]


def is_failing(rec: dict) -> bool:
    return rec.get("lumen_status") != "OK" or bool(failed_checks(rec))


def is_foreign(rec: dict) -> bool:
    """Антибот / отказ сайта: не баг движка."""
    if rec.get("lumen_status") == "SITE_REFUSED":
        return True
    walls = check(rec, "walls")
    hits = list(walls.get("chrome_hits") or []) + list(walls.get("lumen_hits") or [])
    return any(h.startswith("hard:") for h in hits)


def pixels_differing(rec: dict) -> bool:
    c = check(rec, "pixels")
    if c.get("ok") is not False:
        return False
    return "diff" in (c.get("detail") or "") and "нет кадра" not in (c.get("detail") or "")


def primary_class(rec: dict) -> str | None:
    """Первичный класс сайта или None, если сайт прошёл."""
    if not is_failing(rec):
        return None
    status = rec.get("lumen_status")
    if is_foreign(rec):
        return "не наш"
    if status in ("TIMEOUT", "HUNG"):
        return "TIMEOUT/HUNG"
    if status == "NET_FAIL":
        return "NET_FAIL"
    walls = check(rec, "walls")
    if "blank:near-empty" in (walls.get("extra") or []):
        return "blank:near-empty"
    if status == "BROKEN_RENDER":
        return "BROKEN_RENDER"
    if pixels_differing(rec):
        return "pixels"
    if check(rec, "a11y").get("ok") is False:
        return "a11y"
    return "прочее"


def net_failure_type(nf) -> str:
    if isinstance(nf, dict):
        for k in ("kind", "error", "reason", "type", "message"):
            if nf.get(k):
                return str(nf[k])[:60]
        return "net"
    return re.sub(r"https?://\S+", "URL", str(nf))[:60]


def stderr_signature(lumen: dict) -> str | None:
    sigs = lumen.get("stderr_error_sigs") or {}
    items = [(k, v.get("count", 1) if isinstance(v, dict) else 1) for k, v in sigs.items()
             if not k.startswith(NOISE_STDERR)]
    if not items:
        return None
    items.sort(key=lambda kv: -kv[1])
    return items[0][0][:100]


URL_RE = re.compile(r"(?:https?|wss?|chrome-extension)://\S+")
HEX_RE = re.compile(r"[0-9a-f]{8,}")


def norm_cause(text: str) -> str:
    """Без URL, хешей и чисел, чтобы однотипные причины сливались в один кластер."""
    text = URL_RE.sub("URL", text)
    text = HEX_RE.sub("H", text)
    text = re.sub(r"\d+", "N", text)
    return re.sub(r"\s+", " ", text).strip()[:90]


def cause(rec: dict, klass: str) -> str:
    return norm_cause(raw_cause(rec, klass))


def raw_cause(rec: dict, klass: str) -> str:
    """Причина внутри класса (ключ кластера)."""
    lumen = last_lumen(rec)
    new_sigs = check(rec, "exceptions").get("new_sigs") or []
    if not new_sigs:
        # проверка исключений могла не считаться (эталон не открыл сайт) — берём сырые
        raw = lumen.get("exceptions") or []
        if raw:
            from_raw = raw[0].strip().split("\n", 1)[0]
            new_sigs = [re.sub(r"^(Uncaught\s+)", "", from_raw)[:120].lower()]
    for s in new_sigs:
        m = NO_API_RE.search(s)
        if m:
            return f"нет API: {m.group(1).lower()}"
    if new_sigs:
        return f"исключение: {new_sigs[0][:100]}"
    nfs = lumen.get("net_failures") or []
    if nfs:
        top = Counter(net_failure_type(n) for n in nfs).most_common(1)[0][0]
        return f"сеть: {top}"
    sig = stderr_signature(lumen)
    if sig:
        return f"stderr: {sig}"
    if klass == "pixels":
        return "без улик: расхождение кадра"
    return "без улик"


def build(records: dict[str, dict], weights: dict[str, dict], user_slugs: set[str]) -> list[dict]:
    sites = []
    for slug, rec in records.items():
        klass = primary_class(rec)
        if klass is None:
            continue
        info = weights.get(slug, {})
        w = USER_WEIGHT if slug in user_slugs else info.get("weight", 1)
        sites.append({
            "slug": slug, "url": rec.get("url") or info.get("url", ""), "class": klass,
            "cause": cause(rec, klass), "weight": w, "base_weight": info.get("weight", 1),
            "status": rec.get("lumen_status"), "ready_s": last_lumen(rec).get("ready_s"),
            "segment": info.get("segment", ""), "failed": failed_checks(rec),
        })
    return sites


def clusters(sites: list[dict]) -> list[dict]:
    by: dict[tuple, list[dict]] = defaultdict(list)
    for s in sites:
        by[(s["class"], s["cause"])].append(s)
    out = []
    for (klass, why), members in by.items():
        members.sort(key=lambda s: (-s["weight"], s["slug"]))
        out.append({"class": klass, "cause": why, "n": len(members),
                    "weight": sum(m["weight"] for m in members), "sites": members})
    out.sort(key=lambda c: (-c["weight"], -c["n"], c["class"], c["cause"]))
    return out


def fmt_ready(v) -> str:
    return "—" if v is None else f"{float(v):.1f}"


def md_escape(s: str) -> str:
    return s.replace("|", "\\|").replace("\n", " ")


def render(sites: list[dict], cls: list[dict], user_rows: list[dict] | None,
           total: int, sources: list[str], note: str = "") -> str:
    L = ["# Кластеризация прогона корпуса C (SITES-TRIAGE-1)", ""]
    L.append(f"Источники: {', '.join(f'`{s}`' for s in sources)}. Сайтов в прогоне: {total}, не прошли: {len(sites)}.")
    if note:
        L += ["", note]
    L.append("Первичный класс — по приоритету: не наш → TIMEOUT/HUNG → NET_FAIL → blank:near-empty → "
             "BROKEN_RENDER → pixels (differing) → a11y → прочее. Вес сайта 1–3 из `corpus-c.tsv`, "
             f"сайт пользователя — {USER_WEIGHT}.")
    L.append("")
    if user_rows is not None:
        L += ["## Сайты пользователя (без прокси)", "",
              "| сайт | статус | класс | причина | ready_s |", "|---|---|---|---|---|"]
        for r in user_rows:
            L.append(f"| {r['slug']} | {r['status']} | {r['class']} | {md_escape(r['cause'])} | {fmt_ready(r['ready_s'])} |")
        L.append("")
    L += ["## Классы", "", "| класс | сайтов | вес |", "|---|---|---|"]
    per = defaultdict(lambda: [0, 0])
    for s in sites:
        per[s["class"]][0] += 1
        per[s["class"]][1] += s["weight"]
    for k in CLASS_ORDER:
        if k in per:
            L.append(f"| {k} | {per[k][0]} | {per[k][1]} |")
    L.append(f"| **итого** | **{len(sites)}** | **{sum(s['weight'] for s in sites)}** |")
    L.append("")
    L += ["## Кластеры по убыванию веса", ""]
    multi = [c for c in cls if c["n"] >= 2]
    single: dict[str, list[dict]] = defaultdict(list)
    for c in cls:
        if c["n"] < 2:
            single[c["class"]].extend(c["sites"])
    for i, c in enumerate(multi, 1):
        L.append(f"### {i}. [{c['class']}] {md_escape(c['cause'])}")
        L.append("")
        L.append(f"- сайтов: {c['n']}, вес: {c['weight']}")
        ex = "; ".join(f"{m['slug']} ({m['url']}, ready_s={fmt_ready(m['ready_s'])}, в{m['weight']})"
                       for m in c["sites"][:5])
        L.append(f"- примеры: {ex}")
        L.append("")
    L += ["### Единичные причины (по одному сайту; в кластеры не сведены)", ""]
    for k in CLASS_ORDER:
        if k in single:
            ms = sorted(single[k], key=lambda m: (-m["weight"], m["slug"]))
            L.append(f"- **{k}**: {len(ms)} сайтов, вес {sum(m['weight'] for m in ms)} — "
                     + ", ".join(m["slug"] for m in ms[:25]) + (" …" if len(ms) > 25 else ""))
    L.append("")
    top = sorted((s for s in sites if s["base_weight"] == 3 and s["slug"] not in {u["slug"] for u in (user_rows or [])}),
                 key=lambda s: (s["class"], s["slug"]))[:30]
    L += ["## Топ-30 сайтов веса 3 и их класс", "", "| сайт | класс | причина | ready_s |", "|---|---|---|---|"]
    for s in top:
        L.append(f"| {s['slug']} | {s['class']} | {md_escape(s['cause'])} | {fmt_ready(s['ready_s'])} |")
    L.append("")
    return "\n".join(L)


def user_table(records: dict[str, dict]) -> list[dict]:
    rows = []
    for slug, rec in records.items():
        klass = primary_class(rec) or "OK"
        rows.append({"slug": slug, "status": rec.get("lumen_status"), "class": klass,
                     "cause": cause(rec, klass) if klass != "OK" else "",
                     "ready_s": last_lumen(rec).get("ready_s")})
    return rows


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--results", action="append", type=Path, required=True)
    ap.add_argument("--corpus", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--note", default="", help="абзац про происхождение данных для шапки отчёта")
    ap.add_argument("--user-results", type=Path, help="results.jsonl прогона сайтов пользователя")
    args = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    weights = load_weights(args.corpus)
    records = load_records(args.results)
    user_records = load_records([args.user_results]) if args.user_results else {}
    user_slugs = set(user_records)
    sites = build(records, weights, user_slugs)
    cls = clusters(sites)
    text = render(sites, cls, user_table(user_records) if args.user_results else None,
                  len(records), [str(p).replace("\\", "/") for p in args.results], args.note)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(f"сайтов {len(records)}, не прошли {len(sites)}, кластеров {len(cls)} → {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
