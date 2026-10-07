# BUG-1318 — subgrid не передаёт вклад своих элементов в авто-дорожки родителя

**Статус:** FIXED 2026-10-07 (P6)
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

## Решение (2026-10-07, P6)

Причина оказалась шире описания: движок вообще не считал `auto`-столбцы по содержимому — `auto` получал равную долю свободного места (`auto_col_width = free / n`), а ширина `inline-grid` бралась по схеме «по кругу» и откатывалась на «самого широкого ребёнка», если у кого-то был явный `grid-column`. Контроль «дети прямо в родителе → 60» выходил верным лишь потому, что дети равны. Проба: `display:grid; width:300px; grid-template-columns:auto auto` с детьми 30 и 100 раскладывала столбцы 150/150 вместо 115/185.

Исправление (`box_tree/grid_auto_cols.rs`, новый модуль):
- `place_grid_items` вынесена из `build_grid_init` (авто-размещение L1 §8.5) — теперь ею размещаются и дети subgrid; `grid_item_indices` — общий отбор элементов.
- Вклады: элемент subgrid (`grid-template-columns: subgrid`) заменяется своими детьми в координатах родителя (рекурсивно), краевым детям добавляются margin+border+padding самого subgrid (L2 §9, «extra margin»).
- Размер дорожек по L1 §11.5–11.8: base size / growth limit по одно-дорожечным элементам, затем многодорожечные по возрастанию span (недостающее делится поровну между `auto`-дорожками охвата), свободное место растит дорожки к пределам, остаток растягивает `auto` поровну.
- Применяется, когда все дорожки — `auto` или длины, столбцов не меньше двух, нет `repeat(auto-*)`, нет вертикального `writing-mode`; иначе прежний путь (`fr`, `minmax()`, `min/max-content`).
- Ширина shrink-to-fit (`grid_col_intrinsic_sum`) для сеток с subgrid или явным `grid-column` считается той же функцией по настоящему размещению (раньше — откат на самого широкого ребёнка).

Тесты: `box_tree/tests/grid_subgrid_contribution.rs` (3 строки таблицы из «Симптома», padding самого subgrid на краю, неравные дети, многодорожечный элемент, неравные `auto` без subgrid); два теста BUG-740/1317 в `intrinsic_and_wrap.rs` закрепляли прежнюю «честную заглушку» (откат на самого широкого ребёнка) — их ожидания пересчитаны на верные (80 и 70).

Не сделано: собственный `gap` subgrid не участвует во вкладе (дорожки берут зазор родителя, `with_own_gap` работает только при раскладке); `fr`/`minmax()` по-прежнему идут старым путём — отдельная работа. WPT-`.ini` не пересчитывались.
