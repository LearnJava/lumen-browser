# BUG-1190 — фон второго из соседних inline-элементов теряется

**Статус:** FIXED 2026-10-01 (P3)
**Заведён:** 2026-09-26 (P1, при A/B display list для GAP-UASHADOWSLOT; воспроизводится на сборке до неё).
**Область:** layout — [`crates/engine/layout/src/box_tree/inline_wrap.rs`](../crates/engine/layout/src/box_tree/inline_wrap.rs)
(слияние соседних слов в один `InlineFrag`), критерий слияния —
[`ComputedStyle::text_rendering_eq`](../crates/engine/layout/src/style/computed.rs).

## Симптом

```html
<div><span style="background:#fc0">a</span><span style="background:blue">b</span></div>
```

`--dump-display-list`: один `FillRect` жёлтого цвета шириной на оба символа и один
`DrawText "ab"`; синего прямоугольника нет. `--screenshot` подтверждает: «ab» целиком на
жёлтом. Chrome: «a» на жёлтом, «b» на синем. То же в flex-item и под `display: contents`;
с пробелом между `span`-ами (`a</span> <span`) — тоже один жёлтый фон на «a b».

## Механизм

`--dump-layout`: у `InlineRun` два сегмента (`seg[0] "a"`, `seg[1] "b"`), но одна строка с
одним фрагментом `frag[0] "ab"`. Сегменты сливаются в `inline_wrap.rs` (ветка «Слияние: только
когда нет pre/post space»), когда `last.style.text_rendering_eq(style)`; этот предикат сравнивает
цвет текста, шрифт, интервалы и `text-decoration`, но не `background_color` (и не прочее, что
рисуется по фрагменту). Фон рисуется по стилю первого фрагмента, так что стиль второго теряется.

## Что требуется

Не сливать фрагменты с разным фоном (и другими свойствами, которые paint берёт с фрагмента);
критерий — пример выше рисует два прямоугольника разного цвета, графические эталоны
перегенерированы.

## Исправление

`ComputedStyle::text_rendering_eq` теперь сравнивает `background_color`: соседние слова с
разным фоном остаются отдельными `InlineFrag`, и paint рисует каждому свой прямоугольник
(`--dump-display-list`: `FillRect #ffcc00` + `FillRect #0000ff`). Тест —
`adjacent_spans_with_different_background_stay_separate_frags`
([`box_sizing_text_props.rs`](../crates/engine/layout/src/tests/box_sizing_text_props.rs)).
Эталоны `dump_golden.py` 12/12 без изменений; полный `graphic_tests/run.py` из сессии менеджера
не запускается (TEST-00 FAIL, захват экрана).
