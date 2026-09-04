"""Сводка по наблюдению за живым lumen.exe: зависания, память, потоки, сеть.

Читает samples.jsonl/events.jsonl наблюдателя watch_lumen.ps1. Только чтение —
ничего не трогает в идущем прогоне.

Перенесён из .tmp/observe/ (THREAD-0, 2026-09-04) в трекаемое место (THREAD-1
срез 2) вместе с watch_lumen.ps1 — рабочая копия в .tmp/ не версионируется.
"""

import io
import json
import sys
from collections import Counter

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

RUN = sys.argv[1] if len(sys.argv) > 1 else ".tmp/observe"


def load(path):
    out = []
    try:
        with io.open(path, encoding="utf-8-sig") as fh:
            for line in fh:
                line = line.strip()
                if line:
                    try:
                        out.append(json.loads(line))
                    except ValueError:
                        pass
    except OSError:
        pass
    return out


rows = load(RUN + "/samples.jsonl")
events = load(RUN + "/events.jsonl")

if not rows:
    print("нет сэмплов в", RUN)
    raise SystemExit(0)

print("=== ОКНО НАБЛЮДЕНИЯ ===")
print("сэмплов: %d, с %s по %s" % (len(rows), rows[0]["ts"][11:19], rows[-1]["ts"][11:19]))
pids = sorted({r["pid"] for r in rows})
print("PID браузера за окно: %s" % pids)

# --- зависания ---
hangs = [e for e in events if e.get("kind") == "hang_end"]
starts = [e for e in events if e.get("kind") == "hang_start"]
in_hang = sum(1 for r in rows if r.get("in_hang"))
print()
print("=== ЗАВИСАНИЯ ===")
print("сэмплов в зависании: %d из %d (%.0f%% времени)" % (in_hang, len(rows), 100.0 * in_hang / len(rows)))
print("завершившихся эпизодов: %d, открытых: %d" % (len(hangs), len(starts) - len(hangs)))
if hangs:
    durs = sorted(h.get("dur_s") or 0 for h in hangs)
    print("длительность: медиана %.1fс, макс %.1fс, сумма %.0fс" % (durs[len(durs) // 2], durs[-1], sum(durs)))
    print("классы: %s" % dict(Counter(h.get("class") for h in hangs)))
    print()
    print("%-9s %8s %8s %9s %9s %s" % ("конец", "длит,с", "класс", "пик cpu%", "ср cpu%", "заголовок"))
    for h in sorted(hangs, key=lambda h: -(h.get("dur_s") or 0))[:15]:
        print("%-9s %8.1f %8s %9s %9s %s" % (
            h["ts"][11:19], h.get("dur_s") or 0, h.get("class"), h.get("peak_cpu_pct"),
            h.get("avg_cpu_pct"), (h.get("title") or "")[:40]))

# --- перезапуски/падения ---
proc_events = [e for e in events if e.get("kind") in ("process_new", "process_gone")]
if proc_events:
    print()
    print("=== ЖИЗНЬ ПРОЦЕССА ===")
    for e in proc_events:
        if e["kind"] == "process_gone":
            print("%s УМЕР pid=%s exit=%s ws=%sМБ  %s" % (
                e["ts"][11:19], e.get("pid"), e.get("exit_code"), e.get("last_ws_mb"),
                (e.get("last_title") or "")[:40]))
            tail = (e.get("log_tail") or "").splitlines()
            for t in tail[-6:]:
                if t.strip():
                    print("      | " + t[:150])
            for we in e.get("winevents") or []:
                print("      WER[%s] %s" % (we.get("id"), (we.get("msg") or "")[:200]))
        else:
            print("%s СТАРТ pid=%s (был %s)" % (e["ts"][11:19], e.get("pid"), e.get("prev_pid")))

# --- память ---
print()
print("=== ПАМЯТЬ ===")
ws = [r.get("ws_mb") for r in rows if r.get("ws_mb")]
pv = [r.get("priv_mb") for r in rows if r.get("priv_mb")]
print("working set: %.0f → %.0f МБ (мин %.0f, макс %.0f), пик процесса %s МБ" % (
    ws[0], ws[-1], min(ws), max(ws), rows[-1].get("peak_ws_mb")))
print("private:     %.0f → %.0f МБ (макс %.0f)" % (pv[0], pv[-1], max(pv)))
print("virtual:     %.1f ГБ" % ((rows[-1].get("virt_mb") or 0) / 1024.0))
span_min = (len(rows) * 1.0) / 60.0
if span_min > 0.5:
    print("темп роста WS: %.1f МБ/мин" % ((ws[-1] - ws[0]) / span_min))
hs = [r.get("handles") for r in rows if r.get("handles")]
th = [r.get("threads") for r in rows if r.get("threads")]
print("хэндлы: %d → %d (макс %d); потоки: %d → %d (макс %d)" % (
    hs[0], hs[-1], max(hs), th[0], th[-1], max(th)))
gdi = [r.get("gdi") for r in rows if r.get("gdi")]
usr = [r.get("user") for r in rows if r.get("user")]
if gdi:
    print("GDI макс %d, USER макс %d (лимит 10000)" % (max(gdi), max(usr or [0])))
free = [r.get("sys_free_mb") for r in rows if r.get("sys_free_mb")]
if free:
    print("свободно физпамяти в системе: %d → %d МБ (мин %d)" % (free[0], free[-1], min(free)))

# --- отзывчивость ---
print()
print("=== ОТЗЫВЧИВОСТЬ ОКНА (WM_NULL) ===")
pumps = sorted(r.get("pump_ms") for r in rows if r.get("pump_ms") is not None)
if pumps:
    def pct(p):
        return pumps[min(len(pumps) - 1, int(len(pumps) * p))]
    print("p50 %.1f мс, p90 %.1f мс, p99 %.1f мс, макс %.1f мс" % (pct(.5), pct(.9), pct(.99), pumps[-1]))
    print("сэмплов с pump > 1с: %d (%.0f%%)" % (
        sum(1 for x in pumps if x >= 1000), 100.0 * sum(1 for x in pumps if x >= 1000) / len(pumps)))
    print("не ответило вовсе (таймаут 3с): %d" % sum(1 for r in rows if r.get("pump_ok") is False))

# --- CPU ---
print()
print("=== CPU ===")
cpu = [r.get("cpu_pct") for r in rows if r.get("cpu_pct") is not None]
if cpu:
    cs = sorted(cpu)
    print("медиана %.1f%%, p90 %.1f%%, макс %.1f%% (100%% = одно ядро из 8)" % (
        cs[len(cs) // 2], cs[int(len(cs) * .9)], cs[-1]))
    print("сэмплов с cpu > 90%%: %d (%.0f%%)" % (
        sum(1 for x in cpu if x > 90), 100.0 * sum(1 for x in cpu if x > 90) / len(cpu)))

# --- горячие потоки ---
hot = Counter()
for r in rows:
    for t in r.get("top_threads") or []:
        if (t.get("d_ms") or 0) > 300:
            hot[(t.get("tid"), t.get("start"))] += 1
if hot:
    print()
    print("=== ПОТОКИ, ЖГУЩИЕ CPU (>300 мс за сэмпл) ===")
    for (tid, st), n in hot.most_common(8):
        print("  tid=%-7s создан %s — в %d сэмплах" % (tid, st, n))

# --- сеть ---
tcps = [r for r in rows if r.get("tcp")]
if tcps:
    print()
    print("=== TCP ===")
    last = tcps[-1]
    print("сейчас: всего %s, %s" % (last.get("tcp_total"), json.dumps(last.get("tcp"), ensure_ascii=False)))
    tot = [r.get("tcp_total") or 0 for r in tcps]
    print("всего соединений: макс %d, медиана %d" % (max(tot), sorted(tot)[len(tot) // 2]))

# --- активность движка ---
lb = [r.get("log_bytes") for r in rows if r.get("log_bytes") is not None]
if lb:
    print()
    print("=== АКТИВНОСТЬ ДВИЖКА (прирост stderr) ===")
    print("всего записано за окно: %.1f МБ, тихих сэмплов: %d из %d" % (
        sum(lb) / 1048576.0, sum(1 for x in lb if x == 0), len(lb)))
    hang_lb = [r.get("log_bytes") or 0 for r in rows if r.get("in_hang")]
    if hang_lb:
        print("в зависании: тихих %d из %d сэмплов, записано %.0f КБ" % (
            sum(1 for x in hang_lb if x == 0), len(hang_lb), sum(hang_lb) / 1024.0))
