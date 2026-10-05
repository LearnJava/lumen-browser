#!/usr/bin/env python3
"""Regression check: the reftest executor does not read a late IPC reply as
the next test's answer (`executorlumen.py`: `LumenIpcProtocol.drop`,
`LumenRefTestExecutor.screenshot`; `docs/tasks/p2-wpt-runner-throughput.md`
§Рассинхрон IPC рефтестов).

`lumen --ipc-server` speaks strict request/response with no request id. When
a render overran its timeout, the executor returned TIMEOUT but kept the
socket; the late `Screenshot` reply was then read by the *next* test's
`NavigateTab` ("expected Navigated, got variant 9" → FAIL), that test's own
`Navigated` by its `Screenshot`, and so on — every later reftest on that
browser was a FAIL that never compared a pixel (37 of 1 970 ids on a
2026-10-05 control set, all after six render timeouts in `density-size-
correction`).

Against a fake `--ipc-server` (no browser needed) whose first `Screenshot`
answers only after the client's timeout:

1. the timed-out test is `EXTERNAL-TIMEOUT` (reported as TIMEOUT, restarts the
   browser) and the protocol is no longer alive — the stale socket is dropped;
2. a `TabError` reply stays a plain FAIL and keeps the connection (the stream
   is still in step);
3. the pin reproduces the bug: on a socket that is *not* dropped, the next
   `NavigateTab` really does read the late `Screenshot` (variant 9) — without
   this, check 1 would prove nothing.

Usage (from repo root, venv as in tests/wpt/README.md):

    <venv>/python tests/wpt/verify_ipc_desync.py

Exits 0 and prints "verify_ipc_desync: PASS" on success.
"""

import os
import socket
import struct
import sys
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools"),
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    os.path.join(REPO_ROOT, "tools", "wptrunner"),
]

from wptrunner.executors import executorlumen as el  # noqa: E402

TOKEN = "t0k"
PNG = b"\x89PNG\r\n\x1a\nfake"
#: The client times out after `CLIENT_TIMEOUT_S`; the fake server answers the
#: slow screenshot after `SLOW_REPLY_S`.
CLIENT_TIMEOUT_S = 0.5
SLOW_REPLY_S = 1.0


def _frame(body: bytes) -> bytes:
    return struct.pack("<I", len(body)) + body


def _read_exact(conn, n):
    buf = b""
    while len(buf) < n:
        chunk = conn.recv(n - len(buf))
        if not chunk:
            raise EOFError
        buf += chunk
    return buf


class FakeIpcServer(threading.Thread):
    """Answers Auth/CreateTab/NavigateTab/Screenshot like `run_ipc_server`.
    A navigation to a URL containing `slow` makes the following screenshot
    late; one containing `taberror` makes it a `TabError`."""

    def __init__(self):
        super().__init__(daemon=True)
        self.sock = socket.socket()
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(4)
        self.port = self.sock.getsockname()[1]

    def run(self):
        while True:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self.serve, args=(conn,), daemon=True).start()

    def serve(self, conn):
        mode = ""
        try:
            while True:
                (length,) = struct.unpack("<I", _read_exact(conn, 4))
                body = _read_exact(conn, length)
                (tag,) = struct.unpack("<I", body[:4])
                if tag == el._REQ_AUTH:
                    conn.sendall(_frame(struct.pack("<I", el._RESP_AUTH_OK)))
                elif tag == el._REQ_CREATE_TAB:
                    conn.sendall(_frame(struct.pack("<II", el._RESP_TAB_CREATED, 1)))
                elif tag == el._REQ_NAVIGATE_TAB:
                    (n,) = struct.unpack("<Q", body[8:16])
                    url = body[16:16 + n].decode()
                    mode = "slow" if "slow" in url else "taberror" if "taberror" in url else ""
                    conn.sendall(_frame(struct.pack("<II", el._RESP_NAVIGATED, 1)))
                elif tag == el._REQ_SCREENSHOT:
                    if mode == "slow":
                        time.sleep(SLOW_REPLY_S)
                    if mode == "taberror":
                        msg = b"render failed"
                        conn.sendall(_frame(struct.pack("<II", el._RESP_TAB_ERROR, 1)
                                            + struct.pack("<Q", len(msg)) + msg))
                    else:
                        conn.sendall(_frame(struct.pack("<II", el._RESP_SCREENSHOT, 1)
                                            + struct.pack("<Q", len(PNG)) + PNG))
        except (EOFError, OSError):
            pass


class FakeBrowser:
    def __init__(self, port):
        self.ipc_port = port
        self.ipc_token = TOKEN


class FakeTest:
    def __init__(self, url):
        self.url = url
        self.timeout = CLIENT_TIMEOUT_S


def make_executor(port):
    executor = el.LumenRefTestExecutor.__new__(el.LumenRefTestExecutor)
    executor.timeout_multiplier = 1
    executor.extra_timeout = 0
    executor.test_url = lambda test: test.url
    executor.protocol = el.LumenIpcProtocol(executor, FakeBrowser(port))
    executor.protocol.connect()
    executor.protocol.after_connect()
    return executor


def main() -> int:
    server = FakeIpcServer()
    server.start()
    failures = []

    executor = make_executor(server.port)
    ok, data = executor.screenshot(FakeTest("http://x/slow.html"), None, None, None)
    print(f"  timed-out render: {ok}, {data[0]}, alive={executor.protocol.is_alive()}")
    if ok or data[0] != "EXTERNAL-TIMEOUT":
        failures.append(f"timed-out render gave {ok}, {data[0]!r}, expected EXTERNAL-TIMEOUT")
    if executor.protocol.is_alive():
        failures.append("the socket that still owes a reply was kept — the next test "
                        "would read it")
    ok, data = executor.screenshot(FakeTest("http://x/next.html"), None, None, None)
    if ok or data[0] != "CRASH":
        failures.append(f"a test on the dropped connection gave {ok}, {data!r}, expected CRASH")

    executor = make_executor(server.port)
    ok, data = executor.screenshot(FakeTest("http://x/taberror.html"), None, None, None)
    print(f"  TabError: {ok}, {data[0]}, alive={executor.protocol.is_alive()}")
    if ok or data[0] != "FAIL" or not executor.protocol.is_alive():
        failures.append(f"TabError gave {ok}, {data!r}, alive={executor.protocol.is_alive()}; "
                        f"expected FAIL on a live connection")
    ok, data = executor.screenshot(FakeTest("http://x/fine.html"), None, None, None)
    if not ok:
        failures.append(f"the test after a TabError gave {data!r}, expected a screenshot")

    # The bug itself, reproduced on a socket the executor does not drop.
    protocol = make_executor(server.port).protocol
    protocol.sock.settimeout(CLIENT_TIMEOUT_S)
    protocol.navigate("http://x/slow.html")
    try:
        protocol.screenshot_png()
        failures.append("the slow screenshot did not time out — the pin is not exercising "
                        "the late reply")
    except socket.timeout:
        pass
    time.sleep(SLOW_REPLY_S)
    protocol.sock.settimeout(5)
    try:
        protocol.navigate("http://x/next.html")
        failures.append("a kept socket read the next reply in step — the pin no longer "
                        "reproduces the desync, so the checks above prove nothing")
    except el.IpcError as e:
        print(f"  kept socket, next navigate: {e}")

    for failure in failures:
        print(f"FAIL  {failure}")
    print("verify_ipc_desync: " + ("FAIL" if failures else "PASS"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
