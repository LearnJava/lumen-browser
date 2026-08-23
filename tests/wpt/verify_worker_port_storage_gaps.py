#!/usr/bin/env python3
"""WPT-RUN-6 slice 26: shared workers, ports, messaging, storage quota.

The residual of slice 25 is 185 ids and no longer has a dominant cluster, but
it does have a dominant *neighbourhood*: 29 of those ids are a worker, a
`MessagePort`, a `postMessage` target origin or a `WebSocket` — the machinery
by which one JS global talks to another. This probe measures that
neighbourhood, each question against a live browser, on http, with the
probe's own server recording what was actually requested:

* `Test connect event for a shared worker`, `getting name`, `Test the name
  property of shared workers mixing constructor options and constructor
  strings`, `Web Workers: SharedWorker - same name, different URL`,
  `URL encoding, shared worker` (7 ids of `workers/`) all hang on the same
  first step: a `SharedWorker` that must reach its `onconnect` and answer over
  `port`. `sw-connect` and `sw-name` split that into "does the global start",
  "does the port deliver", "is one global shared by name" and "does a name
  collision on a different URL throw".
* `Test worker construction with the "module" worker type.` and its two
  siblings (`workers/modules/*-options-type.html`, plus the three
  `*.any.sharedworker-module.html` ids) — `worker-type` asks what
  `{type: 'module'}` does and whether the two invalid types throw the
  `TypeError` the same tests assert.
* `Test sending messages to workers with no port.` / `on a port` /
  `MessageChannel/MessagePort should not work after a worker self.close()` /
  `Entangled port is garbage collected, and the close event is fired.` (4 ids)
  — `worker-port` and `port-lifecycle`, which are about `e.ports` surviving
  the trip in either direction and about what `close()` settles.
* `resolving url with stuff in host-specific`, `resolving a same origin
  targetOrigin with trailing slash`, `no targetOrigin`, `unknown parameter`
  (6 ids of `webmessaging/`) — `wm-targetorigin` sends the exact seven
  argument shapes those tests send, so the answer is per-shape rather than
  "postMessage is broken" (BUG-717).
* `Throws QuotaExceededError when the quota has been exceeded` (4 ids of
  `webstorage/`) — `storage-quota` writes 1 KiB keys in a bounded loop.
  The tests' loop is unbounded, so an engine with no quota does not fail
  them, it wedges the browser for the rest of the shard.
* `sending 50 messages of size 65536 with backpressure applied should not
  hang` and `WebSockets: 20s inactivity after handshake` (4 ids) need a real
  server, so the probe carries a minimal RFC 6455 one: `ws-backpressure`
  sends what the test sends, `ws-idle` leaves the socket untouched and then
  uses it.

* `WebSockets: 20s inactivity after handshake` (2 ids) looked like a
  neighbour of the backpressure pair and is not: the runner allowed those
  60 s and the harness cut them at 10.2 s. `harness-timeout-meta` replays
  `WindowTestEnvironment.prototype.test_timeout`
  (`resources/testharness.js:225`) verbatim and reports which of its steps
  gives the wrong answer — `meta.content` is `undefined` although
  `getAttribute("content")` is not, because the wrapper's own
  `HTMLTemplateElement.content` getter shadows the reflection (BUG-796).

Same harness as slices 15/17-22/24/25 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died".

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
        [--binary target/dev-release/lumen] [--seconds 8] [--variant NAME]

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

#: Paths the probe server was asked for, with the request method. A worker
#: script that never appears here was never fetched, whatever the page says.
SERVED = []
_SERVED_LOCK = threading.Lock()

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-26 probe: __NAME__</title>
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
fetch("vwps-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
var w = new Worker("vwps-echo-worker.js");
w.onmessage = function (e) { console.log("PROBE ctl-worker " + e.data); };
w.postMessage("hello");
</script>
""", "raf+timeout+fetch-ok+ctl-worker echo:hello"),

    # ── shared workers ─────────────────────────────────────────────────────
    # `connect-event.html` asserts three things about the event the global
    # receives (`e.data === ''`, `e instanceof MessageEvent`,
    # `e.ports.length == 1`) and then answers over `e.ports[0]`. Every other
    # SharedWorker test in the residual needs that same round trip first, so
    # measure it on its own before anything about names.
    "sw-connect": ("""
<script>
var log = function (m) { console.log("PROBE swc-" + m); };
try {
    var w = new SharedWorker("vwps-connect-worker.js");
    log("constructed port=" + (w.port ? typeof w.port : "none") +
        " onerror-settable=" + ("onerror" in w));
    w.port.onmessage = function (e) { log("message " + JSON.stringify(e.data)); };
    w.port.start && w.port.start();
    setTimeout(function () { w.port.postMessage("ping"); log("pinged"); }, 400);
} catch (e) { log("throws " + e); }
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "swc-message [true,true,true] — connect fired and the port answers"),

    # `shared-worker-name-via-options.html` builds three SharedWorkers on the
    # same script — two with `{name}`, one with the bare string — and requires
    # all three to reach the *same* global (a counter that increments per
    # connection). `SharedWorkerGlobalScope/name/getting.html` reads
    # `self.name` back, and `URLMismatchError.htm` requires a same-name,
    # different-URL construction to throw.
    "sw-name": ("""
<script>
var log = function (m) { console.log("PROBE swn-" + m); };
try {
    var a = new SharedWorker("vwps-name-worker.js", { name: "my name" });
    var b = new SharedWorker("vwps-name-worker.js", { name: "my name" });
    var c = new SharedWorker("vwps-name-worker.js", "my name");
    [["a", a], ["b", b], ["c", c]].forEach(function (pair) {
        pair[1].port.onmessage = function (e) {
            log("msg-" + pair[0] + " " + JSON.stringify(e.data));
        };
        pair[1].port.start && pair[1].port.start();
        pair[1].port.postMessage("who");
    });
} catch (e) { log("ctor-throws " + e); }
setTimeout(function () {
    try {
        var d = new SharedWorker("vwps-name-worker2.js", "my name");
        log("mismatch-no-throw " + (d ? "constructed" : "null"));
    } catch (e) { log("mismatch-throws " + e.name + " " + e); }
}, 600);
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "swn-msg-a/b/c name='my name' counter 1,2,3 + swn-mismatch-throws URLMismatchError"),

    # `dedicated-worker-options-type.html` / `shared-worker-options-type.html`:
    # the default and `'classic'` must load, `'module'` must load a module
    # script, and `''`/`'unknown'` must throw TypeError *synchronously*.
    "worker-type": ("""
<script>
var log = function (m) { console.log("PROBE wt-" + m); };
function build(tag, opts) {
    try {
        var w = opts === undefined ? new Worker("vwps-echo-worker.js")
                                   : new Worker("vwps-echo-worker.js", opts);
        w.onmessage = function (e) { log("msg-" + tag + " " + e.data); };
        w.onerror = function (e) { log("err-" + tag + " " + (e.message || e)); };
        w.postMessage("hello");
        log("built-" + tag);
    } catch (e) { log("throws-" + tag + " " + e.name); }
}
build("default", undefined);
build("classic", { type: "classic" });
build("module", { type: "module" });
build("empty", { type: "" });
build("unknown", { type: "unknown" });
try {
    var s = new SharedWorker("vwps-connect-worker.js", { type: "module" });
    log("shared-module-built");
} catch (e) { log("shared-module-throws " + e.name); }
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "wt-msg-default/classic/module echo:hello + wt-throws-empty/unknown TypeError"),

    # `Worker-messageport.html`: a `MessageChannel` port handed to a worker in
    # the transfer list must arrive as `e.ports[0]` there, and a port the
    # worker creates must arrive back here the same way. BUG-717 recorded the
    # window->window half as dropping `transfer` entirely; the worker half is
    # a different call path (`worker.postMessage`) and has never been measured.
    "worker-port": ("""
<script>
var log = function (m) { console.log("PROBE wp-" + m); };
var w = new Worker("vwps-port-worker.js");
w.onmessage = function (e) {
    log("from-worker data=" + JSON.stringify(e.data) +
        " ports=" + (e.ports ? e.ports.length : "undefined"));
    if (e.ports && e.ports.length) {
        e.ports[0].onmessage = function (ev) { log("via-worker-port " + ev.data); };
        e.ports[0].start && e.ports[0].start();
        e.ports[0].postMessage("hi-from-page");
    }
};
w.onerror = function (e) { log("worker-error " + (e.message || e)); };
w.postMessage("noport");
setTimeout(function () {
    var ch = new MessageChannel();
    ch.port2.onmessage = function (e) { log("page-port " + e.data); };
    ch.port2.start && ch.port2.start();
    try {
        w.postMessage("port", [ch.port1]);
        log("sent-with-transfer");
    } catch (e) { log("transfer-throws " + e.name + " " + e); }
    setTimeout(function () { ch.port2.postMessage("ping"); log("pinged-page-port"); }, 300);
}, 500);
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "wp-from-worker ports=1 and wp-page-port pong — the transfer survives both ways"),

    # `worker-post-after-close.any.html` (a port must stop delivering once the
    # worker called `self.close()`) and
    # `message-channels/close-event/garbage-collected.tentative.any.html` (the
    # `close` event on an entangled port). Both are about what *settles*, which
    # is where every Streams-shaped defect in this engine has been so far.
    "port-lifecycle": ("""
<script>
var log = function (m) { console.log("PROBE pl-" + m); };
var ch = new MessageChannel();
log("has-onclose " + ("onclose" in ch.port1) + " has-close " + (typeof ch.port1.close));
ch.port1.onmessage = function (e) { log("p1 " + e.data); };
ch.port2.onmessage = function (e) { log("p2 " + e.data); };
ch.port1.addEventListener && ch.port1.addEventListener("close", function () { log("p1-close-event"); });
ch.port1.start && ch.port1.start();
ch.port2.start && ch.port2.start();
ch.port2.postMessage("to-p1");
setTimeout(function () {
    ch.port2.close();
    log("closed-p2");
    ch.port1.postMessage("after-close");
    setTimeout(function () { log("post-close-quiet"); }, 300);
}, 500);
var w = new Worker("vwps-selfclose-worker.js");
w.onmessage = function (e) { log("worker " + e.data); };
setTimeout(function () { w.postMessage("close-now"); }, 900);
setTimeout(function () { w.postMessage("after-self-close"); log("poked-closed-worker"); }, 1400);
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "pl-p1 to-p1, pl-worker closing, then nothing after close"),

    # ── window.postMessage target origins ──────────────────────────────────
    # The seven argument shapes the six residual `webmessaging` ids send. Each
    # one is a separate `postMessage` with its own tag, so the answer is a per
    # shape yes/no rather than one verdict about the API (BUG-717).
    "wm-targetorigin": ("""
<script>
var log = function (m) { console.log("PROBE wm-" + m); };
var got = [];
onmessage = function (e) { got.push(e.data); log("received " + e.data); };
window.addEventListener("message", function (e) { log("listener " + e.data); });
var origin = location.protocol + "//" + location.host;
function send(tag, args) {
    try { postMessage.apply(window, args); log("sent-" + tag); }
    catch (e) { log("throws-" + tag + " " + e.name + " " + e); }
}
send("star", ["star", "*"]);
send("exact", ["exact", origin]);
send("slash", ["slash", origin + "/"]);
send("path", ["path", origin + "/some/path?q=1"]);
send("selfslash", ["selfslash", "/"]);
send("one-arg", ["one-arg"]);
send("empty-options", ["empty-options", {}]);
send("unknown-option", ["unknown-option", { targetOrigin: "*", unknown: 1 }]);
send("transfer-option", ["transfer-option", { targetOrigin: "*", transfer: [] }]);
setTimeout(function () { log("summary " + JSON.stringify(got)); log("checked"); }, 1200);
</script>
""", "wm-received for all nine shapes"),

    # ── storage quota ──────────────────────────────────────────────────────
    # The four residual `webstorage` ids loop `setItem` until it throws. The
    # probe bounds the loop so a browser with no quota reports a number
    # instead of wedging, and reports the wall clock so "no quota" is
    # separable from "quota, but very large".
    "storage-quota": ("""
<script>
var log = function (m) { console.log("PROBE sq-" + m); };
var val = new Array(1025).join("x");
function fill(store, tag, limit) {
    var t0 = Date.now(), i = 0, err = null;
    try {
        store.clear();
        for (; i < limit; i++) { store.setItem("name" + i, val + i); }
    } catch (e) { err = e; }
    log(tag + " wrote=" + i + " ms=" + (Date.now() - t0) +
        " err=" + (err ? (err.name + ":" + err.message) : "none") +
        " length=" + store.length);
}
setTimeout(function () { fill(localStorage, "local", 20000); }, 200);
setTimeout(function () { fill(sessionStorage, "session", 20000); }, 400);
setTimeout(function () {
    try { localStorage.clear(); sessionStorage.clear(); } catch (e) {}
    log("checked");
}, 900);
</script>
""", "sq-local err=QuotaExceededError well before 20000 keys (20 MiB)"),

    # ── websockets ─────────────────────────────────────────────────────────
    # `send-many-64K-messages-with-backpressure.any.js` verbatim, against the
    # probe's own echo server: 50 messages of 65536 bytes, each answered with
    # its size. The test's own name says what a failure looks like — "should
    # not hang" — so the tick counter is half the measurement.
    "ws-backpressure": ("""
<script>
var log = function (m) { console.log("PROBE wb-" + m); };
var SIZE = 65536, COUNT = 50;
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/echo-size");
log("constructed readyState=" + ws.readyState);
var received = 0, t0 = Date.now();
ws.onopen = function () {
    log("open ms=" + (Date.now() - t0));
    var msg = new Uint8Array(SIZE), sent = 0;
    try {
        for (var i = 0; i < COUNT; i++) { ws.send(msg); sent++; }
        log("sent " + sent + " ms=" + (Date.now() - t0) +
            " buffered=" + ws.bufferedAmount);
    } catch (e) { log("send-throws after " + sent + " " + e.name + " " + e); }
};
ws.onmessage = function (e) {
    received++;
    if (received === 1 || received === COUNT) {
        log("received " + received + " data=" + e.data + " ms=" + (Date.now() - t0));
    }
    if (received === COUNT) { ws.close(); }
};
ws.onclose = function (e) { log("close clean=" + e.wasClean + " code=" + e.code); };
ws.onerror = function () { log("error received=" + received); };
setTimeout(function () { log("checked received=" + received); }, 5000);
</script>
""", "wb-received 50 and wb-close clean=true, with ticks running throughout"),

    # Which half of `ws-backpressure` hangs: one 64 KiB frame, or fifty of
    # them? Each send is followed by its own marker, so the log stops on the
    # send that never returns.
    "ws-send-sizes": ("""
<script>
var log = function (m) { console.log("PROBE ws-" + m); };
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/echo");
var sizes = [1, 1024, 16384, 65535, 65536, 131072];
ws.onmessage = function (e) { log("echo len=" + (e.data.length || e.data.byteLength)); };
ws.onerror = function () { log("error"); };
ws.onclose = function (e) { log("close code=" + e.code); };
ws.onopen = function () {
    log("open");
    sizes.forEach(function (n) {
        try {
            ws.send(new Uint8Array(n));
            log("sent-" + n);
        } catch (e) { log("send-throws-" + n + " " + e.name); }
    });
    log("all-sent");
    var many = 0;
    try {
        for (; many < 50; many++) { ws.send(new Uint8Array(1024)); }
        log("sent-50-small");
    } catch (e) { log("small-throws-after-" + many + " " + e.name); }
};
setTimeout(function () { log("checked"); }, 4000);
</script>
""", "ws-sent-1 … ws-sent-131072, ws-all-sent, ws-sent-50-small, ticks running"),

    # `{type:'module'}` was accepted above and the worker ran — but the script
    # it ran is valid classic JS, so that says nothing. A worker script whose
    # body is only valid as a module answers the question outright.
    "worker-module-syntax": ("""
<script>
var log = function (m) { console.log("PROBE wms-" + m); };
function build(tag, url, opts) {
    try {
        var w = opts ? new Worker(url, opts) : new Worker(url);
        w.onmessage = function (e) { log("msg-" + tag + " " + e.data); };
        w.onerror = function (e) { log("err-" + tag + " " + (e.message || e)); };
        setTimeout(function () { w.postMessage("poke"); }, 300);
    } catch (e) { log("throws-" + tag + " " + e.name); }
}
build("module", "vwps-module-worker.js", { type: "module" });
build("module-as-classic", "vwps-module-worker.js", undefined);
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "wms-msg-module imported:42 while wms-module-as-classic errors on the import"),

    # Does an exception thrown by a `MessagePort`/`Worker` message listener
    # reach `window.onerror`? It decides whether the assertion failures the
    # `sw-connect` variant found surface as a FAIL or keep the test hanging:
    # `testharness.js` reports a failed assertion inside `step_func_done`
    # through the `error` event (BUG-591, fixed 2026-08-22 for other paths).
    "listener-exception": ("""
<script>
var log = function (m) { console.log("PROBE le-" + m); };
window.onerror = function (msg) { log("window-onerror " + String(msg).slice(0, 60)); return false; };
window.addEventListener("error", function (e) { log("error-event " + String(e.message).slice(0, 60)); });
var ch = new MessageChannel();
ch.port1.onmessage = function () { log("port-listener-ran"); throw new Error("from-port"); };
ch.port1.start && ch.port1.start();
ch.port2.postMessage("x");
var w = new Worker("vwps-echo-worker.js");
w.onerror = function (e) { log("worker-onerror " + (e.message || e)); };
w.onmessage = function () { log("worker-listener-ran"); throw new Error("from-worker"); };
setTimeout(function () { w.postMessage("y"); }, 300);
setTimeout(function () { window.postMessage("z", "*"); }, 600);
window.addEventListener("message", function () { log("window-listener-ran"); throw new Error("from-window"); });
setTimeout(function () { log("checked"); }, 2500);
</script>
""", "le-window-onerror three times — one per throwing listener"),

    # `workers/modules/*-options-type.html` waits for a worker that posts
    # `LOADED` **on load**, without being poked first — and worker timers are
    # known to be flushed only on message dispatch (BUG-815). Does an
    # unsolicited `postMessage` from a worker reach the page at all?
    "worker-unsolicited-post": ("""
<script>
var log = function (m) { console.log("PROBE wu-" + m); };
var a = new Worker("vwps-onload-worker.js");
a.onmessage = function (e) { log("unsolicited " + e.data); };
a.onerror = function (e) { log("err-a " + (e.message || e)); };
var b = new Worker("vwps-onload-worker.js");
b.onmessage = function (e) { log("after-poke " + e.data); };
setTimeout(function () { b.postMessage("poke"); log("poked"); }, 800);
var c = new SharedWorker("vwps-onload-shared.js");
c.port.onmessage = function (e) { log("shared " + e.data); };
c.port.start && c.port.start();
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "wu-unsolicited LOADED without any poke"),

    # Why the *default*-type subtest of `dedicated-worker-options-type.html`
    # hangs although a plain worker works: its worker script guards its
    # `postMessage` with `self instanceof DedicatedWorkerGlobalScope`. This
    # variant asks the worker to name what its own global scope is.
    "worker-global-interfaces": ("""
<script>
var log = function (m) { console.log("PROBE wg-" + m); };
var w = new Worker("vwps-introspect-worker.js");
w.onmessage = function (e) { log("dedicated " + e.data); };
w.onerror = function (e) { log("err " + (e.message || e)); };
w.postMessage("poke");
var s = new SharedWorker("vwps-introspect-shared.js");
s.port.onmessage = function (e) { log("shared " + e.data); };
s.port.start && s.port.start();
setTimeout(function () { log("checked"); }, 3000);
</script>
""", "wg-dedicated instanceof=true with DedicatedWorkerGlobalScope defined"),

    # Why two `websockets/keeping-connection-open/001.html` ids time out at
    # 10 s although the runner allowed them 60 (`test_timeout: 60` in the
    # shard's `test_end`): the harness decides its own budget in
    # `WindowTestEnvironment.prototype.test_timeout`
    # (`resources/testharness.js:225`) by walking `getElementsByTagName`
    # ("meta") and reading `.name`/`.content` off each. This variant replays
    # that loop verbatim and, when it returns the wrong answer, reports which
    # of its three steps produced it.
    "harness-timeout-meta": ("""
<meta name=timeout content=long>
<script>
var log = function (m) { console.log("PROBE htm-" + m); };
var metas = document.getElementsByTagName("meta");
log("count " + metas.length);
for (var i = 0; i < metas.length; i++) {
    var m = metas[i];
    log("meta-" + i +
        " ctor=" + (m.constructor && m.constructor.name) +
        " name=" + JSON.stringify(m.name) +
        " content=" + JSON.stringify(m.content) +
        " getAttr-name=" + JSON.stringify(m.getAttribute("name")) +
        " getAttr-content=" + JSON.stringify(m.getAttribute("content")));
}
// resources/testharness.js:225 — WindowTestEnvironment.prototype.test_timeout
var verdict = "normal";
for (var i = 0; i < metas.length; i++) {
    if (metas[i].name === "timeout") {
        if (metas[i].content === "long") { verdict = "long"; }
        break;
    }
}
log("verdict " + verdict);
// Same question for a meta built from script and put in <head>, to separate
// "the parser did not keep it" from "the reflection does not answer".
var made = document.createElement("meta");
made.setAttribute("name", "timeout");
made.setAttribute("content", "long");
document.head.appendChild(made);
log("scripted ctor=" + (made.constructor && made.constructor.name) +
    " name=" + JSON.stringify(made.name) + " content=" + JSON.stringify(made.content));
log("in-head " + (document.head.getElementsByTagName("meta").length));
// Where the `undefined` comes from: an own accessor on the instance beats the
// reflection installed on HTMLMetaElement.prototype (dom.rs:14503).
var own = Object.getOwnPropertyDescriptor(made, "content");
var proto = Object.getOwnPropertyDescriptor(HTMLMetaElement.prototype, "content");
log("desc own=" + (own ? "accessor:" + typeof own.get : "none") +
    " proto=" + (proto ? "accessor:" + typeof proto.get : "none"));
if (proto && proto.get) { log("via-proto " + JSON.stringify(proto.get.call(made))); }
log("template-control " + JSON.stringify(String(document.createElement("template").content)));
log("checked");
</script>
""", "htm-verdict long — the harness grants a `timeout=long` test 60 s"),

    # Which send of the fifty is the one that never returns. The server
    # answers `/echo-size` with a 0.1 s delay per message, so the socket's
    # send buffer fills exactly the way the real test makes it fill.
    "ws-backpressure-steps": ("""
<script>
var log = function (m) { console.log("PROBE wbs-" + m); };
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/echo-size");
var msg = new Uint8Array(65536);
ws.onmessage = function (e) { log("echo " + e.data); };
ws.onopen = function () {
    log("open");
    for (var i = 0; i < 50; i++) {
        var t0 = Date.now();
        ws.send(msg);
        log("sent-" + i + " ms=" + (Date.now() - t0) + " buffered=" + ws.bufferedAmount);
    }
    log("all-sent");
};
setTimeout(function () { log("checked"); }, 5000);
</script>
""", "wbs-sent-0 … wbs-sent-49, wbs-all-sent, ticks running"),

    # `keeping-connection-open/001.html` in miniature: the real test idles for
    # 20 s, which no probe budget survives; 4 s is enough to catch a socket
    # that is torn down the moment it goes quiet.
    "ws-idle": ("""
<script>
var log = function (m) { console.log("PROBE wi-" + m); };
var ws = new WebSocket("ws://127.0.0.1:__WSPORT__/echo");
var events = [];
["open", "close", "error", "message"].forEach(function (name) {
    ws.addEventListener(name, function (e) {
        events.push(name);
        if (name !== "message") { log("event-" + name + " readyState=" + ws.readyState); }
    });
});
ws.onopen = function () {
    log("open");
    setTimeout(function () {
        log("idle-done readyState=" + ws.readyState + " events=" + events.join(","));
        ws.onmessage = function (e) { log("echo-after-idle " + e.data); };
        try { ws.send("test"); log("sent-after-idle"); }
        catch (e) { log("send-after-idle-throws " + e.name); }
    }, 4000);
};
setTimeout(function () { log("checked events=" + events.join(",")); }, 6000);
</script>
""", "wi-echo-after-idle test — the socket survives 4 s of silence"),
}

_MAX_MARKERS = 40

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages and worker scripts, recording every request."""

    protocol_version = "HTTP/1.1"

    def _record(self, method):
        with _SERVED_LOCK:
            SERVED.append(f"{method} {self.path}")

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


#: Worker scripts. They are ordinary files under `tests/wpt/`, written before
#: the run and removed after it, so the browser fetches them over the same
#: http origin as the page (a worker script cannot come from `file://`).
#:
#: They avoid `navigator`/`location` (BUG-776: neither is defined in a worker
#: global, and reading one throws on the first line) and report everything
#: through `postMessage`, never by throwing (BUG-813: an exception inside a
#: started worker reaches nobody).
ASSETS = {
    "vwps-asset.js": "window.vwpsRan = (window.vwpsRan || 0) + 1;\n",

    "vwps-echo-worker.js": """
self.onmessage = function (e) { self.postMessage("echo:" + e.data); };
""",

    # `connect-event.js` upstream, minus the harness: answer the three
    # questions the test asks about the connect event itself.
    "vwps-connect-worker.js": """
self.onconnect = function (e) {
    var port = e.ports[0];
    port.postMessage([e.data === '',
                      typeof MessageEvent !== 'undefined' && e instanceof MessageEvent,
                      e.ports.length === 1]);
    port.onmessage = function (ev) { port.postMessage(["pong", ev.data]); };
    port.start && port.start();
};
""",

    # `support/shared-name.js` upstream: one global per (url, name), so the
    # counter says whether three constructions reached one global or three.
    "vwps-name-worker.js": """
var counter = 0;
self.onconnect = function (e) {
    var port = e.ports[0];
    ++counter;
    port.postMessage({ counter: counter, name: self.name, script: "worker1" });
    port.start && port.start();
};
""",

    "vwps-name-worker2.js": """
self.onconnect = function (e) {
    e.ports[0].postMessage({ script: "worker2", name: self.name });
};
""",

    # `support/Worker-messageport.js` upstream in miniature.
    "vwps-port-worker.js": """
self.onmessage = function (e) {
    if (e.data === "noport") {
        var ch = new MessageChannel();
        ch.port1.onmessage = function (ev) { ch.port1.postMessage("pong:" + ev.data); };
        ch.port1.start && ch.port1.start();
        self.postMessage("made-port", [ch.port2]);
        return;
    }
    if (e.data === "port") {
        var ports = e.ports || [];
        self.postMessage("saw-ports:" + ports.length);
        if (ports.length) {
            ports[0].onmessage = function (ev) { ports[0].postMessage("pong:" + ev.data); };
            ports[0].start && ports[0].start();
        }
        return;
    }
    self.postMessage("other:" + e.data);
};
""",

    # Only valid as a module: a classic worker must fail on the import.
    "vwps-module-dep.js": """
export const answer = 42;
""",

    "vwps-module-worker.js": """
import { answer } from "./vwps-module-dep.js";
self.onmessage = function () { self.postMessage("imported:" + answer); };
self.postMessage("imported:" + answer);
""",

    # `modules/resources/post-message-on-load-worker.js` upstream.
    "vwps-onload-worker.js": """
self.postMessage("LOADED");
self.onmessage = function () { self.postMessage("LOADED-after-poke"); };
""",

    "vwps-onload-shared.js": """
self.onconnect = function (e) { e.ports[0].postMessage("LOADED"); };
""",

    # What WPT's own `post-message-on-load-worker.js` branches on.
    "vwps-introspect-worker.js": """
function probe() {
    var names = ["WorkerGlobalScope", "DedicatedWorkerGlobalScope",
                 "SharedWorkerGlobalScope", "MessageEvent", "MessageChannel",
                 "MessagePort", "WorkerNavigator", "ErrorEvent"];
    var out = [];
    for (var i = 0; i < names.length; i++) {
        out.push(names[i] + "=" + (names[i] in self));
    }
    out.push("ctor=" + (self.constructor && self.constructor.name));
    try { out.push("instanceof=" + (self instanceof DedicatedWorkerGlobalScope)); }
    catch (e) { out.push("instanceof-throws=" + e.name); }
    return out.join(" ");
}
self.postMessage(probe());
self.onmessage = function () { self.postMessage(probe()); };
""",

    "vwps-introspect-shared.js": """
self.onconnect = function (e) {
    var out = [];
    ["WorkerGlobalScope", "SharedWorkerGlobalScope", "MessageEvent"].forEach(function (n) {
        out.push(n + "=" + (n in self));
    });
    try { out.push("instanceof=" + (self instanceof SharedWorkerGlobalScope)); }
    catch (err) { out.push("instanceof-throws=" + err.name); }
    e.ports[0].postMessage(out.join(" "));
};
""",

    "vwps-selfclose-worker.js": """
self.onmessage = function (e) {
    if (e.data === "close-now") {
        self.postMessage("closing");
        self.close();
        self.postMessage("after-close-in-worker");
        return;
    }
    self.postMessage("still-alive:" + e.data);
};
""",
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
    """One connection. `/echo` echoes every frame back; `/echo-size` answers
    each frame with its byte count after a 0.1 s delay, which is what
    `send-many-64K-messages-with-backpressure` needs to apply backpressure."""
    try:
        request = b""
        while b"\r\n\r\n" not in request:
            chunk = conn.recv(4096)
            if not chunk:
                return
            request += chunk
        head = request.decode("latin-1")
        path = head.split(" ", 2)[1] if " " in head else "/"
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
                if path.startswith("/echo-size"):
                    time.sleep(0.1)
                    conn.sendall(_ws_frame(0x1, str(len(payload)).encode("ascii")))
                else:
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
    log_path = os.path.join(REPO, ".tmp", f"vwps-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.vwps-{name}.html"],
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
        served = [p for p in SERVED if "/.vwps-" not in p]
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

    ws_port, ws_shutdown = _serve_ws()
    written = []
    for name in wanted:
        body = VARIANTS[name][0].replace("__WSPORT__", str(ws_port))
        path = os.path.join(HERE, f".vwps-{name}.html")
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
              "independent half — a worker script missing there was never "
              "fetched, whatever the page or the browser log says (BUG-826).")
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
