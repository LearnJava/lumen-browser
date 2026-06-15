# BUG-050

**Статус:** FIXED 2026-05-31
**Компонент:** network
**Файл:** `crates/network/src/mock.rs:9`

## Описание

doctest mock.rs:16 не компилировался — fetch() is a trait method, но `use NetworkTransport` не импортирован в примере → добавлен импорт
