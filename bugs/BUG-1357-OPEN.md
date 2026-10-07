# BUG-1357 — `float` / `position:absolute` на `table-row-group|row|cell` не блокифицируются: бокс остаётся табличным (CSS 2.1 §9.7)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/` — вычисление `display` при `float`/`position:absolute|fixed`; `table.rs`)

## Симптом

CSS 2.1 §9.7: при `float` ≠ `none` или `position: absolute|fixed` `display` вычисляется как блочный аналог (`table-row-group`, `table-row`, `table-cell` → `block`). Lumen оставляет `TableRowGroup`/`TableRow`/`Block display=table-cell` и раскладывает их по табличным правилам.

`--dump-layout`, `display:table` с одной ячейкой 50×50:

| разметка | получено | ожидается |
|---|---|---|
| ребёнок `display:table-row-group; position:absolute; width:40; height:20; left:0; top:0` | `TableRowGroup rect=(100,30,50,50)` — размер таблицы, `width/height` игнорируются | блок 40×20 в `(100,30)` |
| ребёнок `display:table-row; position:absolute` (те же размеры) | `TableRow rect=(100,30,50,20)` — ширина 50 вместо 40 | 40×20 |
| ребёнок `display:table-cell; position:absolute` | `rect=(0,0,0,0)` — бокс схлопнут в нуль | блок 40×20 |
| ребёнок `display:table-row-group; float:right` | `TableRowGroup rect=(0,0,300,50)` на всю ширину таблицы | сдвинут к правому краю |

Второй, связанный симптом (`--dump-display-list`): **`position:absolute` потомок, лежащий прямо в `display:table` или `table-row-group`, не рисуется вообще** — ни `FillRect`, ни бокса нужного размера (в `--dump-layout` `rect=(…,0,0)` при `w=40 h=20`). Тот же потомок в `display:block` или внутри `table-cell` рисуется (`FillRect (100,30,40,20)`). Строка выше про `table-cell` с `position:absolute` (`0×0`) — тот же путь: абсолютный бокс, прямой ребёнок табличного контейнера, выпадает из раскладки. Один механизм или два — пробой не разделено.

## Как найдено

WPT-RUN-14 срез 13: серии `*-applies-to-*` (`positioning/{left,right,top,bottom,position}-applies-to-*`, `floats-clear/{float,clear}-applies-to-*`) — тест ставит свойство на `#test` с `display: table-row-group|header-group|footer-group|row|column|column-group|cell|caption` и ждёт зелёный квадрат. 60 id по правилу отнесения («applies-to» + `display: table-*` + `float|clear|position:absolute` в стиле); на `left-applies-to-001.xht` снимок: у теста зелёный квадрат 96×96 в `(8,26)`, у эталона — в `(0,50)` (поля `p {margin:1em 8px}` эталона — разница в сдвиге по `y` из-за UA-полей `<p>`, то есть BUG-1334; сама блокификация пробой в этом id не отделена). Верхняя граница — часть id пересекается с BUG-1334 (UA-поля у `<p>`).

## Что делать

В вычислении `display` применять таблицу блокификации §9.7 для float и abspos/fixed ко всем `table-*` (кроме `table`/`inline-table`, которые становятся `table`).

## Как проверить

`css/CSS2/positioning/left-applies-to-001.xht`, `floats-clear/float-applies-to-001.xht`, `bottom-applies-to-001.xht`.
