# BUG-1777 — html/: 6 регрессий WPT 10-22→10-27 без бисекта (reftest dir-shadow-41, draw-element-image, 4 теста −1…−3 подтеста)

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, WPT-DROP-1027-S3)
**Область:** layout / paint / shell (окна ниже; виновники не найдены)

## Симптом и окна

Все шесть id стабильно хуже `d4114145a` (WPT-RUN-16) в обоих перепрогонах и на `main` (`12ff9f37d`)
воспроизводятся с теми же числами, что на `a76e4c7b9`. Окно = между соседними бинарями сетки
`.tmp/bisect-bin` (хронологически: ok…bad); порядковые номера — позиции в
`git log --first-parent d4114145a..a76e4c7b9` (1 = `a76e4c7b9`, 144 = самый старый).

| id | old | new / main | окно (ok…bad) | позиции |
|---|---|---|---|---|
| `/html/dom/elements/global-attributes/dir-shadow-41.html` | reftest PASS | FAIL | `bf47a575d`…`417f685ab` | 25–42 |
| `/html/canvas/element/manual/draw-element-image/css-backdrop-filter-and-mask.tentative.html` | reftest PASS | FAIL | `417f685ab`…`a76e4c7b9` | 1–25 |
| `/html/rendering/non-replaced-elements/tables/table-ua-stylesheet.html` | 2/5 | 1/5 | `417f685ab`…`a76e4c7b9` | 1–25 |
| `/html/semantics/disabled-elements/event-propagate-disabled-keyboard.tentative.html` | 29/29 | 28/30 | `90cf87014`…`38f31ef3c` | 91–112 |
| `/html/semantics/forms/the-label-element/proxy-modifier-click-to-associated-element.tentative.html` | 2/4 | 1/4 | `38f31ef3c`…`e3536bfb3` | 75–91 |
| `/html/semantics/interactive-elements/the-dialog-element/abspos-dialog-layout.html` | 6/10 | 4/10 | `old`…`da258ae4f` | 128–144 |

Список merge-коммитов окна: `git log --first-parent --oneline <ok>..<bad>`. Для `abspos-dialog-layout`
подтесты «Dialog should not still be centered when removed…» (expected 381 got 820) и
«recentered if showModal() after removing open» (expected −4604 got 0) — геометрия
`showModal()`/`abspos`; для `proxy-modifier-click` и `event-propagate-disabled` — события клика/
клавиатуры на disabled/label (shell/js input). Следующий шаг — бисект по окну (≤ 4 сборки на id).

## Воспроизведение

`tests/wpt/run_smoke.py --binary <lumen.exe> <id>`; reftest — без `LUMEN_NO_PAINT`. Бинари
сетки лежат в `.claude/worktrees/p2-work/.tmp/bisect-bin/<sha>/lumen.exe`.
