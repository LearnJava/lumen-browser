#!/usr/bin/env python3
"""Regression check for the executor's early TIMEOUT on a test that navigated
its own top-level document away (`executorlumen.py`, `TEST_CONTEXT_GLOBAL` /
`LUMEN_WPT_FOREIGN_GRACE_S`; `docs/tasks/p2-wpt-runner-throughput.md`
§уход со страницы теста).

Before: once a test's top level was replaced by a page that is not a test
(`<form target>` resolved to the top level — BUG-1269, `location =`, an
un-returned bfcache round trip), the executor polled a document whose harness
could never report until the full timeout ran out — 65 s for `timeout: long`,
for a verdict (TIMEOUT) known after the first poll.

Drives the real `_run_testharness` coroutine against a spawned
`lumen --bidi-port <port>`:

1. a test page that carries the test-context marker and stashes a result
   -> must return exactly that result (the marker check must not get in the
   way of a normal test);
2. a test page that carries the marker and then navigates itself to a plain
   page, with the grace set to 1.5 s and a 30 s test timeout
   -> must raise TIMEOUT naming the foreign page, well before 30 s;
3. the same page with the grace `off`, 3 s test timeout
   -> must raise the ordinary "Timed out waiting" TIMEOUT (old behaviour kept
   behind the switch).

Usage (from repo root, venv as in tests/wpt/README.md):

    <venv>/python tests/wpt/verify_navaway_early_timeout.py [--binary PATH]

Exits 0 and prints "NAVAWAY OK" on success.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    os.path.join(REPO_ROOT, "tools", "wptrunner"),
    os.path.dirname(os.path.abspath(__file__)),
]

from wptrunner.executors.base import ExecutorException  # noqa: E402
from wptrunner.executors.executorlumen import (  # noqa: E402
    FOREIGN_GRACE_ENV,
    RESULTS_GLOBAL,
    TEST_CONTEXT_GLOBAL,
    LumenTestharnessExecutor,
)
from verify_bug380_navigation_staleness import (  # noqa: E402
    _StubBrowser,
    _StubLogger,
    get_free_port,
    read_token_and_drain,
    wait_for_port,
)

RESULT = ["/navaway/stays.html", 0, None, None, []]


def _expect_timeout(executor, url, timeout, needle):
    t0 = time.monotonic()
    try:
        got = executor.protocol.run(executor._run_testharness(url, timeout))
    except ExecutorException as e:
        elapsed = time.monotonic() - t0
        assert e.status == "TIMEOUT", f"expected TIMEOUT, got {e.status!r}: {e.message}"
        assert needle in e.message, f"unexpected message: {e.message!r}"
        return elapsed, e.message
    raise AssertionError(f"expected TIMEOUT, got a result: {got!r}")


def verify(bidi_url, token, stays, leaves):
    executor = LumenTestharnessExecutor(
        _StubLogger(), _StubBrowser(bidi_url, token), server_config=None)
    protocol = executor.protocol
    protocol.connect()
    try:
        protocol.after_connect()

        os.environ[FOREIGN_GRACE_ENV] = "1.5"
        first = protocol.run(executor._run_testharness(stays, 10))
        assert first == RESULT, f"stays result: {first!r}"
        print(f"  1 test page reports          -> {first}")

        elapsed, msg = _expect_timeout(executor, leaves, 30, "navigated away")
        assert "plain.html" in msg, f"foreign URL not named: {msg!r}"
        assert elapsed < 12, f"early TIMEOUT took {elapsed:.1f}s (grace 1.5s, timeout 30s)"
        print(f"  2 leaves, grace 1.5s         -> TIMEOUT after {elapsed:.1f}s")

        os.environ[FOREIGN_GRACE_ENV] = "off"
        elapsed, _ = _expect_timeout(executor, leaves, 3, "Timed out waiting")
        print(f"  3 leaves, grace off, t/o 3s  -> TIMEOUT after {elapsed:.1f}s")
    finally:
        os.environ.pop(FOREIGN_GRACE_ENV, None)
        protocol.teardown()
    print("NAVAWAY OK: a test that left its own page ends early, "
          "a test that stays is untouched")


def default_binary():
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()
    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        def page(name, body):
            path = os.path.join(tmp, name)
            with open(path, "w", encoding="utf-8") as f:
                f.write(f"<!DOCTYPE html><html><body>{body}</body></html>")
            return path

        marker = f"window.{TEST_CONTEXT_GLOBAL} = true;"
        stays = page("stays.html", f"<script>{marker} window.{RESULTS_GLOBAL} = "
                                   f"{json.dumps(json.dumps(RESULT))};</script>")
        page("plain.html", "not a test")
        leaves = page("leaves.html", f"<script>{marker} setTimeout(() => "
                                     "{ location.href = 'plain.html'; }, 100);</script>")

        port = get_free_port()
        proc = subprocess.Popen([args.binary, "--bidi-port", str(port)],
                                stderr=subprocess.PIPE, text=True)
        try:
            wait_for_port(port, proc, timeout=40)
            token = read_token_and_drain(proc.stderr)
            verify(f"ws://127.0.0.1:{port}", token, stays, leaves)
        except Exception as e:
            print(f"NAVAWAY FAILED: {e}", file=sys.stderr)
            return 1
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
