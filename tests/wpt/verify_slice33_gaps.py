#!/usr/bin/env python3
"""WPT-RUN-6 slice 33: two of the 50 `unclassified` ids left by slice 32.

Reading the residual by hand (no dominant directory left, same as slice 31/32)
surfaced two candidates with a concrete, grep-backed hypothesis each:

    /html/semantics/forms/the-select-element/select-add.html
    /html/semantics/embedded-content/media-elements/location-of-the-media-resource/currentSrc.html

**select-add**: `select-add.html`'s second `test()` calls
`testselect2.add(opt2)` where `opt2` is `document.getElementById("testoption")`
— the markup makes `opt2` an ANCESTOR of `testselect2` (`<option
id=testoption><select id=testselect2>…`). Per DOM §4.2.3, inserting a node
into one of its own descendants must throw `HierarchyRequestError`. The
native `_lumen_append_child`/`_lumen_insert_before`/`_lumen_insert_after`
bindings (`v8_runtime/install/dom_core.rs`) call straight into
`lumen_dom::Document::append_child`/`insert_before`/`insert_after`
(`crates/engine/dom/src/lib.rs`), whose only cycle guard is
`debug_assert!(!self.is_self_or_ancestor(...))` — compiled to nothing under
`profile.dev-release`/`release` (`inherits = "release"`, no
`debug-assertions` override, confirmed by grep). No `HierarchyRequestError` is
ever thrown, in EITHER direction (`appendChild`, `insertBefore`,
`insertAfter`): the call silently creates a real two-node parent cycle
(`is_self_or_ancestor` itself, called with the two IDs swapped, would say so).
This is a distinct defect from BUG-894 (`insertBefore` not checking the
*reference* node is a child) — this one is the *inclusive-ancestor* cycle
check entirely missing at the JS-visible boundary, not merely mis-checked.

**currentSrc**: `currentSrc.html` waits on `loadstart` for `<audio>`/`<video>`
with `src=""` (and the same-shaped `<source src="">` case) before doing its
assertions and calling `done()`. `video_bindings.rs::startFetch` computes
`abs = (url === '') ? null : resolveUrl(url)` and returns via
`failResource(...)` on `abs === null` — but `queueEvent('loadstart')` sits
*after* that early return, so an empty-string src never gets the event.
`audio_element.rs::startLoad` is worse: `if (!HAS_PROVIDER || !url) return;`
no-ops entirely on falsy url — neither `loadstart` NOR `error` fires for
`<audio src="">`. Same shape as the "per-feature shim, same defect fixed
twice" gotcha (CLAUDE.md): two independent implementations of "assign an
empty src", two different broken outcomes.

Same harness as slices 30-32: one browser process per page, served over http,
evidence read off the browser's own stderr.

## Results (2026-09-02, dev-release, Linux, `main` = `8b634befc`)

- `dom-cycle-appendchild` — confirms the grep read. `appendChild`/
  `insertBefore` never throw for the ancestor-cycle case; the tree becomes
  cyclic, and a subsequent tree walk (`outerHTML` serialization) hangs the
  page (the marker for that step never prints — everything up to it does).
  Filed as BUG-954 (P3 — DOM §4.2.3 "pre-insertion validity" check absent at
  the JS boundary, not merely mis-checked as BUG-894 already covers).
- `media-empty-src-loadstart` — confirms both halves. `<video src="">` fires
  `error` and never `loadstart`; `<audio src="">` fires neither. Filed as
  BUG-955 (P3 — extends the already-known "per-feature shim, same defect
  twice" shape onto the resource-selection algorithm's `loadstart` step).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice33_gaps.py
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
<title>slice-33 probe: __NAME__</title>
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

    # select-add.html's exact shape: opt2 is an ancestor of testselect2 in the
    # markup; `testselect2.add(opt2)` calls `appendChild(opt2)` with no
    # `before` argument.
    "dom-cycle-appendchild": ("""
<form style="display:none">
  <option id="testoption">
    <select id="testselect2">
      <option>TEST</option>
    </select>
  </option>
</form>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var testselect2 = document.getElementById("testselect2");
  var opt2 = document.getElementById("testoption");
  rep("opt2-is-ancestor-of-testselect2-before", function () { return opt2.contains(testselect2); });
  var threw = null;
  try {
    testselect2.add(opt2);
  } catch (e) {
    threw = (e && e.name) || String(e);
  }
  console.log("PROBE add-threw = " + threw);
  rep("testselect2-parent-after", function () { return testselect2.parentNode && testselect2.parentNode.id; });
  rep("opt2-parent-after", function () { return opt2.parentNode && (opt2.parentNode.id || opt2.parentNode.tagName); });
  console.log("PROBE about-to-serialize");
  rep("outerHTML-length", function () { return document.body.outerHTML.length; });
  console.log("PROBE serialize-done");
});
</script>
""", "add() throws HierarchyRequestError; no cycle; serialize-done prints"),

    # currentSrc.html's exact shape: script-created <video>/<audio>, `.src`
    # assigned to the empty string, `loadstart` armed before assignment.
    "media-empty-src-loadstart": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  ["video", "audio"].forEach(function (tag) {
    var e = document.createElement(tag);
    e.addEventListener("loadstart", function () { console.log("PROBE " + tag + "-loadstart"); });
    e.addEventListener("error", function () { console.log("PROBE " + tag + "-error"); });
    e.src = "";
    document.body.appendChild(e);
  });
  setTimeout(function () { console.log("PROBE media-empty-src-done"); }, 3000);
});
</script>
""", "loadstart fires on both tags before/regardless of the later error"),
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
    log_path = os.path.join(REPO, ".tmp", f"s33-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".s33-{name}.html"
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
    parser.add_argument("--seconds", type=float, default=6.0)
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
        path = os.path.join(HERE, f".s33-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)

    try:
        print(f"{'variant':32s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:32s} {ticks:5d}  {expected:62s} {seen}")
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
