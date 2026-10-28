# LOAD — быстрая и постепенная загрузка страницы в живом окне

Решение пользователя 2026-10-28: сайты грузятся долго и «всё или ничего». Lumen `ready_s`
13,7 с против Chrome `load` 5,7 с на top100 (`docs/perf/journal.md:418`); на netflix/uber
`fetch-images` ≈ 10–11 с (≈50 % времени, `docs/perf/metrics.md:111+`).

## Как устроено сейчас (2026-10-28)

- Поток `lumen-stream` шлёт `LoadEvent::HtmlChunk` (`crates/shell/src/page_load.rs:1042`);
  черновик — `IncrementalTreeBuilder` + `paint_partial_dom` (`page_load.rs:1138-1183`) раз
  в 16 мс, только если `!stream_css_pending` (`app/user_event.rs:71-72`). Обработчик
  `CssLoaded` (`user_event.rs:80-94`) кадр не рисует.
- По `LoadDone` поток `lumen-pipeline` (`user_event.rs:306-309`) → `render_bytes` →
  `parse_and_layout` (`crates/shell/src/page_pipeline.rs:957`) **последовательно**: разбор
  HTML заново (`:1029-1036`) → fetch-scripts (`:1144`) → CSS (`:1176`) → layout для
  скриптов (`:1238`, только если скрипты есть) → скрипты (`:1259`) → DOMContentLoaded
  (`:1442-1452`) → fetch-iframes (`:1520-1524`) → **fetch-images всех не-lazy `<img>`**
  (`:1533-1538`) → экранный layout (`:1823`) → fetch-bg-images (`:1864-1886`) →
  `report_shape_images` (`:1890`) → `RenderDone` → `apply_loaded_page`
  (`user_event.rs:369-395`), где сразу шлются `load` и `readyState=complete`
  (`page_load.rs:2139-2161`, `notify_window_loaded`).
- Асинхронный путь картинок после RenderDone уже есть: `spawn_dynamic_image_loads`
  (`page_load.rs:1218`, вызов после релейаута `relayout_job.rs:513`, дедуп
  `stream_images_requested`) → `LoadEvent::ImageDecoded` (`user_event.rs:96-137`) →
  `apply_stream_intrinsic_sizes` (`page_load.rs:1321`: intrinsic-размеры, релейаут,
  `load`/`error`, BUG-1048). Фреймы: FRAME-8 `poll_dynamic_frames` (`frame_dynamic.rs:51`,
  `FrameNewLoadDone` `:195-202`).
- Headless (`dump_mode.rs:181/527/623/867/873`, `page_source.rs:471`) вызывает тот же
  `parse_and_layout`. **Новое поведение включать только из живого окна** (флаг/параметр из
  `user_event.rs:309`): `--screenshot`, `graphic_tests`, `dump_golden` и `--trace-nav`
  обязаны остаться полностью загруженными и попиксельно прежними.

Общий стенд для проверки: локальный сервер с задержками (по образцу `.tmp/seqlab`,
BUG-1116, `docs/perf/journal.md:358`): HTML сразу, CSS +700 мс, картинки +3 с, iframe +3 с.
Положить в `tests/fixtures/load-lab/` + скрипт `scripts/load_lab.py`, если ещё нет (делает
первый, кому нужен; LOAD-0 его использует).

## LOAD-0 — метки загрузки в живом окне (P2, S)

**Depends on:** —. **Files:** `crates/shell/src/lumen/*.rs` (поле в `Lumen`),
`page_load.rs`, `app/user_event.rs`, `app/redraw_requested.rs`, `health_log.rs`,
`scripts/perf_audit.py`, `docs/perf/metrics.md`.

1. `LoadMarks { t0, first_content_ms, final_applied_ms, final_frame_ms }` в `Lumen`;
   сбрасывать в `reload()` рядом с `nav_start` (`page_load.rs:508`), но отдельно от него
   (`nav_start` забирается в RenderDone, `user_event.rs:405`).
2. `first_content_ms` — в `paint_partial_dom` после `set_display_list`, если список не пуст;
   `final_applied_ms` — после `apply_loaded_page` (`user_event.rs:395`); `final_frame_ms` —
   первый `RedrawRequested` после него (флаг).
3. Запись `health_log::log_load_timing(url, marks)` рядом с `log_render_health`
   (`health_log.rs:128`); `perf_audit.py` достаёт новый `kind` из `health.<pid>.log`
   (`perf_audit.py:1021-1026, 1217`) в поля `first_content_s`, `final_frame_s`.
4. Базовый замер: `perf_audit.py` в режиме stability на сайтах пользователя
   `docs/perf/corpus-user-ru.txt` (30 RU-сайтов, **без прокси**: в `data/fingerprint.toml` нет
   `proxy =`), запись в `docs/perf/journal.md`. Этот же список — замер «до/после» для всех LOAD-*.
   База снимается **до** LOAD-1: если к моменту замера LOAD-1 уже влит (`git log --grep=LOAD-1
   origin/main`), собрать `dev-release` в отдельном worktree из родителя merge-коммита LOAD-1 с
   cherry-pick коммита LOAD-0 и замерить на нём; в журнале указать оба хеша.

**Done when:** поля есть в JSON perf_audit; unit-тест на сброс/заполнение меток; запись
базы в журнале (медианы `first_content_s`, `final_frame_s`, `ready_s`).

## LOAD-1 — страница на экран без ожидания картинок (P1, M)

**Depends on:** — (замер до/после — LOAD-0). **Files:** `page_pipeline.rs`, `page_load.rs`,
`app/user_event.rs`, `dynamic_image_hook.rs`, `crates/shell/src/tests/page_pipeline.rs`.

1. Параметр `parse_and_layout` (или `RenderMode::Live`) из `user_event.rs:309`: в живом
   режиме пропустить `fetch-images` (`:1533-1538`) и `fetch-bg-images` (`:1864-1886`),
   строить экранный layout сразу после DOMContentLoaded; картинки с известным
   `width`/`height` раскладываются по атрибутам, прочие — 0×0 до прихода.
2. После `apply_loaded_page` один раз вызвать `spawn_dynamic_image_loads`; фоновые картинки —
   тем же путём, что BUG-939 (`page_load.rs:1236-1245`). Регистрацию bitmap для canvas и
   `img.onload` (`page_pipeline.rs:1650-1700`) в живом режиме отдать
   `apply_stream_intrinsic_sizes`.
3. **Событие `load` и `readyState=complete`** отложить до нуля незавершённых картинок
   (`ImageRequestLedger`); DOMContentLoaded — как сейчас. Иначе `load` придёт раньше
   картинок, а `ready_s` в perf_audit поменяет смысл.
4. Релейауты от прихода картинок — пакетно, не чаще раза в кадр (уже так в
   `apply_stream_intrinsic_sizes`; проверить на странице с 200 картинками).

**Сделано (2026-10-28).** Живой режим = `dynamic_image_hook_ctx.is_some() && !--deterministic`
(`defer_images` в `parse_and_layout`); headless и `graphic_tests` не затронуты. В живом режиме
`fetch_and_decode_images` берёт из `IMAGE_CACHE::peek_current` только уже декодированное, фоны
(`defer_plain`) оставляют в конвейере лишь `shape-outside: url()` и CSP-проверку, `<img crossorigin>`
на чужой origin остаётся на синхронном пути (CORS и taint canvas только там). Остальное уходит
`spawn_dynamic_image_loads`/`spawn_dynamic_background_image_loads` из `apply_loaded_page`;
`load`/`readyState=complete`/`pageshow` и запись Navigation Timing ждут в
`Lumen::pending_window_load`, пока `maybe_fire_window_load` не увидит нуль незавершённых картинок
(`unsettled_images`) и отработавший проход `apply_stream_intrinsic_sizes` (зовётся из
`RedrawRequested` и `about_to_wait`). Картинки, декодированные до `RenderDone`, возвращаются рендереру
после `clear_images` из `stream_image_pixels`; `fired_image_events` страницы засевает
`stream_image_events_fired`, чтобы `load` не уходил картинке дважды. Стенд — `scripts/load_lab.py`
(HTML сразу, CSS +700 мс, картинки/iframe +3 с; метки `raf`/`load` в журнале сервера).
Замер на стенде без iframe (`--frame-ms 0`, dev-release, нагруженная машина): первый кадр после HTML
8,9 с → 5,5 с (в `--deterministic`, т.е. по-старому, он приходит после картинок), `load` 8,5 с — после
последней картинки; 12 картинок отрисованы. Остаток: iframe всё ещё держит `RenderDone` — LOAD-1B.

**Done when:** unit-тест: живой режим отдаёт RenderDone до прихода картинок, `load` — после
последней; headless-режим без изменений (`python graphic_tests/run.py --continue-on-fail`
зелёный без новых эталонов, `dump_golden.py` 12/12); на стенде с картинками +3 с финальный
кадр ≤ 1 с после HTML (метки LOAD-0); `ready_s` на netflix/uber не хуже прежнего.

## LOAD-1B — iframe не держат страницу (P1, S)

**Depends on:** LOAD-1. **Files:** `page_pipeline.rs`, `frame_dynamic.rs`, `page_load.rs`.

В живом режиме пропустить `fetch-iframes` (`:1520-1524`), после RenderDone запустить
FRAME-8 `poll_dynamic_frames` для ещё не загруженных host-элементов (проверить, что он их
подхватывает без `dom_dirty`); `sync_frame_viewports` (`:1831`) и
`fetch_frame_lazy_images` (`:1840`) — по приходу `FrameNewLoadDone`. `load` окна — после
фреймов (счётчик `pending_new_frames`).

**Сделано (2026-10-28).** Живой режим (`defer_images`) не зовёт `load_frame_sub_documents`:
`ParsedPage::frames` пуст. `apply_loaded_page` кладёт в `PendingWindowLoad` флаг
`frames_scan_pending`; `Lumen::drive_deferred_frames` (из `about_to_wait`, пока ждёт `load`) делает
первый `poll_dynamic_frames`, повторяет его, если документ занят движковым потоком, и будит цикл
для `dispatch_pending_frame_loads` (спавн отделён от скана проходом цикла). Фреймы приходят
`FrameNewLoadDone` → `refresh_frames(None)` (это и есть `sync_frame_viewports` +
`register_frame_lazy_images`, отдельно звать не нужно). `load` окна ждёт скан и пустые
`pending_new_frames`/`pending_frame_load_dispatch` (`window_load_due(.., frames_busy)`). Находка:
`on_frame_new_load_done` не сверял хозяина — ответ, переживший навигацию, вклеивал фреймы старой
страницы в новую; теперь хозяин обязан быть документом текущей страницы или живого фрейма.
Стенд `load_lab.py` (CSS +700 мс, картинки и iframe +3 с, dev-release): первый кадр (`raf`)
1,16 с, запрос iframe уходит сразу после него, `load` 4,31 с — после iframe и последней картинки.

**Done when:** тест: RenderDone до загрузки iframe со стенда +3 с, `load` окна после него;
graphic_tests без изменений.

## LOAD-2 — черновик по приходу CSS и дедлайн на стили (P1, S)

**Depends on:** —. **Files:** `app/user_event.rs`, `page_load/css_pending.rs`,
`crates/shell/src/tests/subdocument_frames.rs`.

1. В `CssLoaded`: если `!stream_css_pending(generation) && stream_builder.is_some()` —
   `paint_partial_dom` (сейчас, если HTML пришёл раньше CSS, черновика нет вовсе).
2. `css_pending.rs`: `Instant` первого `add` и дедлайн 2 с, после которого `pending == false`
   (Chrome тоже блокирует отрисовку на CSS из `<head>`, но один медленный CSS не должен
   держать экран пустым до 60 с `FETCH_READ_TIMEOUT`, `network/src/lib.rs:1242`). Пробуждение
   по дедлайну — `ControlFlow::WaitUntil` или wake-событие (как `AutomationWake`).
3. Если финальный `build_page_cascade` ждёт тот же CSS (`stylesheets.rs:336`) — не менять,
   только записать в отчёт задачи, сколько это стоит на стенде.

**Done when:** тест рядом с `stream_css_pending_gates_frames_per_generation`
(`subdocument_frames.rs:1488`): черновик рисуется по CssLoaded и по дедлайну; на стенде с
CSS +5 с первый кадр с контентом ≤ 2,2 с (LOAD-0).

**Сделано (2026-10-28).** `css_pending.rs`: счётчик хранит момент первого `add`; через
`STREAM_CSS_DEADLINE` (2 с) `stream_css_pending` становится `false`, а `stream_css_pending_expired`
отдаёт дедлайн ровно один раз. `Lumen::stream_css_deadline_tick` (из `about_to_wait`) рисует черновик
и вкладывает дедлайн в `WaitUntil`; `CssLoaded` после снятия ожидания тоже рисует
(`paint_stream_draft`, без 16-мс троттлинга). Находка: `LoadDone` сбрасывал `stream_builder` и
`stream_sheet`, и пока финальный pipeline ждал медленный CSS, рисовать было не из чего — сброс
перенесён в `RenderDone`. Стенд `load_lab.py --css-ms 5000` (dev-release): `first_content_ms`
(LOAD-0) 2,03 с; без дедлайна черновика не было вовсе. Финальный `build_page_cascade`
(`stylesheets.rs:336`) по-прежнему ждёт тот же CSS: `final_applied_ms` 8,6 с при CSS +5 с и
картинках/iframe +3 с (CSS 5,0 с + скрипты/iframe), не менялся.

## LOAD-3C — прогрев скриптов без потока на каждую подсказку (P1, S)

**Depends on:** —. **Files:** `page_load.rs:2244-2307`, `prefetch.rs`.

Прогрев из стримингового сканера (BUG-171, `PREFETCH_CACHE`) делает `std::thread::spawn` на
каждую подсказку (`page_load.rs:2276`) и клиент без referrer-политики документа (`:2286`),
а финальный проход — с ней (`scripts.rs:455`). Перевести на ограниченный пул (как
`parallel_fetch`, ≤ 8) и общий клиент с той же политикой, чтобы прогретый ответ
гарантированно переиспользовался `fetch_current` (`scripts.rs:454`).

**Done when:** тест: страница с 30 внешними скриптами создаёт ≤ 8 потоков прогрева, каждый
скрипт скачан один раз (счётчик запросов на стенде).

## LOAD-3A — `async`-скрипты не блокируют страницу (P1, M)

**Depends on:** LOAD-1. **Files:** `crates/shell/src/scripts.rs` (`:70`, `:116-124`, `:369`),
`page_pipeline.rs`, движковый поток JS.

Сейчас классический `async` идёт в общий список по порядку документа, и пайплайн ждёт
загрузку всех скриптов (`resolve_script_sources`). Нужно: отдельная очередь async, которая
не входит в ожидание `fetch-scripts`; выполнение по готовности — до или после RenderDone,
через движковый поток. Проверить: `document.currentScript`, `document.write` из async
игнорируется, DOMContentLoaded не ждёт async, `load` — ждёт.

**Done when:** WPT `html/semantics/scripting-1/the-script-element/` async-подтесты не хуже
прежнего (A/B по `docs/wpt-*`), unit-тест: async-скрипт +3 с на стенде не задерживает
RenderDone.

## LOAD-3B — не разбирать HTML второй раз (P1, S)

**Depends on:** LOAD-1. **Files:** `app/user_event.rs` (LoadDone), `page_pipeline.rs:1029-1036`,
`html_parser` (`tree_builder.rs`).

В LoadDone забрать `stream_builder`, `finish()` и передать готовый `Document` в
`lumen-pipeline` вместо `parse`. Откат на повторный разбор: кодировка не UTF-8
(`lumen_encoding::detect`, `page_pipeline.rs:1010`; стриминг декодирует только UTF-8,
`push_tokenizer.rs:160-198`), XML по Content-Type, документ с DTD-сущностями
(`tree_builder.rs:410-417`), фоновая вкладка (BUG-1214).

**Done when:** тест: UTF-8 страница разбирается один раз (счётчик/спан `parse-html`), cp1251 —
откат и прежний DOM; `html_parse_ms` в perf_metrics на 850-КБ документе (BUG-1694) падает.
