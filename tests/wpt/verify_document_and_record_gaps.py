#!/usr/bin/env python3
"""WPT-RUN-6 slice 16: does an XML document run scripts, and does a
`MutationObserver` ever deliver?

Three residual TIMEOUT clusters of the WPT-RUN-5 snapshot point at something
neither the evidence stage nor the existing source markers can see, because in
all three the browser prints nothing at all:

* 18 `.svg` test files (13 of them the whole `dom/nodes/Element-*-svg.svg`
  family) — the snapshot log shows the document parsed and painted and
  `testharness.js` never even *requested*;
* 16 files whose whole body is a `MutationObserver` round trip
  (`dom/nodes/MutationObserver-*.html`, `ParentNode-*.html`), all of which the
  shim is supposed to support — three bugs (BUG-317/318/326) were fixed against
  exactly these files;
* the `window` `error` / `unhandledrejection` pair (BUG-591/BUG-716), already
  known to be undispatched but never measured on the *attribute* form
  (`window.onerror = …`), which is a different code path from
  `addEventListener`.

Each variant is one page in its own browser process, served over http, logging
`PROBE <marker>` from the code under test and `PROBE tick N` from a 500 ms
`setInterval` — a page that stays alive but never hears its callback is then
distinguishable from a page that wedged (slice 15's rule; a frozen page also
answers an MCP `eval` with "JS context not available", which slice 6 misread as
a broken live window). For the document-type variants the interesting evidence
is one step earlier: whether the browser ever issued a `GET` for the external
script the document references, which is read out of the same stderr.

Measured 2026-08-21 (dev-release, Linux, commit cfca4049c, `--seconds 8`).
`ext-js` is blank where the variant's markup does not reference the external
file at all; the `stderr` column is the browser's own line, not a marker:

    variant                   ticks ext-js  markers seen        stderr
    control                      15  yes    external,raf,timeout
    svg-doc-external              0  no     —                   —
    svg-doc-inline                0  no     —                   —
    svg-doc-svgns                 0  no     —                   Unexpected token '<'
    svg-markup-as-html            0   —     —                   Unexpected token '<'
    svg-root-cdata-script         0   —     —                   Unexpected token '<'
    svg-root-plain-script        15   —     svg-root-plain-script
    svg-script-in-html           15  yes    svg-script-in-html
    xhtml-doc                     0  yes    external            Unexpected token '<'
    xhtml-inline-plain           15  yes    external, xhtml-inline-plain
    xhtml-selfclosed-script       0  yes    external            —
    mo-attributes                15  yes    mo-cb 1 attributes data-x
    mo-attributes-oldvalue       15  yes    mo-cb attributes old=c01
    mo-childlist                 15  yes    mo-cb childList added=1
    mo-characterdata             15  yes    mo-cb characterData old=text
    mo-subtree                   15  yes    mo-cb childList
    mo-takerecords               15  yes    takeRecords=1
    window-error-listener        15  yes    —                   —
    window-error-attr            15  yes    —                   —
    unhandledrejection           15  yes    —                   —

Read three ways:

1. **The XML clusters are BUG-786, extended from `<style>` to `<script>`.** An
   XML document is parsed by the HTML tree builder like everything else
   (`crates/shell/src/main.rs:5365`), and three XML-only constructs do not
   survive that: a prefixed `<h:script src>` never becomes a script at all (no
   GET, no error — `svg-doc-external`/`svg-doc-inline`), a self-closing
   `<script src="..."/>` swallows the rest of the document as its own text
   (`xhtml-selfclosed-script`: the external file loads, nothing after it runs),
   and `<![CDATA[` is a syntax error at the head of the script text. The
   content type is *not* what decides: `svg-markup-as-html` serves the same
   bytes as `text/html` and fails identically, and `svg-root-plain-script` —
   the same SVG-rooted document with a plain script — runs fine.
2. **MutationObserver is not the `dom/nodes` culprit.** All six record shapes
   deliver, with the right `type`, `oldValue` and `addedNodes`, and
   `takeRecords()` returns the pending one. Whatever hangs
   `MutationObserver-*.html` is something else; do not re-open that trail here.
3. **The attribute form is as dead as the listener form.** `window.onerror =`
   and `window.onunhandledrejection =` are separate code paths from
   `addEventListener`, and neither fires — the page keeps ticking, so BUG-591
   and BUG-716 cover both spellings.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_document_and_record_gaps.py
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

#: Every variant references this file, so "did the document run scripts at all"
#: can be answered from the browser's own request log even when the document
#: type is one whose script the engine may not execute.
EXTERNAL_JS = ".docgap-external.js"

#: name -> (document text, extension, what a spec-compliant engine does)
#:
#: HTML variants are spliced into `HTML_PAGE`, which arms the tick timer; the
#: XML ones carry their own tick because the point of the variant is whether
#: their script runs at all.
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
</script>
""", ".html", "external+raf+timeout"),

    # --- does an XML document execute script? -----------------------------
    # `dom/nodes/Element-childElementCount-svg.svg` is exactly this shape: an
    # SVG document whose harness is pulled in through XHTML-namespaced
    # `<h:script src>` and whose test body is a CDATA section.
    "svg-doc-external": ("""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml"
     version="1.1" width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-doc-external</title>
<h:script src="/%(external)s"/>
<text x="20" y="40" font-size="20">svg-doc-external</text>
</svg>
""", ".svg", "external script fetched and run"),

    "svg-doc-inline": ("""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml"
     version="1.1" width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-doc-inline</title>
<h:script><![CDATA[
console.log("PROBE svg-inline-h-script");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
]]></h:script>
<text x="20" y="40" font-size="20">svg-doc-inline</text>
</svg>
""", ".svg", "inline CDATA script runs"),

    "svg-doc-svgns": ("""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" version="1.1"
     width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-doc-svgns</title>
<script type="application/ecmascript"><![CDATA[
console.log("PROBE svg-inline-svg-script");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
]]></script>
<script xlink:href="/%(external)s" xmlns:xlink="http://www.w3.org/1999/xlink"/>
<text x="20" y="40" font-size="20">svg-doc-svgns</text>
</svg>
""", ".svg", "SVG-namespaced script runs"),

    # The `svg-doc-svgns` markup byte for byte, served as `text/html` from a
    # `.htm` file. Splits "the engine dispatches on the XML content type" from
    # "the HTML tree builder loses this markup": both documents reach the same
    # parser (`main.rs:5365` parses every navigation as HTML and only stamps
    # `document.contentType` afterwards), so a difference here would have to
    # come from the content type.
    "svg-markup-as-html": ("""<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" version="1.1"
     width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-markup-as-html</title>
<script type="application/ecmascript"><![CDATA[
console.log("PROBE svg-markup-as-html");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
]]></script>
<text x="20" y="40" font-size="20">svg-markup-as-html</text>
</svg>
""", ".htm", "same as svg-doc-svgns iff the content type is irrelevant"),

    # The same again with the XML prolog dropped *and* the CDATA section
    # dropped: the pair `svg-root-cdata-script` / `svg-root-plain-script`
    # isolates which of the two is what the SVG-document variants really die
    # on. (It is the CDATA: this one runs, the next one throws.)
    "svg-root-plain-script": ("""<svg xmlns="http://www.w3.org/2000/svg" version="1.1"
     width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-root-plain-script</title>
<script type="application/ecmascript">
console.log("PROBE svg-root-plain-script");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
<text x="20" y="40" font-size="20">svg-root-plain-script</text>
</svg>
""", ".htm", "script runs"),

    "svg-root-cdata-script": ("""<svg xmlns="http://www.w3.org/2000/svg" version="1.1"
     width="100%%" height="100%%" viewBox="0 0 400 400">
<title>slice-16 probe: svg-root-cdata-script</title>
<script type="application/ecmascript"><![CDATA[
console.log("PROBE svg-root-cdata-script");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
]]></script>
<text x="20" y="40" font-size="20">svg-root-cdata-script</text>
</svg>
""", ".htm", "nothing — `<![CDATA[` is a syntax error in HTML script text"),

    "xhtml-doc": ("""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>slice-16 probe: xhtml-doc</title>
<script src="/%(external)s"></script>
</head>
<body>
<script><![CDATA[
console.log("PROBE xhtml-inline");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
]]></script>
<p>xhtml-doc</p>
</body>
</html>
""", ".xhtml", "both scripts run"),

    # Same document, inline script written the way an HTML page writes it.
    # Splits "XHTML runs no inline script" from "the CDATA section is what
    # kills it": under HTML parsing `<![CDATA[` is a syntax error at the head
    # of the script text, and BUG-591 eats the exception.
    "xhtml-inline-plain": ("""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>slice-16 probe: xhtml-inline-plain</title>
<script src="/%(external)s"></script>
</head>
<body>
<script>
console.log("PROBE xhtml-inline-plain");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
<p>xhtml-inline-plain</p>
</body>
</html>
""", ".xhtml", "inline script runs"),

    # `<script src="..."/>` self-closes in XML and does NOT in HTML, where
    # everything up to the first `</script>` becomes the element's text. This
    # is the shape of `css/cssom/MediaList2.xhtml` and both `cssom-view`
    # `.xht` residuals: the *second* `<script src>` and the test body are
    # swallowed as text of the first one.
    "xhtml-selfclosed-script": ("""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>slice-16 probe: xhtml-selfclosed-script</title>
<script src="/%(external)s"/>
</head>
<body>
<p>xhtml-selfclosed-script</p>
<script>
console.log("PROBE after-selfclosed");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
</html>
""", ".xhtml", "external + after-selfclosed"),

    # Control for the three SVG-document variants: the same SVG-namespaced
    # `<script>`, but inside an ordinary HTML document. Separates "an `.svg`
    # document runs nothing" from "a script inside `<svg>` never runs".
    "svg-script-in-html": ("""
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
  <script type="application/ecmascript">
console.log("PROBE svg-script-in-html");
  </script>
  <rect width="50" height="50" fill="green"/>
</svg>
""", ".html", "svg-script-in-html"),

    # --- does MutationObserver deliver? -----------------------------------
    # One variant per record type the `dom/nodes/MutationObserver-*.html`
    # family covers; each mutates once from a `setTimeout` so the observation
    # is armed before the mutation, exactly as `runMutationTest` does.
    "mo-attributes": ("""
<p id=t></p>
<script>
var t = document.getElementById("t");
new MutationObserver(function (records) {
    console.log("PROBE mo-cb " + records.length + " " + records[0].type
                + " " + records[0].attributeName);
}).observe(t, {attributes: true});
setTimeout(function () {
    console.log("PROBE mutating");
    t.setAttribute("data-x", "1");
}, 700);
</script>
""", ".html", "mo-cb 1 attributes data-x"),

    "mo-attributes-oldvalue": ("""
<p id=t class="c01"></p>
<script>
var t = document.getElementById("t");
new MutationObserver(function (records) {
    console.log("PROBE mo-cb " + records[0].type + " old=" + records[0].oldValue);
}).observe(t, {attributes: true, attributeOldValue: true,
               attributeFilter: ["class"]});
setTimeout(function () {
    console.log("PROBE mutating");
    t.className = "c02";
}, 700);
</script>
""", ".html", "mo-cb attributes old=c01"),

    "mo-childlist": ("""
<div id=t></div>
<script>
var t = document.getElementById("t");
new MutationObserver(function (records) {
    console.log("PROBE mo-cb " + records[0].type
                + " added=" + records[0].addedNodes.length);
}).observe(t, {childList: true});
setTimeout(function () {
    console.log("PROBE mutating");
    t.appendChild(document.createElement("span"));
}, 700);
</script>
""", ".html", "mo-cb childList added=1"),

    "mo-characterdata": ("""
<p id=t>text</p>
<script>
var t = document.getElementById("t").firstChild;
new MutationObserver(function (records) {
    console.log("PROBE mo-cb " + records[0].type + " old=" + records[0].oldValue);
}).observe(t, {characterData: true, characterDataOldValue: true});
setTimeout(function () {
    console.log("PROBE mutating");
    t.data = "changed";
}, 700);
</script>
""", ".html", "mo-cb characterData old=text"),

    "mo-subtree": ("""
<div id=t><div id=inner></div></div>
<script>
new MutationObserver(function (records) {
    console.log("PROBE mo-cb " + records[0].type);
}).observe(document.getElementById("t"), {childList: true, subtree: true});
setTimeout(function () {
    console.log("PROBE mutating");
    document.getElementById("inner").appendChild(document.createElement("b"));
}, 700);
</script>
""", ".html", "mo-cb childList"),

    "mo-takerecords": ("""
<p id=t></p>
<script>
var t = document.getElementById("t");
var obs = new MutationObserver(function () {});
obs.observe(t, {attributes: true});
t.setAttribute("data-x", "1");
console.log("PROBE takeRecords=" + obs.takeRecords().length);
</script>
""", ".html", "takeRecords=1"),

    # --- the two events a harness reports a failure through ---------------
    "window-error-listener": ("""
<script>
window.addEventListener("error", function (e) {
    console.log("PROBE error-listener " + (e && e.message));
});
setTimeout(function () { throw new Error("boom-listener"); }, 700);
</script>
""", ".html", "error-listener boom-listener"),

    "window-error-attr": ("""
<script>
window.onerror = function (msg) { console.log("PROBE error-attr " + msg); };
setTimeout(function () { throw new Error("boom-attr"); }, 700);
</script>
""", ".html", "error-attr … boom-attr"),

    "unhandledrejection": ("""
<script>
window.addEventListener("unhandledrejection", function (e) {
    console.log("PROBE rejection-listener " + e.reason);
});
window.onunhandledrejection = function (e) {
    console.log("PROBE rejection-attr " + e.reason);
};
setTimeout(function () { Promise.reject(new Error("boom-rejection")); }, 700);
</script>
""", ".html", "rejection-listener + rejection-attr"),
}

HTML_PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-16 probe: %(name)s</title>
<body>
<script src="/%(external)s"></script>
%(body)s
<script>
console.log("PROBE script-start");
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

EXTERNAL_BODY = 'console.log("PROBE external");\n'

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_EXTERNAL_GET_RE = re.compile(r"GET \S*" + re.escape(EXTERNAL_JS))


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages; silent so the probe output stays readable."""

    #: `SimpleHTTPRequestHandler` guesses from the extension, and a stock
    #: Python install has no mapping for `.xhtml` — serving it as
    #: `application/octet-stream` would make the variant measure the wrong
    #: thing (a download, not an XML document).
    extensions_map = dict(http.server.SimpleHTTPRequestHandler.extensions_map,
                          **{".xhtml": "application/xhtml+xml",
                             ".svg": "image/svg+xml"})

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


def _run_variant(binary, name, ext, http_port, seconds):
    """Launch one browser on one probe page; return (ticks, requested, markers)."""
    log_path = os.path.join(REPO, ".tmp", f"docgap-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.docgap-{name}{ext}"],
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
    requested = bool(_EXTERNAL_GET_RE.search(text))
    markers = []
    for marker in _MARKER_RE.findall(text):
        marker = marker.strip()
        if marker.startswith("tick ") or marker == "script-start":
            continue
        if marker not in markers:
            markers.append(marker)
    return ticks, requested, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",
                        default=os.path.join(REPO, "target", "dev-release", "lumen"))
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

    written = [os.path.join(HERE, EXTERNAL_JS)]
    with open(written[0], "w", encoding="utf-8") as handle:
        handle.write(EXTERNAL_BODY)
    for name in wanted:
        body, ext, _ = VARIANTS[name]
        path = os.path.join(HERE, f".docgap-{name}{ext}")
        subst = {"name": name, "body": body, "external": EXTERNAL_JS}
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(HTML_PAGE % subst if ext == ".html" else body % subst)
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':26s} {'ticks':>5s} {'ext-js':>6s}  "
              f"{'expected':34s} markers seen")
        silent = []
        for name in wanted:
            _, ext, expect = VARIANTS[name]
            ticks, requested, markers = _run_variant(
                args.binary, name, ext, http_port, args.seconds)
            print(f"{name:26s} {ticks:5d} {'yes' if requested else 'no':>6s}  "
                  f"{expect:34s} {', '.join(markers) if markers else '— nothing'}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("nothing was ever reported by:", ", ".join(silent))
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
