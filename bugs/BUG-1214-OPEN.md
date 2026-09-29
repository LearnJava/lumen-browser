# BUG-1214 — `Lumen::load_generation` — единый глобальный счётчик, а не per-tab

**Статус:** OPEN (частично исправлено — см. «Прогресс 2026-09-29»)
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

## Прогресс 2026-09-29 (ветка `p3-bug1214-load-generation`)

Реализована первая половина архитектурной переделки — **маршрутизация по tab_id**, не только
per-tab generation:

- `LoadEvent` (все варианты streaming-пайплайна: `EarlyPreloadHints`, `DocumentBase`, `HtmlChunk`,
  `CssLoaded`, `LoadDone`, `LoadError`, `CertError`, `RenderDone`) теперь несёт `tab_id: usize`,
  снятый ОДИН раз в `start_streaming_load`/`reload`/`resumed` в момент запуска конкретной
  навигации — не читается заново на приёме.
- `Lumen::is_active_tab(tab_id)` — единственная точка правды «это события АКТИВНОЙ сейчас
  вкладки?». Каждый обработчик в `user_event.rs` проверяет ЕЁ ПЕРВОЙ, до сверки generation.
- Если `tab_id` — уже не активная вкладка (второй `window.open()` в батче увёл `self` на себя,
  пока первый ещё грузится): событие для НЕЁ отбрасывается, но — критично — НЕ применяется к
  полям `self`, которые сейчас принадлежат ДРУГОЙ (текущей активной) вкладке. Вместо этого
  `Lumen::mark_bg_tab_needs_reload(tab_id)` ставит `PageSnapshot::pending_reload` этой вкладки
  в `bg_tabs` — тот же флаг, что `switch_tab` уже проверяет для queue_task/UserInteraction reload.
  Когда пользователь (или тест) переключится на эту вкладку, `switch_tab` сам вызовет `reload()`.
- `PageSnapshot` получила собственные `load_generation`/`document_base` — так что при восстановлении
  из `bg_tabs` вкладка сверяет СВОЮ generation, а не generation вкладки, из которой её только что
  вытеснили.

**Проверено:** `cargo clippy -p lumen-shell --all-targets -- -D warnings` — чисто;
`bash scripts/scoped-test.sh main` — все тесты проходят; `lumen.exe --dump-layout` на обычной
одновкладочной странице — без изменений в поведении (early-return не срабатывает, когда
`tab_id == активная вкладка`, то есть путь для всех НЕ-popup сценариев побайтово идентичен).

**НЕ закрывает бага полностью.** Критерий выше требует, чтобы `run_scripts_with_dom` для ПЕРВОГО
попапа выполнился и он отправил `postMessage` БЕЗ переключения вкладок — именно так работает WPT-тест
(`origin-from-messageevent.window.html`), который не делает `switch_tab`, а читает состояние
backgrounded-вкладки напрямую через `automation_tab_in_background()`/`AutomationCommand::Eval`.
Этот фикс НЕ доставляет фоновой вкладке её собственный `run_scripts_with_dom` — он лишь
гарантирует, что событие фоновой вкладки больше не корродирует состояние АКТИВНОЙ (регрессия,
которую внесла бы наивная keyed-по-табу правка одного generation, см. обсуждение в начале файла),
и что навигация фоновой вкладки не теряется НАВСЕГДА (`reload()` подхватит её при следующем
переключении). Это реальное улучшение (было: тихая порча/потеря; стало: отложенный, но
гарантированный reload), но не закрывает исходный симптом «оба попапа доходят до
`run_scripts_with_dom` без переключения вкладок».

**Что осталось для полного закрытия:** финальный pipeline (`render_bytes`/`apply_loaded_page`)
должен уметь применить результат НЕ к `self`, а напрямую к `PageSnapshot` фоновой вкладки в
`bg_tabs[tab_id]` — то есть завести вариант `apply_loaded_page`, параметризованный целью
(`&mut Self` для активной / `&mut PageSnapshot` для фоновой), либо материализовать фоновую
вкладку во временный `Lumen`-подобный контекст для пайплайна. Это отдельная, более крупная
правка (каждый писатель `LoadEvent`-обработчика в `user_event.rs`, ~10 точек, плюс
`apply_loaded_page` в `page_load.rs`/`page_pipeline.rs`) — не выполнена в этой сессии
из-за её объёма; текущий коммит — необходимая, но не достаточная часть архитектурной переделки.
Следующая сессия: применить готовую инфраструктуру (`tab_id` на `LoadEvent`, `is_active_tab`)
и добавить фактическую запись результата пайплайна в `bg_tabs[tab_id]` вместо `mark_bg_tab_needs_reload`
для веток `LoadDone`/`RenderDone`; тогда WPT-критерий выше должен пройти без reload-обхода.
