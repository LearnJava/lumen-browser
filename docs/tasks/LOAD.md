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
