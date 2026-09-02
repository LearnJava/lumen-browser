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

import errno
import io
import os
import time
import traceback

import mozprocess

from .base import ExecutorBrowser, OutputHandler, WebDriverBrowser, get_timeout_multiplier, require_arg  # noqa: F401
from ..environment import wait_for_service
from ..executors import executor_kwargs as base_executor_kwargs
from ..executors.executorlumen import LumenRefTestExecutor, LumenTestharnessExecutor  # noqa: F401

#: Prefix of the stderr line `crates/bidi-server/src/server.rs::spawn` prints
#: once per process (ADR-024 §Access model, DEVX-15).
_TOKEN_LINE_PREFIX = b"[bidi] token: "

#: Prefixes of the two stdout lines `crates/shell/src/main.rs::run_ipc_server`
#: prints once per process (TEST-4). Unlike `--bidi-port`, `--ipc-port` is
#: documented there as informational only — the OS always picks the real
#: port and the shell prints it, so `LumenBrowser` cannot pre-assign one the
#: way `WebDriverBrowser` normally does; both lines must be scraped from
#: output instead.
_IPC_PORT_LINE_PREFIX = b"LUMEN_IPC_PORT="
_IPC_TOKEN_LINE_PREFIX = b"LUMEN_IPC_TOKEN="

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
        "reftest": "LumenRefTestExecutor",
    },
}


def check_args(**kwargs):
    require_arg(kwargs, "binary")


def browser_kwargs(logger, test_type, run_info_data, config, **kwargs):
    # TEST-4: reftests are driven over `--ipc-server` (deterministic CPU
    # screenshots, see the module docstring of `executorlumen.py`'s reftest
    # section) instead of `--bidi-port` — `LumenBrowser.ipc_mode` switches
    # `make_command`/`executor_browser` between the two wire protocols.
    #
    # BUG-785: `--ca-cert-path` (wptrunner's own vendored test CA,
    # `tests/wpt/certs/ca-cert.pem`) is forwarded to `LumenBrowser` so it can
    # set `LUMEN_EXTRA_CA_CERT` on the child process — see
    # `crates/network/src/tls/mod.rs::trusted_root_store`. Without it every
    # `.https.` test fails `UnknownIssuer` before the request is even sent.
    return {
        "binary": kwargs["binary"],
        "ipc_mode": test_type == "reftest",
        "ca_cert_path": kwargs.get("ca_cert_path"),
    }


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


class _IpcCapturingOutputHandler(OutputHandler):
    """`OutputHandler` variant for `--ipc-server` (TEST-4): stashes the
    `LUMEN_IPC_PORT=`/`LUMEN_IPC_TOKEN=` stdout lines
    (`crates/shell/src/main.rs::run_ipc_server`) instead of the BiDi token
    line — same "runs on every line regardless of state" reasoning as
    `_TokenCapturingOutputHandler`."""

    def __init__(self, logger, command, **kwargs):
        super().__init__(logger, command, **kwargs)
        self.ipc_port = None
        self.ipc_token = None

    def __call__(self, line):
        if self.ipc_port is None and line.startswith(_IPC_PORT_LINE_PREFIX):
            raw = line[len(_IPC_PORT_LINE_PREFIX):].decode("utf-8", "replace").strip()
            try:
                self.ipc_port = int(raw)
            except ValueError:
                pass
        elif self.ipc_token is None and line.startswith(_IPC_TOKEN_LINE_PREFIX):
            self.ipc_token = line[len(_IPC_TOKEN_LINE_PREFIX):].decode("utf-8", "replace").strip()
        super().__call__(line)


class LumenBrowser(WebDriverBrowser):
    """Launches `lumen --bidi-port <port>` (testharness) or
    `lumen --ipc-server` (reftest, TEST-4) and waits for the corresponding
    listener to come up. `binary` doubles as `webdriver_binary` — Lumen
    speaks BiDi itself, there is no separate driver process."""

    def __init__(self, logger, binary, ipc_mode=False, ca_cert_path=None, **kwargs):
        env = dict(kwargs.pop("env", None) or {})
        if ca_cert_path:
            # BUG-785: the browser has no CLI flag for this, only an env var
            # (single point of trust-root construction — see
            # `crates/network/src/tls/mod.rs`).
            env["LUMEN_EXTRA_CA_CERT"] = ca_cert_path
        # BUG-800: the persisted ad-block ("shields") default is on, same as
        # a real user session. EasyList false-positives on WPT's own
        # test-infra request shapes (e.g. `common/security-features/
        # subresource/document.py?...action=purge...`), the blocked request
        # silently fails the navigation that depended on it (BUG-438), and
        # the stale document poisons the next result read
        # (`AssertionError: Got results from X, expected Y`). No test in
        # this corpus should ever be exercising the ad-blocker itself.
        env["LUMEN_NO_ADBLOCK"] = "1"
        super().__init__(logger, binary=binary, webdriver_binary=binary, env=env or None, **kwargs)
        self.ipc_mode = ipc_mode

    def make_command(self):
        if self.ipc_mode:
            return [self.binary, "--ipc-server"]
        return [self.binary, "--bidi-port", str(self.port)]

    def create_output_handler(self, cmd):
        if self.ipc_mode:
            return _IpcCapturingOutputHandler(self.logger, cmd)
        return _TokenCapturingOutputHandler(self.logger, cmd)

    def _run_server(self, group_metadata, **kwargs):
        if not self.ipc_mode:
            return self._run_server_bidi(group_metadata, **kwargs)
        # `--ipc-port` is documented as informational only
        # (`crates/shell/src/main.rs::extract_ipc_server`) — the shell always
        # lets the OS pick the real port and prints it, so the base class's
        # `wait_for_service(self.port)` (which polls a *pre-assigned* port)
        # would poll an unrelated, unbound one here. Wait for the output
        # handler to capture the printed port instead.
        assert self.init_deadline is not None
        cmd = self.make_command()
        self._output_handler = self.create_output_handler(cmd)
        self._proc = mozprocess.ProcessHandler(
            cmd, processOutputLine=self._output_handler, env=self.env, storeOutput=False,
            # BUG-961: see `_run_server_bidi`'s comment — same fix, same reason.
            bufsize=io.DEFAULT_BUFFER_SIZE)
        self.logger.info("Starting Lumen --ipc-server: %s" % " ".join(cmd))
        self._proc.run()
        self._output_handler.after_process_start(self._proc.pid)
        try:
            while (self._output_handler.ipc_port is None
                   and time.time() < self.init_deadline):
                time.sleep(0.05)
            if self._output_handler.ipc_port is None:
                raise RuntimeError(
                    "lumen --ipc-server did not print LUMEN_IPC_PORT within timeout")
            self._port = self._output_handler.ipc_port
        finally:
            self._output_handler.start(group_metadata=group_metadata, **kwargs)
        self.logger.info("Lumen --ipc-server started successfully.")

    def _run_server_bidi(self, group_metadata, **kwargs):
        """Same launch as `WebDriverBrowser._run_server` (`base.py`), with ONE
        change: an explicit buffered `bufsize` on the `mozprocess.ProcessHandler`
        call, instead of `mozprocess.processhandler.Process`'s own default
        (`bufsize=0`, i.e. raw/unbuffered stdout+stderr pipes,
        `mozprocess/processhandler.py:97`).

        BUG-961 (срез 47, confirmed by `verify_bug961_slice47b_bufsize0_readline.py`
        with zero lumen/wptrunner code involved — a bare `os.pipe()` writing one
        20MB line with no embedded newline): `io.FileIO.readline()` on an
        UNBUFFERED raw stream has no buffer to search, so it falls back to
        `io.RawIOBase`'s generic implementation, which reads ONE BYTE PER
        SYSCALL until it sees `\\n`. `mozprocess.ProcessReader._read_stream`
        (`processhandler.py:1076`) calls exactly `stream.readline()` on that
        raw pipe. For a single ~20MB line (e.g.
        `console/console-log-large-array.any.js`'s `console.log` of a huge
        array — no `\\n` until the very end), that is ~20 million Python-level
        1-byte reads inside ONE `.readline()` call: measured 30.09s for a
        20 000 005-byte line, vs. 0.024s with a normal buffered reader
        (1241×) — a bare-pipe measurement matching this bug's observed ~30-32s
        `browsingContext.navigate` stall almost exactly, with no engine,
        wptrunner-orchestration, or content-serving explanation needed (all
        three were tested and refuted across срезы 39-46). A `subprocess.Popen`
        NOT going through `mozprocess` (срез 43/45/46's bare-Popen repro,
        ≤1/187 stall rate) does not hit this: `bufsize=1` in binary mode is
        silently promoted by `subprocess.py` to `io.DEFAULT_BUFFER_SIZE`
        ("line buffering isn't supported in binary mode"), giving an ordinary
        `io.BufferedReader` whose `readline()` is O(n) total, not O(n) syscalls.

        `mozprocess.ProcessHandler.__init__` forwards unrecognized kwargs
        (`self.keywordargs`) straight into `Process.__init__`
        (`processhandler.py:822-849`'s `run()`: `args.update(self.keywordargs)`),
        so passing `bufsize=` here reaches `subprocess.Popen` exactly the way
        srez 47b's isolated repro did — no vendor patch, no reapply-per-venv
        step (unlike the pywebsocket3 patch, CLAUDE.md's WPT-harness gotcha).
        """
        assert self.init_deadline is not None
        cmd = self.make_command()
        self._output_handler = self.create_output_handler(cmd)

        self._proc = mozprocess.ProcessHandler(
            cmd,
            processOutputLine=self._output_handler,
            env=self.env,
            storeOutput=False,
            bufsize=io.DEFAULT_BUFFER_SIZE)

        self.logger.info("Starting WebDriver: %s" % " ".join(cmd))
        try:
            self._proc.run()
        except OSError as e:
            if e.errno == errno.ENOENT:
                raise OSError(
                    "WebDriver executable not found: %s" % self.webdriver_binary) from e
            raise
        self._output_handler.after_process_start(self._proc.pid)

        try:
            wait_for_service(
                self.logger,
                self.host,
                self.port,
                timeout=self.init_deadline - time.time(),
                server_process=self._proc,
            )
        except Exception:
            self.logger.error(f"WebDriver was not accessible within {self.init_timeout} seconds.")
            self.logger.error(traceback.format_exc())
            raise
        finally:
            self._output_handler.start(group_metadata=group_metadata, **kwargs)
        self.logger.info("Webdriver started successfully.")

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
    def ipc_token(self):
        """Same blocking pattern as `token`, for the `--ipc-server` line pair
        (`LUMEN_IPC_PORT=` always precedes `LUMEN_IPC_TOKEN=` — both are
        `println!`-ed back to back — so by the time `_run_server` observes
        the port the token is normally already there too)."""
        deadline = time.time() + 10
        while self._output_handler.ipc_token is None and time.time() < deadline:
            time.sleep(0.05)
        if self._output_handler.ipc_token is None:
            raise RuntimeError("lumen --ipc-server did not print LUMEN_IPC_TOKEN")
        return self._output_handler.ipc_token

    @property
    def url(self):
        # No trailing path: `webdriver.bidi.client.BidiSession` appends
        # `/session` itself when given a bare `ws://host:port` URL.
        return f"ws://{self.host}:{self.port}"

    def executor_browser(self):
        if self.ipc_mode:
            return ExecutorBrowser, {"ipc_port": self.port, "ipc_token": self.ipc_token}
        # BUG-498: `token` must be forwarded here — the executor process
        # only sees this `ExecutorBrowser` view, never `self` directly.
        return ExecutorBrowser, {"bidi_url": self.url, "token": self.token}
