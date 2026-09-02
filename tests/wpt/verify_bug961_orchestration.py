#!/usr/bin/env python3
"""BUG-961 срез 41, "Что нужно" item 1: reproduce with `LumenTestharnessExecutor`/
`LumenBidiProtocol`'s own classes directly, driven against a real `wptserve`
instance (so `.any.html` routing — `AnyHtmlHandler` in `tools/serve/serve.py` —
matches a real corpus run byte for byte), but WITHOUT `TestRunnerManager`'s
multiprocess scheduling around them.

Срез 39/40 (`bugs/BUG-961-OPEN.md`) already ruled out two hypotheses by direct
measurement: the array/console.log computation itself (<1s standalone), and
`OutputHandler`'s real line-processing callback (0.019s decode + 0.099s
`process_output`, and that callback only *sees* the line six seconds after
wptrunner had already declared TIMEOUT — the gap is upstream of it). What
neither slice tested is whether `executorlumen.py`'s own code — the
`_reset_and_mark` pre-navigate eval, the `POLL_EXPRESSION` `script.evaluate`
every 50ms — is itself slow *when run through the real corpus server*, as
opposed to a bare `webdriver.bidi.client.BidiSession` (srez 39 point 2, which
measured `navigate` completion only, not console.log-arrival) or wptrunner's
full `TestRunnerManager` orchestration (the actual corpus run, which times
out).

This script builds the same `TestEnvironment` (real `wptserve`, same
`env_options()` override, same ssl config) `run_smoke.py`/`run_report.py`
build via `wptcommandline`/`wptrunner.environment`, then instantiates
`LumenBrowser` + `LumenTestharnessExecutor`/`LumenBidiProtocol` (imported
unmodified from `executorlumen.py`/`browsers/lumen.py`) and calls
`_run_testharness` directly — no `TestRunnerManager`, no multiprocessing, no
health-check polling. Timing wraps the call from *outside*; `executorlumen.py`
itself is untouched, so if this reproduces the ~30-40s stall it is on
`executorlumen.py`'s own polling loop or something upstream of wptrunner
entirely; if it stays <2s (like the bare-BiDi-client probe already did) it
narrows the culprit to `TestRunnerManager`'s own scheduling machinery
specifically.

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug961_orchestration.py
        [--binary target/dev-release/lumen] [--seconds 55]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import asyncio
import os
import sys
import time

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
]

import localpaths  # noqa: E402,F401  (repo_root bootstrap wptrunner expects)
from wptrunner import environment as env_mod  # noqa: E402
from wptrunner import wptcommandline, wptrunner as wr  # noqa: E402
from wptrunner.executors.base import ExecutorException  # noqa: E402
from wptrunner.executors.protocol import Protocol  # noqa: E402,F401  (sanity import)
from wptrunner.testloader import Subsuite  # noqa: E402

#: The test id whose TIMEOUT срез 39/40 investigate — a synchronous `test()`
#: that logs two 10M-element arrays, then `done()`s.
TEST_ID = "/console/console-log-large-array.any.html"

#: `test_type` this test id runs under, per `.any.js`'s `AnyHtmlHandler`
#: naming — used only to pick `LumenBrowser`/`LumenTestharnessExecutor`.
TEST_TYPE = "testharness"


def _build_kwargs(binary):
    """Same argv wptcommandline builds for a real run (`run_smoke.py::run`),
    minus the ones that only matter to `TestRunnerManager`/multiprocessing
    (pause-after-test, restart-on-unexpected) — irrelevant here since this
    script never enters that code path at all."""
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

        bk_kwargs = dict(kwargs)
        bk_kwargs["config"] = test_env.config
        bk_kwargs["subsuite"] = subsuite
        browser_kwargs = product.get_browser_kwargs(
            logger, TEST_TYPE, run_info_data, **bk_kwargs)

        browser_cls = product.get_browser_cls(TEST_TYPE)
        browser = browser_cls(logger, manager_number=0, **browser_kwargs)
        browser.start(group_metadata={})
        try:
            executor_browser_cls, executor_browser_kwargs = browser.executor_browser()
            executor_browser = executor_browser_cls(**executor_browser_kwargs)

            ek_kwargs = dict(kwargs)
            ek_kwargs["subsuite"] = subsuite
            executor_kwargs = product.get_executor_kwargs(
                logger, TEST_TYPE, test_env, run_info_data, **ek_kwargs)

            executor_cls = product.executor_classes[TEST_TYPE]
            executor = executor_cls(logger, executor_browser, **executor_kwargs)

            from wptrunner.executors.base import server_url
            url = server_url(test_env.config, "http") + TEST_ID

            executor.protocol.connect()
            executor.protocol.after_connect()
            try:
                print(f"[probe] variant A (LumenTestharnessExecutor, no "
                      f"TestRunnerManager): navigating {url}")

                t0 = time.monotonic()
                try:
                    # `protocol.run` uses `LumenBidiProtocol`'s OWN dedicated
                    # event loop (`asyncio.new_event_loop()` in its
                    # `__init__`) — exactly how `do_test` normally drives
                    # `_run_testharness` (`self.protocol.run(self.
                    # _run_testharness(url, timeout))`). This script's own
                    # driver code stays synchronous so it never has two
                    # event loops fighting over the same thread.
                    raw_result = executor.protocol.run(asyncio.wait_for(
                        executor._run_testharness(url, seconds),
                        timeout=seconds + 15))
                except asyncio.TimeoutError:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT: hung past {elapsed:.1f}s "
                          f"(asyncio.wait_for timeout)")
                except ExecutorException as e:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT: ExecutorException after "
                          f"{elapsed:.1f}s: {e.args!r}")
                else:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT: _run_testharness returned "
                          f"in {elapsed:.2f}s: {raw_result!r}")
            finally:
                executor.protocol.teardown()
        finally:
            browser.stop()

        # Variant B: bare `webdriver.bidi.client.BidiSession` (no
        # `executorlumen.py` code at all — no `_reset_and_mark`, no
        # POLL_EXPRESSION loop, just `session.start()` +
        # `browsing_context.navigate(wait="complete")`) against the SAME
        # live `wptserve` (`test_env`, still running) and the SAME dynamic
        # `AnyHtmlHandler`-generated `.any.html` response. Isolates the one
        # variable срез 39 point 2's bare-client control never held constant:
        # that probe's server was a hand-copied static file (serve_wpt_like.py
        # or a curl-fetched byte-for-byte copy), not wptserve's real routing.
        print("[probe] variant B (bare BidiSession, no executorlumen.py code, "
              "same real wptserve): starting a fresh lumen process")
        _bare_control(product, kwargs, subsuite, run_info_data, test_env, url)


def _bare_control(product, kwargs, subsuite, run_info_data, test_env, url):
    bk_kwargs = dict(kwargs)
    bk_kwargs["config"] = test_env.config
    bk_kwargs["subsuite"] = subsuite
    browser_kwargs = product.get_browser_kwargs(
        wr.logger, TEST_TYPE, run_info_data, **bk_kwargs)
    browser_cls = product.get_browser_cls(TEST_TYPE)
    browser = browser_cls(wr.logger, manager_number=1, **browser_kwargs)
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
                    print(f"[probe] variant B RESULT: hung past {elapsed:.1f}s "
                          f"(asyncio.wait_for timeout)")
                    return
                except Exception as e:  # noqa: BLE001 — report whatever BiDi raised
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant B RESULT: exception after "
                          f"{elapsed:.1f}s: {e!r}")
                    return
                elapsed = time.monotonic() - t0
                print(f"[probe] variant B RESULT: navigate(wait=complete) "
                      f"returned in {elapsed:.2f}s")
            finally:
                await session.end()

        loop.run_until_complete(_probe())
        loop.close()
    finally:
        browser.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", default=os.path.join(REPO_ROOT, "target", "dev-release", "lumen"))
    parser.add_argument("--seconds", type=float, default=55.0)
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
