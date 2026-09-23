#!/usr/bin/env python3
"""BUG-924: `<audio src>` relative/root-relative resolution — reproduce or confirm fixed.

Serves `tests/wpt/media/sine440.mp3` from a throwaway http root at three URL
forms on one page (absolute, root-relative, relative) and reads back both the
browser's own `loadeddata`/`error` events (via stderr `console.log`, BUG-799's
technique — a wedged/broken load answers no MCP `eval`) and the server's own
request log (`docs/probe-method.md` §1 — the page cannot be trusted about
whether it made a request).

Usage (from repo root):

    tests/wpt/.venv/Scripts/python.exe tests/wpt/verify_bug924_audio_src_resolve.py
        [--binary target/dev-release/lumen.exe] [--seconds 6]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import http.server
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
MEDIA_SRC = os.path.join(HERE, "media", "sine440.mp3")

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>BUG-924 probe</title>
<body>
<audio id=abs src="ABS_URL"></audio>
<audio id=rootrel src="/sine440.mp3"></audio>
<audio id=rel src="sine440.mp3"></audio>
<script>
["abs", "rootrel", "rel"].forEach(function (id) {
  var el = document.getElementById(id);
  el.addEventListener("loadeddata", function () {
    console.log("PROBE " + id + ":loadeddata currentSrc=" + el.currentSrc
      + " duration=" + el.duration);
  });
  el.addEventListener("error", function () {
    console.log("PROBE " + id + ":error code=" + (el.error ? el.error.code : "?"));
  });
});
</script>
"""

_REQ_RE = re.compile(r"REQ (\S+)")
_EVT_RE = re.compile(r"PROBE (\S+)")


class _Logging(http.server.SimpleHTTPRequestHandler):
    """Serves a throwaway root; prints every request path to stdout (captured)."""

    def log_message(self, fmt, *args):
        pass

    def do_GET(self):
        print(f"REQ {self.path}", flush=True)
        super().do_GET()


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen.exe"))
    parser.add_argument("--seconds", type=float, default=6.0)
    args = parser.parse_args()

    root = os.path.join(REPO, ".tmp", "bug924-probe")
    os.makedirs(root, exist_ok=True)
    shutil.copyfile(MEDIA_SRC, os.path.join(root, "sine440.mp3"))

    port = _free_port()
    abs_url = f"http://127.0.0.1:{port}/sine440.mp3"
    with open(os.path.join(root, "index.html"), "w", encoding="utf-8") as fh:
        fh.write(PAGE.replace("ABS_URL", abs_url))

    def handler(*a, **kw):
        return _Logging(*a, directory=root, **kw)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()

    log_path = os.path.join(REPO, ".tmp", "bug924-probe.log")
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [args.binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{port}/index.html"],
            stdout=log, stderr=subprocess.STDOUT, text=True)
        try:
            time.sleep(args.seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    server.shutdown()

    with open(log_path, encoding="utf-8", errors="replace") as log:
        text = log.read()
    reqs = _REQ_RE.findall(text)
    events = _EVT_RE.findall(text)

    print("requests seen by server:", reqs or "(none)")
    print("events seen by page:", events or "(none)")

    for name in ("abs", "rootrel", "rel"):
        loaded = any(e.startswith(f"{name}:loadeddata") for e in events)
        errored = any(e.startswith(f"{name}:error") for e in events)
        print(f"  {name:8s} loadeddata={loaded} error={errored}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
