#!/usr/bin/env python3
"""WPT-RUN-6 slice 38: the two MathML ids left by slice 37, plus one fresh id.

    /mathml/presentation-markup/mrow/legacy-mrow-like-elements-001.html
    /mathml/presentation-markup/mpadded/mpadded-003.html
    /uievents/legacy-domevents-tests/approved/dispatchEvent.click.checkbox.html

**legacy-mrow-like-elements-001.html / mpadded-003.html**: both use the exact
idiom already refuted for `mathml/relations/css-styling/ignored-properties-001.
html` in slice 37 — `setup({explicit_done:true})` + `window.addEventListener(
"load", runTests)` with a synchronous `done()` at the end of `runTests`. Slice
37 left them as "задел на следующий срез" because the same hang cause was
unlikely (`test`/`assert_true` catch their own exceptions) but unmeasured.
Served through `serve_wpt_like.py` (now committed — was the throwaway
`.tmp/serve_wpt_like.py` slice 37's commit message described but never
checked in) with its `add_completion_callback` marker, both complete
instantly and cleanly under a live `--mcp-live-port` window. Hypothesis
refuted for both — same pattern as `video_crash_empty_src.html` (slice 34)
and `ignored-properties-001.html` (slice 37): the TIMEOUT in the WPT-RUN-5
snapshot finds no cause in the test's own code. Both stay unclassified with
no new marker.

**dispatchEvent.click.checkbox.html**: `grep -rn "createEvent\\b"
crates/js/src/shim/*.js` — 0 matches, matching already-open BUG-590
(`document.createEvent` missing entirely, filed 2026-08-04). The test's
`TestEvent` handler calls `e = document.createEvent("MouseEvent");
e.initMouseEvent(...)` *inside* a `test()` callback (so the `TypeError` is
caught by `testharness.js` and only fails that one subtest) but then calls
`TARGET.dispatchEvent(e)` *outside* the `test()` callback, in the raw
`TestEvent` function body — a native-event listener callback invoked by
`BUTTON.click()`. Reduced live to the general shape (`--variant
create-event-throws` below): a `TypeError` thrown inside a callback the
*browser itself* invokes via native `.click()` prints nothing at all — no
`script error:`, no `[JS error]` — and execution resumes normally on the
*next* top-level statement, exactly the same swallow BUG-871 already
describes for `message` listeners, just for a native click dispatch instead.
`TARGET.dispatchEvent(e)` with `e` left `undefined` never runs, `TARGET`
never receives its `click`, and the harness never reaches `done()`.
Classified against BUG-590 (the missing API is this id's own explanation;
BUG-871 already owns the general swallow shape, so no new bug filed) —
`legacy-create-event-missing` marker added to `SOURCE_MARKERS`.

Also fixed in this slice: slice 37's own commit (`b1c9458bf`) filed BUG-959
but never added a classifier entry for it, so `timeout_audit.py` still
counted 42 unclassified instead of the 41 the commit message claimed. Added
`worker-raf-missing` to `WORKER_SOURCE_MARKERS`.

unclassified 42 (script drifted from the real count) → 41 (BUG-959 marker,
correcting slice 37) → 40 (BUG-590 marker, this slice's own finding).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice38_gaps.py
        [--binary target/dev-release/lumen] [--seconds 10]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import os
import re
import socketserver
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

sys.path.insert(0, HERE)
import serve_wpt_like  # noqa: E402

_CREATE_EVENT_PAGE = """<!doctype html><meta charset=utf-8>
<title>slice-38 probe: create-event-throws</title>
<body>
<button id=b>x</button>
<script>
function rep(s) { console.log("PROBE " + s); }
document.getElementById("b").addEventListener("click", function (evt) {
  rep("button-listener-fired");
  var e = document.createEvent("MouseEvent");  // throws TypeError, uncaught
  e.initMouseEvent("click", false, true, window, 1, 0, 0, 0, 0,
                    false, false, false, false, 0, null);
  rep("after-throw-never-prints");
}, true);
document.getElementById("b").click();
rep("native-click-returned");
setTimeout(function () { rep("tick-2s"); }, 2000);
</script>
"""

_TIMEOUT_RE = re.compile(r"PROBE tick")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


def _serve(port_out):
    def handler(*a, **kw):
        return serve_wpt_like.Handler(*a, directory=HERE, **kw)

    server = socketserver.TCPServer(("127.0.0.1", 0), handler)
    port_out.append(server.server_address[1])
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def _run(binary, url, seconds, log_name):
    log_path = os.path.join(REPO, ".tmp", log_name)
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", "0", url],
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
        return log.read()


def _markers(text):
    out = []
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker not in out:
            out.append(marker)
    for err in dict.fromkeys(_ERROR_RE.findall(text)):
        out.append(f"[engine] {err.strip()}")
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=10.0)
    args = parser.parse_args()

    port_out = []
    server = _serve(port_out)
    port = port_out[0]

    probe_path = os.path.join(HERE, ".s38-create-event-throws.html")
    with open(probe_path, "w", encoding="utf-8") as f:
        f.write(_CREATE_EVENT_PAGE)

    cases = [
        ("mathml-legacy-mrow", f"http://127.0.0.1:{port}/mathml/presentation-markup/mrow/"
                                "legacy-mrow-like-elements-001.html",
         "harness completes cleanly (refutes hang)"),
        ("mathml-mpadded-003", f"http://127.0.0.1:{port}/mathml/presentation-markup/mpadded/"
                                "mpadded-003.html",
         "harness completes cleanly (refutes hang)"),
        ("create-event-throws", f"http://127.0.0.1:{port}/.s38-create-event-throws.html",
         "after-throw never prints; native-click-returned/tick-2s still do"),
    ]

    try:
        print(f"{'variant':22s} {'expected':55s} markers seen")
        for name, url, expected in cases:
            text = _run(args.binary, url, args.seconds, f"s38-{name}.log")
            markers = _markers(text)
            seen = ", ".join(markers) if markers else "— nothing"
            print(f"{name:22s} {expected:55s} {seen}")
    finally:
        server.shutdown()
        try:
            os.remove(probe_path)
        except OSError:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
