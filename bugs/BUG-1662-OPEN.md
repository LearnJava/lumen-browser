# BUG-1662 — `getComputedStyle(el, '::first-line')` пуст для `display: inline`/`flex` и не отдаёт `text-decoration`, `font-kerning`, `text-underline-position`

**Статус:** OPEN
**Заведён:** 2026-10-21 (P3, остаток [BUG-1437](BUG-1437-FIXED.md))
**Область:** layout (`crates/engine/layout/src/lib.rs::collect_pseudo_computed_styles`, `box_tree/build.rs` — стиль `::first-line` считается только для блока с inline-содержимым; `selector_query.rs::computed_style_to_map`)

## Симптом

`css/css-pseudo/first-line-allowed-properties.html`: 38 из 112 сабтестов (до BUG-1437 — 32).

1. Цель `display: inline` или `display: flex` — `getComputedStyle(target, '::first-line')[prop]` возвращает `""` для всех «применимых» (`color`, `background-*`, `font-size`, `letter-spacing`, …) и «неприменимых» свойств: 2 × 37 сабтестов. Chrome/Firefox отдают вычисленный стиль псевдоэлемента, даже когда первой строки как бокса нет (стиль псевдоэлемента определён каскадом независимо от того, порождает ли элемент `::first-line`). У Lumen запись в `collect_pseudo_computed_styles` берётся из сегмента первой строки, а его нет: стиль `::first-line` вычисляется в `build.rs` только для блок-контейнера.
2. Даже у блока (`display: block`) пусты `fontFeatureSettings`, `fontKerning`, `fontSizeAdjust`, `fontVariationSettings`, `textDecoration` (шорткод), `textUnderlinePosition` — этих свойств нет в `computed_style_to_map` вообще (тот же механизм, что [BUG-1407](BUG-1407-OPEN.md); `font-kerning`, `text-decoration`, `text-underline-position` в его список не входят).

## Что делать

1. Для элемента, у которого лист стилей содержит правило `::first-line`, но сегмента нет, считать стиль псевдоэлемента при сборке снимка (нужен доступ к документу и листу стилей — сейчас `collect_pseudo_computed_styles` получает только дерево боксов).
2. Добавить `font-kerning`, `text-underline-position` и шорткод `text-decoration` в `computed_style_to_map` (после BUG-1407).

## Как проверить

`run_report.py --all --root css/css-pseudo --offset <позиция first-line-allowed-properties> --limit 1`.
