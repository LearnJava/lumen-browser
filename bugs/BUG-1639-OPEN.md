# BUG-1639 — `FontFace`: `self._rejectLoaded is not a function`

**Статус:** OPEN
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
