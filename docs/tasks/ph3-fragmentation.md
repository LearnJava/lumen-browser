# Задача: CSS Fragmentation — `break-inside` / `widows` / `orphans`

**Developer:** P1
**Ветка:** `p1-fragmentation`
**Размер:** M
**Крейты:** `lumen-layout`

## Goal

Реализовать применение `break-inside: avoid`, `orphans` и `widows` (CSS Fragmentation L3 §3.3–§4) в алгоритме печатной пагинации: не разрывать блок между страницами, когда `break-inside: avoid`, и удерживать минимум `orphans` строк в конце страницы / `widows` строк в начале следующей.

## Current state (сверено с кодом 2026-07-05)

ROADMAP помечает PARTIAL «применяются в pagination.rs». **Это не так**: `break_inside`/`widows`/`orphans` только парсятся и хранятся, но в алгоритме пагинации НЕ применяются — `pagination.rs` использует лишь `break-before`/`break-after`.

- **Парсинг + ComputedStyle** готовы:
  - `break-before/after/inside` → `style.rs:13013`, `13018`, `13023`; enum `BreakValue` — `style.rs:1554`; поля — `style.rs:2760`–`2762`; парсер `parse_break_value` — `style.rs:19239` (`auto`/`avoid`/`always`/`page`/`column`/`region`).
  - `orphans`/`widows` → `style.rs:13026`, `13034`; поля `orphans: u32`, `widows: u32` — `style.rs:3037`, `3041`; **наследуются**, initial 2 — `style.rs:5977`, `7064`.
  - Регистрация в css-parser — `crates/engine/css-parser/src/lib.rs`.
- **Pagination** — `crates/engine/layout/src/pagination.rs`:
  - `paginate()` (`pagination.rs:112`) разбивает **только** прямых детей корня по высоте и по `break-before`/`break-after`.
  - Хелперы `should_break_before` (`:230`), `should_break_after` (`:238`), `should_avoid_break_before` (`:246`), `should_avoid_break_after` (`:254`) — но `avoid`-хелперы помечены `#[allow(dead_code)]` и почти не влияют (в `:148` `should_avoid_break_after` лишь провоцирует новый page, не сохраняет неразрывность).
  - Шапка честно признаёт: «Single-page assumed (no break-inside handling yet)» (`pagination.rs:111`).
  - `break_inside`, `widows`, `orphans` в `pagination.rs` **не читаются вообще** (grep пуст).
- **Тесты**: unit в `pagination.rs` покрывают только `BreakValue` matcher-логику и размеры контекста; в `style.rs` — парсинг/наследование orphans/widows (`:26960`+).

**Остаток (≈45%):** алгоритм break-inside/orphans/widows целиком; строчная фрагментация (для widows/orphans нужен доступ к строкам `InlineRun` внутри блока-кандидата на разрыв).

Scope: **только печатная пагинация** (@media print). Multicol/regions — вне задачи (в CSS-SPECS помечены отдельно).

## Entry points

- `crates/engine/layout/src/pagination.rs:112` — `paginate()`, главный цикл разбиения.
- `crates/engine/layout/src/pagination.rs:230`–`256` — break-хелперы (расширить `break_inside`).
- `crates/engine/layout/src/style.rs:2762` — `break_inside`; `:3037/3041` — `orphans`/`widows`.
- `BoxKind::InlineRun { lines, .. }` (в `box_tree.rs`) — источник строк для widows/orphans (доступ к `lines: Vec<Vec<InlineFrag>>` и их y).

## Срезы (декомпозиция)

### Срез 1 — XS — хелпер `break_inside`
Добавить `should_avoid_break_inside(box) -> bool` (`BreakValue::Avoid`) рядом с существующими (`pagination.rs:246`). Снять `#[allow(dead_code)]` там, где хелпер станет живым.

### Срез 2 — S — применение `break-inside: avoid`
В `paginate()` (`pagination.rs:139`), когда бокс не влезает в остаток страницы И `should_avoid_break_inside(child)` И бокс сам ≤ высоты страницы — переносить его целиком на новую страницу (не разрывать), даже если частично поместился бы. Unit-тест: блок с `break-inside: avoid`, не влезающий в остаток, уходит на следующую страницу целиком.

### Срез 3 — M — orphans/widows на строках
Для блока, который **разрывается** между страницами (без `avoid`), считать строки его `InlineRun`-детей: точку разрыва двигать так, чтобы ≥ `orphans` строк осталось на текущей странице и ≥ `widows` строк ушло на следующую; если удержать нельзя — перенести весь блок. Это требует прохода по `lines` внутри `PageFragment` и порождения частичных фрагментов (или отказа от разрыва). Unit-тесты: orphans=3 не оставляет 1–2 строки; widows=3 не переносит 1–2 строки.

### Срез 4 — S — сцепка avoid-цепочек
`break-before: avoid` / `break-after: avoid` между соседними блоками (заголовок + первый абзац): удерживать пару вместе, если возможно. Расширить проверку в главном цикле. Unit-тест на связку h2+p.

### Срез 5 — XS — тест-страница
`graphic_tests/NN-fragmentation.html` не обязателен (пагинация — печатный путь, не оконный рендер). Вместо этого — интеграционные тесты `paginate()` на синтетическом дереве; при наличии печатного дампа добавить проверку числа страниц. Обновить `COVERAGE.md`, если добавляется визуальный артефакт.

## Tests

- Unit `pagination.rs`: break-inside:avoid перенос; orphans/widows пороги; avoid-сцепка соседей.
- Строчный доступ проверять на дереве с известным числом строк (fixed line-height).

## Definition of done

- [ ] `break-inside: avoid` переносит неразрывный блок целиком (срез 2)
- [ ] `orphans`/`widows` соблюдаются при разрыве блока (срез 3)
- [ ] `break-before/after: avoid`-сцепка соседей работает (срез 4)
- [ ] Живые unit-тесты в `pagination.rs`; удалены неуместные `#[allow(dead_code)]`
- [ ] `cargo clippy -p lumen-layout --all-targets -- -D warnings` чистый
- [ ] `CSS-SPECS.md:534/535` (`break-inside`, `widows`/`orphans` → ✅ для print) и `CAPABILITIES.md` обновлены; шапка `pagination.rs:111` («no break-inside handling yet») переписана
