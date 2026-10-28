"""Тесты compat_triage на синтетических строках results.jsonl."""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compat_triage as ct  # noqa: E402


def rec(slug, status="OK", walls=None, exc=None, pixels=None, a11y=None, lumen=None):
    checks = {
        "status": {"ok": True},
        "walls": {"ok": not (walls or {}).get("extra") and not (walls or {}).get("lumen_hits"),
                  "extra": [], "chrome_hits": [], "lumen_hits": [], **(walls or {})},
        "exceptions": {"ok": not exc, "new_sigs": exc or []},
        "pixels": pixels or {"ok": True},
        "a11y": a11y or {"ok": True},
    }
    return {"slug": slug, "url": f"https://{slug}/", "lumen_status": status, "computed": True,
            "checks": checks, "runs": [{"lumen": {"ready_s": 4.2, **(lumen or {})}}]}


CORPUS = ("# заголовок\n"
          "slug\tсегмент\tпозиция\tвес\tранг\tкатегория\tисточник\tстатус\tглавная\tвнутренняя\n"
          "a.ru\tru\t1\t3\t1\tx\tx\t200\thttps://a.ru/\thttps://a.ru/i\n"
          "b.com\tglobal\t60\t2\t1\tx\tx\t200\thttps://b.com/\thttps://b.com/i\n"
          "c.com\tglobal\t290\t1\t1\tx\tx\t200\thttps://c.com/\thttps://c.com/i\n"
          "d.com\tglobal\t3\t3\t1\tx\tx\t200\thttps://d.com/\thttps://d.com/i\n"
          "e.com\tglobal\t4\t3\t1\tx\tx\t200\thttps://e.com/\thttps://e.com/i\n")


class TriageTest(unittest.TestCase):
    def test_priority_and_causes(self):
        self.assertIsNone(ct.primary_class(rec("ok")))
        self.assertEqual(ct.primary_class(rec("t", "TIMEOUT", walls={"extra": ["blank:near-empty"]})),
                         "TIMEOUT/HUNG")
        self.assertEqual(ct.primary_class(rec("n", "NET_FAIL")), "NET_FAIL")
        self.assertEqual(ct.primary_class(rec("b", "DEGRADED", walls={"extra": ["blank:near-empty"]})),
                         "blank:near-empty")
        self.assertEqual(ct.primary_class(rec("r", "BROKEN_RENDER", exc=["x"])), "BROKEN_RENDER")
        self.assertEqual(ct.primary_class(rec("p", pixels={"ok": False, "detail": "diff 60%"})), "pixels")
        self.assertEqual(ct.primary_class(rec("np", pixels={"ok": False, "detail": "нет кадра"})), "прочее")
        self.assertEqual(ct.primary_class(rec("a", a11y={"ok": False})), "a11y")
        self.assertEqual(ct.primary_class(rec("w", "DEGRADED", walls={"chrome_hits": ["hard:cloudflare-chl"]})),
                         "не наш")
        self.assertEqual(ct.primary_class(rec("s", "SITE_REFUSED")), "не наш")

    def test_cause(self):
        r = rec("x", "BROKEN_RENDER", exc=["typeerror: foo.bar is not a function"])
        self.assertEqual(ct.cause(r, "BROKEN_RENDER"), "нет API: is not a function")
        r = rec("x", "BROKEN_RENDER", exc=["syntaxerror: bad"])
        self.assertEqual(ct.cause(r, "BROKEN_RENDER"), "исключение: syntaxerror: bad")
        r = rec("x", "NET_FAIL", lumen={"net_failures": ["tls handshake https://h/"] * 2})
        self.assertTrue(ct.cause(r, "NET_FAIL").startswith("сеть: tls handshake"))
        r = rec("x", "TIMEOUT", lumen={"stderr_error_sigs": {"→ get u": {"count": 9},
                                                              "panic at x": {"count": 1}}})
        self.assertEqual(ct.cause(r, "TIMEOUT/HUNG"), "stderr: panic at x")
        self.assertEqual(ct.cause(rec("x", "TIMEOUT"), "TIMEOUT/HUNG"), "без улик")

    def test_report_totals_and_weights(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            corpus = d / "c.tsv"
            corpus.write_text(CORPUS, encoding="utf-8")
            rows = [rec("a.ru", "TIMEOUT"), rec("b.com", "TIMEOUT"), rec("c.com"),
                    rec("d.com", "BROKEN_RENDER", exc=["typeerror: q is not defined"]),
                    rec("e.com", "BROKEN_RENDER", exc=["typeerror: q is not defined"])]
            res = d / "r.jsonl"
            res.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
            user = d / "u.jsonl"
            user.write_text(json.dumps(rec("a.ru", "TIMEOUT")), encoding="utf-8")
            out = d / "o.md"
            self.assertEqual(ct.main(["--results", str(res), "--corpus", str(corpus), "--out", str(out),
                                      "--user-results", str(user)]), 0)
            text = out.read_text(encoding="utf-8")
            weights = ct.load_weights(corpus)
            sites = ct.build(ct.load_records([res]), weights, {"a.ru"})
            self.assertEqual(len(sites), 4)
            cl = ct.clusters(sites)
            self.assertEqual([(c["class"], c["n"], c["weight"]) for c in cl],
                             [("TIMEOUT/HUNG", 2, 7), ("BROKEN_RENDER", 2, 6)])
            self.assertEqual(sum(c["n"] for c in cl), len(sites))
            self.assertIn("## Сайты пользователя", text)
            self.assertIn("| **итого** | **4** |", text)
            self.assertIn("| BROKEN_RENDER | 2 | 6 |", text)
            self.assertIn("| TIMEOUT/HUNG | 2 | 7 |", text)


if __name__ == "__main__":
    unittest.main()
