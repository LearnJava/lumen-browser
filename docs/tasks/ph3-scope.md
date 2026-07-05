# Задача: CSS @scope (scoped styling + donut scope)

**Developer:** P1
**Ветка:** `p1-scope`
**Размер:** S (основной остаток — proximity в каскад-сортировке)
**Крейты:** `lumen-css-parser`, `lumen-layout` (style.rs каскад)

## Goal
`@scope (.root) { ... }` ограничивает правила поддеревом; `@scope (.root) to (.limit) { ... }` создаёт «пончик» (исключает поддерево limit). При конфликте нескольких scope-правил выигрывает ближайший scope-root (scope proximity, CSS Cascade L6 §5.2).

## Current state (сверено с кодом 2026-07-05)
Роадмап-семя (ROADMAP.md:131) помечает как PARTIAL «root-matching готов; дошить donut/limit». По коду **donut/limit уже реализован**; реально отсутствует **scope proximity в сортировке каскада**.

### Готово ✅
- **Парсинг at-rule.** `parse_scope_rule` — `crates/engine/css-parser/src/parser.rs:2820-2915`: парсит `(<root>)` (2825-2846) и `to (<limit>)` (2849-2880, лимит хранится в 2875). Тест `at_scope_root_and_limit` — `parser.rs:6662` (проверяет `.card` + `to (.footer)`).
- **Структура.** `ScopeRule` — `parser.rs:944-951`: `root: String` (947), `limit: Option<String>` (949), `rules: Vec<Rule>` (950).
- **Root-matching.** `node_is_in_scope` — `crates/engine/layout/src/style.rs:10305-10328`: идёт вверх по предкам, матчит root; пустой root → всегда true.
- **Donut/limit.** Каскад-цикл — `style.rs:6357-6389`: проверка root (6359), **исключение пончика** — если узел матчит `limit`, scope-правила пропускаются (6365-6369). Реализовано и рабочее.
- **CSS-SPECS.md:612** — `@scope` = ✅ (`parse_scope_rule` parser.rs:2346 — примечание: реальная строка `2820`, ссылка в доке протухла); **CSS-SPECS.md:116** — «@scope root matching ✅; limit/inner-scope — Phase 2» (протухло, limit готов).

### Отсутствует ⬜
- **Scope proximity в сортировке.** Ключ сортировки каскада — `style.rs:6482-6483`: кортеж `(imp, inline, lp, spec, rule_idx, decl_idx)` — **без дистанции scope-вложенности**. Декларации scope-правил добавляются с `next_rule_idx` (порядок в таблице стилей), `style.rs:6384`. Значит при конфликте двух scope с разной глубиной root решает source-order/специфичность, а НЕ близость scope-root, как требует спек.
  - Пример падения: `@scope (.outer) { .t{color:red} }` и `@scope (.outer .inner) { .t{color:blue} }` для `.t` внутри `.outer .inner` → спек: blue (ближе), реализация: по rule_idx.
- Тестов на многоуровневую вложенность scope нет.

## Entry points
- `crates/engine/css-parser/src/parser.rs:2820` — `parse_scope_rule` (root + limit)
- `crates/engine/css-parser/src/parser.rs:944` — struct `ScopeRule`
- `crates/engine/layout/src/style.rs:6357` — применение scope-правил в каскаде
- `crates/engine/layout/src/style.rs:10305` — `node_is_in_scope`
- `crates/engine/layout/src/style.rs:6482` — ключ сортировки каскада (сюда добавлять proximity-тир)

## Срезы (декомпозиция на мелкие задачи)
### Срез 1 — XS — Верификация текущего поведения root+donut
Собрать страницу с `@scope (.a) { ... }` и `@scope (.a) to (.b) { ... }`. Прогнать, убедиться что root-scope и пончик работают (узлы внутри `.b` не получают стилей scope). Зафиксировать baseline. Юнит-тесты на donut, если их нет (`style.rs`).

### Срез 2 — S — Вычисление scope proximity
Добавить вычисление «дистанции» узла до scope-root: число шагов вверх по предкам до ближайшего элемента, матчащего root (в `node_is_in_scope` / рядом, `style.rs:10305`). Вернуть эту дистанцию наружу (сейчас функция возвращает `bool`). Меньшая дистанция = выше приоритет.

### Срез 3 — S — Proximity в ключ сортировки
Расширить кортеж сортировки (`style.rs:6482-6483`) полем scope-proximity, вставив его на правильном месте согласно CSS Cascade L6 §5.2 (после layer, до/вокруг specificity — сверить порядок по спеку: proximity сравнивается ПОСЛЕ specificity нормальных правил, но между scope-правилами по близости). Пробросить дистанцию из Среза 2 в `matched.push` (`style.rs:6384`) для scope-деклараций; не-scope правила получают нейтральное значение.

### Срез 4 — S — Тесты proximity
Юнит-тесты на конфликт вложенных scope (ближний root выигрывает); тест на равную дистанцию → source-order. Файлы: `crates/engine/layout/src/` (каскад-тесты).

### Срез 5 — XS — Графический тест + доки
Демо @scope (root + donut + вложенность) в `graphic_tests/NN-*.html` + `1000000-final.html` + `run.py`/`COVERAGE.md`. Синхронизировать `CSS-SPECS.md:116` (limit ✅, proximity ✅), поправить протухшую ссылку `parse_scope_rule` в `CSS-SPECS.md:612` (2346→2820), ROADMAP.md:131 (владелец), `CAPABILITIES.md`.

## Tests
- Юнит: donut-exclusion (Срез 1), scope proximity конфликты (Срез 4), `cargo test -p lumen-layout` + `cargo test -p lumen-css-parser`.
- Графический: демо @scope (Срез 5), порог 0.5%.

## Definition of done
- [ ] Подтверждено, что root-matching + donut/limit работают (baseline + юнит-тесты)
- [ ] `node_is_in_scope` (или спутник) возвращает дистанцию до scope-root
- [ ] Scope proximity включён в ключ сортировки каскада согласно Cascade L6 §5.2
- [ ] Юнит-тесты на вложенные scope (ближний выигрывает) зелёные
- [ ] Графический тест @scope добавлен; CSS-SPECS:116/612 синхронизированы; ROADMAP/CAPABILITIES обновлены
