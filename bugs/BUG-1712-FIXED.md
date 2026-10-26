# BUG-1712 — `text-underline-position: from-font`: позиция подчёркивания не берётся из метрик шрифта (`post.underlinePosition`), рисуется `0.10 × font-size`

**Статус:** FIXED 2026-10-26 (P6)
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

## Исправление

- `lumen-layout`: тип `FontUnderlineMetrics { position, thickness }` и метод `TextMeasurer::underline_metrics_with_families` (по умолчанию `None`); поле `InlineFrag::font_underline`, которое заполняет `annotate_font_underline` после раскладки строк `InlineRun` — только фрагментам с `underline` и `from-font` в позиции или толщине.
- `lumen-paint`: `OwnedFontMetrics` читает `post.underlinePosition/underlineThickness` и (для вариативных шрифтов) `MVAR` `undo`/`unds` по осям `text_variation_axes`; `MultiFontMeasurer` отдаёт их для @font-face и системных face-ов. Бандл Inter-fallback метрик не отдаёт (остаётся `auto`).
- `push_text_decoration_pass`: `from-font` берёт позицию верха линии и толщину (не меньше 1 px) из метрик; явный `text-underline-offset` складывается с позицией шрифта (L4 §5.3); шрифт без метрик — прежняя эвристика `auto`.

Проверка: `text-underline-offset-variable.html` — identical (было thick, 2112 px), `text-decoration-thickness-from-font-variable.html` — identical. Тесты: `multi_font_tests::underline_metrics_follow_variation_axis`, `display_list::text_and_images::underline_from_font_uses_measurer_metrics`.
