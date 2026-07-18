#!/usr/bin/env python3
"""S6 verification (docs/tasks/p2-wpt-integration.md): verify how Lumen's
WebDriver BiDi `script.evaluate` handles the `awaitPromise` parameter, against
a real spawned `lumen --bidi-port <port>` process.

Why this exists: the P2-wpt async subset (S6) drives `promise_test`/`async_test`
tests through `LumenTestharnessExecutor`, which deliberately uses
`awaitPromise=False` and polls `window.__lumen_wpt_results` — async tests
complete via the page's own event loop + testharness completion callback, not
via BiDi `awaitPromise`. This script pins the *independent* `awaitPromise`
behavior of `script.evaluate` so a future fix (or regression) is visible.

Current, verified behavior (2026-07-18): `awaitPromise` is **ignored** — a
promise-valued expression returns the unsettled promise object (serialized as
`{"type":"string","value":"{}"}`) regardless of `awaitPromise`. Tracked as
[BUG-317](../../bugs/BUG-317-OPEN.md). When BUG-317 lands, flip the
`EXPECT_AWAIT_PROMISE_RESOLVES` assertions below to the resolved-value form.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/verify_s6_await_promise.py [--binary PATH]

Defaults to `target/<LUMEN_PROFILE>/lumen.exe`. Exits 0 and prints "S6 OK" when
the observed behavior matches the pinned expectation; non-zero otherwise.
"""

import argparse
import asyncio
import os
import socket
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
]

from webdriver.bidi.client import BidiSession  # noqa: E402
from webdriver.bidi.modules.script import ContextTarget  # noqa: E402

#: Flip to True once BUG-317 (awaitPromise support) lands — then a
#: promise-valued expression evaluated with awaitPromise=True must return its
#: resolved RemoteValue, not the promise object.
EXPECT_AWAIT_PROMISE_RESOLVES = False

#: A promise object currently serializes to this best-effort RemoteValue (full
#: RemoteValue serialization is future work; see `eval_result_to_remote_value`).
PROMISE_OBJECT_REMOTE_VALUE = {"type": "string", "value": "{}"}


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


async def _eval(session, ctx, expr, await_promise):
    """Evaluate `expr`, retrying past the off-thread JS-context install race
    (`script.evaluate` reports "JS context not available" until the new
    document's JS runtime is up — same as `LumenTestharnessExecutor`)."""
    from webdriver.bidi.error import UnknownErrorException

    deadline = asyncio.get_running_loop().time() + 10
    while True:
        try:
            return await session.script.evaluate(
                expression=expr, target=ContextTarget(ctx),
                await_promise=await_promise)
        except UnknownErrorException as e:
            if ("JS context not available" in e.message
                    and asyncio.get_running_loop().time() < deadline):
                await asyncio.sleep(0.1)
                continue
            raise


async def verify(ws_url: str, page_url: str) -> None:
    session = BidiSession.bidi_only(ws_url)
    await session.start()
    try:
        contexts = await session.browsing_context.get_tree()
        ctx = contexts[0]["context"]
        # A page with a <script> installs the JS runtime (an empty about:blank
        # window has no JS context — `script.evaluate` would never succeed).
        await session.browsing_context.navigate(
            context=ctx, url=page_url, wait="complete")

        # Sanity: synchronous eval returns a proper typed RemoteValue.
        sync = await _eval(session, ctx, "1+1", False)
        assert sync == {"type": "number", "value": 2}, f"sync eval: {sync!r}"
        print(f"  sync 1+1 -> {sync}")

        # awaitPromise behavior (the thing under test).
        resolved = await _eval(session, ctx, "Promise.resolve(42)", True)
        print(f"  Promise.resolve(42) awaitPromise=True -> {resolved}")
        if EXPECT_AWAIT_PROMISE_RESOLVES:
            assert resolved == {"type": "number", "value": 42}, \
                f"awaitPromise should resolve, got {resolved!r}"
        else:
            # BUG-317: awaitPromise ignored, promise object returned as-is.
            assert resolved == PROMISE_OBJECT_REMOTE_VALUE, \
                (f"awaitPromise behavior changed (BUG-317 fixed? flip "
                 f"EXPECT_AWAIT_PROMISE_RESOLVES): got {resolved!r}")

        print(f"S6 OK: awaitPromise verified "
              f"({'resolves' if EXPECT_AWAIT_PROMISE_RESOLVES else 'ignored - BUG-317'})")
    finally:
        await session.end()


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
        page = os.path.join(tmp, "s6-probe.html")
        with open(page, "w", encoding="utf-8") as f:
            f.write("<!DOCTYPE html><html><body>s6"
                    "<script>window.__s6=1;</script></body></html>")

        port = get_free_port()
        proc = subprocess.Popen([args.binary, "--bidi-port", str(port)])
        try:
            wait_for_port(port, proc, timeout=30)
            asyncio.run(verify(f"ws://127.0.0.1:{port}", page))
        except Exception as e:
            print(f"S6 FAILED: {e}", file=sys.stderr)
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
