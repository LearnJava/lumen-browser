# BUG-1214 — `Lumen::load_generation` — единый глобальный счётчик, а не per-tab

**Статус:** OPEN
**Заведён:** 2026-09-29 (P3, при проверке фикса BUG-1212 через WPT)
**Область:** shell (`crates/shell/src/lumen/state.rs:671` — `Lumen::load_generation`;
`crates/shell/src/page_load.rs` — `reload`/`start_streaming_load`; `crates/shell/src/app/user_event.rs`
— каждый `LoadEvent::*` обработчик сверяет `generation != self.load_generation`)

## Симптом

`Lumen::load_generation` — одно поле `u64` на весь процесс, инкрементируемое в `reload()`
при КАЖДОЙ навигации, независимо от того, в какой вкладке она произошла (`page_load.rs:1020`).
Каждое событие streaming-пайплайна (`EarlyPreloadHints`, `HtmlChunk`, `CssLoaded`, `LoadDone`, …)
несёт generation, под которым оно было запущено; `user_event.rs` отбрасывает событие, чей
generation не совпадает с ТЕКУЩИМ значением поля — механизм задуман как защита от гонки
устаревшей навигации (быстрый back/forward, два клика по ссылкам подряд) в ОДНОЙ вкладке.

Но когда `window.open()` открывает ДВЕ вкладки подряд в одном тике (`about_to_wait`'s
`window_open_requests`-цикл, см. [BUG-1212](BUG-1212-OPEN.md)), каждый попап получает свою
навигацию через `self.navigate_to(source)` → `reload()` → `start_streaming_load(generation)`,
и generation инкрементируется ОБЩИЙ счётчик `self.load_generation` — а не что-то per-tab. Пока
первый попап ещё грузится в фоновом потоке, второй попап уже бампнул `load_generation` на
общем поле. Когда первый попап досылает свой `LoadEvent::LoadDone`, guard
`generation != self.load_generation` видит чужое (более новое) значение и **отбрасывает
событие целиком** — `run_scripts_with_dom` для первого попапа никогда не вызывается, его
скрипт (`post-to-opener.html`) никогда не выполняется, `window.opener.postMessage()` никогда
не отправляется.

Воспроизведено 2026-09-29 через `tests/wpt/run_smoke.py` (`origin-from-messageevent.window.html`)
после фикса BUG-1212's `opener_tab_id`-бага: батч из двух `window.open()` (same-origin,
затем `REMOTE_ORIGIN`) в одном скрипте — лог показывает `Распарсено: N DOM-узлов` только
для ОДНОГО из двух попапов; второй (или первый, порядок зависит от тайминга сети) не
доходит до `run_scripts_with_dom` вовсе — TIMEOUT на его subtest.

## Отношение к BUG-1212

BUG-1212 (эта пара «два `window.open()` не доставляют сообщение») закрывалась по частям —
`opener_tab_id`, читанный ВНУТРИ цикла драйна попапов, ловил tab id уже переключённой (на
первый попап) активной вкладки для второго попапа. Этот фикс landed и подтверждён:
`origin-from-messageevent.window.html` до фикса — TIMEOUT у ОБОИХ popup-сабтестов (2/2),
после фикса — TIMEOUT только у ОДНОГО (1/2). Оставшийся TIMEOUT — это ДАННЫЙ баг
(load_generation), не opener addressing: сообщение от второго попапа теперь адресуется
правильно, но первый попап (в порядке теста — same-origin) вообще не successfully
инициализирует `window.opener` и не отправляет `postMessage`, потому что его собственный
`LoadDone` был отброшен generation-guard'ом.

## Что требуется

`load_generation` (или его эквивалент) должен быть per-tab, не per-process — каждая вкладка
ведёт свою собственную навигацию независимо, и завершение загрузки одной вкладки не должно
инвалидировать streaming-события другой. Затрагивает как минимум: `Lumen::load_generation`
(`state.rs:671`), `reload()`/`start_streaming_load()` (`page_load.rs`), каждый
`generation != self.load_generation` guard в `user_event.rs` (8 точек на 2026-09-29),
`document_base: Option<(ResourceBase, u64)>` (`state.rs:678`, та же generation-пара). Требует
архитектурного решения: либо ключевать generation по tab id (`HashMap<u32, u64>`), либо
привязать сравнение к тому, какая вкладка была активной в момент запуска конкретной
streaming-загрузки (сложнее — активная вкладка на момент ЗАВЕРШЕНИЯ может отличаться от
момента ЗАПУСКА, что и есть весь смысл фоновых попапов).

Критерий: батч из двух `window.open()` (same-origin + cross-origin) в одном скрипте — оба
popup-документа успешно доходят до `run_scripts_with_dom`, оба отправляют
`postMessage` опенеру; WPT `origin-from-window.window.html`/`origin-from-messageevent.window.html`
— PASS на обоих popup-сабтестах.
