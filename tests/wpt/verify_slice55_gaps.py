#!/usr/bin/env python3
"""WPT-RUN-6 slice 55: four fresh unclassified-list candidates, none touched
by an earlier slice, read by hand out of the 35-id residual
(`.tmp/audit-s55.json`'s `residual_ids`):

    /svg/coordinate-systems/outer-svg-intrinsic-size-002.html
    /html/semantics/scripting-1/the-script-element/change-src-attr-prepare-a-script.html
    /html/rendering/non-replaced-elements/the-page/iframe-marginwidth-marginheight.html
    /html/semantics/embedded-content/the-img-element/sizes/implicit-sizes-ignores-width.html

Same evidence channel as slices 52-54: serve the real, unmodified files
through `serve_wpt_like.py` (the same substituted `testharnessreport.js` a
real corpus run uses) and read the `add_completion_callback` marker it
injects, plus any `[JS error]`/`script error:` line on stderr.

## Results (2026-09-03, dev-release, Linux, `main` = `d05d62fd2`)

- `outer-svg-intrinsic-size-002.html` — hangs for real:
  `harness-complete status=2 tests=0` (harness's own internal timeout, zero
  tests ever registered). The page's single `<object data="support/simple.svg"
  onload="go()">` never fires `onload` — every `test()` call lives inside
  `go()`, so the harness never registers a single test, let alone completes
  one. This is a live re-measurement of
  [BUG-798](../../bugs/BUG-798-OPEN.md) (`<object data>`/`<embed src>` never
  fetch, never dispatch `load`/`error`), NOT a new defect — but the existing
  `embed-object-no-load` mechanism in `timeout_audit.py` failed to classify
  this specific id, because its second half of the `mode="all"` pair only
  matched the *scripted* forms (`foo.onload = ...`/
  `addEventListener('load', ...)`), not the plain HTML attribute form
  `onload="go()"` this test actually uses — a gap in the marker's regex, not
  in the classification itself. Fixed by widening the second pattern to also
  match a bare `onload=`/`onerror=` (covers the attribute spelling too);
  reclassified via the existing `BUG-798` mechanism, no new bug filed.
- `change-src-attr-prepare-a-script.html` — hangs for real:
  `harness-complete status=2 tests=1 ...:2` (one registered `promise_test`,
  stuck at subtest status TIMEOUT). The test creates a `<script
  type="invalid" src="resources/flag-setter.js">`, inserts it (no-op because
  `type="invalid"`), sets `.type = ''`, then mutates `.src` to a second URL
  and awaits `.onload`. `serve_wpt_like.py`'s own access log (`--dump`) shows
  **no second GET at all** — only the request for
  `change-src-attr-prepare-a-script.html` itself and the two harness
  scripts; `resources/flag-setter.js` (either spelling) is never requested.
  Mutating `.src` on an already-connected, non-parser-inserted `<script>`
  does not re-run the "prepare a script" algorithm — the element's fetch
  path only fires once, at initial insertion. New bug
  [BUG-968](../../bugs/BUG-968-OPEN.md); classified via `_exact_id_marker`
  in `timeout_audit.py` (`script-src-mutation-not-prepared`).
- `iframe-marginwidth-marginheight.html` — completes cleanly, but not because
  the mechanism it targets works: `harness-complete status=0 tests=1 ...:1`
  (subtest FAIL, not TIMEOUT) alongside `[JS error] Uncaught TypeError:
  Cannot read properties of undefined (reading 'length')`. The error is
  specifically about `.length`, not `.document`/`.body` — `window[0]`
  (indexed frame access, BUG-480 slice 3) and the cross-frame document/body
  facades all resolve; only `.attributes` on the returned body facade is
  missing entirely (`frame_bridge.rs::frameElem` never defines it). Not a
  hang — `testharness.js`'s `single_test` mode routes the uncaught exception
  to a prompt FAIL via `window.onerror` — so this id is **not** reclassified
  here (stays unclassified, same as slice 53's precedent for a real defect
  found while probing a TIMEOUT it doesn't explain), but the gap is real.
  New bug [BUG-970](../../bugs/BUG-970-OPEN.md), filed on its own merits.
- `implicit-sizes-ignores-width.html` — hangs for real, despite completing
  in a couple of seconds: `harness-complete status=2 tests=1 ...:1` — the
  *subtest* is already FAIL (`img.width` doesn't read back the expected
  `400`, because the `sizes`+`srcset` `Nw`-descriptor density correction is
  never computed anywhere in `picture.rs::pick_from_srcset` — it discards
  `source_size_px` and always returns `intrinsic_width: None`), but the test
  uses `setup({explicit_done: true})` and never reaches its own `done()`
  call because the assertion throws first — so the **overall harness**
  status is `2` (its own internal timeout), matching the corpus's TIMEOUT
  classification for this id even though the individual subtest's outcome
  was already decided. New bug [BUG-969](../../bugs/BUG-969-OPEN.md);
  classified via `_exact_id_marker` in `timeout_audit.py`
  (`srcset-density-correction-missing`).

Net: 3 of 4 reclassified (one folded into the existing BUG-798 mechanism via
a marker-regex fix, two new bugs BUG-968/BUG-969), the fourth stays
unclassified but earned its own bug (BUG-970) for a real defect that doesn't
itself explain a TIMEOUT. unclassified 35 -> 32.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice55_gaps.py
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
    "/svg/coordinate-systems/outer-svg-intrinsic-size-002.html",
    "/html/semantics/scripting-1/the-script-element/change-src-attr-prepare-a-script.html",
    "/html/rendering/non-replaced-elements/the-page/iframe-marginwidth-marginheight.html",
    "/html/semantics/embedded-content/the-img-element/sizes/implicit-sizes-ignores-width.html",
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
    log_path = os.path.join(REPO, ".tmp", f"s55-{test_id.strip('/').replace('/', '_')}.log")
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
                         help="restrict to one or more of the four ids (repeatable)")
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
