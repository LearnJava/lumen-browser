# BUG-1609 — form controls не растягиваются в grid-области

**Статус:** OPEN
**Заведён:** 2026-10-11 (P6, остаток BUG-1311)
**Область:** layout (`grid_trampoline.rs` — `post_final_item`)

## Симптом

`--dump-layout`, grid `200px × 100px`, `<input type=checkbox style="align-self:stretch;justify-self:stretch">` → 15×15; `<input type=range>` → 129×20; `<button>` → 10×23. Ожидается область. WPT `stretch-grid-item-checkbox-input.html` (без стилей `align`: `normal` растягивает контрол без пропорции).

## Что делать

У FormControl без пропорции `normal`/`stretch` растягивают каждую auto-ось независимо; размеры контролов сейчас приходят из UA-стиля как явные `width`/`height`.
