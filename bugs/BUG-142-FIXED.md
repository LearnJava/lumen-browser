# BUG-142

**Статус:** FIXED 2026-06-17
**Компонент:** paint/shadow-dom (cascade + html-parser)
**Файлы:** `crates/engine/layout/src/style.rs`, `crates/engine/layout/src/box_tree.rs`, `crates/engine/html-parser/src/tree_builder.rs`

## Описание

`:host` / `::slotted` rendering diverged from Edge — TEST-72: 11.24% (CSS Scoping L1 §6.1-6.2).
Все три shadow-хоста красились одним цветом (#3366cc), `:host(.special)`/`:host(.missing)`
не учитывались, slotted-контент не рисовался.

## Корень (две причины)

1. **Каскад без скоупинга.** Стили внутри shadow-tree (`<style>` в declarative shadow
   `<template>`) вообще не собирались, а document-scope `:host`/`::slotted` из `<head>`
   `<style>` матчились на любой shadow-хост / slotted-элемент. Так host2 (нужен
   `:host(.special)` → #996600) и host3 (нужен white, `:host(.missing)` не совпадает)
   получали #3366cc от глобального `:host`.

2. **Парсер терял `<slot>`.** В declarative shadow `<template>` после rawtext-элемента
   `<style>` insertion mode оставался `InHead` (через `original_insertion_mode`), поэтому
   следующий `<slot>` обрабатывался в head-контексте и не попадал в shadow root → slot
   distribution пустая → slotted-дети (host3 red-box #cc3300) не раскладывались.

## Фикс

- **`box_tree.rs`**: `build_shadow_sheets(doc)` собирает per-host author-stylesheet из
  `<style>` каждого shadow root; устанавливается через `set_shadow_sheets` в начале каждого
  layout-прохода (3 точки входа).
- **`style.rs`**: thread-local `SHADOW_SHEETS` + `SHADOW_HOST_SCOPE`. `:host` матчится только
  когда `SHADOW_HOST_SCOPE == node` (document-scope `:host` — no-op). В `compute_style` добавлены
  scoped-проходы: (a) `:host`/`:host()` из собственного shadow-листа узла-хоста, (b) `::slotted()`
  из shadow-листа хоста для slotted-детей. Document-проход `::slotted` удалён.
- **`tree_builder.rs`**: после делегирования rawtext head-тега в `mode_in_template` корректируем
  `original_insertion_mode` `InHead` → `InTemplate`, чтобы конец `</style>` возвращал парсер в
  template-content mode.

## Регресс-тесты

- `style::shadow_dom_selectors::*` (8 тестов, в т.ч. `host_rule_in_document_scope_is_noop`,
  `slotted_rule_in_document_scope_is_noop`) — каскад-скоупинг.
- `tree_builder::tests::declarative_shadow_dom_slot_after_style_preserved` — парсер.
- TEST-72 (graphic): 11.24% → 0.00%.
