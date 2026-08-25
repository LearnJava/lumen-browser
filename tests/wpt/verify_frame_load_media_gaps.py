#!/usr/bin/env python3
"""WPT-RUN-6 slice 24: what the residual's own partial harness report points at.

Slices 11-23 read the residual TIMEOUTs off the browser's stderr, off a
standalone `--dump-layout` run, or off a grep of the test source. This slice
adds the one piece of evidence nobody had used yet: **the harness's own partial
report**. `wptrunner` writes the subtests it managed to collect before the
timeout into the shard's `wptreport` json, so for 203 of the 281 residual ids
the run itself already names the exact `async_test` that never called `done()`
— and the subtests that PASSed next to it say the page was alive.

That report turns a category into a mechanism. The densest shapes it exposes,
and what each is measured with here:

* `load nested browsing context <frame src>` / `<iframe src>` / `<object data>`
  / `<embed src>` — 20 subtests over 5 ids of
  `html/infrastructure/urls/resolving-urls/query-encoding`, 24 more in
  `html/browsers/windows/nested-browsing-contexts/name-attribute.window.html`,
  plus `the-object-element/object-handler.html`. All four elements are created
  from script, given a URL and waited on for `load` (`nbc-*` variants). The
  probe's server records what was asked for, so "the element fired nothing" is
  separated from "nothing was ever requested" — the lesson of slice 20
  (BUG-826: the shell logs a preload it never performs).
* `CSS Test: Shape from image …` — 9 ids of `css/css-shapes/spec-examples`,
  every one of them NOTRUN with the page alive. Those tests do their work from
  `<body onload>`; the variants `load-*` ask whether the page's own load event
  fires at all, with and without an `<img>` on the page, from the attribute and
  from `addEventListener`.
* `contain-intrinsic-size: auto` (4) and
  `ContentVisibilityAutoStateChange fires…` (3) wait for a `ResizeObserver`
  callback and for `contentvisibilityautostatechange` (`ro-*`, `cv-*`).
* `Adding open to 'details' should fire a toggle event` (9 subtests) —
  `details-toggle`.
* `Geolocation element display style validation` etc. — 9 ids of
  `html/semantics/permission-element` wait for `onvalidationstatuschange` on
  `<usermedia>`/`<geolocation>`/`<install>`, an incubating element
  (`permission-element`). Measured to be classified, not to be filed.
* media resource selection (`resource-selection-candidate-*`,
  `load-removes-queued-error-event`, `playbackRate`, `event_volumechange`,
  `currentSrc`) — 8 ids; `media-*` re-measures BUG-825/BUG-799 from the
  `<source>` side, where the *candidate* is what the test manipulates.
* `Script src with an empty URL` (2) and `Mutating src attribute…` —
  `script-empty-src`.

Same harness as slices 15/17-22 and for the reasons recorded in `CLAUDE.md`:
one browser process per page, served over http (never `file://`), evidence read
off the browser's own stderr rather than through an MCP `eval`, and a 500 ms
`setInterval` tick so "the page is alive and heard nothing" is separable from
"the page died".

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_frame_load_media_gaps.py
        [--binary target/dev-release/lumen] [--seconds 6] [--variant NAME]

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

#: Paths the probe server was asked for, per variant. A request that never
#: arrives here is evidence that does not depend on the page being able to
#: report anything.
SERVED = []
_SERVED_LOCK = threading.Lock()

#: 1x1 transparent GIF — enough for `<img>`; the point is the load event.
PIXEL_GIF = bytes([
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
    0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
])

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-24 probe: __NAME__</title>
<body__BODYATTR__>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: `body` is spliced into the page above; `expect` is what a spec-compliant
#: engine prints, kept next to the measurement so a change in either direction
#: is visible. The optional third element is an attribute list for `<body>`
#: (the `onload=` content attribute cannot be added any other way).
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("vflm-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
</script>
""", "raf+timeout+fetch-ok"),

    # ── nested browsing contexts ───────────────────────────────────────────
    # `query-encoding/resources/resolve-url.js` (`?include=nested-browsing`):
    # create the element, set the URL, name it, append it, wait for `load`,
    # then read `window[name].document.documentElement.textContent`.
    "nbc-iframe": ("""
<script>
var el = document.createElement("iframe");
el.setAttribute("src", "vflm-child.html?iframe=1");
el.name = "nbc_iframe";
el.onload = function () {
    var w = window["nbc_iframe"];
    console.log("PROBE nbc-iframe-load named=" + (typeof w) +
                " doc=" + (el.contentDocument ? "yes" : "no"));
};
el.onerror = function () { console.log("PROBE nbc-iframe-error"); };
document.body.appendChild(el);
setTimeout(function () {
    console.log("PROBE nbc-iframe-checked named=" + (typeof window["nbc_iframe"]) +
                " contentWindow=" + (typeof el.contentWindow) +
                " contentDocument=" + (typeof el.contentDocument));
}, 1500);
</script>
""", "nbc-iframe-load named=object doc=yes"),
    "nbc-frame": ("""
<script>
var el = document.createElement("frame");
el.setAttribute("src", "vflm-child.html?frame=1");
el.name = "nbc_frame";
el.onload = function () { console.log("PROBE nbc-frame-load"); };
document.body.appendChild(el);
setTimeout(function () {
    console.log("PROBE nbc-frame-checked named=" + (typeof window["nbc_frame"]) +
                " tag=" + el.tagName + " ctor=" + el.constructor.name +
                " frameset-ctor=" + document.createElement("frameset").constructor.name +
                " window.frames=" + (window.frames ? window.frames.length : "absent") +
                " frameElement=" + typeof window.frameElement);
}, 1500);
</script>
""", "server sees vflm-child.html?frame=1 + nbc-frame-load"),
    "nbc-object": ("""
<script>
var el = document.createElement("object");
el.setAttribute("data", "vflm-child.html?object=1");
el.name = "nbc_object";
el.onload = function () { console.log("PROBE nbc-object-load"); };
el.onerror = function () { console.log("PROBE nbc-object-error"); };
document.body.appendChild(el);
setTimeout(function () {
    console.log("PROBE nbc-object-checked named=" + (typeof window["nbc_object"]) +
                " ctor=" + el.constructor.name +
                " contentDocument=" + (typeof el.contentDocument));
}, 1500);
</script>
""", "server sees vflm-child.html?object=1 + nbc-object-load"),
    "nbc-embed": ("""
<script>
var el = document.createElement("embed");
el.setAttribute("src", "vflm-child.html?embed=1");
el.name = "nbc_embed";
el.onload = function () { console.log("PROBE nbc-embed-load"); };
el.onerror = function () { console.log("PROBE nbc-embed-error"); };
document.body.appendChild(el);
setTimeout(function () {
    console.log("PROBE nbc-embed-checked named=" + (typeof window["nbc_embed"]) +
                " ctor=" + el.constructor.name);
}, 1500);
</script>
""", "server sees vflm-child.html?embed=1 + nbc-embed-load"),
    # The parser-written form of the same four, since BUG-804 showed the
    # parser path and the script path are different mechanisms entirely.
    "nbc-parser": ("""
<iframe src="vflm-child.html?p-iframe=1" name="p_iframe"></iframe>
<object data="vflm-child.html?p-object=1" name="p_object"></object>
<embed src="vflm-child.html?p-embed=1" name="p_embed">
<script>
["p_iframe", "p_object", "p_embed"].forEach(function (n) {
    var el = document.getElementsByName(n)[0];
    if (!el) { console.log("PROBE nbc-parser-missing " + n); return; }
    el.addEventListener("load", function () { console.log("PROBE nbc-parser-load " + n); });
});
setTimeout(function () {
    console.log("PROBE nbc-parser-checked iframe=" + (typeof window["p_iframe"]) +
                " object=" + (typeof window["p_object"]) +
                " embed=" + (typeof window["p_embed"]));
}, 1500);
</script>
""", "3 x nbc-parser-load + server sees all three children"),

    # ── the page's own load event ──────────────────────────────────────────
    # `css/css-shapes/spec-examples/shape-outside-0**.html` do all their work
    # from `<body onload>`, with an `<img>` the shape is read from. Both
    # halves are separated here: the attribute vs addEventListener, and a page
    # with an image vs one without.
    "load-body-onload-img": ("""
<img src="vflm-pixel.gif?body-onload=1" alt="">
<script>
window.addEventListener("load", function () { console.log("PROBE load-listener"); });
document.addEventListener("DOMContentLoaded", function () { console.log("PROBE dcl"); });
function vflmBodyOnload() { console.log("PROBE load-body-attr readyState=" + document.readyState); }
setTimeout(function () {
    console.log("PROBE load-checked readyState=" + document.readyState +
                " window.onload=" + typeof window.onload +
                " body.onload=" + typeof document.body.onload);
}, 1500);
</script>
""", "load-body-attr + load-listener + dcl",
     ' onload="vflmBodyOnload()"'),
    # The same attribute without an image on the page, to separate "the load
    # event never fires" from "the attribute is never wired to it".
    "load-body-onload-noimg": ("""
<script>
window.addEventListener("load", function () { console.log("PROBE load-listener"); });
function vflmBodyOnload() { console.log("PROBE load-body-attr readyState=" + document.readyState); }
setTimeout(function () {
    console.log("PROBE load-checked window.onload=" + typeof window.onload +
                " body.onload=" + typeof document.body.onload +
                " attr=" + JSON.stringify(document.body.getAttribute("onload")));
}, 1500);
</script>
""", "load-body-attr + load-listener",
     ' onload="vflmBodyOnload()"'),
    # `<body onerror>`/`<body onresize>` and friends are the same mechanism;
    # `html/webappapis/scripting/events/event-handler-onresize.html` reads
    # `document.onresize` after setting the body attribute.
    "body-event-attrs": ("""
<script>
setTimeout(function () {
    var b = document.body;
    console.log("PROBE body-attrs onload=" + typeof b.onload +
                " win.onload=" + typeof window.onload +
                " win.onresize=" + typeof window.onresize +
                " doc.onresize=" + typeof document.onresize +
                " div.onclick=" + typeof document.getElementById("d").onclick);
    var fired = 0;
    document.getElementById("d").click();
    setTimeout(function () { console.log("PROBE body-attrs-clicked"); }, 100);
}, 600);
</script>
<div id="d" onclick="console.log('PROBE inline-onclick-fired')">x</div>
""", "div.onclick=function + inline-onclick-fired",
     ' onload="console.log(\'PROBE body-attr-inline\')" onresize="0"'),
    "load-window-img": ("""
<img src="vflm-pixel.gif?window-load=1" alt="">
<script>
var img = document.getElementsByTagName("img")[0];
img.addEventListener("load", function () { console.log("PROBE img-load"); });
window.addEventListener("load", function () { console.log("PROBE load-listener readyState=" + document.readyState); });
document.addEventListener("readystatechange", function () {
    console.log("PROBE readystatechange " + document.readyState);
});
setTimeout(function () { console.log("PROBE load-checked readyState=" + document.readyState); }, 1500);
</script>
""", "readystatechange interactive+complete, load-listener"),
    "load-window-noimg": ("""
<script>
window.addEventListener("load", function () { console.log("PROBE load-listener readyState=" + document.readyState); });
document.addEventListener("readystatechange", function () {
    console.log("PROBE readystatechange " + document.readyState);
});
setTimeout(function () { console.log("PROBE load-checked readyState=" + document.readyState); }, 1500);
</script>
""", "readystatechange interactive+complete, load-listener"),

    # ── ResizeObserver / content-visibility ────────────────────────────────
    # `css/css-sizing/contain-intrinsic-size/auto-00*.html`: observe an
    # element, then read `offsetHeight` from inside the callback.
    "ro-basic": ("""
<div id="target" style="width:100px;height:50px;background:#0f0"></div>
<script>
var target = document.getElementById("target");
var ro = new ResizeObserver(function (entries) {
    console.log("PROBE ro-callback n=" + entries.length +
                " h=" + (entries[0] && entries[0].contentRect && entries[0].contentRect.height));
});
ro.observe(target);
target.offsetWidth;
setTimeout(function () { target.style.height = "120px"; console.log("PROBE ro-resized"); }, 400);
setTimeout(function () { console.log("PROBE ro-checked h=" + target.offsetHeight); }, 1500);
</script>
""", "ro-callback (initial) then ro-callback after ro-resized"),
    "cv-auto-state": ("""
<div style="height:2000px"></div>
<div id="target" style="contain-intrinsic-size: auto 1px; content-visibility: auto">
  <div style="height:50px;background:#0f0"></div>
</div>
<script>
var target = document.getElementById("target");
console.log("PROBE cv-support onevent=" + ("oncontentvisibilityautostatechange" in target));
target.addEventListener("contentvisibilityautostatechange", function (e) {
    console.log("PROBE cv-statechange skipped=" + e.skipped);
});
setTimeout(function () {
    var cs = getComputedStyle(target);
    console.log("PROBE cv-checked offsetHeight=" + target.offsetHeight +
                " prop-cv=" + JSON.stringify(cs.getPropertyValue("content-visibility")) +
                " prop-cis=" + JSON.stringify(cs.getPropertyValue("contain-intrinsic-size")) +
                " camel-cv=" + JSON.stringify(cs.contentVisibility) +
                " inline=" + JSON.stringify(target.style.contentVisibility));
}, 1200);
</script>
""", "cv-support onevent=true + cv-statechange + non-empty computed values"),

    # BUG-852, added by P1 2026-08-25 before the fix. `cv-auto-state` above
    # measures one element that is skipped from the start; the WPT test the bug
    # names (`content-visibility-auto-state-changed-first-observation.html`)
    # measures the *first* observation of an element that has just gained
    # `content-visibility: auto` — and asserts it fires exactly once, in BOTH
    # directions: `skipped=false` for an element in the viewport,
    # `skipped=true` for one below it. Both elements are empty, which is the
    # part no reading of the shell's diff would have flagged: `collect_cv_skipped`
    # calls a box skipped when its children are empty, so an empty element is
    # indistinguishable from a skipped one there while layout's own
    # `cv_should_skip` never even considers it (it is gated on `!children.is_empty()`).
    "cv-first-observation": ("""
<div id="topdiv"></div>
<div style="height:10000px"></div>
<div id="bottomdiv"></div>
<script>
var n = 0;
function watch(host, label) {
    var div = document.createElement("div");
    div.addEventListener("contentvisibilityautostatechange", function (e) {
        console.log("PROBE cvfo-event " + label + " n=" + (++n) + " skipped=" + e.skipped);
    });
    div.style.contentVisibility = "auto";
    host.appendChild(div);
    return div;
}
console.log("PROBE cvfo-support onevent=" +
            ("oncontentvisibilityautostatechange" in document.createElement("div")));
var top = watch(document.getElementById("topdiv"), "top");
var bottom = watch(document.getElementById("bottomdiv"), "bottom");
setTimeout(function () {
    console.log("PROBE cvfo-checked total=" + n);
    // A disconnected element must go quiet: the removal must not itself count
    // as a state change (`content-visibility-auto-state-changed-removed.html`).
    bottom.remove();
    setTimeout(function () {
        console.log("PROBE cvfo-after-remove total=" + n);
    }, 800);
}, 1500);
</script>
""", "cvfo-event top skipped=false + bottom skipped=true, once each"),

    # BUG-852, same day. The event has a content-attribute form too, and the
    # WPT file asserts it on `<svg>` as well as on `<div>`; and the computed
    # value half of the bug covers four names, of which `contain-intrinsic-size`
    # is a shorthand whose `auto` keyword `ComputedStyle` does not store at all.
    "cv-computed": ("""
<div id="a" style="content-visibility: auto; contain-intrinsic-size: auto 1px"></div>
<div id="b" style="contain-intrinsic-size: 30px 40px; contain: layout paint"></div>
<div id="c" oncontentvisibilityautostatechange="cvAttrFired('div')"
     style="content-visibility: auto"></div>
<svg id="d" oncontentvisibilityautostatechange="cvAttrFired('svg')"
     style="content-visibility: auto"></svg>
<script>
function cvAttrFired(what) { console.log("PROBE cvc-attr-fired " + what); }
function dump(id, names) {
    var cs = getComputedStyle(document.getElementById(id));
    var out = "PROBE cvc-" + id;
    for (var i = 0; i < names.length; i++) {
        out += " " + names[i] + "=" + JSON.stringify(cs.getPropertyValue(names[i]));
    }
    console.log(out);
}
setTimeout(function () {
    dump("a", ["content-visibility", "contain-intrinsic-size",
               "contain-intrinsic-width", "contain-intrinsic-height"]);
    dump("b", ["content-visibility", "contain-intrinsic-size", "contain"]);
    console.log("PROBE cvc-camel " +
                JSON.stringify(getComputedStyle(document.getElementById("a")).contentVisibility) +
                " " +
                JSON.stringify(getComputedStyle(document.getElementById("a")).containIntrinsicSize));
}, 1500);
</script>
""", "cvc-a content-visibility=auto, contain-intrinsic-size=\"auto 1px\" + cvc-attr-fired"),

    # ── an exception thrown from a load handler ────────────────────────────
    # `css/css-shapes/spec-examples/*` are `setup({single_test: true})` pages
    # whose whole body runs from `<body onload>`: an assertion that fails
    # there throws, `done()` is never reached and the file is NOTRUN rather
    # than FAIL. BUG-591 was wired for timers/rAF/listeners on 2026-08-22 —
    # this variant asks whether the load-handler paths are covered too.
    "onload-throw": ("""
<script>
window.addEventListener("error", function (e) {
    console.log("PROBE window-error " + e.message + " src=" + (e.filename || "?"));
});
window.onerror = function (msg) { console.log("PROBE window-onerror " + msg); };
window.addEventListener("load", function () {
    console.log("PROBE load-listener-entered");
    throw new Error("vflm-load-listener-boom");
});
function vflmBodyOnload() {
    console.log("PROBE body-attr-entered");
    throw new Error("vflm-body-onload-boom");
}
setTimeout(function () { console.log("PROBE throw-checked alive"); }, 1500);
</script>
""", "window-error/window-onerror naming both booms",
     ' onload="vflmBodyOnload()"'),

    # ── details / summary ──────────────────────────────────────────────────
    # `the-details-element/toggleEvent.html`: 9 of its 11 subtests hang on a
    # `toggle` event that must be fired asynchronously after `open` changes.
    "details-toggle": ("""
<details id="d"><summary>s</summary>body</details>
<script>
var d = document.getElementById("d");
d.addEventListener("toggle", function (e) {
    console.log("PROBE details-toggle open=" + d.open +
                " oldState=" + e.oldState + " newState=" + e.newState);
});
d.ontoggle = function () { console.log("PROBE details-ontoggle open=" + d.open); };
setTimeout(function () { d.open = true; console.log("PROBE details-opened prop"); }, 300);
setTimeout(function () { d.removeAttribute("open"); console.log("PROBE details-closed removeAttribute"); }, 700);
setTimeout(function () { d.setAttribute("open", ""); console.log("PROBE details-opened setAttribute"); }, 1000);
setTimeout(function () {
    d.getElementsByTagName("summary")[0].click();
    console.log("PROBE details-summary-clicked open=" + d.open);
}, 1300);
setTimeout(function () { console.log("PROBE details-checked open=" + d.open); }, 1800);
</script>
""", "details-toggle twice (open=true then open=false)"),

    # The click path is the shim's only `toggle` dispatch site
    # (`dom.rs:15519-15537`), so it is measured on its own: the state the
    # event reports must agree with the state the attribute ends in.
    "details-summary-click": ("""
<details id="d"><summary id="s">s</summary>body</details>
<script>
var d = document.getElementById("d"), s = document.getElementById("s");
d.addEventListener("toggle", function (e) {
    console.log("PROBE dsc-toggle open=" + d.open + " hasAttr=" + d.hasAttribute("open") +
                " oldState=" + e.oldState + " newState=" + e.newState);
});
function dscState(tag) {
    console.log("PROBE dsc-" + tag + " open=" + d.open +
                " hasAttr=" + d.hasAttribute("open") +
                " attr=" + JSON.stringify(d.getAttribute("open")) +
                " outerHTML=" + JSON.stringify(d.outerHTML.slice(0, 40)));
}
setTimeout(function () {
    dscState("before");
    s.click();
    dscState("after-1");
}, 400);
setTimeout(function () {
    dscState("between");
    s.click();
    dscState("after-2");
}, 900);
setTimeout(function () { console.log("PROBE dsc-checked open=" + d.open); }, 1500);
</script>
""", "dsc-toggle newState=open then newState=closed, matching dsc-after-N"),

    # Generalization of what `details-summary-click` measured: is an
    # attribute written *during* click dispatch rolled back when dispatch
    # returns, and does it matter whose attribute it is?
    "click-attr-write": ("""
<div id="src">click me</div><div id="other">other</div>
<script>
var src = document.getElementById("src"), other = document.getElementById("other");
src.addEventListener("click", function () {
    src.setAttribute("data-self", "1");
    other.setAttribute("data-other", "1");
    src.className = "clicked";
    console.log("PROBE caw-inside self=" + src.hasAttribute("data-self") +
                " other=" + other.hasAttribute("data-other") +
                " class=" + src.className);
});
setTimeout(function () {
    src.click();
    console.log("PROBE caw-after self=" + src.hasAttribute("data-self") +
                " other=" + other.hasAttribute("data-other") +
                " class=" + src.className);
}, 400);
setTimeout(function () {
    console.log("PROBE caw-later self=" + src.hasAttribute("data-self") +
                " other=" + other.hasAttribute("data-other") +
                " class=" + src.className);
}, 900);
setTimeout(function () {
    src.setAttribute("data-timer", "1");
    console.log("PROBE caw-timer-write " + src.hasAttribute("data-timer"));
}, 1200);
setTimeout(function () {
    console.log("PROBE caw-timer-checked " + src.hasAttribute("data-timer"));
}, 1600);
</script>
""", "caw-after/caw-later keep every write made inside the listener"),

    # Which of the two writes the click path makes survives, and how many
    # times the document-level handler runs per click.
    "details-click-trace": ("""
<details id="d"><summary id="s">s</summary>body</details>
<script>
var d = document.getElementById("d"), s = document.getElementById("s");
var docClicks = 0;
document.addEventListener("click", function (e) {
    docClicks++;
    console.log("PROBE dct-doc-click n=" + docClicks + " target=" + e.target.tagName +
                " open=" + d.hasAttribute("open"));
});
d.addEventListener("toggle", function () {
    console.log("PROBE dct-toggle hasAttr=" + d.hasAttribute("open"));
});
setTimeout(function () {
    s.click();
    console.log("PROBE dct-sync-after hasAttr=" + d.hasAttribute("open") + " docClicks=" + docClicks);
    Promise.resolve().then(function () {
        console.log("PROBE dct-microtask hasAttr=" + d.hasAttribute("open"));
    });
    setTimeout(function () {
        console.log("PROBE dct-macrotask hasAttr=" + d.hasAttribute("open"));
    }, 0);
}, 400);
setTimeout(function () {
    d.setAttribute("open", "");
    console.log("PROBE dct-manual-set hasAttr=" + d.hasAttribute("open"));
}, 900);
setTimeout(function () {
    console.log("PROBE dct-manual-checked hasAttr=" + d.hasAttribute("open") +
                " docClicks=" + docClicks);
}, 1400);
</script>
""", "dct-doc-click once, hasAttr stays true after the click"),

    # ── the permission element family ──────────────────────────────────────
    # `html/semantics/permission-element/*`: an incubating element
    # (WICG/PEPC). The tests wait for `onvalidationstatuschange`; measured to
    # confirm the element is simply unknown rather than half-built.
    "permission-element": ("""
<script>
["usermedia", "geolocation", "install"].forEach(function (tag) {
    var el = document.createElement(tag);
    el.setAttribute("type", "camera");
    el.onvalidationstatuschange = function () { console.log("PROBE pepc-statechange " + tag); };
    document.body.appendChild(el);
    console.log("PROBE pepc-created " + tag + " ctor=" + el.constructor.name +
                " isValid=" + el.isValid + " reason=" + el.invalidReason +
                " onevent=" + ("onvalidationstatuschange" in el));
});
setTimeout(function () { console.log("PROBE pepc-checked"); }, 1200);
</script>
""", "pepc-created … ctor=HTMLPermissionElement + pepc-statechange"),

    # ── media resource selection ───────────────────────────────────────────
    # `loading-the-media-resource/resource-selection-candidate-*`: the
    # candidate is a `<source>` child, not a `src` attribute, and the test
    # waits for `error` on the source or on the element.
    "media-source-candidate": ("""
<video id="v"><source id="s" src="vflm-media.mp4?candidate=1" type="video/mp4"></video>
<script>
var v = document.getElementById("v"), s = document.getElementById("s");
v.addEventListener("loadstart", function () { console.log("PROBE media-loadstart currentSrc=" + v.currentSrc); });
v.addEventListener("error", function () { console.log("PROBE media-element-error"); });
s.addEventListener("error", function () { console.log("PROBE media-source-error"); });
console.log("PROBE media-initial networkState=" + v.networkState +
            " readyState=" + v.readyState + " currentSrc=" + JSON.stringify(v.currentSrc));
setTimeout(function () {
    s.parentNode.removeChild(s);
    console.log("PROBE media-candidate-removed");
}, 400);
setTimeout(function () {
    console.log("PROBE media-checked networkState=" + v.networkState +
                " currentSrc=" + JSON.stringify(v.currentSrc));
}, 1500);
</script>
""", "media-loadstart + media-source-error, server sees vflm-media.mp4"),
    # `load-removes-queued-error-event.html`: `load()` must reset the element
    # and fire `emptied`, dropping the queued `error`.
    "media-load-method": ("""
<video id="v" src="vflm-missing.mp4?load-method=1"></video>
<script>
var v = document.getElementById("v");
["loadstart", "emptied", "error", "abort", "loadedmetadata"].forEach(function (name) {
    v.addEventListener(name, function () {
        console.log("PROBE media-event " + name + " networkState=" + v.networkState);
    });
});
setTimeout(function () { console.log("PROBE media-calling-load"); v.load(); }, 500);
setTimeout(function () { console.log("PROBE media-checked error=" + (v.error && v.error.code)); }, 1500);
</script>
""", "media-event loadstart, error; then emptied after media-calling-load"),
    # `playing-the-media-resource/playbackRate.html` and
    # `event_volumechange.html`, both on `<audio>` — the tag BUG-799 freezes.
    # The audio half is deliberately last: if it wedges the page, everything
    # above it has already been measured.
    "media-rate-volume": ("""
<video id="v"></video>
<script>
var v = document.getElementById("v");
v.addEventListener("ratechange", function () { console.log("PROBE media-ratechange " + v.playbackRate); });
v.addEventListener("volumechange", function () { console.log("PROBE media-volumechange v=" + v.volume + " m=" + v.muted); });
console.log("PROBE media-defaults rate=" + v.playbackRate + " default=" + v.defaultPlaybackRate +
            " volume=" + v.volume + " muted=" + v.muted);
setTimeout(function () { v.playbackRate = 0.5; console.log("PROBE media-rate-set " + v.playbackRate); }, 300);
setTimeout(function () { v.volume = 0.25; console.log("PROBE media-volume-set " + v.volume); }, 600);
setTimeout(function () { v.muted = true; console.log("PROBE media-muted-set " + v.muted); }, 900);
setTimeout(function () { console.log("PROBE media-checked"); }, 1500);
</script>
""", "media-ratechange 0.5, media-volumechange twice"),

    # ── <script src=""> ────────────────────────────────────────────────────
    # `fetch-src/empty.html`: an empty `src` is not a valid URL, so the
    # element must fire `error` (and must not request anything).
    "script-empty-src": ("""
<script>
var s = document.createElement("script");
s.onload = function () { console.log("PROBE script-empty-load"); };
s.onerror = function () { console.log("PROBE script-empty-error"); };
s.setAttribute("src", "");
document.body.appendChild(s);
var good = document.createElement("script");
good.onload = function () { console.log("PROBE script-good-load ran=" + window.vflmRan); };
good.onerror = function () { console.log("PROBE script-good-error"); };
good.src = "vflm-asset.js?good=1";
document.body.appendChild(good);
setTimeout(function () { console.log("PROBE script-checked ran=" + window.vflmRan); }, 1500);
</script>
""", "script-empty-error + script-good-load ran=1"),
}

_MAX_MARKERS = 40

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every path asked for."""

    protocol_version = "HTTP/1.1"

    def do_GET(self):  # noqa: N802 — http.server's own casing
        with _SERVED_LOCK:
            SERVED.append(self.path)
        path = self.path.split("?")[0]
        if path.endswith((".gif", ".png")):
            self._blob(PIXEL_GIF, "image/gif")
            return
        if path == "/vflm-missing.mp4":
            self.send_error(404)
            return
        if path.endswith((".mp4", ".mp3")):
            # Deliberately not a real media file: the question is whether the
            # request is made and what the element says about it.
            self._blob(b"\0\0\0\x18ftypmp42", "video/mp4")
            return
        super().do_GET()

    def _blob(self, payload, mime):
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


ASSETS = {
    "vflm-asset.js": "window.vflmRan = (window.vflmRan || 0) + 1;\n",
    "vflm-child.html": ("<!doctype html><meta charset=utf-8><title>child</title>"
                        "<script>console.log('PROBE child-ran ' + location.search);"
                        "</" "script>vflm-child-text\n"),
}


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
    log_path = os.path.join(REPO, ".tmp", f"vflm-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.vflm-{name}.html"],
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
        fetched = [p for p in SERVED if not p.startswith("/.vflm-")]
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
        spec = VARIANTS[name]
        body_attr = spec[2] if len(spec) > 2 else ""
        path = os.path.join(HERE, f".vflm-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE.replace("__NAME__", name)
                             .replace("__BODYATTR__", body_attr)
                             .replace("__BODY__", spec[0]))
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':24s} {'ticks':>5s}  {'expected':56s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, fetched = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if fetched:
                seen += "   [server saw: " + ", ".join(sorted(set(fetched))) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:24s} {ticks:5d}  {VARIANTS[name][1]:56s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half — a subresource missing there was never "
              "requested, whatever the page or the browser log says "
              "(BUG-826).")
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
