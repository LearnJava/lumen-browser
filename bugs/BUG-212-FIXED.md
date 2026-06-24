# BUG-212

**Статус:** FIXED 2026-06-21
**Компонент:** layout (style.rs + box_tree.rs)
**Тест:** TEST-95 (diff 3.39% → 2.86% CPU-render → KNOWN_DEBTORS BUG-128)

## Описание

CSS Fonts L5 `font-size-adjust` — масштабирование x-height; used-size = size × adjust/aspect.

## Расследование

Масштабирование `font-size-adjust` оказалось **корректным**: `font_size_adjust_used`
читает реальный `sxHeight` Inter из OS/2, и used-size = size·z/aspect даёт одинаковый
видимый x-height = size·z для всех строк независимо от шрифта (в этом весь смысл
свойства). Пиксельный замер подтвердил: высоты глифов строк a1–a4 (0.60/0.45/0.30/0.20)
совпадают с Edge.

Реальный дефект был в **вертикальном позиционировании**. Замер центроидов:
прогрессивный сдвиг текста вверх (row none ~2px → row 0.20 ~**32px** выше Edge), причём
шаг между строками у Lumen был непостоянным (111/98/99/104px) против ровного 112px у Edge.

`--dump-layout` показал причину: `line-height: 100px` (фикс-длина) хранится как
**коэффициент** (`100/60 = 1.667`). `apply_font_size_adjust` меняет used-`font_size`
пост-каскадно (60 → 21.98 для row 0.20), и line-box пересчитывался как
`1.667 × 21.98 = 36.6px` вместо фиксированных `100px` → текст центрировался в
схлопнутом боксе, уезжая вверх.

Per CSS2 §10.8.1: absolute `<length>`/`<percentage>` line-height замораживается на
computed-value-time и НЕ масштабируется с used-font-size; только `normal`/`<number>`
относительны.

## Фикс

1. `ComputedStyle.line_height_is_relative: bool` — `true` для `normal`/unitless
   `<number>` (line-box масштабируется), `false` для unit-bearing значений
   (`<length>`/`<percentage>`/`em`/`rem` — absolute, заморожен). Устанавливается в
   `apply_declaration` арме `line-height`, наследуется вместе с `line_height`.
2. `apply_font_size_adjust_to_style` (box_tree.rs): при `!line_height_is_relative`
   корректирует ratio обратно (`line_height *= old_size/new_size`), сохраняя absolute
   line-box в px. Относительные значения остаются нетронутыми (масштабируются как раньше).

После фикса все 5 line-box остаются 100px; baseline-сдвиг row 0.20 32px → 0.5px;
CPU-diff 3.27% → 2.86%.

## Остаток

2.86% = font-parity: глифы «xoxoxoxo»/метки рисуются Inter sans, Edge — своим sans;
ширина/начертание расходятся (rule 3) + row `none` x-height естественно отличается без
нормализации. → KNOWN_DEBTORS (BUG-128, baseline 3.0).

## Регресс-тесты

- `box_tree::tests::font_size_adjust_keeps_absolute_line_height_fixed`
- `box_tree::tests::font_size_adjust_scales_relative_number_line_height`
- `style::tests::line_height_px_is_absolute_number_is_relative`
