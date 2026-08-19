# mypy: allow-untyped-defs

"""BiDi-only executor pieces for Lumen (`docs/tasks/p2-wpt-integration.md`, S4).

Lumen has no classic WebDriver HTTP session: `LumenBidiProtocol.connect()`
negotiates a session directly over the WebSocket via
`webdriver.bidi.client.BidiSession.bidi_only()`, unlike
`executorwebdriver.WebDriverBidiProtocol`, which layers BiDi on top of an
already-established classic HTTP session (`Session.start()` first, then
`bidi_session.start()`).

`LumenTestharnessExecutor.do_test` (S4) drives a test with three BiDi calls
per run: `browsingContext.navigate` (blocks on Lumen's real
`document.readyState == "complete"` signal, P2-wpt S1), then
`script.evaluate` polled until `tests/wpt/resources/testharnessreport.js`'s
`add_completion_callback` has stashed a JSON-stringified result on
`window.__lumen_wpt_results`.

A *successful* navigation does give every test a fresh `window`, but a
**failed** one does not, and Lumen reports both the same way
([BUG-380](../../../../bugs/BUG-380-FIXED.md)): `browsingContext.navigate`
answers `{navigation, url}` for an unreachable URL just as it does for a real
one (`bc_navigate` only surfaces an error when `LiveWindowSession::navigate`
or the `DocumentReady` wait itself fails — a load that never completes is
reported asynchronously and never reaches the BiDi reply), while the previous
document, its `location.href` and all its globals stay live. Since this
executor reuses one browsing context for the whole run, the next test would
then read the *previous* test's `RESULTS_GLOBAL` — hence `RESET_EXPRESSION`,
which both clears the result/testdriver slots and fingerprints the outgoing
document with `STALE_GLOBAL` so a document that was never replaced is
recognisable (a fresh document cannot carry the marker, verified for both
same-URL reloads and cross-URL navigations).

`test_driver.*` support (WPT-RUN-2, `docs/tasks/p2-wpt-runner-throughput.md`)
reuses the exact same poll loop rather than the `channel`/`script.message`
machinery `executorwebdriver.WebDriverBidiProtocol` builds on (Lumen's BiDi
server does not implement `script.addPreloadScript`'s `channel` argument or
`script.message` events — see `crates/bidi-server/src/protocol.rs`). The
vendored `/resources/testdriver.js` bundle (always served concatenated with
wptrunner's own `executors/message-queue.js` + `testdriver-extra.js`,
hardcoded in `environment.py::get_routes`, not overridable) already exposes a
polling contract of its own: `test_driver_internal.*` calls push an action
onto `window.__wptrunner_message_queue`, which only drains once
`window.__wptrunner_testdriver_callback` is armed. `_run_testharness` arms it
every poll iteration and checks both `RESULTS_GLOBAL` and the action slot in
one `script.evaluate` round trip, so this stays a single poll loop instead of
two. `click` (`input.performActions`, which the BiDi server does implement)
and `generate_test_report` (a direct call into the page-visible
`_lumen_deliver_report` JS global, no new BiDi surface needed) are actually
executed; every other action fails cleanly (rejects the test's promise)
rather than hanging forever — the DoD is "not silently SKIPped", not "every
`test_driver.*` method works".
"""

import asyncio
import base64
import json
import socket
import struct
import traceback

from webdriver.bidi.client import BidiSession
from webdriver.bidi.error import BidiException, UnknownErrorException
from webdriver.bidi.modules.input import Actions
from webdriver.bidi.modules.script import ContextTarget

from .base import ExecutorException, RefTestExecutor, RefTestImplementation, TestharnessExecutor
from .protocol import Protocol

#: Global `tests/wpt/resources/testharnessreport.js` stashes the JSON-encoded
#: `[url, harness_status, harness_message, harness_stack, subtests]` result
#: on once `add_completion_callback` fires.
RESULTS_GLOBAL = "__lumen_wpt_results"

#: Global set on the document being navigated *away* from (`RESET_EXPRESSION`),
#: so the poll loop can tell "this document was never replaced" from "the new
#: document just hasn't produced a result yet" — a fresh document never carries
#: it (BUG-380).
STALE_GLOBAL = "__lumen_wpt_stale"

#: Poll interval while waiting for `RESULTS_GLOBAL`/a testdriver action to
#: appear (seconds).
POLL_INTERVAL_S = 0.05

#: How long `STALE_GLOBAL` may still be observed after `navigate` returned
#: before the navigation is declared failed (seconds). `navigate` already
#: blocked on `document.readyState == "complete"`, so a replaced document is
#: normally visible on the first poll; this only absorbs the same
#: document-swap lag the "JS context not available" retry below covers.
NAV_SETTLE_S = 2.0

#: Run in the outgoing document immediately before `browsingContext.navigate`:
#: drops any result/testdriver state the *previous* test left behind (a failed
#: navigation keeps that document alive, and one browsing context is reused for
#: the whole run) and marks it, so a document still answering after the
#: navigation is identifiable as the old one. Plain assignment rather than
#: `delete`: `testharnessreport.js` sets `RESULTS_GLOBAL` on `window` and the
#: poll below tests `!== undefined`, so this works regardless of property
#: configurability.
RESET_EXPRESSION = f"""(() => {{
  window.{RESULTS_GLOBAL} = undefined;
  window.__lumen_td_slot = undefined;
  window.{STALE_GLOBAL} = true;
}})()"""

#: Single `script.evaluate` expression polled every iteration: returns a
#: JSON-encoded `{"k": "s", "v": <location.href>}` while the pre-navigation
#: document is still the live one, `{"k": "r", "v": <RESULTS_GLOBAL string>}`
#: once the harness completes, `{"k": "a", "v": [url, "action", {...}]}` once
#: `test_driver_internal` queues an action (arming
#: `__wptrunner_testdriver_callback` if it isn't already, so a
#: previously-queued action that arrived before this poll loop started gets
#: drained too — `message-queue.js`'s `push()` only calls
#: `__wptrunner_process_next_event()` on push, which itself no-ops without a
#: callback armed), or `null` while neither has happened yet.
#:
#: The staleness check comes first deliberately: an async test whose completion
#: callback fires *after* the runner moved on would otherwise re-populate
#: `RESULTS_GLOBAL` on the un-replaced document and hand the next test the
#: previous one's result all over again.
POLL_EXPRESSION = f"""(() => {{
  if (window.{STALE_GLOBAL} === true) {{
    return JSON.stringify({{k: "s", v: String(location.href)}});
  }}
  if (window.{RESULTS_GLOBAL} !== undefined) {{
    return JSON.stringify({{k: "r", v: window.{RESULTS_GLOBAL}}});
  }}
  if (!window.__wptrunner_testdriver_callback) {{
    window.__wptrunner_testdriver_callback = (r) => {{ window.__lumen_td_slot = r; }};
    if (window.__wptrunner_process_next_event) {{ window.__wptrunner_process_next_event(); }}
  }}
  if (window.__lumen_td_slot !== undefined) {{
    const v = window.__lumen_td_slot;
    delete window.__lumen_td_slot;
    return JSON.stringify({{k: "a", v: v}});
  }}
  return null;
}})()"""


class ActionError(Exception):
    """Raised by an action handler to report a clean action failure (as
    opposed to a bug in the executor itself) — becomes the rejected promise's
    message on the test page, not an executor crash."""


class LumenBidiProtocol(Protocol):
    """Bare BiDi session over `browser.bidi_url` — no ProtocolParts.

    `implements` stays empty: `do_test` talks to `self.session` (a raw
    `webdriver.bidi.client.BidiSession`) directly rather than through
    `Bidi*ProtocolPart` wrappers, since the only capabilities needed
    (navigate + evaluate) are simple, single-shot calls.
    """

    def __init__(self, executor, browser, capabilities, **kwargs):
        super().__init__(executor, browser)
        self.capabilities = capabilities
        self.loop = asyncio.new_event_loop()
        self.session = None
        #: Top-level browsing context tests navigate in; fetched once in
        #: `after_connect` and reused for every test (single-window executor).
        self.context_id = None

    def connect(self):
        # ADR-024 §Access model (DEVX-15): `--bidi-port` requires
        # `capabilities.alwaysMatch.token` on `session.new`. The token is only
        # known once `self.browser`'s process has actually started (it's a
        # fresh per-run value Lumen prints to stderr), so it cannot be part of
        # the static `executor_kwargs()` capabilities dict — merge it in here.
        capabilities = dict(self.capabilities or {})
        always_match = dict(capabilities.get("alwaysMatch") or {})
        always_match["token"] = self.browser.token
        capabilities["alwaysMatch"] = always_match
        self.session = BidiSession.bidi_only(
            self.browser.bidi_url, requested_capabilities=capabilities)
        self.loop.run_until_complete(self.session.start(self.loop))

    def after_connect(self):
        contexts = self.run(self.session.browsing_context.get_tree())
        self.context_id = contexts[0]["context"]

    def run(self, coro):
        """Run a coroutine to completion on this protocol's event loop."""
        return self.loop.run_until_complete(coro)

    def teardown(self):
        if self.session is not None:
            try:
                self.loop.run_until_complete(self.session.end())
            except Exception:
                self.logger.debug(traceback.format_exc())
            self.session = None
        self.loop.stop()

    def is_alive(self):
        return self.session is not None and self.session.transport is not None


class LumenTestharnessExecutor(TestharnessExecutor):
    """testharness.js executor for Lumen, driven over WebDriver BiDi."""

    #: `test.testdriver` (set from the manifest's `<script
    #: src=".../testdriver.js">` scan) blanket-`SKIP`s a test before it ever
    #: runs when the executor class doesn't advertise support
    #: (`wptrunner.py:300`) — this is what unblocks those tests, not a claim
    #: that every `test_driver.*` method works (see module docstring).
    supports_testdriver = True
    protocol_cls = LumenBidiProtocol

    def __init__(self, logger, browser, server_config, timeout_multiplier=1,
                capabilities=None, debug_info=None, **kwargs):
        TestharnessExecutor.__init__(self, logger, browser, server_config,
                                     timeout_multiplier=timeout_multiplier,
                                     debug_info=debug_info)
        self.protocol = self.protocol_cls(self, browser, capabilities)

    def do_test(self, test):
        url = self.test_url(test)
        timeout = (test.timeout * self.timeout_multiplier
                   if self.debug_info is None else None)
        raw_result = self.protocol.run(self._run_testharness(url, timeout))
        return self.convert_result(test, raw_result)

    async def _run_testharness(self, url, timeout):
        session = self.protocol.session
        context = self.protocol.context_id

        await self._reset_and_mark(session, context)
        try:
            await session.browsing_context.navigate(context=context, url=url, wait="complete")
        except BidiException as e:
            # A navigation Lumen *does* reject (bad context, load timeout) —
            # report it as this test's ERROR with the BiDi message instead of
            # letting the traceback surface as an INTERNAL-ERROR. A navigation
            # that merely fails to load is not this case: it answers
            # successfully and is caught by the staleness check below.
            raise ExecutorException(
                "ERROR", f"browsingContext.navigate({url}) failed: {e}") from e

        loop = asyncio.get_running_loop()
        deadline = None if timeout is None else loop.time() + timeout + self.extra_timeout
        settle_deadline = loop.time() + NAV_SETTLE_S
        while True:
            try:
                # `await_promise=False` is deliberate: async tests
                # (`promise_test`/`async_test`) complete via the page's own
                # event loop + testharness completion callback, which stashes
                # the final result on `RESULTS_GLOBAL` — we poll that global
                # synchronously rather than awaiting a promise here. BiDi
                # `awaitPromise=True` is a separate, currently-unimplemented
                # path (BUG-319, pinned by `tests/wpt/verify_s6_await_promise.py`)
                # this executor does not depend on (P2-wpt S6).
                value = await session.script.evaluate(
                    expression=POLL_EXPRESSION,
                    target=ContextTarget(context),
                    await_promise=False)
            except UnknownErrorException as e:
                # `browsingContext.navigate`'s `wait="complete"` can return
                # before the JS runtime for the new document has finished
                # installing (Lumen builds the JS context off the UI thread,
                # after the streaming HTML/layout pipeline that
                # `document.readyState` is based on) — `script.evaluate`
                # reports this as "JS context not available" rather than an
                # empty result. Treat it as "not ready yet" and keep polling,
                # same as the null-result case below (found running P2-wpt S4).
                if "JS context not available" not in e.message:
                    raise
            else:
                if value.get("type") == "string":
                    outer = json.loads(value["value"])
                    if outer["k"] == "s":
                        # The document that was live before `navigate` is still
                        # answering: the new page never replaced it.
                        if loop.time() > settle_deadline:
                            raise ExecutorException(
                                "ERROR",
                                f"browsingContext.navigate({url}) reported success but the "
                                f"document was never replaced (still at {outer['v']}); "
                                f"the page did not load")
                    elif outer["k"] == "r":
                        return json.loads(outer["v"])
                    else:
                        # outer["k"] == "a": [url, "action", {type, action, params, id}].
                        # Dispatch and post the completion back, then keep polling
                        # in the same loop — an action never ends the test itself.
                        _, msg_type, payload = outer["v"]
                        if msg_type == "action":
                            await self._handle_action(session, context, payload)
            if deadline is not None and loop.time() > deadline:
                raise ExecutorException(
                    "TIMEOUT",
                    f"Timed out waiting for testharnessreport.js results: {url}")
            await asyncio.sleep(POLL_INTERVAL_S)

    async def _reset_and_mark(self, session, context):
        """Clear the outgoing document's result/testdriver slots and mark it
        (`RESET_EXPRESSION`). Best-effort: a context with no JS runtime yet
        (the initial `about:blank`, before the first test) reports "JS context
        not available" and has nothing to carry over anyway."""
        try:
            await session.script.evaluate(
                expression=RESET_EXPRESSION,
                target=ContextTarget(context),
                await_promise=False)
        except UnknownErrorException as e:
            if "JS context not available" not in e.message:
                raise

    async def _handle_action(self, session, context, payload):
        """Execute one `test_driver_internal.*` action and post its
        `testdriver-complete` message back (`testdriver-extra.js`'s `pending`
        map is keyed by `payload["id"]` and rejects/resolves the page-side
        promise from that message alone — nothing else observes this)."""
        action = payload.get("action")
        cmd_id = payload["id"]
        params = payload.get("params") or {}
        try:
            if action == "click":
                result = await self._action_click(session, context, params)
            elif action == "generate_test_report":
                result = await self._action_generate_test_report(session, context, params)
            else:
                raise ActionError(
                    f"action {action!r} not implemented by Lumen's minimal WPT executor")
            status, message = "success", json.dumps({"result": result})
        except ActionError as e:
            status, message = "failure", str(e)
        complete_expression = (
            "window.postMessage({type: 'testdriver-complete', "
            f"cmd_id: {json.dumps(cmd_id)}, status: {json.dumps(status)}, "
            f"message: {json.dumps(message)}}}, '*')"
        )
        await session.script.evaluate(
            expression=complete_expression, target=ContextTarget(context), await_promise=False)

    async def _action_click(self, session, context, params):
        """`test_driver.click(element)` — resolve `params["selectors"]`
        (`testdriver-extra.js::get_selector_array`, outermost document down
        through nested shadow roots) to a viewport point and replay a real
        pointer click there via `input.performActions` (implemented — see
        `crates/bidi-server/src/protocol.rs::input_perform_actions`)."""
        target_context = params.get("context") or context
        point = await self._resolve_element_center(
            session, target_context, params.get("selectors") or [])
        if point is None:
            raise ActionError(f"element not found for selectors {params.get('selectors')!r}")
        x, y = point
        actions = Actions()
        pointer = actions.add_pointer()
        pointer.pointer_move(round(x), round(y), origin="viewport")
        pointer.pointer_down(0)
        pointer.pointer_up(0)
        await session.input.perform_actions(actions, context=target_context)
        return None

    async def _action_generate_test_report(self, session, context, params):
        """`test_driver.generate_test_report(message)` — deliver a `"test"`
        report carrying `{message}` to the target context's `ReportingObserver`s
        (W3C Reporting API §8.2). Lumen's Reporting API shim already exposes
        the delivery entry point as a page-visible JS global
        (`crates/js/src/reporting_api.rs::_lumen_deliver_report`), so this
        needs no new engine binding — just call it with `location.href` as the
        report URL and a JSON-encoded `TestReportBody`."""
        target_context = params.get("context") or context
        message = params.get("message")
        expression = (
            "_lumen_deliver_report('test', location.href, "
            f"{json.dumps(json.dumps({'message': message}))})"
        )
        await session.script.evaluate(
            expression=expression, target=ContextTarget(target_context), await_promise=False)
        return None

    async def _resolve_element_center(self, session, context, selectors):
        expression = f"""(() => {{
  const selectors = {json.dumps(selectors)};
  let root = document, el = null;
  for (let i = 0; i < selectors.length; i++) {{
    el = root.querySelector(selectors[i]);
    if (!el) return null;
    root = el.shadowRoot;
    if (!root && i < selectors.length - 1) return null;
  }}
  const r = el.getBoundingClientRect();
  return JSON.stringify({{x: r.left + r.width / 2, y: r.top + r.height / 2}});
}})()"""
        value = await session.script.evaluate(
            expression=expression, target=ContextTarget(context), await_promise=False)
        if value.get("type") != "string":
            return None
        point = json.loads(value["value"])
        return point["x"], point["y"]


# ── reftest support (TEST-4, docs/tasks/p2-test-track.md) ──────────────────
#
# `RefTestImplementation` (base.py) needs pixel-identical, backend-independent
# screenshots to compare test vs. reference: `browsingContext.captureScreenshot`
# (the BiDi call `LumenTestharnessExecutor` could otherwise reuse) rasterizes
# via the *live* window's wgpu renderer (`WinitSession::screenshot` ->
# `Renderer::new_headless`, `crates/driver/src/winit_session.rs`), which is
# backend-dependent (Vulkan vs. Dx12 produce different antialiasing/blend
# output on the same machine — see the wgpu-backend gotcha in CLAUDE.md, BUG-405
# slice 14) and therefore unfit for a pass/fail pixel diff. `lumen --ipc-server`
# (`crates/shell/src/main.rs::run_ipc_server`) renders through the deterministic
# tiny-skia CPU path instead (`render_source_to_png`, the same one `--screenshot`
# uses) — this is the surface the reftest executor drives.
#
# `LumenIpcProtocol` below is a second, independent port of the bincode client
# already proven working in `graphic_tests/run.py` (`LumenIpcClient`, TAB-7) —
# not imported from there, since that script runs under the system Python
# rather than `tests/wpt/.venv` and the two call sites have no shared import
# path. Wire format source of truth: `crates/ipc/src/lib.rs`.

#: `IpcRequest`/`IpcResponse` enum variant tags — see `crates/ipc/src/lib.rs`.
_REQ_AUTH = 3
_REQ_CREATE_TAB = 4
_REQ_NAVIGATE_TAB = 6
_REQ_SCREENSHOT = 7

_RESP_AUTH_OK = 4
_RESP_TAB_CREATED = 6
_RESP_NAVIGATED = 8
_RESP_SCREENSHOT = 9
_RESP_TAB_ERROR = 10


class IpcError(Exception):
    """Failure talking to `lumen --ipc-server` (protocol, connection, or a
    `TabError` reply)."""


def _u32(v):
    return struct.pack("<I", v)


def _bstr(s):
    """bincode `String`/`Vec<u8>`: u64 LE length + UTF-8 bytes."""
    b = s.encode("utf-8")
    return struct.pack("<Q", len(b)) + b


class _IpcCursor:
    """Cursor over a decoded bincode response body."""

    def __init__(self, data):
        self.d = data
        self.p = 0

    def _take(self, n):
        b = self.d[self.p:self.p + n]
        if len(b) != n:
            raise IpcError("truncated IPC message body")
        self.p += n
        return b

    def u32(self):
        return struct.unpack("<I", self._take(4))[0]

    def vec(self):
        n = struct.unpack("<Q", self._take(8))[0]
        return self._take(n)

    def string(self):
        return self.vec().decode("utf-8", "replace")


class LumenIpcProtocol(Protocol):
    """Bare bincode-over-TCP session against `lumen --ipc-server`
    (`crates/ipc/src/lib.rs`). One tab is created in `after_connect` and
    reused for every test in the run, mirroring `LumenBidiProtocol`'s
    single-context reuse — safe here because `NavigateTab` only stashes a
    `PageSource` in the tab slot (`crates/shell/src/main.rs`); the actual
    load + layout + rasterize happens fresh on each `Screenshot` call, so
    there is no BUG-380-class stale-document race to guard against."""

    def __init__(self, executor, browser, **kwargs):
        super().__init__(executor, browser)
        self.sock = None
        self.tab_id = None

    def connect(self):
        self.sock = socket.create_connection(
            ("127.0.0.1", self.browser.ipc_port), timeout=30)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self._send(_u32(_REQ_AUTH) + _bstr(self.browser.ipc_token))
        c = self._recv()
        if c.u32() != _RESP_AUTH_OK:
            raise IpcError("lumen --ipc-server rejected the auth token")

    def after_connect(self):
        self._send(_u32(_REQ_CREATE_TAB))
        c = self._recv()
        if c.u32() != _RESP_TAB_CREATED:
            raise IpcError("expected TabCreated")
        self.tab_id = c.u32()

    def teardown(self):
        if self.sock is not None:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None

    def is_alive(self):
        return self.sock is not None

    def _send(self, payload):
        self.sock.sendall(_u32(len(payload)) + payload)

    def _read_exact(self, n):
        buf = bytearray()
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise IpcError("IPC connection closed by lumen --ipc-server")
            buf += chunk
        return bytes(buf)

    def _recv(self):
        body_len = struct.unpack("<I", self._read_exact(4))[0]
        return _IpcCursor(self._read_exact(body_len))

    def navigate(self, url):
        self._send(_u32(_REQ_NAVIGATE_TAB) + _u32(self.tab_id) + _bstr(url))
        c = self._recv()
        tag = c.u32()
        if tag == _RESP_NAVIGATED:
            return
        if tag == _RESP_TAB_ERROR:
            c.u32()
            raise IpcError(f"NavigateTab: {c.string()}")
        raise IpcError(f"expected Navigated, got variant {tag}")

    def screenshot_png(self):
        self._send(_u32(_REQ_SCREENSHOT) + _u32(self.tab_id))
        c = self._recv()
        tag = c.u32()
        if tag == _RESP_SCREENSHOT:
            c.u32()
            return c.vec()
        if tag == _RESP_TAB_ERROR:
            c.u32()
            raise IpcError(f"Screenshot: {c.string()}")
        raise IpcError(f"expected Screenshot, got variant {tag}")


class LumenRefTestExecutor(RefTestExecutor):
    """reftest executor for Lumen, driven over `lumen --ipc-server` (TEST-4).

    Scope cut (smoke pass, see `docs/tasks/p2-test-track.md#test-4`):
    `class=reftest-wait` is not supported — `NavigateTab`/`Screenshot` run the
    same single-shot, non-interactive pipeline as `--screenshot`/`--dump-layout`
    (initial `<script>`s execute once during parse, but nothing pumps a later
    JS-driven `reftest-wait` class removal before the render), and the IPC
    protocol has no `script.evaluate`-equivalent to poll for one even if it
    did. Pick reftest fixtures that render correctly on the first pass.
    """

    protocol_cls = LumenIpcProtocol

    def __init__(self, logger, browser, server_config, timeout_multiplier=1,
                 screenshot_cache=None, debug_info=None, **kwargs):
        RefTestExecutor.__init__(self, logger, browser, server_config,
                                 screenshot_cache=screenshot_cache,
                                 timeout_multiplier=timeout_multiplier,
                                 debug_info=debug_info)
        self.protocol = self.protocol_cls(self, browser)
        self.implementation = RefTestImplementation(self)

    def reset(self):
        self.implementation.reset()

    def is_alive(self):
        return self.protocol.is_alive()

    def do_test(self, test):
        result = self.implementation.run_test(test)
        return self.convert_result(test, result)

    def screenshot(self, test, viewport_size, dpi, page_ranges):
        # https://github.com/web-platform-tests/wpt/issues/7135 — Lumen has no
        # notion of a resizable viewport or a print-page range in this headless
        # path (fixed at the CLI's own default, `crates/shell/src/main.rs`
        # `SCREENSHOT_VP_W`/`SCREENSHOT_MIN_H`), same restriction Selenium's
        # executor documents (`executorselenium.py`).
        assert viewport_size is None
        assert dpi is None
        url = self.test_url(test)
        timeout = test.timeout * self.timeout_multiplier + self.extra_timeout
        self.protocol.sock.settimeout(timeout)
        try:
            self.protocol.navigate(url)
            png = self.protocol.screenshot_png()
        except socket.timeout:
            return False, ("TIMEOUT", f"Timed out rendering {url}")
        except IpcError as e:
            return False, ("FAIL", str(e))
        return True, [base64.b64encode(png).decode("ascii")]
