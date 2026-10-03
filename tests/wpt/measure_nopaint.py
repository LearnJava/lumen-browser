#!/usr/bin/env python3
"""PERF-10 slice 2: A/B of `LUMEN_NO_PAINT=1` against the default pipeline.

Slice 1 added `--no-paint` and the wptrunner plumbing and showed identical
verdicts on 115 ids with no wall-clock win. That left three questions open,
which this script answers with numbers instead of impressions:

* CPU time and peak RSS of the whole `lumen.exe` process tree per mode;
* flake rate - the same run repeated, ids whose verdict differs between
  repeats of the *same* mode vs ids whose verdict differs across modes;
* which categories are safe to move to `--no-paint` by default.

It drives `run_corpus.py` (the real corpus runner, not a bespoke loop) once
per (mode, repeat), alternating the modes so that machine drift hits both
equally, and samples `psutil` on the runner's descendants every second.
CPU time is the sum of the final cumulative user+system time of every
`lumen.exe` pid ever seen (a pid that dies between two samples is counted at
its last observed value, so short-lived processes are slightly under-counted -
equally in both modes).

Usage (venv per tests/wpt/README.md, from the repo root):

    <venv>/python tests/wpt/measure_nopaint.py --binary PATH \
        --prefixes dom/nodes,dom/events --repeats 2 [--out-root DIR]

Output: a table on stdout and `<out-root>/summary.json`. Reftests are not
affected by the flag (`--ipc-server` ignores it), so measure testharness
directories only; the script says so in the summary rather than filtering.
"""

import argparse
import json
import os
import subprocess
import sys
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT_ROOT = os.path.join(REPO_ROOT, ".tmp", "nopaint-ab")

sys.path.insert(0, HERE)


class TreeSampler(threading.Thread):
    """Samples CPU time and RSS of `lumen*` processes under `root_pid`."""

    def __init__(self, root_pid: int, interval: float = 1.0):
        super().__init__(daemon=True)
        import psutil
        self._psutil = psutil
        self.root_pid = root_pid
        self.interval = interval
        self.cpu_by_pid: dict = {}
        self.peak_rss = 0
        self.peak_procs = 0
        self.samples = 0
        self.rss_sum = 0
        self._halt = threading.Event()

    def run(self) -> None:
        psutil = self._psutil
        try:
            root = psutil.Process(self.root_pid)
        except psutil.Error:
            return
        while not self._halt.is_set():
            rss = 0
            procs = 0
            try:
                tree = root.children(recursive=True)
            except psutil.Error:
                tree = []
            for proc in tree:
                try:
                    if not proc.name().lower().startswith("lumen"):
                        continue
                    t = proc.cpu_times()
                    self.cpu_by_pid[(proc.pid, proc.create_time())] = t.user + t.system
                    rss += proc.memory_info().rss
                    procs += 1
                except psutil.Error:
                    continue
            self.peak_rss = max(self.peak_rss, rss)
            self.peak_procs = max(self.peak_procs, procs)
            if procs:
                self.samples += 1
                self.rss_sum += rss
            self._halt.wait(self.interval)

    def stop(self) -> None:
        self._halt.set()
        self.join(timeout=5)

    @property
    def cpu_seconds(self) -> float:
        return sum(self.cpu_by_pid.values())

    @property
    def mean_rss(self) -> float:
        return self.rss_sum / self.samples if self.samples else 0.0


def verdict_of(result: dict) -> str:
    """Comparable fingerprint of one id: test status + subtest statuses."""
    subs = sorted((s.get("name", ""), s.get("status", "")) for s in result.get("subtests", []))
    return json.dumps([result.get("status"), subs], ensure_ascii=False)


def run_once(mode: str, repeat: int, args) -> dict:
    out_dir = os.path.join(args.out_root, f"{mode}-{repeat}")
    os.makedirs(out_dir, exist_ok=True)
    env = dict(os.environ)
    env.pop("LUMEN_NO_PAINT", None)
    if mode == "nopaint":
        env["LUMEN_NO_PAINT"] = "1"
    cmd = [sys.executable, os.path.join(HERE, "run_corpus.py"),
           "--binary", args.binary, "--prefixes", args.prefixes,
           "--out-dir", out_dir, "--processes", str(args.processes), "--skip-manifest-update"]
    if args.skip_https:
        cmd.append("--skip-https")
    log_path = os.path.join(out_dir, "runner.log")
    started = time.time()
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT, cwd=REPO_ROOT)
        sampler = TreeSampler(proc.pid)
        sampler.start()
        proc.wait()
        sampler.stop()
    wall = time.time() - started

    import run_corpus
    results, _recovered, _empty = run_corpus.load_results(out_dir)
    verdicts = {test: verdict_of(r) for test, r in results.items()}
    return {
        "mode": mode, "repeat": repeat, "out_dir": out_dir, "exit": proc.returncode,
        "wall_s": round(wall, 1), "cpu_s": round(sampler.cpu_seconds, 1),
        "peak_rss_mb": round(sampler.peak_rss / 2**20), "mean_rss_mb": round(sampler.mean_rss / 2**20),
        "peak_procs": sampler.peak_procs, "ids": len(verdicts), "verdicts": verdicts,
    }


def category_of(test_id: str) -> str:
    parts = test_id.strip("/").split("/")
    return "/".join(parts[:2]) if len(parts) > 2 else parts[0]


def diff_ids(a: dict, b: dict) -> list:
    return sorted(t for t in set(a) | set(b) if a.get(t) != b.get(t))


def summarize(runs: list) -> dict:
    by_mode: dict = {"paint": [], "nopaint": []}
    for r in runs:
        by_mode[r["mode"]].append(r)
    summary: dict = {"modes": {}, "flaky": {}, "cross": {}, "per_category": {}}
    for mode, rs in by_mode.items():
        if not rs:
            continue
        n = len(rs)
        summary["modes"][mode] = {
            "runs": n,
            "wall_s_mean": round(sum(r["wall_s"] for r in rs) / n, 1),
            "cpu_s_mean": round(sum(r["cpu_s"] for r in rs) / n, 1),
            "peak_rss_mb_max": max(r["peak_rss_mb"] for r in rs),
            "mean_rss_mb_mean": round(sum(r["mean_rss_mb"] for r in rs) / n),
            "ids": rs[0]["ids"],
        }
        flaky = set()
        for i in range(1, n):
            flaky.update(diff_ids(rs[0]["verdicts"], rs[i]["verdicts"]))
        summary["flaky"][mode] = sorted(flaky)
    if by_mode["paint"] and by_mode["nopaint"]:
        # An id counts as a real cross-mode difference only if it is stable
        # inside each mode; otherwise it is flake, not a flag effect.
        flaky_any = set(summary["flaky"].get("paint", [])) | set(summary["flaky"].get("nopaint", []))
        cross = [t for t in diff_ids(by_mode["paint"][0]["verdicts"], by_mode["nopaint"][0]["verdicts"])
                 if t not in flaky_any]
        summary["cross"] = {"stable_differences": cross, "flaky_ids": sorted(flaky_any)}
        cats: dict = {}
        ids = set(by_mode["paint"][0]["verdicts"]) | set(by_mode["nopaint"][0]["verdicts"])
        for t in ids:
            c = cats.setdefault(category_of(t), {"ids": 0, "diff": 0, "flaky": 0})
            c["ids"] += 1
            c["diff"] += t in cross
            c["flaky"] += t in flaky_any
        summary["per_category"] = dict(sorted(cats.items()))
    return summary


def print_summary(summary: dict) -> None:
    print("\nmode      runs  ids   wall_s   cpu_s  peak_rss_MB  mean_rss_MB")
    for mode, m in summary["modes"].items():
        print(f"{mode:<9} {m['runs']:<5} {m['ids']:<5} {m['wall_s_mean']:<8} {m['cpu_s_mean']:<7} "
              f"{m['peak_rss_mb_max']:<12} {m['mean_rss_mb_mean']}")
    for mode, ids in summary["flaky"].items():
        print(f"flaky inside {mode}: {len(ids)}")
    cross = summary.get("cross")
    if cross:
        print(f"stable cross-mode differences: {len(cross['stable_differences'])}")
        for t in cross["stable_differences"][:30]:
            print("  ", t)
        print("\ncategory                          ids  diff  flaky")
        for c, v in summary["per_category"].items():
            print(f"{c:<33} {v['ids']:<4} {v['diff']:<5} {v['flaky']}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen.exe"))
    parser.add_argument("--prefixes", required=True, help="comma-separated WPT path prefixes (testharness dirs)")
    parser.add_argument("--repeats", type=int, default=2, help="runs per mode (default 2: one repeat shows flake)")
    parser.add_argument("--processes", type=int, default=4)
    parser.add_argument("--out-root", default=DEFAULT_OUT_ROOT)
    parser.add_argument("--skip-https", action="store_true")
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1
    os.makedirs(args.out_root, exist_ok=True)
    runs = []
    for repeat in range(args.repeats):
        # Alternate who goes first so a warm cache / thermal drift favours neither.
        order = ("paint", "nopaint") if repeat % 2 == 0 else ("nopaint", "paint")
        for mode in order:
            print(f"[{time.strftime('%H:%M:%S')}] {mode} #{repeat} ...", flush=True)
            r = run_once(mode, repeat, args)
            runs.append(r)
            print(f"    exit={r['exit']} ids={r['ids']} wall={r['wall_s']}s cpu={r['cpu_s']}s "
                  f"peak_rss={r['peak_rss_mb']}MB", flush=True)
    summary = summarize(runs)
    summary["prefixes"] = args.prefixes
    summary["runs"] = [{k: v for k, v in r.items() if k != "verdicts"} for r in runs]
    with open(os.path.join(args.out_root, "summary.json"), "w", encoding="utf-8") as fh:
        json.dump(summary, fh, ensure_ascii=False, indent=1)
    print_summary(summary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
