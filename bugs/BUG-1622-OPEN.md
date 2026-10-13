# BUG-1622 — KeyboardEvent странице: keyCode 0, без модификаторов, code=символ

**Статус:** OPEN
**Заведён:** 2026-10-12 (координационная сессия: сверка задач по `docs/research/` с кодом на `main`)
**Область:** shell/js (`lumen/text_input.rs:130`, `frame_text_input.rs:392`, `shim/web_api_shim_mid.js:1703`)

## Симптом и место в коде

Шим объявляет _lumen_dispatch_key_event(start_nid, type, key, code, keyCode, location, mod, repeat, isComposing) (web_api_shim_mid.js:1703-1714), а все четыре вызывающих передают «key, code, false, false, false, false», то есть по старой схеме ctrl/shift/alt/meta: lumen/text_input.rs:130 и :640, lumen/frame_text_input.rs:392, crates/driver/src/session.rs:803. В итоге keyCode и which = 0 (конструктор KeyboardEvent, web_api_shim_mid.js:113-115), ctrlKey/shiftKey/altKey всегда false, location 0. Для набранного символа dispatch_injected_key передаёт key дважды (text_input.rs:641), поэтому code = «с», а не «KeyC». Ломаются обработчики на e.keyCode/e.which (jQuery, проверка Enter=13) и на e.shiftKey/e.code. Существующего бага нет (grep keyCode по BUGS*.md пуст).

## Где смотреть

- crates/js/src/shim/web_api_shim_mid.js:1703
- crates/js/src/shim/web_api_shim_mid.js:113
- crates/shell/src/lumen/text_input.rs:130
- crates/shell/src/lumen/text_input.rs:640
- crates/shell/src/lumen/frame_text_input.rs:392
- crates/driver/src/session.rs:803

## Объём и приёмка

Все вызовы _lumen_dispatch_key_event передают 8 аргументов как (nid,type,key,code,ctrl,shift,alt,meta) — text_input.rs:130, :640, frame_text_input.rs:392, driver/session.rs:803, — а шим ждёт (…,code,keyCode,location,mod,repeat,isComposing) (web_api_shim_mid.js:1703): keyCode/which = 0 у каждой клавиши, ctrl/shift/alt всегда false; для набранного символа code = сам символ (text_input.rs:641 передаёт key дважды) — «с» вместо «KeyC». Ломает обработчики на e.keyCode/e.which (jQuery-формы, Enter=13) и e.shiftKey. Приёмка: в поле набор «С» с Shift в RU → keydown key «С», code «KeyC», keyCode 67, shiftKey true. Баг для BUGS.md (P3). docs/research/browser-ux/report.md §4.2 INPUT-18

## Источник

Найдено при сверке задач по исследованиям [`docs/research/`](../docs/research/README.md) (уровень 0 «сайт работает» и UX-мелочи), решение — [ADR-034](../docs/decisions/ADR-034-site-compatibility-strategy.md). Номера строк сняты на `main` 2026-10-12 — перед правкой перепроверить.
