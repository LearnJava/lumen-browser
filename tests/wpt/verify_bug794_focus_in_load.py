#!/usr/bin/env python3
"""BUG-794: what a window `load` handler can and cannot reach.

The report (WPT-RUN-6 slice 2) read the symptom as "`element.focus()` never
returns" and localized it no further than "somewhere in the transition to
`complete`".  Neither half survives measurement: `focus()` returns fine, and
what the report's repro actually hit is that the element it reached through
the window's *named access* (`el`, HTML LS §7.3.3) is a `ReferenceError`
inside a `load` handler and nowhere else — swallowed whole until BUG-591
wired the reporting, which is why it read as a hang.

The probe measures it the way the other slices do — one browser process per
page, served over http (never `file://`), evidence read off the browser's own
stderr, and a 500 ms `setInterval` heartbeat started *before* the `load` event
so that "the JS thread is wedged" is separable from "the handler stopped and
the thread kept running".

`named-access` is the variant that carries the bug; the rest are the controls
that took the diagnosis away from focus:

* `named-access`    — the bug: an element reached by `id` as a bare global,
                      in the top-level script, from `DOMContentLoaded`, from
                      the `load` handler, from rAF and from a timer. Only the
                      `load` row used to answer `undefined`, because that
                      dispatch is routed through the engine thread and races
                      the UI thread's own pass over the document, which the
                      interceptor's `try_lock` lost.
* `load-focus`      — the report's repro verbatim (`{preventScroll: true}`).
* `load-focus-scroll` — same, without `preventScroll`: does the §6.6.3 scroll
                      step matter?
* `dcl-focus`       — the control the report says works (`DOMContentLoaded`).
* `timer-focus`     — `focus()` from a timer, i.e. off any engine-driven
                      dispatch loop.
* `load-native`/`load-plain` — a `load` handler that calls a *different*
                      native, and one that touches nothing native at all.
* `wpt010`          — `css/selectors/focus-visible-010.html` minus the
                      harness, which is where the bug was found.

Usage (from repo root):

    python tests/wpt/verify_bug794_focus_in_load.py
        [--binary target/dev-release/lumen.exe] [--seconds 6] [--variant NAME]

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
<title>BUG-794 __NAME__</title>
<body>
<div id="el" tabindex="-1">hi</div>
<input id="i">
<script>
console.log("PROBE script-start");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
var target = document.getElementById("el");
target.addEventListener("focus", function () { console.log("PROBE focus-fired"); });
target.addEventListener("focusin", function () { console.log("PROBE focusin-fired"); });
function probeFocus(tag, opts) {
    console.log("PROBE " + tag + "-before");
    try {
        target.focus(opts);
        console.log("PROBE " + tag + "-returned active=" +
                    (document.activeElement ? document.activeElement.id : "none"));
    } catch (e) {
        console.log("PROBE " + tag + "-threw " + e);
    }
    console.log("PROBE " + tag + "-end");
}
__BODY__
</script>
</body>
"""

VARIANTS = {
    # The bug. `p()` asks the same three questions in five lifecycle phases;
    # every row must answer `typeof-el=object in-window=true`, and `byId=ok`
    # is the control that says the document itself was reachable at that very
    # moment (it is — `getElementById`'s native takes the same lock, blocking).
    "named-access": ("""
function p(tag) {
    console.log("PROBE " + tag + " typeof-el=" + (typeof el) +
                " in-window=" + ('el' in window) +
                " byId=" + (document.getElementById('el') ? "ok" : "null"));
}
p("toplevel");
document.addEventListener("DOMContentLoaded", function () { p("dcl"); });
window.addEventListener("load", function () { p("load"); });
requestAnimationFrame(function () { p("raf"); });
setTimeout(function () { p("timer"); }, 300);
""", "five rows, all typeof-el=object in-window=true"),

    "load-focus": ("""
window.addEventListener("load", function () {
    console.log("PROBE load-fired");
    probeFocus("load", {preventScroll: true});
    console.log("PROBE load-handler-end");
});
""", "load-fired, focus-fired, load-returned, load-handler-end"),

    "load-focus-scroll": ("""
window.addEventListener("load", function () {
    console.log("PROBE load-fired");
    probeFocus("load", undefined);
    console.log("PROBE load-handler-end");
});
""", "same, with the §6.6.3 scroll step left in"),

    "dcl-focus": ("""
document.addEventListener("DOMContentLoaded", function () {
    console.log("PROBE dcl-fired");
    probeFocus("dcl", {preventScroll: true});
    console.log("PROBE dcl-handler-end");
});
""", "dcl-fired, focus-fired, dcl-returned, dcl-handler-end"),

    "timer-focus": ("""
setTimeout(function () {
    console.log("PROBE timer-fired");
    probeFocus("timer", {preventScroll: true});
    console.log("PROBE timer-handler-end");
}, 300);
""", "timer-fired, focus-fired, timer-returned, timer-handler-end"),

    "load-native": ("""
window.addEventListener("load", function () {
    console.log("PROBE load-fired");
    document.title = "bug794";
    console.log("PROBE load-title " + document.title);
    console.log("PROBE load-style " + getComputedStyle(target).display);
    console.log("PROBE load-attr " + target.getAttribute("tabindex"));
    console.log("PROBE load-handler-end");
});
""", "load-fired, load-title/style/attr, load-handler-end"),

    # `css/selectors/focus-visible-010.html` verbatim, minus the harness: the
    # element is reached through the window's named access (`el`, no `var`),
    # `focus()` takes no argument, and the `focus` listener reads computed
    # style — the three things the report's own repro did differently.
    "wpt010": ("""
window.addEventListener("load", function () {
    console.log("PROBE load-fired");
    el.focus();
    console.log("PROBE load-returned active=" +
                (document.activeElement ? document.activeElement.id : "none"));
});
el.addEventListener("focus", function () {
    console.log("PROBE wpt-outline " + getComputedStyle(el).outlineColor +
                " bg " + getComputedStyle(el).backgroundColor);
});
""", "load-fired, focus-fired, wpt-outline rgb(0, 128, 0), load-returned"),

    "load-plain": ("""
window.addEventListener("load", function () {
    console.log("PROBE load-fired");
    var s = 0;
    for (var k = 0; k < 1000; k++) { s += k; }
    console.log("PROBE load-sum " + s);
    console.log("PROBE load-handler-end");
});
""", "load-fired, load-sum, load-handler-end"),
}

_MARKER_RE = re.compile(r"PROBE ([^\n\r]*)")
_TICK_RE = re.compile(r"PROBE tick ")
_MAX_MARKERS = 24


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Static file server that does not spam the probe's own output."""

    def log_message(self, fmt, *args):
        pass


def _serve(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds):
    """Launch one browser on one probe page; return (ticks, markers)."""
    log_path = os.path.join(REPO, ".tmp", f"bug794-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.bug794-{name}.html"],
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
    seen = set()
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker.startswith("tick ") or marker in seen:
            continue
        seen.add(marker)
        if len(markers) < _MAX_MARKERS:
            markers.append(marker)
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    default_bin = os.path.join(REPO, "target", "dev-release",
                               "lumen.exe" if os.name == "nt" else "lumen")
    parser.add_argument("--binary", default=default_bin)
    parser.add_argument("--seconds", type=float, default=6.0,
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
        path = os.path.join(HERE, f".bug794-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE.replace("__NAME__", name)
                             .replace("__BODY__", VARIANTS[name][0]))
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':20s} {'ticks':>5s}  {'expected':56s} markers seen")
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            print(f"{name:20s} {ticks:5d}  {VARIANTS[name][1]:56s} {seen}")
        print()
        print("ticks == 0 after the marker where the page stopped means the JS "
              "thread itself is wedged; ticks still climbing means the handler "
              "was abandoned while the runtime kept pumping.")
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
