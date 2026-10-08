# BUG-1447 — `::first-letter` без `float` не рисует `background` и `border`; computed `::first-letter` теряет `background-*`, у `::first-line` пропадают `background-image` и часть свойств

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout/paint (`::first-letter`, `::first-line`: фон, границы, computed; `crates/engine/layout/src/box_tree/pseudo_text.rs`)

## Симптом

`p::first-letter { background: #0f0 }` без `float` не создаёт команды заливки (`--dump-display-list` пуст, на снимке 0 зелёных px); с `float:left` заливка есть (`FillRect 29×45`). `border` у `::first-letter` без `float` тоже не рисуется. `color` и `font-size` работают. `getComputedStyle(p,'::first-letter').backgroundColor` = `rgba(0, 0, 0, 0)` при заданном `green` (у `::first-line` — `rgb(0, 128, 0)`). `first-line-allowed-properties.html` — 77 из 112 сабтестов падают (`backgroundImage` `none` вместо градиента, пустые `backgroundPosition`/`backgroundAttachment`/`textDecoration` и др.; сообщения вида `expected "none" but got ""`), `first-letter-allowed-properties.html` — 24 из 36. Вместе: 34 id среза с `::first-letter` (33 reftest `thick`) и 24 id с `::first-line` (19 reftest `thick`, остальные — testharness); из них часть относится к уже заведённым [BUG-1373](BUG-1373-OPEN.md)/[BUG-1374](BUG-1374-OPEN.md) (пунктуация) и [BUG-1366](BUG-1366-OPEN.md) (однодвоеточная запись) — разнесено по именам файлов, без пробы на каждом id.

## Проба

`--screenshot` 200×100, `p{margin:0;font:40px Ahem}`, подсчёт ярко-зелёных px:

| правило | зелёных px |
|---|---|
| `p::first-letter{background:green}` | **0** |
| `p::first-letter{background:green;color:green}` | 208 (только глиф) |
| `p::first-letter{float:left;background:green;color:green}` | 1 440 |
| `p::first-letter{border:5px solid green;color:white}` | **0** |
| `p::first-line{background:green}` | 3 563 |

`getComputedStyle(p,"::first-letter")` / `"::first-line"` после `background-color:green;background-image:linear-gradient(black,white);opacity:.5`: `backgroundColor` `rgba(0, 0, 0, 0)` / `rgb(0, 128, 0)`, `backgroundImage` `none` / `none`, `opacity` `1` / `0.5`.

## Как найдено

WPT-RUN-14 срез 20: `css-pseudo/first-letter-002.html`…, `first-line-allowed-properties.html` (77/112), `first-letter-allowed-properties.html`.

## Что делать

Заливка/границы первой буквы — как у встроенного бокса с фоном (у `float` она уже есть); computed — читать полный набор разрешённых для псевдоэлемента свойств (Pseudo 4 §3.3, §3.4).

## Как проверить

Таблица выше; `css/css-pseudo/first-letter-002.html`, `css/css-pseudo/first-line-allowed-properties.html`.
