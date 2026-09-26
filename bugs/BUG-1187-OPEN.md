# BUG-1187 — `window.top` внутри фрейма возвращает сам фрейм

**Статус:** OPEN
**Заведён:** 2026-09-26 (P6, найден при закрытии [BUG-648](BUG-648-FIXED.md): WPT
`performance-timeline/not-clonable.html` уходит в TIMEOUT).
**Область:** js — [`crates/js/src/shim/web_api_shim_tail_b.js:5858`](../crates/js/src/shim/web_api_shim_tail_b.js)
(`top` на `window` с `configurable: false`, BUG-587) против
[`crates/js/src/frame_bridge.rs:2322`](../crates/js/src/frame_bridge.rs) (`installHierarchyAccessors`
переопределяет `top` для изолята фрейма).

## Симптом

Проба 2026-09-26 (dev-release, видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, `.tmp/compat/probe.py`):
родитель с `<iframe src=child.html>`, в ребёнке через 200 мс
`parent.postMessage("top===parent:" + (window.top === parent) + " top===self:" + (window.top === window), "*")`.
Родитель получает `top===parent:false top===self:true` (Chrome: `true` / `false`). Следствие:
`window.top.postMessage(...)` из фрейма уходит самому фрейму и родителю не доходит — ровно этим
ребёнок `performance-timeline/resources/postmessage-entry.html` не доставляет ответ, и
`not-clonable.html` висит до TIMEOUT (на текущем `main` так же, это не регрессия BUG-648).

## Причина

BUG-587 сделал `top` собственным `[LegacyUnforgeable]` свойством глобала: `defineProperty(window,
'top', {get: → globalThis, configurable: false})`. Изолят фрейма потом в `installHierarchyAccessors`
пытается переопределить `top` на `topOfContext()`, но `defineProperty` на неконфигурируемом свойстве
бросает `TypeError`, а `try {} catch (e) {}` глотает её. `parent` (обычное `[Replaceable]`
свойство) переопределяется успешно, поэтому `parent.postMessage` работает, а `top.postMessage` нет.

## Что требуется

Геттер `top` должен сам знать о контексте фрейма (например, читать тот же `topOfContext()`/привязку
родителя, если она есть), оставаясь неконфигурируемым для BUG-587, либо устанавливаться один раз уже
с правильным геттером. Критерий: в пробе выше `top===parent:true top===self:false`, WPT
`not-clonable.html` доходит до своего утверждения, тесты BUG-587 (`html/browsers`) не регрессируют.
