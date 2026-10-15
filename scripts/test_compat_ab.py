"""Тесты чистых функций плеча Lumen и свода A/B 2×2: `python scripts/test_compat_ab.py`."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compat_ab_lumen as al  # noqa: E402
import compat_ab_matrix as am  # noqa: E402


class ParseStderr(unittest.TestCase):
    def test_redirect_takes_last_response(self):
        err = "→ GET http://a/r\n← 302 http://a/r\n→ GET http://a/x\n← 403 http://a/x\nПолучено 45 байт\n"
        r = al.parse_stderr(err)
        self.assertEqual((r["http_status"], r["final_url"], r["redirects"], r["net_error"]), (403, "http://a/x", 1, None))

    def test_network_failure(self):
        err = "→ GET https://x/\n✗ https://x/ (tls: TLS handshake: certificate expired)\nОшибка dump\n"
        r = al.parse_stderr(err)
        self.assertIsNone(r["http_status"])
        self.assertEqual(r["net_error"], "tls: TLS handshake: certificate expired")

    def test_cached_answer_has_no_events(self):
        # дисковый кэш Lumen: ни «→», ни «←» — именно поэтому у каждого запуска свой каталог данных
        r = al.parse_stderr("Получено 577 байт\nКодировка: utf-8\n")
        self.assertIsNone(r["http_status"])


class Classify(unittest.TestCase):
    NONE = {"kind": "none", "vendor": None, "signals": []}

    def test_statuses(self):
        c = al.classify
        self.assertEqual(c({"http_status": 200, "body_bytes": 10}, self.NONE), "OK")
        self.assertEqual(c({"http_status": 200, "body_bytes": 0}, self.NONE), "EMPTY")
        self.assertEqual(c({"http_status": 403, "body_bytes": 10}, self.NONE), "SITE_REFUSED")
        self.assertEqual(c({"http_status": 404, "body_bytes": 10}, self.NONE), "HTTP_ERROR")
        self.assertEqual(c({"http_status": None, "net_error": "dns: x"}, self.NONE), "NET_FAIL")
        self.assertEqual(c({"http_status": None}, self.NONE), "TIMEOUT")
        self.assertEqual(c({"http_status": 200, "body_bytes": 10}, {"kind": "soft", "vendor": "unsupported-browser", "signals": []}), "WALL_SOFT")


def rec(slug: str, variant: str, status: str, http: int | None = 200) -> dict:
    return {"slug": slug, "variant": variant, "status": status, "http_status": http, "wall": {"kind": "none"}}


def write_results(path: Path, records: list[dict]) -> Path:
    path.write_text(json.dumps({"meta": {}, "results": records}), encoding="utf-8")
    return path


class Matrix(unittest.TestCase):
    def cells(self, table: dict[str, tuple[str, str, str, str]]):
        """slug → (chrome/plain, chrome/lumen, lumen/plain, lumen/lumen)."""
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            ch, lu = [], []
            for slug, (cp, cl, lp, ll) in table.items():
                ch += [rec(slug, "plain", cp), rec(slug, "lumen", cl)]
                lu += [rec(slug, "plain", lp), rec(slug, "lumen", ll)]
            return am.load_cells({"chrome": [write_results(d / "c.json", ch)], "lumen": [write_results(d / "l.json", lu)]})

    def test_token_and_stack_attribution(self):
        cells = self.cells({
            "ok": ("OK", "OK", "OK", "OK"),
            "token": ("OK", "OK", "OK", "SITE_REFUSED"),       # Lumen с токеном заблокирован
            "stack": ("OK", "OK", "SITE_REFUSED", "SITE_REFUSED"),  # заблокирован стек, токен ни при чём
            "both": ("SITE_REFUSED", "SITE_REFUSED", "SITE_REFUSED", "SITE_REFUSED"),
            "dead": ("NET_FAIL", "NET_FAIL", "TIMEOUT", "NET_FAIL"),
        })
        res = am.analyse(cells, {"token": 3, "stack": 2})
        self.assertEqual(res["token"]["lumen"], ["token"])
        self.assertEqual(res["stack"], ["stack"])
        self.assertEqual(res["both_block"], ["both"])
        self.assertEqual(res["dead"], ["dead"])
        self.assertEqual(len(res["measured"]), 4)
        self.assertEqual(res["token_must"], ["token"])
        self.assertEqual(res["verdict_r1"], "оставить chrome_unbranded")  # must-work заблокирован токеном

    def test_thresholds_pass(self):
        table = {f"s{i}": ("OK", "OK", "OK", "OK") for i in range(300)}
        res = am.analyse(self.cells(table), {})
        self.assertEqual((res["token_pct"], res["stack_pct"]), (0.0, 0.0))
        self.assertEqual(res["verdict_r1"], "переход на chrome_lumen допустим")
        self.assertEqual(res["verdict_r4"], "TLS/H2 не трогать")

    def test_majority_of_reruns_wins(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            first = write_results(d / "1.json", [rec("a", "plain", "SITE_REFUSED", 403)])
            re1 = write_results(d / "2.json", [rec("a", "plain", "OK")])
            re2 = write_results(d / "3.json", [rec("a", "plain", "OK")])
            cells = am.load_cells({"chrome": [first, re1, re2], "lumen": []})
            self.assertEqual(cells[("chrome", "plain", "a")]["status"], "OK")
            self.assertEqual(cells[("chrome", "plain", "a")]["runs"], 3)
            tie = am.load_cells({"chrome": [first, re1], "lumen": []})
            self.assertEqual(tie[("chrome", "plain", "a")]["status"], "OK")  # ничья — поздняя

    def test_incomplete_site_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            ch = write_results(d / "c.json", [rec("a", "plain", "OK"), rec("a", "lumen", "OK")])
            lu = write_results(d / "l.json", [rec("a", "plain", "OK")])  # нет lumen/lumen
            res = am.analyse(am.load_cells({"chrome": [ch], "lumen": [lu]}), {})
            self.assertEqual(res["measured"], [])

    def test_render_has_verdicts(self):
        cells = self.cells({"a": ("OK", "OK", "OK", "OK")})
        md = am.render_md(am.analyse(cells, {"a": 3}), cells, {"a": 3}, "meta")
        self.assertIn("## Вердикты", md)
        self.assertIn("must-work (вес 3): 1", md)


class Corpus(unittest.TestCase):
    def test_build_corpus_unique_and_big_enough(self):
        rows = am.build_corpus()
        self.assertGreaterEqual(len(rows), 100)
        self.assertEqual(len({u for _, u, _ in rows}), len(rows))
        self.assertEqual(len({s for s, _, _ in rows}), len(rows))
        self.assertTrue(any(s.startswith("stand-") for s, _, _ in rows))


if __name__ == "__main__":
    unittest.main()
