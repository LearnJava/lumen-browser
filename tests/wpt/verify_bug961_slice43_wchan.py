#!/usr/bin/env python3
"""BUG-961 срез 43: "Что нужно" item 2 — profile the `lumen` process itself
during the 31s silent window to find what it is actually blocked on.

Срез 42 refuted content-serving as the variable: a byte-identical static
copy, served by a plain `http.server` with zero relation to wptserve, stalls
the same ~32s as the live wptserve route. This slice therefore does not need
`wptrunner`/`wptserve`/`TestEnvironment`/`executorlumen.py` at all — it
spawns `lumen --bidi-port <port>` directly with `subprocess.Popen` (bypassing
`LumenBrowser`'s process management entirely, per "Что нужно" item 2's first
option) and samples `/proc/<pid>/task/*/wchan` (+ `/proc/<pid>/task/*/comm`,
`/proc/<pid>/task/*/stack` when readable) for every thread every ~150ms
through the stall window. No `perf`/`gdb` on this box (`gdb` only attaches to
its own child per `ptrace_scope`, and this probe's `lumen` is exactly that —
its own child — but sampling `/proc` is cheaper and needs no debugger
attach/detach dance around the BiDi client holding the connection open).

The page content is a static copy of the real
`/console/console-log-large-array.any.html` wrapper — captured once by hand
from a live `run_report.py` run's `curl` (see `bugs/BUG-961-OPEN.md` срез 39
point 3) and re-served verbatim here, since срез 42 already proved content
identity doesn't change the outcome; no need to boot wptserve just to refetch
the same bytes.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice43_wchan.py
        [--binary target/dev-release/lumen] [--seconds 40]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import asyncio
import glob
import http.server
import os
import socket
import socketserver
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")

sys.path[:0] = [
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    TESTS_ROOT,  # for serve_wpt_like's _build_testharnessreport()
]

import serve_wpt_like  # noqa: E402

#: Same test id срез 39-42 investigate.
TEST_ID = "/console/console-log-large-array.any.html"

#: Byte-identical wrapper wptserve's `AnyHtmlHandler` generates for
#: `TEST_ID` — captured live in срез 39 point 2/срез 42 (this is exactly what
#: срез 42's Variant C served, which stalled identically to the live route,
#: so re-using it here carries over that already-proven equivalence).
_ANY_HTML_TEMPLATE = """<!doctype html>
<meta charset=utf-8>
<script src=/resources/testharness.js></script>
<script src=/resources/testharnessreport.js></script>
<div id=log></div>
<script>
self.GLOBAL = {
  isWindow: function() { return true; },
  isWorker: function() { return false; },
  isShadowRealm: function() { return false; },
};
</script>
<script src=/console/console-log-large-array.any.js></script>
"""


def _free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _make_static_server():
    """Plain `http.server`, own thread, own port — zero relation to
    wptserve, matching срез 42's Variant C setup."""

    body = _ANY_HTML_TEMPLATE.encode("utf-8")

    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            if path == TEST_ID:
                self.send_response(200)
                self.send_header("Content-Type", "text/html;charset=utf8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            if path == "/resources/testharnessreport.js":
                # Substituted the same way `serve_wpt_like.py` does — a raw
                # `SimpleHTTPRequestHandler` fallback would serve the file
                # unsubstituted, a `SyntaxError: Unexpected token '%'` that
                # reads exactly like a hung page (WPT-RUN-6 срез 37's
                # gotcha, CLAUDE.md "WPT harness").
                rbody = serve_wpt_like._build_testharnessreport()
                self.send_response(200)
                self.send_header("Content-Type", "text/javascript;charset=utf8")
                self.send_header("Content-Length", str(len(rbody)))
                self.end_headers()
                self.wfile.write(rbody)
                return
            return super().do_GET()

    def handler(*a, **kw):
        return Handler(*a, directory=TESTS_ROOT, **kw)

    httpd = socketserver.TCPServer(("127.0.0.1", 0), handler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    return httpd, port


class _WchanSampler:
    """Polls `/proc/<pid>/task/*/{comm,wchan}` at a fixed interval and keeps
    a (timestamp, thread_id, comm, wchan) tuple per sample. `wchan` is the
    kernel symbol a sleeping thread is parked in — a futex/condvar wait names
    the specific lock, a `poll`/`epoll_wait` names a network-side wait
    instead ("Что нужно" item 2's own framing)."""

    def __init__(self, pid, interval=0.15):
        self.pid = pid
        self.interval = interval
        self.samples = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self):
        self._thread.start()

    def stop(self):
        self._stop.set()
        self._thread.join(timeout=2)

    def _run(self):
        task_glob = f"/proc/{self.pid}/task/*"
        while not self._stop.is_set():
            t = time.monotonic()
            for task_dir in glob.glob(task_glob):
                tid = os.path.basename(task_dir)
                comm = _read_first_line(os.path.join(task_dir, "comm"))
                wchan = _read_first_line(os.path.join(task_dir, "wchan"))
                if comm is not None or wchan is not None:
                    self.samples.append((t, tid, comm, wchan))
            time.sleep(self.interval)


def _read_first_line(path):
    try:
        with open(path, "r") as f:
            return f.readline().strip()
    except OSError:
        return None


def _summarize(samples, t_start):
    """Group samples by (comm, wchan) and print counts + first/last relative
    timestamp — enough to see which thread was parked on what, and for how
    long, without dumping every single sample."""
    if not samples:
        print("[probe] no /proc samples collected (process exited too fast?)")
        return
    by_key = {}
    for t, tid, comm, wchan in samples:
        key = (tid, comm, wchan)
        rel = t - t_start
        if key not in by_key:
            by_key[key] = [rel, rel, 0]
        entry = by_key[key]
        entry[0] = min(entry[0], rel)
        entry[1] = max(entry[1], rel)
        entry[2] += 1
    print(f"[probe] {len(samples)} raw samples, {len(by_key)} distinct "
          f"(tid, comm, wchan) states:")
    for (tid, comm, wchan), (first, last, count) in sorted(
            by_key.items(), key=lambda kv: -kv[1][2]):
        print(f"  tid={tid:>7} comm={comm!r:<20} wchan={wchan!r:<28} "
              f"count={count:>4} span=[{first:6.2f}s .. {last:6.2f}s]")


def _spawn_lumen(binary, bidi_port):
    env = dict(os.environ)
    env["LUMEN_NO_ADBLOCK"] = "1"
    cmd = [binary, "--bidi-port", str(bidi_port)]
    print(f"[probe] spawning directly (no wptrunner/mozprocess in the loop): "
          f"{' '.join(cmd)}")
    proc = subprocess.Popen(
        cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        bufsize=1, universal_newlines=False)
    print(f"[probe] lumen pid={proc.pid}")
    return proc


#: Lines shorter than this are echoed verbatim (normal log chatter); longer
#: ones are the array dump itself — echoing 20MB of "x,x,x,..." would dwarf
#: the actual finding, so those are logged as (length, elapsed) only.
_ECHO_LEN_CUTOFF = 2000


def _drain_stderr(proc, token_box, events, t_process_start, stop_after):
    """Runs for the whole probe lifetime (not just until the token is seen):
    tags every stderr line with elapsed-since-process-start, captures the
    bidi token, and — the point of this slice — timestamps the FIRST line
    long enough to be the array dump (`events["huge_line_at"]`), so it can be
    correlated against the wchan samples and against when `navigate()`
    itself returned."""
    prefix = b"[bidi] token: "
    while True:
        line = proc.stderr.readline()
        if not line:
            if proc.poll() is not None or time.monotonic() - t_process_start > stop_after:
                return
            continue
        elapsed = time.monotonic() - t_process_start
        if len(line) > _ECHO_LEN_CUTOFF:
            if events.get("huge_line_at") is None:
                events["huge_line_at"] = elapsed
                events["huge_line_len"] = len(line)
            print(f"[lumen stderr @ {elapsed:6.2f}s] <{len(line)}-byte line, "
                  f"not echoed> {line[:60]!r}...")
        else:
            print(f"[lumen stderr @ {elapsed:6.2f}s] {line.decode('utf-8', 'replace').rstrip()}")
        if token_box.get("token") is None and line.startswith(prefix):
            token_box["token"] = line[len(prefix):].decode("utf-8", "replace").strip()
        if time.monotonic() - t_process_start > stop_after:
            return


def _wait_port(host, port, deadline):
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.05)
    return False


async def _navigate_and_sample(url, token, pid, seconds, events, t_process_start):
    from webdriver.bidi.client import BidiSession

    sampler = _WchanSampler(pid)
    session = BidiSession.bidi_only(
        f"ws://127.0.0.1:{PORT_BOX['bidi_port']}",
        requested_capabilities={"alwaysMatch": {"token": token}})
    await session.start(asyncio.get_event_loop())
    try:
        contexts = await session.browsing_context.get_tree()
        context = contexts[0]["context"]
        sampler.start()
        t0 = time.monotonic()
        try:
            await asyncio.wait_for(
                session.browsing_context.navigate(
                    context=context, url=url, wait="complete"),
                timeout=seconds)
            elapsed = time.monotonic() - t0
            events["navigate_done_at"] = time.monotonic() - t_process_start
            print(f"[probe] navigate returned OK in {elapsed:.2f}s "
                  f"(t={events['navigate_done_at']:.2f}s since process start)")
        except asyncio.TimeoutError:
            elapsed = time.monotonic() - t0
            events["navigate_done_at"] = None
            print(f"[probe] asyncio.wait_for timed out after {elapsed:.1f}s")
        except Exception as e:  # noqa: BLE001 — report whatever BiDi raised
            elapsed = time.monotonic() - t0
            events["navigate_done_at"] = None
            print(f"[probe] navigate raised after {elapsed:.1f}s: {e!r}")

        # The point of this slice: keep sampling/observing PAST navigate()'s
        # own return, up to the same total budget, so a case where navigate
        # returns fast but the huge line arrives much later (as opposed to
        # срез 39-42's "navigate itself never returns") is visible instead of
        # being cut off right when navigate() resolves.
        remaining = seconds - (time.monotonic() - t0)
        while remaining > 0 and events.get("huge_line_at") is None:
            await asyncio.sleep(min(0.2, remaining))
            remaining = seconds - (time.monotonic() - t0)
        sampler.stop()
        return sampler.samples, t_process_start
    finally:
        await session.end()


PORT_BOX = {}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=40.0)
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    httpd, static_port = _make_static_server()
    static_url = f"http://127.0.0.1:{static_port}{TEST_ID}"
    print(f"[probe] static server up on port {static_port}: {static_url}")

    bidi_port = _free_port()
    PORT_BOX["bidi_port"] = bidi_port
    t_process_start = time.monotonic()
    proc = _spawn_lumen(args.binary, bidi_port)

    token_box = {"token": None}
    events = {"huge_line_at": None, "huge_line_len": None, "navigate_done_at": None}
    # Runs for the whole probe (`stop_after` mirrors `--seconds` plus slack
    # for process-startup time already spent), not just until the token
    # line — see `_drain_stderr`'s docstring for why.
    reader = threading.Thread(
        target=_drain_stderr,
        args=(proc, token_box, events, t_process_start, args.seconds + 15),
        daemon=True)
    reader.start()

    try:
        if not _wait_port("127.0.0.1", bidi_port, time.time() + 15):
            print("lumen did not open the bidi port within 15s", file=sys.stderr)
            return 1
        deadline = time.time() + 10
        while token_box["token"] is None and time.time() < deadline:
            time.sleep(0.05)
        if token_box["token"] is None:
            print("did not see the '[bidi] token: ' stderr line within 10s",
                  file=sys.stderr)
            return 1
        print(f"[probe] bidi port up, token captured, pid={proc.pid}")

        samples, t0 = asyncio.run(
            _navigate_and_sample(static_url, token_box["token"], proc.pid,
                                  args.seconds, events, t_process_start))
        _summarize(samples, t0)
        print(f"[probe] timeline: navigate_done_at="
              f"{events['navigate_done_at']!r}s huge_line_at="
              f"{events['huge_line_at']!r}s (len={events['huge_line_len']!r})")
    finally:
        httpd.shutdown()
        httpd.server_close()
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
