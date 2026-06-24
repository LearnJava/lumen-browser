# BUG-182

**Статус:** FIXED 2026-06-24
**Компонент:** layout/paint
**Тест:** TEST-24 (0.93% → 0.50% PASS)

## Описание

`vertical-align: middle` на inline-block центрировал коробку по line-box, а не по
baseline. В TEST-24 row1 высокий `vertical-align: top` бокс (100px) в той же строке
смещает baseline от центра line-box (CSS 2.1 §10.8.1), поэтому 60px middle-ячейка
оказывалась на 20px ниже, чем в Edge (y=41 вместо y=21).

## Причина

`InlineBlockRow` фаза 2 считала `Middle => (row_full_h - child_full_h) / 2.0` —
геометрический центр строки. Это верно только когда baseline лежит ровно посередине
line-box. При наличии top/bottom-выровненных боксов baseline смещается.

## Фикс

`box_tree.rs` — перед применением vertical-align вычисляется позиция baseline в
строке (`above` от верха line-box):
1. strut (шрифт строки) даёт `above = ascent`, `below = descent`;
2. baseline-боксы: `above = max(above, full_h)` (нижний margin-edge на baseline);
3. middle-боксы: `above = max(above, h/2 + x/2)`, `below = max(below, h/2 − x/2)`;
4. top/bottom-боксы растягивают строку: `below = max(below, h − above)` /
   `above = max(above, h − below)`.

Затем `Middle => above − x_height/2 − full_h/2`. Baseline/Top/Bottom формулы
сохранены без изменений (регрессий нет).

TEST-24 row1 теперь пиксель-в-пиксель с Edge. Остаток 0.50% = row2 SUP/MID/SUB
(текст + фоны inline-спанов, font-parity, rule 3 / класс BUG-128).

## Регресс-тест

`bug182_vertical_align_middle_uses_baseline_not_line_center` (box_tree.rs):
middle-бокс выравнивается к верху line-box (dy=0), а не к центру (dy=20).
