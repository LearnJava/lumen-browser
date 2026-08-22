#!/usr/bin/env python3
"""WPT-RUN-6 slice 18: three silent waits behind the densest residual clusters.

After slice 17 the unexplained TIMEOUT residual of the WPT-RUN-5 snapshot is
729 ids, and three shapes cover a third of it:

* 86 ids wait for a `securitypolicyviolation` event (`content-security-policy`
  75, plus `trusted-types`, `html`, `preload`);
* 55 ids print `invalid url: missing scheme: "resources/…"` — a *relative* URL
  that reached the network layer unresolved (`xhr` dominates, but the same
  line appears under `html`, `navigation-api`, `websockets`, `resource-timing`);
* 45 ids build a `Worker`/`SharedWorker` and then wait on it.

(Those three counts are what the residual *looked* like from its signatures,
before the markers below existed. What the markers actually claim once written
is more, because a marker also reaches ids whose evidence is in a helper or in
the worker script: 105 + 71 + 32, residual 729 -> 543.)

None of those can be read off the browser's output the way an error text can
(the first and third print nothing at all), so a source marker for them has to
be confirmed against the engine first — the way slice 15
(`verify_event_delivery_gaps.py`) and slice 17
(`verify_layout_shift_and_peer_gaps.py`) confirmed theirs.

Same harness as those two, and for the same reasons recorded in `CLAUDE.md`:
one browser process per page, served over http (never `file://`), evidence read
off the browser's own stderr rather than through an MCP `eval`, and a 500 ms
`setInterval` tick so "the page is alive and heard nothing" is distinguishable
from "the page died".

Measured 2026-08-21 (dev-release, Linux, commit `41ee56b73`, `--seconds 6`;
every variant ticked 10–11 times, i.e. no page died on us):

    variant                  markers seen
    control                  raf, timeout                            <- the control
    csp-meta-img             (nothing at all)          <- BUG-804, not CSP: see below
    csp-meta-script          inline-script-ran         <- CSP said script-src 'self'
    csp-meta-spv             spv-class=function, and no event ever
    csp-header-spv           header-seen, and no event ever
    xhr-relative             xhr-error status=0        <- "missing scheme"
    xhr-root-relative        xhr-error status=0
    xhr-absolute             xhr-load status=200                     <- the control
    fetch-relative           fetch-resolved status=200               <- the control
    worker-postmessage       worker-message ready, echo:hi           <- the control
    worker-async-postmessage (nothing at all)
    worker-throw             (no error event, ever)
    worker-onerror-inside    (no error event, ever)
    worker-navigator         typeof navigator=undefined self=object
                             location=undefined setTimeout=function
    worker-timers            microtask                 <- and nothing else, ever
    worker-timers-poked      microtask, poked, timeout, interval:1

One row proves nothing and is kept only to say so: `csp-meta-img` prints
neither `load` nor `error`, but a parser-written `<img>` never fires either
event regardless of policy (BUG-804), so the image variants cannot be read as
evidence about CSP. The enforcement claim rests on `csp-meta-script`, where the
violation is observable from inside the page.

So three gaps, all silent:

1. **CSP is parsed and never enforced.** `crates/js/src/csp.rs` says so in its
   own header ("Phase 0 … No enforcement — the shell wires actual blocking in
   Phase 1"), and the hook it names, `_lumen_dispatch_csp_violation`, has no
   caller anywhere outside that file's own unit test. So a page declaring
   `img-src 'none'` still loads the image and a page declaring `script-src
   'self'` still runs its inline script, and — the part that produces TIMEOUTs
   rather than FAILs — the `securitypolicyviolation` event is never dispatched,
   so every `async_test` that waits for one waits forever. Filed as BUG-811.
   Not the same as BUG-692, which is one directive (`upgrade-insecure-requests`)
   not being applied to a URL; this is the whole enforcement step missing.

2. **A relative URL passed to `XMLHttpRequest.open()` is never resolved.**
   `XMLHttpRequest.prototype.open` (`crates/js/src/xhr.rs:216`) stores
   `String(url)` verbatim and `send()` hands that straight to
   `_lumen_fetch_sync`/`_lumen_fetch_sync_with_body`, so `open('GET',
   'resources/status.py')` reaches `lumen-network` as the literal string and
   dies with `invalid url: missing scheme`. The same bug was already fixed once
   for `window.open` (BUG-359) and the `fetch()` shim resolves against
   `_lumen_loc_href`; XHR is the surface that was missed. Filed as BUG-812,
   merged into BUG-780 (the earlier report of the same defect) on 2026-08-22 —
   the evidence this script produces now lives in BUG-780.

3. **A worker that throws never fires `error` at the `Worker` object.**
   The shim's `_errorListeners`/`_onerror` are invoked from exactly one place —
   the script-fetch-failure path of the constructor (`crates/js/src/worker.rs`,
   the `script === null` branch, BUG-364) — so an uncaught exception *inside* a
   started worker propagates nowhere, and neither does a worker's own
   `onerror` returning false. Message delivery itself works (the
   `worker-postmessage` control above), which is why this is a distinct,
   narrower gap than "workers do not work". Filed as BUG-813.

Two more came out of the worker variants and are filed separately, because
neither is fixed by fixing the third:

4. **The worker global scope has neither `navigator` nor `location`** — the
   shim defines `self`, `name`, `postMessage`, `console`, `importScripts`, the
   timer stubs and `queueMicrotask`, and nothing else, so
   `workers/support/WorkerNavigator.js` throws on its first line and, by (3),
   silently. Filed as BUG-814, merged into BUG-776 (the earlier and wider
   report of the same defect) on 2026-08-22.

5. **Worker timers run only when a message is dispatched to that worker**, the
   delay is ignored and `setInterval` is an alias of `setTimeout`, so it never
   repeats. The `worker-timers`/`worker-timers-poked` pair is what separates
   that from "worker timers do not work". Filed as BUG-815.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_csp_url_worker_gaps.py
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

#: Files the variants below load as subresources. Written next to the probe
#: pages and removed with them; the names are prefixed so a crashed run leaves
#: nothing that could be mistaken for a vendored WPT file.
SUPPORT = {
    ".cspgap-worker-echo.js": (
        "onmessage = function (e) { postMessage('echo:' + e.data); };\n"
        "postMessage('ready');\n"
    ),
    ".cspgap-worker-async.js": (
        # The shape `workers/support/WorkerNavigator.js` uses: a top-level
        # async IIFE that posts once it has read `navigator`.
        "(async () => {\n"
        "  const obj = { platform: navigator.platform, onLine: navigator.onLine };\n"
        "  postMessage('async:' + obj.platform + ',' + obj.onLine);\n"
        "})();\n"
    ),
    ".cspgap-worker-throw.js": (
        "onmessage = function (e) { throw new Error(e.data); };\n"
    ),
    ".cspgap-worker-onerror.js": (
        # `workers/support/ErrorEvent.js`: the worker handles its own error,
        # posts the details back, and returns false so the error still
        # propagates to the page's `Worker.onerror`.
        "onmessage = function (e) { throw new Error(e.data); };\n"
        "onerror = function (message, location, line, col) {\n"
        "  postMessage('inner-onerror:' + message);\n"
        "  return false;\n"
        "};\n"
    ),
    ".cspgap-worker-typeof.js": (
        # `typeof` of an undeclared name is the one read that cannot itself
        # throw, so a missing global is reported rather than swallowed.
        "postMessage('typeof navigator=' + typeof navigator"
        " + ' self=' + typeof self"
        " + ' location=' + typeof location"
        " + ' setTimeout=' + typeof setTimeout);\n"
    ),
    ".cspgap-worker-timers.js": (
        # Microtask against timer: both are armed at script load, and only the
        # first has a driver that does not need a message to run.
        "Promise.resolve().then(function () { postMessage('microtask'); });\n"
        "setTimeout(function () { postMessage('timeout'); }, 10);\n"
        "var _i = 0;\n"
        "setInterval(function () { postMessage('interval:' + (++_i)); }, 20);\n"
        "onmessage = function () { postMessage('poked'); };\n"
    ),
    ".cspgap-target.txt": "probe-body\n",
}

#: `body` is spliced into a page that also arms a `setInterval` logging
#: `PROBE tick`, so "the marker never came" is separable from "the page died".
#: `expect` is what the shape is believed to do today, printed next to the
#: measurement so a change in either direction is visible without re-reading
#: the docstring.
VARIANTS = {
    "control": ("", """
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
</script>
""", "raf+timeout"),

    # ── CSP ────────────────────────────────────────────────────────────────
    # Enforcement, not the event: `img-src 'none'` must stop the load, so
    # `onerror` is the correct outcome and `onload` is the defect.
    "csp-meta-img": ("", """
<meta http-equiv="Content-Security-Policy" content="img-src 'none'">
<img src=".cspgap-pixel.png"
     onload="console.log('PROBE img-onload')"
     onerror="console.log('PROBE img-onerror')">
""", "img-onerror (blocked)"),
    # `script-src 'self'` forbids *inline* script, so the marker below must
    # not be printed at all.
    "csp-meta-script": ("", """
<meta http-equiv="Content-Security-Policy" content="script-src 'self'">
<script>console.log("PROBE inline-script-ran");</script>
""", "nothing (inline blocked)"),
    # The wait itself, in the shape `content-security-policy/*` uses: an
    # `async_test` that only ever completes from the violation handler.
    "csp-meta-spv": ("", """
<meta http-equiv="Content-Security-Policy" content="img-src 'none'; script-src 'self'">
<script>
window.addEventListener("securitypolicyviolation", function (e) {
    console.log("PROBE spv-window directive=" + e.violatedDirective);
});
document.addEventListener("securitypolicyviolation", function (e) {
    console.log("PROBE spv-document directive=" + e.violatedDirective);
});
console.log("PROBE spv-class=" + typeof window.SecurityPolicyViolationEvent);
</script>
<img src=".cspgap-pixel.png" onload="console.log('PROBE img-loaded-anyway')">
""", "spv-window / spv-document"),
    # Same, but the policy arrives as a response header rather than a `<meta>`
    # — a different parse path, and the one most WPT `.sub.html` files use.
    "csp-header-spv": ("img-src 'none'; script-src 'self'", """
<script>
window.addEventListener("securitypolicyviolation", function (e) {
    console.log("PROBE spv-window directive=" + e.violatedDirective);
});
console.log("PROBE header-seen");
</script>
<img src=".cspgap-pixel.png" onload="console.log('PROBE img-loaded-anyway')">
""", "spv-window"),

    # ── relative URLs ──────────────────────────────────────────────────────
    # `xhr/*` opens its helpers as `resources/status.py`; this is that shape
    # reduced to a static file, so a failure cannot be the helper's fault.
    "xhr-relative": ("", """
<script>
var x = new XMLHttpRequest();
x.onload = function () { console.log("PROBE xhr-load status=" + x.status + " text=" + x.responseText.trim()); };
x.onerror = function () { console.log("PROBE xhr-error status=" + x.status); };
x.open("GET", ".cspgap-target.txt");
x.send();
</script>
""", "xhr-load status=200"),
    "xhr-root-relative": ("", """
<script>
var x = new XMLHttpRequest();
x.onload = function () { console.log("PROBE xhr-load status=" + x.status); };
x.onerror = function () { console.log("PROBE xhr-error status=" + x.status); };
x.open("GET", "/.cspgap-target.txt");
x.send();
</script>
""", "xhr-load status=200"),
    # The control that separates "XHR is broken" from "relative URLs are":
    # the identical request with the origin spelled out.
    "xhr-absolute": ("", """
<script>
var x = new XMLHttpRequest();
x.onload = function () { console.log("PROBE xhr-load status=" + x.status); };
x.onerror = function () { console.log("PROBE xhr-error status=" + x.status); };
x.open("GET", location.origin + "/.cspgap-target.txt");
x.send();
</script>
""", "xhr-load status=200"),
    # The other half of the same control: the identical *relative* URL through
    # `fetch()`, which resolves it against the document base (BUG-347). If this
    # succeeds where `xhr-relative` fails, the defect is XHR's URL handling and
    # not the network layer's. Kept a GET on purpose — the probe server is a
    # `SimpleHTTPRequestHandler` with no `do_POST`, so a POST would measure the
    # server rather than the engine.
    "fetch-relative": ("", """
<script>
fetch(".cspgap-target.txt").then(function (r) {
    console.log("PROBE fetch-resolved status=" + r.status);
}, function (e) {
    console.log("PROBE fetch-rejected " + e);
});
</script>
""", "fetch-resolved status=200"),

    # ── workers ────────────────────────────────────────────────────────────
    # Delivery works or it does not; every other worker variant is only
    # readable against this one.
    "worker-postmessage": ("", """
<script>
var w = new Worker(".cspgap-worker-echo.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
w.onerror = function (e) { console.log("PROBE worker-error " + e.message); };
w.postMessage("hi");
</script>
""", "worker-message data=ready/echo:hi"),
    "worker-async-postmessage": ("", """
<script>
var w = new Worker(".cspgap-worker-async.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
w.onerror = function (e) { console.log("PROBE worker-error " + e.message); };
</script>
""", "worker-message data=async:…"),
    # `workers/Worker_ErrorEvent_*.htm`: the page waits on `worker.onerror`
    # after making the worker throw.
    "worker-throw": ("", """
<script>
var w = new Worker(".cspgap-worker-throw.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
w.onerror = function (e) { console.log("PROBE worker-error message=" + e.message); };
w.addEventListener("error", function (e) { console.log("PROBE worker-error-listener"); });
w.postMessage("boom");
</script>
""", "worker-error message=…"),
    # `workers/support/ErrorEvent.js`: the worker's own `onerror` reports the
    # details *and* returns false so the page still gets the event.
    "worker-onerror-inside": ("", """
<script>
var w = new Worker(".cspgap-worker-onerror.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
w.onerror = function (e) { console.log("PROBE worker-error message=" + e.message); };
w.postMessage("boom");
</script>
""", "worker-message inner-onerror:… + worker-error"),
    # Which globals a worker actually has. `WorkerNavigator_*.htm` and the
    # whole `workers/support/WorkerNavigator.js` family read `navigator` at
    # the top of the worker script, so a missing one takes the `postMessage`
    # with it.
    "worker-navigator": ("", """
<script>
var w = new Worker(".cspgap-worker-typeof.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
w.onerror = function (e) { console.log("PROBE worker-error message=" + e.message); };
</script>
""", "worker-message typeof navigator=object"),
    # A worker that arms a timer and is never sent a message.
    "worker-timers": ("", """
<script>
var w = new Worker(".cspgap-worker-timers.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
</script>
""", "microtask, timeout, interval:1, interval:2…"),
    # The same worker, poked once a second in. Separates "worker timers never
    # run" from "worker timers run only when a message is dispatched".
    "worker-timers-poked": ("", """
<script>
var w = new Worker(".cspgap-worker-timers.js");
w.onmessage = function (e) { console.log("PROBE worker-message data=" + e.data); };
setTimeout(function () { w.postMessage("poke"); }, 1500);
</script>
""", "microtask, timeout, interval:1… before the poke"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-18 probe: %(name)s</title>
<body>
%(body)s
<script>
console.log("PROBE script-start");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: 1x1 transparent PNG — the `img-src` variants need a real image so a failed
#: decode cannot be mistaken for a blocked load.
PIXEL = bytes.fromhex(
    "89504e470d0a1a0a0000000d494844520000000100000001080600000"
    "01f15c4890000000a49444154789c6300010000050001"
    "0d0a2db40000000049454e44ae426082"
)

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages; silent so the probe output stays readable.

    Adds a `Content-Security-Policy` response header to any page whose
    variant declared one (`csp-header-spv`) — the header path is a different
    parser entry point from `<meta http-equiv>` and most WPT CSP tests use it.
    """

    #: path suffix → header value, filled in by `main()` before serving.
    csp_headers = {}

    def end_headers(self):
        policy = self.csp_headers.get(self.path.lstrip("/"))
        if policy:
            self.send_header("Content-Security-Policy", policy)
        super().end_headers()

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
    log_path = os.path.join(REPO, ".tmp", f"cspgap-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.cspgap-{name}.html"],
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
    parser.add_argument("--seconds", type=float, default=8.0,
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
    for filename, content in SUPPORT.items():
        path = os.path.join(HERE, filename)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)
    pixel_path = os.path.join(HERE, ".cspgap-pixel.png")
    with open(pixel_path, "wb") as handle:
        handle.write(PIXEL)
    written.append(pixel_path)

    for name in wanted:
        policy, body, _ = VARIANTS[name]
        page = f".cspgap-{name}.html"
        if policy:
            _Quiet.csp_headers[page] = policy
        path = os.path.join(HERE, page)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':24s} {'ticks':>5s}  {'expected':40s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            print(f"{name:24s} {ticks:5d}  {VARIANTS[name][2]:40s} "
                  f"{', '.join(markers) if markers else '— nothing'}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers above against the `expected` "
              "column: a live page (ticks > 0) that never printed its "
              "expected marker is waiting for something the engine does not "
              "produce, and a test built on that wait can only TIMEOUT")
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
