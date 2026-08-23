#!/usr/bin/env python3
"""WPT-RUN-6 slice 25: the residual's own hung subtests, measured.

Slice 24 taught `timeout_audit.py` to read the run's *partial* harness report,
so 161 of the 239 ids still unexplained now carry the name of the exact
`async_test` that never called `done()`. This probe measures the mechanisms
those names point at — each one against a live browser, on http, with the
probe's own server recording what was actually requested:

* `:focus-visible does not match after script focus move` and its 11 siblings
  (`css/selectors/focus-visible-*`, plus `focus/scroll-matches-focus.html` and
  `the-label-element/forward-focus-to-associated-element.html`) all hang
  waiting for a `focus` event. `focus-*` asks the three separable questions:
  does `element.focus()` move `document.activeElement`, does it *dispatch*
  anything, and does a click (the way those tests actually start) focus what
  it hits.
* `childList Node.insertBefore: addition mutation` and the rest of
  `dom/nodes/MutationObserver-*` (5 ids) wait for a `MutationObserver`
  callback on a mutation made from script — the half of BUG-827 that was
  reported as working. `mo-*` re-measures it record by record.
* `Animation finish event is fired again after seeking back to start`,
  `onremove event is fired when replaced animation is removed`, `Allows an
  animation to be persisted after being removed` (7 ids of `web-animations`
  and `scroll-animations`) wait on the Web Animations event/replacement
  machinery — `wa-*`.
* `WebSockets: sending non-strings ([object Object])` (8 subtests) and
  `WebSockets: close() when connecting` (4 ids) need a real server, so the
  probe carries a minimal RFC 6455 one: `ws-echo` sends every value WPT's
  `send/010.html` sends, `ws-close-connecting` closes a socket whose
  handshake is deliberately never answered.
* `Test content-type header for a body string` etc. (4 ids of
  `beacon/headers`) — `beacon`, read off the *server's* record of the request
  and its headers, never off the page (BUG-826: the browser log is not
  evidence that a request happened).
* `'resource' entries should be observable` / `PerformanceObserver with
  buffered flag sees previous resource entries` (~20 ids of `resource-timing`,
  `performance-timeline`, `largest-contentful-paint`, `longtask-timing`) —
  `perf-resource` re-measures BUG-839 and separates the two halves nobody had
  split: the observer stream and `performance.getEntriesByType`.
* `selectionchange bubbles from input`, `text setRangeText fires a select
  event` (4 ids) — `selection-events`.
* `Document-written script executes.` (3 ids) — `docwrite-script`.

Same harness as slices 15/17-22/24 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died".

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_focus_mutation_animation_gaps.py
        [--binary target/dev-release/lumen] [--seconds 6] [--variant NAME]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import base64
import hashlib
import http.server
import os
import re
import socket
import struct
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

#: Paths the probe server was asked for, per variant, with the request method
#: and the headers that carry the answer for `beacon` (`Content-Type`,
#: `Origin`, `Referer`). A request that never arrives here is evidence that
#: does not depend on the page being able to report anything.
SERVED = []
_SERVED_LOCK = threading.Lock()

#: 1x1 transparent GIF — enough for `<img>`; the point is the resource entry.
PIXEL_GIF = bytes([
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
    0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
])

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-25 probe: __NAME__</title>
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
fetch("vfma-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
</script>
""", "raf+timeout+fetch-ok"),

    # ── focus ──────────────────────────────────────────────────────────────
    # Every `focus-visible-*` test hangs in the same shape: it attaches a
    # `focus` listener and then causes focus to move, from a click or from
    # script. `focus-script` asks whether the *dispatch* happens at all, and
    # keeps the state check both synchronous (BUG-560: the shell applies the
    # request only on its next pump) and deferred.
    "focus-script": ("""
<style>
  #t:focus { background-color: rgb(0, 255, 0); }
  #t:focus-visible { outline: solid thick rgb(0, 128, 0); }
  #t:focus:not(:focus-visible) { background-color: rgb(255, 0, 0); }
</style>
<div id="t" tabindex="0">target</div>
<input id="i">
<script>
var t = document.getElementById("t"), i = document.getElementById("i");
["focus", "blur", "focusin", "focusout"].forEach(function (name) {
    t.addEventListener(name, function () { console.log("PROBE fs-event t." + name); });
    i.addEventListener(name, function () { console.log("PROBE fs-event i." + name); });
});
function state(tag) {
    var a = document.activeElement;
    console.log("PROBE fs-" + tag + " active=" + (a ? (a.id || a.tagName) : "none") +
                " matches:focus=" + t.matches(":focus") +
                " bg=" + getComputedStyle(t).backgroundColor);
}
setTimeout(function () { state("before"); t.focus(); state("sync"); }, 300);
setTimeout(function () { state("later"); }, 800);
setTimeout(function () { i.focus(); console.log("PROBE fs-moved"); }, 1200);
setTimeout(function () { state("after-move"); console.log("PROBE fs-checked"); }, 1700);
</script>
""", "fs-event t.focus + fs-later active=t matches:focus=true"),

    # How those tests actually start: a click on a focusable element. The
    # synthetic `click()` is the only click a probe can make without the
    # driver, and it is what `focus-visible-script-focus-*` reaches through
    # `document.addEventListener('click', () => target.focus())`.
    "focus-click": ("""
<div id="t" tabindex="0">target</div>
<button id="b">button</button>
<input id="i">
<script>
var t = document.getElementById("t"), b = document.getElementById("b"), i = document.getElementById("i");
[t, b, i].forEach(function (el) {
    el.addEventListener("focus", function () { console.log("PROBE fc-focus " + (el.id)); });
    el.addEventListener("click", function () { console.log("PROBE fc-click " + (el.id)); });
});
console.log("PROBE fc-defaultview " + (document.defaultView === window) +
            " typeof=" + (typeof document.defaultView));
document.addEventListener("click", function (e) {
    console.log("PROBE fc-doc-click target=" + e.target.id +
                " active=" + (document.activeElement ? document.activeElement.id : "none"));
});
function after(el, tag) {
    el.click();
    console.log("PROBE fc-after-" + tag + " active=" +
                (document.activeElement ? (document.activeElement.id || document.activeElement.tagName) : "none"));
}
setTimeout(function () { after(t, "div"); }, 300);
setTimeout(function () { after(b, "button"); }, 700);
setTimeout(function () { after(i, "input"); }, 1100);
setTimeout(function () { console.log("PROBE fc-checked"); }, 1600);
</script>
""", "fc-focus for each + fc-after-* active=that element"),

    # `the-label-element/forward-focus-to-associated-element.html`: focusing a
    # label must forward to its control (BUG-621, filed 2026-08-04 from a
    # different test — re-measured here because the family is in the residual).
    "focus-label": ("""
<label id="l1" for="c1">label-for</label><input id="c1">
<label id="l2">label-wrap <input id="c2"></label>
<script>
var c1 = document.getElementById("c1"), c2 = document.getElementById("c2");
[c1, c2].forEach(function (el) {
    el.addEventListener("focus", function () { console.log("PROBE fl-focus " + el.id); });
});
function state(tag) {
    var a = document.activeElement;
    console.log("PROBE fl-" + tag + " active=" + (a ? (a.id || a.tagName) : "none"));
}
setTimeout(function () { document.getElementById("l1").focus(); state("after-for"); }, 300);
setTimeout(function () { state("later-for"); document.getElementById("l2").focus(); state("after-wrap"); }, 800);
setTimeout(function () { state("later-wrap"); console.log("PROBE fl-checked"); }, 1300);
</script>
""", "fl-focus c1 + fl-later-for active=c1, then the same for c2"),

    # `@supports not selector(:focus-visible)` decides whether those tests
    # even consider themselves applicable, and `test_valid_selector` (which
    # several of them call first) parses the selector through CSSOM.
    "focus-visible-css": ("""
<style>
  #a { color: rgb(1, 2, 3); }
  @supports selector(:focus-visible) { #a { color: rgb(0, 255, 0); } }
  @supports not selector(:focus-visible) { #a { color: rgb(255, 0, 0); } }
</style>
<div id="a" tabindex="0">a</div>
<script>
var a = document.getElementById("a");
console.log("PROBE fv-supports css.supports=" + (window.CSS && CSS.supports("selector(:focus-visible)")) +
            " at-supports-color=" + getComputedStyle(a).color);
try { console.log("PROBE fv-query " + (document.querySelector(":focus-visible") === null)); }
catch (e) { console.log("PROBE fv-query-throws " + e); }
try { console.log("PROBE fv-matches " + a.matches(":focus-visible")); }
catch (e) { console.log("PROBE fv-matches-throws " + e); }
try { console.log("PROBE fv-within " + a.matches(":focus-within")); }
catch (e) { console.log("PROBE fv-within-throws " + e); }
setTimeout(function () {
    a.focus();
    setTimeout(function () {
        try { console.log("PROBE fv-after-focus matches=" + a.matches(":focus-visible") +
                          " outline=" + getComputedStyle(a).outlineColor); }
        catch (e) { console.log("PROBE fv-after-throws " + e); }
        console.log("PROBE fv-checked");
    }, 400);
}, 400);
</script>
""", "fv-supports css.supports=true at-supports-color=rgb(0, 255, 0)"),

    # What `test_driver.click(element)` — the way every `focus-visible-*` test
    # starts — actually runs *in the page* before the executor is reached.
    # `testdriver.js::click` calls `inView` -> `getPointerInteractablePaintTree`
    # (`getClientRects` + `document.elementsFromPoint`) and rejects with
    # "element click intercepted error" if the tree does not contain the
    # element; `testdriver-extra.js::get_context` then reads
    # `ownerDocument.defaultView` (BUG-622). This variant replays both,
    # verbatim, so the first step that fails is the one the cluster hangs on.
    "testdriver-click-path": ("""
<button id="b">click target</button>
<script>
var el = document.getElementById("b");
console.log("PROBE tdc-api getClientRects=" + (typeof el.getClientRects) +
            " elementsFromPoint=" + ("elementsFromPoint" in document) +
            " elementFromPoint=" + ("elementFromPoint" in document) +
            " defaultView=" + (typeof document.defaultView) +
            " contains=" + document.contains(el) +
            " scrollIntoView=" + (typeof el.scrollIntoView));
function paintTree() {
    var rects = el.getClientRects();
    console.log("PROBE tdc-rects n=" + rects.length +
                (rects.length ? " first=" + JSON.stringify([rects[0].left, rects[0].top,
                                                            rects[0].width, rects[0].height]) : ""));
    if (!rects.length) { return []; }
    var left = Math.max(0, rects[0].left), right = Math.min(window.innerWidth, rects[0].right);
    var top = Math.max(0, rects[0].top), bottom = Math.min(window.innerHeight, rects[0].bottom);
    var x = 0.5 * (left + right), y = 0.5 * (top + bottom);
    console.log("PROBE tdc-center " + x + "," + y);
    if (!("elementsFromPoint" in document)) { console.log("PROBE tdc-no-elementsFromPoint"); return []; }
    var tree = document.elementsFromPoint(x, y);
    console.log("PROBE tdc-tree n=" + (tree ? tree.length : "null") +
                " tags=" + (tree ? Array.prototype.map.call(tree, function (n) { return n.tagName; }).join(">") : ""));
    return tree || [];
}
setTimeout(function () {
    try {
        var tree = paintTree();
        console.log("PROBE tdc-inview " + (Array.prototype.indexOf.call(tree, el) !== -1) +
                    " contains-first=" + (tree.length ? el.contains(tree[0]) : "n/a"));
        var win = el.ownerDocument.defaultView;
        console.log("PROBE tdc-context " + (win ? "ok" : "THROWS Browsing context for element was detached"));
    } catch (e) { console.log("PROBE tdc-throws " + e); }
    console.log("PROBE tdc-checked");
}, 500);
</script>
""", "tdc-inview true + tdc-context ok — both are preconditions of test_driver.click"),

    # ── MutationObserver ───────────────────────────────────────────────────
    # BUG-827 measured the parser side (silent) and reported the script side
    # as working. `dom/nodes/MutationObserver-attributes.html` and friends are
    # still in the residual, so each record kind is measured on its own.
    "mo-attributes": ("""
<p id="n" class="c1"></p>
<script>
var n = document.getElementById("n");
var seen = [];
var mo = new MutationObserver(function (records) {
    records.forEach(function (r) {
        seen.push(r.type + ":" + r.attributeName + ":" + JSON.stringify(r.oldValue));
    });
    console.log("PROBE mo-attr-callback n=" + records.length + " [" + seen.join(", ") + "]");
});
mo.observe(n, { attributes: true, attributeOldValue: true });
n.setAttribute("data-x", "1");
n.setAttribute("class", "c2");
n.removeAttribute("data-x");
n.id = "n2";
n.className = "c3";
console.log("PROBE mo-attr-mutated sync-seen=" + seen.length);
setTimeout(function () { console.log("PROBE mo-attr-checked total=" + seen.length); }, 900);
// A second, independent observer answers the `takeRecords` half without
// draining the first one's queue (the mistake the first run of this probe
// made: `takeRecords()` empties the queue, so no callback can follow).
var taker = new MutationObserver(function () { console.log("PROBE mo-take-callback-unexpected"); });
var m2 = document.createElement("p");
document.body.appendChild(m2);
taker.observe(m2, { attributes: true });
m2.setAttribute("data-y", "1");
m2.id = "m2";
console.log("PROBE mo-take n=" + taker.takeRecords().length + " (setAttribute + id assignment)");
</script>
""", "mo-attr-callback n=5 and mo-take n=2"),

    "mo-characterdata": ("""
<p id="n">text</p>
<script>
var n = document.getElementById("n"), tn = n.firstChild;
var seen = [];
var mo = new MutationObserver(function (records) {
    records.forEach(function (r) { seen.push(r.type + ":" + JSON.stringify(r.oldValue)); });
    console.log("PROBE mo-cd-callback n=" + records.length + " [" + seen.join(", ") + "]");
});
mo.observe(n, { characterData: true, characterDataOldValue: true, subtree: true });
tn.data = "changed";
tn.appendData("-more");
console.log("PROBE mo-cd-mutated data=" + JSON.stringify(tn.data));
setTimeout(function () { console.log("PROBE mo-cd-checked total=" + seen.length); }, 900);
</script>
""", "mo-cd-callback n=2 with oldValue \"text\" / \"changed\""),

    "mo-childlist": ("""
<div id="p"><span id="a"></span><span id="b"></span></div>
<script>
var p = document.getElementById("p");
var seen = [];
var mo = new MutationObserver(function (records) {
    records.forEach(function (r) {
        seen.push(r.type + " +" + r.addedNodes.length + " -" + r.removedNodes.length +
                  " prev=" + (r.previousSibling ? r.previousSibling.id : "null") +
                  " next=" + (r.nextSibling ? r.nextSibling.id : "null"));
    });
    console.log("PROBE mo-cl-callback n=" + records.length + " [" + seen.join(" | ") + "]");
});
mo.observe(p, { childList: true });
var c = document.createElement("span"); c.id = "c";
p.insertBefore(c, document.getElementById("b"));
p.removeChild(document.getElementById("a"));
p.replaceChild(document.createElement("i"), c);
console.log("PROBE mo-cl-mutated children=" + p.childNodes.length);
setTimeout(function () {
    var sub = new MutationObserver(function (rs) {
        rs.forEach(function (r) {
            console.log("PROBE mo-cl-subtree +" + r.addedNodes.length + " -" + r.removedNodes.length +
                        " target=" + (r.target && (r.target.id || r.target.nodeName)));
        });
    });
    sub.observe(document.documentElement, { childList: true, subtree: true });
    document.getElementById("b").appendChild(document.createElement("u"));
}, 700);
setTimeout(function () { console.log("PROBE mo-cl-checked total=" + seen.length); }, 1400);
</script>
""", "mo-cl-callback n=3 + mo-cl-subtree n=1"),

    # `MutationObserver-sanity.html`'s first subtest is the argument check —
    # it is synchronous, so its presence in the residual says the *file* hung
    # somewhere later, not that the check is missing. Measured to tell those
    # two apart.
    "mo-validation": ("""
<script>
function report(tag, fn) {
    try { fn(); console.log("PROBE mv-" + tag + " no-throw"); }
    catch (e) { console.log("PROBE mv-" + tag + " " + e.name); }
}
report("empty-init", function () { new MutationObserver(function () {}).observe(document, {}); });
report("childlist", function () { new MutationObserver(function () {}).observe(document, { childList: true }); });
report("attr-filter", function () {
    new MutationObserver(function () {}).observe(document.body,
        { attributeFilter: ["class"], attributes: true });
});
report("olddata-without-cd", function () {
    new MutationObserver(function () {}).observe(document.body, { characterDataOldValue: true });
});
report("no-callback", function () { new MutationObserver(); });
console.log("PROBE mv-record-proto " + (typeof MutationRecord) +
            " observer-proto=" + (typeof MutationObserver.prototype.takeRecords));
setTimeout(function () { console.log("PROBE mv-checked"); }, 700);
</script>
""", "mv-empty-init TypeError, mv-no-callback TypeError, the rest no-throw"),

    # ── Web Animations ─────────────────────────────────────────────────────
    # `updating-the-finished-state.html`: finish, seek back, and the `finish`
    # event must fire *again*. `onfinish.html`, `persist.html`, `onremove.html`
    # and `effect-value-replaced-animations.html` are the same machinery.
    "wa-finish": ("""
<div id="t" style="width:50px;height:50px;background:blue"></div>
<script>
var t = document.getElementById("t");
var anim = t.animate([{ opacity: 1 }, { opacity: 0 }], 300);
console.log("PROBE wa-created type=" + (anim && anim.constructor && anim.constructor.name) +
            " playState=" + anim.playState + " hasFinished=" + (typeof anim.finished));
anim.onfinish = function () { console.log("PROBE wa-onfinish t=" + anim.currentTime); };
console.log("PROBE wa-eventtarget addEventListener=" + (typeof anim.addEventListener) +
            " cancel=" + (typeof anim.cancel) + " finish=" + (typeof anim.finish) +
            " reverse=" + (typeof anim.reverse) + " timeline=" + (anim.timeline && anim.timeline.constructor.name));
if (typeof anim.addEventListener === "function") {
    anim.addEventListener("finish", function () { console.log("PROBE wa-finish-listener"); });
}
if (anim.finished && anim.finished.then) {
    anim.finished.then(function () { console.log("PROBE wa-finished-promise"); },
                       function (e) { console.log("PROBE wa-finished-reject " + e); });
}
setTimeout(function () {
    console.log("PROBE wa-mid playState=" + anim.playState + " t=" + anim.currentTime);
}, 150);
setTimeout(function () {
    console.log("PROBE wa-after playState=" + anim.playState);
    anim.currentTime = 0;
    anim.play();
    console.log("PROBE wa-seeked-back playState=" + anim.playState);
}, 800);
setTimeout(function () { console.log("PROBE wa-checked playState=" + anim.playState); }, 1600);
</script>
""", "wa-onfinish + wa-finished-promise, then a second wa-onfinish after the seek"),

    # `updating-the-finished-state.html`'s hung subtest verbatim: null
    # keyframes, 1 ms, and the seek is a bare `currentTime = 0` with no
    # `play()` — which is what separates it from `wa-finish` above.
    "wa-seek-refire": ("""
<div id="t" style="width:50px;height:50px;background:blue"></div>
<script>
var t = document.getElementById("t");
var anim = t.animate(null, 1);
console.log("PROBE ws2-created type=" + (anim && anim.constructor && anim.constructor.name) +
            " ready=" + (anim && typeof anim.ready) + " finished=" + (anim && typeof anim.finished));
var n = 0;
var first = anim.finished;
anim.onfinish = function () {
    n++;
    console.log("PROBE ws2-finish n=" + n + " t=" + anim.currentTime + " playState=" + anim.playState);
    if (n === 1) {
        anim.currentTime = 0;
        console.log("PROBE ws2-seeked playState=" + anim.playState +
                    " finished-replaced=" + (anim.finished !== first));
    }
};
if (anim.ready && anim.ready.then) { anim.ready.then(function () { console.log("PROBE ws2-ready"); }); }
setTimeout(function () { console.log("PROBE ws2-checked n=" + n); }, 1500);
</script>
""", "ws2-finish n=1 then n=2 after the bare currentTime seek"),

    "wa-persist": ("""
<div id="t" style="width:50px;height:50px;background:blue"></div>
<script>
var t = document.getElementById("t");
var a = t.animate([{ opacity: 1 }, { opacity: 0.5 }], { duration: 200, fill: "forwards" });
var b;
a.onremove = function () { console.log("PROBE wp-onremove replaceState=" + a.replaceState); };
if (typeof a.addEventListener === "function") {
    a.addEventListener("remove", function () { console.log("PROBE wp-remove-listener"); });
}
console.log("PROBE wp-api persist=" + (typeof a.persist) + " commitStyles=" + (typeof a.commitStyles) +
            " replaceState=" + a.replaceState + " getAnimations=" + (typeof t.getAnimations));
setTimeout(function () {
    b = t.animate([{ opacity: 1 }, { opacity: 0.25 }], { duration: 200, fill: "forwards" });
    console.log("PROBE wp-second-created count=" +
                (t.getAnimations ? t.getAnimations().length : "n/a"));
}, 500);
setTimeout(function () {
    console.log("PROBE wp-after count=" + (t.getAnimations ? t.getAnimations().length : "n/a") +
                " a.replaceState=" + a.replaceState + " opacity=" + getComputedStyle(t).opacity);
    try { a.persist(); console.log("PROBE wp-persist-ok replaceState=" + a.replaceState); }
    catch (e) { console.log("PROBE wp-persist-throws " + e); }
    try { b.commitStyles(); console.log("PROBE wp-commit-ok style=" + t.style.opacity); }
    catch (e) { console.log("PROBE wp-commit-throws " + e); }
}, 1100);
setTimeout(function () { console.log("PROBE wp-checked"); }, 1600);
</script>
""", "wp-onremove replaceState=removed once the second animation replaces the first"),

    # ── selection ──────────────────────────────────────────────────────────
    # `selectionchange bubbles from input` / `text setRangeText fires a select
    # event when fully selected`.
    "selection-events": ("""
<input id="i" value="abcdef"><textarea id="a">abcdef</textarea>
<script>
var i = document.getElementById("i"), a = document.getElementById("a");
[i, a].forEach(function (el) {
    el.addEventListener("select", function () { console.log("PROBE se-select " + el.tagName); });
    el.addEventListener("selectionchange", function (e) {
        console.log("PROBE se-elem-selectionchange " + el.tagName + " bubbles=" + e.bubbles);
    });
});
document.addEventListener("selectionchange", function (e) {
    console.log("PROBE se-doc-selectionchange target=" +
                (e.target && (e.target.tagName || e.target.nodeName)));
});
console.log("PROBE se-api setRangeText=" + (typeof i.setRangeText) +
            " setSelectionRange=" + (typeof i.setSelectionRange) +
            " onselectionchange-in-el=" + ("onselectionchange" in i));
setTimeout(function () { i.select(); console.log("PROBE se-selected start=" + i.selectionStart + " end=" + i.selectionEnd); }, 300);
setTimeout(function () {
    try { i.setRangeText("XY", 1, 3, "select"); console.log("PROBE se-rangetext value=" + i.value); }
    catch (e) { console.log("PROBE se-rangetext-throws " + e); }
}, 700);
setTimeout(function () { a.setSelectionRange(1, 4); console.log("PROBE se-textarea-range"); }, 1100);
setTimeout(function () { console.log("PROBE se-checked"); }, 1600);
</script>
""", "se-select INPUT after select() and after setRangeText, se-doc-selectionchange"),

    # ── document.write ─────────────────────────────────────────────────────
    # `nonce-hiding/svgscript-nonces-hidden*.html` waits on
    # "Document-written script executes."; `document.open-03.html` on the
    # reopened document.
    "docwrite-script": ("""
<script>
window.dwRan = 0;
document.write("<scr" + "ipt>window.dwRan++; console.log('PROBE dw-inline-ran');</scr" + "ipt>");
console.log("PROBE dw-after-write ran=" + window.dwRan);
var s = document.createElement("script");
s.textContent = "console.log('PROBE dw-textcontent-ran');";
document.head.appendChild(s);
setTimeout(function () {
    document.write("<scr" + "ipt>console.log('PROBE dw-late-ran');</scr" + "ipt>");
    console.log("PROBE dw-late-written");
}, 400);
setTimeout(function () {
    try {
        var d = document.open();
        console.log("PROBE dw-open-returned same=" + (d === document));
        document.write("<p id=written>written</p>");
        document.close();
        console.log("PROBE dw-open-wrote found=" + !!document.getElementById("written"));
    } catch (e) { console.log("PROBE dw-open-throws " + e); }
}, 900);
setTimeout(function () { console.log("PROBE dw-checked ran=" + window.dwRan); }, 1500);
</script>
""", "dw-inline-ran before dw-after-write ran=1"),

    # ── sendBeacon ─────────────────────────────────────────────────────────
    # `beacon/headers/*`: the body kind decides the `Content-Type`, and the
    # test reads it back from the server. Whether the request happens at all
    # is the probe server's answer, not the page's (BUG-826).
    "beacon": ("""
<script>
console.log("PROBE bc-api sendBeacon=" + (navigator && typeof navigator.sendBeacon));
// Both URL forms: `XMLHttpRequest.open` keeps a relative URL verbatim and
// dies in the network layer (BUG-780), so a probe that only sends a relative
// one cannot tell "no request was made" from "the URL never resolved".
function send(tag, body, absolute) {
    var url = (absolute ? location.origin : "") + "/vfma-beacon?" + tag;
    try {
        var ok = navigator.sendBeacon(url, body);
        console.log("PROBE bc-sent " + tag + " returned=" + ok + " url=" + url);
    } catch (e) { console.log("PROBE bc-throws " + tag + " " + e); }
}
send("rel-string", "a-string-body");
send("abs-string", "a-string-body", true);
send("abs-arraybuffer", new ArrayBuffer(8), true);
send("abs-view", new Uint8Array([1, 2, 3]), true);
try { send("abs-blob", new Blob(["blob-body"], { type: "text/plain" }), true); }
catch (e) { console.log("PROBE bc-blob-unavailable " + e); }
try {
    var fd = new FormData(); fd.append("k", "v");
    send("abs-formdata", fd, true);
} catch (e) { console.log("PROBE bc-formdata-unavailable " + e); }
send("abs-nobody", undefined, true);
// A plain fetch POST to the same absolute URL is the control: it proves the
// path is reachable from this page at this moment.
fetch(location.origin + "/vfma-beacon?control-fetch", { method: "POST", body: "x" })
    .then(function () { console.log("PROBE bc-control-fetch-ok"); },
          function (e) { console.log("PROBE bc-control-fetch-err " + e); });
setTimeout(function () { console.log("PROBE bc-checked"); }, 1500);
</script>
""", "bc-sent … returned=true and the server sees every abs-* beacon"),

    # ── resource timing ────────────────────────────────────────────────────
    # BUG-839 measured that a `resource` PerformanceObserver never fires. The
    # residual's subtests split further: `getEntriesByName`/`getEntriesByType`
    # (a buffer read, no observer) and `buffered: true` (a replay of the
    # buffer through an observer).
    "perf-resource": ("""
<img id="im" src="vfma-pixel.gif?rt=1">
<script>
var got = [];
try {
    var po = new PerformanceObserver(function (list) {
        list.getEntries().forEach(function (e) { got.push(e.entryType + ":" + e.name.split("/").pop()); });
        console.log("PROBE rt-observer n=" + list.getEntries().length + " [" + got.join(", ") + "]");
    });
    po.observe({ type: "resource", buffered: true });
    console.log("PROBE rt-observe-ok supported=" +
                (PerformanceObserver.supportedEntryTypes || []).join("|"));
} catch (e) { console.log("PROBE rt-observe-throws " + e); }
performance.mark("m1");
performance.measure("mm", "m1");
fetch("vfma-asset.js?rt=2").then(function () { console.log("PROBE rt-fetch-done"); },
                                 function (e) { console.log("PROBE rt-fetch-err " + e); });
function dump(tag) {
    ["resource", "mark", "measure", "navigation", "paint"].forEach(function (type) {
        var n = performance.getEntriesByType ? performance.getEntriesByType(type).length : "n/a";
        console.log("PROBE rt-" + tag + " " + type + "=" + n);
    });
}
setTimeout(function () { dump("early"); }, 400);
setTimeout(function () {
    dump("late");
    console.log("PROBE rt-byname=" +
                (performance.getEntriesByName ? performance.getEntriesByName("mm").length : "n/a") +
                " clearResourceTimings=" + (typeof performance.clearResourceTimings) +
                " setResourceTimingBufferSize=" + (typeof performance.setResourceTimingBufferSize) +
                " onresourcetimingbufferfull=" + ("onresourcetimingbufferfull" in performance));
    console.log("PROBE rt-checked");
}, 1400);
</script>
""", "rt-observer with the img and the fetch; rt-late resource=2"),

    # ── WebSocket ──────────────────────────────────────────────────────────
    # `send/010.html` sends exactly this list and asserts the echo equals
    # `String(value)`; the probe's server echoes text frames verbatim, so a
    # missing echo is the engine's.
    "ws-echo": ("""
<script>
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/echo");
console.log("PROBE ws-created readyState=" + ws.readyState + " url=" + ws.url);
var stuff = [null, undefined, 1, {}, [], function () {}, new Error(), "plain"];
var i = 0;
ws.onerror = function () { console.log("PROBE ws-error readyState=" + ws.readyState); };
ws.onclose = function (e) { console.log("PROBE ws-close code=" + e.code + " clean=" + e.wasClean); };
ws.onmessage = function (e) {
    console.log("PROBE ws-message i=" + i + " data=" + JSON.stringify(String(e.data)) +
                " expected=" + JSON.stringify(String(stuff[i])));
    i++;
    next();
};
function next() {
    if (i >= stuff.length) { console.log("PROBE ws-all-echoed"); return; }
    try { ws.send(stuff[i]); console.log("PROBE ws-sent i=" + i + " buffered=" + ws.bufferedAmount); }
    catch (e) { console.log("PROBE ws-send-throws i=" + i + " " + e); }
}
ws.onopen = function () { console.log("PROBE ws-open readyState=" + ws.readyState); next(); };
setTimeout(function () { console.log("PROBE ws-checked readyState=" + ws.readyState + " echoed=" + i); }, 3000);
</script>
""", "ws-open then eight ws-message with data === String(value)"),

    # Where the page went in the first run of this probe: not one marker, not
    # even the `script-start` of the template's own trailing script. The
    # constructor is the suspect, so this variant brackets it — and the
    # `refused` half separates "a server that accepts and stays silent" from
    # "nothing is listening at all".
    "ws-connect-hang": ("""
<script>
console.log("PROBE wsh-before-ctor");
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/sleep");
console.log("PROBE wsh-after-ctor readyState=" + ws.readyState);
setTimeout(function () { console.log("PROBE wsh-checked readyState=" + ws.readyState); }, 1500);
</script>
""", "wsh-after-ctor readyState=0 — the constructor must not block"),

    "ws-connect-refused": ("""
<script>
console.log("PROBE wsr-before-ctor");
var ws = new WebSocket("ws://127.0.0.1:9/closed");
console.log("PROBE wsr-after-ctor readyState=" + ws.readyState);
ws.onerror = function () { console.log("PROBE wsr-error readyState=" + ws.readyState); };
ws.onclose = function (e) { console.log("PROBE wsr-close code=" + e.code + " clean=" + e.wasClean); };
setTimeout(function () { console.log("PROBE wsr-checked readyState=" + ws.readyState); }, 1500);
</script>
""", "wsr-after-ctor readyState=0 then wsr-error + wsr-close code=1006"),

    # `close/close-connecting.html`: the handshake is never answered, so the
    # socket stays CONNECTING; `close()` must move it to CLOSING and then fire
    # `close` with `wasClean === false`.
    "ws-close-connecting": ("""
<script>
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/sleep");
ws.onopen = function () { console.log("PROBE wsc-open-unexpected"); };
ws.onerror = function () { console.log("PROBE wsc-error readyState=" + ws.readyState); };
setTimeout(function () {
    console.log("PROBE wsc-before readyState=" + ws.readyState);
    try { ws.send("x"); console.log("PROBE wsc-send-no-throw"); }
    catch (e) { console.log("PROBE wsc-send-throws " + e.name); }
    ws.onclose = function (e) {
        console.log("PROBE wsc-close readyState=" + ws.readyState +
                    " code=" + e.code + " clean=" + e.wasClean);
    };
    ws.close();
    console.log("PROBE wsc-after-close readyState=" + ws.readyState);
}, 1000);
setTimeout(function () { console.log("PROBE wsc-checked readyState=" + ws.readyState); }, 3000);
</script>
""", "wsc-before readyState=0, wsc-after-close readyState=2, then wsc-close clean=false"),
}

_MAX_MARKERS = 40

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")

#: Headers worth recording: the four `beacon/headers/*` tests read exactly
#: these back off the server.
_INTERESTING_HEADERS = ("content-type", "origin", "referer")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every request with its method and
    the headers the `beacon` variant is about."""

    protocol_version = "HTTP/1.1"

    def _record(self, method):
        extras = []
        for name in _INTERESTING_HEADERS:
            value = self.headers.get(name)
            if value is not None:
                extras.append(f"{name}={value}")
        suffix = (" " + " ".join(extras)) if extras else ""
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.path}{suffix}")

    def do_GET(self):  # noqa: N802 — http.server's own casing
        self._record("GET")
        path = self.path.split("?")[0]
        if path.endswith((".gif", ".png")):
            self._blob(PIXEL_GIF, "image/gif")
            return
        super().do_GET()

    def do_POST(self):  # noqa: N802 — http.server's own casing
        self._record("POST")
        length = int(self.headers.get("content-length") or 0)
        if length:
            self.rfile.read(length)
        self._blob(b"", "text/plain")

    def _blob(self, payload, mime):
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


ASSETS = {
    "vfma-asset.js": "window.vfmaRan = (window.vfmaRan || 0) + 1;\n",
}

#: RFC 6455 §1.3 handshake constant.
_WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def _ws_frame(opcode, payload):
    """Server->client frame (never masked, §5.1)."""
    head = bytes([0x80 | opcode])
    length = len(payload)
    if length < 126:
        head += bytes([length])
    elif length < (1 << 16):
        head += bytes([126]) + struct.pack("!H", length)
    else:
        head += bytes([127]) + struct.pack("!Q", length)
    return head + payload


def _ws_read_frame(conn):
    """Read one client frame; return `(opcode, payload)` or None on EOF."""
    def recv(n):
        buf = b""
        while len(buf) < n:
            chunk = conn.recv(n - len(buf))
            if not chunk:
                return None
            buf += chunk
        return buf

    head = recv(2)
    if head is None:
        return None
    opcode = head[0] & 0x0F
    masked = bool(head[1] & 0x80)
    length = head[1] & 0x7F
    if length == 126:
        extra = recv(2)
        if extra is None:
            return None
        length = struct.unpack("!H", extra)[0]
    elif length == 127:
        extra = recv(8)
        if extra is None:
            return None
        length = struct.unpack("!Q", extra)[0]
    mask = recv(4) if masked else None
    if masked and mask is None:
        return None
    payload = recv(length) if length else b""
    if payload is None:
        return None
    if masked:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return opcode, payload


def _ws_serve(conn):
    """One connection: `/echo` echoes every frame, anything else (`/sleep`)
    never answers the handshake — the shape `close-connecting.html` needs."""
    try:
        request = b""
        while b"\r\n\r\n" not in request:
            chunk = conn.recv(4096)
            if not chunk:
                return
            request += chunk
        head = request.decode("latin-1")
        path = head.split(" ", 2)[1] if " " in head else "/"
        if not path.startswith("/echo"):
            # Deliberately silent: the client must stay in CONNECTING.
            while conn.recv(4096):
                pass
            return
        key = ""
        for line in head.split("\r\n"):
            if line.lower().startswith("sec-websocket-key:"):
                key = line.split(":", 1)[1].strip()
        accept = base64.b64encode(
            hashlib.sha1((key + _WS_GUID).encode("ascii")).digest()).decode("ascii")
        conn.sendall(("HTTP/1.1 101 Switching Protocols\r\n"
                      "Upgrade: websocket\r\nConnection: Upgrade\r\n"
                      f"Sec-WebSocket-Accept: {accept}\r\n\r\n").encode("ascii"))
        while True:
            frame = _ws_read_frame(conn)
            if frame is None:
                return
            opcode, payload = frame
            if opcode == 0x8:
                conn.sendall(_ws_frame(0x8, payload[:2]))
                return
            if opcode == 0x9:
                conn.sendall(_ws_frame(0xA, payload))
                continue
            if opcode in (0x1, 0x2):
                conn.sendall(_ws_frame(opcode, payload))
    except OSError:
        pass
    finally:
        try:
            conn.close()
        except OSError:
            pass


def _serve_ws():
    """Start the minimal RFC 6455 server; return (port, shutdown)."""
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(8)
    port = listener.getsockname()[1]
    stop = threading.Event()

    def loop():
        while not stop.is_set():
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=_ws_serve, args=(conn,), daemon=True).start()

    threading.Thread(target=loop, daemon=True).start()

    def shutdown():
        stop.set()
        try:
            listener.close()
        except OSError:
            pass

    return port, shutdown


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
    log_path = os.path.join(REPO, ".tmp", f"vfma-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.vfma-{name}.html"],
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
        served = [p for p in SERVED if "/.vfma-" not in p]
    return ticks, markers, served


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

    ws_port, ws_shutdown = _serve_ws()
    written = []
    for name in wanted:
        body = VARIANTS[name][0].replace("__WSPORT__", str(ws_port))
        path = os.path.join(HERE, f".vfma-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE.replace("__NAME__", name).replace("__BODY__", body))
        written.append(path)
    for asset, content in ASSETS.items():
        path = os.path.join(HERE, asset)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(content)
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':22s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, served = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if served:
                seen += "   [server saw: " + ", ".join(sorted(set(served))) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:22s} {ticks:5d}  {VARIANTS[name][1]:62s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
        print("read the per-variant markers against the `expected` column: a "
              "live page (ticks > 0) that never printed its expected marker is "
              "waiting for something the engine does not produce, and a test "
              "built on that wait can only TIMEOUT. `server saw` is the "
              "independent half — a request missing there was never made, "
              "whatever the page or the browser log says (BUG-826).")
    finally:
        shutdown()
        ws_shutdown()
        for path in written:
            try:
                os.remove(path)
            except OSError:
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
