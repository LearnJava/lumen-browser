# BUG-1856 — инсталляция `window.documentPictureInPicture` сломана: «Cannot assign to read only property»

**Статус:** OPEN
**Заведён:** 2026-11-26 (P6, попутно гейту MCP-TOTAL-S7a)
**Область:** js (`crates/js/src/document_pip.rs` — инсталляция API в `window`; что сделало свойство read-only — под бисектом)

## Симптом

5 тестов `document_pip` детерминированно падают (3.8 с, соло и в полном прогоне):

```
v8: document_pip::install_document_pip_api_v8 failed: JS runtime error:
Cannot assign to read only property 'documentPictureInPicture' of object '#<Window>'
```

- `document_pip_picture_in_picture_window_class_exists` (`document_pip.rs:303`)
- `document_pip_picture_in_picture_event_class_exists` (`:328`)
- `document_pip_window_class_is_unified_with_video_pip`
- `document_pip_resize_round_trip_updates_active_window` (`:369`, далее
  `requestWindow` — `NotSupportedError`, свойство не встало)
- `document_pip_native_request_window_enqueues_open_document`

Пять соседних тестов модуля проходят — сломана именно запись свойства окна.

## Причина (предварительная)

Свойство `documentPictureInPicture` на `Window` создаётся read-only (или
инсталляционное присвоение натыкается на новую защиту window-свойств).
Регрессия того же дня, что и [BUG-1855](BUG-1855-OPEN.md): на гейте этого же
диффа раньше замыкание было зелёным. Окно бисекта 13:40–16:54 UTC+3,
первичный подозреваемый `7e06d0dc49` (stack-buffer аргументов нативного
trampoline — если флаги свойств из нативной инсталляции больше не
протаскиваются). Возможно, однорневое с BUG-1855.

## Что делать

Бисект по окну; восстановить инсталляцию (создание свойства с корректными
флагами `writable`/`configurable` вместо присвоения через голый `Set`).

## Как проверить

`cargo test -p lumen-js --lib -- document_pip` — 10/10 зелёные.
