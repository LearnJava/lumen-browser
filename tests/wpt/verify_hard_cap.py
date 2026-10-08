#!/usr/bin/env python3
"""Regression check for the executor's per-test hard cap and result-URL
canonicalisation (`executorlumen.py`: `hard_cap_s`/`run_capped`,
`canonical_result_url`; `docs/tasks/p2-wpt-runner-throughput.md`
§URL результата и потолок теста).

Before:

* a test whose page never yields to the event loop kept one
  `script.evaluate` pending until Lumen's own automation timeout (30-65 s);
  the poll loop could not look at its deadline meanwhile, the test ended on
  `testrunner.py`'s external timer and the manager then spent `join(10)` on a
  runner process still stuck in that call;
* an id with a raw space in its query (`/xhr/xmlhttprequest-timeout-*.html?
  aborted immediately after send()`) came back from `testharnessreport.js`
  percent-encoded, failed `result_url == test.url` and was thrown away as
  INTERNAL-ERROR, restarting the browser.

Checks:

1. `canonical_result_url` — encoded vs literal id is accepted, a result from a
   *different* test (BUG-1268) still is not;
2. `hard_cap_s` — `timeout + 2 * extra_timeout`, `None` without a timeout or
   with `LUMEN_WPT_HARD_CAP=off`;
3. live, against a spawned `lumen --bidi-port`: a page that spins forever with
   a 3 s test timeout ends as EXTERNAL-TIMEOUT within the cap (13 s), and the
   protocol teardown afterwards is bounded.

Usage (from repo root, venv as in tests/wpt/README.md):

    <venv>/python tests/wpt/verify_hard_cap.py [--binary PATH]

Exits 0 and prints "HARD CAP OK" on success.
"""

import argparse
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
    HARD_CAP_ENV,
    TEST_CONTEXT_GLOBAL,
    LumenTestharnessExecutor,
    canonical_result_url,
    hard_cap_s,
    run_capped,
)
from verify_bug380_navigation_staleness import (  # noqa: E402
    _StubBrowser,
    _StubLogger,
    get_free_port,
    read_token_and_drain,
    wait_for_port,
)


class _Test:
    url = "/xhr/xmlhttprequest-timeout-aborted.html?aborted immediately after send()"


def check_pure():
    encoded = ["/xhr/xmlhttprequest-timeout-aborted.html?aborted%20immediately%20after%20send()",
               0, None, None, []]
    got = canonical_result_url(_Test, encoded)
    assert got[0] == _Test.url and got[1:] == encoded[1:], f"encoded id not accepted: {got!r}"
    foreign = ["/html/dom/idlharness.https.html", 0, None, None, []]
    assert canonical_result_url(_Test, foreign) is foreign, "foreign result was rewritten"
    print("  1 canonical_result_url       -> encoded accepted, foreign kept")

    os.environ.pop(HARD_CAP_ENV, None)
    assert hard_cap_s(10, 5) == 20 and hard_cap_s(60, 5) == 70, "cap formula"
    assert hard_cap_s(None, 5) is None, "no timeout (debugger) must mean no cap"
    os.environ[HARD_CAP_ENV] = "off"
    try:
        assert hard_cap_s(10, 5) is None, f"{HARD_CAP_ENV}=off must disable the cap"
    finally:
        os.environ.pop(HARD_CAP_ENV, None)
    print("  2 hard_cap_s                 -> t+2*extra, None when off/no timeout")


def check_live(bidi_url, token, spin_url):
    executor = LumenTestharnessExecutor(
        _StubLogger(), _StubBrowser(bidi_url, token), server_config=None)
    protocol = executor.protocol
    protocol.connect()
    try:
        protocol.after_connect()
        timeout = 3
        cap = hard_cap_s(timeout, executor.extra_timeout)
        t0 = time.monotonic()
        try:
            got = run_capped(protocol, executor._run_testharness(spin_url, timeout), cap, spin_url)
        except ExecutorException as e:
            elapsed = time.monotonic() - t0
            assert e.status in ("EXTERNAL-TIMEOUT", "TIMEOUT"), f"{e.status}: {e.message}"
            assert elapsed <= cap + 2, f"took {elapsed:.1f}s, cap {cap}s"
            print(f"  3 spinning page, t/o {timeout}s     -> {e.status} after {elapsed:.1f}s "
                  f"(cap {cap}s, wedged={protocol.wedged})")
        else:
            raise AssertionError(f"spinning page returned a result: {got!r}")
    finally:
        t0 = time.monotonic()
        protocol.teardown()
        took = time.monotonic() - t0
    assert took < 5, f"teardown took {took:.1f}s"
    print(f"  4 teardown after the cap     -> {took:.2f}s")


def default_binary():
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()
    check_pure()
    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        spin = os.path.join(tmp, "spin.html")
        with open(spin, "w", encoding="utf-8") as f:
            f.write(f"<!DOCTYPE html><html><body><script>window.{TEST_CONTEXT_GLOBAL} = true;"
                    "setTimeout(() => { for (;;) {} }, 200);</script></body></html>")
        port = get_free_port()
        proc = subprocess.Popen([args.binary, "--bidi-port", str(port)],
                                stderr=subprocess.PIPE, text=True)
        try:
            wait_for_port(port, proc, timeout=40)
            token = read_token_and_drain(proc.stderr)
            check_live(f"ws://127.0.0.1:{port}", token, spin)
        except Exception as e:
            print(f"HARD CAP FAILED: {e!r}", file=sys.stderr)
            return 1
        finally:
            proc.kill()
            proc.wait(timeout=10)
    print("HARD CAP OK: a stuck BiDi call ends at the cap, an encoded result URL is kept")
    return 0


if __name__ == "__main__":
    sys.exit(main())
