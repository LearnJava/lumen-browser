"""Тесты логики сравнения geometry/computed-styles diff: `python scripts/test_geom_diff.py`."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import geom_diff as gd  # noqa: E402

STYLE = ["block", "static", "16px", "Arial, sans-serif", "visible", "visible"]


def node(key: str, x: float, y: float, w: float, h: float, ord_: list[int], style: list[str] | None = None) -> dict:
    return {"k": key, "o": ord_, "x": x, "y": y, "w": w, "h": h, "s": style or STYLE}


def snap(nodes: list[dict], hidden: list[str] = (), extra_all: list[list] = (), truncated: bool = False) -> dict:
    return {
        "nodes": nodes, "hidden": list(hidden), "truncated": truncated,
        "all": [[n["k"], n["x"], n["y"], n["w"], n["h"]] for n in nodes] + list(extra_all),
    }


class NormStyle(unittest.TestCase):
    def test_font_size_and_family_and_overflow(self):
        self.assertEqual(
            gd.norm_style(["block", "static", "13.333333px", '"Fira Sans", serif', "visible", "hidden hidden"]),
            ("block", "static", "13.3px", "fira sans", "visible", "hidden"),
        )
        self.assertEqual(gd.norm_style(STYLE)[2], "16px")
        self.assertEqual(gd.norm_style(STYLE[:5] + ["hidden auto"])[5], "hidden auto")


class Compare(unittest.TestCase):
    def test_identical_is_clean(self):
        nodes = [node("body", 0, 0, 1280, 100, []), node("body>p:1", 8, 8, 100, 20, [0])]
        cmp = gd.compare(snap(nodes), snap([dict(n) for n in nodes]), 4)
        self.assertEqual((cmp["matched"], cmp["compared"], cmp["matched_pct"]), (2, 2, 100.0))
        self.assertTrue(gd.clean(cmp))

    def test_threshold_is_exclusive(self):
        c = snap([node("body", 0, 0, 100, 100, [])])
        self.assertEqual(gd.compare(c, snap([node("body", 4, 0, 100, 100, [])]), 4)["geom"], 0)
        self.assertEqual(gd.compare(c, snap([node("body", 4.5, 0, 100, 100, [])]), 4)["geom"], 1)

    def test_children_of_shifted_parent_are_not_roots(self):
        c = snap([node("body", 0, 0, 1000, 100, []), node("body>div:1", 400, 0, 200, 50, [0]),
                  node("body>div:1>p:1", 400, 0, 200, 20, [0, 0])])
        lu = snap([node("body", 0, 0, 1000, 100, []), node("body>div:1", 0, 0, 200, 50, [0]),
                   node("body>div:1>p:1", 0, 0, 200, 20, [0, 0])])
        cmp = gd.compare(c, lu, 4)
        self.assertEqual(cmp["geom"], 2)
        self.assertEqual([g["k"] for g in cmp["geom_roots"]], ["body>div:1"])

    def test_sibling_pushed_down_by_taller_sibling_is_not_root(self):
        c = snap([node("body", 0, 0, 100, 60, []), node("body>p:1", 0, 0, 100, 20, [0]),
                  node("body>p:2", 0, 20, 100, 20, [1]), node("body>p:3", 0, 40, 100, 20, [2])])
        lu = snap([node("body", 0, 0, 100, 90, []), node("body>p:1", 0, 0, 100, 50, [0]),
                   node("body>p:2", 0, 50, 100, 20, [1]), node("body>p:3", 0, 70, 100, 20, [2])])
        cmp = gd.compare(c, lu, 4)
        self.assertEqual([g["k"] for g in cmp["geom_roots"]], ["body>p:1"])

    def test_zero_rect_is_own_class(self):
        c = snap([node("body", 0, 0, 100, 100, []), node("body>a:1", 10, 10, 50, 16, [0], ["inline"] + STYLE[1:])])
        lu = snap([node("body", 0, 0, 100, 100, []), node("body>a:1", 0, 0, 0, 0, [0], ["inline"] + STYLE[1:])])
        cmp = gd.compare(c, lu, 4)
        self.assertEqual((cmp["geom"], cmp["lumen_zero_rect"], cmp["lumen_zero_tags"]), (0, 1, [("a", 1)]))

    def test_style_mismatch_grouped_by_property(self):
        serif = STYLE[:3] + ["serif"] + STYLE[4:]
        c = snap([node("body>td:1", 0, 0, 10, 10, [0]), node("body>td:2", 10, 0, 10, 10, [1])])
        lu = snap([node("body>td:1", 0, 0, 10, 10, [0], serif), node("body>td:2", 10, 0, 10, 10, [1], serif)])
        cmp = gd.compare(c, lu, 4)
        self.assertEqual(cmp["style"], 2)
        self.assertEqual(cmp["style_by_prop"], [(("font-family", "arial", "serif"), 2, "td×2")])

    def test_missing_split_by_reason(self):
        c = snap([node("body", 0, 0, 100, 100, []), node("body>div:1", 0, 0, 10, 10, [0]),
                  node("body>div:1>span:1", 0, 0, 5, 5, [0, 0]), node("body>nav:1", 0, 10, 10, 10, [1]),
                  node("body>aside:1", 0, 20, 10, 10, [2])])
        lu = snap([node("body", 0, 0, 100, 100, [])], hidden=["body>nav:1"])
        cmp = gd.compare(c, lu, 4)
        why = {m["k"]: (m["why"], m["size"]) for m in cmp["missing_in_lumen"]}
        self.assertEqual(why, {"body>div:1": ("нет в DOM", 2), "body>nav:1": ("display:none", 1),
                               "body>aside:1": ("нет в DOM", 1)})
        self.assertEqual(cmp["missing_total"], 4)

    def test_node_outside_region_is_geometry_not_missing(self):
        c = snap([node("body", 0, 0, 100, 100, []), node("body>p:1", 0, 50, 100, 20, [0])])
        lu = snap([node("body", 0, 0, 100, 100, [])], extra_all=[["body>p:1", 0, 3000, 100, 20]])
        cmp = gd.compare(c, lu, 4)
        self.assertEqual((cmp["missing_total"], cmp["geom"]), (0, 1))
        self.assertEqual(cmp["geom_roots"][0]["outside"], "lumen")
        self.assertEqual(cmp["geom_roots"][0]["d"], (0, 2950, 0, 0))

    def test_beyond_other_limit_is_not_counted(self):
        c = snap([node("body", 0, 0, 100, 100, []), node("body>p:1", 0, 0, 10, 10, [0]),
                  node("body>p:2", 0, 10, 10, 10, [1])])
        lu = snap([node("body", 0, 0, 100, 100, []), node("body>p:1", 0, 0, 10, 10, [0])],
                  extra_all=[["body>p:2", 0, 10, 10, 10]], truncated=True)
        cmp = gd.compare(c, lu, 4)
        self.assertEqual((cmp["beyond_limit"], cmp["compared"], cmp["matched_pct"]), (1, 2, 100.0))


def tree(entries: list[tuple[str, str]], hidden: list[tuple[str, str]] = ()) -> dict:
    """Снимок нового формата из `(ключ, подпись)` в порядке документа; индекс — номер среди соседей."""
    idx: dict[str | None, int] = {}
    nodes, all_ = [], []
    for k, sig in entries:
        p = gd.parent_key(k)
        idx[p] = idx.get(p, -1) + 1
        nodes.append(node(k, 0, len(nodes) * 10, 10, 10, [idx[p]]))
        all_.append([k, 0, len(all_) * 10, 10, 10, idx[p], sig])
    hid = []
    for k, sig in hidden:
        p = gd.parent_key(k)
        idx[p] = idx.get(p, -1) + 1
        hid.append([k, idx[p], sig])
    return {"nodes": nodes, "all": all_, "hidden": hid, "truncated": False}


class Align(unittest.TestCase):
    def test_extra_sibling_does_not_shift_keys(self):
        c = tree([("body", "body"), ("body>div:1", "div.banner"), ("body>div:2", "div.app"),
                  ("body>div:2>p:1", "p")])
        lu = tree([("body", "body"), ("body>div:1", "div.app"), ("body>div:1>p:1", "p")])
        m = gd.align_keys(c, lu)
        self.assertEqual(m["body>div:1"], "body>div:2")
        self.assertEqual(m["body>div:1>p:1"], "body>div:2>p:1")
        cmp = gd.compare(c, lu, 1000)
        self.assertEqual((cmp["matched"], [x["k"] for x in cmp["missing_in_lumen"]]), (3, ["body>div:1"]))

    def test_class_change_falls_back_to_tag(self):
        c = tree([("body", "body"), ("body>nav:1", "nav.open")])
        lu = tree([("body", "body"), ("body>nav:1", "nav.closed")])
        self.assertEqual(gd.align_keys(c, lu)["body>nav:1"], "body>nav:1")

    def test_unmatched_lumen_node_gets_unique_key(self):
        c = tree([("body", "body"), ("body>div:1", "div.a")])
        lu = tree([("body", "body"), ("body>span:1", "span"), ("body>span:1>i:1", "i"), ("body>div:1", "div.a")])
        m = gd.align_keys(c, lu)
        self.assertEqual((m["body>span:1"], m["body>span:1>i:1"]), ("body>span:1~l", "body>span:1~l>i:1~l"))
        self.assertEqual(m["body>div:1"], "body>div:1")

    def test_hidden_node_is_aligned(self):
        c = tree([("body", "body"), ("body>div:2", "div.b")], hidden=[("body>div:1", "div.x")])
        lu = tree([("body", "body"), ("body>div:1", "div.b")])
        self.assertEqual(gd.align_keys(c, lu)["body>div:1"], "body>div:2")

    def test_old_format_is_left_alone(self):
        self.assertIsNone(gd.align_keys(snap([node("body", 0, 0, 1, 1, [])]), snap([node("body", 0, 0, 1, 1, [])])))

    def test_same_site(self):
        self.assertTrue(gd.same_site("https://www.microsoft.com/de-de", "https://www.microsoft.com/"))
        self.assertFalse(gd.same_site("https://www.apple.com/", "https://www.microsoft.com/"))
        self.assertFalse(gd.same_site("about:blank", "https://example.com/"))


class Report(unittest.TestCase):
    def test_summary_line_counts_clean_and_failed(self):
        ok = gd.compare(snap([node("body", 0, 0, 1, 1, [])]), snap([node("body", 0, 0, 1, 1, [])]), 4)
        bad = gd.compare(snap([node("body", 0, 0, 1, 1, [])]), snap([node("body", 9, 0, 1, 1, [])]), 4)
        results = [{"cmp": ok}, {"cmp": bad}, {"cmp": None}]
        self.assertEqual(gd.summary_line(results),
                         "Итог: 1 сайтов из 2 без расхождений (несопоставимы: 0, не сняты: 1, всего 3)")

    def test_different_pages_are_incomparable_and_out_of_total(self):
        c = snap([node("body", 0, 0, 1, 1, [])] + [node(f"body>p:{i}", 0, i, 1, 1, [i]) for i in range(1, 10)])
        c["title"] = "Stack Overflow"
        lu = snap([node("body", 0, 0, 1, 1, []), node("body>div:1", 0, 0, 1, 1, [0])])
        lu["title"] = "Just a moment..."
        cmp = gd.compare(c, lu, 4)
        self.assertTrue(cmp["incomparable"])
        self.assertEqual(gd.summary_line([{"cmp": cmp}]),
                         "Итог: 0 сайтов из 0 без расхождений (несопоставимы: 1, не сняты: 0, всего 1)")
        lu["title"] = "  Stack\n  Overflow "  # тот же заголовок с несхлопнутыми пробелами — сравнимы
        self.assertFalse(gd.compare(c, lu, 4)["incomparable"])

    def test_triage_block_survives_regeneration(self):
        meta = {"date": "2026-10-21", "lumen": "l", "lumen_mtime": "t", "chrome": "c", "limit": 500}
        first = gd.report_md([], meta, 4)
        self.assertIn(gd.TRIAGE_STUB, first)  # заглушка, пока триажа нет
        manual = first.replace("_Не заполнен", "BUG-1: ручная запись\n_Не заполнен")
        kept = gd.extract_triage(manual)
        self.assertIn("BUG-1: ручная запись", kept)
        self.assertIn(kept, gd.report_md([], meta, 4, kept))
        self.assertIsNone(gd.extract_triage("# отчёт без блока"))

    def test_stale_children_is_not_clean(self):
        c = snap([node("body", 0, 0, 1, 1, [])])
        lu = snap([node("body", 0, 0, 1, 1, [])])
        lu["stale_children"] = [["body", 0, 1]]
        cmp = gd.compare(c, lu, 4)
        self.assertEqual(cmp["stale_children"], {"chrome": [], "lumen": [["body", 0, 1]]})
        self.assertFalse(gd.clean(cmp))


if __name__ == "__main__":
    unittest.main()
