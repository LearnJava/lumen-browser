#!/usr/bin/env python3
"""WPT-RUN-6 slice 54: three fresh unclassified-list candidates, none touched
by an earlier slice, chosen because `timeout_audit.py`'s `SUBTEST_MARKERS`
stage already names the exact hung subtest for two of them (the strongest
evidence the audit produces short of a live probe):

    /css/css-cascade/scope-implicit-external.html
        (both subtests: "@scope with external stylesheet through link
        element", "... through @import")
    /css/css-contain/content-visibility/content-visibility-069.html
        ("Content Visibility: pending visibility changes")
    /css/cssom-view/background-change-during-smooth-scroll.html
        ("background change during smooth scroll")

Same evidence channel as slices 52/53: serve the real, unmodified files
through `serve_wpt_like.py` (the same substituted `testharnessreport.js` a
real corpus run uses) and read the `add_completion_callback` marker it
injects, plus any `[JS error]`/`script error:` line on stderr.

## Results (2026-09-03, dev-release, Linux, `main` = `64b92633c`)

- `content-visibility-069.html` — completes cleanly:
  `harness-complete status=0 tests=1 Content Visibility: pending visibility
  changes:0` (PASS). No engine defect; stays unclassified without a marker.
- `background-change-during-smooth-scroll.html` — completes cleanly:
  `harness-complete status=0 tests=1 background change during smooth
  scroll:0` (PASS). No engine defect; stays unclassified without a marker.
- `scope-implicit-external.html` — `harness-complete status=2 tests=2
  @scope with external stylesheet through link element:1|@scope with
  external stylesheet through @import:2`. The `@import` subtest is a real
  hang: a `<style>` obtained from `<template>.content.cloneNode(true)` and
  appended into the live tree never fetches its `@import` and never fires
  `load`/`error` — the awaited promise never settles. Root cause and
  ad hoc `cloneNode` diagnostics in
  [BUG-967](../../bugs/BUG-967-OPEN.md); classified via `_exact_id_marker`
  in `timeout_audit.py` (`style-clone-import-not-fetched`). The `link
  element` subtest doesn't hang (its `<link>` fires `load` normally) but
  FAILs a separate assertion — an `@scope` implicit-root leak, noted in
  BUG-967 but not itself a TIMEOUT so not classified here.

Net: 1 of 3 candidates reclassified (BUG-967), 2 no longer reproduce as a
hang at all. unclassified 36 → 35.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice54_gaps.py
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
    "/css/css-cascade/scope-implicit-external.html",
    "/css/css-contain/content-visibility/content-visibility-069.html",
    "/css/cssom-view/background-change-during-smooth-scroll.html",
]

_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
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
    log_path = os.path.join(REPO, ".tmp", f"s54-{test_id.strip('/').replace('/', '_')}.log")
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
