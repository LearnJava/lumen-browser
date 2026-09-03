#!/usr/bin/env python3
"""WPT-RUN-6 slice 56: six unclassified-list candidates read by hand out of
the 32-id residual (`.tmp/audit-s55-after.json`'s `residual_ids`):

    /resize-observer/scrollbars-2.html
    /svg/linking/scripted/a.ping-functionality.html
    /workers/interfaces/WorkerGlobalScope/location/redirect-sharedworker.html
    /css/css-grid/grid-definition/
        grid-change-intrinsic-size-with-auto-repeat-tracks-001.html
    /css/css-viewport/zoom/computed-initial.html
    /html/browsers/browsing-the-web/overlapping-navigations-and-traversals/
        anchor-fragment-history-back-on-click.html
    /scroll-to-text-fragment/find-range-from-text-directive-no-reveal.html

(seven ids listed, `IDS` below carries all seven — the first four turned out
to duplicate earlier work, discovered only *after* running the probe; see
"Duplicates" below).

Same evidence channel as slices 52-55: serve the real, unmodified files
through `serve_wpt_like.py` (the same substituted `testharnessreport.js` a
real corpus run uses) and read the `add_completion_callback` marker it
injects, plus any `[JS error]`/`script error:` line on stderr.

## Results (2026-09-03, dev-release, Linux, `main` = `8a750386e`)

Two real hangs, both reclassified:

- `anchor-fragment-history-back-on-click.html` — `harness-complete
  status=2 tests=1 Anchor with a fragment href and a click handler that
  navigates back:2` (TIMEOUT). Root-caused with two scratch pages outside
  `testharness.js` (`.tmp/probe56_popstate_sources.html`,
  `.tmp/probe56_anchor_fragment.html`, not committed): a forward
  same-document fragment navigation (`location.hash =`, or the default
  activation of `<a href="#x">`) pushes a real history entry
  (`history.length` measured going 1 → 2 → 3) and fires `hashchange`, but
  **never** `popstate` — only `history.back()`/`forward()`/`go()` does (two
  separate code paths have the same gap: `_lumen_navigate_or_fragment` and
  the dedicated `location.hash` setter `_lumen_set_location_hash`). The test
  awaits two `popstate`s (from the anchor's own click-navigation and from
  `history.back()`); only the second ever arrived. New
  [BUG-971](../../bugs/BUG-971-FIXED.md) — **fixed in this slice**: both
  paths now call a shared `_lumen_dispatch_popstate` (factored out of
  `_lumen_deliver_popstate`) before firing `hashchange`. Re-measured after
  the fix: the id no longer hangs (`harness-complete status=0`), but its one
  subtest now FAILs — `history.back()` lands on the wrong entry
  (`navigations=["#3","#2"]` instead of `["#3","#1"]`), a second, distinct
  defect filed separately as [BUG-973](../../bugs/BUG-973-OPEN.md) (not
  fixed here). Classified via `_exact_id_marker` in `timeout_audit.py`
  (`fragment-nav-no-popstate`) — the TIMEOUT this id showed in the WPT-RUN-5
  corpus is explained by BUG-971 regardless of BUG-973 remaining open.
- `find-range-from-text-directive-no-reveal.html` — `harness-complete
  status=2 tests=1 Text fragment with suffix should only reveal the
  matching details element:2` (TIMEOUT). Scroll To Text Fragment
  (`:~:text=`) does not exist anywhere in the workspace (`grep` for
  `until-found`/`beforematch`/`fragment_directive`/`text_fragment` outside
  `tests/wpt/` is empty; `onbeforematch` is only a reflected generic
  event-handler attribute, nothing dispatches it) — the awaited `toggle` on
  the matching `<details>` never fires. Filed as a ДОРАБОТКА, not a plain
  bug (`docs/probe-method.md` §8: absent wholesale, family-sized —
  URL-directive parsing + DOM text search + a new hidden-state model + a new
  event, not a point fix): [BUG-972](../../bugs/BUG-972-OPEN.md) →
  [STTF-1](../../ROADMAP.md); classified via `_exact_id_marker` in
  `timeout_audit.py` (`scroll-to-text-fragment-missing`).

One real, harness-completing FAIL with no hang (no bug filed — WPT-RUN-6 is
about TIMEOUTs, and this one FAILs promptly):

- `computed-initial.html` — `harness-complete status=0 tests=126 ...`,
  123/126 subtests PASS, the three `font-size` variants FAIL (a CSS `zoom`
  scaling leak into `getComputedStyle()`'s initial-value reporting). No
  engine defect worth filing here; stays unclassified without a marker.

Three duplicates of already-filed findings, discovered only by grepping
`bugs/*.md` for the id *after* the probe ran — a step this slice's write-up
does for future ones so the same id doesn't get re-picked a third time:

- `a.ping-functionality.html` — reproduces exactly
  [BUG-963](../../bugs/BUG-963-OPEN.md) (WPT-RUN-6 slice 50): `harness-complete
  status=0 tests=3 ...:1|...:1|...:1`, all three subtests FAIL, no hang.
  Hyperlink auditing (`ping`) is confirmed still entirely unimplemented, but
  BUG-963 already covers it and already explains why it doesn't explain a
  TIMEOUT (the harness completes in ~9s). Not re-filed.
- `scrollbars-2.html` — reproduces exactly slice 53's finding
  (`tests/wpt/verify_slice53_gaps.py`): `harness-complete status=0 tests=1
  ResizeObserver content-box size and scrollbars:1` (subtest FAIL, no
  hang). Already documented; not re-filed.
- `redirect-sharedworker.html` — **inconclusive, a probe-tool gap, not an
  engine measurement.** `harness-complete status=2 tests=0` with
  `[engine] script error: Runtime("Unexpected identifier 'main'")`, which
  looks like a hang at first, but `serve_wpt_like.py` is a bare
  `SimpleHTTPRequestHandler` — it does not execute `common/redirect.py` as
  a wptserve CGI handler, it serves the `.py` source as literal bytes. The
  `SharedWorker` then tries to run `def main(request, response): ...` as
  JavaScript and hits a syntax error at the `main` token — an artifact of
  this probe method having no CGI support, not a measurement of the real
  redirect mechanism. [BUG-866](../../bugs/BUG-866-OPEN.md) already lists
  this exact id among seven SharedWorker-identity residuals with a
  different, independently-measured mechanism
  (`verify_worker_port_storage_gaps.py`); this slice does not add a
  classification for it. New gotcha recorded in `docs/probe-method.md` so a
  future slice doesn't spend time on a `.py`-handler-dependent id through
  this script again.

`grid-change-intrinsic-size-with-auto-repeat-tracks-001.html` was picked
before checking `bugs/*.md`/prior `verify_slice*.py` and turned out fresh
after all: `harness-complete status=0 tests=8 .grid 1:1|...|.grid 8:1`, all
eight subtests FAIL, no hang. A real CSS Grid gap (mutating a grid item's
intrinsic size doesn't reflow `repeat(auto-fill, ...)`'s resolved track
count), but out of this slice's TIMEOUT-only scope; not filed here.

Net: 2 of 7 candidates reclassified (BUG-971, BUG-972). unclassified
32 → 30 (re-verified against the cached `.tmp/wpt-corpus` WPT-RUN-5 snapshot
with `timeout_audit.py --out-dir .tmp/wpt-corpus`). BUG-971 is fixed in this
slice's commit; re-measuring it after the fix surfaced a second, distinct
defect (`history.back()` resolves its target against the nav stack at drain
time, not at call time) filed separately as
[BUG-973](../../bugs/BUG-973-OPEN.md), left open.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice56_gaps.py
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
    "/resize-observer/scrollbars-2.html",
    "/svg/linking/scripted/a.ping-functionality.html",
    "/workers/interfaces/WorkerGlobalScope/location/redirect-sharedworker.html",
    "/css/css-grid/grid-definition/"
    "grid-change-intrinsic-size-with-auto-repeat-tracks-001.html",
    "/css/css-viewport/zoom/computed-initial.html",
    "/html/browsers/browsing-the-web/overlapping-navigations-and-traversals/"
    "anchor-fragment-history-back-on-click.html",
    "/scroll-to-text-fragment/find-range-from-text-directive-no-reveal.html",
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
    log_path = os.path.join(REPO, ".tmp", f"s56-{test_id.strip('/').replace('/', '_')}.log")
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
