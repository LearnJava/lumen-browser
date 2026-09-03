#!/usr/bin/env python3
"""WPT-RUN-6 slice 53: the real, unmodified files behind slice 52's three
refuted reduced-form hypotheses.

Slice 52 built a hand-reduced reproduction of each candidate's *own* body and
found no engine defect in the reduced shape for three of the four ids:

    /css/css-view-transitions/elements-at-point.html
    /resize-observer/scrollbars-2.html
    /selection/selection-nested-video.html

A reduced page proves the mechanism it reproduces is not the cause; it says
nothing about a mechanism the reduction dropped. All three reductions in
slice 52 dropped `testharness.js`/`testharnessreport.js` entirely (a bare
`console.log`-only page) and, for the view-transition and selection cases,
replaced `promise_test`/`async_test` with a raw `window.addEventListener`
listener. This slice removes that gap: it serves the real files byte-for-byte
through `serve_wpt_like.py` (the same substituted `testharnessreport.js` a
real corpus run uses) and reads the same `add_completion_callback` marker
`serve_wpt_like.py` injects, so what runs is the actual WPT harness plumbing
around the actual assertions -- the one thing slice 52 could not observe.

Same evidence channel as slice 52: `console.log` reaches the browser
process's own stderr, read back after a fixed wall-clock window.

## Results (2026-09-03, dev-release, Linux, `main` = `87c97af64`)

- `elements-at-point.html` — completes cleanly in well under the window:
  `harness-complete status=0 tests=1 elementsFromPoint resolves pseudos to
  owning element:0` (subtest status 0 = PASS). No engine defect; the id no
  longer reproduces as a TIMEOUT on current `main` at all. Stays
  unclassified without a marker — nothing in `timeout_audit.py` explains a
  TIMEOUT that a fresh run doesn't show (same shape as the slice 37 MathML
  precedent).
- `scrollbars-2.html` — also completes cleanly: `harness-complete status=0
  tests=1 ResizeObserver content-box size and scrollbars:1` (subtest status
  1 = FAIL, not TIMEOUT — the delivered `contentBoxSize` doesn't match the
  test's scrollbar-size arithmetic, a real assertion mismatch, but the
  harness reaches it and reports promptly). Stays unclassified without a
  marker for the same reason as above.
- `selection-nested-video.html` — the one real defect. `harness-complete
  status=1` (ERROR) with the sole subtest stuck at status `2` (TIMEOUT) and
  a genuine `[JS error] Uncaught Error` on stderr: an `AssertionError`
  thrown from `assert_equals(sel.focusNode, b)` inside the test's
  `DOMContentLoaded` listener, which is a bare arrow (not wrapped in
  `t.step()`), so testharness.js can't route the failure to a normal FAIL.
  A follow-up diagnostic (`.tmp/s53-diag.html`, not committed — ad hoc)
  isolated the root cause: `sel.anchorNode === b` reads correctly but
  `sel.focusNode` stays the raw shadow root instead of collapsing into the
  anchor's tree (Selection API §4.3's cross-tree-scope adjustment is simply
  absent from `setBaseAndExtent`/`_lumen_set_selection`). New bug
  [BUG-966](../../bugs/BUG-966-OPEN.md), classified via `_exact_id_marker`
  in `timeout_audit.py` (`selection-cross-tree-scope-focus`).

Net: this slice's own residual reclassifies to 1 of 3 (BUG-966), the other
2 no longer reproduce at all. unclassified 37 → 36.

Method note (also added to `docs/probe-method.md` §1): slice 52's reduced
reproductions of these three ids dropped `testharness.js` and the
`promise_test`/`async_test` wrapping entirely, so they could only measure
whether the *page-level* mechanism worked (it does, for all three) — they
could not see how the real harness *reports* a failure inside an unwrapped
listener. Re-running the real, unmodified file through `serve_wpt_like.py`
is what actually surfaced the mechanism for the third id.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice53_gaps.py
        [--binary target/dev-release/lumen] [--seconds 15]

Exit code is 0 whatever the outcome -- this is a measurement, not a gate.
"""

import argparse
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
SERVE_SCRIPT = os.path.join(HERE, "serve_wpt_like.py")

IDS = [
    "/css/css-view-transitions/elements-at-point.html",
    "/resize-observer/scrollbars-2.html",
    "/selection/selection-nested-video.html",
]

_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
# `[JS]`/`[JS error]`/`script error:`/`[unhandled-rejection]` are the four
# stderr sinks console.log/console.error/top-level-eval/promise-rejection
# actually go through (crates/js/src/v8_runtime/{install/platform,eval,
# promise_reject}.rs) -- same shape `timeout_audit.py`'s `_ERROR_LINE_RE` uses.
_ERROR_RE = re.compile(
    r"((?:script|module) error: [^\n\r]+"
    r"|\[JS error\] [^\n\r]+"
    r"|\[unhandled-rejection\] [^\n\r]+)")


def _start_server():
    proc = subprocess.Popen(
        [sys.executable, SERVE_SCRIPT],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    port_line = proc.stdout.readline().strip()
    return proc, int(port_line)


def _free_port():
    import socket
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _run_one(binary, http_port, test_id, seconds):
    log_path = os.path.join(REPO, ".tmp", f"s53-{test_id.strip('/').replace('/', '_')}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    url = f"http://127.0.0.1:{http_port}{test_id}"
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()), url],
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
    return markers, text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=15.0)
    parser.add_argument("--id", action="append", default=None,
                         help="restrict to one or more of the three ids (repeatable)")
    parser.add_argument("--dump", action="store_true", help="print the full captured stderr too")
    args = parser.parse_args()

    wanted = args.id or IDS
    server, http_port = _start_server()
    try:
        for test_id in wanted:
            print(f"== {test_id} ==")
            markers, text = _run_one(args.binary, http_port, test_id, args.seconds)
            if markers:
                for m in markers:
                    print(f"  {m}")
            else:
                print("  — no PROBE/error marker seen at all")
            if args.dump:
                print("  --- full stderr ---")
                for line in text.splitlines():
                    print(f"  | {line}")
            print()
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
