# Задача: CSS Subgrid (grid-template-rows/columns: subgrid)

**Developer:** P1
**Ветка:** `p1-subgrid`
**Размер:** XS (в основном верификация; фича фактически завершена)
**Крейты:** `lumen-layout` (при необходимости `lumen-css-parser`)

## Goal
`grid-template-columns: subgrid` / `grid-template-rows: subgrid` заставляет вложенный grid переиспользовать треки родителя вместо задания собственных (CSS Grid L2 §9). По спеку это уже должно работать end-to-end.

## Current state (сверено с кодом 2026-07-05)
Роадмап-семя (ROADMAP.md:128) помечает задачу как PARTIAL «дошить CSS-проводку keyword subgrid в каскад». **Эта пометка протухла** — проводка уже есть. Проверено по коду:

- **Parsing keyword.** `GridTrackSize::parse_track_list()` — `crates/engine/layout/src/style.rs:4708-4717`: строка `subgrid` (case-insensitive) даёт `vec![GridTrackSize::Subgrid]` (сентинел).
- **Enum-вариант.** `GridTrackSize::Subgrid` — `crates/engine/layout/src/style.rs:4626`; хелпер `is_subgrid()` — `style.rs:4657-4659`; `resolve_fixed()` возвращает `None` для `Subgrid` (откладывает на layout) — `style.rs:4638-4644`.
- **Каскад-проводка.** `apply_declaration`: `grid-template-columns` — `style.rs:12042-12050`, `grid-template-rows` — `style.rs:12052-12060`. Оба зовут `parse_track_list`. **Проводка ЗАВЕРШЕНА.**
- **ComputedStyle.** Поля `grid_template_columns` / `grid_template_rows: Vec<GridTrackSize>` — `style.rs:2983, 2986`.
- **Layout-алгоритм.** `crates/engine/layout/src/subgrid.rs` (весь файл): `SubgridContext` (24-43), `from_parent_tracks` (35-43), thread-local `SUBGRID_COL_CTX`/`SUBGRID_ROW_CTX` (57-64), RAII `SubgridContextGuard` (70-87), `collect_subgrid_items` (113).
- **Интеграция в grid.** `lay_out_grid` — `crates/engine/layout/src/box_tree.rs:8501`: чтение сентинела и наследуемых треков (8517-8518, 8577-8586), настройка контекста для детей (8883-8909), использование наследованных ширин/высот (8783-8798, 8850-8864), релейаут (8995-9011).
- **Тесты уже есть.** `crates/engine/layout/src/lib.rs:13956-14063` (parse columns/rows, column layout, collect items) + юнит-тесты в `subgrid.rs:147-189`.
- **CSS-SPECS.md:482** — `subgrid` = 🟡, layout ✅, parsing ✅.

Вывод: фича реализована. Остаток — верификация «на живой странице» + графический тест + актуализация статусов (🟡→✅).

## Entry points
- `crates/engine/layout/src/style.rs:4708` — `parse_track_list` (keyword → сентинел)
- `crates/engine/layout/src/style.rs:12042` — каскад grid-template-*
- `crates/engine/layout/src/subgrid.rs:24` — контекст наследования треков
- `crates/engine/layout/src/box_tree.rs:8501` — `lay_out_grid`, потребитель сентинела

## Срезы (декомпозиция на мелкие задачи)
### Срез 1 — XS — Верификация end-to-end
Собрать тестовую страницу с родительским grid и вложенным `subgrid`, прогнать `--dump-layout`. Убедиться, что колонки/строки ребёнка совпадают с треками родителя. Зафиксировать реальные наблюдаемые баги (если есть) как BUG-NNN — иначе задача сводится к обновлению статусов.

### Срез 2 — XS — Крайние случаи субгрида
Проверить сочетания, которые могли не покрыться: `subgrid` только по одной оси (columns subgrid + rows фиксированные); `grid-column: span N` у ребёнка; gaps родителя vs ребёнка; вложенность subgrid в subgrid. На любой явный дефект — BUG-NNN (P3), не чинить в этом брифе, если это отдельный баг.

### Срез 3 — XS — Графический тест
Добавить демо subgrid в подходящий `graphic_tests/NN-*.html` (grid-раздел) + в `graphic_tests/1000000-final.html`, запись в `TESTS` (`graphic_tests/run.py`) и в `graphic_tests/COVERAGE.md`. Магента-рамка по паттерну.

### Срез 4 — XS — Синхронизация доков
`CSS-SPECS.md:482` 🟡→✅ (если верификация чистая); строку `subgrid` в CSS-SPECS priority-queue (`723`) и ROADMAP.md:128 — снять пометку PARTIAL/`planned`→`done` (правит владелец ROADMAP через `gen_roadmap.py`, не этим брифом). `CAPABILITIES.md` — grid-подсистема.

## Tests
- Юнит: уже есть в `lib.rs:13956-14063` и `subgrid.rs:147-189` — прогнать `cargo test -p lumen-layout`.
- Графический: новое демо subgrid (Срез 3), порог 0.5%.

## Definition of done
- [ ] End-to-end проверка subgrid на живой странице (`--dump-layout`) без регрессий
- [ ] Проверены крайние случаи (одна ось, span, gaps, вложенность); дефекты заведены как BUG-NNN
- [ ] Графический тест добавлен (NN + final + COVERAGE + run.py)
- [ ] `cargo test -p lumen-layout` зелёный
- [ ] Статусы синхронизированы (CSS-SPECS 🟡→✅, CAPABILITIES, ROADMAP через владельца)
