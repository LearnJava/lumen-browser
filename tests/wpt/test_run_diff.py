"""Tests for `run_diff.py` and the ids file: `python tests/wpt/test_run_diff.py`."""

import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ids_file  # noqa: E402
import run_diff  # noqa: E402


def row(status, passed=0, total=0, score=None, test_type="testharness", shard="s"):
    if score is None:
        score = passed / total if total else (1.0 if status == "PASS" else 0.0)
    return (status, test_type, passed, total, score, shard)


def table(**tests):
    """`{id: row}` as `ids_file.read` returns it; keys use `__` for `/`."""
    out = {}
    for key, (status, test_type, passed, total, score, shard) in tests.items():
        out["/" + key.replace("__", "/")] = {"type": test_type, "status": status,
                                              "passed": passed, "total": total,
                                              "score": score, "shard": shard}
    return out


def write_snapshot(directory, name, tests, with_ids=True):
    """Write `<name>.json` (+ `<name>.ids.tsv.gz`) the way `run_corpus.py` does."""
    snapshot = {"commit": "abc"}
    if with_ids:
        path = os.path.join(directory, name + ids_file.SUFFIX)
        ids_file.write(path, [(i, r["type"], r["status"], r["passed"], r["total"], r["score"],
                               r["shard"]) for i, r in tests.items()])
        snapshot["ids_file"] = os.path.basename(path)
    json_path = os.path.join(directory, name + ".json")
    with open(json_path, "w", encoding="utf-8") as fh:
        json.dump(snapshot, fh)
    return json_path


class CompareTests(unittest.TestCase):
    def setUp(self):
        self.old = table(
            quirks__a_html=row("PASS"),
            quirks__b_html=row("OK", 4, 4),
            quirks__same_html=row("PASS"),
            quirks__flaky_html=row("FAIL"),
            css__css_break__c_html=row("PASS"),
            css__css_break__gone_html=row("PASS"),
            workers__w_html=row("OK", 1, 4),
        )
        self.new = table(
            quirks__a_html=row("FAIL"),
            quirks__b_html=row("OK", 3, 4),
            quirks__same_html=row("PASS"),
            quirks__flaky_html=row("TIMEOUT"),
            css__css_break__c_html=row("PASS"),
            css__css_break__new_html=row("PASS"),
            workers__w_html=row("OK", 3, 4),
        )

    def test_worse_better_moved_are_split(self):
        result = run_diff.compare(self.old, self.new)
        self.assertEqual([e["id"] for e in result["worse"]],
                         ["/css/css_break/gone_html", "/quirks/a_html", "/quirks/b_html"])
        self.assertEqual([e["id"] for e in result["better"]],
                         ["/css/css_break/new_html", "/workers/w_html"])
        self.assertEqual([e["id"] for e in result["moved"]], ["/quirks/flaky_html"])

    def test_absent_side_scores_zero(self):
        result = run_diff.compare(self.old, self.new)
        gone = result["worse"][0]
        self.assertEqual((gone["new_status"], gone["new_score"], gone["delta"]),
                         (run_diff.ABSENT, 0.0, -1.0))
        added = result["better"][0]
        self.assertEqual((added["old_status"], added["delta"]), (run_diff.ABSENT, 1.0))

    def test_category_deltas_add_up_to_the_total(self):
        result = run_diff.compare(self.old, self.new)
        self.assertAlmostEqual(
            sum(row["delta"] for row in result["categories"].values()),
            result["totals"]["delta"], places=2)
        quirks = result["categories"]["quirks"]
        self.assertEqual((quirks["worse"], quirks["better"], quirks["moved"]), (2, 0, 1))
        self.assertEqual(quirks["delta"], -1.25)

    def test_prefix_selects_a_subdirectory_only(self):
        result = run_diff.compare(self.old, self.new, ["css/css_break"])
        self.assertEqual(set(result["categories"]), {"css"})
        self.assertEqual(result["totals"]["worse"], 1)
        # `quirks` must not match a prefix `quirk`
        self.assertEqual(run_diff.compare(self.old, self.new, ["quirk"])["totals"]["worse"], 0)

    def test_min_delta_hides_small_score_changes_but_not_status_moves(self):
        result = run_diff.compare(self.old, self.new, min_delta=0.5)
        self.assertEqual([e["id"] for e in result["worse"]],
                         ["/css/css_break/gone_html", "/quirks/a_html"])
        self.assertEqual(len(result["moved"]), 1)

    def test_identical_snapshots_are_empty(self):
        result = run_diff.compare(self.old, self.old)
        self.assertEqual((result["worse"], result["better"], result["moved"]), ([], [], []))
        self.assertEqual(result["totals"]["delta"], 0)

    def test_variant_id_belongs_to_its_file_category(self):
        self.assertEqual(run_diff.category_of("/quirks/x.html?a=1/b"), "quirks")
        self.assertEqual(run_diff.category_of("/root.html"), "root.html")

    def test_transition_text(self):
        result = run_diff.compare(self.old, self.new)
        by_id = {e["id"]: e for e in result["worse"] + result["better"]}
        self.assertEqual(run_diff.transition(by_id["/quirks/a_html"]), "PASS -> FAIL")
        self.assertEqual(run_diff.transition(by_id["/quirks/b_html"]), "OK 4/4 -> OK 3/4")

    def test_worst_tests_limits_and_orders(self):
        worst = run_diff.worst_tests(self.old, self.new, "quirks", limit=1)
        self.assertEqual([e["id"] for e in worst], ["/quirks/a_html"])


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="lumen-run-diff-test-")
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)

    def test_ids_file_roundtrip_and_stable_bytes(self):
        rows = [("/b.html", "reftest", "FAIL", 0, 0, 0.0, "s"),
                ("/a.html", "testharness", "OK", 2, 3, 2 / 3, "s")]
        first, second = (os.path.join(self.dir, n + ids_file.SUFFIX) for n in ("x", "y"))
        ids_file.write(first, rows)
        ids_file.write(second, list(reversed(rows)))
        with open(first, "rb") as a, open(second, "rb") as b:
            self.assertEqual(a.read(), b.read())
        back = ids_file.read(first)
        self.assertEqual(list(back), ["/a.html", "/b.html"])
        self.assertEqual((back["/a.html"]["passed"], back["/a.html"]["total"]), (2, 3))
        self.assertAlmostEqual(back["/a.html"]["score"], 0.6667, places=4)
        self.assertEqual(back["/b.html"]["status"], "FAIL")

    def test_ids_path_for(self):
        self.assertEqual(ids_file.ids_path_for("docs/wpt/runs/2026-10-22.json"),
                         "docs/wpt/runs/2026-10-22.ids.tsv.gz")

    def test_cli_json_output(self):
        a = write_snapshot(self.dir, "a", table(t__x_html=row("PASS")))
        b = write_snapshot(self.dir, "b", table(t__x_html=row("FAIL")))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            status = run_diff.main([a, b, "--json"])
        self.assertEqual(status, 0)
        parsed = json.loads(buf.getvalue())
        self.assertEqual(parsed["totals"]["worse"], 1)
        self.assertEqual(parsed["worse"][0]["id"], "/t/x_html")

    def test_cli_text_output_names_the_test(self):
        a = write_snapshot(self.dir, "a", table(t__x_html=row("PASS")))
        b = write_snapshot(self.dir, "b", table(t__x_html=row("FAIL")))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            run_diff.main([a, b])
        self.assertIn("/t/x_html  PASS -> FAIL  -1.00", buf.getvalue())

    def test_snapshot_without_ids_file_is_refused(self):
        a = write_snapshot(self.dir, "a", table(t__x_html=row("PASS")), with_ids=False)
        b = write_snapshot(self.dir, "b", table(t__x_html=row("PASS")))
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            status = run_diff.main([a, b])
        self.assertEqual(status, 2)
        self.assertIn("no `ids_file`", err.getvalue())

    def test_missing_ids_file_on_disk_is_refused(self):
        a = write_snapshot(self.dir, "a", table(t__x_html=row("PASS")))
        os.remove(os.path.join(self.dir, "a" + ids_file.SUFFIX))
        with self.assertRaises(run_diff.NoIdsFile):
            run_diff.load_snapshot(a)


if __name__ == "__main__":
    unittest.main()
