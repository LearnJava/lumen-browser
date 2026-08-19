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
    os.path.join(run_smoke.REPO_ROOT, "tools"),
]
from wptrunner.manifestupdate import get_test_name  # noqa: E402
from wptrunner.wptmanifest.node import DataNode, KeyValueNode, ValueNode  # noqa: E402
from wptrunner.wptmanifest.parser import parse as wptmanifest_parse  # noqa: E402
from wptrunner.wptmanifest.serializer import serialize  # noqa: E402
from manifest import manifest as wpt_manifest  # noqa: E402

# Native wptrunner defaults (wptrunner/wpttest.py): a testharness test with no
# `.ini` override is expected "OK"; a subtest with no override is expected
# "PASS". Anything else needs an explicit `expected:` line.
DEFAULT_TEST_STATUS = "OK"
DEFAULT_SUBTEST_STATUS = "PASS"
HARNESS_GOOD_STATUSES = {"OK", "PASS"}
SUBTEST_GOOD_STATUSES = {"PASS"}

# Hand-curated by S5/S6, gated by run_suite.py — never auto-write here.
GUARDED_ROOTS = {"dom/nodes"}

_url_to_source_path_cache = None


def _url_to_source_path() -> dict:
    """Map a manifest item's `id` (URL, what `run_report.py`'s results carry
    as `"test"`) to its `path` (source file, relative to `TESTS_ROOT`).

    For a plain `.html` test the two coincide, but WPT's generated test
    types don't: a `.any.js` source expands to `.any.html` /
    `.any.worker.html` / ... (one URL per global), and `.window.js` /
    `.worker.js` each expand to one differently-named `.html` URL
    (`tools/manifest/sourcefile.py::global_variant_url`/`replace_end`).
    `wptrunner`'s own `expected.expected_path` (unmodified vendor code, see
    `tools/wptrunner/wptrunner/testloader.py::load_metadata`) keys the
    committed `.ini` off `path` — the *source* file — not the URL, at
    runtime. Reusing the same vendored manifest reader here (rather than
    reimplementing the suffix-stripping rules, which have several
    special-cased variants for shadowrealm/print-reftest) keeps this
    resolution byte-for-byte identical to what `wptrunner` will actually
    look up, so a baseline this module writes is guaranteed to be the file
    `wptrunner` re-reads on the next `--check`.
    """
    global _url_to_source_path_cache
    if _url_to_source_path_cache is None:
        manifest_path = os.path.join(run_smoke.METADATA_ROOT, "MANIFEST.json")
        mapping = {}
        m = wpt_manifest.load(run_smoke.TESTS_ROOT, manifest_path)
        if m is not None:
            for _item_type, path, items in m:
                for item in items:
                    mapping[item.id] = path.replace(os.sep, "/")
        _url_to_source_path_cache = mapping
    return _url_to_source_path_cache


def metadata_ini_path(test_id: str) -> str:
    """`/websockets/foo.html?wss` -> `<METADATA_ROOT>/websockets/foo.html.ini`;
    `/console/console-is-a-namespace.any.html` ->
    `<METADATA_ROOT>/console/console-is-a-namespace.any.js.ini` (source path,
    see `_url_to_source_path`).

    Multiple `test_id`s can share one underlying file (a `?query`/`#fragment`
    variant selector, or — for `.any.js`/`.window.js`/`.worker.js` — several
    per-global URLs generated from one source) — wptrunner's own
    `expected.expected_path` keys the `.ini` off the source path alone, so
    all variants of one file share one `.ini` with one `[section]` per
    variant (see `_test_node_for`/`get_test_name`).
    """
    source_path = _url_to_source_path().get(test_id)
    if source_path is not None:
        rel = source_path.split("/")
    else:
        rel = urlsplit(test_id).path.lstrip("/").split("/")
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

    if harness_status and harness_status != DEFAULT_TEST_STATUS:
        kv = KeyValueNode("expected")
        kv.append(ValueNode(harness_status))
        test_node.append(kv)

    for st in subtests:
        st_status = st.get("status", DEFAULT_SUBTEST_STATUS)
        if not st_status or st_status == DEFAULT_SUBTEST_STATUS:
            continue
        st_name = st.get("name", "")
        if not _expressible_heading(st_name):
            continue
        sub_node = DataNode(st_name)
        kv = KeyValueNode("expected")
        kv.append(ValueNode(st_status))
        sub_node.append(kv)
        test_node.append(sub_node)

    return test_node if test_node.children else None


def _expressible_heading(name: str) -> bool:
    """Whether `name` can be a `[section]` heading at all.

    `wptmanifest`'s grammar closes a heading on the line it opened
    (`parser.py::heading_state` raises "EOL in heading"), so a subtest whose
    name contains a newline cannot be expressed — and writing one produces a
    file that makes wptrunner **abort the entire shard** before running a
    single test, not merely ignore that one expectation. WPT-RUN-4 hit exactly
    this: `css/css-shadow/part/pseudo-elements-after-part.html.ini` (a subtest
    named after a multi-line CSS block) silently disabled all 206 ids of
    `css/css-shadow`, and a second broken file disabled all 896 of
    `content-security-policy` — 759 tests absent from every run and from the
    `--check` gate, with no error anywhere except that shard's log.

    Dropping the expectation is the honest failure mode: the subtest then
    reports as unexpected (visible) instead of taking its whole category down
    with it (invisible).
    """
    return not any(ch in name for ch in "\r\n")


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
    text = serialize(root)

    # Round-trip through the same parser wptrunner uses at load time. A file
    # this generator cannot read back is one that aborts a whole shard at run
    # time (see `_expressible_heading`), so refusing to write it is strictly
    # better than emitting it: a missing baseline shows up as unexpected
    # results, a poisoned one shows up as a silently empty category.
    try:
        # bytes, not str: `manifestexpected.py` opens `.ini` files with
        # `open(path, "rb")`, and the tokenizer only handles the byte form —
        # handing it a `str` fails on every file, valid or not.
        wptmanifest_parse(text.encode("utf-8"))
    except Exception as exc:  # noqa: BLE001 — any parse failure is disqualifying
        print(f"refusing to write unparseable .ini ({exc}); expectations dropped for this file",
              file=sys.stderr)
        return ""
    return text


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
