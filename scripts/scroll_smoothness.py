#!/usr/bin/env python3
"""Плавность прокрутки колесом: метрики из журнала Lumen (THREAD-5).

Lumen пишет журнал сам (`LUMEN_PRESENT_LOG=<файл>`, формат —
crates/shell/src/present_log.rs): `P <unix_us> <commit> frame|tick` — кадр
показан, `W <unix_us> <dy>` — событие колеса. Колесо крутит человек в окне на
переднем плане (CDP/MCP обходят путь ОС), скрипт только читает журнал.

    python scripts/scroll_smoothness.py lumen.log [--period-ms 16.66] [--json]

Метрики (по кадрам внутри серий прокрутки):
  on_time   доля интервалов между present ≤ периода vsync (+1 мс на дрожание часов)
  max_gap   наибольший интервал внутри серии, мс
  jerks     число интервалов > 2 периодов
  latency   от первого щелчка серии до первого present после него, мс
Серия = кадры, соседние не дальше `--run-gap-ms` (100); пауза между щелчками
без сдвига — не рывок, поэтому интервалы длиннее границы серии считаются
остановками, а не рывками. Остановка с щелчком колеса внутри (кадра не было,
хотя ввод пришёл) выводится отдельно: это настоящая потеря плавности.
"""
import argparse
import json
import statistics
import sys


def parse(path):
    frames, wheels = [], []
    with open(path, encoding="utf-8") as f:
        for line in f:
            p = line.split()
            if len(p) >= 2 and p[0] == "P":
                frames.append(int(p[1]) / 1000.0)
            elif len(p) >= 2 and p[0] == "W":
                wheels.append(int(p[1]) / 1000.0)
    return sorted(frames), sorted(wheels)


def analyze(frames, wheels, period, run_gap, tol=1.0, burst_gap=200.0):
    out = {"frames": len(frames), "wheel_events": len(wheels), "period_ms": period}
    if len(frames) < 2:
        return out
    intervals, stalls = [], []
    for a, b in zip(frames, frames[1:]):
        d = b - a
        if d <= run_gap:
            intervals.append(d)
        else:
            inside = any(a < w < b - run_gap for w in wheels)
            stalls.append({"gap_ms": round(d, 1), "wheel_inside": inside})
    if intervals:
        ok = sum(1 for d in intervals if d <= period + tol)
        out.update(
            intervals=len(intervals),
            on_time=round(ok / len(intervals), 4),
            max_gap_ms=round(max(intervals), 2),
            median_ms=round(statistics.median(intervals), 2),
            p95_ms=round(sorted(intervals)[int(len(intervals) * 0.95) - 1], 2),
            jerks=sum(1 for d in intervals if d > 2 * period),
        )
    out["stalls_over_run_gap"] = len(stalls)
    out["stalls_with_wheel_inside"] = sum(1 for s in stalls if s["wheel_inside"])
    # задержка: первый щелчок каждой серии щелчков → ближайший present после него
    lat, last = [], None
    for w in wheels:
        if last is None or w - last > burst_gap:
            nxt = next((f for f in frames if f >= w), None)
            if nxt is not None:
                lat.append(nxt - w)
        last = w
    if lat:
        out["latency_ms_median"] = round(statistics.median(lat), 1)
        out["latency_ms_max"] = round(max(lat), 1)
        out["bursts"] = len(lat)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("log")
    ap.add_argument("--period-ms", type=float, default=16.66)
    ap.add_argument("--run-gap-ms", type=float, default=100.0)
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    frames, wheels = parse(a.log)
    r = analyze(frames, wheels, a.period_ms, a.run_gap_ms)
    if a.json:
        json.dump(r, sys.stdout, ensure_ascii=False, indent=2)
        print()
    else:
        for k, v in r.items():
            print(f"{k:26} {v}")


if __name__ == "__main__":
    main()
