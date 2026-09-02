#!/usr/bin/env python3
"""WPT-RUN-6 slice 34: two of the 48 `unclassified` ids left by slice 33.

Reading the residual by hand (still no dominant directory after slice 33)
surfaced two candidates with a concrete, grep-backed hypothesis each:

    /html/webappapis/scripting/events/event-handler-processing-algorithm-error/frameset-element-synthetic-errorevent.html
    /html/webappapis/scripting/events/event-handler-processing-algorithm-error/frameset-element-synthetic-event.html
    /html/semantics/embedded-content/the-video-element/video_crash_empty_src.html

**frameset-element-synthetic-\\*.html**: both tests build `frameset.dispatchEvent(new
ErrorEvent("error", { bubbles: true, ... }))` inside an `<iframe>` and arm an
`EventWatcher` on `framesetWindow` (the iframe's own `window`) for the
bubbled/window-forwarded event. This is the already-known BUG-873 shape — "an
event dispatched from script reaches only the node it was dispatched on, no
ancestor, no document, no window" — read here on a cross-document target
instead of the same-document one BUG-873's own probes used. If confirmed, this
needs no new bug, only an `_exact_id_marker`/`SOURCE_MARKERS` entry pointing at
BUG-873.

**video_crash_empty_src.html**: two `async_test`s each build a `<video>` via
`document.createElement("video")`, set `.src` to `"about:blank"` / `""`, and
wait for the `error` event before calling `test.done()`. Reading
`video_bindings.rs` end to end: `document.createElement` is intercepted and
calls `patchVideoElement` for a script-created `<video>` (so the "script path
is dead where the parser's works" shape, BUG-938/939, should NOT apply here);
`startFetch` computes `abs = (url === '') ? null : resolveUrl(url)`, and
either branch (`abs === null` for `""`, or `isGifSrc("about:blank") === false`
for the non-empty case) reaches `failResource(gen, null, ...)`, whose
"dedicated media source failure steps" branch fires `error` on `el` itself —
an at-target listener, so BUG-873's target/ancestor gap does not block it
either. The source reading finds no gap; this variant exists to measure
whether that reading is complete (CLAUDE.md: reading finds a hypothesis,
still needs measuring on the built binary).

Same harness as slices 30-33: one browser process per page, served over http,
evidence read off the browser's own stderr.

## Results (2026-09-02, dev-release, Linux, `main` = `76c58b60e`)

- `exec-command-insert-text` — confirms the hypothesis. `document.
  activeElement === el` is `true` for all three (`.focus()` works),
  `execCommand("insertText", false, "a")` returns `true` for all three, but
  `value-after` is the empty string on all three and `input-fired` never
  prints once. Filed as BUG-956 (P3 — `_lumen_exec_command`'s `"insertText"`
  arm requires `doc.get_selection().anchor`, which `.focus()` never sets for
  any of the three tags).
- `frameset-synthetic-error-crosswindow` — `fw.addEventListener` (`fw =
  iframe.contentWindow`) throws `TypeError: fw.addEventListener is not a
  function`; the same call on `frameset` (an element inside the frame's own
  document) succeeds. Filed as BUG-957 (P3 — `winFacade` in `frame_bridge.rs`
  is a bare `{}`, never given `EventTarget` methods). Does NOT explain the
  TIMEOUT of `frameset-element-synthetic-errorevent.html`/`-event.html`:
  `testharness.js`'s `Test.prototype.step` catches the throw and settles the
  test as `FAIL`, and the probe's own script continues executing past the
  `rep()` call that threw (`frameset-addEventListener = ok`, `dispatch =
  dispatched` still print) — no hang. Both ids stay unclassified; see
  `docs/probe-method.md` §1 for the transferable lesson.
- `video-crash-exact-shape` (and the simpler `video-empty-and-about-blank-
  error`) — hypothesis refuted. Reproducing `video_crash_empty_src.html`'s
  exact `makeCrashTest` body (script-created `<video>`, `.src =` before the
  `error` listener, `removeChild` inside the handler) for both `"about:
  blank"` and `""`: `error` fires and `removeChild` completes cleanly for
  both. No engine defect found; the real TIMEOUT source for this id is
  unexplained (possibly shard collateral damage, CLAUDE.md's "hung browser"
  gotcha).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_slice34_gaps.py
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
<title>slice-34 probe: __NAME__</title>
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

    # frameset-element-synthetic-*.html's exact shape, reduced: a bubbling
    # ErrorEvent dispatched at an element inside an iframe, watched for on the
    # iframe's own window.
    "frameset-synthetic-error-crosswindow": ("""
<iframe id="f" srcdoc="<frameset></frameset>"></iframe>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  var fw = document.getElementById("f").contentWindow;
  var fd = document.getElementById("f").contentDocument;
  rep("frameset-window", function () { return fw && fw.location && fw.location.href; });
  rep("frameset-doc", function () { return fd && fd.readyState; });
  rep("frameset-el", function () { var fs = fd.querySelector("frameset"); return fs && fs.tagName; });
  rep("fw-dot-document", function () { return fw.document === fd; });
  rep("fw-dot-document-queryselector", function () { return fw.document.querySelector("frameset").tagName; });
  var frameset = fd.querySelector("frameset");
  rep("frameset-error-event-ctor", function () { return typeof ErrorEvent; });
  rep("window-addEventListener", function () {
    fw.addEventListener("error", function (e) {
      console.log("PROBE window-error-received target=" + (e && e.target));
    });
    return "ok";
  });
  rep("frameset-addEventListener", function () {
    frameset.addEventListener("error", function (e) {
      console.log("PROBE frameset-error-received target=" + (e && e.target));
    });
    return "ok";
  });
  rep("dispatch", function () {
    frameset.dispatchEvent(new ErrorEvent("error", { bubbles: true, cancelable: true }));
    return "dispatched";
  });
});
</script>
""", "frameset-error-received always; window-error-received if bubbling reaches the Window"),

    # video_crash_empty_src.html's exact shape: script-created <video>, .src
    # set to "about:blank" then "", error armed before assignment.
    "video-empty-and-about-blank-error": ("""
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  ["about:blank", ""].forEach(function (src, i) {
    var v = document.createElement("video");
    v.controls = true;
    v.addEventListener("error", function () { console.log("PROBE video-error[" + i + "] src=" + JSON.stringify(src)); });
    v.src = src;
    document.body.appendChild(v);
  });
  setTimeout(function () { console.log("PROBE video-crash-done"); }, 3000);
});
</script>
""", "error fires for both about:blank and empty src"),

    # Exact statement order and body of video_crash_empty_src.html's
    # makeCrashTest, without testharness.js (which needs wptrunner's template
    # substitution and is not servable by a raw http.server — see the
    # "Uncaught SyntaxError: Unexpected token '%'" trap in probe-method.md).
    "video-crash-exact-shape": ("""
""" + REPORT + """
<script>
function makeCrashTest(src, i) {
  const video = document.createElement("video");
  video.src = src;
  video.controls = true;
  video.addEventListener("error", () => {
    console.log("PROBE error-fired[" + i + "] src=" + JSON.stringify(src));
    document.body.removeChild(video);
    console.log("PROBE removeChild-done[" + i + "]");
  });
  document.body.appendChild(video);
}
window.addEventListener("load", function () {
  makeCrashTest("about:blank", 0);
  makeCrashTest("", 1);
  setTimeout(function () { console.log("PROBE video-crash-exact-done"); }, 3000);
});
</script>
""", "error-fired and removeChild-done for both, in makeCrashTest's own statement order"),

    # textInput/api.html's exact per-element loop body, reduced to one
    # <input>: focus() then execCommand('insertText', false, 'a'), waiting on
    # 'input'. Hypothesis: `_lumen_exec_command`'s "insertText" arm
    # (dom_core.rs) requires `doc.get_selection().anchor` to already be set —
    # if `.focus()` does not synthesize one, the whole arm is a silent no-op:
    # no text inserted, no `input` event, promise never settles.
    "exec-command-insert-text": ("""
<input class=t1>
<textarea class=t2></textarea>
<div contenteditable class=t3></div>
""" + REPORT + """
<script>
window.addEventListener("load", function () {
  [".t1", ".t2", ".t3"].forEach(function (sel) {
    var el = document.querySelector(sel);
    el.addEventListener("input", function () {
      var v = ("value" in el) ? el.value : el.textContent;
      console.log("PROBE input-fired[" + sel + "] value=" + JSON.stringify(v));
    });
    el.focus();
    rep("active-element-is[" + sel + "]", function () { return document.activeElement === el; });
    var ok = document.execCommand("insertText", false, "a");
    console.log("PROBE exec-command-returned[" + sel + "] = " + ok);
    rep("value-after[" + sel + "]", function () { return ("value" in el) ? el.value : el.textContent; });
  });
  setTimeout(function () { console.log("PROBE exec-command-done"); }, 2000);
});
</script>
""", "input-fired + value-after=='a' for all three elements"),
}


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


_MAX_MARKERS = 40
_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([^\n\r]+)")
_ERROR_RE = re.compile(r"((?:script|module) error: [^\n\r]+)")


def _run_variant(binary, name, http_port, seconds):
    log_path = os.path.join(REPO, ".tmp", f"s34-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    page = f".s34-{name}.html"
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
    parser.add_argument("--seconds", type=float, default=8.0)
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
        path = os.path.join(HERE, f".s34-{name}.html")
        body = VARIANTS[name][0]
        page = PAGE.replace("__NAME__", name).replace("__BODY__", body)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(page)
        written.append(path)

    try:
        print(f"{'variant':40s} {'ticks':>5s}  {'expected':62s} markers seen")
        silent = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            seen = ", ".join(markers) if markers else "— nothing"
            expected = VARIANTS[name][1]
            print(f"{name:40s} {ticks:5d}  {expected:62s} {seen}")
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
