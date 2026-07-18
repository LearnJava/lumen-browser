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
    # `/resources/testharnessreport.js`. wptrunner's default
    # (`environment.py::get_routes`) is a *static route* that serves its own
    # bundled `executors/message-queue.js` + `testharnessreport.js` pair —
    # results are pushed onto `window.__wptrunner_message_queue` and drained by
    # the stock `WebDriverProtocol`. Our bespoke BiDi-only executor
    # (`executorlumen.py`) instead polls `window.__lumen_wpt_results`, which
    # only Lumen's own `tests/wpt/resources/testharnessreport.js` sets. Because
    # that static route wins over any on-disk file of the same URL, the vendored
    # report script is *never served* under wptrunner+wptserve — so the poll
    # times out forever. Pointing the route at our own script restores the
    # `__lumen_wpt_results` contract the executor expects. This was the sole
    # root cause of the `run_smoke.py` timeout tracked as BUG-301 (a manual BiDi
    # driver over a plain HTTP server always served the on-disk file, hence
    # "works manually, times out under wptrunner").
    return {
        "browser_host": "127.0.0.1",
        "bind_address": True,
        "testharnessreport": [_LUMEN_TESTHARNESSREPORT],
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
