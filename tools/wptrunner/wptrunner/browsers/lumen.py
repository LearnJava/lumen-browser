# mypy: allow-untyped-defs

"""wptrunner product plugin for Lumen (`docs/tasks/p2-wpt-integration.md`, S3).

Lumen has no separate WebDriver-classic HTTP server: `lumen --bidi-port <port>`
serves WebDriver BiDi directly over a raw WebSocket
(`ws://127.0.0.1:<port>/session`, `crates/bidi-server/src/server.rs`). This
reuses `WebDriverBrowser` for process lifecycle (spawn, wait-for-port, kill)
but overrides `make_command`/`url`/`executor_browser`, since there is no HTTP
session endpoint and `binary` doubles as `webdriver_binary` — there is no
separate driver process to point `--webdriver-binary` at.
"""

import os

from .base import ExecutorBrowser, WebDriverBrowser, get_timeout_multiplier, require_arg  # noqa: F401
from ..executors import executor_kwargs as base_executor_kwargs
from ..executors.executorlumen import LumenTestharnessExecutor  # noqa: F401

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
    # BUG-295: `TestEnvironment.get_routes` (environment.py) ALWAYS registers a
    # static route for `/resources/testharnessreport.js`, defaulting to
    # wptrunner's own generic `executors/message-queue.js` + `testharnessreport.js`
    # (the postMessage/testdriver-message-queue based one) unless the product's
    # `env_options()` supplies a `"testharnessreport"` override — this default
    # unconditionally shadows whatever is on disk at
    # `tests/wpt/resources/testharnessreport.js`, including our own
    # Lumen-specific one that stashes results on `window.__lumen_wpt_results`
    # (`LumenTestharnessExecutor` in `executorlumen.py` polls exactly that
    # global). Without this override, wptserve silently serves the wrong
    # file: no crash, no error — the page just never sets the global the
    # executor is polling for, which manifested as an unconditional
    # `run_smoke.py` TIMEOUT no matter what else was fixed (found chasing
    # BUG-291 — a direct BiDi probe against a plain `http.server` for the same
    # page succeeded quickly, isolating the fault to something wptserve-specific
    # once BUG-291 itself was ruled out).
    testharnessreport_path = os.path.normpath(os.path.join(
        os.path.dirname(__file__), os.pardir, os.pardir, os.pardir, os.pardir,
        "tests", "wpt", "resources", "testharnessreport.js"))
    return {
        "browser_host": "127.0.0.1",
        "bind_address": True,
        "testharnessreport": [testharnessreport_path],
    }


def env_extras(**kwargs):
    return []


class LumenBrowser(WebDriverBrowser):
    """Launches `lumen --bidi-port <port>` and waits for the BiDi WebSocket
    listener to come up. `binary` doubles as `webdriver_binary` — Lumen
    speaks BiDi itself, there is no separate driver process."""

    def __init__(self, logger, binary, **kwargs):
        super().__init__(logger, binary=binary, webdriver_binary=binary, **kwargs)

    def make_command(self):
        return [self.binary, "--bidi-port", str(self.port)]

    @property
    def url(self):
        # No trailing path: `webdriver.bidi.client.BidiSession` appends
        # `/session` itself when given a bare `ws://host:port` URL.
        return f"ws://{self.host}:{self.port}"

    def executor_browser(self):
        return ExecutorBrowser, {"bidi_url": self.url}
