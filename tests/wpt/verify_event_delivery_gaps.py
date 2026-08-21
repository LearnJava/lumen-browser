#!/usr/bin/env python3
"""WPT-RUN-6 slice 15: which *event families* the engine never dispatches.

Three residual TIMEOUT clusters of the WPT-RUN-5 snapshot (`css/css-animations`
+ `css/css-transitions` + `css/css-variables`, `svg/animations`,
`intersection-observer`) share one shape: the page arms a listener and waits.
Nothing is printed and nothing throws — a missing event is silent by
construction, which is exactly why the classifier's evidence stages cannot see
it and why a source marker needs the fact confirmed before it is written.

This probe confirms it directly. Each variant is one page in its own browser
process, served over http (a dump mode has no frame loop worth measuring),
logging `PROBE <marker>` from the listener under test and `PROBE tick N` from a
500 ms `setInterval`, so a page that stays alive but never hears the event is
distinguishable from a page that wedged. Evidence is read off the browser's own
stderr rather than through an MCP `eval`, for the reason recorded in
`CLAUDE.md`: a frozen page answers `eval` with "JS context not available", and
slice 6 misread exactly that as a broken live window.

Measured 2026-08-21 (dev-release, Linux, commit a7ee9468f, `--seconds 8`;
15 `setInterval` ticks means the page stayed alive throughout):

    variant                  ticks  markers seen
    control                     15  raf, timeout
    waapi-finish                15  waapi-finish        <- WAAPI events do fire
    css-animation               15  — nothing           <- animation* never
    css-transition              15  — nothing           <- transition* never
    css-animation-progress      15  opacity 1           <- and no interpolation
    css-animation-layout        15  left 8              <- nor any geometry
    smil-events                 15  rect-width 10       <- begin/end/repeat never
    smil-dom                    15  ctor HTMLUnknownElement, no-beginElement
    io-initial                  15  — nothing           <- no initial delivery
    io-initial-then-relayout    15  mutating-unrelated, io-cb ratio=1
    io-after-mutation           15  io-cb ratio=1       <- only the change
    io-v2-trackvisibility       15  — nothing

So three separate gaps, one shape: the CSS animation/transition machinery is
invisible to JS end to end (no event, no computed value, no geometry — while
WAAPI on the same page is fine, so this is the CSS-driven path specifically,
[BUG-503]/[BUG-536]); SMIL does not exist at all ([BUG-806]); and
`IntersectionObserver` delivers only as a side effect of a relayout, never the
observation `observe()` is supposed to queue ([BUG-807]) — a *foreign*
element's mutation is enough to flush it, which is what makes the gap look
intermittent from a test's point of view.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_event_delivery_gaps.py
        [--binary target/dev-release/lumen] [--seconds 12] [--variant NAME]

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

#: `body` is spliced into a page that also arms a `setInterval` logging
#: `PROBE tick`, so "the event never came" is separable from "the page died".
#: `expect` is what the shape is believed to do today, printed next to the
#: measurement so a change in either direction is visible without re-reading
#: the docstring.
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
</script>
""", "raf+timeout"),
    "waapi-finish": ("""
<div id=t style="width:50px;height:50px;background:blue"></div>
<script>
var anim = document.getElementById("t").animate(
    [{opacity: 1}, {opacity: 0}], {duration: 100});
anim.onfinish = function () { console.log("PROBE waapi-finish"); };
anim.addEventListener("finish", function () { console.log("PROBE waapi-finish-listener"); });
</script>
""", "finish"),
    "css-animation": ("""
<style>
@keyframes fade { from { opacity: 1 } to { opacity: 0 } }
#t { width:50px; height:50px; background:blue;
     animation: fade 100ms linear 2; }
</style>
<div id=t></div>
<script>
var t = document.getElementById("t");
["animationstart", "animationend", "animationiteration", "animationcancel"]
    .forEach(function (type) {
        t.addEventListener(type, function () { console.log("PROBE " + type); });
    });
</script>
""", "start+iteration+end"),
    "css-transition": ("""
<style>#t { width:50px; height:50px; background:blue; transition: opacity 100ms linear; }</style>
<div id=t></div>
<script>
var t = document.getElementById("t");
["transitionrun", "transitionstart", "transitionend", "transitioncancel"]
    .forEach(function (type) {
        t.addEventListener(type, function () { console.log("PROBE " + type); });
    });
getComputedStyle(t).opacity;
requestAnimationFrame(function () { t.style.opacity = "0"; });
setTimeout(function () { t.style.opacity = "0"; }, 100);
</script>
""", "run+start+end"),
    "css-animation-progress": ("""
<style>
@keyframes fadeout { from { opacity: 1 } to { opacity: 0 } }
#t { width:50px; height:50px; background:blue; animation: fadeout 2s linear; }
</style>
<div id=t></div>
<script>
var t = document.getElementById("t");
var seen = 0;
var timer = setInterval(function () {
    console.log("PROBE opacity " + getComputedStyle(t).opacity);
    if (++seen > 5) clearInterval(timer);
}, 300);
</script>
""", "opacity falling 1→0"),
    "css-animation-layout": ("""
<style>
@keyframes slide { from { margin-left: 0px } to { margin-left: 300px } }
#t { width:50px; height:50px; background:blue; animation: slide 2s linear; }
</style>
<div id=t></div>
<script>
var t = document.getElementById("t");
var seen = 0;
var timer = setInterval(function () {
    console.log("PROBE left " + t.getBoundingClientRect().left);
    if (++seen > 5) clearInterval(timer);
}, 300);
new ResizeObserver(function () { console.log("PROBE ro-fired"); }).observe(t);
</script>
""", "left climbing 0→300"),
    "smil-events": ("""
<svg width="200" height="100">
  <rect id=r width="10" height="10" fill="blue">
    <animate id=a attributeName="width" from="10" to="100" begin="0s" dur="100ms"
             repeatCount="2" onbegin="console.log('PROBE onbegin-attr')"/>
  </rect>
</svg>
<script>
var a = document.getElementById("a");
["beginEvent", "endEvent", "repeatEvent"].forEach(function (type) {
    a.addEventListener(type, function () { console.log("PROBE " + type); });
});
setTimeout(function () {
    console.log("PROBE rect-width " + document.getElementById("r").getAttribute("width"));
}, 1000);
</script>
""", "beginEvent+repeatEvent+endEvent"),
    "smil-dom": ("""
<svg width="200" height="100">
  <rect width="10" height="10" fill="blue">
    <set id=s attributeName="width" to="100" begin="indefinite"/>
  </rect>
</svg>
<script>
var s = document.getElementById("s");
console.log("PROBE ctor " + s.constructor.name);
if (typeof s.beginElement === "function") {
    console.log("PROBE has-beginElement");
    s.beginElement();
} else {
    console.log("PROBE no-beginElement");
}
</script>
""", "has-beginElement"),
    "io-initial": ("""
<div id=t style="width:50px;height:50px;background:blue"></div>
<script>
new IntersectionObserver(function (entries) {
    console.log("PROBE io-cb ratio=" + entries[0].intersectionRatio);
}).observe(document.getElementById("t"));
</script>
""", "io-cb"),
    "io-after-mutation": ("""
<div id=t style="width:50px;height:50px;background:blue;position:absolute;top:5000px"></div>
<script>
var n = 0;
new IntersectionObserver(function (entries) {
    console.log("PROBE io-cb" + (++n > 1 ? n : "") + " ratio=" + entries[0].intersectionRatio);
}).observe(document.getElementById("t"));
requestAnimationFrame(function () {
    requestAnimationFrame(function () { document.getElementById("t").style.top = "10px"; });
});
</script>
""", "io-cb twice"),
    "io-initial-then-relayout": ("""
<div id=t style="width:50px;height:50px;background:blue"></div>
<div id=other>other</div>
<script>
new IntersectionObserver(function (entries) {
    console.log("PROBE io-cb ratio=" + entries[0].intersectionRatio);
}).observe(document.getElementById("t"));
setTimeout(function () {
    console.log("PROBE mutating-unrelated");
    document.getElementById("other").style.height = "300px";
}, 1000);
</script>
""", "io-cb only after the unrelated relayout"),
    "io-v2-trackvisibility": ("""
<div id=t style="width:50px;height:50px;background:blue"></div>
<script>
new IntersectionObserver(function (entries) {
    console.log("PROBE io-cb isVisible=" + entries[0].isVisible);
}, {trackVisibility: true, delay: 100}).observe(document.getElementById("t"));
</script>
""", "io-cb isVisible=false"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-15 probe: %(name)s</title>
<body>
%(body)s
<script>
console.log("PROBE script-start");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages; silent so the probe output stays readable."""

    def log_message(self, fmt, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    """Start a background http server on `root`, return (port, shutdown)."""
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds):
    """Launch one browser on one probe page; return (ticks, markers seen)."""
    log_path = os.path.join(REPO, ".tmp", f"evgap-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.evgap-{name}.html"],
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
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker.startswith("tick ") or marker == "script-start":
            continue
        if marker not in markers:
            markers.append(marker)
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=12.0,
                        help="how long each page is allowed to run")
    parser.add_argument("--variant", action="append", default=None,
                        help="run only these variants (repeatable)")
    args = parser.parse_args()

    wanted = args.variant or list(VARIANTS)
    unknown = [name for name in wanted if name not in VARIANTS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    written = []
    for name in wanted:
        body, _ = VARIANTS[name]
        path = os.path.join(HERE, f".evgap-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)
    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':24s} {'ticks':>5s}  {'expected':24s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            print(f"{name:24s} {ticks:5d}  {VARIANTS[name][1]:24s} "
                  f"{', '.join(markers) if markers else '— nothing'}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no event ever arrived on:", ", ".join(silent))
            print("=> those families are never dispatched; a test that waits "
                  "for one hangs to the runner's timeout with an empty log")
        else:
            print("=> every probed family delivered something")
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
