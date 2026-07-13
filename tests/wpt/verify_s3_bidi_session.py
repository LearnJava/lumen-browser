#!/usr/bin/env python3
"""S3 verification (docs/tasks/p2-wpt-integration.md): prove that a real,
spawned `lumen --bidi-port <port>` process completes WebDriver BiDi session
negotiation over a raw WebSocket, with no classic HTTP session involved —
exactly what `wptrunner.browsers.lumen.LumenBrowser` +
`wptrunner.executors.executorlumen.LumenBidiProtocol` do together, exercised
here directly (not through the full `wpt run` harness, which needs a working
`do_test`/testharnessreport shim — that's S4).

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/verify_s3_bidi_session.py [--binary PATH]

Defaults to `target/<LUMEN_PROFILE>/lumen.exe` (`LUMEN_PROFILE` env var,
default `release`), same convention as `graphic_tests/run.py`. Exits 0 and
prints "S3 OK" on success; non-zero with a message otherwise.
"""

import argparse
import asyncio
import os
import socket
import subprocess
import sys
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
]

from webdriver.bidi.client import BidiSession  # noqa: E402


def get_free_port() -> int:
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def wait_for_port(host: str, port: int, proc: subprocess.Popen, timeout: float) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"lumen exited early with code {proc.returncode}")
        s = socket.socket()
        try:
            s.connect((host, port))
            return
        except OSError:
            time.sleep(0.05)
        finally:
            s.close()
    raise TimeoutError(f"BiDi port {port} did not open within {timeout}s")


async def negotiate_session(ws_url: str) -> None:
    session = BidiSession.bidi_only(ws_url)
    await session.start()
    try:
        assert session.session_id, "session.new did not return a sessionId"
        assert isinstance(session.capabilities, dict), "session.new did not return capabilities"
        print(f"S3 OK: sessionId={session.session_id!r} capabilities={session.capabilities!r}")
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

    host = "127.0.0.1"
    port = get_free_port()
    proc = subprocess.Popen([args.binary, "--bidi-port", str(port)])
    try:
        wait_for_port(host, port, proc, timeout=30)
        asyncio.run(negotiate_session(f"ws://{host}:{port}"))
    except Exception as e:
        print(f"S3 FAILED: {e}", file=sys.stderr)
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
