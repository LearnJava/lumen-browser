# BUG-1471 — `position: relative` на `<tr>`, `<tbody>`, `<thead>`, `<tfoot>` не смещает строки таблицы (на `<td>` работает)

**Статус:** FIXED 2026-11-18 (P3, срез 1)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — относительное смещение `position:relative` у `tr`/`tbody`/`thead`/`tfoot`)

## Симптом

CSS Positioned Layout 3 §3.1 распространяет относительное смещение на все боксы, включая строки и группы строк таблицы. У нас смещается только ячейка (`td{position:relative;left:30px}` → `x=30`), а `tr{position:relative;top:30px}` и `tbody{position:relative;left:30px}` ничего не сдвигают (`FillRect` ячейки остаётся в `(0,0)`). 18 id `css-position/position-relative-table-*` (`-tr-`, `-tbody-`, `-thead-`, `-tfoot-` × `left`/`top` × `-absolute-child`), все reftest `thick`; `td` и `th` проходят.

## Проба

Проба (`--dump-display-list`, `border-spacing:0`, ячейка 20×20 `background:green`):

| разметка | `FillRect` у нас | ожидается |
|---|---|---|
| `td{position:relative;left:30px}` | `(30,0)` | `(30,0)` |
| `tr{position:relative;top:30px}` | `(0,0)` | `(0,30)` |
| `tbody{position:relative;left:30px}` | `(0,0)` | `(30,0)` |

## Как найдено

WPT-RUN-14 срез 21: `css-position/position-relative-table-tr-top.html`, `position-relative-table-tbody-left.html`.

## Что делать

Применять `relative_offset` (`layout_dispatch.rs`) к боксам `TableRow`/`TableRowGroup` и переносить его на потомков-ячейки; абсолютные потомки с `-absolute-child` берут их как содержащий блок.

## Как исправлено

Строки и группы строк таблицы раскладывает `table_trampoline` напрямую (боксы `TableRow`/`TableRowGroup` не проходят через `finish_after_match`, который применяет относительное смещение обычным блокам). В `finish_table` после финализации всех rect добавлен пост-проход `apply_row_group_relative`:

1. **Смещение строки/группы.** Для каждой позиционированной строки/группы считается `relative_offset` (те же `left`/`right`/`top`/`bottom`, что у блока) и применяется `shift_tree` — сдвиг переносит и все ячейки-потомки, не меняя раскладку таблицы (высота/ширина таблицы посчитаны по несмещённым rect, как у обычного `position: relative`).
2. **Содержащий блок для абсолютных потомков.** Перед сдвигом вызывается `fix_out_of_flow_descendants`, который переставляет абсолютных потомков строки/группы против её padding-box (в `-absolute-child` тестах `top`/`left` отсчитываются от строки/группы, а не от containing block таблицы — критично для `tfoot` после `tbody`, где группа не в начале таблицы).

## Как проверить

Таблица выше; `css/css-position/position-relative-table-tr-top.html` (и остальные 15 reftest `position-relative-table-{tr,tbody,thead,tfoot}-{left,top}[-absolute-child]`). Добавлены unit-тесты `box_tree::tests::relative_offset_flow::{relative_on_tr_shifts_its_cells, relative_on_tbody_shifts_its_cells, relative_group_is_containing_block_for_abs_child}`.
