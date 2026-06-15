# BUG-154

**Статус:** FIXED 2026-06-15
**Компонент:** layout
**Файл:** `crates/engine/layout/src/color_mix.rs:166`

## Описание

mix_polar путает индекс hue для Lab-полярных пространств: `srgb_to_lch`/`srgb_to_oklch` возвращают `[L, C, h, a]` (hue на индексе 2), но `mix_polar` берёт hue из индекса 0. Итог: `color-mix(in oklch/lch, …)` и градиенты `linear-gradient(in oklch/lch, …)` интерполируют hue ЛИНЕЙНО (red→blue идёт через зелёный) и крутят shortest-arc по L. Проверено: mix_colors(Oklch, red, blue) даёт rgb(0,146,0) вместо magenta. HSL/HWB корректны (hue на индексе 0). Фикс: в ветках Lch/Oklch переставить компоненты в [h, L, C, a] перед mix_polar и обратно, либо обобщить mix_polar на hue-индекс. Из-за этого gradient interpolation (p4-gradient-interpolation) и graphic test 116 исключают oklch/lch
