# BUG-068

**Статус:** FIXED 2026-06-08
**Компонент:** shell
**Файл:** `crates/shell/src/reader_view.rs:292`

## Описание

clippy lumen-shell: reader_view.rs:292,309 «collapsible_if» — два nested if-let схлопываемы; pre-existing с D-3, блокирует clippy -D warnings
