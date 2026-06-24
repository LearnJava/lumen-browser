# BUG-232

**Статус:** FIXED 2026-06-22
**Компонент:** layout
**Файл:** `crates/engine/layout/src/box_tree.rs`

## Описание

Column flex (`flex-direction: column`) двойного-считал border у flex-item с явной `height`.
Например `div { height: 28px; border: 2px solid }` внутри column-flex рендерился высотой 36px
вместо 32px (border-box = 28 + 2·2).

## Реальная причина

`lay_out_flex` для column-оси присваивал `children[i].style.height = inner_main`, где
`inner_main` — уже border-box main-size (получен из предварительной border-box высоты и
результата flex grow/shrink). Но item по умолчанию `box-sizing: content-box`, поэтому
повторный `lay_out` интерпретировал `height` как content-высоту и добавлял border/padding
сверху → двойной счёт border. Cross-axis stretch-путь делал это правильно (форсировал
`box-sizing: border-box`), а main-axis column-путь — нет.

## Фикс

В column-ветке перед re-layout форсировать `box_sizing = BorderBox` (зеркало stretch-пути,
box_tree.rs:7705) и передавать `Some(inner_main)` как explicit-height. Геометрия для item
без border не меняется; для item с border/padding высота больше не раздувается.

Найдено при F2-3 (BUG-143): `.label`-полосы в TEST-75 (column-flex секции) садились на 4px
выше и сдвигали grid вниз.
