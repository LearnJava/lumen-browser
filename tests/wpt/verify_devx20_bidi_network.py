#!/usr/bin/env python3
"""DEVX-20 (`ROADMAP.md`): live-window scenarios for the BiDi network/storage
tails left by BUG-295 — the `responseStarted` intercept phase with
`network.continueResponse`, the honest `authRequired` answer, and
`storage.getCookies|setCookie|deleteCookies` against the real cookie jar.

Checks (against a real window started with `--bidi-port`):

1. **authRequired is not faked** — `network.addIntercept` with the
   `authRequired` phase is `unsupported operation`; `network.continueWithAuth`
   and `network.continueResponse` for an unknown request are `no such request`.
2. **Cookies reach the live jar** — a `Set-Cookie` from the local test server is
   visible through `storage.getCookies`; `storage.setCookie` shows up in the
   page's `document.cookie`; `storage.deleteCookies` removes it from both.
3. **responseStarted pause** — `network.addIntercept{responseStarted}` on a
   `fetch()`: the server has already received the request, the page's promise
   stays pending, `network.responseStarted` arrives with `isBlocked: true` and
   the real status, `network.continueResponse` lets the page finish; and
   `network.failRequest` on a second paused response rejects the page's fetch.

Usage (from the repo root, after `pip install -r tests/wpt/requirements.txt`
in a venv — see tests/wpt/README.md):

    LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_devx20_bidi_network.py [--binary PATH]

Exit code: 0 if every check passed or was skipped (`SKIP(env)`: the live JS
runtime never installed, same tolerance as `verify_devx6_bidi_scenarios.py`);
non-zero on any FAIL.
"""

import argparse
import asyncio
import http.server
import os
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from verify_devx6_bidi_scenarios import (  # noqa: E402
    JsContextUnavailable,
    Report,
    default_binary,
    eval_json,
    get_default_context,
    get_free_port,
    poll_eval_json,
    read_token_and_drain,
    send,
    wait_for_port,
)
from webdriver.bidi.client import BidiSession  # noqa: E402

PAGE_HTML = b"<!doctype html><html><body>devx20</body></html>"
DATA_BODY = b"payload"
SERVER_COOKIE = "devx20_server"
BIDI_COOKIE = "devx20_bidi"


class _Handler(http.server.BaseHTTPRequestHandler):
    """`/page.html` sets a cookie; `/data*.txt` serve a small text body and
    count hits (so a scenario can prove the server already got the request
    while the response is still paused in the browser)."""

    hits: dict = {}
    lock = threading.Lock()

    def do_GET(self):  # noqa: N802 (stdlib method name)
        path = self.path.split("?")[0]
        with _Handler.lock:
            _Handler.hits[path] = _Handler.hits.get(path, 0) + 1
        if path == "/page.html":
            body, ctype = PAGE_HTML, "text/html"
        else:
            body, ctype = DATA_BODY, "text/plain"
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        if path == "/page.html":
            self.send_header("Set-Cookie", f"{SERVER_COOKIE}=from-server; Path=/")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        pass


def hit_count(path: str) -> int:
    with _Handler.lock:
        return _Handler.hits.get(path, 0)


async def expect_error(session: BidiSession, method: str, params: dict, needle: str):
    """Returns (ok, detail): `ok` when the command fails with `needle` in the
    error text (BiDi error code or message)."""
    try:
        await send(session, method, params)
    except Exception as e:  # noqa: BLE001 — the error is the expected outcome
        text = f"{type(e).__name__}: {e}"
        return needle in text.lower().replace("_", " "), text
    return False, "expected an error, got success"


async def check_protocol_honesty(session: BidiSession, report: Report) -> None:
    ok, detail = await expect_error(
        session, "network.addIntercept", {"phases": ["authRequired"]}, "unsupported operation"
    )
    report.protocol("addIntercept{authRequired}: unsupported operation", ok, "" if ok else detail)
    ok, detail = await expect_error(
        session, "network.continueWithAuth", {"request": "nope", "action": "default"}, "no such request"
    )
    report.protocol("continueWithAuth: no such request", ok, "" if ok else detail)
    ok, detail = await expect_error(
        session, "network.continueResponse", {"request": "nope"}, "no such request"
    )
    report.protocol("continueResponse(unknown id): no such request", ok, "" if ok else detail)


def cookie_value(cookie: dict):
    value = cookie.get("value")
    return value.get("value") if isinstance(value, dict) else value


async def check_cookies(session: BidiSession, ctx: str, base_url: str, report: Report) -> None:
    await send(session, "browsingContext.navigate", {"context": ctx, "url": f"{base_url}/page.html"})

    got = await send(session, "storage.getCookies", {"filter": {"name": SERVER_COOKIE}})
    cookies = got.get("cookies", [])
    ok = len(cookies) == 1 and cookie_value(cookies[0]) == "from-server"
    report.protocol(
        "storage.getCookies: sees Set-Cookie from the server",
        ok,
        "" if ok else f"got {cookies!r}",
    )

    await send(
        session,
        "storage.setCookie",
        {"cookie": {"name": BIDI_COOKIE, "value": {"type": "string", "value": "via-bidi"},
                    "domain": "127.0.0.1", "path": "/"}},
    )
    try:
        doc_cookie = await eval_json(session, ctx, "document.cookie")
    except JsContextUnavailable as e:
        report.skip("storage.setCookie: visible in document.cookie", str(e))
        doc_cookie = None
    if doc_cookie is not None:
        ok = f"{BIDI_COOKIE}=via-bidi" in doc_cookie
        report.protocol(
            "storage.setCookie: visible in document.cookie", ok, "" if ok else f"document.cookie = {doc_cookie!r}"
        )
    got = await send(session, "storage.getCookies", {"filter": {"name": BIDI_COOKIE}})
    ok = len(got.get("cookies", [])) == 1
    report.protocol("storage.getCookies: returns the cookie just set", ok, "" if ok else f"got {got!r}")

    await send(session, "storage.deleteCookies", {"filter": {"name": BIDI_COOKIE}})
    got = await send(session, "storage.getCookies", {"filter": {"name": BIDI_COOKIE}})
    ok = got.get("cookies", []) == []
    report.protocol("storage.deleteCookies: removed from getCookies", ok, "" if ok else f"got {got!r}")
    try:
        doc_cookie = await eval_json(session, ctx, "document.cookie")
    except JsContextUnavailable as e:
        report.skip("storage.deleteCookies: gone from document.cookie", str(e))
    else:
        ok = BIDI_COOKIE not in doc_cookie
        report.protocol(
            "storage.deleteCookies: gone from document.cookie", ok, "" if ok else f"document.cookie = {doc_cookie!r}"
        )
    # The server cookie must survive a filtered delete.
    got = await send(session, "storage.getCookies", {"filter": {"name": SERVER_COOKIE}})
    ok = len(got.get("cookies", [])) == 1
    report.protocol("storage.deleteCookies: leaves other cookies alone", ok, "" if ok else f"got {got!r}")


async def wait_response_started(session: BidiSession, events: list, timeout: float = 10.0):
    """Events ride on command responses (no push channel), so keep issuing a
    cheap command until a `network.responseStarted` shows up."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if events:
            return events.pop(0)
        await send(session, "session.status", {})
        await asyncio.sleep(0.05)
    return None


async def start_fetch(session: BidiSession, ctx: str, url: str, var: str) -> None:
    await eval_json(
        session,
        ctx,
        f"(() => {{ window.{var} = 'pending'; "
        f"fetch({url!r}).then(r => r.text()).then(t => {{ window.{var} = t; }}, "
        f"() => {{ window.{var} = 'rejected'; }}); return 1; }})()",
    )


async def check_response_started(session: BidiSession, ctx: str, base_url: str, report: Report) -> None:
    events: list = []

    async def on_event(_name, params):
        events.append(params)

    session.add_event_listener("network.responseStarted", on_event)
    await send(session, "session.subscribe", {"events": ["network.responseStarted"]})
    await send(session, "browsingContext.navigate", {"context": ctx, "url": f"{base_url}/page.html"})

    url = f"{base_url}/data.txt"
    add = await send(
        session,
        "network.addIntercept",
        {"phases": ["responseStarted"], "urlPatterns": [{"type": "string", "pattern": url}]},
    )
    intercept_id = add.get("intercept")
    report.protocol("addIntercept{responseStarted}: returns intercept id", bool(intercept_id))

    try:
        await start_fetch(session, ctx, url, "__devx20_continue")
    except JsContextUnavailable as e:
        report.skip("responseStarted: pause / continueResponse / failRequest", str(e))
        await send(session, "network.removeIntercept", {"intercept": intercept_id})
        return

    event = await wait_response_started(session, events)
    if event is None:
        report.protocol("network.responseStarted event delivered", False, "no event within 10s")
        await send(session, "network.removeIntercept", {"intercept": intercept_id})
        return
    request_id = (event.get("request") or {}).get("request")
    status = (event.get("response") or {}).get("status")
    ok = event.get("isBlocked") is True and status == 200 and bool(request_id)
    report.protocol(
        "network.responseStarted: isBlocked + status 200 + request id", ok, "" if ok else f"event = {event!r}"
    )

    ok = hit_count("/data.txt") >= 1
    report.protocol("responseStarted: server already received the request", ok)
    still = await eval_json(session, ctx, "window.__devx20_continue")
    report.protocol(
        "responseStarted: page fetch stays pending while paused",
        still == "pending",
        "" if still == "pending" else f"window var = {still!r}",
    )

    await send(session, "network.continueResponse", {"request": request_id})
    result = await poll_eval_json(session, ctx, "window.__devx20_continue", "pending", timeout=10.0)
    ok = result == DATA_BODY.decode()
    report.protocol(
        "continueResponse: page finished with the body", ok, "" if ok else f"window var = {result!r}"
    )

    ok, detail = await expect_error(
        session, "network.continueResponse", {"request": request_id}, "no such request"
    )
    report.protocol("continueResponse(already resumed): no such request", ok, "" if ok else detail)

    # failRequest on a second paused response must reject the page's fetch.
    await start_fetch(session, ctx, url, "__devx20_fail")
    event = await wait_response_started(session, events)
    if event is None:
        report.protocol("failRequest at responseStarted", False, "second pause not announced")
    else:
        await send(session, "network.failRequest", {"request": event["request"]["request"]})
        result = await poll_eval_json(session, ctx, "window.__devx20_fail", "pending", timeout=10.0)
        ok = result == "rejected"
        report.protocol(
            "failRequest at responseStarted: page fetch rejects", ok, "" if ok else f"window var = {result!r}"
        )

    await send(session, "network.removeIntercept", {"intercept": intercept_id})
    await start_fetch(session, ctx, url, "__devx20_free")
    result = await poll_eval_json(session, ctx, "window.__devx20_free", "pending", timeout=10.0)
    ok = result == DATA_BODY.decode()
    report.protocol(
        "removeIntercept: fetch no longer pauses", ok, "" if ok else f"window var = {result!r}"
    )


async def run_scenarios(ws_url: str, http_port: int, token: str) -> Report:
    report = Report()
    session = BidiSession.bidi_only(ws_url, requested_capabilities={"alwaysMatch": {"token": token}})
    await session.start()
    try:
        ctx = await get_default_context(session)
        base_url = f"http://127.0.0.1:{http_port}"
        await check_protocol_honesty(session, report)
        await check_cookies(session, ctx, base_url, report)
        await check_response_started(session, ctx, base_url, report)
    finally:
        await session.end()
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    http_server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
    http_port = http_server.server_port
    threading.Thread(target=http_server.serve_forever, daemon=True).start()

    host = "127.0.0.1"
    bidi_port = get_free_port()
    env = dict(os.environ, LUMEN_NO_ADBLOCK="1")
    proc = subprocess.Popen(
        [args.binary, "--bidi-port", str(bidi_port)], stderr=subprocess.PIPE, text=True, env=env
    )
    try:
        wait_for_port(host, bidi_port, proc, timeout=30)
        token = read_token_and_drain(proc.stderr)
        report = asyncio.run(run_scenarios(f"ws://{host}:{bidi_port}", http_port, token))
    except Exception as e:  # noqa: BLE001
        print(f"DEVX-20 FAILED: {e}", file=sys.stderr)
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
        print("DEVX-20 OK: BiDi responseStarted / authRequired / storage.* verified against the live window")
        return 0
    print("DEVX-20 FAILED: see the FAIL rows above", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
