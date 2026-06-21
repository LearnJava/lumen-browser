# BUG-230

**Статус:** OPEN
**Компонент:** layout/paint
**Файл:** `crates/engine/layout/src/style.rs` (`parse_gradient_stops` → resolve), gradient emission

## Описание

`calc()` в позиции color-stop линейного градиента схлопывает весь градиент.
`linear-gradient(to right, red 0%, blue calc(50% + 10px))` рендерится сплошным
синим; `linear-gradient(to bottom right, transparent calc(50% - 2px), #30363d
calc(50% - 2px), …)` рендерится полностью прозрачным.

`%`-стопы работают (`transparent 49%, #30363d 49%, …` даёт корректную диагональную
полосу), и `parse_length_q` сам по себе ПАРСИТ `calc(...)` в `Length::Calc`. Дефект
ниже по стеку: позиции color-stop типа `Length::Calc` не резолвятся против длины
линии градиента (нужен `percent_basis` = gradient-line length), из-за чего стоп
теряет позицию и список стопов вырождается.

## Где замечен

TEST-76 (CSS Motion Path): диагональный трек-индикатор `.track-diag` использует
`linear-gradient(to bottom right, transparent calc(50% - 2px), #30363d calc(50% -
2px), #30363d calc(50% + 2px), transparent calc(50% + 2px))` для рисования тонкой
диагональной линии. Линия не рисуется → остаточный diff 0.54% после фикса
motion-path (BUG-125). TEST-76 запаркован как KNOWN_DEBTOR на этот баг.

## Починка (для P4/градиентов)

Резолвить `GradientStop.position == Some(Length::Calc(..))` против длины линии
градиента в densification (`style.rs:~28413` «resolved stop positions») — передать
gradient-line length как `percent_basis` в `CalcNode::resolve`.
