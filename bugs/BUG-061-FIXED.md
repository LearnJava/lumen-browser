# BUG-061

**Статус:** FIXED 2026-06-04
**Компонент:** driver
**Файл:** `crates/driver/tests/test_32.rs:30`

## Описание

test_32_list_markers падал (ожидал 22 li, получал 26): коммит d70391d9 (C9) добавил 2 новые секции в 32-list-markers.html (custom-marker + content-marker), не обновив тест; ожидания обновлены до 26 li / 24 маркеров
