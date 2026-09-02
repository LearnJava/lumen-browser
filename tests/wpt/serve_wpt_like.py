#!/usr/bin/env python3
"""Serve the WPT checkout the way `wptserve` does, for a single-test live probe.

`docs/probe-method.md` §2 requires a probe to run against a real http origin, not
`file://`. A bare `python -m http.server` over `tests/wpt/` gets the file bytes
right but serves `/resources/testharnessreport.js` *unsubstituted* — the real
route (`environment.py::get_routes`) concatenates `executors/message-queue.js`
with `wptrunner/testharnessreport.js` and fills in `%(output)d` /
`%(timeout_multiplier)s` / `%(explicit_timeout)s` / `%(debug)s`. Without that
substitution the file is a syntax error (`Unexpected token '%'`), which reads
exactly like a synchronously-hung page regardless of what the test under probe
actually does (WPT-RUN-6 slice 37's gotcha — CLAUDE.md "WPT harness").

This script reproduces only that one route; everything else is a plain static
file server rooted at `tests/wpt/` (the WPT doc root in this checkout), which is
enough for `<script src="/resources/...">`/`/mathml/support/...` to resolve.
`/resources/testdriver.js` is NOT reproduced — a probe that needs
`test_driver_internal` should not use this script (BUG-810: almost nothing is
wired anyway).

**Observability.** A real corpus run drives `output: 0` (see `_FORMAT_ARGS`
below, matching `run_report.py`'s defaults), which is `testharness.js`'s switch
for "do not render the `#log` summary" (`this.output = settings.output && …`,
`resources/testharness.js`) — the same value wptrunner uses, so a probe under
this server sees exactly what the corpus run saw, but that also means nothing
observable is written to the DOM. `--inject-marker` (on by default) appends a
harmless `add_completion_callback` right before `</body>` of any `.html`
response, printing one `console.log("PROBE harness-complete …")` line with the
final status and every subtest's name/status. This does not touch any state
the test itself registers — it only *adds* a second, independent completion
callback — so it cannot change what the test asserts, only whether the probe
can see the result. Pass `--no-inject-marker` to serve bytes completely
unmodified (e.g. when the hypothesis under test is about parsing/loading
itself, not about completion).

Usage:

    tests/wpt/.venv/bin/python tests/wpt/serve_wpt_like.py [--port N] [--no-inject-marker]

Prints the port and blocks serving until Ctrl-C.
"""

import argparse
import http.server
import os
import socketserver
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WPTRUNNER = os.path.join(os.path.dirname(HERE), "..", "tools", "wptrunner", "wptrunner")
WPTRUNNER = os.path.normpath(WPTRUNNER)

# Same defaults `run_report.py`'s corpus runs use: no pause-after-test, no
# multiplier, no debugger, no --debug-test.
_FORMAT_ARGS = {
    "output": 0,
    "timeout_multiplier": 1,
    "explicit_timeout": "false",
    "debug": "false",
}


def _build_testharnessreport():
    message_queue = os.path.join(WPTRUNNER, "executors", "message-queue.js")
    report_template = os.path.join(WPTRUNNER, "testharnessreport.js")
    with open(message_queue, encoding="utf-8") as f:
        mq_text = f.read()
    with open(report_template, encoding="utf-8") as f:
        report_text = f.read() % _FORMAT_ARGS
    return (mq_text + "\n" + report_text).encode("utf-8")


_MARKER_SCRIPT = b"""
<script>
(function () {
  if (typeof add_completion_callback !== "function") return;
  add_completion_callback(function (tests, status) {
    var subtests = tests.map(function (t) { return t.name + ":" + t.status; }).join("|");
    console.log("PROBE harness-complete status=" + status.status +
                " tests=" + tests.length + " " + subtests);
  });
})();
</script>
</body>"""


class Handler(http.server.SimpleHTTPRequestHandler):
    inject_marker = True

    def log_message(self, *args):
        pass

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/resources/testharnessreport.js":
            body = _build_testharnessreport()
            self.send_response(200)
            self.send_header("Content-Type", "text/javascript;charset=utf8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.inject_marker and path.endswith(".html"):
            fs_path = self.translate_path(self.path)
            if os.path.isfile(fs_path):
                with open(fs_path, "rb") as f:
                    body = f.read()
                if b"</body>" in body:
                    body = body.replace(b"</body>", _MARKER_SCRIPT, 1)
                else:
                    body = body + _MARKER_SCRIPT
                self.send_response(200)
                self.send_header("Content-Type", "text/html;charset=utf8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
        return super().do_GET()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--inject-marker", dest="inject_marker", action="store_true", default=True)
    parser.add_argument("--no-inject-marker", dest="inject_marker", action="store_false")
    args = parser.parse_args()

    def handler(*a, **kw):
        h = Handler(*a, directory=HERE, **kw)
        return h

    Handler.inject_marker = args.inject_marker

    with socketserver.TCPServer(("127.0.0.1", args.port), handler) as httpd:
        print(httpd.server_address[1], flush=True)
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
