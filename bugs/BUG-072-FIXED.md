# BUG-072

**Статус:** FIXED 2026-06-08
**Компонент:** js
**Файл:** `crates/js/src/form_validation.rs:169`

## Описание

Form Constraint Validation API init failed: FORM_VALIDATION_SHIM ссылается на bare `HTMLInputElement`/`HTMLTextAreaElement`/`HTMLSelectElement`/`HTMLButtonElement` (строки 169-172) — в install_dom эти конструкторы не определены глобально → ReferenceError «HTMLInputElement is not defined», шим не устанавливается. Нужны typeof-гварды
