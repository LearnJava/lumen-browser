# mypy: allow-untyped-defs

"""wptrunner product plugin for Lumen (`docs/tasks/p2-wpt-integration.md`, S3).

Lumen has no separate WebDriver-classic HTTP server: `lumen --bidi-port <port>`
serves WebDriver BiDi directly over a raw WebSocket
(`ws://127.0.0.1:<port>/session`, `crates/bidi-server/src/server.rs`). This
reuses `WebDriverBrowser` for process lifecycle (spawn, wait-for-port, kill)
but overrides `make_command`/`url`/`executor_browser`, since there is no HTTP
session endpoint and `binary` doubles as `webdriver_binary` — there is no
separate driver process to point `--webdriver-binary` at.

ADR-024 §Access model (DEVX-15): `--bidi-port` requires a per-run token, which
Lumen prints to stderr as `[bidi] token: <token>` right after the port line —
no escape hatch. `_TokenCapturingOutputHandler` taps `WebDriverBrowser`'s
existing per-line output callback (`create_output_handler` is a documented
subclass hook, not a vendor patch) to capture it; `LumenBrowser.token` exposes
it, and `LumenBidiProtocol.connect()` (`executorlumen.py`) reads it to build
`capabilities.alwaysMatch.token` for `session.new` dynamically — the token is
only known once the process has actually started, so it cannot be baked into
the static `executor_kwargs()` dict below.
"""

import os
import time

from .base import ExecutorBrowser, OutputHandler, WebDriverBrowser, get_timeout_multiplier, require_arg  # noqa: F401
from ..executors import executor_kwargs as base_executor_kwargs
from ..executors.executorlumen import LumenTestharnessExecutor  # noqa: F401

#: Prefix of the stderr line `crates/bidi-server/src/server.rs::spawn` prints
#: once per process (ADR-024 §Access model, DEVX-15).
_TOKEN_LINE_PREFIX = b"[bidi] token: "

#: Absolute path to Lumen's own `testharnessreport.js` (`tests/wpt/resources/`),
#: four levels up from this plugin file
#: (`tools/wptrunner/wptrunner/browsers/lumen.py` → repo root).
_LUMEN_TESTHARNESSREPORT = os.path.normpath(os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "..", "..", "..", "tests", "wpt", "resources", "testharnessreport.js"))

__wptrunner__ = {
    "product": "lumen",
    "check_args": "check_args",
    "browser": "LumenBrowser",
    "browser_kwargs": "browser_kwargs",
    "executor_kwargs": "executor_kwargs",
    "env_options": "env_options",
    "env_extras": "env_extras",
    "timeout_multiplier": "get_timeout_multiplier",
    "executor": {
        "testharness": "LumenTestharnessExecutor",
    },
}


def check_args(**kwargs):
    require_arg(kwargs, "binary")


def browser_kwargs(logger, test_type, run_info_data, config, **kwargs):
    return {"binary": kwargs["binary"]}


def executor_kwargs(logger, test_type, test_environment, run_info_data, **kwargs):
    executor_kwargs = base_executor_kwargs(test_type, test_environment, run_info_data, **kwargs)
    # `capabilities.alwaysMatch.token` (ADR-024 §Access model, DEVX-15) is
    # merged in dynamically by `LumenBidiProtocol.connect()` — the token is
    # only known once the browser process has started, so it cannot live in
    # this static, pre-launch dict.
    executor_kwargs["capabilities"] = {}
    return executor_kwargs


def env_options():
    # `wptserve`'s own default (`browser_host = "web-platform.test"`,
    # `serve.py`) requires that hostname (and a long list of
    # `*.web-platform.test` subdomains) resolve via the OS resolver — normally
    # satisfied by adding entries to `/etc/hosts` (`wpt make-hosts-file`).
    # This project's "no live network / fully offline" rule (P2-wpt task doc)
    # rules out relying on that machine-wide setup step, and BiDi automation
    # doesn't exercise WPT's cross-origin subdomain tests anyway (S4/S5 scope
    # is same-origin `dom/` tests) — a literal IP needs no resolution at all,
    # sidestepping the `[Errno 11001] getaddrinfo failed` this produced
    # against the default hostname (found while implementing S4).
    #
    # `testharnessreport`: override the report script `wptserve` serves at
    # `/resources/testharnessreport.js`. `TestEnvironment.get_routes`
    # (`environment.py`) ALWAYS registers a *static route* for that URL,
    # defaulting to wptrunner's own generic `executors/message-queue.js` +
    # `testharnessreport.js` pair (results pushed onto
    # `window.__wptrunner_message_queue`, drained by the stock
    # `WebDriverProtocol`) unless the product's `env_options()` supplies a
    # `"testharnessreport"` override. That default unconditionally shadows
    # whatever is on disk at `tests/wpt/resources/testharnessreport.js`,
    # including our own Lumen-specific one that stashes results on
    # `window.__lumen_wpt_results` (`LumenTestharnessExecutor` in
    # `executorlumen.py` polls exactly that global) — because the static
    # route wins over any on-disk file at the same URL, the vendored report
    # script is *never served* under wptrunner+wptserve, so the poll times
    # out forever with no crash, no error. This was the sole root cause of
    # the `run_smoke.py` timeout tracked as BUG-301 (found independently
    # while chasing BUG-291/BUG-295 too — a direct BiDi probe against a
    # plain `http.server` for the same page always served the on-disk file
    # and succeeded quickly, isolating the fault to something
    # wptserve-specific once the engine-side bugs were ruled out; "works
    # manually, times out under wptrunner"). Pointing the route at our own
    # script restores the `__lumen_wpt_results` contract the executor
    # expects.
    return {
        "browser_host": "127.0.0.1",
        "bind_address": True,
        "testharnessreport": [_LUMEN_TESTHARNESSREPORT],
    }


def env_extras(**kwargs):
    return []


class _TokenCapturingOutputHandler(OutputHandler):
    """`OutputHandler` that also stashes the ADR-024 §Access model (DEVX-15)
    token line, in addition to its normal logging behavior. `__call__` runs
    on every line regardless of `self.state` (even lines buffered before
    `start()`), so the token is captured no later than a plain `OutputHandler`
    would have logged the same line."""

    def __init__(self, logger, command, **kwargs):
        super().__init__(logger, command, **kwargs)
        self.token = None

    def __call__(self, line):
        if self.token is None and line.startswith(_TOKEN_LINE_PREFIX):
            self.token = line[len(_TOKEN_LINE_PREFIX):].decode("utf-8", "replace").strip()
        super().__call__(line)


class LumenBrowser(WebDriverBrowser):
    """Launches `lumen --bidi-port <port>` and waits for the BiDi WebSocket
    listener to come up. `binary` doubles as `webdriver_binary` — Lumen
    speaks BiDi itself, there is no separate driver process."""

    def __init__(self, logger, binary, **kwargs):
        super().__init__(logger, binary=binary, webdriver_binary=binary, **kwargs)

    def make_command(self):
        return [self.binary, "--bidi-port", str(self.port)]

    def create_output_handler(self, cmd):
        return _TokenCapturingOutputHandler(self.logger, cmd)

    @property
    def token(self):
        """Blocks (briefly) until `_TokenCapturingOutputHandler` has captured
        the token line — printed synchronously before the BiDi accept loop
        starts, so it is normally available immediately once the port itself
        is reachable (which `start()` already waited for)."""
        deadline = time.time() + 10
        while self._output_handler.token is None and time.time() < deadline:
            time.sleep(0.05)
        if self._output_handler.token is None:
            raise RuntimeError("lumen --bidi-port did not print [bidi] token")
        return self._output_handler.token

    @property
    def url(self):
        # No trailing path: `webdriver.bidi.client.BidiSession` appends
        # `/session` itself when given a bare `ws://host:port` URL.
        return f"ws://{self.host}:{self.port}"

    def executor_browser(self):
        # BUG-498: `token` must be forwarded here — the executor process
        # only sees this `ExecutorBrowser` view, never `self` directly.
        return ExecutorBrowser, {"bidi_url": self.url, "token": self.token}
