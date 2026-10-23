"""Тесты сводного отчёта качества: `python scripts/test_quality_report.py`."""

from __future__ import annotations

import contextlib
import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import quality_report as qr  # noqa: E402


def wpt_run(finished: str, pass_n: int, ok: int, skip: int, no_exec: int, css_score: float,
            commit: str = "abc1234") -> dict:
    ids = pass_n + ok + skip + no_exec
    return {
        "commit": commit, "finished": finished, "scope": "full-corpus", "processes": 4,
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


def gt_run(stamp: str, statuses: dict, capture=None, commit: str = "def5678", subject: str = "") -> dict:
    """capture — способ съёмки всех тестов (str) или {stem: способ}; None — прогон без поля (legacy)."""
    key = {"PASS": "passed", "FAIL": "failed", "ERROR": "errors", "DEBTOR": "debtors", "SKIP": "skipped"}
    summary = {v: 0 for v in key.values()}
    for s in statuses.values():
        summary[key[s]] += 1
    summary["total"] = len(statuses)
    tests = [{"id": k[:2], "stem": k, "status": s, "diff_pct": 0.0} for k, s in statuses.items()]
    if capture is not None:
        for t in tests:
            t["capture"] = capture if isinstance(capture, str) else capture[t["stem"]]
    return {"timestamp": stamp, "git": {"commit": commit, "branch": "main", "subject": subject},
            "summary": summary, "tests": tests}


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
        write(self.root, "docs/wpt/runs/b.json", wpt_run("2026-10-02T10:00:00", 2, 3, 1, 1, 0.0, "fff9999"))
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
        self.assertIn("просадок 2", head, "категория WPT + PASS → FAIL, коммиты разные")
        self.assertNotIn("## Разброс прогонов", text)
        self.assertEqual(snap["lag"], {"wpt": "n/a", "graphic": "n/a"}, "вне git-репозитория — n/a")

    def test_same_commit_is_noise_not_regression(self):
        b = wpt_run("2026-10-02T10:00:00", 2, 3, 1, 1, 0.0)
        b["processes"] = 8
        write(self.root, "docs/wpt/runs/b.json", b)
        text, head = qr.render(qr.build(self.root), None)
        self.assertIn("просадок 1", head, "в просадках только graphic_tests")
        self.assertIn("тот же коммит — разброс", head)
        noise = text.split("## Разброс прогонов", 1)[1]
        self.assertIn("WPT `css`: балл", noise)
        self.assertIn("`processes` 4 → 8", text)

    def test_derived_snapshot_replaces_its_base(self):
        c = wpt_run("2026-10-02T12:00:00", 3, 2, 1, 1, 0.0, "fff9999")
        c["derived_from"] = "b.json"
        write(self.root, "docs/wpt/runs/b-popups.json", c)
        snap = qr.build(self.root)
        self.assertEqual(snap["wpt"]["run"], "b-popups.json")
        self.assertEqual(snap["wpt_prev"]["run"], "a.json", "исходник составного снимка в пару не идёт")

    def test_reclass_noted(self):
        write(self.root, "graphic_tests/results/20261002-100000.json",
              gt_run("2026-10-02T10:00:00", {"01-a": "PASS", "02-b": "PASS", "03-c": "FAIL"}))
        text, head = qr.render(qr.build(self.root), None)
        self.assertIn("Из них FAIL ↔ DEBTOR: 1", text)
        self.assertIn("просадок 1", head, "DEBTOR → FAIL — не просадка, остаётся категория WPT")

    def test_stale_sources_warned(self):
        write(self.root, "graphic_tests/results/20261020-100000.json",
              gt_run("2026-10-20T10:00:00", {"01-a": "PASS", "02-b": "FAIL", "03-c": "DEBTOR"}))
        text, head = qr.render(qr.build(self.root), None)
        self.assertIn("WPT старше соседнего источника на 18 дн.", head)
        self.assertIn("разнесены на 18 дн.", text)

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


def stems(status: str = "PASS") -> dict:
    return {"01-a": status, "02-b": "PASS", "03-c": "DEBTOR"}


class GraphicModes(Fixture):
    """TEST-12: прогоны разных способов съёмки не сравниваются."""

    def put(self, name: str, stamp: str, statuses: dict, capture, **kw) -> None:
        write(self.root, f"graphic_tests/results/{name}.json", gt_run(stamp, statuses, capture, **kw))

    def test_mode(self):
        self.assertEqual(qr.graphic_mode(gt_run("t", stems(), "ipc")), "ipc")
        self.assertEqual(qr.graphic_mode(gt_run("t", stems(), "gdigrab")), "gdigrab")
        self.assertEqual(qr.graphic_mode(gt_run("t", stems())), "legacy")
        self.assertEqual(qr.graphic_mode(gt_run("t", stems(), {"01-a": "gdigrab", "02-b": "ipc", "03-c": "ipc"})), "default")

    def test_pair_skips_other_mode(self):
        mixed = {"01-a": "gdigrab", "02-b": "ipc", "03-c": "ipc"}
        self.put("20261001-100000", "2026-10-01T10:00:00", stems(), mixed)
        self.put("20261002-100000", "2026-10-02T10:00:00", stems("FAIL"), "ipc")
        self.put("20261003-100000", "2026-10-03T10:00:00", stems(), mixed)
        snap = qr.build(self.root)
        self.assertEqual((snap["graphic"]["run"], snap["graphic_prev"]["run"]),
                         ("20261003-100000.json", "20261001-100000.json"), "ipc-прогон между ними пропущен")
        text, _ = qr.render(snap, None)
        self.assertIn("режим `default`", text)
        self.assertNotIn("20261002-100000", text)

    def test_no_comparable_run(self):
        self.put("20261001-100000", "2026-10-01T10:00:00", stems(), "ipc")
        self.put("20261002-100000", "2026-10-02T10:00:00", stems("FAIL"), "gdigrab")
        snap = qr.build(self.root)
        self.assertIsNone(snap["graphic_prev"])
        text, head = qr.render(snap, None)
        self.assertIn("graphic_tests 1/3 PASS (Δ n/a)", head)
        self.assertIn("нет более раннего полного прогона режима `gdigrab`", text)
        self.assertIn("Сменившие статус: n/a", text)
        self.assertNotIn("`01-a`: PASS → FAIL", text)

    def test_capture_change_is_not_a_regression(self):
        before = {"01-a": "ipc", "02-b": "ipc", "03-c": "ipc"}
        after = {"01-a": "gdigrab", "02-b": "ipc", "03-c": "ipc"}
        cur = qr.graphic_summary("b", gt_run("t", {"01-a": "DEBTOR", "02-b": "FAIL", "03-c": "DEBTOR"}, after))
        prev = qr.graphic_summary("a", gt_run("t", {"01-a": "PASS", "02-b": "PASS", "03-c": "DEBTOR"}, before))
        changes = qr.graphic_changes(cur, prev)
        self.assertEqual([c[0] for c in changes], ["02-b"], "01-a сменил способ съёмки — не в «сменивших статус»")
        self.assertEqual(qr.graphic_capture_changes(cur, prev), [("01-a", "ipc", "gdigrab")])
        self.assertEqual([c for c in changes if qr.graphic_is_regression(c)][0][0], "02-b")

    def test_capture_change_listed_in_report(self):
        a = {"01-a": "gdigrab", "02-b": "ipc", "03-c": "ipc"}
        c = {"01-a": "gdigrab", "02-b": "gdigrab", "03-c": "ipc"}
        self.put("20261001-100000", "2026-10-01T10:00:00", stems(), a)
        self.put("20261002-100000", "2026-10-02T10:00:00", {"01-a": "FAIL", "02-b": "PASS", "03-c": "DEBTOR"}, c)
        text, _ = qr.render(qr.build(self.root), None)
        self.assertIn("Сменился способ съёмки: 1 — `02-b` (ipc → gdigrab)", text)
        self.assertIn("`01-a`: PASS → FAIL", text, "тест с тем же способом съёмки остаётся просадкой")

    def test_origin_marks(self):
        self.assertEqual(qr.origin_mark({"in_main": False, "wip": True}), "не в main / WIP")
        self.assertEqual(qr.origin_mark({"in_main": False, "wip": False}), "не в main")
        self.assertEqual(qr.origin_mark({"in_main": True, "wip": True}), "WIP")
        self.assertEqual(qr.origin_mark({"in_main": None, "wip": False}), "")
        self.assertEqual(qr.git_origin(None, {"commit": "x", "subject": "WIP: что-то"}), {"in_main": None, "wip": True})
        self.assertEqual(qr.git_origin(self.root, {"commit": "deadbeef"})["in_main"], None, "не git-репозиторий")

    @unittest.skipUnless(shutil.which("git"), "нужен git")
    def test_in_main_via_git(self):
        def git(*args):
            return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=self.root,
                                  capture_output=True, text=True, check=True).stdout.strip()
        git("init", "-q", "-b", "main")
        (self.root / "f.txt").write_text("1", encoding="utf-8")
        git("add", "f.txt")
        git("commit", "-q", "-m", "first")
        merged = git("rev-parse", "HEAD")
        git("checkout", "-q", "-b", "side")
        (self.root / "f.txt").write_text("2", encoding="utf-8")
        git("commit", "-q", "-am", "WIP: side")
        side = git("rev-parse", "HEAD")
        self.assertTrue(qr.git_origin(self.root, {"commit": merged})["in_main"])
        self.assertFalse(qr.git_origin(self.root, {"commit": side})["in_main"])
        self.assertEqual(qr.commit_lag(self.root, merged), "0 коммитов")
        self.assertEqual(qr.commit_lag(self.root, side), "вне main")
        self.put("20261001-100000", "2026-10-01T10:00:00", stems(), "ipc", commit=merged)
        self.put("20261002-100000", "2026-10-02T10:00:00", stems(), "ipc", commit=side, subject="WIP: side")
        text, _ = qr.render(qr.build(self.root), None)
        self.assertIn("**не в main / WIP**", text)

    def test_snapshot_ignores_git_state(self):
        self.put("20261001-100000", "2026-10-01T10:00:00", stems(), "ipc")
        payload = qr.snapshot_payload(qr.build(self.root))
        self.assertNotIn("origin", payload["graphic"])
        self.assertEqual(payload["graphic"]["mode"], "ipc")


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
