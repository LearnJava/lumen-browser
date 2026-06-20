# BUG-224

**Статус:** FIXED 2026-06-20
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`) — фактически: stale unit-test
**Тест:** `crates/driver/tests/test_33.rs::test_33_multi_column`

## Описание

Изначально заведён как регрессия высоты multi-column контейнера с `column-span: all`
(auto-height): `test_33_multi_column` падал на чистом main с
`.mc[4] should be 660x88, got 660x64`, и причина приписывалась влитию
`p3-bug198-objectfit-svg`.

**Диагноз при разборе (2026-06-20) оказался иным — это не регрессия движка:**

1. `p3-bug198` (commit `883d6e17`) к multicol-коду **не прикасался** —
   его diff по `box_tree.rs` затрагивает только SVG-функции (`lay_out_svg_root`,
   `parse_dominant_baseline`) и тесты, не `lay_out_multicol_children`.
2. Реальное изменение высоты внёс **BUG-186** (`d08021b1`,
   «multi-column фрагментация колонок, TEST-33 14.89% → 0.12%»): добавлена
   геометрическая фрагментация sliceable-боксов по колонкам. Это **корректное**
   приближение к поведению Edge.
3. После BUG-186 высота `mc[4]` стала **64px**, что совпадает с Edge:
   - две `.col-sm` (height:36) балансируются-фрагментируются на 24px по 3 колонкам
     (72/3 = 24), затем span 8px + margins 4+4 = 16px, затем ещё две `.col-sm` → 24px:
     **24 + 16 + 24 = 64**.
   - Подтверждено напрямую: Edge headless `getBoundingClientRect().height` для `.mc` = **64**.
   - Подтверждено пиксельно: TEST-33 ≈ 0.1% паритет с Edge именно при высоте 64.
4. Юнит-тест `test_33_multi_column` хранил **устаревший** ground-truth (88px) —
   atomic one-box-per-column раскладку, которая была до BUG-186 и которую Edge **не**
   воспроизводит. После корректного улучшения фрагментации регрессионный гард не был
   ре-базлайнен.

## Исправление

Ре-базлайн юнит-теста под Edge-верифицированную геометрию:
`crates/driver/tests/test_33.rs` — ожидаемая высота `mc[4]` 88 → 64 + комментарий
с выводом 24+16+24 и пометкой про BUG-186. Код движка не менялся (он уже корректен).

## Воспроизведение (до фикса)

```bash
cargo test -p lumen-driver --test test_33
# .mc[4] should be 660x88, got 660x64
```

После фикса — `test result: ok. 1 passed`.
