# BUG-1209 — два `window.open()` подряд: ни один попап не доставляет сообщение опенеру

**Статус:** OPEN
**Заведён:** 2026-09-28 (P6, при закрытии [BUG-1199](BUG-1199-FIXED.md))
**Область:** shell/js — открытие вкладки по `window.open()` (`crates/shell/src/lumen/tabs_cmd.rs`
`open_new_tab`) и доставка `postMessage` попап → опенер
(`lumen_js::window_messaging`, насос `_lumen_window_pump_messages` в
`crates/shell/src/app/about_to_wait.rs`); точное место потери не локализовано.

## Симптом

WPT `html/browsers/origin/api/origin-from-window.window.html` и
`origin-from-messageevent.window.html`: сабтесты `… for same-origin windows.` и
`… for cross-origin windows.` — TIMEOUT. Каждый файл открывает два попапа
(`/html/browsers/windows/resources/post-to-opener.html`, same-origin и `REMOTE_ORIGIN`) в
двух `async_test` одного синхронного скрипта; попап в первом скрипте делает
`window.opener.postMessage({name, isTop}, "*")`.

Проба 2026-09-28 (dev-release, `tests/wpt/run_smoke.py`, одноразовая страница в
`html/browsers/origin/api/`):

- **один** `window.open(same-origin)` — опенер получает сообщение, `e.source === w` верно,
  `e.origin` правильный;
- **два** `window.open()` подряд (same-origin, затем `REMOTE_ORIGIN`) в одном скрипте — за 3 с
  опенер не получил **ни одного** сообщения, хотя оба URL попапов загружаются (в логе
  раннера `Reload:` для обоих). `w1 !== w2`.

## Что требуется

Каждое окно, открытое `window.open()`, — отдельный вспомогательный контекст просмотра со своим
`opener` (HTML LS §7.3.2); его `opener.postMessage` ставит задачу в очередь опенера независимо от
того, сколько окон открыто и какое из них активно. Критерий: проба с двумя попапами получает
оба сообщения, у каждого `e.source` совпадает со своим `WindowProxy`; сабтесты с окнами в двух
файлах выше — PASS.
