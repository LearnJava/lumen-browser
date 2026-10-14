"""Тесты чистых функций краулера паритета: `python scripts/test_compat_crawl.py`."""

from __future__ import annotations

import sys
import tempfile
import unittest
import zlib
import struct
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compat_crawl as cc  # noqa: E402


def make_png(w: int, h: int, rgb_at) -> bytes:
    """RGB-PNG без фильтров: rgb_at(x, y) -> (r, g, b)."""
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            raw += bytes(rgb_at(x, y))

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b""))


ARGS = SimpleNamespace(px_threshold=10.0, px_tolerance=32, ax_threshold=0.95)


class StatusAndWalls(unittest.TestCase):
    def test_status_class(self):
        self.assertTrue(cc.check_status({"http_status": 200}, {"http_status": 204})["ok"])
        self.assertFalse(cc.check_status({"http_status": 200}, {"http_status": 403})["ok"])
        self.assertTrue(cc.check_status({"http_status": 403}, {"http_status": 451})["ok"])
        self.assertFalse(cc.check_status({"http_status": 200}, {"http_status": None})["ok"])

    def test_wall_hits_from_shared_classifier(self):
        hard = cc.wall_hits({"http_status": 403, "text": "Checking if the site connection is secure"})
        self.assertTrue(any(h.startswith("hard:") for h in hard), hard)
        soft = cc.wall_hits({"http_status": 200, "text": "Your browser is not supported. Please upgrade your browser."})
        self.assertTrue(any(h.startswith("soft:") for h in soft), soft)
        self.assertEqual(cc.wall_hits({"http_status": 200, "text": "Обычная страница"}), set())

    def test_wall_only_extra_counts(self):
        wall = {"http_status": 403, "text": "Checking if the site connection is secure"}
        self.assertTrue(cc.check_walls(wall, wall)["ok"])
        r = cc.check_walls({"http_status": 200, "text": "Welcome " * 300}, wall)
        self.assertFalse(r["ok"])
        self.assertTrue(r["extra"])

    def test_near_empty_page(self):
        r = cc.check_walls({"text": "word " * 400}, {"text": "  "})
        self.assertEqual(r["extra"], ["blank:near-empty"])
        self.assertTrue(cc.check_walls({"text": "short"}, {"text": ""})["ok"])


class Exceptions(unittest.TestCase):
    def test_sig_normalisation(self):
        a = cc.exc_sig("Uncaught TypeError: x is not a function at https://a.com/app.js:10:5")
        b = cc.exc_sig("[js] TypeError: x is not a function at https://b.org/z.js:99:1")
        self.assertEqual(a, b)
        self.assertEqual(cc.exc_sig("Uncaught (in promise) Error: boom 42"), cc.exc_sig("Error: boom 7"))

    def test_new_only(self):
        ch = {"exceptions": ["TypeError: a is not a function"]}
        self.assertTrue(cc.check_exceptions(ch, {"exceptions": ["Uncaught TypeError: a is not a function"]})["ok"])
        r = cc.check_exceptions(ch, {"exceptions": ["ReferenceError: q is not defined"]})
        self.assertFalse(r["ok"])
        self.assertEqual(len(r["new_sigs"]), 1)
        self.assertTrue(cc.check_exceptions({}, {})["ok"])


class A11y(unittest.TestCase):
    def test_flatten_lumen_skips_hidden_and_non_interactive(self):
        tree = {"role": "generic", "name": "", "children": [
            {"role": "link", "name": " Новости  дня ", "children": []},
            {"role": "heading", "name": "H", "children": []},
            {"role": "button", "name": "Скрытая", "state": {"hidden": True}, "children": [
                {"role": "link", "name": "внутри", "children": []}]},
        ]}
        self.assertEqual(cc.flatten_lumen_ax(tree), [("link", "новости дня")])

    def test_flatten_chrome_skips_ignored(self):
        nodes = [{"role": {"value": "button"}, "name": {"value": "OK"}},
                 {"role": {"value": "button"}, "name": {"value": "x"}, "ignored": True},
                 {"role": {"value": "StaticText"}, "name": {"value": "t"}}]
        self.assertEqual(cc.flatten_chrome_ax(nodes), [("button", "ok")])

    def test_coverage_threshold(self):
        chrome = [("link", f"l{i}") for i in range(20)]
        lumen = chrome[:19] + [("link", "extra")]
        self.assertTrue(cc.check_a11y(chrome, lumen, 0.95)["ok"])  # 19/20 = 0.95
        self.assertFalse(cc.check_a11y(chrome, chrome[:18], 0.95)["ok"])
        self.assertTrue(cc.check_a11y([], [("link", "a")], 0.95)["ok"])  # у эталона пусто — нечего терять
        self.assertFalse(cc.check_a11y(chrome, None, 0.95)["ok"])

    def test_multiset_matching(self):
        r = cc.ax_coverage([("link", "a"), ("link", "a")], [("link", "a")])
        self.assertEqual((r["matched"], r["coverage"]), (1, 0.5))
        self.assertEqual(r["missing"], [["link", "a", 1]])


class Pixels(unittest.TestCase):
    def frame(self, fn, w=64, h=64):
        png = make_png(w, h, fn)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "f.png"
            p.write_bytes(png)
            return cc.load_frame(str(p))

    def test_identical_and_changed(self):
        white = self.frame(lambda x, y: (255, 255, 255))
        self.assertEqual(cc.frame_diff(white, white, [], 1.0, 32)["differing"], 0)
        # правая половина чёрная -> 8 из 16 блоков
        half = self.frame(lambda x, y: (0, 0, 0) if x >= 32 else (255, 255, 255))
        d = cc.frame_diff(white, half, [], 1.0, 32)
        self.assertEqual((d["blocks"], d["differing"], d["diff_pct"]), (16, 8, 50.0))

    def test_mask_removes_blocks(self):
        white = self.frame(lambda x, y: (255, 255, 255))
        half = self.frame(lambda x, y: (0, 0, 0) if x >= 32 else (255, 255, 255))
        d = cc.frame_diff(white, half, [[32, 0, 32, 64]], 1.0, 32)
        self.assertEqual((d["masked"], d["differing"], d["compared"]), (8, 0, 8))

    def test_mask_scale(self):
        # DPR 2: маска в CSS-пикселях [16,0,16,16] -> блок (2,0) кадра
        self.assertEqual(cc.mask_blocks([[16, 0, 8, 8]], 2.0, 4, 4), {(2, 0)})

    def test_tolerance(self):
        a = self.frame(lambda x, y: (100, 100, 100))
        b = self.frame(lambda x, y: (120, 100, 100))
        self.assertEqual(cc.frame_diff(a, b, [], 1.0, 32)["differing"], 0)
        self.assertEqual(cc.frame_diff(a, b, [], 1.0, 10)["differing"], 16)

    def test_check_pixels_threshold_and_missing(self):
        white = self.frame(lambda x, y: (255, 255, 255))
        self.assertTrue(cc.check_pixels(white, white, [], 1.0, 10.0, 32)["ok"])
        self.assertFalse(cc.check_pixels(None, white, [], 1.0, 10.0, 32)["ok"])


class Aggregation(unittest.TestCase):
    def checks(self, **fails):
        return {k: {"ok": k not in fails} for k in cc.CHECKS}

    def test_combine_fail_only_if_both(self):
        final, flaps = cc.combine_runs(self.checks(pixels=1), self.checks())
        self.assertTrue(final["pixels"]["ok"])
        self.assertEqual(flaps, ["pixels"])
        final, flaps = cc.combine_runs(self.checks(pixels=1), self.checks(pixels=1))
        self.assertFalse(final["pixels"]["ok"])
        self.assertEqual(flaps, [])
        final, flaps = cc.combine_runs(self.checks(), None)
        self.assertTrue(all(c["ok"] for c in final.values()))

    def test_summary(self):
        recs = [
            {"computed": True, "checks": self.checks(), "flaps": []},
            {"computed": True, "checks": self.checks(a11y=1), "flaps": ["pixels"], "reran": True},
            {"computed": False, "ref_failed": True},
            {"computed": True, "checks": self.checks(), "flaps": []},
        ]
        s = cc.summarize(recs)
        self.assertEqual((s["sites"], s["computed"], s["parity"], s["ref_failed"]), (4, 3, 2, 1))
        self.assertEqual(s["cpr_pct"], 66.7)
        self.assertEqual((s["flapped"], s["flap_pct"], s["reran"]), (1, 33.3, 1))
        self.assertEqual(s["per_check_pass"]["a11y"], 2)

    def test_evaluate_end_to_end(self):
        snap = {"text": "hello " * 400, "http_status": 200, "exceptions": [], "ax": [("link", "a")],
                "frame": None, "masks": []}
        res = cc.evaluate(dict(snap), dict(snap), ARGS)
        self.assertEqual(set(res), set(cc.CHECKS))
        self.assertFalse(res["pixels"]["ok"])  # нет кадра -> не паритет
        self.assertTrue(res["status"]["ok"] and res["walls"]["ok"] and res["exceptions"]["ok"] and res["a11y"]["ok"])


if __name__ == "__main__":
    unittest.main()
