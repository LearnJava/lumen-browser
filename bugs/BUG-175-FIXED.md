# BUG-175

**Статус:** FIXED 2026-06-17
**Компонент:** paint
**Файл:** `crates/engine/paint/src/backends/femtovg_backend.rs` (+ `cpu_raster.rs`, `display_list.rs`)

## Описание

`border-radius` вместе с `border`: фон рисовался скруглённым (`FillRoundedRect`),
но рамка (`DrawBorder`) — четырьмя axis-aligned прямоугольниками сторон без учёта
радиуса. В результате вокруг скруглённого фона рисовалась квадратная рамка: на
пилюлях, кругах и эллипсах с бордером (TEST-36 ряды 2/3/5/6) углы рамки были
прямыми, фон — скруглённым. Видимый артефакт — квадратная обводка вокруг круглой
формы.

## Причина

`DisplayCommand::DrawBorder` несёт поле `radii: CornerRadii`, но оба пиксельных
бэкенда (femtovg — live, cpu_raster — снапшоты) его игнорировали (`radii: _`) и
всегда эмитили 4 прямоугольные стороны.

## Фикс

Когда у бокса есть `border-radius` и все стороны — однородная (один цвет) `solid`
рамка, граница рисуется **even-odd кольцом** между внешним скруглённым rect
(border-box, внешние радиусы) и внутренним скруглённым rect (padding-box,
внутренние радиусы = внешний − ширина стороны, CSS Backgrounds L3 §5.5).
Неоднородные цвета / dashed-dotted-double по-прежнему падают в axis-aligned
стороны (квадратные углы) — редкий случай, вне TEST-36.

Геометрия вынесена в `CornerRadii::clamped_to_box` и `CornerRadii::inner_for_border`
(`display_list.rs`), общий outline-строитель — `append_rounded_rect_outline`
(femtovg) / `push_rounded_rect_outline` (cpu_raster), оба используют кубическую
bézier-аппроксимацию четвертей эллипса (kappa ≈ 0.5523).

TEST-36: 1.50% → 1.11%. Остаток (edge-AA + эллиптические углы) → BUG-176,
TEST-36 в `KNOWN_DEBTORS`.

## Тесты

- `cpu_raster::tests::draw_border_rounded_corner_is_not_square` — пиксельный:
  угол скруглённой рамки пуст (не квадрат), середины сторон — цвет рамки,
  центр (padding-box) пуст (кольцо имеет дырку).
- `display_list::tests::inner_for_border_subtracts_side_widths` /
  `inner_for_border_floors_at_zero` / `clamped_to_box_caps_at_half` — геометрия.
