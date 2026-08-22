#!/usr/bin/env python3
"""WPT-RUN-6 slice 17: two silent waits behind the densest residual clusters.

Two categories of the WPT-RUN-5 snapshot are almost entirely unexplained
TIMEOUT — `layout-instability` (35 of 37) and `webrtc` (41 of 48) — and both
are silent: nothing is printed, nothing throws, the harness loads cleanly and
the page simply never finishes. That is the signature of a wait for something
the engine never produces, which no evidence stage of `timeout_audit.py` can
see; a source marker for it therefore has to be confirmed first, the way slice
15 confirmed the animation/observer gaps (`verify_event_delivery_gaps.py`).

Same shape as that probe, and for the same reason recorded in `CLAUDE.md`: one
browser process per page, served over http, evidence read off the browser's own
stderr rather than through an MCP `eval` (a wedged page answers `eval` with "JS
context not available", which slice 6 misread as a broken live window), and a
500 ms `setInterval` tick so "the page is alive and heard nothing" is
distinguishable from "the page died".

Measured 2026-08-21 (dev-release, Linux, commit 79ea47826, `--seconds 8`;
16 ticks means the page stayed alive throughout):

    variant                  ticks  markers seen
    control                     16  raf, timeout
    cls-feature-detect          16  supported=…,layout-shift,…  <- advertised
                                    LayoutShift=undefined
                                    observe-ok
    cls-shift                   16  shifted           <- and no entry, ever
    cls-shift-buffered          16  shifted
    cls-attribution             16  shifted
    rtc-icecandidate            16  icecandidate      <- the one live event
    rtc-two-peer                16  offer, answer, remote-set
                                    signaling=stable  <- and no datachannel,
                                                         no state change
    rtc-datachannel-open        16  dc-created readyState=connecting

So two gaps, both confirmed silent. Layout Instability is *advertised*
(`PerformanceObserver.supportedEntryTypes` contains `layout-shift` and
`observe()` accepts it) but no entry is ever delivered: the Rust trigger
`deliver_layout_shift` (`crates/shell/src/main.rs:2925`, `#[allow(dead_code)]`)
has no call site in layout/reflow at all, and `window.LayoutShift` does not
exist — so a test built on the category's own `ScoreWatcher` helper
(`layout-instability/resources/util.js`, which awaits `watcher.promise`) waits
forever. Filed as BUG-809. And the `RTCPeerConnection` stub
(`crates/js/src/webrtc_stub.rs`) answers offer/answer locally but never
connects two peers: `ondatachannel` never fires on the remote side and
`connectionState` stays `new`, which is the already-open BUG-727 — this probe
adds the corpus-wide price for it.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_layout_shift_and_peer_gaps.py
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

#: `body` is spliced into a page that also arms a `setInterval` logging
#: `PROBE tick`, so "the entry never came" is separable from "the page died".
#: `expect` is what the shape is believed to do today, printed next to the
#: measurement so a change in either direction is visible without re-reading
#: the docstring.
VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
setTimeout(function () { console.log("PROBE timeout"); }, 100);
</script>
""", "raf+timeout"),
    # What a test's feature detection sees before it decides to wait. WPT's own
    # `ScoreWatcher` throws unless `supportedEntryTypes` lists `layout-shift`,
    # so an honest "not supported" here would turn every one of these tests
    # into a fast FAIL instead of a TIMEOUT.
    "cls-feature-detect": ("""
<script>
console.log("PROBE supported=" + PerformanceObserver.supportedEntryTypes.join(","));
console.log("PROBE LayoutShift=" + typeof window.LayoutShift);
console.log("PROBE LayoutShiftAttribution=" + typeof window.LayoutShiftAttribution);
try {
    new PerformanceObserver(function () {}).observe({entryTypes: ["layout-shift"]});
    console.log("PROBE observe-ok");
} catch (e) {
    console.log("PROBE observe-threw " + e.name);
}
</script>
""", "supported list + LayoutShift ctor"),
    # `simple-block-movement.html` reduced to its essentials: observe, wait two
    # frames, move a 300x200 block down by 160px, expect an entry.
    "cls-shift": ("""
<style>#shifter { position: relative; width: 300px; height: 200px; background: blue; }</style>
<div id=shifter></div>
<script>
new PerformanceObserver(function (list) {
    list.getEntries().forEach(function (e) {
        console.log("PROBE cls-entry value=" + e.value + " sources=" +
                    (e.sources ? e.sources.length : "none"));
    });
}).observe({entryTypes: ["layout-shift"]});
requestAnimationFrame(function () {
    requestAnimationFrame(function () {
        document.getElementById("shifter").style.top = "160px";
        console.log("PROBE shifted");
    });
});
</script>
""", "cls-entry"),
    # The buffered flag is the other half of the API (`sources.html`,
    # `buffered-flag.html`): entries that happened before the observer existed.
    "cls-shift-buffered": ("""
<style>#shifter { position: relative; width: 300px; height: 200px; background: blue; }</style>
<div id=shifter></div>
<script>
document.getElementById("shifter").style.top = "160px";
console.log("PROBE shifted");
setTimeout(function () {
    new PerformanceObserver(function (list) {
        console.log("PROBE cls-buffered-entries=" + list.getEntries().length);
    }).observe({type: "layout-shift", buffered: true});
}, 500);
</script>
""", "cls-buffered-entries>0"),
    # `sources.html`/`attribution-*.html` read `entry.sources[0].node` and the
    # previous/current rects — a second, independent surface of the same API.
    "cls-attribution": ("""
<style>#shifter { position: relative; width: 300px; height: 200px; background: blue; }</style>
<div id=shifter></div>
<script>
new PerformanceObserver(function (list) {
    var e = list.getEntries()[0];
    var s = e.sources && e.sources[0];
    console.log("PROBE cls-source node=" + (s ? String(s.node && s.node.id) : "none"));
}).observe({entryTypes: ["layout-shift"]});
setTimeout(function () {
    document.getElementById("shifter").style.top = "160px";
    console.log("PROBE shifted");
}, 300);
</script>
""", "cls-source node=shifter"),
    # Sanity: the stub does dispatch one event of its own (`_gatherMdns`), so a
    # silent result on the two-peer variant is not "RTC events never fire".
    "rtc-icecandidate": ("""
<script>
var pc = new RTCPeerConnection();
pc.onicecandidate = function (e) {
    console.log("PROBE icecandidate " + (e.candidate ? "candidate" : "end"));
};
pc.createDataChannel("probe");
pc.createOffer().then(function (offer) { return pc.setLocalDescription(offer); });
</script>
""", "icecandidate"),
    # The canonical two-peer WPT shape (`RTCPeerConnection-helper.js`'s
    # `exchangeOfferAnswer`): everything a test needs before its own assertions
    # can even start.
    "rtc-two-peer": ("""
<script>
var pc1 = new RTCPeerConnection();
var pc2 = new RTCPeerConnection();
pc2.ondatachannel = function () { console.log("PROBE remote-datachannel"); };
pc2.onconnectionstatechange = function () {
    console.log("PROBE pc2-connectionState=" + pc2.connectionState);
};
pc1.onconnectionstatechange = function () {
    console.log("PROBE pc1-connectionState=" + pc1.connectionState);
};
pc1.createDataChannel("probe");
pc1.createOffer().then(function (offer) {
    console.log("PROBE offer");
    return pc1.setLocalDescription(offer).then(function () {
        return pc2.setRemoteDescription(offer);
    });
}).then(function () {
    console.log("PROBE remote-set");
    return pc2.createAnswer();
}).then(function (answer) {
    console.log("PROBE answer");
    return pc2.setLocalDescription(answer).then(function () {
        return pc1.setRemoteDescription(answer);
    });
}).then(function () {
    console.log("PROBE signaling=" + pc1.signalingState +
                " conn=" + pc1.connectionState);
}).catch(function (e) { console.log("PROBE rtc-threw " + e); });
</script>
""", "remote-datachannel + connectionState=connected"),
    # The single-peer half: `RTCDataChannel-*.html` waits on `dc.onopen`
    # before it sends anything.
    "rtc-datachannel-open": ("""
<script>
var pc = new RTCPeerConnection();
var dc = pc.createDataChannel("probe");
console.log("PROBE dc-created readyState=" + dc.readyState);
dc.onopen = function () { console.log("PROBE dc-open"); };
dc.onmessage = function (e) { console.log("PROBE dc-message " + e.data); };
pc.createOffer().then(function (offer) { return pc.setLocalDescription(offer); });
</script>
""", "dc-open"),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-17 probe: %(name)s</title>
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
    log_path = os.path.join(REPO, ".tmp", f"clsgap-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.clsgap-{name}.html"],
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

    written = []
    for name in wanted:
        body, _ = VARIANTS[name]
        path = os.path.join(HERE, f".clsgap-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)
    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':24s} {'ticks':>5s}  {'expected':40s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            print(f"{name:24s} {ticks:5d}  {VARIANTS[name][1]:40s} "
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
