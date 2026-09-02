#!/usr/bin/env python3
"""BUG-961 срез 44: isolate `mozprocess.ProcessHandler`'s `preexec_fn`
(`os.setpgid(0, 0)` — new process group for the child) as the actual
variable between срез 43's direct `subprocess.Popen` (<1.3s, no stall) and
срез 41/42's `LumenBrowser`-launched runs (~32s stall every time).

`mozprocess.processhandler.Process.__init__` (tests/wpt/.venv/lib/python3.14/
site-packages/mozprocess/processhandler.py:118-126) does, unconditionally
unless `ignore_children=True` (`WebDriverBrowser`/`LumenBrowser` never pass
that): `preexec_fn = lambda: os.setpgid(0, 0)`. Everything else in its
`subprocess.Popen.__init__` call is either a default srez 43 already used
(`close_fds=False`, `shell=False`, `cwd=None`) or `env`/`bufsize`, both ruled
out already (env: `WebDriverBrowser.env` merges `os.environ`, not a stripped
dict, see `browsers/base.py:324`; bufsize: срез 40 measured the read-side
callback at <0.12s total for the 20MB line). `preexec_fn` is the one
remaining structural difference. This slice re-runs срез 43's exact repro
twice back to back, same binary, same static server, same page — Variant P
(plain, срез 43's control) and Variant G (adds the identical `setpgidfn`).

Usage: tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice44_setpgid.py
"""

import asyncio
import http.server
import os
import socket
import socketserver
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")

sys.path[:0] = [
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    TESTS_ROOT,
]

import serve_wpt_like  # noqa: E402

TEST_ID = "/console/console-log-large-array.any.html"
BINARY = os.path.join(REPO_ROOT, "target", "dev-release", "lumen")

_ANY_HTML_TEMPLATE = """<!doctype html>
<meta charset=utf-8>
<script src=/resources/testharness.js></script>
<script src=/resources/testharnessreport.js></script>
<div id=log></div>
<script>
self.GLOBAL = {
  isWindow: function() { return true; },
  isWorker: function() { return false; },
  isShadowRealm: function() { return false; },
};
</script>
<script src=/console/console-log-large-array.any.js></script>
"""


def _free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _make_static_server():
    body = _ANY_HTML_TEMPLATE.encode("utf-8")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            if path == TEST_ID:
                self.send_response(200)
                self.send_header("Content-Type", "text/html;charset=utf8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            if path == "/resources/testharnessreport.js":
                rbody = serve_wpt_like._build_testharnessreport()
                self.send_response(200)
                self.send_header("Content-Type", "text/javascript;charset=utf8")
                self.send_header("Content-Length", str(len(rbody)))
                self.end_headers()
                self.wfile.write(rbody)
                return
            return super().do_GET()

    def handler(*a, **kw):
        return Handler(*a, directory=TESTS_ROOT, **kw)

    httpd = socketserver.TCPServer(("127.0.0.1", 0), handler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    return httpd, port


def _read_first_line(path):
    try:
        with open(path, "r") as f:
            return f.readline().strip()
    except OSError:
        return None


def _spawn_lumen(bidi_port, use_setpgid):
    env = dict(os.environ)
    env["LUMEN_NO_ADBLOCK"] = "1"
    cmd = [BINARY, "--bidi-port", str(bidi_port)]
    preexec_fn = None
    if use_setpgid:
        # Identical to mozprocess.processhandler.Process.__init__'s
        # `setpgidfn` (line ~123-124 of processhandler.py).
        def setpgidfn():
            os.setpgid(0, 0)
        preexec_fn = setpgidfn
    proc = subprocess.Popen(
        cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        bufsize=0, preexec_fn=preexec_fn)
    return proc


def _drain_stderr(proc, token_box, events, t0, stop_after):
    prefix = b"[bidi] token: "
    while True:
        line = proc.stderr.readline()
        if not line:
            if proc.poll() is not None or time.monotonic() - t0 > stop_after:
                return
            continue
        elapsed = time.monotonic() - t0
        if len(line) > 2000:
            if events.get("huge_line_at") is None:
                events["huge_line_at"] = elapsed
        if token_box.get("token") is None and line.startswith(prefix):
            token_box["token"] = line[len(prefix):].decode("utf-8", "replace").strip()
        if time.monotonic() - t0 > stop_after:
            return


def _wait_port(host, port, deadline):
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.05)
    return False


async def _navigate(url, token, bidi_port, seconds):
    from webdriver.bidi.client import BidiSession
    session = BidiSession.bidi_only(
        f"ws://127.0.0.1:{bidi_port}",
        requested_capabilities={"alwaysMatch": {"token": token}})
    await session.start(asyncio.get_event_loop())
    try:
        contexts = await session.browsing_context.get_tree()
        context = contexts[0]["context"]
        t0 = time.monotonic()
        try:
            await asyncio.wait_for(
                session.browsing_context.navigate(
                    context=context, url=url, wait="complete"),
                timeout=seconds)
            return time.monotonic() - t0, None
        except asyncio.TimeoutError:
            return None, f"timeout after {time.monotonic() - t0:.1f}s"
        except Exception as e:  # noqa: BLE001
            return None, f"{type(e).__name__}: {e} (after {time.monotonic() - t0:.1f}s)"
    finally:
        await session.end()


def _run_variant(label, static_url, use_setpgid, seconds=40.0):
    bidi_port = _free_port()
    t0 = time.monotonic()
    proc = _spawn_lumen(bidi_port, use_setpgid)
    token_box, events = {"token": None}, {"huge_line_at": None}
    reader = threading.Thread(
        target=_drain_stderr, args=(proc, token_box, events, t0, seconds + 15),
        daemon=True)
    reader.start()
    try:
        if not _wait_port("127.0.0.1", bidi_port, time.time() + 15):
            print(f"[{label}] lumen did not open bidi port")
            return
        deadline = time.time() + 10
        while token_box["token"] is None and time.time() < deadline:
            time.sleep(0.05)
        if token_box["token"] is None:
            print(f"[{label}] no bidi token seen")
            return
        elapsed, err = asyncio.run(
            _navigate(static_url, token_box["token"], bidi_port, seconds))
        if err is None:
            print(f"[{label}] navigate OK in {elapsed:.2f}s "
                  f"(huge_line_at={events['huge_line_at']!r})")
        else:
            print(f"[{label}] navigate FAILED: {err} "
                  f"(huge_line_at={events['huge_line_at']!r})")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


def main():
    if not os.path.isfile(BINARY):
        print(f"lumen binary not found: {BINARY}", file=sys.stderr)
        return 1
    httpd, static_port = _make_static_server()
    static_url = f"http://127.0.0.1:{static_port}{TEST_ID}"
    print(f"[probe] static server up: {static_url}")
    try:
        _run_variant("Variant P (plain Popen, no preexec_fn)", static_url, False)
        _run_variant("Variant G (setpgid preexec_fn, mozprocess-identical)", static_url, True)
    finally:
        httpd.shutdown()
        httpd.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
