# BUG-148

**Статус:** FIXED 2026-06-12
**Компонент:** shell
**Файл:** `crates/shell/src/panels/print_panel.rs:924`

## Описание

test panels::print_panel::tests::hit_page_range_field fails on main (с W-2b 61375f84): не перекрытие, а устаревший тест — W-2b вставил строку Scale на row 3, page-range уехал на row 4, тест продолжал кликать row_y(3) (там ScaleDecrease). Рендер и hit_test согласованы; обновлён тест
