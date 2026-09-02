#!/usr/bin/env python3
"""WPT-RUN-6 slice 36: two candidates read off the 44 ids left by slice 35.

    /html/semantics/embedded-content/the-iframe-element/iframe_sandbox_allow_top_navigation_by_user_activation_with_user_gesture.html
    /html/semantics/embedded-content/the-iframe-element/iframe_sandbox_allow_top_navigation_by_user_activation_without_user_gesture.html
    /content-security-policy/frame-ancestors/report-blocked-frame.sub.html
    /content-security-policy/frame-ancestors/report-only-frame.sub.html

**iframe_sandbox_allow_top_navigation_by_user_activation_*.html**: both call
`let parent = open(\n  "support/...");` (arguments on their own line) and arm
`window.addEventListener(\n  "message", ...)` the same way. This is exactly
the already-known `open-freezes-opener` mechanism (BUG-883, slice 28) — but
`timeout_audit.py`'s own regex for it (`open\(['"]`, `addEventListener\(['"]`)
requires the quote to sit immediately after the paren, so a call written
across two lines defeats both patterns and the id falls through to
unclassified. This is a classifier gap, not a new engine defect — confirmed
by reproducing the two-line call shape directly.

**report-blocked-frame.sub.html / report-only-frame.sub.html**: both load
`support/checkReport.sub.js`, which does not wait on any event at all — it
opens a synchronous-looking but actually long-polling `XMLHttpRequest` GET to
`reporting/resources/report.py?op=retrieve_report&timeout=20&...` and reads
the JSON in `.onload`. `report.py`'s `retrieve_from_stash` is a plain
`time.sleep(0.5)` server-side loop bounded by `timeout`; it always returns
(`'[]'` if nothing showed up), so if our XHR's GET completes normally after a
multi-second server delay, this reduces to "the CSP report is never sent"
(BUG-811 territory) and the harness should reach `onload` and FAIL/pass
normally, not TIMEOUT. Hypothesis: an XHR GET whose response is deliberately
delayed several seconds server-side never fires `onload` in this engine —
confirmed or refuted here directly against a throwaway slow endpoint (no CSP
or iframe machinery involved, to isolate the XHR behaviour from BUG-811).

Same harness as slices 30-35: one browser process per page, served over http,
evidence read off the browser's own stderr via `console.log`.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice36_gaps.py
        [--binary target/dev-release/lumen] [--seconds 8] [--variant NAME]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import http.server
import os
import re
import socket
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-36 probe: __NAME__</title>
<body>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

REPORT = """
<script>
function rep(label, fn) {
  try {
    var v = fn();
    console.log("PROBE " + label + " = " + v);
  } catch (e) {
    console.log("PROBE " + label + " THREW " + (e && e.name ? e.name + ": " : "") +
                (e && e.message ? e.message : e));
  }
}
</script>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, load"),

    # iframe_sandbox_allow_top_navigation_by_user_activation_*.html's exact
    # shape: a two-line `open(\n  "url")` call plus a two-line
    # `addEventListener(\n  "message", ...)` listener registered first.
    "sandbox-open-multiline": ("""
""" + REPORT + """
<script>
window.addEventListener(
  "message",
  function (e) {
    console.log("PROBE message-received data=" + JSON.stringify(e.data));
  }
);
console.log("PROBE before-open");
let child = open(
  "slice36-child.html"
);
console.log("PROBE after-open child=" + child);
setTimeout(function () { console.log("PROBE sandbox-open-done"); }, 3000);
</script>
""", "after-open never prints (caller's document is replaced by open())"),

    # checkReport.sub.js's exact shape, reduced: an XHR GET whose response is
    # deliberately delayed 3s server-side (report.py's own retrieve_from_stash
    # sleeps up to `timeout` seconds before answering).
    "slow-xhr-get": ("""
""" + REPORT + """
<script>
console.log("PROBE before-xhr");
var report = new XMLHttpRequest();
report.onload = function () {
  console.log("PROBE xhr-onload status=" + report.status + " body=" + report.responseText);
};
report.onerror = function () { console.log("PROBE xhr-onerror"); };
report.open("GET", "/slow?delay=3", true);
report.send();
console.log("PROBE after-xhr-send");
setTimeout(function () { console.log("PROBE slow-xhr-done"); }, 6000);
</script>
""", "xhr-onload fires ~3s after after-xhr-send"),
}


class _SlowHandler(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path.startswith("/slow"):
            delay = 3.0
            if "delay=" in self.path:
                try:
                    delay = float(self.path.split("delay=", 1)[1].split("&", 1)[0])
                except ValueError:
                    pass
            time.sleep(delay)
            body = b'{"ok":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        return super().do_GET()


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _SlowHandler(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


def _run_variant(binary, name, http_port, seconds):
    log_path = os.path.join(REPO, ".tmp", f"s36-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".s36-{name}.html"
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/{page}"],
            stdout=subprocess.DEVNULL, stderr=log, text=True)
        try:
            time.sleep(seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    with open(log_path, encoding="utf-8", errors="replace") as log:
        text = log.read()
    ticks = len(_TICK_RE.findall(text))
    markers = []
    seen_markers = set()
    dropped = 0
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker.startswith("tick ") or marker in seen_markers:
            continue
        if len(markers) >= _MAX_MARKERS:
            dropped += 1
            continue
        seen_markers.add(marker)
        markers.append(marker)
    if dropped:
        markers.append(f"[+{dropped} more distinct markers, not shown]")
    for err in dict.fromkeys(_ERROR_RE.findall(text)):
        markers.append(f"[engine] {err.strip()}")
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=8.0)
    parser.add_argument("--variant", action="append", default=None)
    args = parser.parse_args()

    wanted = args.variant or list(VARIANTS)
    unknown = [name for name in wanted if name not in VARIANTS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    http_port, shutdown = _serve(HERE)
    written = []
    for name in wanted:
        path = os.path.join(HERE, f".s36-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)
    child_path = os.path.join(HERE, "slice36-child.html")
    with open(child_path, "w", encoding="utf-8") as handle:
        handle.write("<!doctype html><title>child</title><body>child page</body>")
    written.append(child_path)

    try:
        print(f"{'variant':22s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:22s} {ticks:5d}  {expected:62s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
    finally:
        shutdown()
        for path in written:
            try:
                os.remove(path)
            except OSError:
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
