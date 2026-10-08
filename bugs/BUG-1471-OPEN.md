# BUG-1471 — `position: relative` на `<tr>`, `<tbody>`, `<thead>`, `<tfoot>` не смещает строки таблицы (на `<td>` работает)

**Статус:** OPEN
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

## Как проверить

Таблица выше; `css/css-position/position-relative-table-tr-top.html`.
