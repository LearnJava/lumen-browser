#!/usr/bin/env python3
"""S4 smoke driver (`docs/tasks/p2-wpt-integration.md`): run the vendored,
unmodified `wptrunner` against one real Lumen-vendored WPT test over
WebDriver BiDi, end to end.

This is deliberately NOT `tools/wpt/wpt` — that CLI wrapper (venv/browser
bootstrapping on top of wptrunner) isn't vendored (see `tests/wpt/VENDOR.md`,
"intentionally left for S3/S4, which is where it's actually invoked" turned
out to mean S7's polished wrapper instead; this script is the minimal S4
stand-in). It builds the same `wptcommandline`/`wptrunner.run_tests` call
`tools/wpt/wpt run` makes internally, with just the flags our nonstandard
`tests/wpt/` layout needs (`--tests`, `--metadata`, no ini-file test root).
S7 ("CI wrapper + docs") added `tests/wpt/run_suite.py` on top of this: a thin
wrapper that runs the *whole* curated subset (auto-discovered from the committed
`.ini` expectations) as one pass/fail gate, reusing this module's `run()`. Use
this script directly only to run an ad-hoc test-id list.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/run_smoke.py [--binary PATH] [test_id ...]

`test_id` defaults to `/dom/nodes/Element-hasAttribute.html` — a fully
synchronous `test()`-based DOM test with no iframes/XHR/testdriver, chosen as
the S4 proof because it needs none of the machinery (multi-window,
`test_driver.*`) this minimal BiDi-only executor doesn't implement yet.

Exit code mirrors `wpt run`: 0 if every included test's result matched its
(implicit, no-expectations-yet) expectation, i.e. every subtest PASSed;
non-zero otherwise.
"""

import argparse
import os
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
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

# WPT-RUN-9, startup: nothing heavy is imported at module level. wptrunner
# starts its wptserve daemons, managers and test runners with the `spawn`
# start method, and on Windows `spawn` re-executes the parent's `__main__`
# file — this one — in every child (`multiprocessing.spawn`,
# `init_main_from_path`) before the child can even read its target. With
# `wptrunner` imported up here that was ~0.9 s per child, and `ServerProc.start`
# blocks the parent until the child has read its arguments, so the seven
# servers came up strictly one after another, ~1.05 s each. `_load_wptrunner`
# imports the same modules lazily, in the parent only (measurement —
# `docs/tasks/p2-wpt-runner-throughput.md` §старт шарда).


def _load_wptrunner():
    """Import wptrunner (and its `localpaths` bootstrap) on first use."""
    import localpaths  # noqa: PLC0415,F401  (repo_root bootstrap wptrunner expects)
    from wptrunner import wptcommandline, wptrunner  # noqa: PLC0415
    return wptcommandline, wptrunner


# BUG-1024: the browser subprocess wptrunner spawns inherits this process's
# environment, so setting this here (once, at import time — `run_report.py`
# reuses `run()` below rather than re-execing) reaches every `lumen` instance
# launched by either entry point. Canvas fingerprint noise (ADR-007 layer 4,
# docs/plan/privacy.md §9.5, `crates/js/src/canvas2d.rs::session_seed()`) is
# seeded from wall-clock time + PID — correct for the anti-fingerprinting
# threat model, wrong for conformance: it perturbs every `getImageData()` by
# up to ±1/channel, differently on every browser launch, so a WPT canvas test
# that reads pixels back (`canvas-tests.js`'s `_assertPixel`, zero tolerance)
# flakes between separate `--check` runs even though the render itself is
# bit-exact (confirmed in isolation, `bugs/BUG-1024-FIXED.md` §Локализация).
# `setdefault` so an explicit override in the calling shell still wins.
os.environ.setdefault("LUMEN_DISABLE_CANVAS_NOISE", "1")
# Same reasoning, one layer over: an `OfflineAudioContext` render carries the
# ADR-007 layer 4 audio fingerprint noise (BUG-908), which flakes WPT's
# `webaudio/*` constant-source tests (`assert_array_equals` against an exact
# rendered sample) the same way uncontrolled canvas noise did above.
os.environ.setdefault("LUMEN_DISABLE_AUDIO_NOISE", "1")

#: WPT-RUN-9 parallel lanes: path of a `config.json` that replaces
#: `tests/wpt/config.json` for this one wptrunner process. Upstream reads the
#: override from a fixed place (`environment.py::build_config`,
#: `serve_path(test_paths)/config.json`), so two concurrent shards would both
#: bind 18300/18443/… and the second would die at wptserve startup.
#: `run_corpus.py --parallel-shards` writes one port-shifted copy per lane and
#: points each shard at its lane's copy through this variable.
SERVER_CONFIG_ENV = "LUMEN_WPT_SERVER_CONFIG"


def _install_server_config_override() -> None:
    """Make `TestEnvironment.build_config` apply `$LUMEN_WPT_SERVER_CONFIG`.

    Patched rather than edited in `tools/wptrunner` so the vendored tree stays
    upstream-identical. Upstream's `build_config` runs unchanged (defaults,
    then `tests/wpt/config.json`); the lane file is merged on top through the
    same `ConfigBuilder.update` upstream uses, so only the keys it names —
    the ports — differ from a normal run.
    """
    path = os.environ.get(SERVER_CONFIG_ENV)
    if not path:
        return
    if not os.path.isfile(path):
        raise SystemExit(f"{SERVER_CONFIG_ENV}={path}: no such file")
    import json  # noqa: PLC0415
    _load_wptrunner()
    from wptrunner import environment  # noqa: PLC0415 — after sys.path setup

    if getattr(environment.TestEnvironment.build_config, "_lumen_lane_override", False):
        return  # `run()` called again in the same process — already patched
    with open(path, encoding="utf-8") as fh:
        override = json.load(fh)
    original_build_config = environment.TestEnvironment.build_config

    def build_config(self):
        config = original_build_config(self)
        config.update(override)
        return config

    build_config._lumen_lane_override = True
    environment.TestEnvironment.build_config = build_config


#: `run_smoke.py`'s own flag (not wptrunner's): run the selected tests from
#: one shared queue instead of wptrunner's fixed per-process split. See
#: `_install_shared_queue` and `run_corpus.py --shared-queue`.
SHARED_QUEUE_FLAG = "--lumen-shared-queue"

#: wptrunner arguments the shared queue translates to. `--fully-parallel` makes
#: every test its own group (`FullyParallelGroupedSource`), and groups are
#: pulled by whichever process is free — a shared queue, ordered by declared
#: timeout, longest first (`TestQueueBuilder.make_queue`).
#: `--no-restart-on-new-group` keeps that from turning into one browser launch
#: per test: upstream restarts the browser on every group change. A real
#: crash or hang still restarts it (`restart_before_next`, `testrunner.py`).
SHARED_QUEUE_WPT_ARGS = ["--fully-parallel", "--no-restart-on-new-group"]


def interleave_by_directory(items: list, key) -> list:
    """Round-robin `items` over their directories, keeping order inside each.

    `key(item)` is the item's URL path. Used on `FullyParallelGroupedSource`'s
    groups, which come out in manifest order: with `--processes 7` the queue
    head then starts seven neighbouring files of one directory at the same
    moment, and directories of near-identical tests that compete for the same
    thing (the ten `dom/events/non-cancelable-when-passive/*touch*` tests,
    each driving `test_driver.Actions` for ~10 s) all TIMEOUT together — a
    verdict the old per-process hash split, which scatters a directory across
    processes, never produced (A/B in `docs/tasks/p2-wpt-runner-throughput.md`
    §общая очередь). `make_queue` sorts the result by timeout with a stable
    sort, so the interleaving survives inside every timeout tier.
    """
    buckets = {}
    for item in items:
        buckets.setdefault(key(item).rsplit("/", 1)[0], []).append(item)
    queues = list(buckets.values())
    out = []
    depth = 0
    while len(out) < len(items):
        for queue in queues:
            if depth < len(queue):
                out.append(queue[depth])
        depth += 1
    return out


def _install_shared_queue() -> None:
    """Make `FullyParallelGroupedSource` hand out its groups interleaved by
    directory (`interleave_by_directory`). Patched rather than edited in
    `tools/wptrunner`, like `_install_server_config_override`; only takes
    effect for a run that passes `--fully-parallel`."""
    from urllib.parse import urlsplit  # noqa: PLC0415
    _load_wptrunner()
    from wptrunner import testloader  # noqa: PLC0415 — after sys.path setup

    original_make_groups = testloader.FullyParallelGroupedSource.make_groups

    def make_groups(self, tests_by_type):
        groups = original_make_groups(self, tests_by_type)
        return interleave_by_directory(
            groups, lambda group: urlsplit(group.test_queue[0].url).path)

    testloader.FullyParallelGroupedSource.make_groups = make_groups


def default_binary() -> str:
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def run(binary: str, test_ids: list, extra_args: list = None) -> int:
    """Run the vendored `wptrunner` against `test_ids` using `binary`.

    Shared by this script's `--binary`/`test_ids` CLI, S7's `run_suite.py`
    (the whole-curated-subset gate) and `run_report.py` (HTML report, adds
    `--log-wptreport`/`--log-html` via `extra_args`). Returns wptrunner's exit
    code: 0 iff every included test matched its committed expectation (0
    unexpected results), non-zero otherwise.
    """
    if not os.path.isfile(binary):
        print(f"lumen binary not found: {binary}", file=sys.stderr)
        return 1

    os.makedirs(METADATA_ROOT, exist_ok=True)
    wptcommandline, wptrunner = _load_wptrunner()
    import shutdown_guard  # noqa: PLC0415  (BUG-1006)
    _install_server_config_override()

    argv = [
        "--product=lumen",
        f"--binary={binary}",
        f"--tests={TESTS_ROOT}",
        f"--metadata={METADATA_ROOT}",
        "--log-mach=-",
        # `wptcommandline`'s default pauses after each test when only one is
        # selected (`get_pause_after_test`) — that path calls
        # `protocol.base.wait()`, a `BaseProtocolPart` we don't implement
        # (`LumenBidiProtocol` has no ProtocolParts, see executorlumen.py),
        # crashing the runner. Not needed for an automated smoke run.
        "--no-pause-after-test",
        # wptrunner's default (`restart_on_unexpected=True`) respawns the
        # browser process after every test whose result doesn't match its
        # (often absent, under `--all`) expectation — with no committed
        # `.ini` most tests count as "unexpected", so this silently turned
        # into "one lumen.exe process per test". `LumenTestharnessExecutor`
        # already relies on one browser process + one reused browsing
        # context per run (`context_id` fetched once in `after_connect`,
        # navigated fresh per test — see executorlumen.py docstring), so
        # restarting on unexpected is neither needed for isolation (a fresh
        # `browsingContext.navigate` already gives every test a fresh
        # `window`) nor desired here — it only slows the run down. The
        # browser is still restarted on an actual crash/hang
        # (`restart_required` in testrunner.py), just not on a plain
        # FAIL/ERROR/TIMEOUT.
        "--no-restart-on-unexpected",
        # `--ssl-type` left unset auto-detects: "openssl" if an `openssl`
        # binary happens to be on PATH at run time, else silently "none".
        # "none" makes `TestEnvironment._get_ports` (wptserve `config.py`)
        # skip allocating an https port entirely — every `.https.`-only test
        # (most of WebCryptoAPI, `ai`, part of FileAPI/accelerometer) then
        # gets a literal "None" substituted into its `{{ports[https][0]}}`
        # URL, which Lumen can't navigate to, so the executor's readiness
        # poll just times out (`invalid port: "None"`, see
        # docs/tasks/p2-wpt-runner-throughput.md WPT-RUN-2). Pinning
        # "pregenerated" with a checked-in self-signed cert (tests/wpt/certs/,
        # CN=127.0.0.1 + matching SAN, 100-year expiry — this project's
        # offline-only rule rules out ACME/live reissuance) makes https
        # port allocation deterministic across machines instead of depending
        # on whether openssl happens to be installed.
        "--ssl-type=pregenerated",
        f"--ca-cert-path={os.path.join(CERTS_ROOT, 'ca-cert.pem')}",
        f"--host-cert-path={os.path.join(CERTS_ROOT, 'host-cert.pem')}",
        f"--host-key-path={os.path.join(CERTS_ROOT, 'host-key.pem')}",
    ] + list(extra_args or []) + list(test_ids)

    cmd_parser = wptcommandline.create_parser()
    kwargs = vars(cmd_parser.parse_args(argv))
    wptcommandline.check_args(kwargs)

    # BUG-1006: a SIGTERM/SIGBREAK (or Ctrl-C) mid-run must end the process in
    # bounded time — see `shutdown_guard.py` for why upstream's own shutdown
    # can block forever on an orphaned browser's pipes.
    shutdown_guard.install(wptrunner)
    with wptrunner.GlobalLogger(kwargs, {"raw": sys.stdout}):
        try:
            rv = wptrunner.start(**kwargs)
        except KeyboardInterrupt:
            shutdown_guard.arm()
            raise
    return rv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    parser.add_argument("test_ids", nargs="*", default=["/dom/nodes/Element-hasAttribute.html"])
    # WPT-RUN-4: `run_corpus.py` drives this script as a *subprocess* (so a
    # category that hangs or crashes the interpreter can be killed and skipped
    # rather than taking the whole corpus run down with it), and needs to pass
    # `--log-wptreport`/`--processes` straight through to wptrunner. Unknown
    # args are forwarded verbatim instead of being enumerated here, so this
    # stays a passthrough rather than a second copy of wptcommandline.
    args, extra_args = parser.parse_known_args()

    if SHARED_QUEUE_FLAG in extra_args:
        extra_args = [a for a in extra_args if a != SHARED_QUEUE_FLAG] + SHARED_QUEUE_WPT_ARGS
        _install_shared_queue()

    return run(args.binary, args.test_ids, extra_args)


if __name__ == "__main__":
    sys.exit(main())
