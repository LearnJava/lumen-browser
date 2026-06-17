# BUG-183

**Статус:** FIXED 2026-06-17
**Компонент:** paint
**Тест:** TEST-26 (17.74% → 5.02%, остаток = BUG-218 mask-mode:luminance)

## Описание

`mask-image` с linear/radial gradient mask рендерился как no-op (полная
непрозрачность) — femtovg только ставил scissor по rect, а stacking-context
путь паинта вообще терял маску.

## Корень

Две причины:

1. **femtovg-бэкенд** (`femtovg_backend.rs`): gradient/image маски
   аппроксимировались прямоугольным `scissor` — градиент не применялся.

2. **Главная**: `mask-image` создаёт stacking context, поэтому masked-бокс
   рисуется через `build_display_list_ordered` → `fill_buckets` →
   `box_layer_ops`/`emit_box_self`, а НЕ через `walk`. `box_layer_ops`
   (`display_list.rs:2419`) собирал слои blend/opacity/transform/clip-path/
   filter, но **не маску** — `emit_push_mask` вызывался только в `walk`
   (legacy путь `build_display_list`). В результате `PushMaskLinearGradient`/
   `PopMask` для masked-боксов вообще не эмитились (подтверждено
   `--dump-display-list`: ноль mask-команд, только FillRect).

## Фикс

1. `box_layer_ops` (`display_list.rs`): маска эмитится первой в `pre` (самый
   внешний слой, CSS Masking L1 §4 — маска оборачивает полностью
   скомпонованный элемент), парный `PopMask` — в `post` (после `reverse()`
   становится последней командой). Переиспользует `emit_push_mask`.

2. femtovg `composite_mask_layer` (`femtovg_backend.rs`): gradient-маска
   рисуется поверх offscreen-FBO элемента с
   `CompositeOperation::DestinationIn` (умножает alpha слоя на alpha
   градиента, `mask-mode: alpha`), затем слой композитится как opacity-группа.
   Linear/radial — точно (reuse `linear_gradient_endpoints`/`resolve_stops`),
   conic — через `draw_conic_gradient` под тем же composite.

## Регресс-тесты

- `ordered_mask_image_gradient_wraps_box_as_stacking_context` (display_list) —
  ordered-путь оборачивает фон masked-бокса в PushMask/PopMask.
- `mask_gradient_alpha_decreases_black_to_transparent` (femtovg) — alpha-ядро,
  на которое опирается DestinationIn.
- cpu_raster уже имел `mask_linear_alpha_gradient_fades_box` /
  `mask_radial_reveals_center_hides_corner`.

## Остаток

5.02% = единственная ячейка `mask-mode: luminance`
(`linear-gradient(to right, black, white)`): оба стопа непрозрачные → alpha-маска
показывает бокс целиком, Edge гасит левую (luma) половину. Требует свойства
`mask-mode` (P4) — заведено как **BUG-218**, TEST-26 → KNOWN_DEBTORS (5.02).
