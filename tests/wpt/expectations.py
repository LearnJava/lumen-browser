"""TEST-3 (docs/tasks/p2-test-track.md): per-test WPT expectations, generated
from a `run_report.py` run and gated on with `run_report.py --check`.

Turns the 277 WPT-VENDOR categories from a one-off audit (docs/wpt-status.md)
into a regression set, Firefox-metadata style: a committed `.ini` file per
test records which (sub)tests are *known* not to PASS, so `--check` can fail
only on a genuine regression — an expected PASS that stopped passing, or a
new TIMEOUT — instead of on the (large, expected) baseline of FAIL/ERROR
results a freshly-vendored category always has.

Deliberately reuses vendored, unmodified `wptrunner` machinery rather than
inventing a parallel format:

- The `.ini` files live under the same `--metadata` root
  (`run_smoke.METADATA_ROOT`) wptrunner already loads on every run (see
  `run_smoke.run`), in the native `wptmanifest` syntax
  (`tools/wptrunner/wptrunner/manifestexpected.py`) — so a generated file is
  indistinguishable from a hand-written one and needs no custom loader.
- `wptrunner` itself computes the expected-vs-actual comparison: its
  structured logger only includes an `"expected"` key on a `test_status`/
  `test_end` message when the result is *unexpected*
  (`testrunner.py::test_ended`, `is_unexpected = expected != result.status`)
  — matching Mozilla's mozlog convention. So `--check` needs no independent
  `.ini` parsing either: presence/absence of `"expected"` in the wptreport
  JSON already tells us whether this (sub)test deviated from its committed
  expectation, and `"status"` is always the raw actual result either way.

Deliberately does NOT touch `tests/wpt/metadata/dom/nodes/` — that subtree is
a different, hand-curated mechanism (S5/S6, gated by `run_suite.py`): every
file there is fully-PASS by construction (a test is only added once vetted
clean), annotated with prose explaining *why* it passes now (bug numbers,
prerequisites). Auto-regenerating it here would silently delete those
annotations and could shrink `run_suite.curated_test_ids()` (which discovers
its gate set by `.ini` presence). `write_expected` refuses `dom/nodes`
outright.
"""

import os
import sys
from urllib.parse import urlsplit

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_smoke  # noqa: E402

sys.path[:0] = [
    os.path.join(run_smoke.REPO_ROOT, "tools", "wptrunner"),
]
from wptrunner.manifestupdate import get_test_name  # noqa: E402
from wptrunner.wptmanifest.node import DataNode, KeyValueNode, ValueNode  # noqa: E402
from wptrunner.wptmanifest.serializer import serialize  # noqa: E402

# Native wptrunner defaults (wptrunner/wpttest.py): a testharness test with no
# `.ini` override is expected "OK"; a subtest with no override is expected
# "PASS". Anything else needs an explicit `expected:` line.
DEFAULT_TEST_STATUS = "OK"
DEFAULT_SUBTEST_STATUS = "PASS"
HARNESS_GOOD_STATUSES = {"OK", "PASS"}
SUBTEST_GOOD_STATUSES = {"PASS"}

# Hand-curated by S5/S6, gated by run_suite.py — never auto-write here.
GUARDED_ROOTS = {"dom/nodes"}


def metadata_ini_path(test_id: str) -> str:
    """`/websockets/foo.html?wss` -> `<METADATA_ROOT>/websockets/foo.html.ini`.

    Multiple `test_id`s can share one underlying file (a `?query`/`#fragment`
    variant selector, e.g. WPT's `.any.js` multi-global expansion or a plain
    `.html` test navigated with different flags) — wptrunner's own
    `expected.expected_path` keys the `.ini` off the URL *path* alone, so all
    variants of one file share one `.ini` with one `[section]` per variant
    (see `_test_node_for`/`get_test_name`).
    """
    file_path = urlsplit(test_id).path
    rel = file_path.lstrip("/").split("/")
    return os.path.join(run_smoke.METADATA_ROOT, *rel) + ".ini"


def _test_node_for(result: dict) -> DataNode:
    """Build a `DataNode` for one test's overrides, or `None` if fully clean
    (harness OK, every subtest PASS, or no subtests at all).

    Section name is `get_test_name(test_id)` — "base name of test path +
    query string + fragment" per `manifestupdate.py`, the same value
    wptrunner itself uses to key `ExpectedManifest.get_test` at runtime
    (`testloader.py::TestLoader.get_test`) — so a `?query` variant gets its
    own section instead of colliding with its siblings.
    """
    test_name = get_test_name(result["test"])
    harness_status = result.get("status", DEFAULT_TEST_STATUS)
    subtests = result.get("subtests", [])

    test_node = DataNode(test_name)

    if harness_status != DEFAULT_TEST_STATUS:
        kv = KeyValueNode("expected")
        kv.append(ValueNode(harness_status))
        test_node.append(kv)

    for st in subtests:
        st_status = st.get("status", DEFAULT_SUBTEST_STATUS)
        if st_status == DEFAULT_SUBTEST_STATUS:
            continue
        sub_node = DataNode(st.get("name", ""))
        kv = KeyValueNode("expected")
        kv.append(ValueNode(st_status))
        sub_node.append(kv)
        test_node.append(sub_node)

    return test_node if test_node.children else None


def build_expected_ini(results_for_file: list) -> str:
    """Serialize every `?query`/`#fragment` variant of one underlying file
    (as selected by `metadata_ini_path`) into that file's combined `.ini`
    text. Returns "" if every variant is fully clean — caller should remove
    any stale `.ini` instead of writing an empty one.
    """
    root = DataNode(None)
    for result in results_for_file:
        node = _test_node_for(result)
        if node is not None:
            root.append(node)

    if not root.children:
        return ""
    return serialize(root)


def write_expected(results: list, root: str) -> dict:
    """(Re)generate `.ini` baselines for every test in `results`, grouping
    `?query`/`#fragment` variants of the same file into one combined file
    (see `metadata_ini_path`).

    A fully-clean file gets its stale `.ini` removed (if any) rather than an
    empty file written, so the tree only ever holds files that carry real
    overrides — same ratchet spirit as `graphic_tests/KNOWN_DEBTORS`: a file
    disappearing means the test(s) got fixed, not that no one looked at it.

    Returns counts: {"written": N, "removed": N, "unchanged": N}.
    """
    if root in GUARDED_ROOTS:
        raise SystemExit(
            f"refusing --update-expected for {root!r}: that subtree is the hand-curated "
            "S5/S6 gate (run_suite.py), not this ratchet — see tests/wpt/expectations.py docstring"
        )

    groups = {}
    for result in results:
        groups.setdefault(metadata_ini_path(result["test"]), []).append(result)

    written = removed = unchanged = 0
    for path, group in groups.items():
        text = build_expected_ini(group)
        existing = None
        if os.path.isfile(path):
            with open(path, encoding="utf-8") as f:
                existing = f.read()

        if not text:
            if existing is not None:
                os.remove(path)
                removed += 1
            continue

        if existing == text:
            unchanged += 1
            continue

        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        written += 1

    return {"written": written, "removed": removed, "unchanged": unchanged}


def classify(results: list, test_ids: list) -> dict:
    """Compare each result's actual status against wptrunner's own
    unexpected-result annotation (the `"expected"` key, present in the
    wptreport JSON only when the result deviated from the committed `.ini`).

    Returns {"regressions": [...], "improvements": [...], "other": [...]}.
    Each entry is a dict with test/subtest/expected/actual/reason.

    Gate semantics (docs/tasks/p2-test-track.md TEST-3 DoD): a regression is
    either an expected-PASS that stopped passing, or any new TIMEOUT
    (regardless of what it was expected to be — a hang is always worth
    surfacing, even on an already-known-bad test). Everything else that
    deviates is informational: an unexpected PASS narrows the ratchet
    (should tighten the committed `.ini`), any other status swap (e.g.
    FAIL -> ERROR) is a side note, not a gate failure.
    """
    regressions, improvements, other = [], [], []

    def classify_one(test, subtest, expected, actual):
        good_statuses = SUBTEST_GOOD_STATUSES if subtest is not None else HARNESS_GOOD_STATUSES
        was_good = expected in good_statuses
        is_good = actual in good_statuses
        entry = {"test": test, "subtest": subtest, "expected": expected, "actual": actual}
        if actual == "TIMEOUT" and expected != "TIMEOUT":
            entry["reason"] = "new TIMEOUT"
            regressions.append(entry)
        elif was_good and not is_good:
            entry["reason"] = "expected PASS regressed"
            regressions.append(entry)
        elif is_good and not was_good:
            entry["reason"] = "unexpected PASS — narrow expectations"
            improvements.append(entry)
        else:
            entry["reason"] = "status changed"
            other.append(entry)

    for r in results:
        if "expected" in r:
            classify_one(r["test"], None, r["expected"], r["status"])
        for st in r.get("subtests", []):
            if "expected" in st:
                classify_one(r["test"], st.get("name"), st["expected"], st["status"])

    # A selected id is a bare file id (`all_vendored_test_ids`/`curated_test_ids`
    # never carry a `?query`), but a `results` entry may be one of several
    # query-variant expansions of that file (WPT `variant=` meta tags,
    # resolved server-side) — compare by URL *path* so a variant-expanded
    # test isn't misreported as never having run at all.
    seen_paths = {urlsplit(r["test"]).path for r in results}
    for missing in sorted(t for t in test_ids if urlsplit(t).path not in seen_paths):
        regressions.append(
            {
                "test": missing,
                "subtest": None,
                "expected": None,
                "actual": "MISSING",
                "reason": "no result produced (crash before test_start / early abort)",
            }
        )

    return {"regressions": regressions, "improvements": improvements, "other": other}
