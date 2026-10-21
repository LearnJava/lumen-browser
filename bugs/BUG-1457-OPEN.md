# BUG-1457 — Highlight-псевдоэлементы не рисуются в CPU-растре (тот, что снимает `wptrunner`) и не отдают computed: 72 reftest + 10 testharness

**Статус:** OPEN (ДОРАБОТКА → HIGHLIGHT-PAINT-2)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** paint/layout (отрисовка и computed highlight-псевдоэлементов: `::selection`, `::target-text`, `::spelling-error`, `::grammar-error`, `::highlight()`)

## Симптом

Выделение диапазона (`getSelection().addRange(r)`) с `::selection{background:green;color:green}` на `--screenshot` не даёт ни одного зелёного px и ни одной команды в display list; `CSS.highlights.set('x', new Highlight(range))` с `::highlight(x){background:green;color:green}` — тоже 0 px. `getComputedStyle(el,'::selection').backgroundColor` = `rgba(0, 0, 0, 0)` при заданном `green`. Имена файлов: `active-selection-0NN`, `highlight-painting-*`, `target-text-*`, `highlight-cascade/*-painting-*`, `grammar-spelling-errors-*`, `selection-*`, `spelling-error-001`, `svg-text-selection*`, `textpath-selection-011`, `selectors/selection-image-001`; 10 testharness — `highlight-cascade/highlight-pseudos-computed*.html`, `highlight-currentcolor-computed*.html`. Пробы выполнены только для `::selection` и `::highlight()`; `::target-text`/`::spelling-error`/`::grammar-error` — по имени файла. `CSS-SPECS.md`: `::selection` — «parsed; live selection highlight application ⬜».

## Проба

`--screenshot` 200×100, `p{margin:0;font:40px Ahem}`, диапазон по `p`:

| правило | зелёных px |
|---|---|
| `::selection{background:green;color:green}` + выделение | **0** |
| `::highlight(x){background:green;color:green}` + `CSS.highlights.set("x", new Highlight(range))` | **0** |

`getComputedStyle(x,"::selection")` при `p::selection{background-color:green}` → `backgroundColor` `rgba(0, 0, 0, 0)`.

## Как найдено

WPT-RUN-14 срез 20: `css-pseudo/active-selection-011.html`, `highlight-painting-*`, `target-text-001.html`, `highlight-cascade/highlight-pseudos-computed.html` (41 из 41).

## Что делать

Задача `HIGHLIGHT-PAINT` (`ROADMAP.md`): каскад и computed для highlight-псевдоэлементов (Pseudo 4 §3.4, §3.5 — наследование, `currentcolor`), отрисовка фона, цвета, `text-decoration` и `text-shadow` выделенного текста в обоих растрах.

## Как проверить

Таблица выше; `css/css-pseudo/highlight-cascade/highlight-pseudos-computed.html`, `css/css-pseudo/active-selection-011.html`.

## Повторное измерение: WPT-RUN-14 срез 27 (2026-10-09)

`css/css-highlight-api` — 122 id, 32 зелёных (27,5 %), 90 не зелёных: 82 reftest `thick`, 1 `thin-only`, 2 `no-match-ref` (`highlight-image.html`, `highlight-text-dynamic.html`), 5 testharness с упавшими сабтестами (38 из 69). 74 `painting/*` + 16 в корне. Те же симптомы, что выше: `CSS.highlights.set('x', new Highlight(range))` и `::highlight(x){background-color:green;color:green}` на `--screenshot` дают 0 зелёных px; `getComputedStyle(p, "::highlight(x)").backgroundColor` — `rgba(0, 0, 0, 0)`. Сверх того в `css-highlight-api`: `idlharness.window.html` (9 из 33 — `Highlight`/`HighlightRegistry` перечислимы и записываемы, `maplike`/`setlike` не перечислимы), `highlight-pseudo-parsing.html` (`insertRule` в оторванный лист — BUG-1439), `highlight-pseudo-from-font-computed.html` (`text-underline-offset: from-font` — `auto`), `HighlightRegistry-highlightsFromPoint.html` (`contentWindow` — `null`, BUG-480). 14 reftest — `reftest-wait`.

## HIGHLIGHT-PAINT (2026-10-21, ветка `p1-highlight-paint`)

Сделано: каскад и computed highlight-псевдоэлементов, `::selection` и `::highlight(name)` в headless-растре (`--screenshot`, `--dump-display-list`), порядок «тени оригинала → фон → тени подсветки → глифы → декорации». Две пиксель-в-пиксель копии reftest-ов зелёные в `paint/src/display_list/tests/highlight_overlay.rs`. На собранном `lumen.exe` (`--screenshot` 800×600, `--ahem`, срез css-pseudo + css-highlight-api из 212 reftest): совпадают с эталоном 55 → 73, `active-selection-011` и `highlight-painting-001` — идентичны, все 82 проверки `highlight-pseudos-computed.html` проходят.

Остаток — задача `HIGHLIGHT-PAINT-2` (`ROADMAP.md`): живое окно красит только фон (оверлей оболочки), `::target-text` и ошибки орфографии в растре, вертикальный текст и SVG-текст, остаток `thick` разобран по причинам в строке `HIGHLIGHT-PAINT-2` (`ROADMAP.md`).
