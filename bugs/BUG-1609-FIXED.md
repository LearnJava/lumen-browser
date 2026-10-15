# BUG-1609 — form controls не растягиваются в grid-области

**Статус:** FIXED 2026-10-14 (P6)
**Заведён:** 2026-10-11 (P6, остаток BUG-1311)
**Область:** layout (`grid_trampoline.rs` — `post_final_item`)

## Симптом

`--dump-layout`, grid `200px × 100px`, `<input type=checkbox style="align-self:stretch;justify-self:stretch">` → 15×15; `<input type=range>` → 129×20; `<button>` → 10×23. Ожидается область. WPT `stretch-grid-item-checkbox-input.html` (без стилей `align`: `normal` растягивает контрол без пропорции).

## Что делать

У FormControl без пропорции `normal`/`stretch` растягивают каждую auto-ось независимо; размеры контролов сейчас приходят из UA-стиля как явные `width`/`height`.

## Исправление (2026-10-14)

UA-размер контрола (`apply_ua_form_controls`) помечен `width_is_ua_default`/`height_is_ua_default` (сбрасываются авторским `width`/`height`, в т.ч. `initial`/`inherit`). В `post_final_item` (`grid_trampoline.rs`) у `BoxKind::FormControl` без пропорции `normal` и `stretch` растягивают каждую такую (или `auto`) ось независимо до области; перед повторной раскладкой нерастянутая ось фиксируется текущим border-box-размером. Тесты: `tests/grid_replaced_stretch.rs` (BUG-1609). `stretch-grid-item-checkbox-input` проверен дампом (50×50). WPT-прогон не делался.
