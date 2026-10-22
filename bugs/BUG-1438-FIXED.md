# BUG-1438 — проценты в `letter-spacing`/`word-spacing` и числовой `calc()` в `tab-size` не представимы в `ComputedStyle`

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P6, остаток [BUG-1325](BUG-1325-FIXED.md))
**Область:** layout (`crates/engine/layout/src/style/apply/text.rs` — ветки `letter-spacing`, `word-spacing`, `tab-size`; поля `ComputedStyle::{letter_spacing, word_spacing, tab_size}`)

## Симптом

CSS Text L4 допускает `<length-percentage>` в `letter-spacing`/`word-spacing` (процент от ширины пробела/символа) и `calc()` с числами в `tab-size`. В движке `letter_spacing`/`word_spacing` — готовые px (`f32`), процент отбрасывается при разборе, `tab_size` — px:

| проба | получено | ожидается |
|---|---|---|
| `letter-spacing: 110%` | `normal` | `110%` |
| `letter-spacing: calc(10% - 20%)` | `normal` | `-10%` |
| `letter-spacing: calc(10px - (5% + 10%)` (незакрытая скобка) | `normal` | `calc(-15% + 10px)` |
| то же для `word-spacing` | `0px` | как выше |
| `tab-size: calc(10 + (sign(2cqw - 10px) * 5))` | `64px` | `5` |
| `tab-size: calc(10px + (sign(2cqw - 10px) * 5px))` | `64px` | `5px` |

WPT: `css/css-text/parsing/letter-spacing-computed.html` (4), `word-spacing-computed.html` (4), `tab-size-computed.html` (2). Присваивание `style.letterSpacing = "120%"` уже работает (`-valid` зелёные) — не хватает именно computed-значения и самого процента в раскладке.

## Что делать

Хранить `letter-spacing`/`word-spacing` как `Length` (процент резолвится при раскладке от ширины пробела/advance), `tab-size` — enum {число, длина} с `calc()` по числам и функциями `sign()`/`cq*`. Связано с [BUG-1326](BUG-1326-FIXED.md) (раскладка `tab-size` в ширинах пробела).

## Как проверить

`run_report.py --all --root css/css-text/parsing --recursive` (`letter-spacing-computed`, `word-spacing-computed`, `tab-size-computed`).

## Исправление (2026-10-21)

- `letter-spacing: <percentage>` — доля `font-size` самого элемента (CSS Text L4 §11.2): `ComputedStyle::letter_spacing_pct`; `letter_spacing` по-прежнему готовые px (`abs + pct * font_size`), доля дописывается в конце каскада (`cascade.rs`, после zoom), потомок пересчитывает её от своего `font-size` (в наследуемом значении долю вычитают после пре-пасса `font-size`).
- `getComputedStyle()` (`selector_query.rs::length_percentage_css`) отдаёт процентную часть символически: `110%`, `-10%`, `calc(-15% + 10px)` — для `letter-spacing` и `word-spacing`.
- Функция, не закрытая к концу значения, закрывается там (CSS Syntax L3 §5.4.7): `calc(10px - (5% + 10%)` ≡ `calc(10px - (5% + 10%))` (`style/calc.rs::parse_math_function_value`).
- Числовой `calc()` в `tab-size` без `cq*` уже вычислялся (BUG-1430), покрыт тестом.

Остаток: `cq*` в `tab-size` (2 сабтеста `tab-size-computed`) — [BUG-1664](BUG-1664-OPEN.md).
