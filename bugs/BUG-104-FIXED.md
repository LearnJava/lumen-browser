# BUG-104

**Статус:** FIXED 2026-06-19
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

TEST-62 (CSS Scroll Snap L1) расходился с Edge на 63.70% (порог 0.5%). Тест —
**статический** рендер: проверяет геометрию контейнеров/секций внутри скролл-
контейнеров (scroll-snap-type/align/stop парсятся и хранятся; runtime-снаппинг —
отдельная задача). Реальная причина расхождения — **не scroll snap**, а баг
column flex-grow.

## Корень

`lay_out_flex` для column-направления хардкодил `container_main = 0.0` и
`free_space = 0.0` (`box_tree.rs:7191`, `:7330`, `:7356`). Поэтому в column-flex
flex-grow никогда не распределял свободное место по главной оси (вертикали), даже
когда у контейнера была определённая высота. На TEST-62 правая колонка `.right-col`
(`display:flex; flex-direction:column; flex:1`, без явной height, растянута row-
родителем `.__f` до 702px) имела трёх детей `flex:1; height:220px` — они
схлопывались в высоту ≈0 (видны лишь полоски-бордюры), а их `height:100%`-потомки
давали 0.

## Фикс

1. `lay_out_flex` получил параметр `explicit_main: Option<f32>` — определённый
   content-box main-размер для column. `main_definite` включает grow/shrink и
   justify-content remaining для column так же, как для row.
2. Dispatch (`box_tree.rs:5097`) вычисляет `flex_explicit_main` из `s.height`
   (случай явной высоты column-контейнера).
3. Случай растяжения родителем (нет явной height): при `align-items:stretch` в
   row-родителе column-flex ребёнок получает определённый main только после
   stretch. Добавлен re-layout такого ребёнка с определённой высотой
   (ветка Stretch align-блока) — его `flex-grow` дети заполняют растянутую высоту.

Геометрия стала пиксель-точной (diff: все цветные заливки идентичны Edge).
Остаток 2.32% = font-parity меток секций (BUG-128) + border-radius edge-AA
(BUG-176) → запись в `KNOWN_DEBTORS` (`run.py`: `'62': ('BUG-128', 2.32)`).

## Регрессионные тесты

`box_tree.rs::tests::flex_column_explicit_height_grows_items` (явная высота),
`flex_stretched_column_child_grows_its_items` (растяжение родителем — сценарий
`.right-col`).
