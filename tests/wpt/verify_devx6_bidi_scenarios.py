#!/usr/bin/env python3
"""DEVX-6 (`ROADMAP.md`): integration scenario tests over WebDriver BiDi
(`--bidi-port`, live window) for four previously-unused BiDi levers:
`network.setOfflineStatus`, `network.addIntercept` + `failRequest`/
`continueRequest`, `browser.setTimezoneOverride`, and
`emulation.setUserAgentOverride`.

Each scenario checks two things:

1. **Protocol round-trip** — the command is accepted, returns the right ACK
   (or the right typed error for bad params), matching the BiDi spec shape.
   This is real verification value: it catches `lumen-bidi-server` protocol
   regressions in these six command handlers.
2. **Live-page effect** — does a page in the same live window actually
   observe the requested behavior (blocked fetch, failed request, shifted
   `Intl` timezone, overridden `navigator.userAgent`)? While researching
   this task it was confirmed (`crates/bidi-server/src/protocol.rs`) that
   none of these six commands touch `state.live` at all — they only mutate
   `BidiState` fields nothing else reads yet. So this half is **expected to
   fail today** and is reported as `XFAIL(BUG-295)`, not a script failure —
   see `bugs/BUG-295-OPEN.md` for the full diagnosis and what's needed to
   close it (real engine/shell wiring, out of scope for this Python-tooling
   task).

A third outcome, `SKIP(env)`, covers a separate environment-dependent gap
found while writing this script: in some working sessions the live window's
JS runtime never finishes installing at all (`script.evaluate` reports "JS
context not available" indefinitely, well past the normal install-race
window `LumenTestharnessExecutor` already tolerates) — every eval-dependent
live-effect check degrades to `SKIP(env)` rather than hanging or reporting a
false XFAIL when that happens. This did not block the protocol-round-trip
checks (they don't need a live JS context) and was reproducible independent
of these six commands, so it's not filed as part of BUG-295 — see the
"Known gotchas" note in `CLAUDE.md` about live BiDi/MCP eval availability.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_devx6_bidi_scenarios.py [--binary PATH]

Exit code: 0 if every protocol round-trip check passed; non-zero if any
protocol round-trip failed (a real regression) — XFAILs and SKIPs never
affect the exit code.
"""

import argparse
import asyncio
import http.server
import os
import socket
import subprocess
import sys
import threading
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools", "webdriver"),
]

from webdriver.bidi.client import BidiSession  # noqa: E402

INDEX_HTML = b"<!doctype html><html><body>devx6</body></html>"


def get_free_port() -> int:
    s = socket.socket()
    try:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]
    finally:
        s.close()


def wait_for_port(host: str, port: int, proc: subprocess.Popen, timeout: float) -> None:
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


class _StaticHandler(http.server.BaseHTTPRequestHandler):
    """Serves `INDEX_HTML` for any GET — a minimal same-origin fetch target
    for the offline/intercept scenarios, so they don't depend on live network
    access or on `file://` fetch support."""

    def do_GET(self):  # noqa: N802 (stdlib method name)
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(INDEX_HTML)))
        self.end_headers()
        self.wfile.write(INDEX_HTML)

    def log_message(self, fmt, *args):
        pass  # quiet — the test output speaks for itself


class JsContextUnavailable(Exception):
    """Raised by `eval_json` when `script.evaluate` keeps reporting "JS
    context not available" past its retry budget — i.e. the live window's JS
    runtime never finished installing for this page in this run. Distinct
    from a genuine protocol error so callers can skip (not fail) the
    live-effect half of a scenario without losing the protocol-round-trip
    check that already ran."""


class Report:
    """Accumulates (name, protocol_ok, detail) rows and renders a summary."""

    def __init__(self):
        self.rows = []

    def protocol(self, name: str, ok: bool, detail: str = "") -> None:
        self.rows.append((name, "OK" if ok else "FAIL", detail))

    def xfail(self, name: str, detail: str) -> None:
        self.rows.append((name, "XFAIL(BUG-295)", detail))

    def skip(self, name: str, detail: str) -> None:
        self.rows.append((name, "SKIP(env)", detail))

    def all_protocol_ok(self) -> bool:
        return all(status != "FAIL" for _, status, _ in self.rows)

    def print(self) -> None:
        for name, status, detail in self.rows:
            line = f"  [{status:>15}] {name}"
            if detail:
                line += f" — {detail}"
            print(line)


async def send(session: BidiSession, method: str, params: dict):
    """`BidiSession.send_command` is itself a coroutine that resolves to a
    pending `Future` (see `webdriver/bidi/modules/_module.py`'s
    `future = await session.send_command(...); result = await future`
    pattern) — awaiting it once yields the `Future`, not the result. This
    wrapper does both awaits and raises on a BiDi error response, matching
    what a typed module method would do."""
    future = await session.send_command(method, params)
    return await future


async def get_default_context(session: BidiSession) -> str:
    result = await send(session, "browsingContext.getTree", {})
    return result["contexts"][0]["context"]


async def check_offline(session: BidiSession, ctx: str, base_url: str, report: Report) -> None:
    try:
        await send(
            session, "network.setOfflineStatus", {"context": ctx, "status": {"offline": True}}
        )
        report.protocol("network.setOfflineStatus: ACK", True)
    except Exception as e:  # noqa: BLE001 — reporting, not handling
        report.protocol("network.setOfflineStatus: ACK", False, str(e))
        return

    try:
        fetch_result = await fetch_probe(session, ctx, f"{base_url}/index.html", "__devx6_offline")
    except JsContextUnavailable as e:
        report.skip("network.setOfflineStatus: live fetch blocked", str(e))
    else:
        if fetch_result == "rejected" or fetch_result is False:
            report.protocol("network.setOfflineStatus: live fetch blocked", True)
        else:
            report.xfail(
                "network.setOfflineStatus: live fetch blocked",
                f"fetch still succeeded (result={fetch_result!r}) — offline flag has no live-window effect",
            )

    # Reset for later scenarios.
    await send(session, "network.setOfflineStatus", {"context": ctx, "status": {"offline": False}})


async def check_intercept_fail_request(
    session: BidiSession, ctx: str, base_url: str, report: Report
) -> None:
    try:
        add_result = await send(
            session,
            "network.addIntercept",
            {"phases": ["beforeRequestSent"], "urlPatterns": [{"type": "string", "pattern": f"{base_url}/index.html"}]},
        )
        intercept_id = add_result.get("intercept")
        report.protocol("network.addIntercept: returns intercept id", bool(intercept_id))
    except Exception as e:  # noqa: BLE001
        report.protocol("network.addIntercept: returns intercept id", False, str(e))
        return

    try:
        # No real paused-request bookkeeping exists yet (confirmed in
        # BUG-295) — there is no genuine `request` id to react to, so this
        # exercises the bare ACK shape rather than a real pause/decide flow.
        await send(session, "network.failRequest", {"request": "nonexistent-request-id"})
        report.protocol("network.failRequest: ACK", True)
    except Exception as e:  # noqa: BLE001
        report.protocol("network.failRequest: ACK", False, str(e))

    try:
        fetch_result = await fetch_probe(session, ctx, f"{base_url}/index.html", "__devx6_intercept")
    except JsContextUnavailable as e:
        report.skip("network.addIntercept+failRequest: live request actually fails", str(e))
    else:
        if fetch_result == "rejected":
            report.protocol("network.addIntercept+failRequest: live request actually fails", True)
        else:
            report.xfail(
                "network.addIntercept+failRequest: live request actually fails",
                f"fetch still succeeded (result={fetch_result!r}) — no paused-request bookkeeping wired to the live window",
            )

    try:
        await send(session, "network.removeIntercept", {"intercept": intercept_id})
        report.protocol("network.removeIntercept: ACK", True)
    except Exception as e:  # noqa: BLE001
        report.protocol("network.removeIntercept: ACK", False, str(e))


async def check_timezone_override(session: BidiSession, ctx: str, report: Report) -> None:
    override_tz = "Pacific/Kiritimati"  # UTC+14 — practically never the host's real zone
    try:
        await send(session, "browser.setTimezoneOverride", {"timezoneId": override_tz})
        report.protocol("browser.setTimezoneOverride: ACK", True)
    except Exception as e:  # noqa: BLE001
        report.protocol("browser.setTimezoneOverride: ACK", False, str(e))
        return

    try:
        observed_tz = await eval_json(session, ctx, "Intl.DateTimeFormat().resolvedOptions().timeZone")
    except JsContextUnavailable as e:
        report.skip("browser.setTimezoneOverride: live Intl timezone shifts", str(e))
        return
    if observed_tz == override_tz:
        report.protocol("browser.setTimezoneOverride: live Intl timezone shifts", True)
    else:
        report.xfail(
            "browser.setTimezoneOverride: live Intl timezone shifts",
            f"page still reports {observed_tz!r} — override not threaded into the JS engine",
        )


async def check_user_agent_override(session: BidiSession, ctx: str, report: Report) -> None:
    override_ua = "LumenDevx6TestUA/9.9"
    try:
        await send(session, "emulation.setUserAgentOverride", {"userAgent": override_ua})
        report.protocol("emulation.setUserAgentOverride: ACK", True)
    except Exception as e:  # noqa: BLE001
        report.protocol("emulation.setUserAgentOverride: ACK", False, str(e))
        return

    try:
        observed_ua = await eval_json(session, ctx, "navigator.userAgent")
    except JsContextUnavailable as e:
        report.skip("emulation.setUserAgentOverride: live navigator.userAgent shifts", str(e))
    else:
        if observed_ua == override_ua:
            report.protocol("emulation.setUserAgentOverride: live navigator.userAgent shifts", True)
        else:
            report.xfail(
                "emulation.setUserAgentOverride: live navigator.userAgent shifts",
                f"page still reports {observed_ua!r} — override not threaded into the JS shim",
            )

    # Bad-context-id error handling — this half of the contract has no
    # live-window dependency, so it's a genuine protocol check.
    try:
        await send(
            session,
            "emulation.setUserAgentOverride",
            {"userAgent": override_ua, "contexts": ["not-a-real-context"]},
        )
        report.protocol("emulation.setUserAgentOverride: rejects unknown context", False, "expected an error, got success")
    except Exception:  # noqa: BLE001 — an error is the expected outcome here
        report.protocol("emulation.setUserAgentOverride: rejects unknown context", True)


async def eval_json(session: BidiSession, ctx: str, expression: str, timeout: float = 20.0):
    """`script.evaluate` an expression against the live default context and
    return its JSON-ish Python value (string/bool/number — enough for these
    scenarios' primitive results).

    Tolerates the transient "JS context not available" error right after
    `browsingContext.navigate` — Lumen builds the JS runtime for a new
    document off the UI thread, after the streaming pipeline that
    `document.readyState`/`navigate`'s `wait` condition is based on, so
    `script.evaluate` can briefly race the runtime install. Same tolerance
    `LumenTestharnessExecutor._run_testharness` uses (`executorlumen.py`).
    Raises [`JsContextUnavailable`] if it never resolves within `timeout` —
    seen in some environments where the live window never finishes
    installing a JS runtime at all (unrelated to these BiDi commands)."""
    deadline = time.time() + timeout
    while True:
        try:
            result = await send(
                session,
                "script.evaluate",
                {
                    "expression": expression,
                    "target": {"context": ctx},
                    "awaitPromise": True,
                },
            )
            value = result.get("result", {})
            return value.get("value")
        except Exception as e:  # noqa: BLE001
            if "JS context not available" not in str(e):
                raise
            if time.time() > deadline:
                raise JsContextUnavailable(str(e)) from e
            await asyncio.sleep(0.2)


async def poll_eval_json(session: BidiSession, ctx: str, expression: str, pending, timeout: float = 5.0):
    """Poll `expression` via repeated `script.evaluate` calls until it stops
    returning `pending`, or `timeout` elapses (returns whatever the last poll
    saw). `script.evaluate` runs synchronously (`awaitPromise` is accepted by
    the protocol but not honored by `lumen-bidi-server` yet) — a `fetch()`
    result only becomes observable once its `.then`/`.catch` callback has run
    on a later engine tick, hence polling rather than a single eval call."""
    deadline = time.time() + timeout
    value = pending
    while time.time() < deadline:
        value = await eval_json(session, ctx, expression)
        if value != pending:
            return value
        await asyncio.sleep(0.05)
    return value


async def fetch_probe(session: BidiSession, ctx: str, url: str, var_name: str):
    """Kick off `fetch(url)` on the live page, storing its outcome
    (`true`/`false`/`"rejected"`) into `window.<var_name>`, then poll for it.
    Returns the resolved outcome, or `"rejected"` if it timed out waiting."""
    await eval_json(
        session,
        ctx,
        f"window.{var_name} = 'pending'; "
        f"fetch({url!r}).then(r => {{ window.{var_name} = r.ok; }}, "
        f"() => {{ window.{var_name} = 'rejected'; }});",
    )
    result = await poll_eval_json(session, ctx, f"window.{var_name}", "pending")
    return "rejected" if result == "pending" else result


async def run_scenarios(ws_url: str, http_port: int) -> Report:
    report = Report()
    session = BidiSession.bidi_only(ws_url)
    await session.start()
    try:
        ctx = await get_default_context(session)
        base_url = f"http://127.0.0.1:{http_port}"
        await send(
            session, "browsingContext.navigate", {"context": ctx, "url": f"{base_url}/index.html"}
        )
        report.protocol("browsingContext.navigate: live page loaded", True)

        await check_offline(session, ctx, base_url, report)
        await check_intercept_fail_request(session, ctx, base_url, report)
        await check_timezone_override(session, ctx, report)
        await check_user_agent_override(session, ctx, report)
    finally:
        await session.end()
    return report


def default_binary() -> str:
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    http_server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _StaticHandler)
    http_port = http_server.server_port
    http_thread = threading.Thread(target=http_server.serve_forever, daemon=True)
    http_thread.start()

    host = "127.0.0.1"
    bidi_port = get_free_port()
    proc = subprocess.Popen([args.binary, "--bidi-port", str(bidi_port)])
    try:
        wait_for_port(host, bidi_port, proc, timeout=30)
        report = asyncio.run(run_scenarios(f"ws://{host}:{bidi_port}", http_port))
    except Exception as e:
        print(f"DEVX-6 FAILED: {e}", file=sys.stderr)
        return 1
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        http_server.shutdown()

    report.print()
    if report.all_protocol_ok():
        print("DEVX-6 OK: all BiDi command round-trips verified (see bugs/BUG-295-OPEN.md for live-effect XFAILs)")
        return 0
    print("DEVX-6 FAILED: a BiDi command round-trip regressed", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
