# BUG-1212 — два `window.open()` подряд: ни один попап не доставляет сообщение опенеру

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

## Прогресс 2026-09-29 (P3)

Первая половина причины исправлена: `about_to_wait`'s `window_open_requests`-цикл читал
`opener_tab_id = self.tab_strip.tabs[self.tab_strip.active].id` ВНУТРИ цикла — на второй
итерации `self.tab_strip.active` уже был переключён (`open_new_tab()`/`switch_tab()`) на только
что созданную вкладку первого попапа, так что второй попап получал tab id первого попапа как
своего «опенера», а не реального вызывающего таба. Фикс — читать `opener_tab_id` ОДИН раз, до
цикла (`crates/shell/src/app/about_to_wait.rs`).

Подтверждено через `tests/wpt/run_smoke.py` на `origin-from-messageevent.window.html`: до фикса
— TIMEOUT у ОБОИХ popup-сабтестов (`for same-origin windows`/`for cross-origin windows`), 2/2
unexpected; после фикса — TIMEOUT только у ОДНОГО, 1/2 unexpected.

Второй, независимый баг всё ещё блокирует полное закрытие: `Lumen::load_generation` — общий
счётчик на процесс, а не per-tab (см. [BUG-1214](BUG-1214-OPEN.md)) — первый попап batch'а
теряет свой `LoadEvent::LoadDone` (отбрасывается generation-guard'ом, потому что второй попап
уже успел бампнуть тот же общий счётчик), и его `run_scripts_with_dom` никогда не запускается —
`window.opener.postMessage()` со стороны первого попапа никогда не отправляется. Оставлено OPEN
до фикса BUG-1214; `opener_tab_id`-часть фикса landed отдельным коммитом на ветке
`p3-bug1212-opener-batch` — необходимая, но не достаточная часть закрытия.
