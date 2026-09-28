#!/usr/bin/env python3
"""Сравнение прогона perf_audit.py с прошлым прогоном и с базовым замером Chrome.

Выводит markdown для docs/perf/journal.md: статусы и переход каждого сайта,
скорость (ready_s Lumen против `load` Chrome) с разбивкой напрямую/туннель,
память, «Не отвечает» ≥ 2 с, сигнатуры в stderr (BUG-1205 H2-таймаут,
BUG-1206 EvalError, включённый блокировщик), белые кадры среди OK, сверку
ожиданий по закрытым багам. Рядом с результатом пишет compare.json.

  python scripts/perf_compare.py .tmp/perf-audit/<stamp> \\
      --base docs/perf/runs/2026-09-28-top100-split.json \\
      --chrome docs/perf/runs/2026-09-23-top100-split-chrome.json \\
      --tunnel-hosts docs/perf/split-tunnel-hosts.txt \\
      --expect expect.json

--expect — JSON {slug: ["BUG-NNN", ...]}: сайты, которые должны были
починиться закрытыми багами (собирается из файлов bugs/ перед прогоном).
Сигнатуры stderr ищутся в логах, поэтому первым аргументом — каталог прогона,
а не скопированный в docs/perf/runs JSON.
"""
from __future__ import annotations

import argparse
import collections
import json
import re
import statistics as st
import sys
from pathlib import Path

ORDER = ["OK", "DEGRADED", "BROKEN_RENDER", "SITE_REFUSED", "TIMEOUT", "NET_FAIL", "HUNG", "DEAD"]
RANK = {s: i for i, s in enumerate(ORDER)}
SIGS = {
    "H2 stream timed out after 60s (BUG-1205)": re.compile(r"H2 stream \d+ timed out after 60s"),
    "EvalError: Code generation (BUG-1206)": re.compile(r"EvalError: Code generation"),
    "blocked: easylist (блокировщик включён — прогон недействителен)": re.compile(r"blocked: easylist"),
    "421 Misdirected Request (BUG-1209)": re.compile(r"^← 421 ", re.M),
}
NOT_OPENED = ("TIMEOUT", "SITE_REFUSED", "NET_FAIL", "HUNG", "DEAD")


def load(p: Path) -> dict:
    return {r["slug"]: r for r in json.loads(p.read_text(encoding="utf-8"))["results"]}


def ready(r: dict | None) -> float | None:
    if not r or r.get("ready_s") is None or r["status"] in NOT_OPENED:
        return None
    return r["ready_s"]


def chrome_load(c: dict | None) -> float | None:
    if not c or c.get("timed_out") or c.get("error") or not c.get("load"):
        return None
    return c["load"] / 1000


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir", type=Path, help="каталог прогона .tmp/perf-audit/<stamp>")
    ap.add_argument("--base", type=Path, required=True, help="прошлый прогон (docs/perf/runs/*.json)")
    ap.add_argument("--chrome", type=Path, help="базовый замер Chrome (docs/perf/runs/*-chrome.json)")
    ap.add_argument("--tunnel-hosts", type=Path, help="суффиксы доменов, шедших через VPN-туннель")
    ap.add_argument("--expect", type=Path, help="JSON {slug: [BUG-NNN]} — что должно было починиться")
    a = ap.parse_args()

    new, old = load(a.run_dir / "results.json"), load(a.base)
    chrome = json.loads(a.chrome.read_text(encoding="utf-8")) if a.chrome else {}
    tunnel = []
    if a.tunnel_hosts:
        tunnel = [ln.split()[0].lower() for ln in a.tunnel_hosts.read_text(encoding="utf-8").splitlines()
                  if ln.strip() and not ln.startswith("#")]
    common = [k for k in new if k in old]
    out: dict = {}

    def is_tunnel(url: str) -> bool:
        h = re.sub(r"^https?://", "", url).split("/")[0].lower()
        return any(h == s or h.endswith("." + s) for s in tunnel)

    print(f"## Статусы (общих сайтов: {len(common)})\n")
    co = collections.Counter(old[k]["status"] for k in common)
    cn = collections.Counter(new[k]["status"] for k in common)
    print("| Статус | было | стало |\n|---|---|---|")
    for s in ORDER:
        if co[s] or cn[s]:
            print(f"| {s} | {co[s]} | {cn[s]} |")
    better = [f"{k} {old[k]['status']}→{new[k]['status']}" for k in common
              if RANK[new[k]["status"]] < RANK[old[k]["status"]]]
    worse = [f"{k} {old[k]['status']}→{new[k]['status']}" for k in common
             if RANK[new[k]["status"]] > RANK[old[k]["status"]]]
    print(f"\nЛучше ({len(better)}): " + (", ".join(better) or "—"))
    print(f"\nХуже ({len(worse)}): " + (", ".join(worse) or "—"))
    out["transitions"] = {k: [old[k]["status"], new[k]["status"]] for k in common}

    if chrome:
        print("\n## Скорость (ready_s Lumen против load Chrome; сайты, открывшиеся везде)\n")
        print("| Маршрут | Сайтов | Lumen было | Lumen стало | Chrome | Отношение было | Отношение стало |")
        print("|---|---|---|---|---|---|---|")
        groups = [("все", lambda u: True)]
        if tunnel:
            groups = [("напрямую", lambda u: not is_tunnel(u)), ("через туннель", is_tunnel)]
        for label, flt in groups:
            rows = [(k, ready(old[k]), ready(new[k]), chrome_load(chrome.get(k))) for k in common
                    if flt(new[k]["url"])]
            rows = [r for r in rows if None not in r]
            out[f"speed:{label}"] = rows
            if rows:
                print(f"| {label} | {len(rows)} | {st.median(r[1] for r in rows):.1f} с | "
                      f"{st.median(r[2] for r in rows):.1f} с | {st.median(r[3] for r in rows):.1f} с | "
                      f"{st.median(r[1] / r[3] for r in rows):.1f}× | {st.median(r[2] / r[3] for r in rows):.1f}× |")
        rows = [(ready(r), chrome_load(chrome.get(k))) for k, r in new.items()]
        rows = [r for r in rows if None not in r]
        if rows:
            print(f"\nВсе открывшиеся сейчас у обоих: {len(rows)}, Lumen {st.median(r[0] for r in rows):.1f} с, "
                  f"Chrome {st.median(r[1] for r in rows):.1f} с, отношение {st.median(r[0] / r[1] for r in rows):.1f}×")

    print("\n## Память (пик процесса)\n")
    for name, d in (("было", old), ("стало", new)):
        p = [r["proc_peak_mb_total"] for r in d.values() if r.get("proc_peak_mb_total")]
        if p:
            mx = max(d.values(), key=lambda r: r.get("proc_peak_mb_total") or 0)
            print(f"- {name}: медиана {st.median(p):.0f} МБ, максимум {mx['proc_peak_mb_total'] / 1024:.1f} ГБ "
                  f"({mx['slug']}), n={len(p)}")

    print("\n## «Не отвечает» ≥ 2 с\n")
    for name, d in (("было", old), ("стало", new)):
        h = sorted(((r.get("hung_total_s") or 0, r["slug"]) for r in d.values() if (r.get("hung_total_s") or 0) >= 2),
                   reverse=True)
        print(f"- {name}: {len(h)} сайтов: " + ", ".join(f"{s} {t:.0f} с" for t, s in h))
    both = sorted(k for k in common if (old[k].get("hung_total_s") or 0) >= 2 and (new[k].get("hung_total_s") or 0) >= 2)
    print(f"- в обоих прогонах: {', '.join(both) or '—'}")

    print("\n## Сигнатуры в stderr\n")
    hits: dict = {}
    for name, pat in SIGS.items():
        found = []
        for slug, r in new.items():
            lp = a.run_dir / (r.get("stderr_log") or "")
            if lp.is_file():
                n = len(pat.findall(lp.read_text(encoding="utf-8", errors="replace")))
                if n:
                    found.append((slug, n))
        hits[name] = found
        print(f"- {name}: {len(found)} сайтов" + (": " + ", ".join(f"{s}×{n}" for s, n in found) if found else ""))
    out["signatures"] = hits

    blank = [r["slug"] for r in new.values()
             if r["status"] == "OK" and (r.get("frame_blank") or (r.get("frame_dominant_frac") or 0) > 0.97)]
    print(f"\n## Белые кадры среди OK\n\n{', '.join(blank) or '—'}")

    hung = [k for k in common if new[k]["status"] == "HUNG"]
    print(f"\n## HUNG: {', '.join(hung) or '—'} (в прошлом прогоне тоже HUNG/TIMEOUT: "
          f"{', '.join(k for k in hung if old[k]['status'] in ('HUNG', 'TIMEOUT')) or '—'})")

    if a.expect:
        expect = json.loads(a.expect.read_text(encoding="utf-8"))
        print("\n## Ожидания по закрытым багам\n")
        print("| Сайт | Баги | было | стало | Chrome load | Лучше |\n|---|---|---|---|---|---|")
        for k, bugs in expect.items():
            if k not in new or k not in old:
                print(f"| {k} | {', '.join(bugs)} | — | — | — | нет в прогоне |")
                continue
            a_, b_ = old[k]["status"], new[k]["status"]
            cl = chrome_load(chrome.get(k))
            ok = "да" if RANK[b_] < RANK[a_] or b_ == "OK" else "нет"
            print(f"| {k} | {', '.join(bugs)} | {a_} | {b_} | {'—' if cl is None else f'{cl:.1f} с'} | {ok} |")

    (a.run_dir / "compare.json").write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
