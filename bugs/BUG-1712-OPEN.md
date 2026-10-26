# BUG-1712 — `text-underline-position: from-font`: позиция подчёркивания не берётся из метрик шрифта (`post.underlinePosition`), рисуется `0.10 × font-size`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, при BUG-1522)
**Область:** paint (`crates/engine/paint/src/display_list/svg_text_decoration.rs::push_text_decoration_pass` — `auto`/`from-font` дают одну и ту же эвристику `0.10 × fs`, `text-decoration-thickness: from-font` — `0.07 × fs`; метрики шрифта на этапе paint недоступны)

## Симптом

`text-underline-position: from-font` ничем не отличается от `auto`: линия лежит на `baseline + 0.10·fs` независимо от шрифта; толщина `from-font` — `0.07·fs`. Шрифт с нестандартными `post.underlinePosition` / `underlineThickness` (в т.ч. вариативный, `font-variation-settings`) подчёркивается неверно.

## Проба

WPT `css-text-decor/text-underline-offset-variable.html` (`UnderlineTest-VF.ttf`, `from-font`, `font-variation-settings:'UNDO' 640`, `text-underline-offset` ±) — `thick` (1901 px расхождения): позиция должна быть «целым 6 px» от шрифта, у нас `0.1·64 = 6.4`.

## Как найдено

BUG-1522, пробы reftest `text-underline-offset-*`. Сам offset (`px`/`em`/`%`/`calc()`) к этому не относится — он отсчитывается от baseline корректно.

## Что делать

Читать `post.underlinePosition`/`underlineThickness` (`crates/engine/font/src/post.rs`) для выбранной гарнитуры с учётом вариаций и передавать в `InlineFrag`/display list; для `from-font` использовать их вместо эвристики, `auto` оставить прежним.

## Как проверить

`css/css-text-decor/text-underline-offset-variable.html`, `text-decoration-thickness-from-font-variable.html` — `identical`.
