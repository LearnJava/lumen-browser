# BUG-1648 — оверлей CSS-переходов/анимаций не знает `!important`; `getAnimations()` вернул объекты без событий и несколько тестов ушли в TIMEOUT

**Статус:** OPEN
**Заведён:** 2026-10-19 (P3, остаток [BUG-1293](BUG-1293-FIXED.md))
**Область:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — `_wa_motion_read`, `_wa_gap_an_value`, `_wa_gap_an_sync`)

## Симптом

1. `!important` в таблице стилей должно перебивать значение `@keyframes` (CSS Cascade 4 §6.1), а оверлей чтения берёт значение анимации, не зная, откуда пришло установившееся. `css/css-animations/important-rules-override-animations.html`: `getComputedStyle(el).color` — `rgb(15, 15, 15)` вместо `rgb(0, 128, 0)`; `!important overrides animation when no transition is running` — `rgb(0, 0, 255)` вместо `rgb(255, 0, 0)`.
2. `getAnimations()` теперь возвращает объекты вместо `[]`, а событий нет, поэтому ожидавшие их тесты вместо быстрого падения на `undefined.ready` ждут до таймаута: `css-animations/event-order.tentative` (`animationstart`/`animationiteration`/`animationend` после `currentTime = …` — перемотка не доходит до планировщика), `css-animations/CSSAnimation-startTime.tentative`, `css-transitions/transition-events-with-document-change` (нет `transitioncancel` при переносе узла в другой документ), `web-animations/update-and-send-events` (порядок `finish` у CSSTransition/CSSAnimation/ScriptAnimation — нужны классы, [BUG-1581](BUG-1581-OPEN.md)).

## Что делать

1. Узнавать, что победившее объявление — `!important`: нативно по совпавшим правилам узла (тот же источник, что у панели Styles, `MatchedRule` в `selector_query.rs`) или флагом в снимке; при `!important` оверлей возвращает установившееся значение.
2. Доводить события до JS: при перемотке теневой анимации (`currentTime`/`startTime` setter) диспетчеризовать `animationstart`/`animationiteration`/`animationend` из оверлея либо передавать смещение часов в Rust-планировщик.

## Как проверить

`python tests/wpt/run_report.py --all --root css/css-animations --recursive` и `--root css/css-transitions --recursive`: перечисленные тесты не в TIMEOUT, `important-rules-override-animations` проходит.
