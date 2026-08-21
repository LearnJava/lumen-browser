#!/usr/bin/env python3
"""BUG-799: an `<audio>` with a `src` freezes the whole page — reproduce it.

`bugs/BUG-799-OPEN.md` was filed off a run report alone and says so: the
`subcount 0/0` of `audio-loading-eager.html` was read as "the test's own
`step_timeout(…, 5000)` never fired", i.e. page JS stops while an audio
resource loads — a much heavier claim than "`loadeddata` is never dispatched",
and the bug file asks for a live probe before anyone starts fixing.

This is that probe, and the answer is the heavier one. Seven pages, one browser
process each, all served over http from the vendored WPT tree so `/media/*`
resolves exactly as it does under wptserve (a dump mode would prove nothing —
it installs no audio provider at all, so the shim's `startLoad` returns early
and the page stays healthy; see the `fetch()`/`file://` gotcha in `CLAUDE.md`).
Each page logs `PROBE tick N` from a 500 ms `setInterval`, so the evidence is
read off the browser's own stderr rather than through `eval`: a wedged page
cannot answer an MCP `eval` at all ("JS context not available"), which is what
the first attempt at this probe mistook for a broken live window.

Measured 2026-08-21 (WPT-RUN-6 slice 13), and the split is total:

    control (no audio)        23 ticks
    <audio> without src       23 ticks
    <video src=…mp4>          23 ticks
    <audio src=…mp3>           0 ticks, page script never starts at all
    audio.src set in script    0 ticks, the line after the assignment never runs
    <audio src=…404>           0 ticks — the URL does not have to resolve
    __lumen_audio_load direct  0 ticks, and `__lumen_audio_alloc` /
                               `__lumen_audio_ready_state` before it both return

so the blocking call is `AudioPlaybackProvider::load`, reached from the shim's
`startLoad`. Every thread of the wedged process sits in `futex_wait` — a
deadlock, not a spin — and `PlatformAudioPlayer::load`
(`crates/shell/src/platform/audio_player.rs:314-326`) locks `self.handles`
inside the body of an `if let` whose scrutinee still holds a guard on that same
non-reentrant `std::sync::Mutex`. Binding the guard to a local and dropping it
before the second lock makes all seven pages tick.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug799_audio_timers.py
        [--binary target/dev-release/lumen] [--seconds 12] [--variant NAME]

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

#: `body` is spliced into a page that arms a `setInterval` logging `PROBE tick`.
#: `expect_ticks` is what the shape is believed to do, so a regression in either
#: direction is visible without re-reading this docstring.
VARIANTS = {
    "control": ("", True),
    "audio-no-src": ("<audio></audio>", True),
    "video-markup": ('<video src="/media/counting.mp4"></video>', True),
    "audio-markup": ('<audio src="/media/sine440.mp3"></audio>', False),
    "audio-markup-404": ('<audio src="/media/no-such-file.mp3"></audio>', False),
    "audio-script-src": (
        '<script>console.log("PROBE before-src");'
        'var a = new Audio(); a.src = "/media/sine440.mp3";'
        'console.log("PROBE after-src");</script>', False),
    "native-load": (
        '<script>var h = __lumen_audio_alloc();'
        'console.log("PROBE alloc " + h);'
        'console.log("PROBE ready-state " + __lumen_audio_ready_state(h));'
        '__lumen_audio_load(h, "/media/sine440.mp3");'
        'console.log("PROBE load-returned");</script>', False),
}

PAGE = """<!doctype html>
<meta charset=utf-8>
<title>BUG-799 probe: %(name)s</title>
<body>
%(body)s
<script>
console.log("PROBE script-start");
var n = 0;
setInterval(function () { console.log("PROBE tick " + (++n)); }, 500);
</script>
</body>
"""

_TICK_RE = re.compile(r"PROBE tick (\d+)")
_MARKER_RE = re.compile(r"PROBE ([a-z-]+(?: \S+)?)")


class _Quiet(http.server.SimpleHTTPRequestHandler):
    """Serves the vendored WPT tree; silent so the probe output stays readable."""

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
    log_path = os.path.join(REPO, ".tmp", f"bug799-{name}.log")
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    with open(log_path, "w", encoding="utf-8") as log:
        proc = subprocess.Popen(
            [binary, "--mcp-live-port", str(_free_port()),
             f"http://127.0.0.1:{http_port}/.bug799-{name}.html"],
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
    markers = [m for m in _MARKER_RE.findall(text) if not m.startswith("tick")]
    return ticks, markers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=12.0,
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
        path = os.path.join(HERE, f".bug799-{name}.html")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(PAGE % {"name": name, "body": body})
        written.append(path)
    http_port, shutdown = _serve(HERE)
    try:
        print(f"{'variant':20s} {'ticks':>6s}  {'expected':10s} markers")
        wedged = []
        for name in wanted:
            ticks, markers = _run_variant(args.binary, name, http_port, args.seconds)
            expected = "alive" if VARIANTS[name][1] else "wedged"
            print(f"{name:20s} {ticks:6d}  {expected:10s} "
                  f"{', '.join(markers) if markers else '—'}")
            if ticks == 0:
                wedged.append(name)
        print()
        if wedged:
            print("wedged (no page JS ran to completion):", ", ".join(wedged))
            print("=> BUG-799 reproduced: setting `src` on an <audio> never "
                  "returns, so no page script, timer or harness callback runs "
                  "again — the TIMEOUT is a frozen page, not a missing event")
        else:
            print("=> nothing wedged; BUG-799 no longer reproduces on this build")
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
