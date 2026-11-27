# BUG-1860 — после выхода из fullscreen вьюпорт страницы отстаёт на один resize (stay 1904×869 вместо 1024×620)

**Статус:** OPEN
**Заведён:** 2026-11-27 (P6, живая проба MCP-TOTAL-S4b)
**Область:** shell (sync `chrome_page_host_rect` ↔ `relayout` — `crates/shell/src/relayout.rs:1548` `relayout_viewport`, `crates/shell/src/app/mod.rs:247` `WindowEvent::Resized`, `crates/shell/src/lumen/viewport_sync.rs` `poll_fullscreen_resize`); путь не зависит от автоматики — воспроизводится и на неприкосновенном `exitFullscreen()`

## Симптом

Живая проба (окно 1040×831, вьюпорт 1024×620, экран 1920×1080):

1. **Вход** в fullscreen — обеим дорогами (`requestFullscreen()` кнопкой и MCP `set_fullscreen {enabled:true}`) вьюпорт доводится до `1920×908`, но только когда порядок событий удачный; во втором прогоне `set_fullscreen` застрял на переходном `1040×659` (см. «Причина»).
2. **Выход** — `window.innerWidth/innerHeight` остаются `1904×869` (размер fullscreen минус хром) **и после** `exitFullscreen()` и **после** `set_fullscreen {enabled:false}`; при этом OS-окно честно вернулось к 1040×831 (`get_window_rect`, `WindowEvent::Resized`). Вьюпорт догоняет только на следующем реальном resize.

То есть после каждого выхода страница считается в прежнем (fullscreen) layout-размере, пока что-то ещё не дёрнет relayout.

## Причина

`relayout_viewport` берёт размер из `chrome_page_host_rect` — а тот обновляется `relayout_chrome_host()` **после** `relayout()` в обработчике `WindowEvent::Resized` (mod.rs:271→274). Документ это признаёт: «One frame of staleness after a resize/self-corrects on the next pass» (relayout.rs:1567). При обычном ресайзе «следующий проход» приходит с движением мыши/скроллом; после fullscreen-переключения — в частности после выхода — ни `Resized`, ни `poll_fullscreen_resize` не дают второго прохода с уже свежим host-rect:

- `Resized(новый размер)` → `relayout()` (host ещё старый) → `relayout_chrome_host()` (host теперь свежий) — и всё, дальше ничего не триггерит.
- `poll_fullscreen_resize::Apply` (viewport_sync.rs:48) делает `renderer.resize`+`relayout`, но (а) не вызывает `relayout_chrome_host`, (б) в зависимости от порядка с диспетчем `Resized` может применить переходный размер (проба ловила вьюпорт `1040×659` при финальном `1920×1080`) или вовсе не добавиться после `Resized`.

Дороги входа/выхода симметричны: настоящий путь (`take_fullscreen_requests` → тот же `set_fullscreen`+arm) и `AutomationCommand::SetFullscreen` (дословный хвост того же кода) падают в один и тот же лаг — проба это A/B-подтверждает, дефект не в арме автоматики.

## Что делать

1. Сделать sync host-rect → relayout атомарным для fullscreen-переключений: либо `relayout_chrome_host()` **перед** `relayout()` в `Resized` (если rect можно посчитать до layout — он про chrome-хост, не про страницу), либо второй проход после обновления host, либо `poll_fullscreen_resize::Apply` должен ждать совпадения `inner_size` с финальным fullscreen/восстановленным размером (не хватать первый отличившийся переходный) и вызывать `relayout_chrome_host` перед `relayout`.
2. Проверить тот же хвост у media-controls fullscreen (`crates/shell/src/lumen/media_controls.rs:593–613`) — механика та же.

## Как проверить

Проба из MCP-TOTAL-S4b (скрипт снят, воспроизводится руками): `set_fullscreen {enabled:false}` (или `exitFullscreen()`) → сразу `eval("[innerWidth, innerHeight]")` должен вернуть вьюпорт восстановленного окна (1024×620), а не fullscreen-минус-хром (1904×869). Заодно: вход в fullscreen — вьюпорт 1920×908 без переходных значений при повторных прогонах.
