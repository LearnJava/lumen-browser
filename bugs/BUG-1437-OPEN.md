# BUG-1437 — анимации и переходы свойств CSS Text (`tab-size`, `text-indent hanging/each-line`, `text-align-last`, `line-break`, `hyphens`) и `::first-line` отдают неверные computed-значения

**Статус:** OPEN
**Заведён:** 2026-10-08 (P6, побочная находка BUG-1325)
**Область:** layout (`crates/engine/layout/src/animation.rs` — интерполяция/дискретные переходы этих свойств; `style/pseudo.rs` — `::first-line` не фильтрует неразрешённые свойства)

## Симптом

Против прежнего бинаря (`css/css-text`, `--log-raw`): после [BUG-1325](BUG-1325-FIXED.md) свойства появились в `getComputedStyle()`, и тесты, проходившие вхолостую, честно красные:

| тест | сабтестов | сообщение |
|---|---|---|
| `animations/tab-size-interpolation.html` | 149 | `expected "0" but got "5"` — число в `@keyframes` не интерполируется, значение стоит на середине |
| `animations/text-indent-interpolation.html` | 53 | `expected "0px each-line" but got "10px"` — `hanging`/`each-line` дискретны (CSS Text L3 §7.1), переход между ними не отрабатывается |
| `text-align/text-align-last-interpolation.html` | 49 | дискретный переход `auto → start` (`transition-behavior: allow-discrete`) |
| `animations/text-indent-composition.html` | 10 | `add`/`accumulate` с модификаторами |
| `animations/line-break-no-interpolation.html`, `hyphen-no-interpolation.html` | по 10 | дискретный переход |
| `css-pseudo/first-line-allowed-properties.html` | 3 | `getComputedStyle(el, '::first-line').wordBreak` → `break-all`, ожидается `normal` (`word-break` не входит в разрешённые свойства `::first-line`); для `display: flex`/`inline` — пустая строка |

## Почему раньше «проходило»

`interpolation-testcommon.js` сравнивает значение анимированного элемента с эталонным; у свойства, которого нет в карте, оба `""`.

## Что делать

Подключить свойства к `animation.rs` (число — интерполяция, остальные — дискретно с `allow-discrete`), модификаторы `text-indent` — дискретная часть. `::first-line`: применять к computed только разрешённые свойства (CSS Pseudo L4 §first-line-styling).

## Как проверить

`run_report.py --all --root css/css-text/animations --recursive`; `css/css-pseudo/first-line-allowed-properties.html`.
