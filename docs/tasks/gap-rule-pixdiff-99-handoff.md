# Срез 99 css-gaps — заметка передачи (ветка `p4-gap-rule-pixdiff-99`)

Удалить файл в закрывающем коммите среза. Код ещё не менялся: сделан разбор и пробы Edge.

## Базовая линия (800×600, `python .tmp/gaps_pixdiff.py "" .tmp/gaps_base99.json`, 7 мин)

304 identical, 92 no-match-ref, не identical: `flex/fragmentation/008` (2500 px), `grid-gap-decorations-024/025/027` и
`-visited-alpha-clamp` (`:visited`, пропускать), `subgrid/fragmentation/002` (thin-only 30 px, это AA, не геометрия),
multicol `007` (94240 px, вложенный rows-multicol со спаннером, тяжело), `multicol-gap-decorations-writing-mode` и
`...multi-value-writing-mode` (вертикальный multicol в Lumen не поддержан вообще — не брать).
`column-rule-inset-interpolation`/`row-rule-inset-interpolation`: 4+4 субтеста в `.ini` — опечатка самого теста
(`calc(-0% + 10px)` только у `cap-end`, у остальных `calc(0% + 10px)`), чинить нечем.

## Выбранная цель: `flex/fragmentation/008` (row-wrap flex + `break-before: column` в multicol)

Сейчас `emit_grid_fragments` (`crates/engine/layout/src/box_tree/multicol_trampoline.rs`, ~1140) для forced-разрыва
при `column-fill: balance` возвращает `false` (атомарный путь) — flex остаётся одной колонкой.

### Что делает Edge (пробы `.tmp/edge/p008_*.html`, `q8_*.html`, `python .tmp/edge/rects.py <html> '#flexbox, #flexbox > div'`)

Контейнер: `display:flex; flex-wrap:wrap; width:90px; height:H; border:2px; gap:10px`, 4 item'а 30×30,
`break-before: column` у всех, multicol `columns:2; height:90px` (limit = 90).

1. **Коробка контейнера режется ровно по границам колонок** (кратным limit), forced-разрыв её не сдвигает:
   H=170 → фрагменты 90 и 84 (=174−90); H=120 → 90 и 34; H=230 → 90, 90, 54; limit 60 → 60, 60, 54.
   Не зависит от `column-fill` (auto и balance дают то же); при limit > высоты коробки второй фрагмент имеет h=0.
2. **Item'ы строки после forced-разрыва** (item 3, 4) лежат в следующей колонке на `y = flex_y − c`, где
   `c = (низ item'ов строк до разрыва) + row-gap` = натуральный конец предыдущей строки + gap (у 008: 32+10 = 42),
   `flex_y` — позиция строки в растянутой раскладке (92). Проверено: высота item'а 20/30/50, row-gap 10/20,
   border 0/2/6, H 120/170/230, limit 60/90/200 — формула держится (H=170: y=50; item'ы режутся границей колонки,
   остаток уходит в следующую колонку).
   Эквивалентно: item'ы строк ≥ разрыва сдвигаются на `δ = limit − c` (если `c < limit`) в координатах контейнера
   и затем режутся обычными окнами по `limit`.
3. **Правила**: column-rule рисуется по экстенту item'ов (30 px: `top 2` в 1-й колонке, `top 50` во 2-й, `left 32`/`205`),
   а не по растянутой строке (80 px); row-rule (золотой) не рисуется — в каждом фрагменте по одной строке.

### План реализации (в `emit_grid_fragments`)

- После `forced = forced_row_starts(..)`: если `!col_flex`, `display: flex`, `forced.len() == 1`, `total_h > limit`,
  то `c = max(низ item'ов с top < fs, + margin-bottom) + row_gap`; при `c < limit` — «квирк»:
  клон `grid` с item'ами строк ≥ fs, сдвинутыми `super::shift_tree(c, 0.0, limit - c)`.
- Для квирка: `forced = []` (разрыв окон не делать), `cut_at = limit` даже при `balance` (снять `return false` для
  `!forced.is_empty() && balance`), в `grid_windows` передать единственный трек `[(0, total_h)]` (без «сброса» щели
  на границе колонки: коробка режется ровно по 90), а в `rows` для рисования — натуральные экстенты item'ов каждой
  строки `(min top, max bottom)` с тем же сдвигом (тогда `flex_line_gaps` и `visible` дают правила 30 px).
- `total_h` считать по исходным `rows` (174), не по сдвинутым.
- Проверить: `python .tmp/gaps_pixdiff.py flex/fragmentation .tmp/f99.json` → `008` identical, остальные
  `flex/fragmentation/*` не хуже (сейчас все identical кроме 008); затем полный прогон css-gaps (ожидание 304 → 305),
  clippy `-p lumen-layout`, тест `multicol_grid_fragments` + новый тест на 008-геометрию (item 3 на y=50 во 2-й колонке,
  фрагменты 90 и 84).
- Сборка: бинарь `target/dev-release/lumen.exe` в слоте p4-work уже есть (собран на срезе 98); пересобрать
  `cargo build --profile dev-release --bin lumen` после правки (см. память «Сборка при полном диске»).
- Закрытие: запись «Срез 99» в `CSS-SPECS.md:113` (в конец списка срезов, перед `BUG-553`), строка в
  `subsystems/layout.md`, удалить этот файл, `lumen-task-finish`; указатель `CSS-SPECS.md:113` в `STATUS-P4.MD` не
  удалять, пока остаются нерешённые тесты (это строка «срезов»: при завершении удаляется строка только если
  строка CSS Gap Decorations закрыта целиком — решать по тексту задачи).
