# BUG-1318 — subgrid не передаёт вклад своих элементов в авто-дорожки родителя

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs`, реализация subgrid — «subgrid layout algorithm ✅» в CSS-SPECS; наследование дорожек есть, обратного вклада размеров нет — CSS Grid L2 §9, «Subgrid Item Contribution»)

## Симптом

`--dump-layout`:

| разметка | получено | ожидается |
|---|---|---|
| `.o{display:inline-grid;grid-template-columns:auto auto}`, внутри `.s{grid-column:1/3;display:grid;grid-template-columns:subgrid}` с двумя детьми `width:30px` | ширина `.o` 30 (второй ребёнок на x = 15) | 60 |
| контроль: те же дети **без** subgrid (прямо в `.o`) | ширина 60, дети на x = 0 и 30 | 60 |
| то же, дети `width:30px; padding:0 20px` (ширина 70) | ширина `.o` 70, дорожки по 35: второй ребёнок на x = 35 (перекрытие) | 140 |

Контрольная строка показывает, что дефект именно в subgrid: тот же контент напрямую в родителе даёт верный результат.

## Как найдено

WPT-RUN-14 срез 8, `css/css-grid/subgrid/`: 39 reftest + 1 testharness (`alignment-in-subgridded-axes-001`, 16 сабтестов). По группам: `standalone-axis-size-*` 10, `grid-gap-*` 12 (с `larger/smaller/normal`), `auto-track-sizing-*` 4, `subgrid-no-items-on-edges-*` 2, `item-percentage-height-001`, `independent-formatting-context`, `contribution-size-flex-tracks-001`, `subgrid-stretch`, `subgrid-button`, `sticky-subgrid-item`, `overflow-hidden-does-not-prohibit-subgrid`, `subgrid-item-block-size-001`, `subgrid-scroller-auto-height-padding`, `subgrid-item-with-margin-left-auto`. Все 39 reftest — `thick` (`--viewport 800x600 --ahem`).

## Что делать

В расчёте размеров авто-дорожек родителя заменять subgrid на его элементы (каждый — со своим span в координатах родителя), добавляя к вкладу крайних элементов `margin/border/padding` самого subgrid на краю (CSS Grid L2 §9, «extra margin»). По пробам сейчас вклад subgrid — как у одного элемента (первая дорожка 30, вторая 0); код не читался.

## Как проверить

`css/css-grid/subgrid/grid-gap-001.html` (reftest), `auto-track-sizing-001.html`, `standalone-axis-size-002.html`.
