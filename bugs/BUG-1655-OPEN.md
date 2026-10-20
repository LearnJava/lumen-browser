# BUG-1655 — `getComputedStyle()`: остаток BUG-1307 по grid (шорткоды, `repeat(N)`, список `grid-auto-*`, вертикальный writing-mode)

**Статус:** OPEN
**Заведён:** 2026-10-20 (P3, выделено из BUG-1307)
**Область:** layout (`crates/engine/layout/src/grid_computed.rs`, `selector_query.rs`, `box_tree/grid_trampoline.rs`)

## Симптом

Лонгхенды grid и `gap` отдаются с BUG-1307, но остаются:

1. Шорткоды `grid` и `grid-template` не сериализуются (`getComputedStyle(el).grid`/`.gridTemplate` — `""`). WPT `css/css-grid/parsing/grid-shorthand-serialization.html` (89 сабтестов).
2. `repeat(<число>, …)` раскрывается при разборе объявления, поэтому у не-grid элемента (`grid-template-columns-computed-nogrid.html`) вычисленное значение — раскрытый список, а не `repeat(2, 2px 3px)`.
3. `grid-auto-columns`/`-rows` в `ComputedStyle` — один `GridTrackSize`, а не список: `grid-auto-columns: 1px 2px 3px` теряет хвост (`grid-auto-columns-computed.html`).
4. Вертикальный `writing-mode` у grid-контейнера: `finish_frame` идёт по ветке `grid_vertical` и `used_grid_tracks` не пишет — `getComputedStyle` отдаёт вычисленное значение вместо px.
5. Вычисленное значение с `vh`/`vw`/`cq*` в дорожках не приводится к px (нет viewport в `computed_style_to_map`).

## Как проверить

`css/css-grid/parsing/{grid-shorthand-serialization,grid-template-columns-computed-nogrid,grid-auto-columns-computed}.html`, `subgrid/grid-template-computed-nogrid.html`.
