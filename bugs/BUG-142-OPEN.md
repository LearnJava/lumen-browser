# BUG-142

**Статус:** OPEN
**Компонент:** paint/shadow-dom
**Файл:** `crates/engine/layout/src/style.rs`

## Описание

:host / ::slotted rendering diverges — TEST-72: 11.24% (thr 0.5%); CSS Scoping L1 §6.1-6.2; selectors parse and compute but shadow host background and ::slotted child colours do not match Edge; likely cascade specificity or slotted-element paint-order issue
