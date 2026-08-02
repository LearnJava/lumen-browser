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
`window.__lumen_wpt_results` (a fresh navigation gives every test a fresh
`window`, so no explicit reset between tests is needed).

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
two. Only `click` is actually executed (`input.performActions`, which the
BiDi server does implement); every other action fails cleanly (rejects the
test's promise) rather than hanging forever — the DoD is "not silently
SKIPped", not "every `test_driver.*` method works".
"""

import asyncio
import json
import traceback

from webdriver.bidi.client import BidiSession
from webdriver.bidi.error import UnknownErrorException
from webdriver.bidi.modules.input import Actions
from webdriver.bidi.modules.script import ContextTarget

from .base import ExecutorException, TestharnessExecutor
from .protocol import Protocol

#: Global `tests/wpt/resources/testharnessreport.js` stashes the JSON-encoded
#: `[url, harness_status, harness_message, harness_stack, subtests]` result
#: on once `add_completion_callback` fires.
RESULTS_GLOBAL = "__lumen_wpt_results"

#: Poll interval while waiting for `RESULTS_GLOBAL`/a testdriver action to
#: appear (seconds).
POLL_INTERVAL_S = 0.05

#: Single `script.evaluate` expression polled every iteration: returns a
#: JSON-encoded `{"k": "r", "v": <RESULTS_GLOBAL string>}` once the harness
#: completes, `{"k": "a", "v": [url, "action", {...}]}` once
#: `test_driver_internal` queues an action (arming
#: `__wptrunner_testdriver_callback` if it isn't already, so a
#: previously-queued action that arrived before this poll loop started gets
#: drained too — `message-queue.js`'s `push()` only calls
#: `__wptrunner_process_next_event()` on push, which itself no-ops without a
#: callback armed), or `null` while neither has happened yet.
POLL_EXPRESSION = f"""(() => {{
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
        self.session = BidiSession.bidi_only(
            self.browser.bidi_url, requested_capabilities=self.capabilities)
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

        await session.browsing_context.navigate(context=context, url=url, wait="complete")

        loop = asyncio.get_running_loop()
        deadline = None if timeout is None else loop.time() + timeout + self.extra_timeout
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
                    if outer["k"] == "r":
                        return json.loads(outer["v"])
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

    async def _handle_action(self, session, context, payload):
        """Execute one `test_driver_internal.*` action and post its
        `testdriver-complete` message back (`testdriver-extra.js`'s `pending`
        map is keyed by `payload["id"]` and rejects/resolves the page-side
        promise from that message alone — nothing else observes this)."""
        action = payload.get("action")
        cmd_id = payload["id"]
        params = payload.get("params") or {}
        try:
            if action != "click":
                raise ActionError(
                    f"action {action!r} not implemented by Lumen's minimal WPT executor")
            result = await self._action_click(session, context, params)
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
