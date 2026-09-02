#!/usr/bin/env python3
"""BUG-961 срез 47: "Что нужно" item 2 — finally get a `wchan` trace from a
REAL stall, by combining срез 42's actual launch path (real `LumenBrowser`/
`mozprocess.ProcessHandler`, via `product.get_browser_cls("lumen")` —
the shape that stalls 5/5 per срез 46) with срез 43's `_WchanSampler` (which
so far was only ever wired to the bare-`Popen` shape that barely stalls,
≤1/186 per срезы 43/45/46).

Срез 43's sampler polls `/proc/<pid>/task/*/{comm,wchan}` for ONE pid — the
`lumen` process itself. This slice samples TWO process trees concurrently,
since срез 40's real-callback measurement showed the ~20MB console.log line
does not *arrive* at the Python side until ~30s after scripts finish loading,
and mozprocess's `ProcessReader`/`ProcessReaderStdout`/`ProcessReaderStderr`
threads (see `mozprocess/processhandler.py::ProcessReader`) live inside the
CALLING interpreter, not inside `lumen`:

1. `browser.pid` (`lumen`'s own process tree, every thread) — same as срез 43.
2. `os.getpid()` (this script's own interpreter, i.e. wherever
   `mozprocess.ProcessReader`'s reader threads park) — new this slice.

If the stall is genuine engine-side computation, (1) should show a `lumen`
thread using CPU (or a wchan the array/console.log path would plausibly hit)
for ~30s. If it is mozprocess-side backpressure or a reader-thread bug, (2)
should show `ProcessReader*` parked on something unusual for the same window
while (1) shows `lumen` mostly idle/blocked instead.

Reuses срез 42's `_build_kwargs`/`_make_static_server`/environment-boot
machinery verbatim (only variant C — the static copy — is run; срез 42/46
already proved C and D stall identically, so a second launch buys nothing
here) and срез 43's `_WchanSampler` via direct import (no copy-paste).

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_bug961_slice47_wchan_real_launch.py
        [--binary target/dev-release/lumen] [--seconds 45]

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import asyncio
import os
import sys
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
    TESTS_ROOT,  # for serve_wpt_like / verify_bug961_slice42_static / _slice43
]

import localpaths  # noqa: E402,F401  (repo_root bootstrap wptrunner expects)
from wptrunner import environment as env_mod  # noqa: E402
from wptrunner import wptcommandline, wptrunner as wr  # noqa: E402
from wptrunner.testloader import Subsuite  # noqa: E402

import verify_bug961_slice42_static as s42  # noqa: E402  (_build_kwargs, _make_static_server)
import verify_bug961_slice43_wchan as s43  # noqa: E402  (_WchanSampler, _summarize)

TEST_ID = s42.TEST_ID
TEST_TYPE = s42.TEST_TYPE


def _launch_and_sample(product, kwargs, subsuite, run_info_data, config, url,
                        manager_number, label, seconds):
    bk_kwargs = dict(kwargs)
    bk_kwargs["config"] = config
    bk_kwargs["subsuite"] = subsuite
    browser_kwargs = product.get_browser_kwargs(
        wr.logger, TEST_TYPE, run_info_data, **bk_kwargs)
    browser_cls = product.get_browser_cls(TEST_TYPE)
    browser = browser_cls(wr.logger, manager_number=manager_number, **browser_kwargs)
    browser.start(group_metadata={})
    lumen_pid = browser.pid
    interpreter_pid = os.getpid()
    print(f"[probe] {label}: lumen pid={lumen_pid}, interpreter (this "
          f"process, where mozprocess.ProcessReader's threads live) "
          f"pid={interpreter_pid}")
    try:
        from webdriver.bidi.client import BidiSession

        loop = asyncio.new_event_loop()
        lumen_sampler = s43._WchanSampler(lumen_pid)
        interp_sampler = s43._WchanSampler(interpreter_pid)

        async def _probe():
            session = BidiSession.bidi_only(
                browser.url,
                requested_capabilities={"alwaysMatch": {"token": browser.token}})
            await session.start(loop)
            try:
                contexts = await session.browsing_context.get_tree()
                context = contexts[0]["context"]
                lumen_sampler.start()
                interp_sampler.start()
                t0 = time.monotonic()
                try:
                    await asyncio.wait_for(
                        session.browsing_context.navigate(
                            context=context, url=url, wait="complete"),
                        timeout=seconds)
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
                lumen_sampler.stop()
                interp_sampler.stop()
                await session.end()

        loop.run_until_complete(_probe())
        loop.close()

        print(f"[probe] {label}: lumen process tree (pid={lumen_pid}) wchan samples:")
        s43._summarize(lumen_sampler.samples, s43_t_start(lumen_sampler))
        print(f"[probe] {label}: interpreter process (pid={interpreter_pid}, "
              f"where mozprocess's ProcessReader* threads run) wchan samples:")
        s43._summarize(interp_sampler.samples, s43_t_start(interp_sampler))
    finally:
        browser.stop()


def s43_t_start(sampler):
    """`_summarize` wants a `t_start` to compute relative offsets — use the
    first sample's own timestamp (matches how срез 43 calls it with its own
    `t_process_start`, but this probe starts sampling at navigate-time, not
    process-launch-time, so "first sample" is the right zero point here)."""
    return sampler.samples[0][0] if sampler.samples else time.monotonic()


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

        httpd, static_port = s42._make_static_server(static_body)
        static_url = f"http://127.0.0.1:{static_port}{TEST_ID}"
        print(f"[probe] static server up on port {static_port}, serving the "
              f"pre-fetched bytes at {static_url}")

        try:
            print("[probe] variant C (real LumenBrowser/mozprocess launch, "
                  "static-served pre-fetched live-route bytes), instrumented "
                  "with срез 43's _WchanSampler on both process trees:")
            _launch_and_sample(product, kwargs, subsuite, run_info_data,
                                test_env.config, static_url, 2,
                                "variant C (static copy)", seconds)
        finally:
            httpd.shutdown()
            httpd.server_close()


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
    kwargs = s42._build_kwargs(args.binary)

    with wr.GlobalLogger(kwargs, {"raw": sys.stdout}):
        _run_probe(kwargs, args.seconds)
    return 0


if __name__ == "__main__":
    sys.exit(main())
