"""Тесты раннера сквозных сценариев: `python scripts/test_compat_scenarios.py`.

Браузер подменён `FakeDriver`: проверяется формат, SPR-w, оракул, распознавание
экрана провала, повтор и классификация краха/зависания — без окна Lumen.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compat_scenarios as cs  # noqa: E402


def scenario(**over) -> dict:
    sc = {"id": "login-x", "title": "вход", "category": "login",
          "steps": [{"do": "navigate", "url": "https://x.test/"}],
          "oracle": [{"kind": "visible", "value": "input[type=password]"}]}
    sc.update(over)
    return sc


class FakeDriver:
    """Страница = словарь {href, title, text, visible:set(селекторов)}; сценарий поведения — pages по навигации."""

    def __init__(self, pages=None, die_on=None, hang_on=None, status=None):
        self.pages = pages or {}
        self.cur = {"href": "about:blank", "title": "", "text": "", "visible": set()}
        self.die_on, self.hang_on, self.status = die_on, hang_on, status
        self.log: list[tuple] = []
        self.restarts = 0
        self._alive = True

    def _guard(self, what: str) -> None:
        if self.die_on == what:
            self._alive = False
            raise cs.DriverDead("socket closed")
        if self.hang_on == what:
            raise cs.DriverHung("command timed out")

    def navigate(self, url):
        self._guard("navigate")
        self.log.append(("navigate", url))
        self.cur = dict(self.pages.get(url, {"href": url, "title": "", "text": "", "visible": set()}))
        self.cur.setdefault("href", url)

    def click(self, selector):
        self._guard("click")
        if selector not in self.cur.get("visible", set()) and not selector.startswith("[data-lumen-scn"):
            raise cs.StepFailed(f"нет элемента {selector}")
        self.log.append(("click", selector))
        nxt = self.cur.get("on_click", {}).get(selector)
        if nxt:
            self.cur = dict(nxt)

    def type_text(self, selector, text):
        self.log.append(("type", selector, text))

    def wait(self, cond, selector, timeout_s):
        self.log.append(("wait", cond, selector))

    def eval(self, code):
        if self.cur.get("eval_error"):
            raise cs.StepFailed("Eval error: Cannot read properties of null")
        if code.startswith("(() => { const e = document.querySelector("):
            sel = code.split("querySelector(", 1)[1].split(")", 1)[0].strip('"')
            return sel in self.cur.get("visible", set())
        if "data-lumen-scn" in code:
            return self.cur.get("clickable_text", False)
        return self.cur.get("eval", None)

    def state(self):
        return {k: self.cur.get(k, "") for k in ("href", "title", "text")}

    def screenshot(self, path):
        path.write_bytes(b"png")
        return True

    def alive(self):
        return self._alive

    def restart(self):
        self.restarts += 1
        self._alive = True

    def stderr_mark(self):
        return 0

    def main_status(self, mark, url):
        return self.status


class FormatTests(unittest.TestCase):
    def test_valid_scenario_has_no_errors(self):
        self.assertEqual(cs.validate_scenario(scenario()), [])

    def test_missing_oracle_and_steps(self):
        errs = cs.validate_scenario(scenario(oracle=[], steps=[]))
        self.assertTrue(any("oracle" in e for e in errs))
        self.assertTrue(any("steps" in e for e in errs))

    def test_unknown_step_and_missing_fields(self):
        errs = cs.validate_scenario(scenario(steps=[{"do": "dance"}, {"do": "type", "selector": "a"}]))
        self.assertTrue(any("dance" in e for e in errs))
        self.assertTrue(any("нет поля text" in e for e in errs))

    def test_wait_visible_needs_selector(self):
        errs = cs.validate_scenario(scenario(steps=[{"do": "wait", "cond": "visible"}]))
        self.assertTrue(any("нужен selector" in e for e in errs))

    def test_bad_regex_in_oracle(self):
        errs = cs.validate_scenario(scenario(oracle=[{"kind": "text", "value": "("}]))
        self.assertTrue(any("регулярка" in e for e in errs))

    def test_calls_must_be_handoff(self):
        self.assertTrue(cs.validate_scenario(scenario(category="call")))
        self.assertEqual(cs.validate_scenario(scenario(category="call", handoff="calls")), [])

    def test_weights_by_category(self):
        for cat, w in (("login", 3), ("bank", 3), ("payment", 3), ("gov", 3), ("video", 2), ("mail", 2),
                       ("docs", 2), ("call", 2), ("file", 1), ("other", 1)):
            self.assertEqual(cs.effective_weight({"category": cat}), w, cat)
        self.assertEqual(cs.effective_weight({"category": "other", "weight": 2}), 2)

    def test_repository_scenarios_are_valid(self):
        scenarios, errors = cs.load_scenarios()
        self.assertEqual(errors, [])
        ids = [s["id"] for s in scenarios]
        self.assertEqual(len(ids), len(set(ids)))

    def test_select_filters(self):
        scs = [scenario(id="a-1"), scenario(id="b-1", mode="manual"), scenario(id="stand-1", stand=True)]
        self.assertEqual([s["id"] for s in cs.select(scs, [], False, False)], ["a-1", "b-1"])
        self.assertEqual([s["id"] for s in cs.select(scs, [], True, False)], ["a-1"])
        self.assertEqual([s["id"] for s in cs.select(scs, [], False, True)], ["a-1", "b-1", "stand-1"])
        self.assertEqual([s["id"] for s in cs.select(scs, ["stand"], False, False)], ["stand-1"])


class ScreenTests(unittest.TestCase):
    def test_lumen_error_page(self):
        r = cs.classify_screen({"text": "Не удалось открыть страницу\nERR_NAME_NOT_RESOLVED"})
        self.assertEqual((r["class"], r["signals"]), ("lumen_error", ["ERR_NAME_NOT_RESOLVED"]))

    def test_certificate_rejection(self):
        r = cs.classify_screen({"text": ""}, error="Wait error: navigation failed: untrusted certificate chain: UnknownIssuer")
        self.assertEqual((r["class"], r["signals"]), ("cert_error", ["untrusted certificate"]))

    def test_cloudflare_wall(self):
        r = cs.classify_screen({"text": "Just a moment... Checking your browser before accessing", "title": "Just a moment..."},
                               status=403)
        self.assertEqual(r["class"], "wall")

    def test_captcha(self):
        r = cs.classify_screen({"text": "Подтвердите, что вы не робот. " * 3, "title": "Проверка"}, status=200)
        self.assertEqual(r["class"], "captcha")

    def test_blank_and_consent_and_content(self):
        self.assertEqual(cs.classify_screen({"text": "  "})["class"], "blank")
        self.assertEqual(cs.classify_screen({"text": "Мы используем файлы cookie, чтобы сайт работал лучше. Принять все"})["class"],
                         "consent")
        self.assertEqual(cs.classify_screen({"text": "Войти в аккаунт, введите адрес почты и нажмите далее"})["class"], "content")

    def test_crash_and_hang_win(self):
        self.assertEqual(cs.classify_screen({"text": "ERR_FAILED"}, dead=True)["class"], "crash")
        self.assertEqual(cs.classify_screen(None, hung=True)["class"], "hang")

    def test_excerpt_is_trimmed(self):
        r = cs.classify_screen({"text": "слово " * 500})
        self.assertLessEqual(len(r["excerpt"]), cs.EXCERPT_MAX)


class SprTests(unittest.TestCase):
    def rec(self, outcome, weight, counts=True, cat="login", screen=None, **kw):
        r = {"id": "x", "outcome": outcome, "weight": weight, "counts": counts, "category": cat}
        if screen:
            r["screen"] = {"class": screen}
        r.update(kw)
        return r

    def test_weighted_share(self):
        recs = [self.rec("pass", 3), self.rec("fail", 3, screen="wall"), self.rec("pass", 2), self.rec("pass", 1)]
        s = cs.spr_w(recs)
        self.assertEqual((s["weight_total"], s["weight_passed"], s["spr_w_pct"]), (9, 6, 66.7))
        self.assertEqual(s["failures_by_screen"], {"wall": 1})

    def test_crash_and_hang_are_failures(self):
        s = cs.spr_w([self.rec("crash", 3, screen="crash"), self.rec("hang", 2, screen="hang"), self.rec("pass", 1)])
        self.assertEqual((s["crashes"], s["hangs"], s["failed"]), (1, 1, 2))
        self.assertEqual(s["spr_w_pct"], round(100 * 1 / 6, 1))

    def test_skips_handoffs_and_stands_are_outside(self):
        recs = [self.rec("pass", 3), self.rec("skip", 3, skip_reason="no_account"),
                self.rec("pass", 3, counts=False), self.rec("fail", 3, counts=False)]
        s = cs.spr_w(recs)
        self.assertEqual((s["scenarios"], s["spr_w_pct"], s["skipped"]), (1, 100.0, 1))
        self.assertEqual(s["skipped_by_reason"], {"no_account": 1})

    def test_empty_and_base(self):
        self.assertIsNone(cs.spr_w([])["spr_w_pct"])
        self.assertFalse(cs.spr_w([self.rec("pass", 1)])["base_reached"])
        self.assertTrue(cs.spr_w([self.rec("pass", 1) for _ in range(40)])["base_reached"])

    def test_failure_without_screen_is_counted(self):
        self.assertEqual(cs.spr_w([self.rec("fail", 3)])["failures_without_screen"], 1)


class RunTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.out = Path(self.tmp.name)
        cs.DEFAULT_ORACLE_WAIT_S = 0

    def tearDown(self):
        self.tmp.cleanup()

    def run_sc(self, drv, sc, rerun=False):
        sc = dict(sc)
        sc["oracle_wait_s"] = 0
        return cs.run_scenario(drv, sc, {"stand": "http://127.0.0.1:1"}, self.out, rerun)

    def test_pass(self):
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "Вход", "visible": {"input[type=password]"}}})
        r = self.run_sc(drv, scenario())
        self.assertEqual(r["outcome"], "pass")
        self.assertNotIn("screen", r)

    def test_oracle_miss_gives_screen_and_png(self):
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "Введите адрес почты, чтобы продолжить", "visible": set()}})
        r = self.run_sc(drv, scenario())
        self.assertEqual(r["outcome"], "fail")
        self.assertIn("visible input[type=password]", r["failed_oracle"])
        self.assertEqual(r["screen"]["class"], "content")
        self.assertTrue((self.out / r["screen"]["png"]).exists())

    def test_step_failure_stops_before_oracle(self):
        sc = scenario(steps=[{"do": "navigate", "url": "https://x.test/"}, {"do": "click", "selector": "#next"}])
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": {"input[type=password]"}}})
        r = self.run_sc(drv, sc)
        self.assertEqual(r["outcome"], "fail")
        self.assertEqual(r["failed_step"]["index"], 1)

    def test_oracle_eval_error_is_a_miss_not_a_crash(self):
        sc = scenario(oracle=[{"kind": "eval", "value": "document.querySelector('#x').value === '1'"}])
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "eval_error": True}})
        r = self.run_sc(drv, sc)
        self.assertEqual(r["outcome"], "fail")
        self.assertIn("eval:", r["failed_oracle"])

    def test_optional_step_failure_is_ignored(self):
        sc = scenario(steps=[{"do": "navigate", "url": "https://x.test/"}, {"do": "click", "selector": "#cookie", "optional": True}])
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": {"input[type=password]"}}})
        self.assertEqual(self.run_sc(drv, sc)["outcome"], "pass")

    def test_click_leads_to_next_page_and_url_oracle(self):
        sc = scenario(steps=[{"do": "navigate", "url": "https://x.test/"}, {"do": "click", "selector": "#next"}],
                      oracle=[{"kind": "url", "value": r"/step2$"}, {"kind": "not_text", "value": "ошибка"}])
        page2 = {"href": "https://x.test/step2", "text": "Пароль", "visible": set()}
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": {"#next"},
                                              "on_click": {"#next": page2}}})
        self.assertEqual(self.run_sc(drv, sc)["outcome"], "pass")

    def test_click_text_requires_match(self):
        sc = scenario(steps=[{"do": "navigate", "url": "https://x.test/"}, {"do": "click_text", "text": "Далее"}])
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": {"input[type=password]"}}})
        r = self.run_sc(drv, sc)
        self.assertEqual(r["outcome"], "fail")
        self.assertIn("Далее", r["failed_step"]["error"])

    def test_crash_is_classified_and_window_restarted(self):
        drv = FakeDriver(die_on="navigate")
        r = self.run_sc(drv, scenario())
        self.assertEqual((r["outcome"], r["screen"]["class"], r.get("restarted")), ("crash", "crash", True))
        self.assertEqual(drv.restarts, 1)

    def test_hang_is_classified(self):
        r = self.run_sc(FakeDriver(hang_on="navigate"), scenario())
        self.assertEqual((r["outcome"], r["screen"]["class"]), ("hang", "hang"))

    def test_state_after_failure_on_busy_window_is_a_hang(self):
        drv = FakeDriver({"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": set()}})

        def state():
            raise cs.DriverHung("engine thread busy")

        drv.state = state
        r = self.run_sc(drv, scenario())
        self.assertEqual((r["outcome"], r["screen"]["class"]), ("fail", "hang"))

    def test_flaky_pass_on_retry(self):
        pages = {"https://x.test/": {"href": "https://x.test/", "text": "x" * 40, "visible": set()}}
        drv = FakeDriver(pages)
        calls = {"n": 0}
        orig = drv.navigate

        def navigate(url):
            calls["n"] += 1
            if calls["n"] == 2:
                pages[url]["visible"] = {"input[type=password]"}
            orig(url)

        drv.navigate = navigate
        r = self.run_sc(drv, scenario(), rerun=True)
        self.assertEqual((r["outcome"], r.get("flaky")), ("pass", True))

    def test_busy_engine_thread_is_a_hang(self):
        class Mcp:
            def tool(self, name, args):
                raise RuntimeError("tools/call: Eval error: engine thread busy: eval not run within 5.0 s")

        class Br:
            mcp = Mcp()

        with self.assertRaises(cs.DriverHung):
            cs.LiveDriver(Br(), 30).eval("1")

    def test_handoff_and_missing_account_are_skipped(self):
        r = self.run_sc(FakeDriver(), scenario(category="call", handoff="calls"))
        self.assertEqual((r["outcome"], r["skip_reason"], r["counts"]), ("skip", "handoff:calls", False))
        r = self.run_sc(FakeDriver(), scenario(account="LUMEN_SCN_NOPE_XYZ"))
        self.assertEqual((r["outcome"], r["skip_reason"]), ("skip", "no_account"))

    def test_stand_placeholder_is_substituted(self):
        sc = scenario(steps=[{"do": "navigate", "url": "{stand}/login.html"}], stand=True)
        drv = FakeDriver({"http://127.0.0.1:1/login.html": {"href": "http://127.0.0.1:1/login.html", "text": "x" * 40,
                                                            "visible": {"input[type=password]"}}})
        r = self.run_sc(drv, sc)
        self.assertEqual((r["outcome"], r["counts"]), ("pass", False))

    def test_summary_lists_failures(self):
        recs = [{"id": "a", "outcome": "fail", "weight": 3, "counts": True, "category": "login",
                 "failed_oracle": "visible #p", "screen": {"class": "wall", "signals": ["hard:cloudflare"]}},
                {"id": "b", "outcome": "pass", "weight": 1, "counts": True, "category": "news"}]
        md = cs.summary_md(recs, {"commit": "abc"})
        self.assertIn("SPR-w: 25.0%", md)
        self.assertIn("| a | 3 | fail | wall hard:cloudflare |", md)


class CorpusTests(unittest.TestCase):
    """Корпус docs/perf/scenarios/*.toml: формат верен и соблюдены границы приёмки COMPAT-CORPUS-3."""

    @classmethod
    def setUpClass(cls):
        cls.scenarios, cls.errors = cs.load_scenarios()
        cls.real = [s for s in cls.scenarios if cs.counts_in_spr(s)]

    def test_format_is_valid(self):
        self.assertEqual(self.errors, [])

    def test_corpus_size_languages_and_nightly(self):
        self.assertGreaterEqual(len(self.real), 40)
        self.assertGreaterEqual(sum(1 for s in self.real if s.get("lang") == "ru"), 15)
        self.assertGreaterEqual(sum(1 for s in self.real if s.get("lang", "en") == "en"), 15)
        self.assertGreaterEqual(sum(1 for s in self.real if s.get("mode", "nightly") == "nightly"), 10)

    def test_categories_cover_the_report_list(self):
        cats = {s["category"] for s in self.real}
        self.assertTrue({"login", "bank", "payment", "gov", "video", "mail", "docs", "file"} <= cats, cats)

    def test_handoffs_are_outside_the_metric(self):
        handoffs = [s for s in self.scenarios if s.get("handoff")]
        self.assertTrue(handoffs)
        self.assertFalse(any(cs.counts_in_spr(s) for s in handoffs))

    def test_no_real_credentials_in_scenarios(self):
        for s in self.real:
            for st in s["steps"]:
                if st.get("do") == "type" and any(k in st["selector"].lower() for k in ("password", "passwd", "name=pass")):
                    self.assertIn(st["text"], ("not-a-real-password", "{pass}"), s["id"])


if __name__ == "__main__":
    unittest.main(verbosity=1)
