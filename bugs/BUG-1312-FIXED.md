# BUG-1312 — `order` на grid-item не меняет порядок отрисовки

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 7, `css/css-grid`)
**Область:** layout/paint (порядок раскладки/отрисовки grid-items — «order-modified document order», CSS Grid L1 §6.3: `order` влияет и на painting order)

## Симптом

`<div style="display:grid;font:100px/1 Ahem"><div style="order:1;grid-area:1/1;color:green">G</div><div style="grid-area:1/1;color:red">R</div></div>`, `--dump-display-list`:

```
DrawText … "G" #008000ff   (первым)
DrawText … "R" #ff0000ff   (вторым — поверх)
```

Ожидается обратное: `R` (order 0) раньше, `G` (order 1) поверх — зелёный квадрат без красного. Порядок в display list — порядок DOM.

## Как найдено

WPT-RUN-14 срез 7: `grid-items/grid-order-property-painting-001…005.html`, `grid-inline-order-property-painting-001…005.html` — 10 reftest, все thick. Родственные `grid-z-axis-ordering-*` (21 reftest) в снимке `identical`: там падает только Ahem без `@font-face` ([BUG-1273](BUG-1273-FIXED.md)), порядок z верен.

## Причина и исправление

Порядок отрисовки, hit-теста и нумерации stacking-слоёв шёл по `LayoutBox::children` в порядке DOM; `order` читался только раскладкой (`item_idxs`). То же касалось flex (`order` на flex-item не менял порядок отрисовки). Теперь `lumen_layout::paint_child_order` отдаёт order-modified порядок детей flex/grid-контейнера (стабильно по DOM при равных значениях; `None`, если все `order` равны), и его используют `StackingTree::build`, `fill_buckets` (`continue_fill`), legacy `walk`/`walk_with_anim` и оба hit-теста. Дерево боксов остаётся в порядке DOM — на нём держатся graft и инкрементальный layout. Регрессии: `display_list/tests/flex_grid_order_paint.rs`, `hit_test::tests::grid_item_with_higher_order_is_topmost_in_hit_test`.

## Что предлагалось

Строить дочерние боксы grid-контейнера (и порядок paint) по `order`, как уже сделано для размещения (`order` в авто-размещении работает), — стабильно по DOM для равных значений.

## Как проверить

`css/css-grid/grid-items/grid-order-property-painting-001.html`, `grid-inline-order-property-painting-001.html`.
