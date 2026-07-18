#!/usr/bin/env python3
"""S7 CI wrapper (`docs/tasks/p2-wpt-integration.md`): run the whole curated
WPT subset against Lumen over WebDriver BiDi as a single pass/fail gate.

Unlike `run_smoke.py` (the S4 stand-in, which takes an explicit test-id list),
this discovers the curated subset automatically from the committed `.ini`
expectation files under `tests/wpt/metadata/dom/nodes/` — one test id per
`<name>.html.ini` — so "the curated subset" stays defined by exactly the tests
we keep expectations for (the S5 synchronous set + the S6 async set). Add an
`.ini` and the test joins the gate; there is no second list to keep in sync.

This is the repeatable local/CI invocation S7 asks for: exit 0 iff every
included test matched its committed expectation (0 unexpected results),
non-zero otherwise — same ratchet idea as `graphic_tests/`'s KNOWN_DEBTORS.

Usage (from repo root, after `pip install -r tests/wpt/requirements.txt` in a
venv — see tests/wpt/README.md):

    <venv>/python tests/wpt/run_suite.py [--binary PATH]

`--binary` defaults to `target/$LUMEN_PROFILE/lumen.exe` (`LUMEN_PROFILE` env
var, default `release`); pass it explicitly when running from a `git worktree`,
whose own `target/` is empty. On Windows Git Bash also set
`MSYS2_ARG_CONV_EXCL='/dom'` so the leading-slash test ids aren't mangled into
Windows paths (see README).
"""

import argparse
import glob
import os
import sys

# The script's own directory (tests/wpt/) must be importable so `run_smoke`
# resolves regardless of the caller's CWD.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_smoke  # noqa: E402


def curated_test_ids() -> list:
    """Every committed `.ini` under metadata/dom/nodes/ → its test id.

    `metadata/dom/nodes/<name>.html.ini` → `/dom/nodes/<name>.html`.
    """
    subdir = os.path.join(run_smoke.METADATA_ROOT, "dom", "nodes")
    ids = []
    for ini in sorted(glob.glob(os.path.join(subdir, "*.html.ini"))):
        html = os.path.basename(ini)[: -len(".ini")]  # drop trailing ".ini"
        ids.append("/dom/nodes/" + html)
    return ids


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=run_smoke.default_binary())
    args = parser.parse_args()

    test_ids = curated_test_ids()
    if not test_ids:
        print(
            "no curated .ini expectations found under "
            f"{os.path.join(run_smoke.METADATA_ROOT, 'dom', 'nodes')}",
            file=sys.stderr,
        )
        return 1

    print(f"running {len(test_ids)} curated WPT tests", file=sys.stderr)
    return run_smoke.run(args.binary, test_ids)


if __name__ == "__main__":
    sys.exit(main())
