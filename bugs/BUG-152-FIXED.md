# BUG-152

**Статус:** FIXED 2026-06-13
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs:2198`

## Описание

anon_style клонирует ненаследуемые float_side/clear/position родителя: анонимный InlineRun внутри float-блока сам попадает в float-ветку layout-цикла родителя (child_y не продвигается → перекрытие с последующими block-детьми). Анонимные боксы не флоатятся (CSS 2.1 §9.2.2) — нужен сброс float_side (+ ревизия clear/position) с прогоном float-тестов
