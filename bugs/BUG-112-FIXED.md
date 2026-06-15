# BUG-112

**Статус:** FIXED 2026-06-08
**Компонент:** driver
**Файл:** `crates/driver/tests/test_32.rs`

## Описание

test_32_list_markers регрессия: P4 добавил 2 `@counter-style` списка по 3 items в 32-list-markers.html → 32 li (было 26), 30 маркеров (было 24). Тест не обновлён.
