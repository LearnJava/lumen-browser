# mypy: allow-untyped-defs

"""BiDi-only executor pieces for Lumen (`docs/tasks/p2-wpt-integration.md`, S3).

Lumen has no classic WebDriver HTTP session: `LumenBidiProtocol.connect()`
negotiates a session directly over the WebSocket via
`webdriver.bidi.client.BidiSession.bidi_only()`, unlike
`executorwebdriver.WebDriverBidiProtocol`, which layers BiDi on top of an
already-established classic HTTP session (`Session.start()` first, then
`bidi_session.start()`). `LumenTestharnessExecutor.do_test` is a stub —
navigating a test and reading back testharness.js results is S4's job (the
testharnessreport.js shim + smoke test); S3 only proves session negotiation.
"""

import asyncio
import traceback

from webdriver.bidi.client import BidiSession

from .base import TestharnessExecutor
from .protocol import Protocol


class LumenBidiProtocol(Protocol):
    """Bare BiDi session over `browser.bidi_url` — no ProtocolParts yet.

    `implements` stays empty here: later slices add `Bidi*ProtocolPart`
    implementations (`browsingContext.navigate`, `script.callFunction`, ...)
    as the executor grows to actually run tests.
    """

    def __init__(self, executor, browser, capabilities, **kwargs):
        super().__init__(executor, browser)
        self.capabilities = capabilities
        self.loop = asyncio.new_event_loop()
        self.session = None

    def connect(self):
        self.session = BidiSession.bidi_only(
            self.browser.bidi_url, requested_capabilities=self.capabilities)
        self.loop.run_until_complete(self.session.start(self.loop))

    def after_connect(self):
        pass

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
    """testharness.js executor for Lumen. `do_test` lands in S4."""

    supports_testdriver = False
    protocol_cls = LumenBidiProtocol

    def __init__(self, logger, browser, server_config, timeout_multiplier=1,
                capabilities=None, debug_info=None, **kwargs):
        TestharnessExecutor.__init__(self, logger, browser, server_config,
                                     timeout_multiplier=timeout_multiplier,
                                     debug_info=debug_info)
        self.protocol = self.protocol_cls(self, browser, capabilities)

    def do_test(self, test):
        raise NotImplementedError(
            "LumenTestharnessExecutor.do_test: navigating a test and reading back "
            "testharness.js results is S4 (docs/tasks/p2-wpt-integration.md) — "
            "S3 only proves BiDi session negotiation.")
