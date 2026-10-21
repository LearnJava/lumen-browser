"""Тесты сводного отчёта качества: `python scripts/test_quality_report.py`."""

from __future__ import annotations

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import quality_report as qr  # noqa: E402


def wpt_run(finished: str, pass_n: int, ok: int, skip: int, no_exec: int, css_score: float) -> dict:
    ids = pass_n + ok + skip + no_exec
    return {
        "commit": "abc1234", "finished": finished, "scope": "full-corpus",
        "scored": {
            "coverage": {"no_executor": no_exec},
            "status_counts": {"PASS": pass_n, "OK": ok, "SKIP": skip, "NOT-RUN": no_exec},
            "totals": {"ids": ids, "score": pass_n + css_score, "pass_rate": round((pass_n + css_score) / ids, 4),
                       "ran": ids - no_exec, "not_run": no_exec, "harness_ok": pass_n + ok,
                       "subtests_passed": 3, "subtests_total": 10},
            "per_category": {
                "css": {"ids": ids - 1, "ran": ids - 1 - no_exec, "harness_ok": pass_n + ok, "score": pass_n + css_score,
                        "pass_rate": round((pass_n + css_score) / (ids - 1), 4), "subtests_passed": 3,
                        "subtests_total": 10, "by_type": {"reftest": pass_n, "testharness": ok}},
                "fetch": {"ids": 1, "ran": 0, "harness_ok": 0, "score": 0.0, "pass_rate": 0.0,
                          "subtests_passed": 0, "subtests_total": 0, "by_type": {"wdspec": 1}},
            },
        },
    }


def gt_run(stamp: str, statuses: dict) -> dict:
    key = {"PASS": "passed", "FAIL": "failed", "ERROR": "errors", "DEBTOR": "debtors", "SKIP": "skipped"}
    summary = {v: 0 for v in key.values()}
    for s in statuses.values():
        summary[key[s]] += 1
    summary["total"] = len(statuses)
    return {"timestamp": stamp, "git": {"commit": "def5678", "branch": "main"}, "summary": summary,
            "tests": [{"id": k[:2], "stem": k, "status": s, "diff_pct": 0.0} for k, s in statuses.items()]}


def write(root: Path, rel: str, data) -> None:
    p = root / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(data)
    else:
        p.write_text(json.dumps(data), encoding="utf-8")


def run(root: Path, *extra) -> str:
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        qr.main(["--root", str(root), *extra])
    return out.getvalue()


BUGS = ("| ID | Статус | Компонент | Описание |\n|---|---|---|---|\n"
        "| [BUG-001](bugs/BUG-001-OPEN.md) | OPEN | layout/js (`crates/x.rs`) | строка\rс CR |\n"
        "| [BUG-002](bugs/BUG-002-FIXED.md) | FIXED 2026-01-01 | js | — |\n"
        "| [BUG-003](bugs/BUG-003-OPEN.md) | OPEN (DEBTOR) | paint | — |\n")


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def full_repo(self):
        write(self.root, "docs/wpt/runs/a.json", wpt_run("2026-10-01T10:00:00", 2, 3, 1, 1, 1.0))
        write(self.root, "docs/wpt/runs/b.json", wpt_run("2026-10-02T10:00:00", 2, 3, 1, 1, 0.0))
        partial = wpt_run("2026-10-03T10:00:00", 9, 0, 0, 0, 0.0)
        partial["scope"] = ["css"]
        write(self.root, "docs/wpt/runs/c-partial.json", partial)
        write(self.root, "graphic_tests/results/20261001-100000.json",
              gt_run("2026-10-01T10:00:00", {"01-a": "PASS", "02-b": "PASS", "03-c": "DEBTOR"}))
        write(self.root, "graphic_tests/results/20261002-100000.json",
              gt_run("2026-10-02T10:00:00", {"01-a": "PASS", "02-b": "FAIL", "03-c": "DEBTOR"}))
        write(self.root, "graphic_tests/results/latest.json", {"tests": []})
        write(self.root, "BUGS.md", BUGS)
        write(self.root, "tests/wpt/metadata/css/x/t.html.ini", "# BUG-001\n[t.html]\n  expected: TIMEOUT\n")
        write(self.root, "tests/wpt/metadata/css/x/u.html.ini", "# BUG-002 fixed\n[u.html]\n")


class EmptyRepo(Fixture):
    def test_no_sources_gives_na(self):
        out = run(self.root)
        self.assertIn("WPT n/a", out)
        text = (self.root / "docs/quality-report.md").read_text(encoding="utf-8")
        self.assertIn("graphic_tests n/a", text)
        self.assertIn("открытых багов n/a", text)

    def test_single_run_has_no_delta(self):
        write(self.root, "docs/wpt/runs/a.json", wpt_run("2026-10-01T10:00:00", 2, 3, 1, 1, 1.0))
        out = run(self.root, "--check")
        self.assertIn("(Δ n/a)", out.splitlines()[0])
        self.assertFalse((self.root / "docs/quality-report.md").exists(), "--check ничего не пишет")


class FullRepo(Fixture):
    def setUp(self):
        super().setUp()
        self.full_repo()

    def test_statuses_and_pass_numerator(self):
        snap = qr.build(self.root)
        st = snap["wpt"]["statuses"]
        self.assertEqual((st["PASS"], st["OK"], st["SKIPPED"], st["UNSUPPORTED"], st["NOT-RUN"]), (2, 3, 1, 1, 0))
        self.assertTrue(snap["wpt"]["status_sum_ok"])
        # рендеринг: PASS только из reftest/crashtest, SKIP/UNSUPPORTED в числитель не идут
        self.assertEqual(snap["wpt"]["rendering"]["pass"], 2)
        self.assertEqual(snap["wpt"]["run"], "b.json", "частичный прогон не берётся")
        self.assertEqual(snap["wpt_prev"]["run"], "a.json")

    def test_graphic_and_regressions(self):
        snap = qr.build(self.root)
        self.assertEqual(snap["graphic"]["counts"]["PASS"], 1)
        self.assertEqual(snap["graphic"]["summary_mismatch"], [])
        text, head = qr.render(snap, None)
        self.assertIn("graphic_tests 1/3 PASS (Δ -1)", head)
        self.assertIn("`02-b`: PASS → FAIL", text)
        self.assertIn("WPT `css`: балл", text)

    def test_bugs(self):
        snap = qr.build(self.root)
        bugs = snap["bugs"]
        self.assertEqual((bugs["open"], bugs["debtor"]), (2, 1))
        self.assertEqual(bugs["by_component"], {"js": 1, "layout": 1, "paint": 1})
        self.assertEqual([(b["bug"], b["blocked"]) for b in bugs["top5"]], [("BUG-001", 1)])

    def test_idempotent_and_bug_delta(self):
        run(self.root)
        first = (self.root / "docs/quality-report.md").read_bytes()
        run(self.root)
        self.assertEqual(first, (self.root / "docs/quality-report.md").read_bytes())
        self.assertEqual(len(list((self.root / "docs/quality/runs").glob("*.json"))), 1)
        write(self.root, "BUGS.md", BUGS.replace("| FIXED 2026-01-01 |", "| OPEN |"))
        out = run(self.root)
        self.assertIn("открытых багов 3 (Δ +1)", out)
        self.assertEqual(len(list((self.root / "docs/quality/runs").glob("*.json"))), 2)
        again = run(self.root)
        self.assertIn("открытых багов 3 (Δ +1)", again, "повтор считает дельту к тому же прошлому срезу")
        self.assertIn("без изменений", again)


class Helpers(unittest.TestCase):
    def test_components(self):
        self.assertEqual(qr.components("css-parser + layout"), ["css-parser", "layout"])
        self.assertEqual(qr.components("layout (`crates/a/b.rs`)"), ["layout"])
        self.assertEqual(qr.components(""), ["(не указан)"])

    def test_is_test_path(self):
        self.assertTrue(qr.is_test_path("/css/x/a.html"))
        self.assertFalse(qr.is_test_path("/css/x/a-ref.html"))
        self.assertFalse(qr.is_test_path("/css/x/support/a.html"))
        self.assertFalse(qr.is_test_path("/css/x/a.png"))


if __name__ == "__main__":
    unittest.main()
