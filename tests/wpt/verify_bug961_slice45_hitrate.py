#!/usr/bin/env python3
"""BUG-961 срез 45: "Что нужно" item 3 — a real hit-rate, not another
single-shot A/B.

Срезы 43/44 each drew a conclusion from ONE sample per condition: срез 43's
minimal repro (plain `subprocess.Popen`, no `LumenBrowser`/`mozprocess`,
static file server, bare `BidiSession`, `navigate(wait="complete")`)
completed in ~1.1-1.2s six times in a row, then срез 44 re-ran the *same*
shape once more under a controlled A/B and it stalled ~32s — proving the
stall is intermittent, not that either shape is required or sufficient.

This slice runs срез 43's own script (`verify_bug961_slice43_wchan.py`,
unmodified) N times, **once per fresh `python` process** — not looped
inside one interpreter, per item 3's explicit requirement to rule out
interpreter-level state (import caches, asyncio event loop reuse, GC state)
leaking between runs and masking or manufacturing the effect. Each run is
launched via `subprocess.run([sys.executable, ...])`, so every run gets a
brand-new CPython process, a brand-new asyncio loop, a brand-new `lumen`
child.

For every run this records: pass/fail (parsed from the child's own stdout —
"navigate returned OK" vs. a timeout/exception line), wall-clock elapsed,
and `/proc/loadavg` sampled immediately before spawning (срез 44 noted load
2.0-2.3 during its one stall and flagged that no prior slice had logged
load alongside its result). On a stall, срез 43's script already prints its
`_WchanSampler` summary (grouped `(tid, comm, wchan)` states) to stdout —
this slice does not re-implement that, it just keeps the full child stdout
in the per-run log so a stall's wchan evidence is never thrown away, which
item 3 flags as the one kind of evidence срезы 39-44 are missing (every
prior stall happened either with no sampler running, or too briefly
sampled).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice45_hitrate.py
        [--binary target/dev-release/lumen] [--runs 10] [--seconds 40]
        [--log-dir .tmp/bug961-slice45]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
SLICE43_SCRIPT = os.path.join(HERE, "verify_bug961_slice43_wchan.py")


def _loadavg():
    try:
        with open("/proc/loadavg", "r") as f:
            return f.readline().strip()
    except OSError as e:  # noqa: BLE001 — this is diagnostics, not the SUT
        return f"<unreadable: {e!r}>"


def _classify(stdout):
    """`verify_bug961_slice43_wchan.py` always exits 0 (it's a measurement,
    not a gate — its own docstring says so), so pass/fail has to come from
    parsing its stdout, not the return code."""
    if "navigate returned OK in" in stdout:
        for line in stdout.splitlines():
            if "navigate returned OK in" in line:
                return "OK", line.strip()
    if "asyncio.wait_for timed out" in stdout:
        for line in stdout.splitlines():
            if "asyncio.wait_for timed out" in line:
                return "STALL", line.strip()
    if "navigate raised after" in stdout:
        for line in stdout.splitlines():
            if "navigate raised after" in line:
                return "STALL", line.strip()
    return "UNKNOWN", "(no recognized outcome line in child stdout)"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen"))
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--seconds", type=float, default=40.0)
    parser.add_argument(
        "--log-dir", default=os.path.join(REPO_ROOT, ".tmp", "bug961-slice45"))
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1
    if not os.path.isfile(SLICE43_SCRIPT):
        print(f"срез 43 script not found: {SLICE43_SCRIPT}", file=sys.stderr)
        return 1

    os.makedirs(args.log_dir, exist_ok=True)

    results = []
    for i in range(1, args.runs + 1):
        loadavg_before = _loadavg()
        cmd = [
            sys.executable, SLICE43_SCRIPT,
            "--binary", args.binary,
            "--seconds", str(args.seconds),
        ]
        print(f"[hitrate] run {i}/{args.runs}: loadavg_before={loadavg_before!r} "
              f"spawning fresh interpreter: {' '.join(cmd)}")
        t0 = time.monotonic()
        proc = subprocess.run(
            cmd, cwd=REPO_ROOT, capture_output=True, text=True,
            timeout=args.seconds + 30)
        elapsed = time.monotonic() - t0
        outcome, detail = _classify(proc.stdout)
        log_path = os.path.join(args.log_dir, f"run{i:02d}_{outcome.lower()}.log")
        with open(log_path, "w") as f:
            f.write(f"# loadavg_before={loadavg_before}\n")
            f.write(f"# cmd={cmd}\n")
            f.write(f"# wall_clock_elapsed={elapsed:.2f}s exit_code={proc.returncode}\n")
            f.write("## stdout\n")
            f.write(proc.stdout)
            f.write("\n## stderr\n")
            f.write(proc.stderr)
        results.append((i, outcome, detail, elapsed, loadavg_before, log_path))
        print(f"[hitrate] run {i}/{args.runs}: {outcome} ({detail}) "
              f"wall={elapsed:.2f}s log={log_path}")

    stalls = [r for r in results if r[1] == "STALL"]
    oks = [r for r in results if r[1] == "OK"]
    unknown = [r for r in results if r[1] == "UNKNOWN"]
    print()
    print(f"[hitrate] SUMMARY: {len(stalls)}/{len(results)} stalled, "
          f"{len(oks)}/{len(results)} OK, {len(unknown)}/{len(results)} unknown")
    for i, outcome, detail, elapsed, loadavg_before, log_path in results:
        print(f"  run {i:2d}: {outcome:8s} wall={elapsed:6.2f}s "
              f"loadavg_before={loadavg_before!r} {log_path}")
    if stalls:
        print()
        print("[hitrate] stall logs contain the срез-43 wchan sampler's "
              "(tid, comm, wchan) summary for that run — read those before "
              "drawing a mechanism conclusion, per item 3.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
