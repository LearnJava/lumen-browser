# BUG-073

**Статус:** FIXED 2026-06-08
**Компонент:** js
**Файл:** `crates/js/src/dom.rs:10131`

## Описание

chrome_runtime_absent (no_automation_markers.rs) падает: D-6 extension-stub в WEB_API_SHIM безусловно ставит window.chrome.runtime, ломая anti-CDP-detection маркер. Fix: IIFE гардировано флагом `_LUMEN_EXTENSION_ACTIVE`; тесты dom.rs выставляют флаг перед install_dom
