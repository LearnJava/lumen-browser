"""Тесты дашборда отчёта качества: `python scripts/test_quality_report_server.py`."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import quality_report_server as srv  # noqa: E402

REPORT = """# Отчёт качества

**Итог:** WPT 42.92% (Δ +9.21 п.п.) · graphic_tests 112/165 PASS (Δ +1) · открытых багов 293 (Δ -26) · просадок 7

## Источники

| Источник | Свежий | Предыдущий | Отставание от main |
|---|---|---|---|
| WPT (`docs/wpt/runs`) | `new.json` · `abc` · 2026-10-23T12:38:10 | `old.json` · `def` · 2026-10-05T16:15:35 | вне main |

**Внимание:**

- у прогонов WPT разные параметры раннера: `processes` 7 → 10.

## Просадки с прошлого отчёта

- WPT `a`: балл 2 → 1 (-1).
- WPT `b`: балл 2 → 1 (-1).
- WPT `c`: балл 2 → 1 (-1).
- WPT `d`: балл 2 → 1 (-1).
- WPT `e`: балл 2 → 1 (-1).
- WPT `f`: балл 2 → 1 (-1).
- graphic_tests `18-images`: PASS → FAIL (diff 6.1%).

## WPT: статусы тестов

| Статус | Сейчас | Было | Δ |
|---|---|---|---|
| PASS | 16309 | 11211 | +5098 |
| FAIL | 13168 | 17861 | -4693 |
| SKIPPED | 178 | 178 | 0 |
| **всего id** | 68116 | 68116 | 0 |

## WPT по подсистемам

| Подсистема | Тестов | Прогнано | OK/PASS | Сабтесты | Pass rate | Δ |
|---|---|---|---|---|---|---|
| HTML (в т.ч. парсер) | 9981 | 9981 | 7821 | 1/2 | 44.16% | +1.03 п.п. |
| DOM | 1351 | 1351 | 1211 | 1/2 | 37.63% | -2.00 п.п. |
| Сеть | 1373 | 1373 | 1063 | 1/2 | 35.65% | n/a |

## graphic_tests

Pass rate 67.88% (112/165; DEBTOR — долг), Δ +0.20 п.п.

| Тест | Было | Стало | diff % |
|---|---|---|---|
| `165-svg-clip-path` | — | PASS | 0.0355 |
| `20-fixed` | FAIL | PASS | 0.0 |

## WPT по категориям

| Категория | Тестов | OK/PASS | Δ | Pass rate | Δ |
|---|---|---|---|---|---|
| `small` | 10 | 9 | +5 | 90.00% | +50.00 п.п. |
| `css` | 33496 | 21384 | +5047 | 53.47% | +17.66 п.п. |
| `encoding` | 1343 | 409 | -106 | 5.62% | 0.00 п.п. |
"""


class Dashboard(unittest.TestCase):
    def test_bar_gain_marks_previous_value(self) -> None:
        html = srv._bar_html("HTML", 44.16, 1.03, "t")
        self.assertIn('class="prev" style="left:43.13%"', html)
        self.assertIn('class="seg gain" style="left:43.13%;width:1.03', html)
        self.assertIn('delta good">▲ +1.03 п.п.', html)

    def test_bar_loss_paints_lost_part(self) -> None:
        html = srv._bar_html("DOM", 37.63, -2.0, "t")
        self.assertIn('class="fill" style="width:37.63%"', html)
        self.assertIn('class="seg loss" style="left:37.63%;width:2', html)
        self.assertIn('class="prev" style="left:39.63%"', html)
        self.assertIn('delta bad">▼ -2 п.п.', html)

    def test_bar_without_delta_has_no_mark(self) -> None:
        html = srv._bar_html("Сеть", 35.65, None, "t")
        self.assertNotIn('class="prev"', html)
        self.assertIn("прошлого значения нет", html)

    def test_dashboard_reads_subsystem_and_graphic_deltas(self) -> None:
        html = srv.parse_dashboard(REPORT)
        self.assertEqual(html.count('class="bar-row"'), 4)
        self.assertEqual(html.count('class="prev"'), 3)  # у «Сеть» Δ n/a
        self.assertIn("▲ +0.2 п.п.", html)  # graphic_tests из своего раздела

    def test_fewer_bugs_is_good(self) -> None:
        html = srv.parse_dashboard(REPORT)
        self.assertIn('delta good">▼ -26', html)
        self.assertIn('delta good">▲ +9.21 п.п.', html)


class Changes(unittest.TestCase):
    def setUp(self) -> None:
        self.html = srv.parse_changes(REPORT)

    def test_names_compared_runs_and_warnings(self) -> None:
        self.assertIn("<code>new.json</code> (2026-10-23) сравнивается с "
                      "<code>old.json</code> (2026-10-05)", self.html)
        self.assertIn('<ul class="warn">', self.html)

    def test_status_shift_colours(self) -> None:
        self.assertIn('delta good">▲ PASS 11211 → 16309 (+5098)', self.html)
        self.assertIn('delta good">▼ FAIL 17861 → 13168 (-4693)', self.html)
        self.assertNotIn("SKIPPED", self.html)
        self.assertNotIn("всего id", self.html)

    def test_growth_weighted_by_tests(self) -> None:
        css, small = self.html.index("<code>css</code>"), self.html.index("<code>small</code>")
        self.assertLess(css, small)  # +17.66 п.п. на 33496 тестов весомее +50 на 10
        self.assertNotIn("<code>encoding</code>", self.html)
        self.assertIn("новый тест, PASS", self.html)
        self.assertIn("FAIL → PASS", self.html)

    def test_regressions_truncated_with_link(self) -> None:
        self.assertIn("<code>e</code>", self.html)
        self.assertNotIn("<code>f</code>", self.html)
        self.assertIn('ещё 2 — <a href="#s-Просадки-с-прошлого-отчёта">', self.html)
        self.assertIn('id="s-Просадки-с-прошлого-отчёта"', srv.md_to_html(REPORT))

    def test_no_regressions_shows_note(self) -> None:
        text = REPORT.replace("## Просадки с прошлого отчёта\n\n- WPT `a`",
                              "## Просадки с прошлого отчёта\n\nПросадок нет.\n\n## X\n\n- WPT `a`")
        self.assertIn("<li>Просадок нет.</li>", srv.parse_changes(text))

    def test_report_without_sources_has_no_block(self) -> None:
        self.assertEqual(srv.parse_changes("# Отчёт качества\n"), "")


if __name__ == "__main__":
    unittest.main()
