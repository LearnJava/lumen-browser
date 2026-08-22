#!/usr/bin/env python3
"""WPT-RUN-6 slice 22: what the timeline, database, stream and subresource waits do.

After slice 21 the unexplained TIMEOUT residual of the WPT-RUN-5 snapshot is
348 ids. The densest *harness* shapes left (the 19 reftests of the residual are
a separate problem — no harness output exists for them by construction) are
four families that all wait on something being *delivered*:

* `performance-timeline` (3), `resource-timing` (2) and
  `timing-entrytypes-registry` (1) wait for a `PerformanceObserver` callback —
  for `mark`/`measure` entries, for a `resource` entry after a `fetch()`, and
  for the callback's third argument (`droppedEntriesCount`);
* `IndexedDB` (9) waits for a transaction to reach `oncomplete` in a specific
  order relative to the requests inside it;
* `eventsource` (5) waits for a second `message` after an `id:`/`retry:`
  field, i.e. for a reconnect that carries `Last-Event-ID`;
* `compression` (3) waits for `DecompressionStream`'s reader to resolve;
  `html/webappapis/timers` (2) waits for a `setTimeout` whose delay overflows
  a signed 32-bit int to fire *immediately* (HTML §8.6: a delay above
  2147483647 is clamped to 0).

The fifth family is the one slice 20 taught us to measure from the server
side: `fetch/metadata/generated` (6) and
`html/semantics/embedded-content` (18) induce a subresource request through an
element — `<video src>`, `<audio src>`, `<link rel=icon>`, `<video poster>`,
`<input type=image>`, SVG `<image>` — and wait for `load`/`error` on it. The
probe's own http server records every path it is asked for, so "the element
fired nothing" is separated from "nothing was ever requested" without
believing the page (BUG-438) or the browser's log (BUG-826: the shell prints
`⤷ preload …` for a request it never makes).

Same harness as slices 15/17/18/19/20/21 and for the reasons recorded in
`CLAUDE.md`: one browser process per page, served over http (never `file://`),
evidence read off the browser's own stderr rather than through an MCP `eval`,
and a 500 ms `setInterval` tick so "the page is alive and heard nothing" is
separable from "the page died".

Measured 2026-08-22 (dev-release, Linux, commit `bafa603d9`, `--seconds 6`).
What it found, and which bug each finding became:

    po-resource-fetch        no `resource` PerformanceEntry is ever produced on
    po-resource-subresource  a live page, although `resource` is in
                             `supportedEntryTypes` and `observe()` accepts it —
                             `_lumen_record_resource_timing` has no caller
                             outside the shim's own unit tests
    po-callback-options      the observer callback is invoked with two
                             arguments, so `options.droppedEntriesCount`
                             throws                              <- BUG-839
    po-callback-throw        an exception thrown by an observer callback is
                             swallowed by `_perf_deliver_to_observer`'s own
                             try/catch — the FAIL cannot become visible
                                                                 <- BUG-840
    idb-tx-empty             a transaction with no requests in it is never
                             queued, so `complete` never fires; `abort()` does
    idb-tx-abort             not finish the transaction synchronously, so
                             `objectStore()` still succeeds after it
                                                                 <- BUG-841
    idb-keep-alive           a self-rearming request loop (WPT's own
    idb-spin-unbounded       `keep_alive`) starves every other task source:
                             16.3M iterations in 6 s, no timers, no rendering,
                             the rest of the document never runs <- BUG-842
    idb-versionchange-blocked  a second `open()` with a higher version upgrades
                             under a live connection; `versionchange` and
                             `blocked` are never dispatched      <- BUG-843
    sse-lengthed             a stream ended by the response body (Content-Length,
    sse-reconnect-onopen     the `wptserve` shape) is never treated as ended —
                             no error, no reconnect; and the reconnects that do
                             happen fire no `open`               <- BUG-844
    sse-incomplete-block     the trailing incomplete block survives the
                             disconnect, is merged into the next connection's
                             data and its `id:` becomes Last-Event-ID
                                                                 <- BUG-845
    decompression-basic      Compression Streams emit nothing until the
    decompression-formats    writable side is closed, so a read before
                             `writer.close()` never resolves     <- BUG-846
    timer-overflow-delay     a delay above 2^31-1 is not converted to WebIDL
                             `long` (i.e. not clamped to 0) and never fires
                                                                 <- BUG-847
    req-video-poster         an element-induced image request is never made:
    req-input-image-src      the server is never asked for the poster, the
    req-svg-image            `<input type=image>` source, the SVG image or the
    req-link-icon            icon, and no `load`/`error` is fired <- BUG-848
    req-video-src            the media resource selection algorithm never runs
    req-audio-src            (re-measurement of BUG-825/BUG-799, now with
                             server-side proof that no request happens)
    response-from-stream     `new Response(stream).arrayBuffer()` resolves with
                             0 bytes (re-measurement, folded into BUG-824)

What works, and is kept here as the control set: `PerformanceObserver` over
`mark`/`measure` (both the plain and the `buffered: true` form),
`indexedDB.open` → `onupgradeneeded` → `onsuccess`, `objectStore.put` →
`onsuccess`, `<img src>` (the request *is* made) and `fetch()` itself.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py
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
#: page being able to report anything, which is exactly what an element-induced
#: subresource fetch cannot be trusted about (BUG-826 logs a preload it never
#: performs).
SERVED = []
#: `Last-Event-ID` seen on each request to an SSE endpoint, in order. The
#: reconnect half of `eventsource/format-field-id*` is only observable here.
SSE_CONNECTS = []
_SERVED_LOCK = threading.Lock()

#: Compressed forms of `expected output`, produced by python's zlib so the
#: bytes are certainly valid; the page hands them to `DecompressionStream`.
#: WPT's own `compression/resources/decompression-input.js` does the same with
#: a literal table.
DEFLATE_BYTES = [120, 156, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45,
                 41, 40, 45, 1, 0, 48, 173, 6, 36]
GZIP_BYTES = [31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 75, 173, 40, 72, 77, 46, 73, 77,
              81, 200, 47, 45, 41, 40, 45, 1, 0, 176, 1, 57, 179, 15, 0, 0, 0]
RAW_BYTES = [75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40, 45, 1, 0]

#: 1x1 transparent GIF — enough for `<img>`, `<input type=image>` and a
#: favicon; the point of those variants is whether the request happens at all.
PIXEL_GIF = bytes([
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x21, 0xF9, 0x04, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
    0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3B,
])

SVG_IMAGE = (b'<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">'
             b'<rect fill="lime" width="8" height="8"/></svg>')

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-22 probe: __NAME__</title>
<body>
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
#: is visible.
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
fetch("psig-asset.js").then(function () { console.log("PROBE fetch-ok"); },
                            function (e) { console.log("PROBE fetch-err " + e); });
</script>
""", "raf+timeout+fetch-ok"),

    # ── PerformanceObserver / Resource Timing ──────────────────────────────
    # `performance-timeline/po-mark-measure.any.html`: the observer must be
    # called with the mark and measure entries created after observe().
    "po-mark-measure": ("""
<script>
var seen = [];
new PerformanceObserver(function (list) {
    list.getEntries().forEach(function (e) { seen.push(e.entryType + ":" + e.name); });
    console.log("PROBE po-entries " + seen.join(","));
}).observe({entryTypes: ["mark", "measure"]});
performance.mark("mark1");
performance.mark("mark2");
performance.measure("measure1", "mark1", "mark2");
setTimeout(function () { console.log("PROBE po-total " + seen.length); }, 500);
</script>
""", "po-entries mark:mark1..., po-total 3"),
    # `performance-timeline/buffered-flag-observer.any.html`: entries created
    # *before* observe() must be delivered when `buffered: true`.
    "po-buffered": ("""
<script>
for (var i = 0; i < 3; i++) performance.mark("foo" + i);
new PerformanceObserver(function (list) {
    console.log("PROBE po-buffered-count " + list.getEntries().length);
}).observe({type: "mark", buffered: true});
setTimeout(function () {
    console.log("PROBE po-buffered-after getEntriesByType=" +
                performance.getEntriesByType("mark").length);
}, 400);
</script>
""", "po-buffered-count 3"),
    # `resource-timing/buffered-flag.any.html`,
    # `resource-timing/supported_resource_type.any.html`: a fetch() must
    # produce a `resource` entry, both live and buffered.
    "po-resource-fetch": ("""
<script>
console.log("PROBE po-supports " + PerformanceObserver.supportedEntryTypes.join(","));
new PerformanceObserver(function (list) {
    console.log("PROBE po-resource " + list.getEntries().map(function (e) {
        return e.entryType + ":" + e.name;
    }).join(","));
}).observe({entryTypes: ["resource"]});
fetch("psig-asset.js?rt=1").then(function () {
    console.log("PROBE fetch-done");
    setTimeout(function () {
        console.log("PROBE po-resource-buffer " +
                    performance.getEntriesByType("resource").length);
    }, 500);
});
</script>
""", "po-resource resource:...psig-asset.js"),
    # The same question for a parser-inserted subresource: `<script src>`,
    # `<img src>` and `<link rel=stylesheet>` are the three initiators
    # `_lumen_record_resource_timing` documents.
    "po-resource-subresource": ("""
<link rel="stylesheet" href="psig-asset.css">
<img src="psig-pixel.gif" alt="">
<script src="psig-asset.js"></script>
<script>
setTimeout(function () {
    console.log("PROBE po-subresource-buffer " +
                performance.getEntriesByType("resource").length +
                " ran=" + window.psigRan);
}, 800);
</script>
""", "po-subresource-buffer 3 ran=1"),
    # `performance-timeline/droppedentriescount.any.html`: the callback's
    # third argument carries `droppedEntriesCount`.
    "po-callback-options": ("""
<script>
new PerformanceObserver(function (list, obs, options) {
    console.log("PROBE po-args argc=" + arguments.length +
                " options=" + (typeof options));
    try {
        console.log("PROBE po-dropped " + options["droppedEntriesCount"]);
    } catch (e) {
        console.log("PROBE po-dropped-threw " + e);
    }
}).observe({type: "mark"});
performance.mark("m");
</script>
""", "po-args argc=3 options=object, po-dropped 0"),
    # Does a failing assertion inside an observer callback reach anybody?
    # `_perf_deliver_to_observer` wraps the call in `try { } catch (e) {}`,
    # which is the same shape that turns webaudio FAILs into TIMEOUTs
    # (BUG-828).
    "po-callback-throw": ("""
<script>
addEventListener("error", function (e) { console.log("PROBE window-error " + e.message); });
new PerformanceObserver(function () {
    console.log("PROBE po-cb-entered");
    throw new Error("psig-observer-boom");
}).observe({type: "mark"});
performance.mark("m");
setTimeout(function () { console.log("PROBE po-after-throw alive"); }, 300);
</script>
""", "po-cb-entered + window-error/script error naming psig-observer-boom"),
    # `timing-entrytypes-registry/registry.any.html` also reads the
    # `navigation` entry, which the same registry advertises.
    "po-navigation-entry": ("""
<script>
setTimeout(function () {
    console.log("PROBE nav-entries " + performance.getEntriesByType("navigation").length +
                " paint=" + performance.getEntriesByType("paint").length +
                " all=" + performance.getEntries().length);
}, 700);
</script>
""", "nav-entries 1"),

    # ── IndexedDB transactions ─────────────────────────────────────────────
    # `IndexedDB/resources/support.js::indexeddb_test` — every one of the 9
    # residual ids starts here: open, upgrade, then a transaction.
    "idb-open-upgrade": ("""
<script>
var open = indexedDB.open("psig-db-open", 1);
open.onupgradeneeded = function () {
    console.log("PROBE idb-upgrade");
    open.result.createObjectStore("store");
};
open.onsuccess = function () { console.log("PROBE idb-open-success " + open.result.name); };
open.onerror = function () { console.log("PROBE idb-open-error"); };
</script>
""", "idb-upgrade + idb-open-success"),
    # `IndexedDB/transaction-lifetime-empty.any.html`: a transaction with a
    # request in it must reach `oncomplete` after the request's `onsuccess`.
    "idb-tx-complete": ("""
<script>
var open = indexedDB.open("psig-db-tx", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var db = open.result;
    var tx = db.transaction("store", "readwrite");
    tx.oncomplete = function () { console.log("PROBE idb-tx-complete"); };
    tx.onabort = function () { console.log("PROBE idb-tx-abort"); };
    tx.onerror = function () { console.log("PROBE idb-tx-error"); };
    var rq = tx.objectStore("store").put("a", 1);
    rq.onsuccess = function () { console.log("PROBE idb-put-success"); };
    rq.onerror = function () { console.log("PROBE idb-put-error"); };
    console.log("PROBE idb-tx-armed mode=" + tx.mode);
};
</script>
""", "idb-put-success then idb-tx-complete"),
    # `IndexedDB/transaction-scheduling-*.any.html`,
    # `IndexedDB/open-request-queue.any.html`: the *order* of the two
    # transactions' completions is the assertion.
    "idb-tx-ordering": ("""
<script>
var order = [];
function note(s) { order.push(s); console.log("PROBE idb-order " + order.join(",")); }
var open = indexedDB.open("psig-db-order", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var db = open.result;
    var tx1 = db.transaction("store", "readwrite");
    tx1.oncomplete = function () { note("tx1.oncomplete"); };
    var rq1 = tx1.objectStore("store").put("a", 1);
    rq1.onsuccess = function () {
        note("rq1.onsuccess");
        var tx2 = db.transaction("store", "readonly");
        tx2.oncomplete = function () { note("tx2.oncomplete"); };
        tx2.objectStore("store").get(1).onsuccess = function () { note("rq2.onsuccess"); };
    };
};
</script>
""", "rq1.onsuccess,tx1.oncomplete,rq2.onsuccess,tx2.oncomplete"),
    # `IndexedDB/transaction-deactivation-timing.any.html`: a request made
    # from a task (not from an event callback) must throw
    # TransactionInactiveError.
    "idb-tx-deactivation": ("""
<script>
var open = indexedDB.open("psig-db-deact", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var db = open.result;
    var tx = db.transaction("store", "readwrite");
    var store = tx.objectStore("store");
    store.put("a", 1);
    setTimeout(function () {
        try {
            store.put("b", 2);
            console.log("PROBE idb-inactive-accepted");
        } catch (e) {
            console.log("PROBE idb-inactive-threw " + e.name);
        }
    }, 0);
};
</script>
""", "idb-inactive-threw TransactionInactiveError"),
    # `IndexedDB/idbtransaction-objectStore-exception-order.any.html`,
    # `IndexedDB/event-dispatch-active-flag.any.html`: `abort()` must deliver
    # `onabort`, and the store must be unreachable afterwards.
    "idb-tx-abort": ("""
<script>
var open = indexedDB.open("psig-db-abort", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var db = open.result;
    var tx = db.transaction("store", "readwrite");
    tx.onabort = function () { console.log("PROBE idb-abort-fired"); };
    tx.oncomplete = function () { console.log("PROBE idb-abort-complete-instead"); };
    tx.abort();
    try {
        tx.objectStore("store");
        console.log("PROBE idb-store-after-abort-ok");
    } catch (e) {
        console.log("PROBE idb-store-after-abort-threw " + e.name);
    }
};
</script>
""", "idb-abort-fired + idb-store-after-abort-threw InvalidStateError"),

    # `IndexedDB/transaction-lifetime-empty.any.html` also commits a
    # transaction with no requests in it at all.
    "idb-tx-empty": ("""
<script>
var open = indexedDB.open("psig-db-empty", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var tx = open.result.transaction("store", "readonly");
    tx.oncomplete = function () { console.log("PROBE idb-empty-complete"); };
    tx.onabort = function () { console.log("PROBE idb-empty-abort"); };
    console.log("PROBE idb-empty-armed");
};
</script>
""", "idb-empty-complete"),
    # `IndexedDB/open-request-queue.any.html`: a second open with a higher
    # version must send `versionchange` to the holder and `blocked` to the
    # requester, and succeed once the holder closes.
    "idb-versionchange-blocked": ("""
<script>
var first = indexedDB.open("psig-db-vc", 1);
first.onupgradeneeded = function () { first.result.createObjectStore("store"); };
first.onsuccess = function () {
    var db1 = first.result;
    db1.onversionchange = function () {
        console.log("PROBE idb-versionchange");
        setTimeout(function () { db1.close(); }, 0);
    };
    var second = indexedDB.open("psig-db-vc", 2);
    second.onblocked = function () { console.log("PROBE idb-blocked"); };
    second.onupgradeneeded = function () { console.log("PROBE idb-second-upgrade"); };
    second.onsuccess = function () { console.log("PROBE idb-second-success v=" + second.result.version); };
    second.onerror = function () { console.log("PROBE idb-second-error"); };
};
</script>
""", "idb-versionchange + idb-blocked + idb-second-success v=2"),
    # `indexeddb_test` opens with `deleteDatabase` first; every one of the 9
    # residual ids runs that line before anything else.
    "idb-delete-database": ("""
<script>
var del = indexedDB.deleteDatabase("psig-db-del");
del.onsuccess = function () { console.log("PROBE idb-delete-success"); };
del.onerror = function () { console.log("PROBE idb-delete-error"); };
del.onblocked = function () { console.log("PROBE idb-delete-blocked"); };
</script>
""", "idb-delete-success"),
    # `IndexedDB/idbobjectstore_createIndex.any.html`.
    "idb-create-index": ("""
<script>
var open = indexedDB.open("psig-db-index", 1);
open.onupgradeneeded = function () {
    var store = open.result.createObjectStore("store", {keyPath: "k"});
    var index = store.createIndex("idx", "v");
    console.log("PROBE idb-index-created " + index.name + " keyPath=" + index.keyPath);
};
open.onsuccess = function () {
    var tx = open.result.transaction("store", "readwrite");
    var store = tx.objectStore("store");
    store.put({k: 1, v: "x"}).onsuccess = function () {
        var rq = store.index("idx").get("x");
        rq.onsuccess = function () { console.log("PROBE idb-index-get " + JSON.stringify(rq.result)); };
        rq.onerror = function () { console.log("PROBE idb-index-get-error"); };
    };
};
</script>
""", "idb-index-created idx + idb-index-get {k:1,v:x}"),
    # `IndexedDB/resources/support.js::keep_alive` — the spinning `get(0)`
    # loop `event-dispatch-active-flag` and three of its siblings hold their
    # transaction open with.
    "idb-keep-alive": ("""
<script>
var open = indexedDB.open("psig-db-alive", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var tx = open.result.transaction("store", "readonly");
    var completed = false, spins = 0, spinning = true;
    tx.addEventListener("complete", function () {
        completed = true;
        console.log("PROBE idb-alive-complete spins=" + spins);
    });
    function spin() {
        if (!spinning) return;
        spins++;
        tx.objectStore("store").get(0).onsuccess = spin;
    }
    spin();
    setTimeout(function () {
        spinning = false;
        console.log("PROBE idb-alive-checked spins=" + spins + " completed=" + completed);
    }, 1000);
};
</script>
""", "idb-alive-checked spins>1 completed=false, then idb-alive-complete"),

    # Why does the spin wedge the page? A bounded spin says whether the
    # request's `onsuccess` is dispatched *synchronously* from `get()` — which
    # turns `keep_alive`'s self-rearming handler into unbounded recursion
    # instead of one request per task.
    "idb-spin-bounded": ("""
<script>
var open = indexedDB.open("psig-db-spin", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    var tx = open.result.transaction("store", "readonly");
    var left = 5, depth = 0, maxDepth = 0;
    function spin() {
        if (left-- <= 0) { console.log("PROBE idb-spin-done maxDepth=" + maxDepth); return; }
        depth++;
        if (depth > maxDepth) maxDepth = depth;
        var rq = tx.objectStore("store").get(0);
        rq.onsuccess = spin;
        console.log("PROBE idb-spin-returned left=" + left + " depth=" + depth);
        depth--;
    }
    spin();
};
</script>
""", "5 x idb-spin-returned with depth=1 (async dispatch), then idb-spin-done"),

    # Does the unbounded spin run at all, or does it wedge before the first
    # iteration? Logging every 50th spin separates "the loop is starving every
    # other task source" from "the page died at once".
    "idb-spin-unbounded": ("""
<script>
console.log("PROBE spin-page-start");
var open = indexedDB.open("psig-db-spin2", 1);
open.onupgradeneeded = function () { open.result.createObjectStore("store"); };
open.onsuccess = function () {
    console.log("PROBE spin-open-success");
    var tx = open.result.transaction("store", "readonly");
    var spins = 0;
    setTimeout(function () { console.log("PROBE spin-timer-ran spins=" + spins); }, 300);
    function spin() {
        spins++;
        if (spins % 50 === 0) console.log("PROBE spin-count " + spins);
        tx.objectStore("store").get(0).onsuccess = spin;
    }
    spin();
};
</script>
""", "spin-count climbing AND spin-timer-ran (other task sources still run)"),

    # ── EventSource ────────────────────────────────────────────────────────
    # `eventsource/format-*.any.html`: a `data:` block terminated by a blank
    # line is one `message`.
    "sse-basic": ("""
<script>
var es = new EventSource("psig-sse-basic");
console.log("PROBE sse-created readyState=" + es.readyState);
es.onopen = function () { console.log("PROBE sse-open"); };
es.onmessage = function (e) { console.log("PROBE sse-message data=" + e.data + " id=" + e.lastEventId); };
es.onerror = function () { console.log("PROBE sse-error readyState=" + es.readyState); };
</script>
""", "sse-open + sse-message data=hello"),
    # `eventsource/format-field-id.any.html`, `format-field-id-2`,
    # `format-field-retry*`: after `id:`/`retry:`, the reconnect must carry
    # `Last-Event-ID`. The server records every connection, so the reconnect
    # is observable even if the page hears nothing.
    "sse-id-reconnect": ("""
<script>
var es = new EventSource("psig-sse-id");
es.onmessage = function (e) { console.log("PROBE sse-message data=" + e.data + " id=" + e.lastEventId); };
es.onerror = function () { console.log("PROBE sse-error readyState=" + es.readyState); };
</script>
""", "sse-message data=hello id=42, then data=42 (server sees 2 connects)"),
    # `eventsource/format-data-before-final-empty-line.any.html`: a `data:`
    # line *not* followed by a blank line must NOT be dispatched.
    "sse-partial-data": ("""
<script>
var got = 0;
var es = new EventSource("psig-sse-partial");
es.onmessage = function (e) { got++; console.log("PROBE sse-message data=" + e.data); };
setTimeout(function () { console.log("PROBE sse-partial-total " + got); }, 1500);
</script>
""", "sse-partial-total 0 (and no sse-message)"),

    # `eventsource/format-field-retry.any.html` and `format-field-retry-bogus`
    # wait for the *second* `open` event and time the gap against the `retry:`
    # the stream asked for. The field is written without a space here, exactly
    # as those two tests write it.
    "sse-reconnect-onopen": ("""
<script>
var opens = 0, t0 = Date.now();
var es = new EventSource("psig-sse-reconnect");
es.onopen = function () {
    opens++;
    console.log("PROBE sse-open n=" + opens + " at=" + (Date.now() - t0));
};
es.onmessage = function (e) { console.log("PROBE sse-message data=" + e.data); };
es.onerror = function () { console.log("PROBE sse-error readyState=" + es.readyState); };
setTimeout(function () { console.log("PROBE sse-opens-total " + opens); }, 2500);
</script>
""", "sse-open n=2 at≈400 (retry:400, no space)"),
    # `eventsource/format-data-before-final-empty-line.any.html`: the stream
    # ends mid-block, so the last `data:` must be discarded and the reconnect
    # must produce the *first* message again with an empty `lastEventId`.
    "sse-incomplete-block": ("""
<script>
var count = 0;
var es = new EventSource("psig-sse-incomplete");
es.onmessage = function (e) {
    count++;
    console.log("PROBE sse-message n=" + count + " data=" + e.data + " id=" + e.lastEventId);
};
setTimeout(function () { console.log("PROBE sse-incomplete-total " + count); }, 2500);
</script>
""", "two messages, both data=test1 id= (the id:test block is discarded)"),

    # The shape every one of the five residual `eventsource` ids actually
    # gets: `wptserve` answers `resources/message.py` with a Content-Length
    # and holds the connection open, so the stream is never terminated by a
    # close. Nothing else about the page differs from `sse-basic`.
    "sse-lengthed": ("""
<script>
var es = new EventSource("psig-sse-lengthed");
es.onopen = function () { console.log("PROBE sse-open"); };
es.onmessage = function (e) { console.log("PROBE sse-message data=" + e.data); };
es.onerror = function () { console.log("PROBE sse-error readyState=" + es.readyState); };
setTimeout(function () { console.log("PROBE sse-lengthed-checked readyState=" + es.readyState); }, 2000);
</script>
""", "sse-open + sse-message data=lengthed"),

    # ── compression ────────────────────────────────────────────────────────
    # `compression/decompression-correct-input.any.html` and its two
    # siblings: write one compressed chunk, read one plain chunk.
    "decompression-basic": ("""
<script>
console.log("PROBE cs-present ds=" + (typeof DecompressionStream) +
            " cs=" + (typeof CompressionStream));
(function () {
    if (typeof DecompressionStream !== "function") return;
    var ds = new DecompressionStream("deflate");
    var reader = ds.readable.getReader();
    var writer = ds.writable.getWriter();
    writer.write(new Uint8Array(__DEFLATE__));
    reader.read().then(function (r) {
        console.log("PROBE ds-read done=" + r.done + " text=" +
                    (r.value ? new TextDecoder().decode(r.value) : "-"));
    }, function (e) { console.log("PROBE ds-read-rejected " + e); });
})();
setTimeout(function () { console.log("PROBE ds-after alive"); }, 1200);
</script>
""", "ds-read done=false text=expected output"),
    # The same for gzip and deflate-raw, the two other formats the spec
    # requires and WPT parametrizes over.
    "decompression-formats": ("""
<script>
function tryFormat(name, bytes) {
    if (typeof DecompressionStream !== "function") return;
    var ds = new DecompressionStream(name);
    var reader = ds.readable.getReader();
    ds.writable.getWriter().write(new Uint8Array(bytes));
    reader.read().then(function (r) {
        console.log("PROBE ds-" + name + " text=" +
                    (r.value ? new TextDecoder().decode(r.value) : "-"));
    }, function (e) { console.log("PROBE ds-" + name + "-rejected " + e); });
}
tryFormat("gzip", __GZIP__);
tryFormat("deflate-raw", __RAW__);
</script>
""", "ds-gzip text=expected output, ds-deflate-raw text=expected output"),
    # Does the output appear once the writable side is closed? The shim is a
    # buffer-then-flush model by its own header comment, so this separates
    # "never produces output" from "produces it only on close" — the WPT
    # tests read *before* closing, which is what makes them hang.
    "decompression-after-close": ("""
<script>
(function () {
    var ds = new DecompressionStream("deflate");
    var reader = ds.readable.getReader();
    var writer = ds.writable.getWriter();
    writer.write(new Uint8Array(__DEFLATE__));
    writer.close();
    reader.read().then(function (r) {
        console.log("PROBE ds-closed-read done=" + r.done + " text=" +
                    (r.value ? new TextDecoder().decode(r.value) : "-"));
    }, function (e) { console.log("PROBE ds-closed-rejected " + e); });
})();
</script>
""", "ds-closed-read text=expected output"),
    # Round trip: if CompressionStream exists, its output must decompress.
    "compression-roundtrip": ("""
<script>
(function () {
    if (typeof CompressionStream !== "function") { console.log("PROBE cs-missing"); return; }
    var cs = new CompressionStream("gzip");
    var writer = cs.writable.getWriter();
    writer.write(new TextEncoder().encode("expected output"));
    writer.close();
    cs.readable.getReader().read().then(function (r) {
        console.log("PROBE cs-chunk done=" + r.done +
                    " bytes=" + (r.value ? r.value.length : 0));
    }, function (e) { console.log("PROBE cs-reader-rejected " + e); });
})();
</script>
""", "cs-chunk bytes>0"),
    # A `ReadableStream` handed to `Response` as a body: `fetch`-shaped WPT
    # tests read a stream that way, and the stream is also the only way to
    # observe a compressed body end to end.
    "response-from-stream": ("""
<script>
(function () {
    var cs = new CompressionStream("gzip");
    var writer = cs.writable.getWriter();
    writer.write(new TextEncoder().encode("expected output"));
    writer.close();
    new Response(cs.readable).arrayBuffer().then(function (buf) {
        console.log("PROBE response-bytes " + buf.byteLength);
    }, function (e) { console.log("PROBE response-rejected " + e); });
    var rs = new ReadableStream({start: function (c) { c.enqueue(new Uint8Array([1, 2, 3])); c.close(); }});
    new Response(rs).arrayBuffer().then(function (buf) {
        console.log("PROBE response-plain-bytes " + buf.byteLength);
    }, function (e) { console.log("PROBE response-plain-rejected " + e); });
})();
</script>
""", "response-bytes 35, response-plain-bytes 3"),

    # ── timers ─────────────────────────────────────────────────────────────
    # `html/webappapis/timers/type-long-{settimeout,setinterval}.any.html`:
    # HTML §8.6 step 5 — a delay that does not fit a signed 32-bit int is
    # clamped to 0, so the callback runs on the next task.
    "timer-overflow-delay": ("""
<script>
setTimeout(function () { console.log("PROBE timer-overflow-fired"); }, Math.pow(2, 32));
setInterval(function () { console.log("PROBE interval-overflow-fired"); }, Math.pow(2, 32));
setTimeout(function () { console.log("PROBE timer-control-fired"); }, 100);
</script>
""", "timer-overflow-fired + interval-overflow-fired (clamped to 0)"),

    # ── element-induced subresource requests ───────────────────────────────
    # `fetch/metadata/generated/element-video.sub.html` and the media
    # resource-selection family: setting `src` must start a fetch and end in
    # `load`/`error`.
    "req-video-src": ("""
<script>
var v = document.createElement("video");
v.onload = function () { console.log("PROBE video-load"); };
v.onerror = function () { console.log("PROBE video-error"); };
v.addEventListener("loadstart", function () { console.log("PROBE video-loadstart"); });
v.addEventListener("error", function () { console.log("PROBE video-error-listener"); });
document.body.appendChild(v);
v.setAttribute("src", "psig-media.mp4?video=1");
setTimeout(function () {
    console.log("PROBE video-state networkState=" + v.networkState +
                " readyState=" + v.readyState + " currentSrc=" + v.currentSrc);
}, 1000);
</script>
""", "server sees /psig-media.mp4?video=1, video-loadstart + video-error"),
    # `fetch/metadata/generated/element-audio.sub.html`. Kept separate from
    # the video variant because `<audio src>` freezes the page (BUG-799) —
    # a re-measurement of that, not a new question.
    "req-audio-src": ("""
<script>
var a = document.createElement("audio");
a.addEventListener("loadstart", function () { console.log("PROBE audio-loadstart"); });
a.addEventListener("error", function () { console.log("PROBE audio-error"); });
document.body.appendChild(a);
console.log("PROBE audio-before-src");
a.setAttribute("src", "psig-media.mp3?audio=1");
console.log("PROBE audio-after-src");
</script>
""", "audio-after-src + server sees /psig-media.mp3 (BUG-799: page freezes)"),
    # `fetch/metadata/generated/element-link-icon.sub.html`: `<link rel=icon>`
    # must be fetched. BUG-826 covers preload/modulepreload/prefetch only.
    "req-link-icon": ("""
<script>
var link = document.createElement("link");
link.rel = "icon";
link.href = "psig-icon.gif?icon=1";
link.onload = function () { console.log("PROBE icon-load"); };
link.onerror = function () { console.log("PROBE icon-error"); };
document.head.appendChild(link);
var css = document.createElement("link");
css.rel = "stylesheet";
css.href = "psig-asset.css?control=1";
css.onload = function () { console.log("PROBE css-load"); };
document.head.appendChild(css);
</script>
""", "server sees /psig-icon.gif (control: /psig-asset.css + css-load)"),
    # `fetch/metadata/generated/element-video-poster.sub.html`: the poster is
    # an ordinary image request made by the video element.
    "req-video-poster": ("""
<video poster="psig-poster.gif?poster=1" src="psig-media.mp4?poster-video=1"></video>
<script>
setTimeout(function () { console.log("PROBE poster-checked"); }, 800);
</script>
""", "server sees /psig-poster.gif"),
    # `fetch/metadata/generated/svg-image.sub.html`: an SVG `<image href>`
    # inside an inline `<svg>`.
    "req-svg-image": ("""
<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
  <image href="psig-image.svg?svg=1" width="8" height="8"/>
</svg>
<img src="psig-pixel.gif?control=1" alt="">
<script>
setTimeout(function () { console.log("PROBE svg-checked"); }, 800);
</script>
""", "server sees /psig-image.svg (control: /psig-pixel.gif)"),
    # `fetch/metadata/generated/element-input-image.sub.html`: `<input
    # type=image src>` is an image request too.
    "req-input-image-src": ("""
<input type="image" src="psig-pixel.gif?input=1">
<script>
var i = document.createElement("input");
i.type = "image";
i.onload = function () { console.log("PROBE input-image-load"); };
i.onerror = function () { console.log("PROBE input-image-error"); };
i.src = "psig-pixel.gif?input-script=1";
document.body.appendChild(i);
setTimeout(function () { console.log("PROBE input-image-checked"); }, 800);
</script>
""", "server sees both /psig-pixel.gif requests + input-image-load"),
}

#: A page that floods stderr (the IndexedDB spin) would otherwise fill
#: the report with one line per iteration.
_MAX_MARKERS = 40

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the probe pages, recording every path asked for.

    The SSE endpoints are dynamic: they cannot be files because the reconnect
    behaviour (`Last-Event-ID`, `retry:`) is the thing under measurement.

    `protocol_version` is HTTP/1.1 because that is what `wptserve` speaks, and
    the difference matters for `EventSource`: a `wptserve` handler answers with
    a `Content-Length` and *keeps the connection open*, while an HTTP/1.0
    answer ends the stream by closing the socket. `psig-sse-lengthed` is the
    first shape, every other SSE endpoint here is the second.
    """

    protocol_version = "HTTP/1.1"

    def do_GET(self):  # noqa: N802 — http.server's own casing
        with _SERVED_LOCK:
            SERVED.append(self.path)
        path = self.path.split("?")[0]
        if path.startswith("/psig-sse-"):
            self._sse(path)
            return
        if path.endswith((".gif", ".png")):
            self._blob(PIXEL_GIF, "image/gif")
            return
        if path.endswith(".svg"):
            self._blob(SVG_IMAGE, "image/svg+xml")
            return
        if path.endswith((".mp4", ".mp3")):
            # Deliberately not a real media file: the question is whether the
            # request is made, not whether the codec exists.
            self._blob(b"\0\0\0\x18ftypmp42", "video/mp4")
            return
        super().do_GET()

    def _blob(self, payload, mime):
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _sse(self, path):
        """One SSE response, shaped by the endpoint name.

        The stream is terminated by closing the connection, which is what
        makes a spec-compliant `EventSource` reconnect after `retry:` ms.
        """
        last_id = self.headers.get("Last-Event-ID", "")
        with _SERVED_LOCK:
            SSE_CONNECTS.append((path, last_id))
        if path == "/psig-sse-lengthed":
            # Exactly `eventsource/resources/message.py` under `wptserve`: a
            # complete body with a Content-Length, connection kept alive.
            body = b"retry:400\ndata:lengthed\n\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            self.wfile.flush()
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.close_connection = True
        self.end_headers()
        try:
            if path == "/psig-sse-basic":
                self.wfile.write(b"retry: 5000\ndata: hello\n\n")
                self.wfile.flush()
                time.sleep(1.0)
                self.wfile.write(b"data: second\n\n")
                self.wfile.flush()
                time.sleep(2.0)
            elif path == "/psig-sse-id":
                if last_id:
                    self.wfile.write(b"data: " + last_id.encode("utf-8") + b"\n\n")
                else:
                    self.wfile.write(b"id: 42\nretry: 200\ndata: hello\n\n")
                self.wfile.flush()
            elif path == "/psig-sse-partial":
                # No terminating blank line: nothing may be dispatched.
                self.wfile.write(b"data: hello\n")
                self.wfile.flush()
                time.sleep(3.0)
            elif path == "/psig-sse-reconnect":
                # `retry:` written the way `format-field-retry*` writes it —
                # no space after the colon — then the connection closes.
                self.wfile.write(b"retry:400\ndata:x\n\n")
                self.wfile.flush()
            elif path == "/psig-sse-incomplete":
                # Exactly `format-data-before-final-empty-line`'s body: one
                # complete block, then a block with no terminating blank line.
                self.wfile.write(b"retry:400\ndata:test1\n\nid:test\ndata:test2\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *args):
        pass


ASSETS = {
    "psig-asset.js": "window.psigRan = (window.psigRan || 0) + 1;\n",
    "psig-asset.css": "body { color: rgb(1, 2, 3); }\n",
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


def _body(name):
    """The variant's body with the compressed-byte tables spliced in."""
    body = VARIANTS[name][0]
    return (body.replace("__DEFLATE__", repr(DEFLATE_BYTES))
                .replace("__GZIP__", repr(GZIP_BYTES))
                .replace("__RAW__", repr(RAW_BYTES)))


def _run_variant(binary, name, http_port, seconds):
    """Launch one browser on one probe page; return (ticks, markers, fetched)."""
    log_path = os.path.join(REPO, ".tmp", f"psig-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with _SERVED_LOCK:
        del SERVED[:]
        del SSE_CONNECTS[:]
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.psig-{name}.html"],
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
    # Bounded on purpose, and with a set for membership: `idb-spin-unbounded`
    # prints a distinct marker 326 000 times (one per spin), which an
    # `if marker not in list` dedup turns into a quarter-hour of quadratic
    # scanning — the harness would hang on the very page whose freeze it is
    # measuring.
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
        fetched = [p for p in SERVED if not p.startswith("/.psig-")]
        connects = list(SSE_CONNECTS)
    if connects:
        markers.append("[sse connects: " + "; ".join(
            f"{p} last-event-id={i or '-'}" for p, i in connects) + "]")
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
        path = os.path.join(HERE, f".psig-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE.replace("__NAME__", name).replace("__BODY__", _body(name)))
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
