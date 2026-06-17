# BUG-177

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs` (block height resolution, ~5471)
**Тест:** TEST-115 (empty-cells) 13.45% → 0.00%

## Описание

В таблицах со `border-collapse: separate` ячейки, у которых заданная `height` меньше
высоты содержимого, занижались по высоте: содержимое переполняло ячейку, а шаг строки
(row pitch) оставался коротким на величину переполнения. Ошибка накапливалась вниз по
таблице — к нижним рядам внутренние блоки уезжали вверх относительно Edge на несколько px.

TEST-115: ячейки `td { width:96px; height:64px; border:4px; box-sizing:border-box }`
с дочерним `.blk { width:52px; height:32px; margin:16px auto }`. Content-box ячейки = 56px
(64 − 2×4 border), а содержимому нужно 16+32+16 = 64px. Edge растил border-box ячейки до
72px (64 content + 8 border) → pitch строки 80px (72 + 8 border-spacing). Lumen зажимал
ячейку в 64px → pitch 72px, разница 8px/строку.

Замер пикселей (колонка x=95, navy-блоки `.blk`):
- Edge: y = 69, 229, 365, 525
- Lumen (до фикса): y = 69, 213, 341, 485

## Причина

В общей ветке вычисления высоты блока (`b.rect.height = if let Some(h_len) = &s.height`)
заданная `height` бралась как окончательная высота border-box (с `box_sizing`-коррекцией),
без учёта высоты содержимого. Для обычного блока это правильно (overflow просто вытекает),
но **для table-cell `height` — это минимум** (CSS 2.1 §17.5.3): used-высота =
max(заданная, высота содержимого).

## Фикс

В ветке заданной высоты, при `s.display == Display::TableCell`, берётся
`max(specified, content_box)`, где `content_box = content_height + padding + border`:

```rust
if s.display == Display::TableCell {
    let content_box = content_height
        + padding_top + padding_bottom
        + s.border_top_width + s.border_bottom_width;
    specified.max(content_box)
} else {
    specified
}
```

Высота строки (`row_h`) уже считается как max высот ячеек (`lay_out_table_row`, шаг 4),
поэтому подросшая ячейка автоматически поднимает высоту строки и pitch.

## Регрессионные тесты

`box_tree.rs`:
- `table_cell_height_is_minimum_grows_to_fit_content` — ячейка 64px с 64px-содержимым
  растёт до 72px border-box, pitch строки = 80px.
- `table_cell_height_honoured_when_taller_than_content` — высокая ячейка (120px) с мелким
  содержимым сохраняет 120px (минимум не уменьшает).

TEST-115 13.45% → 0.00% PASS. TEST-64 / TEST-69 остаются FAIL по другой причине
(не связана с min-height ячейки). Без регрессий: `cargo test -p lumen-layout` (2904),
`cargo test -p lumen-paint` (741+21).
