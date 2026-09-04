#!/usr/bin/env python3
"""WPT-RUN-6 slice 58: one unclassified-list candidate left unread by slice 57
for budget reasons, plus a minimal synthetic repro of the mechanism it turned
out to hide:

    /mediacapture-record/MediaRecorder-destroy-script-execution.html

Slice 57's docstring deferred this id: `subFrameStart.window`-style
named-window access stacks "multiple already-known mechanisms" (BUG-480
territory) on top of a `captureStream`/fake-track support harness, "not a
clean single root cause worth this slice's budget". Reading it here first —
`grep -rn captureStream crates/` is still empty, but the test never gets that
far: it dies on `subFrameStart.window`, one line into every one of its four
`onload` handlers, before `captureStream` (or the DOMException-identity
check the docstring also flagged) is ever reached — none of those stacked
mechanisms is actually load-bearing for THIS id's TIMEOUT.

`SYNTH_NAMED_WINDOW` below reproduces the shape in isolation, minus
`mediacapture-record` entirely: a same-origin `<iframe name="subFrameA">`
whose own script sets one global (`window.__mark`), read from the parent via
`subFrameA` (bare named access) and via `document.getElementById(...).
contentWindow` (both are the same object per spec, and confirmed to be here
too).

Same evidence channel as slices 55-57: serve the real, unmodified file
through `serve_wpt_like.py` and read the `add_completion_callback` marker it
injects plus any `[JS error]`/`script error:` line on stderr, for the real
id; a bare `http.server` + inline `console.log("PROBE ...")` markers for the
synthetic reduction (same technique as slices 30-34/52).

## Results (2026-09-04, dev-release, Linux, `main` = `4e745d386`)

One real TIMEOUT explained, new bug filed:

- `MediaRecorder-destroy-script-execution.html` — `[JS error] Uncaught
  TypeError: testWindow.prepareForTest is not a function` (and the same
  shape three more times, once per iframe: `subFrameStop.window.
  prepareForTest`/`subFrameAllTrackEnded.window.prepareForTest` — the fourth
  test, `audioBitrateMode`, reads a property rather than calling a function
  so it throws nothing and completes, subtest status FAIL not NOTRUN).
  `harness-complete status=1 tests=4 …:3|…:3|…:3|…:1` — fires at ≈10.5s
  wall-clock (polled every 100ms across a clean run), the "hangs until the
  harness's own internal timeout" shape (BUG-968's), not the sub-second
  false-TIMEOUT class (BUG-961/963). Root cause: `winFacade`
  (`frame_bridge.rs`) is a fixed ~11-name IDL whitelist standing in for
  `contentWindow`/named-window access — same-origin access to a global the
  framed page's own script defines (`prepareForTest`, a plain top-level
  `function` in `support/MediaRecorder-iframe.html`) is invisible through
  it, so `testWindow.prepareForTest` reads `undefined` and the call throws.
  None of the four `onload` assignments is `t.step`/`t.step_func`-wrapped,
  so the exception is an uncaught global error, not a caught test-step
  `FAIL` — none of the four `async_test`s ever reaches `.done()`. Filed as
  [BUG-979](../../bugs/BUG-979-OPEN.md), related to but distinct from
  [BUG-957](../../bugs/BUG-957-OPEN.md) (same `winFacade` literal, but
  BUG-957 is specifically the three missing `EventTarget` methods — a
  narrower fix than "proxy arbitrary globals across the isolate boundary").
  Classified via `_exact_id_marker` in `timeout_audit.py`
  (`frame-facade-missing-page-globals`). unclassified 29 → 25 this slice
  (three more ids closed as marker-only follow-ups against already-filed
  BUG-646/BUG-648, no new probe — see `timeout_audit.py`'s slice 58 comment
  block).

Synthetic confirmation (`synth-named-window` variant, not itself a WPT id):

```
onload-fired
typeof-subFrameA = object
subFrameA-is-element = false            (named access resolves through winFacade, not to the element)
subFrameA-tagName = undefined
ifr.contentWindow === subFrameA = true  (contentWindow and named-window access are the same object, correctly)
Object.keys(subFrameA) = window,self,frames,length,close,postMessage
subFrameA.contentWindow-mark THREW TypeError: Cannot read properties of undefined (reading '__mark')
```

Confirms the facade only ever answers its own hardcoded property list —
`__mark`, a global the framed page's script actually set, is unreachable
through it in exactly the shape the real test hits.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice58_gaps.py
        [--binary target/dev-release/lumen] [--seconds 15]

Exit code is 0 whatever the outcome -- this is a measurement, not a gate.
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
SERVE_SCRIPT = os.path.join(HERE, "serve_wpt_like.py")

REAL_ID = "/mediacapture-record/MediaRecorder-destroy-script-execution.html"

SYNTH_FRAME_HTML = """<!doctype html>
<meta charset="utf-8">
<script>window.__mark = "frameA-window";
function prepareForTest() {}</script>
<body>frameA</body>
"""

SYNTH_INDEX_HTML = """<!doctype html>
<meta charset="utf-8">
<body>
<iframe src="synth-frameA.html" name="subFrameA" id="ifrA"></iframe>
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
document.getElementById('ifrA').onload = function () {
  console.log("PROBE onload-fired");
  rep("typeof-subFrameA", function () { return typeof subFrameA; });
  rep("subFrameA-is-element", function () { return subFrameA === document.getElementById('ifrA'); });
  rep("subFrameA-tagName", function () { return subFrameA.tagName; });
  rep("ifr.contentWindow === subFrameA",
      function () { return document.getElementById('ifrA').contentWindow === subFrameA; });
  rep("Object.keys(subFrameA)", function () { return Object.keys(subFrameA).join(","); });
  rep("subFrameA.contentWindow-mark", function () { return subFrameA.contentWindow.__mark; });
  rep("subFrameA.window.__mark", function () { return subFrameA.window.__mark; });
};
</script>
</body>
"""

_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(
    r"((?:script|module) error: [^\n\r]+"
    r"|\[JS error\] [^\n\r]+"
    r"|\[unhandled-rejection\] [^\n\r]+)")


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _start_serve_wpt_like():
    proc = subprocess.Popen(
        [sys.executable, SERVE_SCRIPT],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    port_line = proc.stdout.readline().strip()
    return proc, int(port_line)


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass


def _serve_dir(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run(binary, url, log_path, seconds, poll=False):
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    t0 = time.monotonic()
    complete_at = None
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()), url],
            stdout=subprocess.DEVNULL, stderr=log, text=True)
        try:
            if poll:
                deadline = t0 + seconds
                while time.monotonic() < deadline:
                    time.sleep(0.1)
                    with open(log_path, encoding="utf-8", errors="replace") as f:
                        if "harness-complete" in f.read():
                            complete_at = time.monotonic() - t0
                            break
            else:
                time.sleep(seconds)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    with open(log_path, encoding="utf-8", errors="replace") as log:
        text = log.read()
    markers = []
    seen = set()
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker in seen:
            continue
        seen.add(marker)
        markers.append(marker)
    for err in dict.fromkeys(_ERROR_RE.findall(text)):
        markers.append(f"[engine] {err.strip()}")
    return markers, complete_at


def _run_real_id(binary, seconds):
    print(f"== {REAL_ID} (real file, serve_wpt_like.py) ==")
    server, port = _start_serve_wpt_like()
    try:
        log_path = os.path.join(REPO, ".tmp", "s58-mediarecorder-destroy.log")
        markers, complete_at = _run(
            binary, f"http://127.0.0.1:{port}{REAL_ID}", log_path, seconds, poll=True)
        for m in markers:
            print(f"  {m}")
        if complete_at is not None:
            print(f"  harness-complete at {complete_at:.2f}s wall-clock")
        else:
            print(f"  — no harness-complete within {seconds}s")
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
    print()


def _run_synth(binary, seconds):
    print("== synth-named-window (minimal reduction, not a WPT id) ==")
    tmp_dir = os.path.join(REPO, ".tmp", "s58-synth")
    os.makedirs(tmp_dir, exist_ok=True)
    with open(os.path.join(tmp_dir, "index.html"), "w", encoding="utf-8") as f:
        f.write(SYNTH_INDEX_HTML)
    with open(os.path.join(tmp_dir, "synth-frameA.html"), "w", encoding="utf-8") as f:
        f.write(SYNTH_FRAME_HTML)
    port, shutdown = _serve_dir(tmp_dir)
    try:
        log_path = os.path.join(REPO, ".tmp", "s58-synth-named-window.log")
        markers, _ = _run(binary, f"http://127.0.0.1:{port}/index.html", log_path, seconds)
        for m in markers:
            print(f"  {m}")
        if not markers:
            print("  — no PROBE/error marker seen at all")
    finally:
        shutdown()
    print()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=15.0)
    parser.add_argument("--only", choices=["real", "synth"], default=None)
    args = parser.parse_args()

    if args.only in (None, "real"):
        _run_real_id(args.binary, args.seconds)
    if args.only in (None, "synth"):
        _run_synth(args.binary, min(args.seconds, 6.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
