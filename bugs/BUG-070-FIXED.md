# BUG-070

**Статус:** FIXED 2026-06-08
**Компонент:** js
**Файл:** `crates/js/src/dom.rs (WEB_API_SHIM)`

## Описание

Дубликат BUG-067 (тот же корень: отсутствовал глобальный EventTarget). Исправлено вместе с BUG-067 — добавлен EventTarget в WEB_API_SHIM
