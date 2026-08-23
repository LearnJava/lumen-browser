#!/usr/bin/env python3
"""WPT-RUN-6 slice 27: engine-driven callbacks, handler IDL attributes,
string-compiled `import()`, import maps and `Link`-header preloads.

The residual of slice 26 is 162 ids with no dominant family, but the largest
*shared question* in it is what a page is allowed to hear back: an exception
thrown where the engine, not the page, is the caller (`requestAnimationFrame`,
`requestIdleCallback`), an event handler installed as an IDL attribute on
something that is not `window` (`document.onresize`,
`document.onreadystatechange`, `meta.onresize`), an `error` event dispatched by
hand, and a module the page asks for through a string, an import map or a
response header. This probe measures each of those against a live browser, on
http, with the probe's own server recording what was actually requested —
`BUG-826` means the browser's own log is not evidence that a request happened:

* `requestAnimationFrame callback exceptions are reported to error handler`
  and its `requestIdleCallback` twin (2 ids) assert `e.error.message`, not just
  that `error` fired. `cbx-report` throws from both callbacks and prints every
  field of the `ErrorEvent` the listener sees, so "no event" and "an event
  with no `.error`" are separable — BUG-591 wired the first and says nothing
  about the second.
* `document.onresize should set the document.onresize handler`,
  `meta.onresize`, and `readystatechange event is fired each time
  document.readyState changes` (2 ids) all install an `on<type>` property on a
  node that is not `window`. `handler-idl` sets six of them and dispatches to
  each.
* `error event is normal (return true does not cancel; one arg) on Window,
  with a synthetic Event` (1 id, and the two `frameset-*` siblings behind
  BUG-480) needs an `error` event dispatched on `document.body` to bubble to
  `window` and to call `body.onerror` with exactly one argument.
* `Resize event not fired at window.visualViewport when content is added`
  (1 id) needs `window.visualViewport` to exist at all.
* `slotchange event: Append a child to a host (onslotchange).` and `inserting
  a document fragment ... should dispatch a slotchange event` (2 ids) —
  `slotchange` after an assignment change, in both listener forms.
* `document.currentScript must not be set to a script element ... in an open
  shadow tree` (1 id) — what `document.currentScript` is during an inline
  script, an external one, and one inside a shadow root.
* `setTimeout should successfully import` and its four siblings (2 ids, the
  most frequent hung-subtest name of the whole residual) — `import()` inside a
  string compiled by `setTimeout`, `eval`, `Function`, a reflected inline
  handler and a UA-triggered one, each resolved against a *different* base URL
  by design.
* `Module map's key is the URL after import map resolution` and the two
  integrity ids (3 ids) — whether `<script type=importmap>` is honoured at all.
* `Makes sure that Link headers on subresources preload resources`,
  its cross-origin and `imagesrcset` siblings, and `Preconnect should not fire
  load (or error) events` (4 ids) — a `Link:` response header on the document
  and on a subresource, measured on the server rather than in the page.
* `The initiator type for for fetch() must be 'fetch'` and `Finite resource
  timing entries buffer size` (2 ids) — the Resource Timing buffer, which
  BUG-839 says is never filled.
* `Test that createImageBitmap from a bitmaprenderer canvas produces correct
  result` and the `OffscreenCanvas`/worker transfer ids (3 ids).
* `event.intercept() should throw if the handler is null` and its four
  siblings (6 ids) — the Navigation API.

Same harness as slices 15/17-22/24/25/26 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died".

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_callback_import_preload_gaps.py
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

#: Paths the probe server was asked for, with the request method. A resource
#: that never appears here was never fetched, whatever the page or the
#: browser's own log says (BUG-826).
SERVED = []
_SERVED_LOCK = threading.Lock()

#: Response headers the probe server adds to specific paths. This is the whole
#: point of the `link-header` variant: a `Link: <...>; rel=preload` header is
#: the only way those four `preload/` ids ask for anything, and the header has
#: to come from the server, not from the markup.
EXTRA_HEADERS = {
    "/.vcip-link-header.html": [("Link", "<vcip-preloaded.js>; rel=preload; as=script")],
    "/vcip-linked.css": [("Link", "<vcip-sub-preloaded.js>; rel=preload; as=script")],
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-27 probe: __NAME__</title>
<body>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("vcip-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf+timeout+fetch-ok+load"),

    # ── an exception thrown where the engine is the caller ─────────────────
    # `animation-frames/callback-exception.html` and its `requestIdleCallback`
    # twin do not assert that `error` fired — they assert
    # `e.error.message === custom_exception`, so an ErrorEvent without an
    # `error` object hangs them exactly as no event at all would. Print every
    # field of what the listener sees.
    "cbx-report": ("""
<script>
window.addEventListener("error", function (e) {
    console.log("PROBE cbx-event type=" + e.type +
                " message=" + JSON.stringify(e.message === undefined ? null : e.message) +
                " error=" + (e.error ? JSON.stringify(e.error.message || "no-message") : "none") +
                " filename=" + JSON.stringify(e.filename === undefined ? null : e.filename) +
                " lineno=" + e.lineno);
});
console.log("PROBE cbx-onerror-settable=" + ("onerror" in window) +
            " ric=" + (typeof window.requestIdleCallback));
requestAnimationFrame(function () { throw new Error("rafBoom"); });
setTimeout(function () {
    if (typeof window.requestIdleCallback === "function") {
        window.requestIdleCallback(function () { throw new Error("ricBoom"); });
        console.log("PROBE cbx-ric-armed");
    }
}, 300);
setTimeout(function () { console.log("PROBE cbx-checked"); }, 3000);
</script>
""", "cbx-event ... error=rafBoom and error=ricBoom"),

    # `event-handler-onresize.html` and `document-readyState.html` install
    # `on<type>` IDL attributes on a *detached* `<body>`, on `document`, on a
    # `<meta>` — and the readyState test on `document.onreadystatechange`.
    # Each is dispatched to by hand except the last, which the engine drives.
    "handler-idl": ("""
<script>
console.log("PROBE hidl-rsc-first=" + document.readyState);
document.onreadystatechange = function () {
    console.log("PROBE hidl-rsc " + document.readyState);
};
console.log("PROBE hidl-rsc-set=" + (typeof document.onreadystatechange));
document.addEventListener("readystatechange", function () {
    console.log("PROBE hidl-rsc-listener " + document.readyState);
});
var body = document.createElement("body");
body.onresize = function (e) {
    console.log("PROBE hidl-body-fired currentTarget-is-window=" + (e.currentTarget === window));
};
console.log("PROBE hidl-body-set=" + (typeof body.onresize));
window.dispatchEvent(new Event("resize"));
document.onresize = function (e) {
    console.log("PROBE hidl-doc-fired currentTarget-is-document=" + (e.currentTarget === document));
};
console.log("PROBE hidl-doc-set=" + (typeof document.onresize));
document.dispatchEvent(new Event("resize"));
var meta = document.createElement("meta");
meta.onresize = function (e) { console.log("PROBE hidl-meta-fired"); };
console.log("PROBE hidl-meta-set=" + (typeof meta.onresize));
meta.dispatchEvent(new Event("resize"));
window.addEventListener("load", function () {
    console.log("PROBE hidl-load rs=" + document.readyState);
});
</script>
""", "hidl-rsc interactive+complete, hidl-doc-fired, hidl-meta-fired, hidl-body-fired"),

    # `body-element-synthetic-event.html`: an `error` Event dispatched on
    # `document.body` must bubble to `window` (the EventWatcher waits there)
    # and must call `body.onerror` with exactly one argument — the "normal"
    # calling convention, as opposed to `window.onerror`'s five.
    "body-error-bubble": ("""
<script>
window.addEventListener("error", function (e) {
    console.log("PROBE bee-window type=" + e.type +
                " defaultPrevented=" + e.defaultPrevented +
                " target-is-body=" + (e.target === document.body));
});
window.addEventListener("load", function () {
    document.body.onerror = function () {
        console.log("PROBE bee-body-onerror args=" + arguments.length);
        return true;
    };
    console.log("PROBE bee-body-set=" + (typeof document.body.onerror));
    document.body.dispatchEvent(new Event("error", {bubbles: true, cancelable: true}));
    console.log("PROBE bee-dispatched");
    var plain = new Event("custom-bubbler", {bubbles: true});
    window.addEventListener("custom-bubbler", function () {
        console.log("PROBE bee-control-bubbled");
    });
    document.body.dispatchEvent(plain);
});
</script>
""", "bee-body-onerror args=1 + bee-window (control: bee-control-bubbled)"),

    # `viewport-no-resize-event-on-overflow-recalc.html` reads
    # `window.visualViewport.addEventListener` on its first line, from inside
    # a rAF callback.
    "visual-viewport": ("""
<script>
console.log("PROBE vv-present=" + (typeof window.visualViewport) +
            " in-window=" + ("visualViewport" in window));
try {
    window.visualViewport.addEventListener("resize", function () {
        console.log("PROBE vv-resize");
    });
    console.log("PROBE vv-listener-attached width=" + window.visualViewport.width +
                " scale=" + window.visualViewport.scale);
} catch (e) { console.log("PROBE vv-throws " + e); }
setTimeout(function () {
    document.body.style.height = "400%";
    console.log("PROBE vv-grown");
}, 300);
</script>
""", "vv-present=object + vv-listener-attached"),

    # `slotchange.html` / `inserting-fragment-under-shadow-host.html`: a slot
    # whose assigned nodes change must fire `slotchange`, in both listener
    # forms, for an appended element and for a DocumentFragment.
    "slotchange": ("""
<div id="host"><div id="c1" slot="s1"></div></div>
<script>
var host = document.getElementById("host");
var root = host.attachShadow({mode: "open"});
root.innerHTML = '<slot id="s" name="s1"></slot>';
var slot = root.getElementById ? root.getElementById("s") : root.querySelector("slot");
console.log("PROBE sc-slot=" + (slot ? slot.localName : "none") +
            " assigned=" + (slot && slot.assignedNodes ? slot.assignedNodes().length : "no-api"));
slot.addEventListener("slotchange", function (e) {
    console.log("PROBE sc-listener assigned=" + slot.assignedNodes().length);
});
slot.onslotchange = function () {
    console.log("PROBE sc-onslotchange assigned=" + slot.assignedNodes().length);
};
setTimeout(function () {
    var d = document.createElement("div");
    d.setAttribute("slot", "s1");
    host.appendChild(d);
    console.log("PROBE sc-appended assigned=" + slot.assignedNodes().length);
}, 300);
setTimeout(function () {
    var frag = document.createDocumentFragment();
    var d2 = document.createElement("div");
    d2.setAttribute("slot", "s1");
    frag.appendChild(d2);
    frag.appendChild(document.createTextNode("text"));
    host.appendChild(frag);
    console.log("PROBE sc-fragment assigned=" + slot.assignedNodes().length);
}, 900);
setTimeout(function () { console.log("PROBE sc-checked"); }, 2500);
</script>
""", "sc-listener + sc-onslotchange after sc-appended and sc-fragment"),

    # `Document-prototype-currentScript.html` asserts what
    # `document.currentScript` is while an external script runs, including one
    # in a shadow tree (must be null) and one that was removed (must be set).
    "currentscript": ("""
<script>
console.log("PROBE cs-inline is-script=" +
            (document.currentScript ? document.currentScript.localName : "null"));
</script>
<div id="shadowhost"></div>
<script src="vcip-currentscript.js"></script>
<script>
window.addEventListener("load", function () {
    var host = document.getElementById("shadowhost");
    var root = host.attachShadow({mode: "open"});
    var s = document.createElement("script");
    s.src = "vcip-currentscript.js?in-shadow";
    root.appendChild(s);
    console.log("PROBE cs-shadow-appended");
    setTimeout(function () { console.log("PROBE cs-checked"); }, 1500);
});
</script>
""", "cs-external is-script=script, then cs-external ... =null for the shadow one"),

    # `string-compilation-base-url-inline-{classic,module}.html` — five
    # evaluators, each compiling a string that calls `import()`. The first is
    # `setTimeout`, which BUG-831 says never compiles a string at all; the
    # test's `promise_test`s run in sequence, so whichever of the five is the
    # first to hang takes the file with it.
    "string-import": ("""
<div id="dummy"></div>
<script>
window.continueTest = function (m) { console.log("PROBE si-resolved " + JSON.stringify(m && m.A ? m.A.from : "no-namespace")); };
window.errorTest = function (e) { console.log("PROBE si-rejected " + e); };
var code = "import('./vcip-imports-a.js').then(window.continueTest, window.errorTest);";
try { setTimeout(code, 0); console.log("PROBE si-setTimeout-string-accepted"); }
catch (e) { console.log("PROBE si-setTimeout-throws " + e); }
setTimeout(function () {
    try { eval(code); console.log("PROBE si-eval-ran"); }
    catch (e) { console.log("PROBE si-eval-throws " + e); }
}, 400);
setTimeout(function () {
    try { Function(code)(); console.log("PROBE si-function-ran"); }
    catch (e) { console.log("PROBE si-function-throws " + e); }
}, 900);
setTimeout(function () {
    var dummy = document.getElementById("dummy");
    dummy.setAttribute("onclick", code);
    try { dummy.onclick(); console.log("PROBE si-reflected-called typeof=" + (typeof dummy.onclick)); }
    catch (e) { console.log("PROBE si-reflected-throws " + e); }
}, 1400);
setTimeout(function () {
    var dummy = document.getElementById("dummy");
    dummy.setAttribute("onclick", code);
    dummy.click();
    console.log("PROBE si-clicked");
}, 1900);
setTimeout(function () { console.log("PROBE si-checked imported=" + (window.vcipImported || 0)); }, 3500);
</script>
""", "si-resolved imports-a.js five times (one per evaluator)"),

    # `dynamic-module-map-key.html` and the two integrity ids need
    # `<script type=importmap>` to be honoured; `no-referencing-script-*`
    # additionally needs a failing integrity check to *reject* the import.
    "importmap": ("""
<script type="importmap">
{ "imports": { "mapped-module": "./vcip-mapped.js" } }
</script>
<script>
console.log("PROBE im-supported=" +
            (HTMLScriptElement.supports ? HTMLScriptElement.supports("importmap") : "no-supports-api"));
import("mapped-module").then(
    function (m) { console.log("PROBE im-bare-resolved " + JSON.stringify(m.NAME || "no-export")); },
    function (e) { console.log("PROBE im-bare-rejected " + e); });
import("./vcip-missing-module.js").then(
    function () { console.log("PROBE im-missing-resolved"); },
    function (e) { console.log("PROBE im-missing-rejected " + e.constructor.name); });
setTimeout(function () { console.log("PROBE im-checked"); }, 3000);
</script>
""", "im-bare-resolved (map honoured) + im-missing-rejected TypeError"),

    # The four `preload/` ids ask for their resource through a `Link:`
    # *response header* — on the document itself and on a subresource — and
    # then look for a Resource Timing entry. The server is the only witness.
    "link-header": ("""
<link rel="stylesheet" href="vcip-linked.css">
<script>
var link = document.createElement("link");
link.rel = "preconnect";
link.href = location.origin;
link.as = "script";
link.addEventListener("load", function () { console.log("PROBE lh-preconnect-load"); });
link.addEventListener("error", function () { console.log("PROBE lh-preconnect-error"); });
document.head.appendChild(link);
var pre = document.createElement("link");
pre.rel = "preload";
pre.as = "script";
pre.href = "vcip-el-preloaded.js";
pre.addEventListener("load", function () { console.log("PROBE lh-preload-load"); });
pre.addEventListener("error", function () { console.log("PROBE lh-preload-error"); });
document.head.appendChild(pre);
window.addEventListener("load", function () {
    var names = performance.getEntriesByType("resource").map(function (e) {
        return e.name.replace(/^https?:\\/\\/[^/]+/, "") + ":" + e.initiatorType;
    });
    console.log("PROBE lh-rt-entries " + JSON.stringify(names));
});
setTimeout(function () { console.log("PROBE lh-checked"); }, 3000);
</script>
""", "server saw vcip-preloaded.js and vcip-sub-preloaded.js; lh-rt-entries non-empty"),

    # `resource-timing/initiator-type/misc.html` and `buffer-full-eventually`
    # read the buffer back after issuing the requests themselves.
    "resource-timing": ("""
<img src="vcip-pixel.png" alt="">
<script>
fetch("vcip-asset.js").then(function () { console.log("PROBE rt-fetch-done"); });
var xhr = new XMLHttpRequest();
xhr.open("GET", "vcip-asset.js?xhr");
xhr.onload = function () { console.log("PROBE rt-xhr-done"); };
xhr.send();
try { new EventSource("vcip-sse.py"); console.log("PROBE rt-es-created"); }
catch (e) { console.log("PROBE rt-es-throws " + e); }
console.log("PROBE rt-api" +
            " getEntriesByType=" + (typeof performance.getEntriesByType) +
            " setResourceTimingBufferSize=" + (typeof performance.setResourceTimingBufferSize) +
            " clearResourceTimings=" + (typeof performance.clearResourceTimings) +
            " onresourcetimingbufferfull=" + ("onresourcetimingbufferfull" in performance));
window.addEventListener("load", function () {
    setTimeout(function () {
        var entries = performance.getEntriesByType("resource");
        console.log("PROBE rt-entries n=" + entries.length + " " +
                    JSON.stringify(entries.slice(0, 6).map(function (e) {
                        return e.initiatorType + ":" + e.name.split("/").pop();
                    })));
    }, 800);
});
</script>
""", "rt-entries n>=3 with initiatorType img/fetch/xmlhttprequest"),

    # `bitmaprenderer-as-imagesource.html`, `createImageBitmap-in-worker-transfer`
    # and the two OffscreenCanvas worker ids.
    "imagebitmap": ("""
<canvas id="c" width="16" height="16"></canvas>
<script>
var c = document.getElementById("c");
console.log("PROBE ib-api" +
            " createImageBitmap=" + (typeof window.createImageBitmap) +
            " ImageBitmap=" + (typeof window.ImageBitmap) +
            " OffscreenCanvas=" + (typeof window.OffscreenCanvas) +
            " transferControlToOffscreen=" + (typeof c.transferControlToOffscreen));
try { console.log("PROBE ib-bitmaprenderer=" + (c.getContext("bitmaprenderer") ? "ok" : "null")); }
catch (e) { console.log("PROBE ib-bitmaprenderer-throws " + e); }
try {
    createImageBitmap(c).then(function (b) {
        console.log("PROBE ib-created " + b.width + "x" + b.height);
    }, function (e) { console.log("PROBE ib-rejected " + e); });
} catch (e) { console.log("PROBE ib-throws " + e); }
setTimeout(function () { console.log("PROBE ib-checked"); }, 2500);
</script>
""", "ib-api all present + ib-created 16x16"),

    # The six `navigation-api/` ids all start from `navigation.navigate(...)`
    # or from a `navigate` event, and four of them from `event.intercept()`.
    "navigation-api": ("""
<script>
console.log("PROBE na-present=" + (typeof window.navigation) +
            " in-window=" + ("navigation" in window));
try {
    console.log("PROBE na-shape" +
                " currentEntry=" + (navigation.currentEntry ? "object" : String(navigation.currentEntry)) +
                " entries=" + (typeof navigation.entries) +
                " navigate=" + (typeof navigation.navigate) +
                " onnavigate=" + ("onnavigate" in navigation) +
                " oncurrententrychange=" + ("oncurrententrychange" in navigation));
    navigation.addEventListener("navigate", function (e) {
        console.log("PROBE na-navigate-event intercept=" + (typeof e.intercept) +
                    " canIntercept=" + e.canIntercept);
        try { e.intercept({handler: function () { return Promise.resolve(); }}); console.log("PROBE na-intercepted"); }
        catch (err) { console.log("PROBE na-intercept-throws " + err); }
    });
    navigation.addEventListener("currententrychange", function (e) {
        console.log("PROBE na-currententrychange from=" + (e.from ? "entry" : String(e.from)) +
                    " navigationType=" + e.navigationType);
    });
    var r = navigation.navigate("#slice27");
    console.log("PROBE na-navigate-called returns=" + (r && r.committed ? "result" : String(r)));
} catch (e) { console.log("PROBE na-throws " + e); }
setTimeout(function () { console.log("PROBE na-checked hash=" + location.hash); }, 2500);
</script>
""", "na-present=object + na-navigate-event + na-intercepted"),

    # ── follow-ups that the first pass could not separate ──────────────────
    # `cbx-report` showed the rAF exception reported and the rIdle one not.
    # That has two possible causes — the callback never ran, or it ran and its
    # exception went nowhere — and only one of them is BUG-591's shape.
    "cbx-ric": ("""
<script>
window.addEventListener("error", function (e) {
    console.log("PROBE ric-error " + (e.error ? e.error.message : "no-error-object"));
});
window.requestIdleCallback(function (deadline) {
    console.log("PROBE ric-ran deadline=" + (deadline ? typeof deadline.timeRemaining : "none"));
    throw new Error("ricBoom");
});
window.requestIdleCallback(function () { console.log("PROBE ric-second-ran"); });
queueMicrotask(function () { throw new Error("microBoom"); });
setTimeout(function () { console.log("PROBE ric-checked"); }, 3000);
</script>
""", "ric-ran then ric-error ricBoom (and ric-error microBoom)"),

    # `body-error-bubble` lost both its `error` event *and* its plain control
    # at the same step, which points at bubbling rather than at anything
    # error-specific. Dispatch from three depths and listen in four places.
    "bubble-to-window": ("""
<div id="outer"><span id="inner">x</span></div>
<script>
window.addEventListener("load", function () {
    var inner = document.getElementById("inner");
    ["window", "document", "body", "outer"].forEach(function (where) {
        var target = where === "window" ? window
                   : where === "document" ? document
                   : where === "body" ? document.body
                   : document.getElementById("outer");
        target.addEventListener("btw-probe", function (e) {
            console.log("PROBE btw-heard-at-" + where + " target=" +
                        (e.target === inner ? "inner" : e.target === document.body ? "body" :
                         e.target === document ? "document" : String(e.target)) +
                        " phase=" + e.eventPhase);
        });
    });
    inner.dispatchEvent(new Event("btw-probe", {bubbles: true}));
    console.log("PROBE btw-from-inner");
    document.body.dispatchEvent(new Event("btw-probe", {bubbles: true}));
    console.log("PROBE btw-from-body");
    document.dispatchEvent(new Event("btw-probe", {bubbles: true}));
    console.log("PROBE btw-from-document");
    inner.addEventListener("btw-path", function (e) {
        console.log("PROBE btw-composedPath n=" +
                    (e.composedPath ? e.composedPath().length : "no-api"));
    });
    inner.dispatchEvent(new Event("btw-path", {bubbles: true}));
});
</script>
""", "btw-heard-at-window three times (from inner, body and document)"),

    # `slotchange` died on its first line: the shadow root built by
    # `innerHTML` had no `<slot>` in it at all. Separate "innerHTML does not
    # parse into a shadow root" from "there is no slot element" from
    # "assignment does not happen".
    "slot-detail": ("""
<div id="host"><div id="c1" slot="s1">light</div></div>
<script>
var host = document.getElementById("host");
var root = host.attachShadow({mode: "open"});
console.log("PROBE sd-root=" + (root ? root.constructor.name : "none") +
            " mode=" + root.mode + " host-shadowRoot=" + (host.shadowRoot === root));
root.innerHTML = '<slot id="s" name="s1"></slot>';
console.log("PROBE sd-after-innerHTML children=" + root.childNodes.length +
            " html=" + JSON.stringify(root.innerHTML) +
            " querySelector=" + (root.querySelector("slot") ? "found" : "null"));
var slot = document.createElement("slot");
slot.name = "s1";
root.appendChild(slot);
console.log("PROBE sd-created localName=" + slot.localName +
            " ctor=" + slot.constructor.name +
            " assignedNodes=" + (typeof slot.assignedNodes) +
            " assignedElements=" + (typeof slot.assignedElements));
try {
    console.log("PROBE sd-assigned n=" + slot.assignedNodes().length);
} catch (e) { console.log("PROBE sd-assigned-throws " + e); }
slot.addEventListener("slotchange", function () {
    console.log("PROBE sd-slotchange-listener");
});
slot.onslotchange = function () { console.log("PROBE sd-slotchange-prop"); };
setTimeout(function () {
    var d = document.createElement("div");
    d.setAttribute("slot", "s1");
    host.appendChild(d);
    console.log("PROBE sd-appended");
}, 400);
setTimeout(function () { console.log("PROBE sd-checked"); }, 2500);
</script>
""", "sd-after-innerHTML querySelector=found + sd-slotchange-listener"),

    # `string-import` resolved *once* and the module map hides which evaluator
    # did it (one URL, one evaluation). Give each evaluator its own query, so
    # the probe's server records exactly which of the five ran.
    "string-import-labels": ("""
<div id="dummy"></div>
<script>
window.continueTest = function (m) { console.log("PROBE sil-resolved " + (m && m.A ? m.A.from : "?")); };
window.errorTest = function (e) { console.log("PROBE sil-rejected " + e); };
function code(label) {
    return "import('./vcip-imports-a.js?label=" + label +
           "').then(window.continueTest, window.errorTest);";
}
setTimeout(code("setTimeout"), 0);
setTimeout(function () { eval(code("eval")); }, 400);
setTimeout(function () { Function(code("Function"))(); }, 800);
setTimeout(function () {
    var d = document.getElementById("dummy");
    d.setAttribute("onclick", code("reflected"));
    d.onclick();
}, 1200);
setTimeout(function () {
    var d = document.getElementById("dummy");
    d.setAttribute("onclick", code("clicked"));
    d.click();
}, 1600);
setTimeout(function () { console.log("PROBE sil-checked"); }, 3500);
</script>
""", "server saw ?label= for all five evaluators"),

    # `importmap` applied the map and then failed to fetch what it mapped to,
    # which is a URL-resolution question, not an import-map one. Ask the same
    # question with an absolute target.
    "importmap-absolute": ("""
<script type="importmap">
{ "imports": { "abs-module": "__ORIGIN__/vcip-mapped.js",
               "rel-module": "./vcip-mapped.js" } }
</script>
<script>
import("abs-module").then(
    function (m) { console.log("PROBE ima-abs-resolved " + m.NAME); },
    function (e) { console.log("PROBE ima-abs-rejected " + e); });
setTimeout(function () {
    import("rel-module").then(
        function (m) { console.log("PROBE ima-rel-resolved " + m.NAME); },
        function (e) { console.log("PROBE ima-rel-rejected " + e); });
}, 400);
setTimeout(function () {
    import("./vcip-mapped.js").then(
        function (m) { console.log("PROBE ima-plain-resolved " + m.NAME); },
        function (e) { console.log("PROBE ima-plain-rejected " + e); });
}, 800);
setTimeout(function () { console.log("PROBE ima-checked"); }, 3000);
</script>
""", "ima-abs-resolved + ima-rel-resolved + ima-plain-resolved"),

    # `bubble-to-window` said an event dispatched on a nested element reaches
    # nobody at all, which is a bigger claim than the one the residual needs.
    # Pin it down: same event on the same element with a listener at every
    # level, an `on<type>` handler next to each listener, and a real click as
    # the control (the shell drives that one through a different path).
    "bubble-detail": ("""
<div id="outer"><span id="inner">x</span></div>
<script>
window.addEventListener("load", function () {
    var inner = document.getElementById("inner");
    var outer = document.getElementById("outer");
    inner.addEventListener("bd-evt", function (e) { console.log("PROBE bd-at-inner target=" + (e.target === inner)); });
    outer.addEventListener("bd-evt", function () { console.log("PROBE bd-at-outer"); });
    document.body.addEventListener("bd-evt", function () { console.log("PROBE bd-at-body"); });
    document.addEventListener("bd-evt", function () { console.log("PROBE bd-at-document"); });
    window.addEventListener("bd-evt", function () { console.log("PROBE bd-at-window"); });
    outer.addEventListener("bd-evt", function () { console.log("PROBE bd-at-outer-capture"); }, true);
    console.log("PROBE bd-dispatch-returned=" +
                inner.dispatchEvent(new Event("bd-evt", {bubbles: true, cancelable: true})));
    outer.addEventListener("click", function () { console.log("PROBE bd-click-at-outer"); });
    document.addEventListener("click", function () { console.log("PROBE bd-click-at-document"); });
    window.addEventListener("click", function () { console.log("PROBE bd-click-at-window"); });
    inner.click();
    console.log("PROBE bd-clicked");
    var custom = new CustomEvent("bd-custom", {bubbles: true});
    outer.addEventListener("bd-custom", function () { console.log("PROBE bd-custom-at-outer"); });
    inner.dispatchEvent(custom);
});
</script>
""", "bd-at-inner/outer/body/document/window for one dispatch on inner"),

    # `slot-detail` stopped at `root.innerHTML = ...` with no error line, so
    # the step that threw is unknown. Wrap each one and print what the object
    # actually is (BUG-676 says it is a plain literal).
    "slot-detail2": ("""
<div id="host"><div id="c1" slot="s1">light</div></div>
<script>
var host = document.getElementById("host");
var root = host.attachShadow({mode: "open"});
console.log("PROBE sd2-keys " + JSON.stringify(Object.keys(root).slice(0, 24)));
console.log("PROBE sd2-shadowRoot host.shadowRoot=" + (host.shadowRoot ? "object" : String(host.shadowRoot)) +
            " same=" + (host.shadowRoot === root) +
            " stable=" + (host.shadowRoot === host.shadowRoot));
try { root.innerHTML = '<slot name="s1"></slot>'; console.log("PROBE sd2-innerHTML-set"); }
catch (e) { console.log("PROBE sd2-innerHTML-throws " + e); }
try { console.log("PROBE sd2-innerHTML-read " + JSON.stringify(root.innerHTML)); }
catch (e) { console.log("PROBE sd2-innerHTML-read-throws " + e); }
try { console.log("PROBE sd2-children " + (root.childNodes ? root.childNodes.length : "no-childNodes")); }
catch (e) { console.log("PROBE sd2-children-throws " + e); }
try { console.log("PROBE sd2-query " + (root.querySelector("slot") ? "found" : "null")); }
catch (e) { console.log("PROBE sd2-query-throws " + e); }
try {
    var s = document.createElement("slot");
    s.setAttribute("name", "s1");
    root.appendChild(s);
    console.log("PROBE sd2-appended ctor=" + s.constructor.name +
                " assignedNodes=" + (typeof s.assignedNodes));
    console.log("PROBE sd2-assigned n=" + s.assignedNodes().length);
    s.addEventListener("slotchange", function () { console.log("PROBE sd2-slotchange"); });
    setTimeout(function () {
        var d = document.createElement("div");
        d.setAttribute("slot", "s1");
        host.appendChild(d);
        console.log("PROBE sd2-host-appended n=" + s.assignedNodes().length);
    }, 400);
} catch (e) { console.log("PROBE sd2-slot-throws " + e); }
setTimeout(function () { console.log("PROBE sd2-checked"); }, 2500);
</script>
""", "sd2-innerHTML-set + sd2-query found + sd2-slotchange after sd2-host-appended"),

    # The five `navigate-event` ids do not call `navigation.navigate()` at
    # all: they assign `navigation.onnavigate` (an `on<type>` IDL property,
    # which `handler-idl` says the engine may not have) and then trigger the
    # navigation with `location.href = "#1"`.
    "navigation-onprops": ("""
<script>
["onnavigate", "onnavigatesuccess", "onnavigateerror", "oncurrententrychange"].forEach(function (p) {
    console.log("PROBE nop-" + p + "-in=" + (p in navigation));
});
navigation.onnavigate = function (e) {
    console.log("PROBE nop-onnavigate-fired canIntercept=" + e.canIntercept +
                " hashChange=" + e.hashChange + " type=" + e.navigationType);
};
navigation.oncurrententrychange = function (e) { console.log("PROBE nop-oncurrententrychange-fired"); };
navigation.addEventListener("navigate", function (e) { console.log("PROBE nop-listener-fired"); });
console.log("PROBE nop-assigned onnavigate=" + (typeof navigation.onnavigate));
setTimeout(function () {
    location.href = "#1";
    console.log("PROBE nop-hash-assigned hash=" + location.hash);
}, 300);
setTimeout(function () {
    console.log("PROBE nop-checked hash=" + location.hash +
                " entries=" + (navigation.entries ? navigation.entries().length : "no-api") +
                " currentEntry=" + (navigation.currentEntry ? "entry" : String(navigation.currentEntry)));
}, 2500);
</script>
""", "nop-onnavigate-fired + nop-oncurrententrychange-fired after the hash assignment"),

    # The two `no-referencing-script-integrity*` ids hang inside an `onload=`
    # *attribute* on a parser-written `<img>` — BUG-630/BUG-804 measured the
    # script-assigned form, not the attribute one, and the difference decides
    # whether those ids belong to that bug or to the import map.
    "img-onload-attr": ("""
<img id="parser-img" src="vcip-pixel.png" alt=""
     onload="console.log('PROBE ioa-parser-attr-fired')"
     onerror="console.log('PROBE ioa-parser-attr-error')">
<script>
var parserImg = document.getElementById("parser-img");
console.log("PROBE ioa-parser-onload=" + (typeof parserImg.onload) +
            " complete=" + parserImg.complete);
parserImg.addEventListener("load", function () { console.log("PROBE ioa-parser-listener-fired"); });
window.addEventListener("load", function () {
    var made = document.createElement("img");
    made.setAttribute("onload", "console.log('PROBE ioa-script-attr-fired')");
    made.onerror = function () { console.log("PROBE ioa-script-prop-error"); };
    made.addEventListener("load", function () { console.log("PROBE ioa-script-listener-fired"); });
    made.src = "vcip-pixel.png?script-made";
    document.body.appendChild(made);
    console.log("PROBE ioa-script-appended");
});
setTimeout(function () { console.log("PROBE ioa-checked"); }, 3000);
</script>
""", "ioa-parser-attr-fired and ioa-script-attr-fired"),

    # `import-maps/dynamic-module-map-key.html` hangs on a subtest that has
    # nothing to do with import maps: a `<script>` first connected as an
    # *empty* `type=importmap`, then removed, retyped `text/javascript`, given
    # a body and re-appended, must run (its "already started" flag is false
    # precisely because it was empty). The same element with a non-empty
    # importmap body must NOT run.
    "script-reinsert": ("""
<script>
window.addEventListener("load", function () {
    var empty = document.createElement("script");
    empty.type = "importmap";
    document.head.appendChild(empty);
    document.head.removeChild(empty);
    empty.type = "text/javascript";
    empty.innerText = "console.log('PROBE sr-empty-retyped-ran')";
    document.head.appendChild(empty);
    console.log("PROBE sr-empty-reappended type=" + empty.type);

    var nonEmpty = document.createElement("script");
    nonEmpty.type = "importmap";
    nonEmpty.innerText = '{ "imports": {} }';
    document.head.appendChild(nonEmpty);
    document.head.removeChild(nonEmpty);
    nonEmpty.type = "text/javascript";
    nonEmpty.innerText = "console.log('PROBE sr-nonempty-retyped-ran')";
    document.head.appendChild(nonEmpty);
    console.log("PROBE sr-nonempty-reappended");

    var plain = document.createElement("script");
    plain.innerText = "console.log('PROBE sr-plain-ran')";
    document.head.appendChild(plain);
    console.log("PROBE sr-plain-appended");
});
setTimeout(function () { console.log("PROBE sr-checked"); }, 2500);
</script>
""", "sr-plain-ran + sr-empty-retyped-ran, and NO sr-nonempty-retyped-ran"),
}


#: Files the probe pages fetch. Ordinary files under `tests/wpt/`, written
#: before the run and removed after it, so they come from the same http origin
#: as the page.
ASSETS = {
    "vcip-asset.js": "window.vcipAsset = (window.vcipAsset || 0) + 1;\n",

    # Reports what `document.currentScript` is while an *external* classic
    # script runs — the question `Document-prototype-currentScript.html` asks
    # for a script in a shadow tree (must be null) and one that was removed
    # (must still be the element).
    "vcip-currentscript.js": """
var cs = document.currentScript;
console.log("PROBE cs-external is=" + (cs ? cs.localName : "null") +
            " src=" + (cs ? String(cs.src).split("/").pop() : "-") +
            " root=" + (cs && cs.getRootNode ?
                        (cs.getRootNode() === document ? "document" : "shadow") : "no-getRootNode"));
""",

    # The module `string-compilation-base-url-*` imports. The tests assert
    # both that it evaluated and that its namespace object is right.
    "vcip-imports-a.js": """
globalThis.vcipImported = (globalThis.vcipImported || 0) + 1;
export const A = {from: "imports-a.js"};
""",

    "vcip-mapped.js": 'export const NAME = "mapped-module";\n',

    # Targets of the two `Link:` response headers and of the `<link
    # rel=preload>` element control. None of them is referenced from markup:
    # if the server is asked for one, the hint was acted on.
    "vcip-preloaded.js": "window.vcipPreloadedDoc = 1;\n",
    "vcip-sub-preloaded.js": "window.vcipPreloadedSub = 1;\n",
    "vcip-el-preloaded.js": "window.vcipPreloadedEl = 1;\n",

    "vcip-linked.css": "#dummy { color: rgb(1, 2, 3); }\n",
}

#: A 1x1 png, so the `resource-timing` variant has a real image request.
_PIXEL_PNG = bytes([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
    0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
])

_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages and their assets, recording every request and
    adding the `Link:` headers the `link-header` variant is about."""

    protocol_version = "HTTP/1.1"

    def _record(self, method):
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.path}")

    def end_headers(self):  # noqa: N802 — http.server's own casing
        path = self.path.split("?", 1)[0]
        for name, value in EXTRA_HEADERS.get(path, ()):
            self.send_header(name, value)
        super().end_headers()

    def do_GET(self):  # noqa: N802 — http.server's own casing
        self._record("GET")
        super().do_GET()

    def do_POST(self):  # noqa: N802 — http.server's own casing
        self._record("POST")
        length = int(self.headers.get("content-length") or 0)
        if length:
            self.rfile.read(length)
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *args):
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
    """Launch one browser on one probe page; return (ticks, markers, served)."""
    log_path = os.path.join(REPO, ".tmp", f"vcip-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    page = f".vcip-{name}.html"
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
    with _SERVED_LOCK:
        served = [p for p in SERVED if "/.vcip-" not in p]
    return ticks, markers, served


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

    http_port, shutdown = _serve(HERE)
    origin = f"http://127.0.0.1:{http_port}"
    written = []
    for name in wanted:
        path = os.path.join(HERE, f".vcip-{name}.html")
        body = VARIANTS[name][0].replace("__ORIGIN__", origin)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE.replace("__NAME__", name).replace("__BODY__", body))
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)
    pixel = os.path.join(HERE, "vcip-pixel.png")
    with open(pixel, "wb") as handle:
        handle.write(_PIXEL_PNG)
    written.append(pixel)

    try:
        print(f"{'variant':16s} {'ticks':>5s}  {'expected':64s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, served = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if served:
                seen += "   [server saw: " + ", ".join(sorted(set(served))) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:16s} {ticks:5d}  {VARIANTS[name][1]:64s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half — a resource missing there was never fetched, "
              "whatever the page or the browser log says (BUG-826).")
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
