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

sys.path[:0] = [
    REPO_ROOT,
    os.path.join(REPO_ROOT, "tools"),
    os.path.join(REPO_ROOT, "tools", "wptserve"),
    os.path.join(REPO_ROOT, "tools", "webdriver"),
    os.path.join(REPO_ROOT, "tools", "wptrunner"),
]

import localpaths  # noqa: E402,F401  (repo_root bootstrap wptrunner expects)
from wptrunner import wptcommandline, wptrunner  # noqa: E402


def default_binary() -> str:
    profile = os.environ.get("LUMEN_PROFILE", "release")
    return os.path.join(REPO_ROOT, "target", profile, "lumen.exe")


def run(binary: str, test_ids: list) -> int:
    """Run the vendored `wptrunner` against `test_ids` using `binary`.

    Shared by this script's `--binary`/`test_ids` CLI and S7's `run_suite.py`
    (the whole-curated-subset gate). Returns wptrunner's exit code: 0 iff every
    included test matched its committed expectation (0 unexpected results),
    non-zero otherwise.
    """
    if not os.path.isfile(binary):
        print(f"lumen binary not found: {binary}", file=sys.stderr)
        return 1

    os.makedirs(METADATA_ROOT, exist_ok=True)

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
    ] + list(test_ids)

    cmd_parser = wptcommandline.create_parser()
    kwargs = vars(cmd_parser.parse_args(argv))
    wptcommandline.check_args(kwargs)

    with wptrunner.GlobalLogger(kwargs, {"raw": sys.stdout}):
        rv = wptrunner.start(**kwargs)
    return rv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=default_binary())
    parser.add_argument("test_ids", nargs="*", default=["/dom/nodes/Element-hasAttribute.html"])
    args = parser.parse_args()

    return run(args.binary, args.test_ids)


if __name__ == "__main__":
    sys.exit(main())
