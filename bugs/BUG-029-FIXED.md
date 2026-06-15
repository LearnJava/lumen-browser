# BUG-029

**Статус:** FIXED 2026-05-21
**Компонент:** paint
**Файл:** `crates/engine/paint/src/display_list.rs`

## Описание

border-style: dotted renders square dots instead of circles

## Детали

TEST-21: `border-style: dotted` рисует квадратные точки. По CSS-спеке dots должны быть круглыми (filled circles). dashed и double работают корректно.

**Где смотреть:** `crates/engine/paint/src/display_list.rs` — секция отрисовки dotted-border, заменить FillRect на рисование окружностей через примитив или GPU-path.

**Примечание:** круглые dots — BUG-039 (финальный фикс с Skia-алгоритмом dash ratio и corner quads).
