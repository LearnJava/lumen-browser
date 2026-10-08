# BUG-1362 — Анонимные табличные боксы (CSS 2.1 §17.2.1) не строятся: осиротевшие `table-cell`/`table-row` не получают таблицу-обёртку, текст внутри `table`/`row` пропадает

**Статус:** OPEN (ДОРАБОТКА → TABLE-ANON)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 13, `css/CSS2` (tables, positioning, floats, floats-clear, abspos, stacking-context, zindex, zorder))
**Область:** layout (`crates/engine/layout/src/box_tree/build.rs`, `table.rs` — `grep -i anonymous table.rs` пуст; в `build.rs:1011` — «Tables keep their own anonymous-box rules (text → anonymous cell)», но кода нет)

функциональности нет вовсе (три стадии §17.2.1 не реализованы), объём — алгоритм, не точечная правка.

## Симптом

`--dump-layout` / `--dump-display-list`, Ahem 20 px, контейнер 500 px:

| разметка | получено | ожидается |
|---|---|---|
| `<div><span style="display:table-cell">A</span><span style="display:table-cell">B</span></div>` (осиротевшие ячейки) | две ячейки как блоки: `y=0` и `y=24.2`, ширина 500 | одна анонимная таблица, ячейки в ряд (`x=0` и `x=250`) |
| `<div><span style="display:table-row">A</span><span style="display:table-row">B</span></div>` | два `TableRow` высоты 0, текст не рисуется | анонимная таблица, 2 строки |
| `<div style="display:table">TXT</div>` | `Table` высоты 0, **`DrawText` нет** | анонимные строка+ячейка с текстом |
| `<div style="display:table"><div style="display:table-row">TXT</div></div>` | `TableRow` без детей, `DrawText` нет | анонимная ячейка с текстом |
| `<table><tr>TXT<td>AA</td></tr></table>` | `TXT` теряется | анонимная ячейка |
| `<div style="display:table"><span display:table-cell>A</span><span display:table-cell>B</span></div>` | две ячейки `w=0` без строки | анонимная строка вокруг ячеек |
| `<div><span style="display:table-row"><span display:table-cell>A</span><span display:table-cell>B</span></span></div>` | работает: `TableRow` с двумя ячейками по 250 | — (это единственный рабочий случай) |

## Как найдено

WPT-RUN-14 срез 13: 153 id `tables/table-anonymous-objects-*` (все 153 `thick`; эталоны — `no_red_3x3_monospace_table-ref.xht`, `no_red_antialiasing_a_bc_d-ref.xht` и т.п.: тест строит ту же сетку из `span`/`div` с `display:table-*`, эталон — из настоящего `<table>`). A/B с `table{width:fit-content}`: 16 из 153 → `identical`/`thin-only` (остальные — отсутствие анонимных боксов). 78 из них запускают JS в `onload` (вставка/удаление узлов) — после реализации сверить отдельно.

## Что делать

Задача TABLE-ANON (`ROADMAP.md`): три стадии §17.2.1 — (1) удалить игнорируемые боксы, (2) обернуть «не-табличных» детей `table`/`row-group`/`row` в анонимные `row`/`cell`, (3) обернуть осиротевшие `table-cell`/`row`/`row-group`/`caption` в анонимную таблицу, соседние — в одну. Построение боксов — в `build.rs`, раскладка уже умеет таблицу из корректного дерева.

## Как проверить

`css/CSS2/tables/table-anonymous-objects-001.xht`, `-009.xht`, `-017.xht`, `-059.xht`.
