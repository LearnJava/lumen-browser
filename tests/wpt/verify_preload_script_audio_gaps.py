#!/usr/bin/env python3
"""WPT-RUN-6 slice 20: four more silent waits behind the densest residual.

After slice 19 the unexplained TIMEOUT residual of the WPT-RUN-5 snapshot is
476 ids. Four shapes stand out, and — as in every slice since 15 — each is a
*wait* rather than an error, so none of them can be read off the browser's
output:

* 19 ids hang on a **`<link>` created from script**: all 10 residual
  `connection-allowlist/tentative` ids poll a server-side key-value store in a
  `while (true)` loop until the preload they just appended shows up
  (`resources/utils.js::nextValueFromServer`), and 7
  `shadow-dom/declarative/.../shadowrootadoptedstylesheets-modulepreload-*`
  plus 2 `html/semantics/scripting-1/.../modulepreload-referrer*` `await` the
  `load` event of a `rel=modulepreload` link;
* 9 ids are `html/dom/render-blocking`, whose shared helper
  (`support/test-render-blocking.js`) does `step_wait(() =>
  performance.getEntriesByType('paint').length)` and, in three of them, first
  waits for a `MutationObserver` record about a *parser-inserted* script;
* 11 ids are `webaudio/the-audio-api` — `OfflineAudioContext` rendering plus
  the `ended` event of a source node;
* ~5 ids in `html/semantics/scripting-1/the-script-element` `await` the
  `load`/`error` event of a **`<script>` created from script**.

The `<link>` group is measured differently from the previous slices, and the
difference is the point: this probe's own http server records every path it
serves, so "the element fired no event" is separated from "the request was
never made at all" without believing anything the page says.

Same harness as slices 15/17/18/19 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died".

Measured 2026-08-22 (dev-release, Linux, commit `79f7df91a`, `--seconds 5`;
every variant ticked 9 times, i.e. no page died on us). What it found:

    link-stylesheet          link-load          [server saw the .css]  <- control
    link-preload-script      link-appended only [server saw nothing]
    link-preload-style       nothing            [server saw nothing]
    link-modulepreload       link-appended only [server saw nothing]
    link-prefetch            nothing            [server saw nothing]
    link-preload-parsed      nothing            [server saw nothing, though
                                                 stderr printed `⤷ preload js`]
    link-preload-404         nothing            [server saw nothing]
    script-dynamic-load      script-load, script-ran=number            <- control
    script-dynamic-404       script-error                              <- control
    script-src-empty         nothing            <- no `error` for an empty src
    script-parsed-load       parsed-script-ran=number, no load event
    script-parsed-load-listener  slow-script-ran=number, neither listener form
    link-parsed-stylesheet-load  the sheet loads, neither listener form fires
    style-element-load       style-appended sheet=no, style-sheet-later=no
    mutationobserver-script-inserted   mo-added DIV                    <- control
    mutationobserver-parser-inserted   mo-armed, parsed-script-2-ran, no record
    paint-timing-entries     first-paint + first-contentful-paint delivered,
                             but `window.PerformancePaintTiming` is undefined
    blocking-attribute       blocking-prop=undefined (attribute reflects)
    audio-source-ended       rendered length=44100, no `ended` at all
    audio-param-automation   gain-value=1, rendered nonzero=0/4410
    audio-oncomplete-throws  oncomplete-ran, render-resolved — the throw is gone
    audio-offline-suspend    offline-suspended currentTime=0 state=closed
    audio-context-state      every transition + statechange              <- control
    history-pushstate-url    after-push href=?psag-push search= (unresolved)
    history-go-popstate      popstate state=null, after-go search=
    document-write-markup    wrote-markup found=yes                      <- control
    document-write-script    wrote — and never written-script-ran
    document-open-write      open-threw TypeError: document.open is not a function

So: BUG-826 (link hints never fetched), BUG-827 (no mutation record for a
parser insertion), BUG-828 (Web Audio renders silence and swallows the
`oncomplete` throw), BUG-829 (`pushState` writes an unresolved URL into
`location`), plus re-measurements that widened BUG-804 (element `load`
through `LoadObserver`) and BUG-568 (a written `<script>` never runs;
`document.open` still missing).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
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

#: Paths the probe server was asked for, per variant. A subresource request
#: that never arrives here is the strongest possible evidence: it does not
#: depend on the page being able to report anything.
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

    # ── <link> created from script ─────────────────────────────────────────
    # The control for the whole group: a `rel=stylesheet` link, the one kind
    # the shell is known to fetch. If this loads and the hint kinds below do
    # not, the defect is the hint, not script-created links as such.
    "link-stylesheet": ("""
<script>
var link = document.createElement("link");
link.rel = "stylesheet";
link.href = "psag-asset.css?stylesheet";
link.onload = function () { console.log("PROBE link-load rel=stylesheet"); };
link.onerror = function () { console.log("PROBE link-error rel=stylesheet"); };
document.head.appendChild(link);
console.log("PROBE link-appended rel=" + link.rel + " href=" + link.href);
</script>
""", "link-load rel=stylesheet + a request for psag-asset.css"),
    # `connection-allowlist/tentative/link_rel_preload.sub.window.js`: the link
    # is built in a `promise_test`, appended to `<head>`, and the test then
    # polls the server until the preloaded URL shows up there.
    "link-preload-script": ("""
<script>
var link = document.createElement("link");
link.rel = "preload";
link.as = "script";
link.href = "psag-asset.js?preload";
link.onload = function () { console.log("PROBE link-load rel=preload"); };
link.onerror = function () { console.log("PROBE link-error rel=preload"); };
document.head.appendChild(link);
console.log("PROBE link-appended rel=" + link.rel + " as=" + link.as);
</script>
""", "a request for psag-asset.js, then link-load rel=preload"),
    # The same shape with `as=style`, which `link_header_preload_allow` uses.
    "link-preload-style": ("""
<script>
var link = document.createElement("link");
link.rel = "preload";
link.as = "style";
link.href = "psag-asset.css?preload-style";
link.onload = function () { console.log("PROBE link-load rel=preload as=style"); };
link.onerror = function () { console.log("PROBE link-error rel=preload as=style"); };
document.head.appendChild(link);
</script>
""", "a request for psag-asset.css, then link-load"),
    # `shadow-dom/declarative/.../shadowrootadoptedstylesheets-modulepreload-*`
    # and `scripting-1/.../modulepreload-referrer-check.html`: `await` a
    # promise resolved by `link.onload` of a `rel=modulepreload` link.
    "link-modulepreload": ("""
<script>
var link = document.createElement("link");
link.rel = "modulepreload";
link.href = "psag-module.js?modulepreload";
link.onload = function () { console.log("PROBE link-load rel=modulepreload"); };
link.onerror = function () { console.log("PROBE link-error rel=modulepreload"); };
document.head.appendChild(link);
console.log("PROBE link-appended rel=" + link.rel);
</script>
""", "a request for psag-module.js, then link-load rel=modulepreload"),
    # `connection-allowlist/tentative/link_rel_prefetch.sub.window.js`.
    "link-prefetch": ("""
<script>
var link = document.createElement("link");
link.rel = "prefetch";
link.href = "psag-asset.js?prefetch";
link.onload = function () { console.log("PROBE link-load rel=prefetch"); };
link.onerror = function () { console.log("PROBE link-error rel=prefetch"); };
document.head.appendChild(link);
</script>
""", "a request for psag-asset.js, then link-load rel=prefetch"),
    # The parser-inserted counterpart, as the second control: the preload
    # scanner does log `⤷ preload js [medium] <URL>` for markup like this, so
    # if the request arrives here and not above, the gap is the script path.
    "link-preload-parsed": ("""
<link rel="preload" as="script" href="psag-asset.js?parsed-preload">
<script>
var link = document.querySelector("link[rel=preload]");
link.onload = function () { console.log("PROBE link-load parsed"); };
link.onerror = function () { console.log("PROBE link-error parsed"); };
console.log("PROBE parsed-link rel=" + link.rel + " as=" + link.as);
</script>
""", "a request for psag-asset.js (preload scanner), then link-load parsed"),
    # A 404 target: `link.onerror` is what `modulepreload-failure` and the
    # `*_deny` half of `connection-allowlist` wait for.
    "link-preload-404": ("""
<script>
var link = document.createElement("link");
link.rel = "preload";
link.as = "script";
link.href = "psag-missing.js?preload-404";
link.onload = function () { console.log("PROBE link-load 404"); };
link.onerror = function () { console.log("PROBE link-error 404"); };
document.head.appendChild(link);
</script>
""", "link-error 404"),

    # ── <script> created from script ───────────────────────────────────────
    # `execution-timing/023.html`, `change-src-attr-prepare-a-script.html`:
    # the script element is created, appended, and its `load` awaited.
    "script-dynamic-load": ("""
<script>
var s = document.createElement("script");
s.src = "psag-asset.js?dynamic";
s.onload = function () { console.log("PROBE script-load"); };
s.onerror = function () { console.log("PROBE script-error"); };
document.body.appendChild(s);
console.log("PROBE script-appended");
setTimeout(function () { console.log("PROBE script-ran=" + (typeof window.psagRan)); }, 500);
</script>
""", "script-ran=number, script-load"),
    # `fetch-src/error-*.html`: a 404 src must produce `error` on the element.
    "script-dynamic-404": ("""
<script>
var s = document.createElement("script");
s.src = "psag-missing.js?dynamic-404";
s.onload = function () { console.log("PROBE script-load"); };
s.onerror = function () { console.log("PROBE script-error type=" + arguments.length); };
document.body.appendChild(s);
</script>
""", "script-error"),
    # `fetch-src/empty.html` / `empty-with-base.html`: `src=""` must fire
    # `error` asynchronously and must not load the document itself.
    "script-src-empty": ("""
<script>
var s = document.createElement("script");
var queued = false;
s.onerror = function (ev) { console.log("PROBE script-error queued=" + queued + " type=" + ev.type); };
s.onload = function () { console.log("PROBE script-load"); };
s.setAttribute("src", "");
document.body.appendChild(s);
queued = true;
</script>
""", "script-error queued=true type=error"),
    # The parser-inserted control: markup `<script src>` does run, so if its
    # `load` is silent too the gap is the event and not the fetch.
    "script-parsed-load": ("""
<script src="psag-asset.js?parsed" onload="console.log('PROBE parsed-script-load')"
        onerror="console.log('PROBE parsed-script-error')"></script>
<script>
setTimeout(function () { console.log("PROBE parsed-script-ran=" + (typeof window.psagRan)); }, 500);
</script>
""", "parsed-script-ran=number, parsed-script-load"),
    # The same question asked through `addEventListener` rather than the
    # content attribute, and asked *before* the resource can have arrived:
    # `/psag-slow.js` is held for a second by the probe server, so the listener
    # below is certainly attached while the load is still in flight.
    "script-parsed-load-listener": ("""
<script id="psag-slow-script" src="psag-slow.js?parsed-listener"></script>
<script>
var s = document.getElementById("psag-slow-script");
console.log("PROBE found-parsed-script=" + (s ? "yes" : "no"));
if (s) {
    s.addEventListener("load", function () { console.log("PROBE parsed-script-load-listener"); });
    s.onload = function () { console.log("PROBE parsed-script-onload"); };
}
setTimeout(function () { console.log("PROBE slow-script-ran=" + (typeof window.psagSlowRan)); }, 2000);
</script>
""", "parsed-script-load-listener + parsed-script-onload"),
    # `remove-attr-stylesheet-link-keeps-blocking.html`: the same for a
    # parser-inserted `<link rel=stylesheet>`, also held by the server.
    "link-parsed-stylesheet-load": ("""
<link id="psag-slow-link" rel="stylesheet" href="psag-slow.css?parsed-listener">
<script>
var l = document.getElementById("psag-slow-link");
console.log("PROBE found-parsed-link=" + (l ? "yes" : "no"));
if (l) {
    l.addEventListener("load", function () { console.log("PROBE parsed-link-load-listener"); });
    l.onload = function () { console.log("PROBE parsed-link-onload"); };
    l.onerror = function () { console.log("PROBE parsed-link-onerror"); };
}
</script>
""", "parsed-link-load-listener + parsed-link-onload"),
    # `script-inserted-style-element.html` / `remove-attr-style-keeps-blocking`
    # wait for the `load` event of a `<style>` element, which has no
    # subresource at all — it fires once the sheet is parsed.
    "style-element-load": ("""
<script>
var st = document.createElement("style");
st.textContent = "#psag-none { color: rgb(4, 5, 6); }";
st.addEventListener("load", function () { console.log("PROBE style-load"); });
st.onerror = function () { console.log("PROBE style-error"); };
document.head.appendChild(st);
console.log("PROBE style-appended sheet=" + (st.sheet ? "yes" : "no"));
setTimeout(function () { console.log("PROBE style-sheet-later=" + (st.sheet ? "yes" : "no")); }, 500);
</script>
""", "style-load, style-sheet-later=yes"),

    # ── render-blocking ────────────────────────────────────────────────────
    # `html/dom/render-blocking/*` all end in the same helper wait:
    # `await test.step_wait(() => performance.getEntriesByType('paint').length)`
    # preceded by `assert_implements(window.PerformancePaintTiming)`.
    "paint-timing-entries": ("""
<div style="height: 200px; background: linear-gradient(red, blue)">Some text</div>
<script>
console.log("PROBE PerformancePaintTiming=" + typeof window.PerformancePaintTiming);
var seen = 0;
try {
    var po = new PerformanceObserver(function (list) {
        list.getEntries().forEach(function (e) {
            console.log("PROBE paint-observer " + e.name + " startTime=" + (e.startTime > 0));
        });
    });
    po.observe({ type: "paint", buffered: true });
    console.log("PROBE paint-observe-ok");
} catch (e) { console.log("PROBE paint-observe-threw " + e); }
setInterval(function () {
    var n = performance.getEntriesByType("paint").length;
    if (n !== seen) { seen = n; console.log("PROBE paint-entries n=" + n); }
}, 200);
</script>
""", "paint-observer first-paint / first-contentful-paint, paint-entries n=2"),
    # The control for the observer itself: a node appended from script. If
    # this is reported and the parser-inserted one below is not, the gap is
    # the parser's mutation records and not `MutationObserver`.
    "mutationobserver-script-inserted": ("""
<script>
new MutationObserver(function (records) {
    records.forEach(function (r) {
        r.addedNodes.forEach(function (n) {
            console.log("PROBE mo-added " + n.nodeName + " id=" + (n.id || "-"));
        });
    });
}).observe(document.documentElement, { childList: true, subtree: true });
setTimeout(function () {
    var d = document.createElement("div");
    d.id = "psag-js-div";
    document.body.appendChild(d);
    console.log("PROBE js-div-appended");
}, 100);
</script>
""", "js-div-appended, mo-added DIV id=psag-js-div"),
    # `remove-attr-*-keeps-blocking.html` waits, before anything else, for a
    # `MutationObserver` record about the *parser-inserted* element below it
    # (`support/test-render-blocking.js::nodeInserted`).
    "mutationobserver-parser-inserted": ("""
<script>
new MutationObserver(function (records) {
    records.forEach(function (r) {
        r.addedNodes.forEach(function (n) {
            console.log("PROBE mo-added " + n.nodeName + " id=" + (n.id || "-"));
        });
    });
}).observe(document.documentElement, { childList: true, subtree: true });
console.log("PROBE mo-armed");
</script>
<div id="psag-parsed-div">parsed after the observer</div>
<script id="psag-parsed-script">console.log("PROBE parsed-script-2-ran");</script>
""", "mo-added DIV id=psag-parsed-div, mo-added SCRIPT id=psag-parsed-script"),
    # The attribute itself: `blocking="render"` must reflect as
    # `el.blocking` (a `DOMTokenList`), which three of the tests assign to.
    "blocking-attribute": ("""
<script id="psag-blocking" blocking="render">console.log("PROBE blocking-inline-ran");</script>
<script>
var s = document.getElementById("psag-blocking");
console.log("PROBE blocking-prop=" + typeof s.blocking + " value=" + s.blocking);
console.log("PROBE blocking-attr=" + s.getAttribute("blocking"));
try { s.blocking = ""; console.log("PROBE blocking-assign-ok"); }
catch (e) { console.log("PROBE blocking-assign-threw " + e); }
</script>
""", "blocking-prop=object value=render"),

    # ── webaudio ───────────────────────────────────────────────────────────
    # `the-audiobuffersourcenode-interface/audiosource-onended.html`: render an
    # `OfflineAudioContext` and `await` the source's `ended` event.
    "audio-source-ended": ("""
<script>
var ctx = new OfflineAudioContext(1, 44100, 44100);
var buf = ctx.createBuffer(1, 5512, 44100);
var src = ctx.createBufferSource();
src.buffer = buf;
src.connect(ctx.destination);
src.onended = function () { console.log("PROBE source-onended"); };
src.addEventListener("ended", function () { console.log("PROBE source-ended-listener"); });
src.start();
ctx.startRendering().then(function (rendered) {
    console.log("PROBE rendered length=" + rendered.length);
}, function (e) { console.log("PROBE render-rejected " + e); });
</script>
""", "source-onended, source-ended-listener, rendered length=44100"),
    # `the-audioparam-interface/audioparam-setValueAtTime.html` and siblings:
    # the whole point of the file is that the rendered samples carry the
    # automation curve, so a silent buffer can only fail — silently, since the
    # audit framework reports through a promise.
    "audio-param-automation": ("""
<script>
var ctx = new OfflineAudioContext(1, 4410, 44100);
var osc = ctx.createOscillator();
var gain = ctx.createGain();
osc.connect(gain);
gain.connect(ctx.destination);
gain.gain.setValueAtTime(0.25, 0);
gain.gain.linearRampToValueAtTime(1.0, 0.05);
console.log("PROBE gain-value=" + gain.gain.value);
osc.start(0);
ctx.startRendering().then(function (rendered) {
    var data = rendered.getChannelData(0);
    var nonzero = 0;
    for (var i = 0; i < data.length; i++) { if (data[i] !== 0) nonzero++; }
    console.log("PROBE rendered nonzero=" + nonzero + "/" + data.length);
}, function (e) { console.log("PROBE render-rejected " + e); });
</script>
""", "rendered nonzero=4410/4410"),
    # `the-audiocontext-interface/audiocontext-suspend-resume-close.html`:
    # `state` transitions and the `statechange` event.
    "audio-context-state": ("""
<script>
var ctx = new AudioContext();
ctx.onstatechange = function () { console.log("PROBE statechange state=" + ctx.state); };
console.log("PROBE state0=" + ctx.state);
ctx.suspend().then(function () { console.log("PROBE suspended state=" + ctx.state); },
                   function (e) { console.log("PROBE suspend-rejected " + e); });
setTimeout(function () {
    ctx.resume().then(function () { console.log("PROBE resumed state=" + ctx.state); },
                      function (e) { console.log("PROBE resume-rejected " + e); });
}, 100);
setTimeout(function () {
    ctx.close().then(function () { console.log("PROBE closed state=" + ctx.state); },
                     function (e) { console.log("PROBE close-rejected " + e); });
}, 200);
</script>
""", "statechange suspended/running/closed for each transition"),
    # `webaudio/resources/audioparam-testing.js::createAudioGraphAndTest` runs
    # its whole comparison from `context.oncomplete`, and the shim calls that
    # handler inside `try { … } catch (e) {}` (`web_audio.rs`,
    # `OfflineAudioContext.prototype.startRendering`). If a throw there is
    # swallowed, an audit task that fails on the silent buffer never finishes
    # and never says why — which is the difference between FAIL and TIMEOUT
    # for the five `audioparam-*` ids.
    "audio-oncomplete-throws": ("""
<script>
var ctx = new OfflineAudioContext(1, 128, 44100);
ctx.oncomplete = function (e) {
    console.log("PROBE oncomplete-ran length=" + (e.renderedBuffer ? e.renderedBuffer.length : "?"));
    throw new Error("boom");
};
ctx.startRendering().then(function () { console.log("PROBE render-resolved"); },
                          function (e) { console.log("PROBE render-rejected " + e.message); });
window.addEventListener("error", function (e) { console.log("PROBE win-error " + e.message); });
</script>
""", "oncomplete-ran, then the throw surfaces somewhere (win-error / render-rejected)"),
    # `the-analysernode-interface/test-analyser-resume-after-suspended.html`:
    # `OfflineAudioContext.suspend(t)` must actually stop the render at `t` and
    # hand control back, which is what the test resumes from.
    "audio-offline-suspend": ("""
<script>
var ctx = new OfflineAudioContext(1, 44100, 44100);
var osc = ctx.createOscillator();
osc.connect(ctx.destination);
osc.start();
ctx.suspend(0.5).then(function () {
    console.log("PROBE offline-suspended currentTime=" + ctx.currentTime + " state=" + ctx.state);
    ctx.resume();
}, function (e) { console.log("PROBE offline-suspend-rejected " + e); });
ctx.startRendering().then(function (r) { console.log("PROBE offline-rendered length=" + r.length); },
                          function (e) { console.log("PROBE offline-render-rejected " + e); });
</script>
""", "offline-suspended currentTime=0.5, then offline-rendered length=44100"),

    # ── history traversal ──────────────────────────────────────────────────
    # `html/browsers/history/the-history-interface/005.html` and siblings:
    # two `pushState`s, then `history.go(-1)` and a `popstate` listener.
    "history-go-popstate": ("""
<script>
window.onpopstate = function (e) { console.log("PROBE popstate state=" + JSON.stringify(e.state)); };
window.addEventListener("popstate", function (e) { console.log("PROBE popstate-listener"); });
history.pushState({ n: 1 }, "", "?psag1");
history.pushState({ n: 2 }, "", "?psag2");
console.log("PROBE pushed length=" + history.length + " search=" + location.search);
setTimeout(function () {
    history.go(-1);
    setTimeout(function () {
        console.log("PROBE after-go search=" + location.search + " state=" + JSON.stringify(history.state));
    }, 300);
}, 200);
</script>
""", "popstate state={\"n\":1}, after-go search=?psag1"),
    # `the-history-interface/008.html`…`010.html` assert on the URL the
    # traversal lands on, so what `pushState` does to `location` is measured
    # on its own, before any traversal.
    "history-pushstate-url": ("""
<script>
console.log("PROBE before href=" + location.href + " search=" + location.search + " length=" + history.length);
history.pushState({ n: 1 }, "", "?psag-push");
console.log("PROBE after-push href=" + location.href + " search=" + location.search
            + " state=" + JSON.stringify(history.state) + " length=" + history.length);
history.replaceState({ n: 2 }, "", "?psag-replace");
console.log("PROBE after-replace href=" + location.href + " state=" + JSON.stringify(history.state));
history.pushState({ n: 3 }, "", "psag-relative.html");
console.log("PROBE after-relative href=" + location.href);
</script>
""", "after-push search=?psag-push state={\"n\":1}"),
    # `back-pushstate-back-history-state.html`: `history.back()` is the same
    # traversal by another name.
    "history-back": ("""
<script>
window.addEventListener("popstate", function (e) { console.log("PROBE popstate-listener state=" + JSON.stringify(e.state)); });
history.pushState({ n: 1 }, "", "?psagb");
setTimeout(function () {
    history.back();
    setTimeout(function () { console.log("PROBE after-back search=" + location.search); }, 300);
}, 200);
</script>
""", "popstate-listener state=null, after-back search="),

    # ── dynamic markup insertion ───────────────────────────────────────────
    # `html/webappapis/dynamic-markup-insertion/document-write/script_001.html`:
    # a `document.write` of a `<script>` during parsing must run it.
    "document-write-script": ("""
<script>
document.write("<scr" + "ipt>console.log('PROBE written-script-ran');</scr" + "ipt>");
console.log("PROBE wrote");
</script>
""", "wrote, written-script-ran"),
    # `document-write/write-active-document.html` and the `002`-style files:
    # plain markup written during parsing, no script involved.
    "document-write-markup": ("""
<script>
document.write("<p id='psag-written'>written</p>");
console.log("PROBE wrote-markup found=" + (document.getElementById("psag-written") ? "yes" : "no"));
setTimeout(function () {
    console.log("PROBE later found=" + (document.getElementById("psag-written") ? "yes" : "no")
                + " writeln=" + typeof document.writeln);
}, 300);
</script>
""", "wrote-markup found=yes"),
    # `opening-the-input-stream/*`: `document.open()` on the *same* document
    # must blow the current document away and start a fresh stream.
    "document-open-write": ("""
<script>
setTimeout(function () {
    try {
        document.open();
        document.write("<p id=psag-after>after open</p>");
        document.close();
        console.log("PROBE opened after=" + (document.getElementById("psag-after") ? "yes" : "no"));
    } catch (e) { console.log("PROBE open-threw " + e); }
}, 200);
</script>
""", "opened after=yes"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-20 probe: %(name)s</title>
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
    "psag-asset.js": "window.psagRan = (window.psagRan || 0) + 1;\n",
    "psag-module.js": "export const psag = 1;\n",
    "psag-asset.css": "#psag-none { color: rgb(1, 2, 3); }\n",
}

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every path asked for.

    `/psag-slow.*` is held for a second before answering — the stand-in for
    wptserve's `?pipe=trickle(d1)`, which every `render-blocking` test uses so
    that a listener attached after the element still beats the response.
    """

    def do_GET(self):  # noqa: N802 — http.server's own casing
        with _SERVED_LOCK:
            SERVED.append(self.path)
        if self.path.startswith("/psag-slow."):
            time.sleep(1.0)
            body = (b"window.psagSlowRan = 1;\n" if ".js" in self.path
                    else b"#psag-none { color: rgb(7, 8, 9); }\n")
            ctype = ("text/javascript" if ".js" in self.path else "text/css")
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
    log_path = os.path.join(REPO, ".tmp", f"psag-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.psag-{name}.html"],
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
        fetched = [p for p in SERVED if not p.startswith("/.psag-")]
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
        path = os.path.join(HERE, f".psag-{name}.html")
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
        print(f"{'variant':34s} {'ticks':>5s}  {'expected':56s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers, fetched = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            if fetched:
                seen += "   [server saw: " + ", ".join(sorted(set(fetched))) + "]"
            else:
                seen += "   [server saw: nothing]"
            print(f"{name:34s} {ticks:5d}  {VARIANTS[name][1]:56s} {seen}")
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
              "requested, whatever the page believes.")
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
