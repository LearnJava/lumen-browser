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
`window`, so no explicit reset between tests is needed). This intentionally
skips the full `testdriver.js`/window-management machinery
`WebDriverTestharnessExecutor` uses (`supports_testdriver = False`,
`executorwebdriver.py`) — out of scope until a test actually needs
`test_driver.*` APIs.
"""

import asyncio
import json
import traceback

from webdriver.bidi.client import BidiSession
from webdriver.bidi.error import UnknownErrorException
from webdriver.bidi.modules.script import ContextTarget

from .base import ExecutorException, TestharnessExecutor
from .protocol import Protocol

#: Global `tests/wpt/resources/testharnessreport.js` stashes the JSON-encoded
#: `[url, harness_status, harness_message, harness_stack, subtests]` result
#: on once `add_completion_callback` fires.
RESULTS_GLOBAL = "__lumen_wpt_results"

#: Poll interval while waiting for `RESULTS_GLOBAL` to appear (seconds).
POLL_INTERVAL_S = 0.05


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

    supports_testdriver = False
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
        expression = f"window.{RESULTS_GLOBAL} !== undefined ? window.{RESULTS_GLOBAL} : null"
        while True:
            try:
                value = await session.script.evaluate(
                    expression=expression,
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
                    return json.loads(value["value"])
            if deadline is not None and loop.time() > deadline:
                raise ExecutorException(
                    "TIMEOUT",
                    f"Timed out waiting for testharnessreport.js results: {url}")
            await asyncio.sleep(POLL_INTERVAL_S)
