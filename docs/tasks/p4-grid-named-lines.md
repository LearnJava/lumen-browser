# Задача: именованные линии грида в `grid-column`/`grid-row`

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:498` · **Размер:** M · **Крейты:** `lumen-layout`

## Цель
`resolve_named_lines` ищет имена только в `grid-template-areas`; `[name]` из списка треков теряется. Ломает full-bleed-вёрстку (`grid-column: content`).

## Точка входа
`crates/engine/layout/src/box_tree/grid.rs:710` (`resolve_named_lines`), `crates/engine/layout/src/style/values/flexgrid.rs:290` (`parse_track_list`), `:474` (`GridLine`).

## Что сделать
Хранить имена линий из `parse_track_list` (включая `repeat()`), неявные `foo-start`/`foo-end`, резолв `GridLine::Named` по линиям, `span name`, `name N`.

## Готово, когда
Юнит layout: `grid-template-columns: [full-start] 1fr [content-start] 600px [content-end] 1fr [full-end]` на 1000px, элемент `grid-column: content` → x = 200, ширина 600.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings`; двигает пиксели → полный `python graphic_tests/run.py --continue-on-fail` + эталоны.
