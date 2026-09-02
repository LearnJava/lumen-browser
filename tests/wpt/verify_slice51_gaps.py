#!/usr/bin/env python3
"""WPT-RUN-6 slice 51: the one id left by slice 50 that this slice classifies.

    /css/css-values/calc-rounds-to-integer.html

The file exercises 18 CSS properties (animation-timing-function, column-span,
counter-increment/reset/set, font-feature-settings, grid-row,
grid-template-rows, hyphenate-limit-chars/lines, initial-letter, max-lines,
order, orphans, text-combine-upright, transition-timing-function, widows,
z-index) purely synchronously: no fetch, no pipe substitution, no
cross-origin request. Its own `verifySupport(el, prop, valPattern)` gates
every `test()`/`testInt()`/`testIntInvalid()` call behind
`getComputedStyle(el)[prop] != nullVal` after `el.style.setProperty(prop,
testVal)` -- a feature-detection idiom, skipping properties the engine
doesn't compute.

Served through `serve_wpt_like.py` (no pipe/template substitution needed --
this file has none) with a live `--mcp-live-port` window. First bisection:
calling `window.runTests(prop, pattern)` again post-load for each of the 18
properties individually completes in ~0.04s each (RPC round-trip only, no
hang) -- rules out an infinite loop inside CSS value parsing/serialization
for any of the 18. Second probe: calling the file's own
`window.verifySupport(el, prop, valPattern)` directly for all 18 shows every
one returns `false`, because `getComputedStyle(document.body)[prop]` reads
back `""` both before and after `style.setProperty` for every property
except `z-index` (where it reads back a fixed `"10"` regardless of what was
set -- a separate, narrower symptom of the same root cause, not
investigated further here).

Root cause: `getComputedStyle()` (`selector_query.rs::computed_style_to_map`)
is a hand-written whitelist of ~64 properties -- already open as
[BUG-472](../../bugs/BUG-472-OPEN.md), filed 2026-08-02, "ДОРАБОТКА ->
CSSOM-3" -- and none of the 18 properties this file exercises are in it.
`runTests()` therefore returns via `if (!verifySupport(...)) return;` before
ever calling `test()`, for all 18, so zero subtests register. With nothing
registered and no `explicit_done`, `testharness.js` still waits out its own
internal ~10s file-level harness timeout (the mechanism the
`resources/testharnessreport.js` comment already documents for the
css-anchor-position cluster, slice 40) before completing with `TIMEOUT` and
an empty `subtests` array -- matching the WPT-RUN-5 snapshot signature
exactly.

Classified against BUG-472 (already filed, already scoped as a whitelist
gap) -- no new bug filed. `computed-style-whitelist-empty` marker added to
`SOURCE_MARKERS`, keyed by exact id (the mechanism is proven by "the
property isn't in the whitelist", which degenerates to the id itself, the
same shape `_exact_id_marker` already documents for slice 31/49's entries).

unclassified 39 -> 38.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice51_gaps.py
        [--binary target/dev-release/lumen] [--seconds 10]

Exit code is 0 whatever the outcome -- this is a measurement, not a gate.
"""

import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

sys.path.insert(0, os.path.join(REPO, "scripts"))
from bench_scroll import mcp_rpc_factory  # noqa: E402

PAGE = "css/css-values/calc-rounds-to-integer.html"

PROPS = [
    ["animation-timing-function", "steps(xxx)"],
    ["column-span", None],
    ["counter-increment", "foo xxx"],
    ["counter-reset", "foo xxx"],
    ["counter-set", "foo xxx"],
    ["font-feature-settings", "\"fooo\" xxx"],
    ["grid-row", None],
    ["grid-template-rows", "repeat(xxx, 10px)"],
    ["hyphenate-limit-chars", None],
    ["hyphenate-limit-lines", None],
    ["initial-letter", "1.1 xxx"],
    ["max-lines", None],
    ["order", None],
    ["orphans", None],
    ["text-combine-upright", "digits xxx"],
    ["transition-timing-function", "steps(xxx)"],
    ["widows", None],
    ["z-index", None],
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/dev-release/lumen")
    parser.add_argument("--http-port", type=int, default=8905)
    parser.add_argument("--mcp-port", type=int, default=7969)
    parser.add_argument("--seconds", type=float, default=10.0,
                         help="how long to wait for MCP to come up")
    args = parser.parse_args()

    os.chdir(REPO)
    httpd = subprocess.Popen(
        [sys.executable, "tests/wpt/serve_wpt_like.py", "--port", str(args.http_port)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    errpath = "/tmp/lumen-verify-slice51.log"
    errfh = open(errpath, "w")
    url = f"http://127.0.0.1:{args.http_port}/{PAGE}"
    proc = subprocess.Popen([args.binary, "--mcp-live-port", str(args.mcp_port), url],
                             stdout=subprocess.DEVNULL, stderr=errfh, text=True)
    try:
        dl = time.monotonic() + args.seconds
        rpc = None
        while time.monotonic() < dl:
            try:
                rpc = mcp_rpc_factory(args.mcp_port, errpath)
                break
            except OSError:
                time.sleep(0.5)
        if rpc is None:
            print("MCP never came up")
            return 0
        time.sleep(1)
        code = f"""
JSON.stringify((function() {{
  var props = {json.dumps(PROPS)};
  var out = [];
  for (var i = 0; i < props.length; i++) {{
    var prop = props[i][0], pattern = props[i][1];
    document.body.removeAttribute('style');
    var nullVal = getComputedStyle(document.body)[prop];
    var supported = window.verifySupport(document.body, prop, pattern === null ? undefined : pattern);
    var setVal = getComputedStyle(document.body)[prop];
    out.push([prop, supported, nullVal, setVal]);
  }}
  return out;
}})())
"""
        r = rpc("tools/call", {"name": "eval", "arguments": {"code": code}})
        print("verifySupport per property:", r)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        errfh.close()
        httpd.terminate()
        httpd.wait(timeout=5)
    return 0


if __name__ == "__main__":
    sys.exit(main())
