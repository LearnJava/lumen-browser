#!/usr/bin/env python3
"""BUG-804: does a `<script>`/`<link>`/`<style>` report the outcome of its load?

The bug report measured one axis (parser insertion vs `createElement`) with one
form of handler. This probe measures the whole matrix, because every previous
fix in this family turned out to cover fewer call sites than its report claimed:

* insertion path — written by the parser, or minted by `createElement`;
* handler form — the `on<type>` content attribute in the markup, the `on<type>`
  IDL attribute assigned from a later script, and `addEventListener`;
* outcome — a resource that arrives, and one that 404s;
* and, for `<style>`, the three moments HTML LS §4.14 «update a style block»
  names: insertion with a body, a later `textContent` write, and an `@import`
  that cannot be obtained.

The controls matter as much as the subjects. An inline parser `<script>` must
fire **nothing** (§4.12.1 fires `load` only when «el's from an external file» is
true), and the `createElement` paths for `<script>`/`<link>` are known-good
since BUG-571/BUG-722 — if those go quiet, the change under test broke them.

Harness and its reasons are the ones recorded in `CLAUDE.md`: one browser
process per page, served over http (never `file://`), evidence read off the
browser's own stderr rather than through an MCP `eval`, a 500 ms `setInterval`
tick so «the page is alive and heard nothing» is separable from «the page
died», and the probe's own server counting every path it is asked for — the
only half of the measurement that does not depend on the page being able to
report anything.

Usage (from repo root):

    tests/wpt/.venv/Scripts/python tests/wpt/verify_bug804_parser_resource_events.py
        --binary D:/RustProjects/.../target/dev-release/lumen.exe [--seconds 6]
        [--variant NAME]

`--binary` must be an absolute native path: an msys-style `/d/...` path reaches
`subprocess` as «binary not found».

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

#: Paths the probe server was asked for. A subresource request that never
#: arrives here was never made, whatever the page or the browser log says.
SERVED = []
_SERVED_LOCK = threading.Lock()

#: `body` is spliced into a page that also arms a `setInterval` logging
#: `PROBE tick`. `expect` is what a spec-compliant engine prints, kept next to
#: the measurement so a change in either direction is visible.
VARIANTS = {
    # ── controls: the paths BUG-571/BUG-722 already fixed ──────────────────
    "script-created-load": ("""
<script>
var s = document.createElement("script");
s.src = "b804-asset.js?created";
s.onload = function () { console.log("PROBE script-load"); };
s.onerror = function () { console.log("PROBE script-error"); };
document.body.appendChild(s);
</script>
""", "script-load"),
    "link-created-load": ("""
<script>
var l = document.createElement("link");
l.rel = "stylesheet";
l.href = "b804-asset.css?created";
l.onload = function () { console.log("PROBE link-load"); };
l.onerror = function () { console.log("PROBE link-error"); };
document.head.appendChild(l);
</script>
""", "link-load"),
    # An inline parser `<script>` must stay SILENT: §4.12.1 fires `load` only
    # for a script that came from an external file. A fix that reports every
    # parser script would break this one.
    "script-parsed-inline-silent": ("""
<script onload="console.log('PROBE inline-script-load')"
        onerror="console.log('PROBE inline-script-error')">
window.b804Inline = 1;
</script>
<script>
setTimeout(function () { console.log("PROBE inline-ran=" + (typeof window.b804Inline)); }, 400);
</script>
""", "inline-ran=number and NO inline-script-load"),

    # ── parser <script src>: the three handler forms × two outcomes ────────
    "script-parsed-attr-load": ("""
<script src="b804-asset.js?parsed-attr"
        onload="console.log('PROBE parsed-script-load')"
        onerror="console.log('PROBE parsed-script-error')"></script>
<script>
setTimeout(function () { console.log("PROBE parsed-script-ran=" + (typeof window.b804Ran)); }, 400);
</script>
""", "parsed-script-ran=number, parsed-script-load"),
    # The `addEventListener`/IDL-attribute forms, armed from a script that
    # stands ABOVE the target. That ordering is the whole point: a classic
    # `<script src>` blocks the parser, so it has already run — and already
    # fired — by the time a script BELOW it could reach the element. A spec
    # browser misses it there too, so a listener attached later says nothing
    # about this bug. `/b804-slow.js` is held for a second so the listener is
    # certainly armed while the load is still in flight.
    "script-parsed-listener-load": ("""
<script>
var s = document.getElementById("b804-slow");
console.log("PROBE found-parsed-script=" + (s ? "yes" : "no"));
if (s) {
    s.addEventListener("load", function (ev) {
        console.log("PROBE parsed-script-load-listener target=" +
                    (ev.target && ev.target.id) + " trusted=" + ev.isTrusted);
    });
    s.onload = function () { console.log("PROBE parsed-script-onload"); };
}
</script>
<script id="b804-slow" src="b804-slow.js?parsed-listener"></script>
<script>
setTimeout(function () { console.log("PROBE slow-script-ran=" + (typeof window.b804SlowRan)); }, 1500);
</script>
""", "parsed-script-load-listener + parsed-script-onload"),
    "script-parsed-404": ("""
<script src="b804-missing.js?parsed-404"
        onload="console.log('PROBE parsed-script-load')"
        onerror="console.log('PROBE parsed-script-error')"></script>
""", "parsed-script-error"),
    "script-parsed-module": ("""
<script type="module" src="b804-module.js?parsed-module"
        onload="console.log('PROBE parsed-module-load')"
        onerror="console.log('PROBE parsed-module-error')"></script>
""", "parsed-module-load"),
    # Ordering: §4.12.1 fires `load` right after the script body runs, i.e.
    # BEFORE the next parser script. A replay that batches the events until the
    # document is parsed reports `order=after` instead.
    "script-parsed-order": ("""
<script src="b804-asset.js?parsed-order"
        onload="window.b804LoadAt = window.b804Phase || 'before-next-script'"></script>
<script>
window.b804Phase = "after-next-script";
setTimeout(function () { console.log("PROBE order=" + window.b804LoadAt); }, 400);
</script>
""", "order=before-next-script"),

    # ── parser <link rel=stylesheet> ──────────────────────────────────────
    "link-parsed-attr-load": ("""
<link rel="stylesheet" href="b804-asset.css?parsed-attr"
      onload="console.log('PROBE parsed-link-load')"
      onerror="console.log('PROBE parsed-link-error')">
""", "parsed-link-load"),
    "link-parsed-listener-load": ("""
<link id="b804-slow-link" rel="stylesheet" href="b804-slow.css?parsed-listener">
<script>
var l = document.getElementById("b804-slow-link");
console.log("PROBE found-parsed-link=" + (l ? "yes" : "no"));
if (l) {
    l.addEventListener("load", function () { console.log("PROBE parsed-link-load-listener"); });
    l.onload = function () { console.log("PROBE parsed-link-onload"); };
    l.onerror = function () { console.log("PROBE parsed-link-onerror"); };
}
</script>
""", "parsed-link-load-listener + parsed-link-onload"),
    "link-parsed-404": ("""
<link rel="stylesheet" href="b804-missing.css?parsed-404"
      onload="console.log('PROBE parsed-link-load')"
      onerror="console.log('PROBE parsed-link-error')">
""", "parsed-link-error"),
    # A `media` that does not match: the shell never fetches such a sheet, so
    # whatever this reports is the residual to record, not a regression.
    "link-parsed-nonmatching-media": ("""
<link rel="stylesheet" media="print" href="b804-asset.css?parsed-print"
      onload="console.log('PROBE print-link-load')"
      onerror="console.log('PROBE print-link-error')">
""", "print-link-load (real browsers fetch it anyway)"),

    # ── <style>: HTML LS §4.14 «update a style block» ──────────────────────
    "style-parsed-load": ("""
<style onload="console.log('PROBE parsed-style-load')"
       onerror="console.log('PROBE parsed-style-error')">
#b804 { color: rgb(1, 2, 3); }
</style>
""", "parsed-style-load"),
    # `style_load_async.html`: the event must not be dispatched inside the
    # insertion, so `sync` is already false by the time the handler runs.
    "style-parsed-async": ("""
<style onload="console.log('PROBE style-load sync=' + window.b804Sync)">
#b804 { color: rgb(1, 2, 3); }
</style>
<script>window.b804Sync = false;</script>
""", "style-load sync=false"),
    "style-created-load": ("""
<script>
var st = document.createElement("style");
st.textContent = "#b804 { color: rgb(4, 5, 6); }";
st.addEventListener("load", function () { console.log("PROBE style-load-listener"); });
st.onload = function () { console.log("PROBE style-onload"); };
st.onerror = function () { console.log("PROBE style-error"); };
document.head.appendChild(st);
console.log("PROBE style-appended sheet=" + (st.sheet ? "yes" : "no"));
</script>
""", "style-load-listener + style-onload"),
    # `style_load_event.html`: a later `textContent` write re-runs the update
    # and must fire `load` a SECOND time.
    "style-textcontent-reload": ("""
<style id="b804-style" onload="window.b804Loads = (window.b804Loads || 0) + 1;">
.box { color: red; }
</style>
<script>
setTimeout(function () {
    console.log("PROBE loads-after-parse=" + window.b804Loads);
    document.getElementById("b804-style").textContent = ".box { color: green; }";
    setTimeout(function () { console.log("PROBE loads-after-mutate=" + window.b804Loads); }, 400);
}, 400);
</script>
""", "loads-after-parse=1, loads-after-mutate=2"),
    # `style_events.html`'s second element: an `@import` that 404s makes the
    # whole style block report `error`.
    "style-parsed-import-404": ("""
<style onload="console.log('PROBE import-style-load')"
       onerror="console.log('PROBE import-style-error')">
@import url(b804-missing.css);
</style>
""", "import-style-error"),
    "style-created-import-ok": ("""
<script>
var st = document.createElement("style");
st.onload = function () { console.log("PROBE import-style-load"); };
st.onerror = function () { console.log("PROBE import-style-error"); };
st.appendChild(document.createTextNode('@import url("b804-asset.css?style-import");'));
document.head.appendChild(st);
</script>
""", "import-style-load + a request for b804-asset.css"),
}

PAGE = """<!doctype html>
<meta charset="utf-8">
<title>bug804 %(name)s</title>
<body>
%(body)s
<script>
console.log("PROBE script-start");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: Files the probe pages point at. Kept tiny: what matters is whether the
#: request arrives at all, not what comes back.
ASSETS = {
    "b804-asset.js": "window.b804Ran = (window.b804Ran || 0) + 1;\n",
    "b804-module.js": "export const b804 = 1;\n",
    "b804-asset.css": "#b804 { color: rgb(1, 2, 3); }\n",
}

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every path asked for.

    `/b804-slow.*` is held for a second before answering — the stand-in for
    wptserve's `?pipe=trickle(d1)`, so a listener attached by the next script
    still beats the response.
    """

    def do_GET(self):  # noqa: N802 — http.server's own casing
        with _SERVED_LOCK:
            SERVED.append(self.path)
        if self.path.startswith("/b804-slow."):
            time.sleep(1.0)
            body = (b"window.b804SlowRan = 1;\n" if ".js" in self.path
                    else b"#b804 { color: rgb(7, 8, 9); }\n")
            ctype = "text/javascript" if ".js" in self.path else "text/css"
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return None
        return super().do_GET()

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
    """Launch one browser on one probe page; return (ticks, markers, fetched)."""
    log_path = os.path.join(REPO, ".tmp", f"b804-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.b804-{name}.html"],
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
    with _SERVED_LOCK:
        # Counted, not de-duplicated: «was it fetched again» is a different
        # question from «was it fetched», and a set cannot answer the first.
        counts = {}
        for path in SERVED:
            if path.startswith("/.b804-"):
                continue
            counts[path] = counts.get(path, 0) + 1
    fetched = [p if n == 1 else f"{p} x{n}" for p, n in sorted(counts.items())]
    return ticks, markers, fetched


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
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
        body, _ = VARIANTS[name]
        path = os.path.join(HERE, f".b804-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':30s} {'ticks':>5s}  {'expected':52s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, fetched = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            seen += "   [server saw: " + (", ".join(fetched) if fetched else "nothing") + "]"
            print(f"{name:30s} {ticks:5d}  {VARIANTS[name][1]:52s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half.")
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
