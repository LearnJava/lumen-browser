# BUG-067

**Статус:** FIXED 2026-06-08
**Компонент:** js
**Файл:** `crates/js/src/dom.rs (WEB_API_SHIM)`

## Описание

document_pip_* тесты падали: WEB_API_SHIM определял Event, но не глобальный EventTarget → `class X extends EventTarget` бросал «EventTarget is not defined». Добавлен функциональный EventTarget в WEB_API_SHIM
