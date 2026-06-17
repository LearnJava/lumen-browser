# BUG-135

**Статус:** FIXED 2026-06-17 (тем же фиксом, что BUG-183)
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`
**Тест:** TEST-104 (51.97% → 0.44% PASS)

## Описание

INTERACTION TEST-104 (mask×gradient×radius): градиентная маска поверх
градиентного фона / скруглений / бордера расходилась во всех ячейках.

## Корень

Та же причина, что BUG-183: `mask-image` создаёт stacking context →
masked-бокс рисуется через `build_display_list_ordered`→`box_layer_ops`,
который не эмитил mask-группу. Градиентные маски были no-op во всех ячейках
теста.

## Фикс

См. BUG-183: `box_layer_ops` теперь эмитит `PushMask*`/`PopMask` как внешний
слой; femtovg `composite_mask_layer` применяет градиент через `DestinationIn`.

Прогон 2026-06-17 20:58: TEST-104 FAIL→PASS 52.01% → 0.44% (delta −51.57%),
без регрессий.
