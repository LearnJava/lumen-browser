# BUG-174

**Статус:** FIXED 2026-06-17
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs` (`lay_out_svg_element_position`, ~1198)
**Тест:** TEST-119 (paint-order) 56.35% → 0.81%

## Описание

In-flow (`display: inline-block`) SVG `<path>` рисовался в «сырых» user-координатах
атрибута `d` **без смещения на origin своего SVG-вьюпорта**. Все пути из разных SVG-ячеек
схлопывались в одну точку (верхний левый угол страницы): видимым оставался только тот путь,
чей clip-rect случайно накрывал raw-координаты (первая ячейка), остальные обрезались своим
scissor-клипом и пропадали.

TEST-119: 4 ячейки `inline-block` с одинаковым `<path>` — рисовалась только первая.

## Причина

`svg_shape_bbox` для `SvgShapeKind::Path` возвращает `Rect::ZERO` (bbox пути считается на
paint-стороне из данных `d`). В `lay_out_svg_element_position` нулевой bbox проходит через
`apply_transform_to_bbox`, который для нулевого размера возвращает `Rect::ZERO` — теряя
origin вьюпорта `(ox, oy)`. Художник (`emit_svg_shape`, ветка Path) сдвигает вершины пути на
`b.rect.x/y` → сдвиг на `(0, 0)`.

Для `position:absolute` SVG это работало случайно: subtree раскладывался в `(0,0)` и потом
сдвигался `shift_tree`-ом на абсолютную позицию, заодно перенося нулевой rect пути в
правильный origin. У in-flow SVG такого пост-сдвига нет — `lay_out_svg_root` задаёт
`b.rect` корня сразу в потоковую позицию, а путь оставался в ZERO.

## Фикс

Симметрично существующему обходному пути в ветке `SvgText` (которая тоже не может
использовать `apply_transform_to_bbox` из-за нулевого bbox): для `Path` якорим layout-box в
document-проекции origin вьюпорта —

```rust
if matches!(shape, SvgShapeKind::Path { .. }) {
    let (px, py) = composed.transform_point(ox, oy);
    b.rect = Rect::new(px, py, 0.0, 0.0);
}
```

Теперь все 4 пути TEST-119 садятся в свои ячейки (29,29 / 509,29 / 29,365 / 509,365),
`paint-order: stroke` корректно показывает более тонкий видимый штрих в нижнем ряду.

Regression-тест: `inflow_svg_path_box_anchored_at_viewport_origin` (две inline-block SVG
рядом — у второй origin пути строго правее первой).

## Остаток

0.81% — толстый 40px штрих с triangle-soup AA-швами → **BUG-173** (TEST-119 добавлен в
`KNOWN_DEBTORS`, baseline 0.81%). Не регрессировали TEST-47/54/60/82 (изменение
path-specific, не трогает rect/circle/ellipse/line).
