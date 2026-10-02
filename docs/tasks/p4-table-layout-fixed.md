# Задача: `table-layout: fixed`

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:374` · **Размер:** S–M · **Крейты:** `lumen-layout`

## Цель
Свойство есть только в списке парсера (`crates/css-parser/src/lib.rs:358`), в `ComputedStyle` не попадает.

## Точка входа
`crates/engine/layout/src/box_tree/table.rs:703` (`compute_table_col_widths`). `crates/engine/layout/src/table.rs` — мёртвый код, не трогать.

## Что сделать
Поле `table_layout`, ветка fixed: ширины колонок из `<col>` и ячеек первой строки, остаток ширины таблицы поровну между колонками без ширины, содержимое не измеряется (CSS 2.1 §17.5.2.1).

## Не трогать
`caption-side` и раскладку `<caption>` — отдельная задача (в `build_table_init`, `table.rs:347`, дети `TableCaption` сейчас пропускаются; кандидат следующего пополнения).

## Готово, когда
Юнит: таблица 300px, `table-layout: fixed`, первая ячейка `width:100px`, в двух других длинный текст → колонки 100/100/100.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings`; двигает пиксели → полный `python graphic_tests/run.py --continue-on-fail` + эталоны.
