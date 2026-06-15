# BUG-147

**Статус:** FIXED 2026-06-12
**Компонент:** shell
**Файл:** `crates/shell/src/main.rs:73`

## Описание

clippy -D warnings fails on main: redundant `use lumen_js;`, dead code collect_import_map (A-8 заявил интеграцию import maps, но вызов не добавил — доведена проводка: collect_import_map → новый QuickJsRuntime::set_import_map перед eval_module), 4× unnecessary f32 cast
