#!/usr/bin/env python3
"""WPT-RUN-6 slice 52: four of the 38 `unclassified` ids left by slice 51.

Reading the residual by hand surfaced four candidates with a concrete,
grep-backed hypothesis each:

    /css/css-view-transitions/pseudo-computed-style-stays-in-sync-with-new-element.html
    /css/css-view-transitions/elements-at-point.html
    /resize-observer/scrollbars-2.html
    /selection/selection-nested-video.html

**pseudo-computed-style-stays-in-sync**: the test body is
`new Promise(async (resolve, reject) => { ... assert_in_array(viewbox,
["none", undefined]) ... transition.finished.then(resolve, reject); })`.
`window.getComputedStyle(document.documentElement, "::view-transition-new(target)")`
resolves against `document.documentElement` itself — `getComputedStyle(el,
pseudoElt)` ignores its second argument entirely (already-open BUG-490,
`web_api_shim_tail_b.js`: "Pseudo-elements are not yet supported (ignored)").
`object-view-box` is not itself a supported property either, so
`_lumen_get_computed_style` answers `''`  — not `"none"` and not `undefined`
— and `assert_in_array` throws. The throw happens inside an **async**
executor passed to `new Promise`: calling an async function never lets an
exception it raises escape synchronously to its caller (the function itself
converts the throw into a rejected, immediately-discarded return value), so
the Promise constructor's own try/catch — which only ever sees a normal
return — never calls `reject`. `resolve`/`reject` are consequently never
invoked by anything and the outer promise hangs forever. This is a
transferable JS idiom trap (`new Promise(async (resolve, reject) => {...})`
swallows every exception thrown before the first path that explicitly calls
`resolve`/`reject`), not a new engine defect — BUG-490 already owns the root
cause. If confirmed, this id gets a `SOURCE_MARKERS`/`_exact_id_marker` entry
pointing at BUG-490, no new bug.

**elements-at-point**: the executor passed to `new Promise` here is a plain
(non-async) arrow that calls `document.startViewTransition(() =>
resolve(document.elementsFromPoint(x, y)))`. `elementsFromPoint` is fully
implemented (`crates/js/src/dom/tests/v8_point_hit_test.rs`), so reading finds
no obvious gap; this variant exists to measure whether `startViewTransition`'s
synchronous-callback contract (`view_transitions.rs`) actually reaches
`resolve()` on the page's own reduced shape, since the two `startViewTransition`
tests share the same file's residual bucket and could plausibly share one
mechanism.

**scrollbars-2**: `new ResizeObserver(entries => resolve(entries[0]
.contentBoxSize[0])).observe(scrollContainer)` — hangs forever if the
observer never delivers its first callback, or delivers one whose
`contentBoxSize` is not shaped as WPT expects (`BUG-809`'s
"PerformanceObserver advertises a type and never delivers" shape, read here
against `ResizeObserver` instead).

**selection-nested-video**: `document.addEventListener("DOMContentLoaded",
() => { const c = a.attachShadow(...); ... sel.setBaseAndExtent(b, 0, c, 0);
... t.done(); })` — hangs if `DOMContentLoaded` never reaches a listener
added on `document` (as opposed to the well-known `window.onload`), or if
`attachShadow`/`setBaseAndExtent` throws before `t.done()`.

Same harness as slices 30-34: one browser process per page, served over
http, evidence read off the browser's own stderr via `console.log("PROBE
...")` markers wrapped in try/catch (`rep()`), since a bare thrown exception
inside an event listener or promise executor prints nothing on its own
(`docs/probe-method.md` §1).

## Results (2026-09-02, dev-release, Linux, `main` = `c93cf5455`)

- `vt-pseudo-computed-style` — confirms the hypothesis. All three transition
  promises resolve normally (`updateCallbackDone-resolved`, `ready-resolved`,
  `finished-resolved`); `viewbox-first`/`viewbox-second` both read back `''`
  (not `"none"`, not `undefined`), so `viewbox-in-array = false` — the exact
  assertion the real test throws on, inside the async-executor idiom that
  swallows it. Attributed to BUG-490 via `computed-style-pseudo-ignored` in
  `timeout_audit.py`; unclassified 38 → 37. The idiom trap itself is now
  documented in `docs/probe-method.md` §1.
- `vt-elements-at-point` — hypothesis refuted. `vt-callback-ran` and
  `resolved tagNames=LI,UL,DIV,BODY,HTML,#document` print — the callback runs
  synchronously, `resolve()` is reached, `elementsFromPoint` returns real
  elements and no pseudo-elements (there are none to return), which is
  exactly what the real assertion expects. No engine defect found in this
  reduced shape; the real TIMEOUT source for this id is unexplained. Stays
  unclassified.
- `resize-observer-scrollbars` — hypothesis refuted. `ro-callback` fires once
  with `contentBoxSize-0 = [{"inlineSize":100,"blockSize":100}]` — the
  observer delivers promptly and with the expected shape. Stays unclassified;
  a `promise_test`'s own `assert_equals` (if it disagrees with the delivered
  size) is caught by the harness and would FAIL, not hang.
- `selection-nested-video` — hypothesis refuted. `dcl-fired` prints,
  `attachShadow`/`setBaseAndExtent` both succeed, `anchorNode-is-b = true` —
  the exact assertion the real test checks. No engine defect found in this
  reduced shape; stays unclassified.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice52_gaps.py
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
<title>slice-52 probe: __NAME__</title>
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
    # pseudo-computed-style-stays-in-sync-with-new-element.html's exact
    # shape: async executor, assert_in_array on objectViewBox.
    "vt-pseudo-computed-style": ("""
<style>
div { width: 100px; height: 100px; background: blue; view-transition-name: target; contain: paint; }
</style>
<div id=first></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  rep("has-startViewTransition", function () { return typeof document.startViewTransition; });
  var transition = document.startViewTransition();
  transition.updateCallbackDone.then(function () {
    console.log("PROBE updateCallbackDone-resolved");
  }, function (e) { console.log("PROBE updateCallbackDone-REJECTED " + e); });
  transition.ready.then(function () {
    console.log("PROBE ready-resolved");
    var cs = window.getComputedStyle(document.documentElement, "::view-transition-new(target)");
    rep("viewbox-first", function () { return cs.objectViewBox; });
    first.style.filter = "blur(5px)";
    var cs2 = window.getComputedStyle(document.documentElement, "::view-transition-new(target)");
    rep("viewbox-second", function () { return cs2.objectViewBox; });
    rep("viewbox-in-array", function () {
      var vb = cs2.objectViewBox;
      return vb === "none" || vb === undefined;
    });
  }, function (e) { console.log("PROBE ready-REJECTED " + e); });
  transition.finished.then(function () {
    console.log("PROBE finished-resolved");
  }, function (e) { console.log("PROBE finished-REJECTED " + e); });
});
</script>
""", "resolved/REJECTED for all three transition promises; viewbox is '' (BUG-490), not none/undefined"),

    # elements-at-point.html's exact shape, reduced: plain (non-async)
    # executor, startViewTransition callback resolves via elementsFromPoint.
    "vt-elements-at-point": ("""
<div>
  <ul style="view-transition-name: list1">
    <li id="target">One</li>
    <li>Two</li>
  </ul>
</div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var rect = target.getBoundingClientRect();
  var point = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  rep("rect", function () { return JSON.stringify(rect); });
  new Promise(function (resolve) {
    var transition = document.startViewTransition(function () {
      console.log("PROBE vt-callback-ran");
      resolve(document.elementsFromPoint(point.x, point.y));
    });
    console.log("PROBE vt-called-returned");
  }).then(function (list) {
    console.log("PROBE resolved tagNames=" + list.map(function (e) { return e.tagName; }).join(","));
  }, function (e) {
    console.log("PROBE REJECTED " + e);
  });
});
</script>
""", "vt-callback-ran then resolved with a tagName list"),

    # scrollbars-2.html's exact shape: ResizeObserver delivering
    # contentBoxSize on a scrolling container.
    "resize-observer-scrollbars": ("""
<style>
  #scrollContainer { width: 100px; height: 100px; padding: 30px; border: 10px solid blue; overflow: scroll; background: #818182; }
</style>
<div id="scrollContainer"></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var el = document.getElementById('scrollContainer');
  rep("offsetWidth", function () { return el.offsetWidth; });
  rep("clientWidth", function () { return el.clientWidth; });
  rep("has-ResizeObserver", function () { return typeof ResizeObserver; });
  var observer = new ResizeObserver(function (entries) {
    console.log("PROBE ro-callback entries.length=" + entries.length);
    rep("contentBoxSize-0", function () { return JSON.stringify(entries[0].contentBoxSize); });
  });
  observer.observe(el);
});
</script>
""", "ro-callback fires once with a contentBoxSize array entry"),

    # selection-nested-video.html's exact shape: DOMContentLoaded listener
    # added on `document`, attachShadow + setBaseAndExtent across shadow
    # tree scopes.
    "selection-nested-video": ("""
<div id="a">A</div>
<video>
  <video id="b"></video>
</video>
""" + REPORT + """
<script>
console.log("PROBE readyState-at-script=" + document.readyState);
document.addEventListener("DOMContentLoaded", function () {
  console.log("PROBE dcl-fired");
  rep("attachShadow", function () { return a.attachShadow({mode: "open"}).constructor.name; });
  var c = a.shadowRoot;
  var sel = window.getSelection();
  rep("setBaseAndExtent", function () { sel.setBaseAndExtent(b, 0, c, 0); return "ok"; });
  rep("anchorNode-is-b", function () { return sel.anchorNode === b; });
});
window.addEventListener("load", function () { console.log("PROBE load-fired"); });
</script>
""", "dcl-fired, then setBaseAndExtent = ok"),
}


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


def _run_variant(binary, name, http_port, seconds):
    log_path = os.path.join(REPO, ".tmp", f"s52-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".s52-{name}.html"
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
        path = os.path.join(HERE, f".s52-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)

    try:
        print(f"{'variant':30s} {'ticks':>5s}  {'expected':70s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:30s} {ticks:5d}  {expected:70s} {seen}")
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
