#!/usr/bin/env python3
"""BUG-961 срез 42, re-scoped item 1 from срез 41: control срез 39 point 2's
"<1s" bare-BidiSession result against *today's* `main`, holding the one
variable срез 41 identified as never actually held constant — what serves the
`.any.html` bytes.

Срез 41 (`bugs/BUG-961-OPEN.md`) ran a bare `webdriver.bidi.client.BidiSession`
(zero `executorlumen.py` code) against wptserve's own live `AnyHtmlHandler`
route for `/console/console-log-large-array.any.html` and it stalled ~32s —
refuting both the "TestRunnerManager orchestration" (срез 39/40) and "engine
contention specific to executorlumen.py's polling" (срез 40) hypotheses,
since neither is present in that bare client either. But срез 39 point 2's
original "<1s" bare-client measurement served the SAME bytes through a
DIFFERENT path — a hand-copied/curl-fetched static file, not wptserve's live
dynamic route — so that difference was never isolated. This script does:

1. Boot the real `wptrunner` `TestEnvironment` (same as срез 41's script) and
   fetch the exact live-route bytes for the test id via `urllib` (byte-for-byte
   what a real corpus run would receive — no hand-copying).
2. Serve those exact bytes from a SEPARATE plain `http.server` static server
   rooted at `tests/wpt/` (so `/resources/testharness.js`,
   `/resources/testharnessreport.js` — substituted the same way
   `serve_wpt_like.py` does — and the `.any.js` file itself all resolve
   normally; only the `.any.html` response itself is swapped for the
   pre-fetched static bytes instead of being generated dynamically).
3. Run a bare `BidiSession` (identical shape to срез 41 Variant B) against
   step 2's static server instead of wptserve's live route, measuring
   `navigate(wait="complete")` elapsed time.

Two outcomes (see `bugs/BUG-961-OPEN.md` "Что нужно" item 1 for the
disposition of each): still <1s here ⇒ the live `AnyHtmlHandler` route vs. a
static copy IS the real variable, next step is diffing the two HTTP
responses directly; also stalls ⇒ срез 39 point 2's original "<1s" result was
itself wrong or has regressed since, and the investigation should stop
chasing content-serving differences and profile the live-window BiDi/engine
path directly instead.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice42_static.py
        [--binary target/dev-release/lumen] [--seconds 45]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import asyncio
import http.server
import os
import socketserver
import sys
import threading
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
METADATA_ROOT = os.path.join(TESTS_ROOT, "metadata")
CERTS_ROOT = os.path.join(TESTS_ROOT, "certs")

sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools"),
    os.path.join(REPO_ROOT, "tools", "wptserve"),
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    os.path.join(REPO_ROOT, "tools", "wptrunner"),
    TESTS_ROOT,  # for serve_wpt_like's _build_testharnessreport()
]

import localpaths  # noqa: E402,F401  (repo_root bootstrap wptrunner expects)
from wptrunner import environment as env_mod  # noqa: E402
from wptrunner import wptcommandline, wptrunner as wr  # noqa: E402
from wptrunner.testloader import Subsuite  # noqa: E402

import serve_wpt_like  # noqa: E402  (for _build_testharnessreport, same substitution a real run applies)

#: Same test id срез 39/40/41 investigate.
TEST_ID = "/console/console-log-large-array.any.html"
TEST_TYPE = "testharness"


def _build_kwargs(binary):
    argv = [
        "--product=lumen",
        f"--binary={binary}",
        f"--tests={TESTS_ROOT}",
        f"--metadata={METADATA_ROOT}",
        "--log-mach=-",
        "--ssl-type=pregenerated",
        f"--ca-cert-path={os.path.join(CERTS_ROOT, 'ca-cert.pem')}",
        f"--host-cert-path={os.path.join(CERTS_ROOT, 'host-cert.pem')}",
        f"--host-key-path={os.path.join(CERTS_ROOT, 'host-key.pem')}",
        TEST_ID,
    ]
    cmd_parser = wptcommandline.create_parser()
    kwargs = vars(cmd_parser.parse_args(argv))
    wptcommandline.check_args(kwargs)
    return kwargs


def _make_static_server(static_body):
    """A `serve_wpt_like.Handler` clone rooted at `tests/wpt/` that answers
    `TEST_ID` with the pre-fetched live-route bytes verbatim (no re-injection
    of a completion marker — `wait="complete"` measures navigation/readystate,
    not test completion, so a marker is not needed and would be one more
    difference from срез 41's Variant B)."""

    class StaticAnyHtmlHandler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            path = self.path.split("?", 1)[0]
            if path == TEST_ID:
                self.send_response(200)
                self.send_header("Content-Type", "text/html;charset=utf8")
                self.send_header("Content-Length", str(len(static_body)))
                self.end_headers()
                self.wfile.write(static_body)
                return
            if path == "/resources/testharnessreport.js":
                body = serve_wpt_like._build_testharnessreport()
                self.send_response(200)
                self.send_header("Content-Type", "text/javascript;charset=utf8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            return super().do_GET()

    def handler(*a, **kw):
        return StaticAnyHtmlHandler(*a, directory=TESTS_ROOT, **kw)

    httpd = socketserver.TCPServer(("127.0.0.1", 0), handler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    return httpd, port


def _bare_control(product, kwargs, subsuite, run_info_data, config, url, manager_number, label):
    bk_kwargs = dict(kwargs)
    bk_kwargs["config"] = config
    bk_kwargs["subsuite"] = subsuite
    browser_kwargs = product.get_browser_kwargs(
        wr.logger, TEST_TYPE, run_info_data, **bk_kwargs)
    browser_cls = product.get_browser_cls(TEST_TYPE)
    browser = browser_cls(wr.logger, manager_number=manager_number, **browser_kwargs)
    browser.start(group_metadata={})
    try:
        from webdriver.bidi.client import BidiSession

        loop = asyncio.new_event_loop()

        async def _probe():
            session = BidiSession.bidi_only(
                browser.url,
                requested_capabilities={"alwaysMatch": {"token": browser.token}})
            await session.start(loop)
            try:
                contexts = await session.browsing_context.get_tree()
                context = contexts[0]["context"]
                t0 = time.monotonic()
                try:
                    await asyncio.wait_for(
                        session.browsing_context.navigate(
                            context=context, url=url, wait="complete"),
                        timeout=45)
                except asyncio.TimeoutError:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] {label} RESULT: hung past {elapsed:.1f}s "
                          f"(asyncio.wait_for timeout)")
                    return
                except Exception as e:  # noqa: BLE001 — report whatever BiDi raised
                    elapsed = time.monotonic() - t0
                    print(f"[probe] {label} RESULT: exception after "
                          f"{elapsed:.1f}s: {e!r}")
                    return
                elapsed = time.monotonic() - t0
                print(f"[probe] {label} RESULT: navigate(wait=complete) "
                      f"returned in {elapsed:.2f}s")
            finally:
                await session.end()

        loop.run_until_complete(_probe())
        loop.close()
    finally:
        browser.stop()


def _run_probe(kwargs, seconds):
    logger = wr.logger
    product = kwargs["product"]

    env_mod.do_delayed_imports(logger, kwargs["test_paths"])

    env_extras = product.get_env_extras(**kwargs)
    ssl_config = {
        "type": kwargs["ssl_type"],
        "openssl": {"openssl_binary": kwargs["openssl_binary"]},
        "pregenerated": {
            "host_key_path": kwargs["host_key_path"],
            "host_cert_path": kwargs["host_cert_path"],
            "ca_cert_path": kwargs["ca_cert_path"],
        },
    }
    run_info_data = {"os": "linux"}
    testharness_timeout_multiplier = product.get_timeout_multiplier(
        TEST_TYPE, run_info_data, **kwargs)

    test_environment = env_mod.TestEnvironment(
        kwargs["test_paths"],
        testharness_timeout_multiplier,
        False,  # pause_after_test
        kwargs["debug_test"],
        kwargs["debug_info"],
        product.env_options,
        ssl_config,
        env_extras,
        kwargs["enable_webtransport_h3"],
        kwargs["enable_dns"],
        None,  # mojojs_path
        None,  # inject_script
        kwargs["suppress_handler_traceback"],
        kwargs["ws_extra"],
    )

    subsuite = Subsuite("", {})

    with test_environment as test_env:
        test_env.ensure_started()
        print(f"[probe] wptserve up: {test_env.config['browser_host']}, "
              f"ports={test_env.config['ports']}")

        from wptrunner.executors.base import server_url
        live_url = server_url(test_env.config, "http") + TEST_ID

        print(f"[probe] fetching live-route bytes: {live_url}")
        with urllib.request.urlopen(live_url, timeout=15) as resp:
            static_body = resp.read()
        print(f"[probe] fetched {len(static_body)} bytes from the live route")

        httpd, static_port = _make_static_server(static_body)
        static_url = f"http://127.0.0.1:{static_port}{TEST_ID}"
        print(f"[probe] static server up on port {static_port}, serving the "
              f"pre-fetched bytes at {static_url}")

        try:
            print("[probe] variant C (bare BidiSession, static-served "
                  "pre-fetched live-route bytes): starting a fresh lumen process")
            _bare_control(product, kwargs, subsuite, run_info_data,
                          test_env.config, static_url, 2, "variant C (static copy)")
        finally:
            httpd.shutdown()
            httpd.server_close()

        print("[probe] variant D (bare BidiSession, wptserve's own live "
              "route, control replay of срез 41 Variant B): starting a "
              "fresh lumen process")
        _bare_control(product, kwargs, subsuite, run_info_data,
                      test_env.config, live_url, 3, "variant D (live route)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=45.0)
    args = parser.parse_args()

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    os.makedirs(METADATA_ROOT, exist_ok=True)
    kwargs = _build_kwargs(args.binary)

    with wr.GlobalLogger(kwargs, {"raw": sys.stdout}):
        _run_probe(kwargs, args.seconds)
    return 0


if __name__ == "__main__":
    sys.exit(main())
