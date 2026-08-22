#!/usr/bin/env python3
"""WPT-RUN-6 slice 21: what the navigation, form and compiled-import waits do.

After slice 20 the unexplained TIMEOUT residual of the WPT-RUN-5 snapshot is
412 ids, and the three densest shapes left are all *waits* again — nothing in
the browser's output names them, which is why the first two stages of
`timeout_audit.py` cannot claim them:

* 27 ids in `html/browsers/browsing-the-web` + `html/browsers/history` wait on
  a document being *replaced*: `pagehide`/`unload` before a navigation, a
  `hashchange` after `location.hash = ...`, `location.assign`/`replace`/
  `reload`, and `history.back()` across a real document boundary;
* 18 ids in `html/semantics/scripting-1/the-script-element` — 8 of them the
  `string-compilation-*` family — evaluate `import()` from inside `eval`, the
  `Function` constructor and a reflected inline event handler, and `await` the
  module's own callback;
* 9 ids in `html/semantics/forms` wait for a `submit` event that a
  `button.click()`/`requestSubmit()` should produce, or for the form's target
  document to load.

Same harness as slices 15/17/18/19/20 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died". Slice 20's addition is kept and leaned on
harder here: the probe's own http server records every path it is asked for,
so "the form never submitted" is separated from "the form submitted and the
target said nothing" without believing anything the page reports — the same
witness answers "did the navigation happen at all", which matters because a
failed navigation reports success (BUG-438).

Measured 2026-08-22 (dev-release, Linux, commit `762a0cad9`, `--seconds 6`;
the three history variants re-measured at `--seconds 14`). What it found, and
which bug each finding became:

    settimeout-string        a *string* handed to setTimeout/setInterval is
                             dropped, a plausible id returned  <- BUG-830
    nav-hashchange-late      `hashchange` is dispatched synchronously from the
                             setter, so a listener attached on the next line
                             never sees it                     <- BUG-831
    anchor-fragment-plain    a click on `<a href="#x">` performs a FULL
                             navigation: the document restarts, no
                             `hashchange`, and a self-clicking page loops
                                                               <- BUG-832
    nav-pagehide-unload      the navigation happens and `pagehide` fires (with
                             `persisted=true`), but `unload`, `beforeunload`
                             and `visibilitychange` never do    <- BUG-833
    nav-back-wedges          `history.back()` across a document boundary
    nav-back-cross-document  freezes the page: no traversal, no request, no
                             `pageshow`, and no timer ever runs again
                                                               <- BUG-834
    session-storage-across-reload
                             `sessionStorage` is empty on the next document
                             while `localStorage` survives      <- BUG-835
    label-click-activates    a click on a `<span>` inside a `<label>` does
                             nothing; the click on the `<label>` itself
                             (`label-click-direct`) works       <- BUG-836
    script-src-empty         `<script src="">` fires neither `load` nor
                             `error`                            <- BUG-837
    iframe-src-child-runs    the child never runs, `contentWindow` is null
                                                               <- BUG-480

What works, and is kept here as the control set: `hashchange` with the
listener attached first (including a chain driven from the handler),
`location.assign`/`replace`/`reload` (all three reach the server),
`import()` from a classic script, from `eval`, from the `Function`
constructor and from a reflected inline handler, `button.click()` →
`submit`, `form.requestSubmit()`, `form.submit()` → the target document with
the right query, `<input type=image>` click → `submit`, `new Option()` +
`select.add()`, and `script.src` changed after preparation (the server is
asked only for the first URL).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
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
#: arrives here is the strongest possible evidence: it does not depend on the
#: page being able to report anything, which is exactly what a navigation or a
#: form submission cannot be trusted about (BUG-438).
SERVED = []
_SERVED_LOCK = threading.Lock()

#: `body` is spliced into a page that also arms a `setInterval` logging
#: `PROBE tick`, so "the marker never came" is separable from "the page died".
#: `expect` is what a spec-compliant engine prints, kept next to the
#: measurement so a change in either direction is visible.
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
</script>
""", "raf+timeout"),

    # ── document replacement ───────────────────────────────────────────────
    # `scroll-to-fragid/004.html`…`007.html`: a same-document fragment
    # navigation must fire `hashchange` on the window.
    "nav-hashchange": ("""
<script>
addEventListener("hashchange", function (e) {
    console.log("PROBE hashchange type=" + e.type + " hash=" + location.hash);
}, true);
window.onhashchange = function () { console.log("PROBE onhashchange-prop"); };
setTimeout(function () {
    location.hash = "pnfi";
    console.log("PROBE hash-set hash=" + location.hash + " href=" + location.href);
    setTimeout(function () { console.log("PROBE after-hash hash=" + location.hash); }, 400);
}, 200);
</script>
""", "hashchange + onhashchange-prop, hash=#pnfi"),
    # The same fragment navigation as above, with the listener attached
    # *after* the assignment — which is exactly the shape of all three
    # residual `scroll-to-fragid` tests (`004`/`005`/`007` set `location.hash`
    # and only then call `addEventListener("hashchange", ...)`). A
    # synchronously dispatched `hashchange` is invisible to them.
    "nav-hashchange-late": ("""
<script>
setTimeout(function () {
    location.hash = "pnfi-late";
    console.log("PROBE hash-set hash=" + location.hash);
    addEventListener("hashchange", function (e) {
        console.log("PROBE late-hashchange old=" + e.oldURL + " new=" + e.newURL);
    }, true);
    console.log("PROBE listener-attached");
}, 200);
</script>
""", "late-hashchange (the event is queued, not synchronous)"),
    # `scroll-to-fragid/007.html` drives 100 further fragment navigations from
    # inside the handler, so the second and later events matter too.
    "nav-hashchange-chain": ("""
<script>
var count = 0;
addEventListener("hashchange", function () {
    console.log("PROBE chain-hashchange " + (++count) + " hash=" + location.hash);
    if (count < 3) { location.hash = "pnfi-" + count; }
}, true);
setTimeout(function () { location.hash = "pnfi-0"; }, 200);
</script>
""", "chain-hashchange 1..3"),
    # `overlapping-navigations-and-traversals/anchor-fragment-history-back-on-click.html`
    # navigates by *clicking an anchor* with a fragment href and traverses
    # from the click handler; `popstate` is what it counts.
    "anchor-fragment-click": ("""
<a id="pnfi-a" href="#pnfi-3">go</a>
<script>
onpopstate = function () { console.log("PROBE popstate hash=" + location.hash); };
addEventListener("hashchange", function () { console.log("PROBE hashchange hash=" + location.hash); });
setTimeout(function () {
    location.hash = "#pnfi-1";
    location.hash = "#pnfi-2";
    var a = document.getElementById("pnfi-a");
    a.onclick = function () { console.log("PROBE anchor-onclick"); history.back(); };
    a.click();
    setTimeout(function () { console.log("PROBE after-click hash=" + location.hash); }, 500);
}, 200);
</script>
""", "anchor-onclick, hash=#pnfi-3, then popstate back to #pnfi-1"),
    # `unloading-documents/unload/006.html`…`009.html` and
    # `pagehide-on-history-forward.html` hang on the *unload* half of a
    # navigation: the outgoing document is expected to hear `pagehide`
    # (and, in the `unload/*` files, `unload`) before it is replaced.
    "nav-pagehide-unload": ("""
<script>
addEventListener("pagehide", function (e) { console.log("PROBE pagehide persisted=" + e.persisted); });
window.onpagehide = function () { console.log("PROBE onpagehide-prop"); };
addEventListener("unload", function () { console.log("PROBE unload"); });
addEventListener("beforeunload", function () { console.log("PROBE beforeunload"); });
addEventListener("visibilitychange", function () {
    console.log("PROBE visibilitychange state=" + document.visibilityState);
});
setTimeout(function () {
    console.log("PROBE navigating");
    location.href = "pnfi-next.html?from=pagehide";
}, 300);
</script>
""", "pagehide + unload, then next-page"),
    # `the-location-interface/assign_after_load.html` / `assign_before_load.html`:
    # `location.assign` must navigate and push a session-history entry.
    "nav-location-assign": ("""
<script>
setTimeout(function () {
    console.log("PROBE before-assign length=" + history.length);
    location.assign("pnfi-next.html?from=assign");
}, 300);
</script>
""", "next-page from=assign"),
    # `location_replace_session_history.html`: `replace` navigates *without*
    # growing the session history.
    "nav-location-replace": ("""
<script>
setTimeout(function () {
    console.log("PROBE before-replace length=" + history.length);
    location.replace("pnfi-next.html?from=replace");
}, 300);
</script>
""", "next-page from=replace"),
    # `location_reload.html` pings its iframe five times across reloads; the
    # page-level half of that is simply whether `reload()` re-fetches.
    # `sessionStorage` is the brake: `location.reload()` keeps the URL, so a
    # branch on `location.search` reloads forever and measures nothing but
    # the probe's own loop (first cut of this variant did exactly that).
    "nav-location-reload": ("""
<script>
var been = sessionStorage.getItem("pnfi-reloaded");
console.log("PROBE reload-flag " + been);
if (!been) {
    sessionStorage.setItem("pnfi-reloaded", "1");
    setTimeout(function () {
        console.log("PROBE before-reload");
        location.reload();
    }, 300);
} else {
    console.log("PROBE reloaded-page");
}
</script>
""", "before-reload, then reloaded-page"),
    # `the-history-interface/009.html`/`010.html` and
    # `back-pushstate-back-history-state.html` traverse *across documents*:
    # slice 20 measured `history.go(-1)` only within one document.
    # The second document has to be *this* page again (with a query), not the
    # shared `pnfi-next.html`: only then does the same script get to run the
    # traversal half and report where it landed.
    "nav-back-cross-document": ("""
<script>
addEventListener("pageshow", function (e) { console.log("PROBE pageshow persisted=" + e.persisted); });
if (location.search.indexOf("second") < 0) {
    console.log("PROBE first-doc length=" + history.length);
    setTimeout(function () { location.href = ".pnfi-nav-back-cross-document.html?second"; }, 300);
} else {
    console.log("PROBE second-doc length=" + history.length);
    setTimeout(function () {
        history.back();
        setTimeout(function () {
            console.log("PROBE after-back search=" + location.search + " length=" + history.length);
        }, 600);
    }, 300);
}
</script>
""", "second-doc, then after-back search= on the first document"),
    # The click half of `anchor-fragment-history-back-on-click.html` on its
    # own, with no traversal to confuse it: a click on `<a href="#x">` is a
    # fragment navigation and must move `location.hash`.
    "anchor-fragment-plain": ("""
<a id="pnfi-a" href="#pnfi-3">go</a>
<script>
addEventListener("hashchange", function () { console.log("PROBE hashchange hash=" + location.hash); });
setTimeout(function () {
    var a = document.getElementById("pnfi-a");
    a.addEventListener("click", function (e) { console.log("PROBE anchor-click defaultPrevented=" + e.defaultPrevented); });
    a.click();
    setTimeout(function () { console.log("PROBE after-click hash=" + location.hash + " href=" + location.href); }, 400);
}, 200);
</script>
""", "anchor-click + hashchange, after-click hash=#pnfi-3"),
    # The traversal half, isolated and given room: `the-history-interface/009`
    # and `010` go back across a *document* boundary and then read the URL.
    "nav-back-wedges": ("""
<script>
if (location.search.indexOf("second") < 0) {
    setTimeout(function () { location.href = ".pnfi-nav-back-wedges.html?second"; }, 300);
} else {
    setTimeout(function () {
        console.log("PROBE calling-back length=" + history.length);
        history.back();
        console.log("PROBE back-returned");
        setTimeout(function () { console.log("PROBE t+500 search=" + location.search); }, 500);
        setTimeout(function () { console.log("PROBE t+2000 search=" + location.search); }, 2000);
    }, 1000);
}
</script>
""", "back-returned, t+500 and t+2000 on the first document"),
    # `sessionStorage` as a probe brake is measured, not assumed — the first
    # cut of `nav-location-reload` used it and the flag came back `null` on
    # every load, which is a finding of its own.
    "session-storage-across-reload": ("""
<script>
var seen = sessionStorage.getItem("pnfi-seen");
console.log("PROBE seen=" + seen + " local=" + localStorage.getItem("pnfi-seen"));
sessionStorage.setItem("pnfi-seen", "1");
localStorage.setItem("pnfi-seen", "1");
if (location.search.indexOf("second") < 0) {
    setTimeout(function () { location.href = ".pnfi-session-storage-across-reload.html?second"; }, 400);
}
</script>
""", "seen=1 on the second document"),
    # The control for every cluster-A test that drives its navigation from a
    # subframe (all of `unload/*`, `assign_*`, `location_reload`): if the
    # child never runs, the unload question is not even reached — BUG-480.
    "iframe-src-child-runs": ("""
<script>
window.pnfiFromChild = function (what) { console.log("PROBE from-child " + what); };
</script>
<iframe src="pnfi-child.html"></iframe>
<script>
setTimeout(function () {
    var f = document.getElementsByTagName("iframe")[0];
    console.log("PROBE iframe-present src=" + f.getAttribute("src")
                + " contentWindow=" + (f.contentWindow ? "yes" : "no"));
}, 500);
</script>
""", "from-child ran (BUG-480 says it will not)"),

    # ── <script> element ───────────────────────────────────────────────────
    # `fetch-src/empty.html` + `empty-with-base.html`: `src=""` must fail the
    # fetch and fire `error` asynchronously.
    "script-src-empty": ("""
<script>
var s = document.createElement("script");
s.onerror = function (e) { console.log("PROBE script-error type=" + e.type); };
s.onload = function () { console.log("PROBE script-load"); };
s.setAttribute("src", "");
document.body.appendChild(s);
console.log("PROBE appended-empty-src");
</script>
""", "script-error type=error"),
    # The control for the `string-compilation-*` family: a dynamic `import()`
    # straight out of the classic inline script.
    "import-dynamic-plain": ("""
<script>
import("./pnfi-module.js").then(function (m) {
    console.log("PROBE import-plain value=" + m.pnfi);
}, function (e) { console.log("PROBE import-plain-rejected " + e); });
</script>
""", "import-plain value=1"),
    # `string-compilation-classic.html` / `-module.html` run the same import
    # through `eval` and `setTimeout`; the base URL must stay the document's.
    "import-eval": ("""
<script>
try {
    eval("import('./pnfi-module.js').then(function (m) { console.log('PROBE import-eval value=' + m.pnfi); },"
       + " function (e) { console.log('PROBE import-eval-rejected ' + e); });");
    console.log("PROBE eval-returned");
} catch (e) { console.log("PROBE eval-threw " + e); }
setTimeout("import('./pnfi-module.js').then(function (m) { console.log('PROBE import-settimeout value=' + m.pnfi); },"
         + " function (e) { console.log('PROBE import-settimeout-rejected ' + e); });", 100);
</script>
""", "import-eval value=1 + import-settimeout value=1"),
    # The `setTimeout` evaluator of the same family, on its own: every
    # `string-compilation-*` test runs its import through a *string* handler
    # (`resources`-level `scripts/setTimeout.js` is literally
    # ``setTimeout(`import(...)`, 0)``), and `promise_test`s are sequential,
    # so a string handler that is never compiled takes the whole file with it.
    "settimeout-string": ("""
<script>
setTimeout("console.log('PROBE string-timeout-ran')", 50);
var id = setInterval("console.log('PROBE string-interval-ran')", 100);
setTimeout(function () { clearInterval(id); }, 400);
setTimeout(function () { console.log("PROBE fn-timeout-ran"); }, 50);
console.log("PROBE armed");
</script>
""", "string-timeout-ran + string-interval-ran"),
    # Same family, `the Function constructor` evaluator.
    "import-function-ctor": ("""
<script>
try {
    Function("import('./pnfi-module.js').then(function (m) { console.log('PROBE import-fn value=' + m.pnfi); },"
           + " function (e) { console.log('PROBE import-fn-rejected ' + e); });")();
    console.log("PROBE fn-returned");
} catch (e) { console.log("PROBE fn-threw " + e); }
</script>
""", "import-fn value=1"),
    # Same family, the two inline-event-handler evaluators: `div.onclick()`
    # (reflected handler called directly) and `div.click()` (UA code).
    "import-inline-handler": ("""
<div id="pnfi-dummy"></div>
<script>
var d = document.getElementById("pnfi-dummy");
d.setAttribute("onclick", "import('./pnfi-module.js').then(function (m) { console.log('PROBE import-onclick value=' + m.pnfi); },"
                        + " function (e) { console.log('PROBE import-onclick-rejected ' + e); });");
console.log("PROBE reflected=" + typeof d.onclick);
try { d.onclick(); console.log("PROBE onclick-called"); }
catch (e) { console.log("PROBE onclick-threw " + e); }
setTimeout(function () {
    d.setAttribute("onclick", "console.log('PROBE ua-click-ran');");
    d.click();
    console.log("PROBE clicked");
}, 200);
</script>
""", "reflected=function, import-onclick value=1, ua-click-ran"),
    # `change-src-attr-prepare-a-script.html`: once a script has been
    # prepared, changing `src` must not start a second fetch.
    "script-change-src-attr": ("""
<script>
var s = document.createElement("script");
s.src = "pnfi-asset.js";
s.onload = function () { console.log("PROBE first-load ran=" + window.pnfiRan); };
s.onerror = function () { console.log("PROBE first-error"); };
document.body.appendChild(s);
s.src = "pnfi-asset2.js";
console.log("PROBE src-changed to=" + s.getAttribute("src"));
</script>
""", "first-load ran=1, and the server is asked only for pnfi-asset.js"),

    # ── forms ──────────────────────────────────────────────────────────────
    # `the-button-element/button-click-submits.html`: `button.click()` inside
    # a connected form fires `submit` on the form.
    "form-button-click-submit": ("""
<script>
var form = document.createElement("form");
var button = document.createElement("button");
form.appendChild(button);
document.body.appendChild(form);
form.addEventListener("submit", function (ev) {
    ev.preventDefault();
    console.log("PROBE submit-event target-is-form=" + (ev.target === form));
});
button.addEventListener("click", function () { console.log("PROBE button-click"); });
button.click();
console.log("PROBE clicked type=" + button.type);
</script>
""", "button-click + submit-event target-is-form=true"),
    # `the-form-element/*`: the programmatic entry points to the same
    # algorithm, plus the `submit` event a listener is supposed to see.
    "form-requestsubmit": ("""
<form id="pnfi-form" action="pnfi-form-target.html" method="get">
  <input name="name" value="value">
</form>
<script>
var form = document.getElementById("pnfi-form");
form.addEventListener("submit", function (ev) {
    ev.preventDefault();
    console.log("PROBE submit-event-listener");
});
console.log("PROBE requestSubmit=" + typeof form.requestSubmit + " submit=" + typeof form.submit);
try { form.requestSubmit(); console.log("PROBE requestSubmit-returned"); }
catch (e) { console.log("PROBE requestSubmit-threw " + e); }
</script>
""", "requestSubmit=function, submit-event-listener"),
    # `form-action-submission.html` / `-with-base-url.html` wait for the
    # *target document* to load and report its own URL. The server is the
    # witness here: a submission that never happened asks for nothing.
    "form-submit-navigates": ("""
<form id="pnfi-form" action="pnfi-form-target.html" method="get">
  <input name="name" value="value">
</form>
<script>
setTimeout(function () {
    var form = document.getElementById("pnfi-form");
    try { form.submit(); console.log("PROBE submit-returned"); }
    catch (e) { console.log("PROBE submit-threw " + e); }
}, 300);
</script>
""", "form-target-loaded search=?name=value, server saw the query"),
    # `the-label-element/label-inside-anchor.html` and
    # `forward-focus-to-associated-element.html`: a click on the label (or on
    # an inline element inside it) is forwarded to the labelled control.
    "label-click-activates": ("""
<label for="pnfi-cb"><span id="pnfi-text">peas?</span></label>
<input type="checkbox" name="peas" id="pnfi-cb">
<script>
var text = document.getElementById("pnfi-text");
var cb = document.getElementById("pnfi-cb");
cb.onchange = function () { console.log("PROBE change checked=" + cb.checked); };
cb.onclick = function () { console.log("PROBE cb-click"); };
cb.addEventListener("focus", function () { console.log("PROBE cb-focus"); });
text.click();
console.log("PROBE clicked-span checked=" + cb.checked
            + " active=" + (document.activeElement ? document.activeElement.id : "none"));
</script>
""", "cb-click + change checked=true"),
    # The control for the variant above: the same click, but on the `<label>`
    # itself. The shim's activation table has a `LABEL` branch
    # (`dom.rs:14647`), so this separates "labels do nothing" from "activation
    # behaviour is looked up on the event *target* instead of on its nearest
    # activatable ancestor".
    "label-click-direct": ("""
<label id="pnfi-label" for="pnfi-cb">peas?</label>
<input type="checkbox" name="peas" id="pnfi-cb">
<script>
var label = document.getElementById("pnfi-label");
var cb = document.getElementById("pnfi-cb");
console.log("PROBE control=" + (label.control ? label.control.id : "none")
            + " labels=" + (cb.labels ? cb.labels.length : "none"));
cb.onchange = function () { console.log("PROBE change checked=" + cb.checked); };
cb.onclick = function () { console.log("PROBE cb-click"); };
label.click();
console.log("PROBE clicked-label checked=" + cb.checked);
</script>
""", "cb-click + change checked=true"),
    # `the-input-element/image-click-form-data.html`: clicking an
    # `<input type=image>` submits the form and contributes `x`/`y`.
    "input-image-click": ("""
<form id="pnfi-form" action="pnfi-form-target.html" method="get">
  <input type="image" id="pnfi-img" name="pic" src="pnfi-pixel.png">
</form>
<script>
var form = document.getElementById("pnfi-form");
form.addEventListener("submit", function (ev) {
    ev.preventDefault();
    console.log("PROBE submit-from-image");
});
document.getElementById("pnfi-img").click();
console.log("PROBE image-clicked");
</script>
""", "submit-from-image"),
    # `the-select-element/select-add.html` is a *synchronous* test, so it
    # should FAIL rather than TIMEOUT — unless the constructor it opens with
    # is missing and BUG-591 swallows the throw.
    "select-add": ("""
<form style="display:none">
  <select id="pnfi-select"></select>
</form>
<script>
try {
    var opt = new Option("Marry", "1");
    console.log("PROBE option-ctor tag=" + opt.tagName);
    var sel = document.getElementById("pnfi-select");
    console.log("PROBE select-add=" + typeof sel.add);
    sel.add(opt);
    console.log("PROBE added value=" + (sel.options && sel.options[0] ? sel.options[0].value : "no-options"));
} catch (e) { console.log("PROBE select-threw " + e); }
</script>
""", "option-ctor tag=OPTION, added value=1"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-21 probe: %(name)s</title>
<body>
%(body)s
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

#: Files the probe pages point at. `pnfi-next.html`/`pnfi-form-target.html`
#: tick too, so a navigation that lands keeps the liveness signal alive.
ASSETS = {
    "pnfi-module.js": "export const pnfi = 1;\n",
    "pnfi-asset.js": "window.pnfiRan = (window.pnfiRan || 0) + 1;\n",
    "pnfi-asset2.js": "window.pnfiRan2 = 1;\n",
    "pnfi-next.html": """<!doctype html>
<meta charset=utf-8>
<title>slice-21 next document</title>
<script>
console.log("PROBE next-page search=" + location.search + " length=" + history.length);
var _m = 0;
setInterval(function () { console.log("PROBE tick " + (++_m)); }, 500);
</script>
""",
    "pnfi-child.html": """<!doctype html>
<meta charset=utf-8>
<title>slice-21 subframe</title>
<script>
console.log("PROBE child-ran");
try { parent.pnfiFromChild("reached-parent"); }
catch (e) { console.log("PROBE child-threw " + e); }
</script>
""",
    "pnfi-form-target.html": """<!doctype html>
<meta charset=utf-8>
<title>slice-21 form target</title>
<script>
console.log("PROBE form-target-loaded search=" + location.search);
var _k = 0;
setInterval(function () { console.log("PROBE tick " + (++_k)); }, 500);
</script>
""",
}

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every path asked for."""

    def do_GET(self):  # noqa: N802 — http.server's own casing
        with _SERVED_LOCK:
            SERVED.append(self.path)
        if self.path.startswith("/pnfi-pixel.png"):
            # A 1x1 transparent GIF is enough for `<input type=image>`; the
            # bytes never matter, only that the request is answered.
            body = (b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff"
                    b"!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00"
                    b"\x01\x00\x00\x02\x02D\x01\x00;")
            self.send_response(200)
            self.send_header("Content-Type", "image/gif")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return None
        if self.path.startswith("/pnfi-form-target.html?"):
            # `SimpleHTTPRequestHandler` serves the query-less file fine, but
            # only if the query is stripped first.
            self.path = "/pnfi-form-target.html"
        if self.path.startswith("/pnfi-next.html?"):
            self.path = "/pnfi-next.html"
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
    log_path = os.path.join(REPO, ".tmp", f"pnfi-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.pnfi-{name}.html"],
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
        if marker.startswith("tick "):
            continue
        if marker not in markers:
            markers.append(marker)
    with _SERVED_LOCK:
        fetched = [p for p in SERVED if not p.startswith("/.pnfi-")]
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
        path = os.path.join(HERE, f".pnfi-{name}.html")
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
        print(f"{'variant':26s} {'ticks':>5s}  {'expected':52s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, fetched = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if fetched:
                seen += "   [server saw: " + ", ".join(sorted(set(fetched))) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:26s} {ticks:5d}  {VARIANTS[name][1]:52s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half — a navigation or a form submission missing "
              "there never happened, whatever the page reports (BUG-438).")
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
