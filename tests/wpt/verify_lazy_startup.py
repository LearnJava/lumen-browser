#!/usr/bin/env python3
"""Pin: `run_smoke.py` imports no wptrunner at module level (WPT-RUN-9, startup).

wptrunner starts every wptserve daemon, manager and test runner with the
`spawn` start method, and on Windows `spawn` re-executes the parent's
`__main__` — `run_smoke.py` — in each child before it reads its target. With
`wptrunner` imported at module level that was ~0.9 s per child, and the seven
servers came up strictly one after another (`ServerProc.start` blocks until
the child has its arguments): ~7 s per shard plus ~1.7 s per test-runner
(re)start. Measurement — `docs/tasks/p2-wpt-runner-throughput.md` §старт
шарда.

Checks, no browser needed:

1. importing `run_smoke` (what every spawned child does) pulls in neither
   `wptrunner` nor `localpaths`, and takes well under the old ~1 s;
2. the import-time monkeypatches are still applied once `run()` loads
   wptrunner, and applying them twice does not stack them.

Usage: <venv>/python tests/wpt/verify_lazy_startup.py   (exit 0 = PASS)
"""

import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))

CHILD = r"""
import json, sys, time
sys.path.insert(0, {here!r})
t = time.perf_counter()
import run_smoke
elapsed = time.perf_counter() - t
heavy = sorted(m for m in sys.modules
               if m == "localpaths" or m == "wptrunner" or m.startswith("wptrunner."))
print(json.dumps({{"elapsed": elapsed, "heavy": heavy}}))
"""

PATCHES = r"""
import json, os, sys
sys.path.insert(0, {here!r})
os.environ["LUMEN_WPT_SERVER_CONFIG"] = {config!r}
import run_smoke
run_smoke._install_server_config_override()
from wptrunner import environment
first = environment.TestEnvironment.build_config
run_smoke._install_server_config_override()
second = environment.TestEnvironment.build_config
run_smoke._install_shared_queue()
from wptrunner import testloader
print(json.dumps({{
    "lane_patched": getattr(first, "_lumen_lane_override", False),
    "lane_not_stacked": first is second,
    "queue_patched": testloader.FullyParallelGroupedSource.make_groups.__module__ == "run_smoke",
}}))
"""


def run_child(code: str) -> dict:
    out = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True,
                         check=False)
    if out.returncode != 0:
        raise SystemExit(f"child failed:\n{out.stderr}")
    return json.loads(out.stdout.strip().splitlines()[-1])


def main() -> int:
    failures = []

    got = run_child(CHILD.format(here=HERE))
    if got["heavy"]:
        failures.append(f"importing run_smoke pulled in {got['heavy']} — every spawned "
                        f"wptserve/runner child would pay for it again")
    if got["elapsed"] > 0.5:
        failures.append(f"importing run_smoke took {got['elapsed']:.2f}s (> 0.5 s)")
    print(f"  import run_smoke: {got['elapsed'] * 1000:.0f} ms, heavy modules: {got['heavy']}")

    with tempfile.TemporaryDirectory() as tmp:
        config = os.path.join(tmp, "lane1-config.json")
        with open(config, "w", encoding="utf-8") as fh:
            json.dump({"ports": {"http": [19300, 19301]}}, fh)
        got = run_child(PATCHES.format(here=HERE, config=config))
    for key, ok in got.items():
        print(f"  {key}: {ok}")
        if not ok:
            failures.append(key)

    for failure in failures:
        print(f"FAIL  {failure}")
    print("verify_lazy_startup: " + ("FAIL" if failures else "PASS"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
