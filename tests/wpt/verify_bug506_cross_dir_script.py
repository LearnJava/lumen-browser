#!/usr/bin/env python3
"""BUG-506 investigation: drive `LumenTestharnessExecutor._run_testharness`
(the real wptrunner mechanism, imported unmodified) against a real `wptserve`
instance, over `css/css-logical/animation-001.html` — one of the 5 files
whose third external `<script src="../css-animations/support/testcommon.js">`
(a cross-directory relative path) reportedly never executes before the
dependent inline `<script>` runs, under the real wptrunner pipeline only.

Modeled directly on `verify_bug961_orchestration.py` (same `TestEnvironment`
bootstrap, same executor classes, no `TestRunnerManager`) so any behaviour
difference is attributable to `executorlumen.py`/the engine, not to a
reimplemented harness.

Variant A: `LumenTestharnessExecutor._run_testharness` (real corpus path).
Variant B: bare `BidiSession.navigate(wait="complete")` + a SEPARATE
`script.evaluate` issued after `navigate` returns, against the SAME live
`wptserve` (isolates whether the discrepancy needs `executorlumen.py`'s own
poll loop at all, or shows up on a plain navigate+eval too).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug506_cross_dir_script.py
        [--binary target/dev-release/lumen]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import asyncio
import json
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
from wptrunner.testloader import Subsuite  # noqa: E402

TEST_ID = os.environ.get("BUG506_TEST_ID", "/css/css-logical/animation-001.html")
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
        False,
        kwargs["debug_test"],
        kwargs["debug_info"],
        product.env_options,
        ssl_config,
        env_extras,
        kwargs["enable_webtransport_h3"],
        kwargs["enable_dns"],
        None,
        None,
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
            helper_url = server_url(test_env.config, "http") + \
                "/css/css-animations/support/testcommon.js"

            # Confirm the helper itself is reachable/correct at that exact
            # cross-directory URL, independent of the page under test.
            import urllib.request
            with urllib.request.urlopen(helper_url, timeout=5) as resp:
                helper_body = resp.read().decode("utf-8", "replace")
            print(f"[probe] helper fetch {helper_url} -> {len(helper_body)} bytes, "
                  f"defines addDiv: {'function addDiv' in helper_body}")

            executor.protocol.connect()
            executor.protocol.after_connect()
            try:
                print(f"[probe] variant A (LumenTestharnessExecutor, real corpus "
                      f"path): navigating {url}")
                t0 = time.monotonic()
                try:
                    raw_result = executor.protocol.run(asyncio.wait_for(
                        executor._run_testharness(url, seconds),
                        timeout=seconds + 15))
                except asyncio.TimeoutError:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT: hung past {elapsed:.1f}s")
                except ExecutorException as e:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT: ExecutorException after "
                          f"{elapsed:.1f}s: {e.args!r}")
                else:
                    elapsed = time.monotonic() - t0
                    print(f"[probe] variant A RESULT ({elapsed:.2f}s):")
                    _print_result(raw_result)
            finally:
                executor.protocol.teardown()
        finally:
            browser.stop()

        print("[probe] variant B (bare BidiSession, navigate then a SEPARATE "
              "eval() after navigate returns, same real wptserve): fresh lumen")
        _bare_control(product, kwargs, subsuite, run_info_data, test_env, url)


def _print_result(raw_result):
    # `[url, harness_status, harness_message, harness_stack, subtests]`
    url, status, message, stack, subtests = raw_result
    print(f"  harness status={status} message={message!r}")
    for name, sub_status, sub_message, sub_stack in subtests:
        print(f"  subtest {sub_status} {name!r}: {sub_message!r}")


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
                await session.browsing_context.navigate(
                    context=context, url=url, wait="complete")
                elapsed = time.monotonic() - t0
                print(f"[probe] variant B navigate(wait=complete) returned in "
                      f"{elapsed:.2f}s, now evaluating in a SEPARATE call")
                from webdriver.bidi.modules.script import ContextTarget
                value = await session.script.evaluate(
                    expression="JSON.stringify({addDiv: typeof window.addDiv, "
                               "addStyle: typeof window.addStyle, "
                               "ready: document.readyState})",
                    target=ContextTarget(context), await_promise=False)
                print(f"[probe] variant B post-navigate eval: {value}")
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
    parser.add_argument("--seconds", type=float, default=20.0)
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
