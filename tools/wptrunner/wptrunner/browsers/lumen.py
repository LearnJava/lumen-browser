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
import threading
import time
import traceback

import mozprocess

from .base import ExecutorBrowser, OutputHandler, WebDriverBrowser, get_free_port, get_timeout_multiplier, require_arg  # noqa: F401
from ..environment import wait_for_service
from ..executors import executor_kwargs as base_executor_kwargs
from ..executors.executorlumen import (  # noqa: F401
    LumenCrashtestExecutor, LumenPrintRefTestExecutor, LumenRefTestExecutor,
    LumenTestharnessExecutor)

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

#: Where every wptserve name points (`LUMEN_RESOLVE`, WPT-HOST-WPT) and the
#: address the servers bind: one IPv4 loopback for both, so neither the
#: browser nor this process needs the OS resolver for a test hostname.
_SERVER_IP = "127.0.0.1"


def lumen_resolve_spec(config, ip=_SERVER_IP):
    """`LUMEN_RESOLVE` value (`crates/network/src/dns.rs::RESOLVE_ENV`) for the
    browser child: every name wptserve builds — `browser_host` and the
    alternate host, each with its 30 subdomains — mapped to `ip`.

    Read off `config.domains_set` (the same set `wpt make-hosts-file` writes),
    not a copied list, so a change to `serve.py::_subdomains` or to
    `browser_host` cannot drift from it. `not_domains_set`
    (`nonexistent.*`) stays out on purpose: those names exist to *fail* to
    resolve, and `.test` is a reserved TLD the OS resolver answers NXDOMAIN
    for. The IDN subdomains already come punycoded from `ConfigBuilder`
    (`_get_domains`); the engine compares ASCII-only, so a non-ASCII name
    here is a config bug, not something to pass through.
    """
    names = sorted(config.domains_set)
    non_ascii = [name for name in names if not name.isascii()]
    if non_ascii:
        raise ValueError(f"LUMEN_RESOLVE needs punycoded names, got {non_ascii}")
    return ",".join(f"{name}={ip}" for name in names)


#: Env var: `off` launches browsers without `_SPAWN_LOCK` (the old behaviour),
#: for an A/B. Anything else (or unset) serializes the launch.
SPAWN_LOCK_ENV = "LUMEN_WPT_SPAWN_LOCK"

#: Serializes `ProcessHandler.run()` across the `TestRunnerManager` threads
#: of one wptrunner process. On Windows mozprocess creates the child with
#: `bInheritHandles=1` and no handle list (`processhandler.py`,
#: `winprocess.CreateProcess(..., 1, ...)`), after `subprocess` has made the
#: child ends of *this* launch's stdout/stderr pipes inheritable. A second
#: manager launching its browser in that window inherits them too — so the
#: first browser's stdout pipe has two writers. When the first browser is
#: killed on a restart, its pipe gets no EOF while the second one lives,
#: mozprocess's reader thread never ends, and `WebDriverBrowser.stop`'s
#: `proc.kill(timeout=5)` waits it out: 6.1 s (12.1 s when the leak is
#: two-deep) per restart instead of 0.1 s. Measured on the 2026-10-04 A/B
#: logs: 14-18 of ~88 restarts per run, 7 managers. The whole `run()` is
#: held — pipe creation through `CreateProcess` through closing the child
#: ends — which is ~20 ms, so the launches themselves are not slowed.
_SPAWN_LOCK = threading.Lock()


def _start_process(proc):
    """`proc.run()` under `_SPAWN_LOCK` (unless `SPAWN_LOCK_ENV` is `off`)."""
    if os.environ.get(SPAWN_LOCK_ENV, "").strip().lower() in ("off", "none", "0"):
        proc.run()
        return
    with _SPAWN_LOCK:
        proc.run()

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
        "print-reftest": "LumenPrintRefTestExecutor",
        "crashtest": "LumenCrashtestExecutor",
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
    #
    # BUG-755: Forced Colors Mode has no on-disk persistence and no BiDi/MCP
    # toggle — the only way to turn it on for a headless run is the
    # `--forced-colors` CLI flag `LumenBrowser.make_command` adds below.
    # `LumenBrowser` builds an explicit, minimal `env` dict for the child
    # process (see `LumenBrowser.__init__`), so setting `LUMEN_FORCED_COLORS`
    # in this wptrunner process's own environment would NOT propagate to the
    # browser — reading it here, in the parent, and turning it into a CLI arg
    # sidesteps that entirely. Invocation: `LUMEN_FORCED_COLORS=1
    # python run_report.py --all --root forced-colors-mode --recursive`.
    return {
        "binary": kwargs["binary"],
        # WPT-RUN-8-S2: print-reftest keeps a cheap `--ipc-server` process as
        # its browser object only; the pages come from a `--print-to-pdf` child
        # per screenshot (`LumenPrintRefTestExecutor`).
        "ipc_mode": test_type in ("reftest", "print-reftest"),
        "ca_cert_path": kwargs.get("ca_cert_path"),
        "resolve_spec": lumen_resolve_spec(config),
        "forced_colors": os.environ.get("LUMEN_FORCED_COLORS") == "1",
        # PERF-10: `LUMEN_NO_PAINT=1` -> `--no-paint` (DOM+JS+layout, no GPU
        # backend). Same parent-env -> CLI-arg trick as above. Only for
        # testharness: reftests need a real raster (`--ipc-server` ignores it
        # anyway, see `make_command`), and a crashtest's verdict is "the whole
        # pipeline did not crash" — skipping paint would hide paint crashes.
        "no_paint": test_type == "testharness" and os.environ.get("LUMEN_NO_PAINT") == "1",
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
    # `browser_host: "web-platform.test"` — wptserve's own default, back
    # since WPT-HOST-WPT (user decision 2026-10-27, path 3 of WPT-RUN-10;
    # measurement in `docs/wpt-vendor-notes/fetch.md` §WPT-HOST-RESOLVE).
    # Upstream resolves it and the whole subdomain family through a hosts
    # file (`wpt make-hosts-file`) — a machine-wide edit this runner may not
    # make. Instead the browser child gets `LUMEN_RESOLVE` with every name of
    # both families (`lumen_resolve_spec`, `LumenBrowser.__init__`), checked
    # by the engine before any resolver.
    #
    # History: `127.0.0.1` turned every subdomain into an unresolvable
    # `www1.127.0.0.1`; `localhost` (WPT-RUN-10, 2026-09-04) revived the
    # subdomains through RFC 6761 but left `not-web-platform.test` (1 129
    # ids) dead, and loopback names broke the tests themselves:
    # `www.localhost` is a different *site* from `localhost` (PSL `*` rule)
    # and `http://localhost` is potentially trustworthy, so 661 `fetch/
    # metadata` subtests expected what no browser can give there.
    #
    # `server_host`: wptserve binds and self-checks on `server_host`, which
    # defaults to `browser_host` — resolved by *this* process's OS resolver,
    # which knows nothing about `web-platform.test`. Pin it to the loopback
    # address `LUMEN_RESOLVE` points at.
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
        "browser_host": "web-platform.test",
        "server_host": _SERVER_IP,
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

    def __init__(self, logger, binary, ipc_mode=False, ca_cert_path=None,
                 resolve_spec=None, forced_colors=False, no_paint=False, **kwargs):
        env = dict(kwargs.pop("env", None) or {})
        if ca_cert_path:
            # BUG-785: the browser has no CLI flag for this, only an env var
            # (single point of trust-root construction — see
            # `crates/network/src/tls/mod.rs`).
            env["LUMEN_EXTRA_CA_CERT"] = ca_cert_path
        if resolve_spec:
            # WPT-HOST-WPT: `browser_host: web-platform.test` and its
            # alternate host resolve only through this map
            # (`lumen_resolve_spec`) — same env-var-only channel as above,
            # checked before any resolver in `crates/network/src/dns.rs`.
            env["LUMEN_RESOLVE"] = resolve_spec
        # BUG-800: the persisted ad-block ("shields") default is on, same as
        # a real user session. EasyList false-positives on WPT's own
        # test-infra request shapes (e.g. `common/security-features/
        # subresource/document.py?...action=purge...`), the blocked request
        # silently fails the navigation that depended on it (BUG-438), and
        # the stale document poisons the next result read
        # (`AssertionError: Got results from X, expected Y`). No test in
        # this corpus should ever be exercising the ad-blocker itself.
        env["LUMEN_NO_ADBLOCK"] = "1"
        # WPT-RUN-16: the interactive shell gates `window.open`, clipboard
        # writes and permission prompts on a live user gesture
        # (UX-USER-ACTIVATION). Tests open popups from a bare script, the way
        # chromedriver's `--disable-popup-blocking` lets them in Chrome; with
        # the gate on, `window.open` returns `null` and every popup-based
        # subtest fails on harness plumbing, not on the feature under test.
        env["LUMEN_ALLOW_POPUPS"] = "1"
        super().__init__(logger, binary=binary, webdriver_binary=binary, env=env or None, **kwargs)
        self.ipc_mode = ipc_mode
        self.forced_colors = forced_colors
        self.no_paint = no_paint

    def make_command(self):
        if self.ipc_mode:
            # BUG-755's `--forced-colors` only affects the live `Lumen` window
            # state built in `run_window_mode` (the `--bidi-port` path below)
            # — `--ipc-server`'s reftest rendering goes through the separate,
            # a11y_store-less `render_source_to_png` path, so passing the
            # flag here would be silently ignored. Not forwarded on purpose.
            return [self.binary, "--ipc-server"]
        forced_colors_args = ["--forced-colors"] if self.forced_colors else []
        no_paint_args = ["--no-paint"] if self.no_paint else []
        return [self.binary, "--bidi-port", str(self.port), *forced_colors_args, *no_paint_args]

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
        _start_process(self._proc)
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

        BUG-1073 срез 6: a fresh port on EVERY launch, not only the first.
        `WebDriverBrowser.port` picks one lazily and then keeps it, so a
        relaunch (restart after an ERROR/TIMEOUT) reused the previous
        process's port. `TerminateProcess` of a `lumen.exe` holding a live
        wgpu device does not free it at once: measured 0.7 s for one process,
        1.8-3.8 s for four killed together, longer under a WPT run (the old
        process outlived `proc.kill(timeout=5)` — `IO Completion Port failed
        to signal process shutdown`). Meanwhile `wait_for_service` below
        connected to the OLD, dying process and reported success; the new
        one failed `bind` (`os error 10048`) and exited without a token, and
        `token` raised `did not print [bidi] token` — three times in a row
        ended the whole `TestRunnerManager` and left its queue `MISSING`.
        """
        assert self.init_deadline is not None
        self._port = get_free_port()
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
            _start_process(self._proc)
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
            return ExecutorBrowser, {"ipc_port": self.port, "ipc_token": self.ipc_token,
                                     "binary": self.binary, "env": self.env}
        # BUG-498: `token` must be forwarded here — the executor process
        # only sees this `ExecutorBrowser` view, never `self` directly.
        return ExecutorBrowser, {"bidi_url": self.url, "token": self.token}
