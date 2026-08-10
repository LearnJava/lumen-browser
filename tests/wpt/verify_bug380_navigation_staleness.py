#!/usr/bin/env python3
"""[BUG-380](../../bugs/BUG-380-FIXED.md) regression check: a test whose
navigation fails must not inherit the *previous* test's testharness results.

`LumenTestharnessExecutor` reuses one browsing context for a whole run and used
to rely on "a navigation always gives a fresh `window`" to isolate tests. That
holds only for navigations that actually load: for an unreachable URL Lumen's
`browsingContext.navigate` still answers `{navigation, url}` while the previous
document — `location.href`, `window.__lumen_wpt_results` and all — stays live,
so the next test read the previous test's result and wptrunner turned that into
`AssertionError: Got results from X, expected Y` (`base.py:104`) instead of the
test's real outcome.

This drives the real `_run_testharness` coroutine (not a reimplementation of it)
against a spawned `lumen --bidi-port <port>`, over two navigations:

1. a local page that stashes a well-formed result on `window.__lumen_wpt_results`
   -> must return exactly that result;
2. an unreachable `http://127.0.0.1:<closed port>/` -> must raise
   `ExecutorException` naming the un-replaced document, **not** return the
   result of step 1.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/verify_bug380_navigation_staleness.py [--binary PATH]

Defaults to `target/<LUMEN_PROFILE>/lumen.exe`. Exits 0 and prints
"BUG-380 OK" when the executor isolates the two tests; non-zero otherwise.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    os.path.join(REPO_ROOT, "tools", "wptrunner"),
]

from wptrunner.executors.base import ExecutorException  # noqa: E402
from wptrunner.executors.executorlumen import (  # noqa: E402
    RESULTS_GLOBAL,
    LumenTestharnessExecutor,
)

#: Result page 1 stashes — the shape `testharnessreport.js` produces and
#: `testharness_result_converter` consumes: [url, status, message, stack, subtests].
PAGE_ONE_RESULT = ["/bug380/page-one.html", 0, None, None, []]


class _StubBrowser:
    """The two attributes `LumenBidiProtocol.connect` reads off `browser`
    (`ExecutorBrowser` in a real run)."""

    def __init__(self, bidi_url, token):
        self.bidi_url = bidi_url
        self.token = token


class _StubLogger:
    """`Protocol.__init__` stores `executor.logger`; only the debug path in
    `teardown` ever calls it here."""

    def debug(self, *args, **kwargs):
        pass

    warning = info = error = debug


def get_free_port() -> int:
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def wait_for_port(port: int, proc: subprocess.Popen, timeout: float) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"lumen exited early with code {proc.returncode}")
        s = socket.socket()
        try:
            s.connect(("127.0.0.1", port))
            return
        except OSError:
            time.sleep(0.05)
        finally:
            s.close()
    raise TimeoutError(f"BiDi port {port} did not open within {timeout}s")


def read_token_and_drain(stderr) -> str:
    """Read the ADR-024 §Access model (DEVX-15) `[bidi] token: <token>` line the
    child prints to stderr, then keep draining the rest in a background thread
    so unrelated stderr output cannot block the child."""
    token = None
    for _ in range(400):
        line = stderr.readline()
        if not line:
            break
        line = line.strip()
        if line.startswith("[bidi] token: "):
            token = line[len("[bidi] token: "):]
            break
    if token is None:
        raise RuntimeError("lumen --bidi-port did not print [bidi] token")

    def _drain() -> None:
        try:
            for _ in stderr:
                pass
        except Exception:
            pass

    threading.Thread(target=_drain, daemon=True).start()
    return token


def verify(bidi_url: str, token: str, page_one: str, dead_url: str) -> None:
    executor = LumenTestharnessExecutor(
        _StubLogger(), _StubBrowser(bidi_url, token), server_config=None)
    protocol = executor.protocol
    protocol.connect()
    try:
        protocol.after_connect()

        first = protocol.run(executor._run_testharness(page_one, 10))
        assert first == PAGE_ONE_RESULT, f"page one result: {first!r}"
        print(f"  test 1 (loads)          -> {first}")

        try:
            second = protocol.run(executor._run_testharness(dead_url, 10))
        except ExecutorException as e:
            assert e.status == "ERROR", f"expected ERROR status, got {e.status!r}"
            assert "never replaced" in e.message, f"unexpected message: {e.message!r}"
            print(f"  test 2 (fails to load)  -> {e.status}: {e.message}")
        else:
            raise AssertionError(
                "BUG-380 regression: a failed navigation returned a result instead "
                f"of erroring: {second!r}"
                + (" (this is test 1's result)" if second == PAGE_ONE_RESULT else ""))
    finally:
        protocol.teardown()

    print("BUG-380 OK: a failed navigation errors out instead of "
          "inheriting the previous test's results")


def default_binary() -> str:
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        page_one = os.path.join(tmp, "page-one.html")
        with open(page_one, "w", encoding="utf-8") as f:
            f.write("<!DOCTYPE html><html><body>bug380 page one<script>"
                    f"window.{RESULTS_GLOBAL} = "
                    f"{json.dumps(json.dumps(PAGE_ONE_RESULT))};"
                    "</script></body></html>")

        # A port nothing listens on: navigation fails, and (BUG-380) Lumen
        # keeps the current document rather than erroring over BiDi.
        dead_url = f"http://127.0.0.1:{get_free_port()}/bug380/never-loads.html"

        port = get_free_port()
        proc = subprocess.Popen(
            [args.binary, "--bidi-port", str(port)],
            stderr=subprocess.PIPE, text=True,
        )
        try:
            wait_for_port(port, proc, timeout=40)
            token = read_token_and_drain(proc.stderr)
            verify(f"ws://127.0.0.1:{port}", token, page_one, dead_url)
        except Exception as e:
            print(f"BUG-380 FAILED: {e}", file=sys.stderr)
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
