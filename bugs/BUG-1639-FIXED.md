# BUG-1639 — `FontFace`: `self._rejectLoaded is not a function`

**Статус:** FIXED 2026-10-19 (P6)
**Заведён:** 2026-10-18 (P6, при разборе [BUG-1239](BUG-1239-FIXED.md))
**Область:** js (`web_api_shim_mid.js`, `_lumen_font_face_try_sources`, строки ~11082–11116)

## Симптом

На загрузке youtube десятки раз `unhandledrejection` с `TypeError: self._rejectLoaded is not a function`.
`_rejectLoaded` инициализируется `null` и присваивается внутри executor'а промиса `loaded`; вызов
происходит раньше, чем он выставлен, или на объекте, у которого `loaded` создаётся лениво/иначе.
Вероятно связано с [BUG-1634](BUG-1634-OPEN.md) (загрузка `@font-face`).

## Что сделать

Найти путь, при котором `_lumen_font_face_try_sources` зовёт `_rejectLoaded` до создания `loaded`; создавать
промис в конструкторе, либо проверять `typeof`. Тест в `crates/js/src/dom/tests/`.

## Исправление (2026-10-19)

Причина — не порядок инициализации, а гонка двух путей: у CSS-связанного лица `_lumen_notify_css_font_loaded` (нативная загрузка шелла) обнуляет `_resolveLoaded`/`_rejectLoaded` после `resolve`, а асинхронный `fetch` из `FontFace.load()` завершался позже и звал `self._rejectLoaded(err)` → `null(...)`, плюс перетирал статус `loaded` на `error`. Теперь `load()` берёт резолверы из замыкания, а запоздалый отказ не меняет уже `loaded` лицо. Тест — `bug1639_late_script_fetch_failure_after_native_settle_is_harmless` (`crates/js/src/dom/tests/v8_fontface_shadow_custom.rs`).
