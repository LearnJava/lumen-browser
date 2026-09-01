#!/usr/bin/env python3
"""WPT-RUN-6 slice 32: label focus forwarding, timer-callback exceptions,
script-created `<video>` empty-src, and script-reassigned parser-iframe `src`
— four of the 58 `unclassified` ids left by slice 31.

Reading the residual by hand (rather than by directory, which slice 31 showed
has no dominant cluster left) surfaced four candidates with a concrete,
grep-backed hypothesis each:

    /html/semantics/forms/the-label-element/forward-focus-to-associated-element.html
    /user-timing/measure.html (+ /user-timing/measure_navigation_timing.html,
        /navigation-timing/test-navigate-within-document.html — same shape)
    /html/semantics/embedded-content/the-video-element/video_crash_empty_src.html
    /websockets/unload-a-document/003.html (+ 004.html — same shape)

`document-policy/reporting/sync-xhr-report-only.html` and its
`permissions-policy` twin needed no live probe: `grep -rn
"document-policy-violation\\|permissions-policy-violation"
crates/js/src/*.rs crates/js/src/shim/*.js` is empty, i.e. `ReportingObserver`
itself exists (`reporting_api.rs`) but nothing ever queues a Document-Policy or
Permissions-Policy violation report — filed straight as a ДОРАБОТКА
(`GAP-POLICYREPORT`) without a variant here, same shape as `GAP-CSPENF`.

Same harness as slices 30/31: one browser process per page, served over http,
evidence read off the browser's own stderr, a 500 ms tick so "alive and heard
nothing" is separable from "died".

## Results (2026-09-01, dev-release, Linux, `main` = `892756356`)

- `label-focus-forward` — confirms the grep read. `_lumen_is_focusable()`
  (`web_api_shim_tail_b.js`) has no `LABEL` case, so `label.focus()` on a
  `<label>` without its own `tabindex` is a silent no-op: no `focus` fires on
  the label, and nothing resolves the label's associated/first-labelable
  control to forward to. Filed as BUG-951 (P3 — one function, not a
  subsystem: HTML LS §6.6.3's "labeled control" special-case).
- `timer-callback-exception` — confirms the hypothesis. `window.performance.
  timing` does not exist (grep-zero across the shim), and `measure.html`'s
  `measure_test_cb` — called via `step_timeout` (a plain `setTimeout`), not
  inside a `test()`-wrapped step — reads `window.performance.timing.
  navigationStart` unconditionally for its first scenario. The resulting
  `TypeError` is thrown from inside a *timer* callback, and unlike a
  synchronously-thrown script error (which the browser log prints as
  `script error: …`) this one produces **nothing at all** on stderr — the
  variant's own `setTimeout`-thrown `TypeError` is exactly as silent live as
  the corpus log for `measure.html` was (§ evidence below). `done()` is never
  reached, so the test hangs to timeout. Filed as BUG-952 (P3 — exceptions
  thrown from a timer callback must reach the same reporting path as ones
  thrown from a direct script evaluation; separately, `window.performance.
  timing` itself is the already-known `GAP-` shaped legacy-Navigation-Timing-L1
  absence, not re-filed here since the crash is the reportable defect, not the
  missing attribute alone — a page that guards the read still hangs on
  nothing if a later unguarded exception exists elsewhere, but this specific
  one is the whole story for the three ids named above).
- `video-empty-src` — comes back clean: both `error` events fire, matching
  the existing `assigning_src_runs_resource_selection_and_reports_the_failure`
  unit test. `video_crash_empty_src.html` is *not* explained by this
  mechanism; it stays unclassified.
- `iframe-late-src-reassign` — confirms an existing gap rather than a new one:
  a parser-inserted `<iframe>` whose `src` is reassigned from script produces
  no request at all, closing the loop on the `frame-late-src` variant already
  measured in BUG-885/FRAME-8. `websockets/unload-a-document/003.html` and
  `004.html` both drive `iframe.src = 'data:text/html,...'` on an
  already-parsed, already-inserted `<iframe>` — same shape, not a new
  mechanism. Added to `timeout_audit.py` as an `_exact_id_marker` pointing at
  BUG-885, no new bug number.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice32_gaps.py
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

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>slice-32 probe: __NAME__</title>
<body>
__BODY__
<script>
console.log("PROBE script-start search=" + location.search);
var _n = 0;
setInterval(function () { console.log("PROBE tick " + (++_n)); }, 500);
</script>
</body>
"""

REPORT = """
<script>
function rep(label, fn) {
  try {
    var v = fn();
    console.log("PROBE " + label + " = " + v);
  } catch (e) {
    console.log("PROBE " + label + " THREW " + (e && e.name ? e.name + ": " : "") +
                (e && e.message ? e.message : e));
  }
}
</script>
"""

VARIANTS = {
    "control": ("""
<script>
requestAnimationFrame(function () { console.log("PROBE raf"); });
window.addEventListener("load", function () { console.log("PROBE load"); });
</script>
""", "raf, load"),

    # `forward-focus-to-associated-element.html`'s exact shape (label-a/
    # label-b cases): a `<label for=...>` and a `<label>` wrapping its input,
    # neither with its own `tabindex`. Spec says `.focus()` on the label must
    # forward to the associated control.
    "label-focus-forward": ("""
<input id=input-a type=checkbox>
<label id=label-a for=input-a>a</label>
<label id=label-b><input id=input-b type=checkbox> b</label>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var la = document.getElementById("label-a"), ia = document.getElementById("input-a");
  ia.addEventListener("focus", function () { console.log("PROBE input-a-focused"); });
  la.addEventListener("focus", function () { console.log("PROBE label-a-focused-BAD"); });
  la.focus();
  rep("activeElement-after-label-a-focus", function () { return document.activeElement && document.activeElement.id; });
  setTimeout(function () {
    var lb = document.getElementById("label-b"), ib = document.getElementById("input-b");
    ib.addEventListener("focus", function () { console.log("PROBE input-b-focused"); });
    lb.addEventListener("focus", function () { console.log("PROBE label-b-focused-BAD"); });
    lb.focus();
    rep("activeElement-after-label-b-focus", function () { return document.activeElement && document.activeElement.id; });
    console.log("PROBE labelfocus-done");
  }, 500);
});
</script>
""", "focus fires on input-a/input-b, never on label-a/label-b"),

    # `measure.html`'s exact shape: a mark, a `step_timeout` (plain
    # `setTimeout`) callback that reads `window.performance.timing.
    # navigationStart` unconditionally — does an exception thrown from inside
    # a timer callback reach the browser's own error log the way a
    # synchronous script error does?
    "timer-callback-exception": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  performance.mark("mark_start");
  setTimeout(function () {
    // `measure.html`'s own first branch: reads window.performance.timing
    // unconditionally regardless of which scenario is being built.
    var d = (new Date()) - window.performance.timing.navigationStart;
    rep("unreached-after-throw", function () { return String(d); });
    console.log("PROBE timercb-done");
  }, 200);
  setTimeout(function () {
    console.log("PROBE outer-alive");
  }, 1000);
});
</script>
""", "TypeError from the timer callback either logs or the page hangs silently"),

    # `video_crash_empty_src.html`'s exact shape: script-created <video>,
    # `.src` assigned BEFORE insertion, `error` listener armed before
    # `appendChild`.
    "video-empty-src": ("""
""" + REPORT + """
<script>
function makeCrashTest(src, label) {
  var video = document.createElement("video");
  video.src = src;
  video.controls = true;
  video.addEventListener("error", function () {
    document.body.removeChild(video);
    console.log("PROBE " + label + "-errored");
  });
  document.body.appendChild(video);
}
window.addEventListener("load", function () {
  makeCrashTest("about:blank", "blank-src");
  makeCrashTest("", "empty-src");
  setTimeout(function () { console.log("PROBE videocrash-done"); }, 3000);
});
</script>
""", "both about:blank and '' fire `error` on the <video>"),

    # `websockets/unload-a-document/003.html`'s exact shape (minus the
    # WebSocket — BUG-885/FRAME-8 predicts the mechanism is the `src`
    # reassignment itself, not anything socket-specific): a parser-inserted
    # `<iframe>` whose `src` is later reassigned from script to a `data:`
    # URL.
    "iframe-late-src-reassign": ("""
<iframe id=f src="about:blank"></iframe>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var f = document.getElementById("f");
  setTimeout(function () {
    f.addEventListener("load", function () { console.log("PROBE iframe-reload-fired"); });
    f.src = "data:text/html,<body>reassigned</body>";
    setTimeout(function () {
      rep("iframe-contentDocument-body", function () {
        var d = f.contentDocument;
        return d && d.body ? d.body.innerHTML : String(d);
      });
      console.log("PROBE iframelate-done");
    }, 2000);
  }, 300);
});
</script>
""", "iframe navigates to the data: URL and fires load"),
}


_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\\n\\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\\n\\r]+)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass


def _free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _serve(root):
    port = _free_port()

    def handler(*args, **kwargs):
        return _Quiet(*args, directory=root, **kwargs)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return port, server.shutdown


def _run_variant(binary, name, http_port, seconds):
    log_path = os.path.join(REPO, ".tmp", f"s32-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".s32-{name}.html"
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
    for err in dict.fromkeys(_ERROR_RE.findall(text)):
        markers.append(f"[engine] {err.strip()}")
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=6.0)
    parser.add_argument("--variant", action="append", default=None)
    args = parser.parse_args()

    wanted = args.variant or list(VARIANTS)
    unknown = [name for name in wanted if name not in VARIANTS]
    if unknown:
        print("unknown variant(s):", ", ".join(unknown), file=sys.stderr)
        return 2

    http_port, shutdown = _serve(HERE)
    written = []
    for name in wanted:
        path = os.path.join(HERE, f".s32-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)

    try:
        print(f"{'variant':32s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:32s} {ticks:5d}  {expected:62s} {seen}")
            if not markers:
                silent.append(name)
        print()
        if silent:
            print("no marker at all on:", ", ".join(silent))
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
