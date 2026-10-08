#!/usr/bin/env python3
"""BUG-999 investigation: measure how long a programmatically inserted
`<iframe src>` (the exact `createRealm()` idiom from
`webidl/ecmascript-binding/support/create-realm.js` — create, set
`onload`, set `src`, `document.body.append(iframe)`) takes to fire `load`,
and whether it fires at all, across repeated runs on the SAME live window.

Modeled on `verify_s3_bidi_session.py` (bare `BidiSession`, no wptrunner
orchestration) — this measures the engine's own timing for FRAME-8
(`crates/shell/src/lumen/frame_dynamic.rs`: dynamic-frame detection is
polled on `dom_dirty`, and background-thread dispatch is deliberately
delayed one full event-loop tick after detection), not wptrunner's poll
loop. A plain `http.server.SimpleHTTPRequestHandler` (no CGI/`?pipe=`
features needed here per `docs/probe-method.md`) serves a tiny local
fixture that reproduces the exact idiom without vendored WPT files.

IMPORTANT: this does NOT use `script.callFunction` or `awaitPromise:true`.
`script.callFunction` (`crates/bidi-server/src/protocol.rs::script_call_function`)
is a Phase-1 stub that forwards to `script_evaluate`, which requires an
`expression` field `callFunction` never sends — so any `callFunction` call
silently no-ops and returns `{type:"undefined"}` without ever running the
function body. Separately, `script.evaluate`'s `awaitPromise:true`
(`eval_await_promise`) only resolves promises that settle within a single
microtask checkpoint; a promise that settles on a later macrotask (like
`iframe.onload`, which needs real navigation/network I/O) is reported
`state:"pending"` and falls back to the placeholder promise object — it is
NOT actually awaited to completion. Both are pre-existing, documented
limitations of the BiDi stub layer, not new findings. To measure the
engine's real iframe-load timing around these limitations, this script
fires `runOnce()` with `script.evaluate`/`awaitPromise:false` (fire-and-
forget) and then polls a plain global flag with repeated, separate
`script.evaluate` round-trips until it flips or a deadline passes.

Usage (from repo root):

    tests/wpt/.venv/Scripts/python.exe tests/wpt/verify_bug999_dynamic_iframe_load.py \
        [--binary target/dev-release/lumen.exe] [--runs 5]

Exit code is always 0 — this is a measurement, not a gate.
"""

import argparse
import asyncio
import http.server
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
]

from webdriver.bidi.client import BidiSession  # noqa: E402
from webdriver.bidi.modules.script import ContextTarget  # noqa: E402

PAGE_HTML = b"""<!DOCTYPE html>
<meta charset="utf-8">
<body>
<script>
window.__lastResult = null;
window.__iframe = null;
window.startRun = function () {
  window.__lastResult = null;
  const t0 = performance.now();
  const iframe = document.createElement("iframe");
  window.__iframe = iframe;
  iframe.onload = () => {
    window.__lastResult = { ok: true, ms: performance.now() - t0, via: "onload-prop" };
  };
  iframe.addEventListener("load", () => {
    if (!window.__lastResult) {
      window.__lastResult = { ok: true, ms: performance.now() - t0, via: "addEventListener" };
    }
  });
  iframe.name = "dummy";
  iframe.src = "dummy-iframe.html?r=" + t0;
  document.body.append(iframe);
};
window.readLastResult = function () { return window.__lastResult; };
window.readDiag = function () {
  const f = window.__iframe;
  if (!f) return { has_iframe: false };
  let doc_state = "no-access";
  let body_text = null;
  try {
    doc_state = f.contentDocument ? f.contentDocument.readyState : "null-doc";
    body_text = f.contentDocument && f.contentDocument.body ? f.contentDocument.body.textContent : null;
  } catch (e) {
    doc_state = "error:" + e;
  }
  return { has_iframe: true, doc_state: doc_state, body_text: body_text, complete_prop: f.complete };
};
</script>
"""

DUMMY_HTML = b"""<!DOCTYPE html>
<meta charset="utf-8">
<body>foo
<button id="element"></button>
"""


def get_free_port() -> int:
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def wait_for_port(host, port, proc, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"lumen exited early with code {proc.returncode}")
        s = socket.socket()
        try:
            s.connect((host, port))
            return
        except OSError:
            time.sleep(0.05)
        finally:
            s.close()
    raise TimeoutError(f"BiDi port {port} did not open within {timeout}s")


def read_token_and_drain(stderr) -> str:
    token = None
    for _ in range(400):
        line = stderr.readline()
        if not line:
            break
        line = line.strip()
        if line.startswith("[bidi] token: "):
            token = line[len("[bidi] token: "):]
            break
    if token is None:
        raise RuntimeError("lumen --bidi-port did not print [bidi] token")

    def _drain():
        try:
            for line in stderr:
                print(f"[lumen-stderr] {line.rstrip()}")
        except Exception:
            pass

    threading.Thread(target=_drain, daemon=True).start()
    return token


def start_static_server(root):
    handler = lambda *a, **kw: http.server.SimpleHTTPRequestHandler(*a, directory=root, **kw)
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    return httpd


async def poll_for_result(session, context, deadline, poll_interval=0.05):
    # BUG-999 investigation, 2026-09-29: Lumen's `script.evaluate` never
    # returns a structural `type:"object"` RemoteValue for a non-primitive
    # result — non-scalar values are always JSON.stringify'd and tagged
    # `type:"string"` (`crates/bidi-server/src/protocol.rs:1235`, matching
    # convention, not a bug — `executorlumen.py`'s own `POLL_EXPRESSION`
    # relies on the exact same shape). Checking for `type=="object"` here
    # was wrong and made this probe report TIMEOUT on every run regardless
    # of whether `load` actually fired.
    while time.monotonic() < deadline:
        result = await session.script.evaluate(
            expression="window.readLastResult()",
            target=ContextTarget(context),
            await_promise=False,
        )
        if result.get("type") == "string":
            return json.loads(result["value"])
        await asyncio.sleep(poll_interval)
    return None


async def run_probe(ws_url, token, page_url, runs, timeout=5.0):
    session = BidiSession.bidi_only(ws_url, requested_capabilities={"alwaysMatch": {"token": token}})
    await session.start()
    try:
        contexts = await session.browsing_context.get_tree()
        context = contexts[0]["context"]
        await session.browsing_context.navigate(context=context, url=page_url, wait="complete")
        print(f"[probe] navigated to {page_url}")

        results = []
        for i in range(runs):
            t0 = time.monotonic()
            await session.script.evaluate(
                expression="window.startRun()",
                target=ContextTarget(context),
                await_promise=False,
            )
            result = await poll_for_result(session, context, deadline=t0 + timeout)
            elapsed = time.monotonic() - t0
            if result is not None:
                print(f"[probe] run {i}: wall={elapsed:.3f}s value={result}")
                results.append((True, elapsed, result))
            else:
                diag = await session.script.evaluate(
                    expression="window.readDiag()",
                    target=ContextTarget(context),
                    await_promise=False,
                )
                print(f"[probe] run {i}: TIMEOUT after {elapsed:.1f}s wall (no `load` delivered) diag={diag}")
                results.append((False, elapsed, None))
        return results
    finally:
        await session.end()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen.exe"))
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    tmp_dir = tempfile.mkdtemp(prefix="bug999_")
    with open(os.path.join(tmp_dir, "page.html"), "wb") as f:
        f.write(PAGE_HTML)
    with open(os.path.join(tmp_dir, "dummy-iframe.html"), "wb") as f:
        f.write(DUMMY_HTML)

    httpd = start_static_server(tmp_dir)
    http_port = httpd.server_address[1]
    page_url = f"http://127.0.0.1:{http_port}/page.html"
    print(f"[probe] static server on {http_port}, serving {tmp_dir}")

    host = "127.0.0.1"
    bidi_port = get_free_port()
    proc = subprocess.Popen(
        [args.binary, "--bidi-port", str(bidi_port)],
        stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
    )
    try:
        wait_for_port(host, bidi_port, proc, timeout=30)
        token = read_token_and_drain(proc.stderr)
        results = asyncio.run(run_probe(f"ws://{host}:{bidi_port}", token, page_url, args.runs))
        ok_count = sum(1 for ok, _, _ in results if ok)
        print(f"[probe] SUMMARY: {ok_count}/{len(results)} runs delivered `load`")
        for i, (ok, elapsed, value) in enumerate(results):
            print(f"  run {i}: ok={ok} wall={elapsed:.3f}s value={value}")
    except Exception as e:
        print(f"[probe] FAILED: {e}", file=sys.stderr)
        return 1
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        httpd.shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(main())
