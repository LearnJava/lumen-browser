# BUG-197

**Статус:** FIXED 2026-06-21
**Компонент:** layout
**Тест:** TEST-69 (diff 3.61% → 0.00%)

## Описание

TEST-69 расходился с Edge на 3.61%. Исходный диагноз («asymmetric
`border-spacing`») оказался неверным: геометрия border-spacing (и равного, и
асимметричного `8px 24px`) в `box_tree.rs::lay_out_table` была пиксельно
корректной (подтверждено `--dump-layout`).

Настоящая причина — **отсутствие UA-дефолта `td, th { padding: 1px }`**
(HTML Rendering §15.3.8). Edge добавляет 1px padding с каждой стороны каждой
ячейки (замерено: ячейка 60px рисуется как 62px, 80px → 82px), Lumen — 0px.
Разница в 2px на ячейку накапливалась по вертикали и сдвигала нижние таблицы
вниз (~10px), давая edge-диф по всем границам ячеек.

## Воспроизведение (до фикса)

`python graphic_tests/run.py --only 69 --ipc` → FAIL 3.61%

## Фикс

`crates/engine/layout/src/style.rs` — новая UA-функция
`apply_ua_table_cell_padding` в pre-cascade фазе: `<td>`/`<th>` получают
`padding: 1px` по умолчанию; атрибут `cellpadding=N` на ближайшем
ancestor-`<table>` переопределяет (в т.ч. `cellpadding="0"` для legacy
layout-таблиц); author `padding` выигрывает (применяется после UA-фазы).

Обновлены 3 table-теста в `box_tree.rs` (ожидаемые размеры +2px на ячейку).
+5 регрессионных тестов в `style.rs`.

TEST-69: 3.61% → 0.00% PASS (через `--ipc` CPU-снимок).
