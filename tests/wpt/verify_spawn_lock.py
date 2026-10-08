#!/usr/bin/env python3
"""Pin: browser launches in one wptrunner process are serialized (WPT-RUN-9).

On Windows mozprocess creates each browser with `bInheritHandles=1` and no
handle list. When two `TestRunnerManager` threads launch browsers at the same
moment, the second browser inherits the child ends of the first one's
stdout/stderr pipes; killing the first then gives its reader thread no EOF,
and `WebDriverBrowser.stop`'s `proc.kill(timeout=5)` sits out the whole
timeout — 6.1 s per restart instead of ~0.1 s, on 14-18 of ~88 restarts per
run. `browsers/lumen.py` holds `_SPAWN_LOCK` over `ProcessHandler.run()` to
close that window. Measurement — `docs/tasks/p2-wpt-runner-throughput.md`
§одновременный запуск браузеров.

The race is forced, not waited for: `winprocess.CreateProcess` is wrapped so
that the first launch pauses inside it (its pipe ends already inheritable)
while a second thread launches. Then the first child is killed with the same
`kill(timeout=5)` the browser plugin uses. No browser needed — the children
are `python -c "sleep"`.

Checks:

1. with the lock (default) the kill returns in well under a second;
2. with `LUMEN_WPT_SPAWN_LOCK=off` it takes the old ~5-6 s — proves the pin
   reproduces the leak at all, so (1) is not passing vacuously.

Windows-only; elsewhere it prints SKIP and exits 0.

Usage: <venv>/python tests/wpt/verify_spawn_lock.py   (exit 0 = PASS)
"""

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

CHILD = r"""
import json, sys, threading, time
sys.path[:0] = [{repo!r}, {tools!r}, {wptrunner!r}, {webdriver!r}, {wptserve!r}]
import localpaths  # noqa: F401
import mozprocess
from mozprocess import winprocess
from wptrunner.browsers import lumen

SLEEPER = [sys.executable, "-c", "import time; time.sleep(60)"]
real_create = winprocess.CreateProcess
first_inside = threading.Event()
calls = [0]

def slow_first(*args, **kwargs):
    calls[0] += 1
    if calls[0] == 1:
        first_inside.set()
        time.sleep(1.0)  # the window a concurrent launch must not fall into
    return real_create(*args, **kwargs)

winprocess.CreateProcess = slow_first

def handler():
    return mozprocess.ProcessHandler(SLEEPER, processOutputLine=lambda line: None,
                                     storeOutput=False)

first, second = handler(), handler()
t = threading.Thread(target=lumen._start_process, args=(first,))
t.start()
first_inside.wait(5)
lumen._start_process(second)
t.join()
started = time.time()
first.kill(timeout=5)
kill_s = time.time() - started
second.kill(timeout=5)
print(json.dumps({{"kill_s": kill_s}}))
"""


def run_child(lock_off: bool) -> float:
    env = dict(os.environ)
    if lock_off:
        env["LUMEN_WPT_SPAWN_LOCK"] = "off"
    else:
        env.pop("LUMEN_WPT_SPAWN_LOCK", None)
    code = CHILD.format(repo=REPO, tools=os.path.join(REPO, "tools"),
                        wptrunner=os.path.join(REPO, "tools", "wptrunner"),
                        webdriver=os.path.join(REPO, "tools", "webdriver"),
                        wptserve=os.path.join(REPO, "tools", "wptserve"))
    out = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True,
                         check=False, env=env, cwd=REPO)
    if out.returncode != 0:
        raise SystemExit(f"child failed:\n{out.stderr}")
    return json.loads(out.stdout.strip().splitlines()[-1])["kill_s"]


def main() -> int:
    if sys.platform != "win32":
        print("verify_spawn_lock: SKIP (the handle leak is Windows-only)")
        return 0
    failures = []
    locked = run_child(lock_off=False)
    print(f"  kill after a concurrent launch, lock on:  {locked:.2f}s")
    if locked > 1.0:
        failures.append(f"lock on: kill took {locked:.2f}s — the second browser still "
                        f"inherits the first one's pipe")
    unlocked = run_child(lock_off=True)
    print(f"  kill after a concurrent launch, lock off: {unlocked:.2f}s")
    if unlocked < 3.0:
        failures.append(f"lock off: kill took only {unlocked:.2f}s — the pin no longer "
                        f"reproduces the leak, so the 'lock on' check proves nothing")
    for failure in failures:
        print(f"FAIL  {failure}")
    print("verify_spawn_lock: " + ("FAIL" if failures else "PASS"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
