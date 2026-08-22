#!/usr/bin/env python3
"""WPT-RUN-6 slice 19: three more silent waits behind the densest residual.

After slice 18 the unexplained TIMEOUT residual of the WPT-RUN-5 snapshot is
543 ids. Three shapes stand out, and every one of them is a *wait* rather than
an error, so none can be read off the browser's output:

* 38 ids are `streams/*` + `encoding/streams/*` — `.any.html` files whose body
  is dozens of `promise_test`s over `ReadableStream`/`WritableStream`/
  `TransformStream`;
* 18 ids are `css/css-scroll-anchoring` (10 of that category's 11 TIMEOUTs) and
  `css/css-scroll-snap/snap-after-relayout` (8 of 20), which scroll
  programmatically and then `await` a `scroll` or `scrollend` event;
* 13 ids are `webmessaging` — all seven async `with-options` tests (7 of 7),
  plus `with-ports`/`without-ports`.

A fourth group (media elements) and a re-measurement of the window-level error
events were added while running, because the first two answers moved the
suspicion: the streams hypothesis this probe started from — "the pull model is
a sketch, so a second `read()` never settles", straight out of the shim's own
header comment — is **refuted** by the `stream-pull-demand` control below, and
the scroll one had to be re-measured after the painting trap described further
down.

Same harness as slices 15/17/18 and for the reasons recorded in `CLAUDE.md`:
one browser process per page, served over http (never `file://`), evidence read
off the browser's own stderr rather than through an MCP `eval`, and a 500 ms
`setInterval` tick so "the page is alive and heard nothing" is separable from
"the page died".

Measured 2026-08-21 (dev-release, Linux, commit `6e60c8aa8`, `--seconds 6`;
every variant ticked 11 times, i.e. no page died on us):

    variant                    markers seen
    control                    raf, timeout                          <- the control
    stream-read-queued         read0 v=a, read1 v=b, read2 done      <- the control
    stream-pull-demand         pull 0, read0 v=0 … pull 3, read3 v=3  <- the control
    stream-async-start         read0 v=late, closed                  <- the control
    stream-async-iter          asynciter=undefined, iter-threw TypeError
    stream-byob                byob-reader=ReadableStreamDefaultReader,
                               has-byob-request=undefined
    stream-writable            write/ready/close/closed all resolved  <- the control
    stream-transform           transform-read0 v=A … read2 done       <- the control
    stream-pipeto              pipe-resolved sink=a, pipe2-rejected boom
    stream-textdecoder         decode0 v=€           <- and `done` never comes
    stream-close-throws        write-resolved, close-rejected boom
                                              <- `writer.closed` never settles
    stream-write-throws        write-rejected boom   <- same for `closed`
    stream-abort               abort-resolved        <- sink `abort()` never ran
    stream-transform-close     tclose-read0 v=a, tclose-read1 done    <- the control
    stream-tee                 tee-locked=false      <- and neither branch reads
    scroll-window              onscrollend-in-window=false,
                               after-scrollTo y=300, after-scrollBy y=400
                                              <- it scrolls; no `scroll` event
    scroll-window-smooth       after-smooth y=300    <- same
    scroll-element-scrollto    el-scroll top=250, after-scrollTo top=250
                                              <- element `scroll` works…
    scroll-element-scrolltop   el-scroll top=400, after-assign top=400
                                              <- …either way, but no `scrollend`
    scroll-intoview            after-intoView y=0    <- no page fallback at all
    scroll-page-metrics        innerHeight=undefined, docEl.scrollHeight=0,
                               scrollingElement=null   (BUG-529/BUG-475/BUG-525)
    media-volumechange         audio-volumechange volume=0.5 …
                               video-defaults readyState=4 networkState=undefined
                                              <- and no video-volumechange ever
    media-resource-selection   video-loadedmetadata (before any src),
                               networkState=undefined, currentSrc=undefined
    window-error-events        (nothing at all)      <- BUG-716/BUG-591
    postmessage-star           message-onmessage/-listener data=x     <- the control
    postmessage-1arg           (nothing at all)
    postmessage-dict           (nothing at all)
    postmessage-dict-origin    (nothing at all)
    postmessage-url-origin     sent target=…//       <- and the message is dropped
    postmessage-transfer       (nothing at all)
    postmessage-legacy-ports   message ports=undefined data=x

One trap is worth stating, because the first run of this probe fell into it:
the tall spacer of every scroll page has to **paint** something. The shell
derives `content_height` from the display list, so an unpainted
`<div style="height:4000px">` leaves `max_scroll()` at 0 and the page really
does not scroll — which reads exactly like the defect under test. With a
gradient on the spacer, `window.scrollTo` moves the page correctly and the
finding narrows to the missing event.

So, of the shapes measured here:

1. **A programmatic page scroll dispatches no `scroll` event** (BUG-821) —
   `fire_window_scroll` has exactly one caller in the whole workspace, the
   mouse-wheel branch of `main.rs`. `scrollIntoView` additionally never falls
   back to scrolling the page.
2. **`scrollend` is dispatched by nothing at all** (BUG-822) — there is no
   `_lumen_fire_scrollend*` to match the `scroll` pair.
3. **Streams settle promises on the happy path only** (BUG-823) and are
   missing `tee`/BYOB/async-iteration/`TextDecoderStream` close (BUG-824).
4. **`<video>` is a thinner stub than `<audio>`** (BUG-825).
5. **`window.postMessage` supports only the legacy string overload**
   (already filed as BUG-717) — the one-argument form and the
   `WindowPostMessageOptions` dictionary both drop the message silently, and
   `transfer` never produces `e.ports`.
6. **Window-level `error`/`unhandledrejection` never fire** — BUG-716/BUG-591,
   re-measured here because three markers in `timeout_audit.py` rest on it.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
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

    # ── streams ────────────────────────────────────────────────────────────
    # The control for every stream variant: a source that enqueues everything
    # it will ever have inside `start()`. If this works and the demand-driven
    # shapes below do not, the defect is the pull model and not streams.
    "stream-read-queued": ("""
<script>
var rs = new ReadableStream({ start: function (c) { c.enqueue("a"); c.enqueue("b"); c.close(); } });
var reader = rs.getReader();
function step(n) {
    reader.read().then(function (r) {
        console.log("PROBE read" + n + " " + (r.done ? "done" : "v=" + r.value));
        if (!r.done && n < 4) step(n + 1);
    }, function (e) { console.log("PROBE read" + n + "-rejected " + e); });
}
step(0);
</script>
""", "read0 v=a, read1 v=b, read2 done"),
    # `streams/readable-streams/general.any.js` and most of its siblings build
    # exactly this: an empty queue plus a `pull` the implementation is required
    # to call again whenever a read finds nothing.
    "stream-pull-demand": ("""
<script>
var pulls = 0;
var rs = new ReadableStream({
    pull: function (c) { console.log("PROBE pull " + pulls); c.enqueue(pulls++); }
});
var reader = rs.getReader();
function step(n) {
    reader.read().then(function (r) {
        console.log("PROBE read" + n + " " + (r.done ? "done" : "v=" + r.value));
        if (!r.done && n < 3) step(n + 1);
    }, function (e) { console.log("PROBE read" + n + "-rejected " + e); });
}
step(0);
</script>
""", "pull 0..3 interleaved with read0 v=0 … read3 v=3"),
    # A source that enqueues from a timer — the shape every `piping` and
    # `transform-streams` test uses to model a slow producer.
    "stream-async-start": ("""
<script>
var rs = new ReadableStream({
    start: function (c) {
        setTimeout(function () { c.enqueue("late"); c.close(); }, 50);
    }
});
var reader = rs.getReader();
reader.read().then(function (r) {
    console.log("PROBE read0 " + (r.done ? "done" : "v=" + r.value));
}, function (e) { console.log("PROBE read0-rejected " + e); });
reader.closed.then(function () { console.log("PROBE closed"); },
                   function (e) { console.log("PROBE closed-rejected " + e); });
</script>
""", "read0 v=late, closed"),
    # `for await (const chunk of stream)` — `ReadableStream[Symbol.asyncIterator]`.
    "stream-async-iter": ("""
<script type="module">
var rs = new ReadableStream({ start: function (c) { c.enqueue("a"); c.close(); } });
console.log("PROBE asynciter=" + typeof rs[Symbol.asyncIterator]);
(async function () {
    try {
        for await (const chunk of rs) { console.log("PROBE iter " + chunk); }
        console.log("PROBE iter-done");
    } catch (e) { console.log("PROBE iter-threw " + e); }
})();
</script>
""", "asynciter=function, iter a, iter-done"),
    # `streams/readable-byte-streams/*`: a byte stream plus a BYOB reader.
    "stream-byob": ("""
<script>
try {
    var rs = new ReadableStream({
        type: "bytes",
        pull: function (c) { c.enqueue(new Uint8Array([1, 2, 3])); }
    });
    console.log("PROBE byob-ctor-ok");
    try {
        var reader = rs.getReader({ mode: "byob" });
        console.log("PROBE byob-reader=" + reader.constructor.name);
        reader.read(new Uint8Array(3)).then(function (r) {
            console.log("PROBE byob-read " + (r.done ? "done" : "n=" + r.value.length));
        }, function (e) { console.log("PROBE byob-read-rejected " + e); });
    } catch (e) { console.log("PROBE byob-getReader-threw " + e); }
} catch (e) { console.log("PROBE byob-ctor-threw " + e); }
console.log("PROBE has-byob-request=" + (typeof ReadableStreamBYOBReader));
</script>
""", "byob-ctor-ok, byob-reader=ReadableStreamBYOBReader, byob-read n=3"),
    # `streams/writable-streams/*`: the sink's `write` is async and the writer
    # must expose `ready`/`closed` promises that settle with it.
    "stream-writable": ("""
<script>
var written = [];
var ws = new WritableStream({
    write: function (chunk) {
        written.push(chunk);
        return new Promise(function (res) { setTimeout(res, 20); });
    },
    close: function () { console.log("PROBE sink-close"); }
});
var writer = ws.getWriter();
writer.write("a").then(function () { console.log("PROBE write-resolved"); },
                       function (e) { console.log("PROBE write-rejected " + e); });
writer.ready.then(function () { console.log("PROBE ready-resolved"); },
                  function (e) { console.log("PROBE ready-rejected " + e); });
writer.close().then(function () { console.log("PROBE close-resolved written=" + written.join(",")); },
                    function (e) { console.log("PROBE close-rejected " + e); });
writer.closed.then(function () { console.log("PROBE closed-resolved"); },
                   function (e) { console.log("PROBE closed-rejected " + e); });
</script>
""", "write-resolved, ready-resolved, sink-close, close-resolved written=a"),
    # `streams/transform-streams/*`: pipeThrough a transform and read the far
    # end. The transform itself is async, as every one of those tests' is.
    "stream-transform": ("""
<script>
var ts = new TransformStream({
    transform: function (chunk, c) { c.enqueue(chunk.toUpperCase()); }
});
var rs = new ReadableStream({ start: function (c) { c.enqueue("a"); c.enqueue("b"); c.close(); } });
var out = rs.pipeThrough(ts);
var reader = out.getReader();
function step(n) {
    reader.read().then(function (r) {
        console.log("PROBE transform-read" + n + " " + (r.done ? "done" : "v=" + r.value));
        if (!r.done && n < 3) step(n + 1);
    }, function (e) { console.log("PROBE transform-read" + n + "-rejected " + e); });
}
step(0);
</script>
""", "transform-read0 v=A, transform-read1 v=B, transform-read2 done"),
    # `streams/piping/*`: the promise `pipeTo` returns must settle — resolve on
    # a clean close, reject when the source errors.
    "stream-pipeto": ("""
<script>
var sink = [];
var rs = new ReadableStream({ start: function (c) { c.enqueue("a"); c.close(); } });
var ws = new WritableStream({ write: function (chunk) { sink.push(chunk); } });
rs.pipeTo(ws).then(function () { console.log("PROBE pipe-resolved sink=" + sink.join(",")); },
                   function (e) { console.log("PROBE pipe-rejected " + e); });
var bad = new ReadableStream({ start: function (c) { c.error(new Error("boom")); } });
var ws2 = new WritableStream({ write: function () {} });
bad.pipeTo(ws2).then(function () { console.log("PROBE pipe2-resolved"); },
                     function (e) { console.log("PROBE pipe2-rejected " + e.message); });
</script>
""", "pipe-resolved sink=a, pipe2-rejected boom"),
    # `encoding/streams/decode-split-character.any.js`: a multi-byte character
    # split across two chunks must come out whole on the far side.
    "stream-textdecoder": ("""
<script>
var tds = new TextDecoderStream();
var writer = tds.writable.getWriter();
var reader = tds.readable.getReader();
function step(n) {
    reader.read().then(function (r) {
        console.log("PROBE decode" + n + " " + (r.done ? "done" : "v=" + r.value));
        if (!r.done && n < 3) step(n + 1);
    }, function (e) { console.log("PROBE decode" + n + "-rejected " + e); });
}
step(0);
writer.write(new Uint8Array([0xe2, 0x82]));
writer.write(new Uint8Array([0xac]));
writer.close();
</script>
""", "decode0 v=€, decode1 done"),
    # `streams/writable-streams/close.any.js`, 4th `promise_test` — the first
    # one of that file the engine does not finish. A sink whose `close()`
    # throws must reject both `writer.close()` and `writer.closed`.
    "stream-close-throws": ("""
<script>
var ws = new WritableStream({ close: function () { throw new Error("boom"); } });
var writer = ws.getWriter();
writer.write("y").then(function () { console.log("PROBE write-resolved"); },
                       function (e) { console.log("PROBE write-rejected " + e.message); });
writer.close().then(function () { console.log("PROBE close-resolved"); },
                    function (e) { console.log("PROBE close-rejected " + e.message); });
writer.closed.then(function () { console.log("PROBE closed-resolved"); },
                   function (e) { console.log("PROBE closed-rejected " + e.message); });
</script>
""", "close-rejected boom, closed-rejected boom"),
    # The same shape one step earlier: a sink whose `write()` throws.
    "stream-write-throws": ("""
<script>
var ws = new WritableStream({ write: function () { throw new Error("boom"); } });
var writer = ws.getWriter();
writer.write("y").then(function () { console.log("PROBE write-resolved"); },
                       function (e) { console.log("PROBE write-rejected " + e.message); });
writer.closed.then(function () { console.log("PROBE closed-resolved"); },
                   function (e) { console.log("PROBE closed-rejected " + e.message); });
</script>
""", "write-rejected boom, closed-rejected boom"),
    # `streams/writable-streams/aborting.any.js`: `abort()` resolves and
    # `closed` rejects with the reason.
    "stream-abort": ("""
<script>
var ws = new WritableStream({ write: function () {}, abort: function (r) { console.log("PROBE sink-abort " + r); } });
var writer = ws.getWriter();
writer.abort("why").then(function () { console.log("PROBE abort-resolved"); },
                         function (e) { console.log("PROBE abort-rejected " + e); });
writer.closed.then(function () { console.log("PROBE closed-resolved"); },
                   function (e) { console.log("PROBE closed-rejected " + e); });
</script>
""", "sink-abort why, abort-resolved, closed-rejected why"),
    # Closing the *writable* end of a TransformStream must close its readable
    # end — the shape `encoding/streams/decode-*.any.js` is built on, via
    # `TextDecoderStream` and `readableStreamToArray`.
    "stream-transform-close": ("""
<script>
var ts = new TransformStream({ transform: function (c, ctrl) { ctrl.enqueue(c); } });
var writer = ts.writable.getWriter();
var reader = ts.readable.getReader();
function step(n) {
    reader.read().then(function (r) {
        console.log("PROBE tclose-read" + n + " " + (r.done ? "done" : "v=" + r.value));
        if (!r.done && n < 3) step(n + 1);
    }, function (e) { console.log("PROBE tclose-read" + n + "-rejected " + e); });
}
step(0);
writer.write("a");
writer.close().then(function () { console.log("PROBE tclose-writer-closed"); },
                    function (e) { console.log("PROBE tclose-writer-rejected " + e); });
</script>
""", "tclose-read0 v=a, tclose-read1 done, tclose-writer-closed"),
    # `streams/readable-streams/tee.any.js`: `tee()` must leave the source
    # readable and feed both branches as chunks arrive.
    "stream-tee": ("""
<script>
var rs = new ReadableStream({ start: function (c) { setTimeout(function () { c.enqueue("late"); c.close(); }, 50); } });
var branches = rs.tee();
console.log("PROBE tee-locked=" + rs.locked);
branches[0].getReader().read().then(function (r) {
    console.log("PROBE tee0 " + (r.done ? "done" : "v=" + r.value));
}, function (e) { console.log("PROBE tee0-rejected " + e); });
branches[1].getReader().read().then(function (r) {
    console.log("PROBE tee1 " + (r.done ? "done" : "v=" + r.value));
}, function (e) { console.log("PROBE tee1-rejected " + e); });
</script>
""", "tee-locked=true, tee0 v=late, tee1 v=late"),

    # ── programmatic scroll ────────────────────────────────────────────────
    # `css/css-scroll-anchoring/*`: scroll the document and await `scroll`.
    # The position is re-read on a timer, because the request is queued for the
    # shell and applied on a later frame — a synchronous read right after the
    # call would report 0 even on an engine that scrolls correctly.
    "scroll-window": ("""
<div style="height: 4000px; background: linear-gradient(red, blue)"></div>
<script>
window.addEventListener("scroll", function () { console.log("PROBE win-scroll y=" + window.scrollY); });
window.addEventListener("scrollend", function () { console.log("PROBE win-scrollend y=" + window.scrollY); });
console.log("PROBE onscrollend-in-window=" + ("onscrollend" in window));
setTimeout(function () { window.scrollTo(0, 300); }, 100);
setTimeout(function () { console.log("PROBE after-scrollTo y=" + window.scrollY); }, 1000);
setTimeout(function () { window.scrollBy(0, 100); }, 1500);
setTimeout(function () { console.log("PROBE after-scrollBy y=" + window.scrollY); }, 2500);
</script>
""", "win-scroll x2, after-scrollTo y=300, after-scrollBy y=400"),
    # The same page scrolled the way a user does it, which is the one path that
    # does fire the event today — kept as the control that separates "the page
    # cannot scroll" from "a programmatic scroll fires no event".
    "scroll-window-smooth": ("""
<div style="height: 4000px; background: linear-gradient(red, blue)"></div>
<script>
window.addEventListener("scroll", function () { console.log("PROBE win-scroll y=" + window.scrollY); });
setTimeout(function () { window.scrollTo({ top: 300, behavior: "smooth" }); }, 100);
setTimeout(function () { console.log("PROBE after-smooth y=" + window.scrollY); }, 1500);
</script>
""", "win-scroll …, after-smooth y=300"),
    # `css/css-scroll-snap/snap-after-relayout/*`: the same on an element, via
    # `scrollTo`. Separate from the `scrollTop=` variant below on purpose —
    # they are different entry points and only one of them is expected to work.
    "scroll-element-scrollto": ("""
<div id="scroller" style="overflow-y: auto; height: 200px; width: 200px">
  <div style="height: 2000px; background: linear-gradient(red, blue)"></div>
</div>
<script>
var s = document.getElementById("scroller");
s.addEventListener("scroll", function () { console.log("PROBE el-scroll top=" + s.scrollTop); });
s.addEventListener("scrollend", function () { console.log("PROBE el-scrollend top=" + s.scrollTop); });
setTimeout(function () { s.scrollTo(0, 250); }, 100);
setTimeout(function () { console.log("PROBE after-scrollTo top=" + s.scrollTop); }, 1000);
</script>
""", "el-scroll top=250, el-scrollend, after-scrollTo top=250"),
    # The `scrollTop=` entry point, which is what the element half of the
    # residual would need if `scrollTo` were merely a wrapper gap.
    "scroll-element-scrolltop": ("""
<div id="scroller" style="overflow-y: auto; height: 200px; width: 200px">
  <div style="height: 2000px; background: linear-gradient(red, blue)"></div>
</div>
<script>
var s = document.getElementById("scroller");
s.addEventListener("scroll", function () { console.log("PROBE el-scroll top=" + s.scrollTop); });
s.addEventListener("scrollend", function () { console.log("PROBE el-scrollend top=" + s.scrollTop); });
setTimeout(function () { s.scrollTop = 400; }, 100);
setTimeout(function () { console.log("PROBE after-assign top=" + s.scrollTop); }, 1000);
</script>
""", "el-scroll top=400, el-scrollend, after-assign top=400"),
    # `scrollIntoView` is how the anchoring tests position their anchor.
    "scroll-intoview": ("""
<div style="height: 3000px; background: linear-gradient(red, blue)"></div>
<div id="target" style="height: 50px; background: blue"></div>
<div style="height: 3000px; background: linear-gradient(red, blue)"></div>
<script>
window.addEventListener("scroll", function () { console.log("PROBE win-scroll y=" + window.scrollY); });
setTimeout(function () {
    document.getElementById("target").scrollIntoView({ block: "center", behavior: "instant" });
}, 100);
setTimeout(function () { console.log("PROBE after-intoView y=" + window.scrollY); }, 1000);
</script>
""", "win-scroll …, after-intoView y > 0"),
    # Separates "the request never reaches the shell" from "the page has no
    # scrollable extent as far as the engine is concerned", and checks the
    # third entry point (`document.scrollingElement.scrollTop`) at the same
    # time — the one CSSOM-View defines page scrolling in terms of.
    "scroll-page-metrics": ("""
<div style="height: 4000px; background: linear-gradient(red, blue)"></div>
<script>
setTimeout(function () {
    var de = document.documentElement;
    console.log("PROBE metrics innerHeight=" + window.innerHeight
        + " docEl.scrollHeight=" + de.scrollHeight
        + " docEl.clientHeight=" + de.clientHeight
        + " scrollingElement=" + (document.scrollingElement ? document.scrollingElement.tagName : "null"));
    if (document.scrollingElement) {
        document.scrollingElement.scrollTop = 300;
    }
}, 100);
setTimeout(function () {
    console.log("PROBE after-scrollingElement y=" + window.scrollY
        + " docEl.scrollTop=" + document.documentElement.scrollTop);
}, 1000);
</script>
""", "scrollHeight ≈ 4000, after-scrollingElement y=300"),

    # ── media elements ─────────────────────────────────────────────────────
    # `html/semantics/embedded-content/media-elements/event_volumechange.html`
    # and kin: no resource is involved at all — setting `volume`/`muted` on a
    # detached element must queue a `volumechange` task. Deliberately without a
    # `src`, so the probe cannot trip the `<audio src>` deadlock (BUG-799).
    "media-volumechange": ("""
<script>
["audio", "video"].forEach(function (tag) {
    var e = document.createElement(tag);
    console.log("PROBE " + tag + "-defaults volume=" + e.volume + " muted=" + e.muted
        + " readyState=" + e.readyState + " networkState=" + e.networkState);
    e.onvolumechange = function () { console.log("PROBE " + tag + "-volumechange volume=" + e.volume); };
    e.addEventListener("volumechange", function () { console.log("PROBE " + tag + "-volumechange-listener"); });
    e.volume = 0.5;
    e.muted = true;
});
</script>
""", "audio-volumechange, video-volumechange (both twice)"),
    # `loading-the-media-resource/*`: the resource selection algorithm must run
    # on its own task and report through `loadstart`/`error`/`emptied` — the
    # tests here point at a URL that does not exist, so `error` is the correct
    # outcome and silence is the defect.
    "media-resource-selection": ("""
<script>
var v = document.createElement("video");
["loadstart", "error", "emptied", "abort", "durationchange", "loadedmetadata"].forEach(function (type) {
    v.addEventListener(type, function () { console.log("PROBE video-" + type + " networkState=" + v.networkState); });
});
v.src = ".ssmgap-missing.mp4";
document.body.appendChild(v);
console.log("PROBE video-src-set networkState=" + v.networkState + " currentSrc=" + v.currentSrc);
v.load();
console.log("PROBE video-load-called networkState=" + v.networkState);
</script>
""", "video-loadstart, video-error"),

    # ── window-level error / rejection events ──────────────────────────────
    # BUG-716/BUG-591 measured once more here, because three markers below
    # depend on it: neither a top-level throw nor a rejected promise reaches a
    # `window` listener, which is exactly how `testharness.js` reports both.
    "window-error-events": ("""
<script>
window.addEventListener("error", function (e) { console.log("PROBE win-error " + e.message); });
window.onerror = function (m) { console.log("PROBE win-onerror " + m); return true; };
window.addEventListener("unhandledrejection", function (e) { console.log("PROBE win-unhandledrejection " + e.reason); });
window.onunhandledrejection = function (e) { console.log("PROBE win-onunhandledrejection"); };
setTimeout(function () { Promise.reject(new Error("rejected")); }, 100);
setTimeout(function () { null.boom; }, 200);
</script>
""", "win-error / win-onerror, win-unhandledrejection"),

    # ── window.postMessage ─────────────────────────────────────────────────
    # The legacy two-argument form with `'*'`, which the shim does implement:
    # the control that separates "postMessage is broken" from "the options
    # overload is".
    "postmessage-star": ("""
<script>
window.addEventListener("message", function (e) { console.log("PROBE message-listener data=" + e.data + " origin=" + e.origin); });
window.onmessage = function (e) { console.log("PROBE message-onmessage data=" + e.data); };
postMessage("x", "*");
</script>
""", "message-onmessage data=x, message-listener data=x"),
    # `webmessaging/with-options/one-arg.html`: `targetOrigin` defaults to
    # `'/'` (same origin), so the message must still arrive.
    "postmessage-1arg": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message data=" + e.data); };
postMessage("x");
</script>
""", "message data=x"),
    # `no-target-origin.html` / `unknown-parameter.html`: the options-dictionary
    # overload.
    "postmessage-dict": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message data=" + e.data + " ports=" + (e.ports ? e.ports.length : "?")); };
postMessage("x", {});
</script>
""", "message data=x ports=0"),
    # `slash-origin.html`: the same overload with the origin spelled out.
    "postmessage-dict-origin": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message data=" + e.data); };
postMessage("x", { targetOrigin: "/" });
</script>
""", "message data=x"),
    # `host-specific-origin.html`: a targetOrigin that is a URL rather than a
    # bare origin — the spec parses it and compares origins, so the trailing
    # path must not matter.
    "postmessage-url-origin": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message origin=" + e.origin); };
postMessage("x", location.protocol + "//" + location.host + "//");
console.log("PROBE sent target=" + location.protocol + "//" + location.host + "//");
</script>
""", "message origin=<origin>"),
    # `message-channel-transferable.html`: ports handed over in `transfer`
    # must arrive as `e.ports`.
    "postmessage-transfer": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message ports=" + (e.ports ? e.ports.length : "undefined")); };
var ch = new MessageChannel();
postMessage("x", { targetOrigin: "*", transfer: [ch.port1, ch.port2] });
</script>
""", "message ports=2"),
    # `with-ports/002.html`: the legacy three-argument form.
    "postmessage-legacy-ports": ("""
<script>
window.onmessage = function (e) { console.log("PROBE message ports=" + (e.ports ? e.ports.length : "undefined") + " data=" + e.data); };
var ch = new MessageChannel();
postMessage("x", "*", [ch.port1]);
</script>
""", "message ports=1 data=x"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-19 probe: %(name)s</title>
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
    log_path = os.path.join(REPO, ".tmp", f"ssmgap-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.ssmgap-{name}.html"],
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
        page = f".ssmgap-{name}.html"
        path = os.path.join(HERE, page)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)

    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':26s} {'ticks':>5s}  {'expected':52s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            print(f"{name:26s} {ticks:5d}  {VARIANTS[name][1]:52s} "
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
