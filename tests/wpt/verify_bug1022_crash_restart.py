#!/usr/bin/env python3
"""[BUG-1022](../../bugs/BUG-1022-FIXED.md) regression check: a test that
kills the browser must be reported as `CRASH`, not `ERROR`.

`run_smoke.py` runs wptrunner with `--no-restart-on-unexpected`, so the only
thing that makes `testrunner.py` respawn Lumen between two tests is a
`CRASH`/`EXTERNAL-TIMEOUT`/`INTERNAL-ERROR` status (`restart_before_next`).
`LumenBidiProtocol.is_alive` used to test `session.transport is not None`,
which stays true after the peer drops, and `do_test` let the
`WebSocket connection closed` failure surface as a plain `ERROR`. The worker
then kept the dead session and the *next* test queued to it failed as well —
which file that was depended on how `--processes N` sharded the run, so every
`--check` of `html/semantics` flipped a different innocent file OK→ERROR.

This drives the real `do_test` against a spawned `lumen --bidi-port <port>`:

1. `is_alive()` is `True` on a live session (negative control);
2. the browser is killed while `do_test` polls a page that never reports
   results -> `do_test` must raise `ExecutorException` with status `CRASH`,
   and `is_alive()` must now be `False`.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/verify_bug1022_crash_restart.py [--binary PATH]

Exits 0 and prints "BUG-1022 OK" on success; non-zero otherwise.
"""

import argparse
import os
import subprocess
import sys
import tempfile
import threading
import types

from verify_bug380_navigation_staleness import (  # noqa: E402  (sets sys.path)
    _StubBrowser,
    _StubLogger,
    default_binary,
    get_free_port,
    read_token_and_drain,
    wait_for_port,
)

from wptrunner.executors.base import ExecutorException  # noqa: E402
from wptrunner.executors.executorlumen import LumenTestharnessExecutor  # noqa: E402

#: Seconds `do_test` is left polling before the browser is killed.
KILL_AFTER_S = 2.0


def verify(bidi_url: str, token: str, page: str, proc: subprocess.Popen) -> None:
    executor = LumenTestharnessExecutor(
        _StubLogger(), _StubBrowser(bidi_url, token), server_config=None)
    executor.test_url = lambda test: test.url
    protocol = executor.protocol
    protocol.connect()
    try:
        protocol.after_connect()
        assert protocol.is_alive(), "is_alive() is False on a live session"
        print("  live session           -> is_alive() True")

        threading.Timer(KILL_AFTER_S, proc.kill).start()
        test = types.SimpleNamespace(url=page, timeout=30)
        try:
            executor.do_test(test)
        except ExecutorException as e:
            assert e.status == "CRASH", (
                f"BUG-1022 regression: browser death reported as {e.status!r}, "
                f"not CRASH — wptrunner will not restart it: {e.message!r}")
            print(f"  browser killed mid-test -> {e.status}")
        else:
            raise AssertionError("do_test returned although the browser was killed")
        assert not protocol.is_alive(), "is_alive() still True after the browser died"
        print("  after the kill          -> is_alive() False")
    finally:
        protocol.teardown()

    print("BUG-1022 OK: a test that kills the browser is a CRASH, so the "
          "next test gets a fresh one")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        page = os.path.join(tmp, "never-reports.html")
        with open(page, "w", encoding="utf-8") as f:
            f.write("<!DOCTYPE html><html><body>bug1022: no results ever</body></html>")
        page_url = "file:///" + page.replace(os.sep, "/")

        port = get_free_port()
        proc = subprocess.Popen(
            [args.binary, "--bidi-port", str(port)],
            stderr=subprocess.PIPE, text=True,
        )
        try:
            wait_for_port(port, proc, timeout=40)
            token = read_token_and_drain(proc.stderr)
            verify(f"ws://127.0.0.1:{port}", token, page_url, proc)
        except Exception as e:
            print(f"BUG-1022 FAILED: {e}", file=sys.stderr)
            return 1
        finally:
            if proc.poll() is None:
                proc.kill()
            proc.wait(timeout=5)
    return 0


if __name__ == "__main__":
    sys.exit(main())
