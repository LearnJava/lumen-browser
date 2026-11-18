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

## LOAD-5 — картинки качаются со стримингового сканера, пул ≤ 8 (P6, M)

**Depends on:** — (LOAD-1, LOAD-3C в `main`). **Files:** `crates/shell/src/page_pipeline.rs`
(`warm_preload_cache` :273 и его комментарий :251-260), `crates/shell/src/page_load.rs`
(`spawn_image_requests` :1625, `std::thread::spawn` :1695), `crates/shell/src/prefetch.rs`.

1. Сканер уже эмитит `<img src/srcset>`-хинты (`dispatch_preload_hints`,
   `PreloadHint::Image`), но байты не греет — комментарий «`<img>` сканера и preconnect
   в байтовом кэше не нужны» устарел: после LOAD-1 не-lazy картинки в живом режиме
   стартуют только из `apply_loaded_page` (`spawn_dynamic_image_loads`), пока идут
   HTML+CSS+скрипты, сеть простаивает. Греять не-lazy `<img>` через `PREFETCH_CACHE`
   из пула ≤ 8 (образец — пул LOAD-3C) общим клиентом с политикой документа, чтобы
   `IMAGE_CACHE.get_or_decode` взял те же байты без второго запроса.
2. `spawn_image_requests` делает `std::thread::spawn` на каждую картинку без предела —
   перевести на тот же пул; видимые в вьюпорте впереди остальных
   (`collect_image_requests` уже принимает viewport).
3. Откат `LUMEN_NO_IMG_POOL=1`; headless-путь (`fetch_and_decode_images`) не трогать —
   он читает только готовое из `IMAGE_CACHE::peek_current`.

**Done when:** тест — 30 внешних картинок создают ≤ 8 потоков, каждая скачана один раз
(счётчик запросов стенда); на стенде `load_lab.py` (картинки +3 с) GET картинок уходят
до RenderDone (журнал сервера); первый кадр и `load` не хуже значений §LOAD-1/1B;
`ready_s` на корпусе `docs/perf/corpus-user-ru.txt` не хуже.

**Сделано (2026-11-17).** Сканер (`preload_scanner.rs`) несёт `lazy`-флаг в
`PreloadHint::Image`; `feed_preload_and_emit` греет байты не-lazy `<img src>` (без
srcset/`<picture>`) в `PREFETCH_CACHE` общим `WarmClient` с политикой документа — гейт CSP
`img-src` через новую ветку `Image` в `speculative_fetch_blocked`, `upgrade-insecure-requests`
переписывает адрес до гейта и до ключа слота. `spawn_image_requests` больше не поднимает
поток на каждую картинку: декод-задачи идут на общий `WARM_POOL` (≤ 8, LOAD-3C), stable-сорт
по `fetchpriority` (high вперёд; геометрический приоритет вьюпорта не считается — обход
layout-дерева на каждый стриминговый кадр дороже выигрыша, над строкой текста картинки почти
всегда раньше в разметке). Откат — `LUMEN_NO_IMG_POOL=1` (`prefetch::img_pool_disabled`):
без него прежний `std::thread::spawn` на картинку и без прогрева. Тесты:
`prefetch.rs::warm_30_images_fetches_each_once_on_at_most_8_threads` (30 URL × 2 объявления =
30 загрузок, пик ≤ 8), `preload_scanner.rs::img_loading_lazy_flagged`,
`csp_enforce.rs::speculative_fetch_blocked_follows_element_directives`. Стенд
`--frame-ms 0 --img-ms 3000`, dev-release, по два прогона на плечо: GET первой волны уходят
на 0,03–0,10 с (прогрев сканера, до CSS) против ~0,9 с прежде (стриминговый продюсер ждал
CSS-pending); первый кадр 1,1–1,9 с (прежде 1,2 — кадру всё равно ждать CSS+layout);
`load` 6,09–6,19 с против 3,92–4,09 с у старого поведения — цена глобального потолка ≤ 8
на однобоковом стенде с равными задержками: 12 картинок идут волнами 8+4 (браузерный лимит
~6/хост даёт тот же размен), обе цифры внутри конверта §LOAD-1/1B (8,5 с на нагруженной
машине). Корпус `ready_s` не перепрогонялся: разброс между прогонами LOAD-0 (vk 4,8/15,7 с)
больше эффекта, следящим прогоном снимет SITES-RECRAWL-1; рычаг A/B — `LUMEN_NO_IMG_POOL=1`.

## LOAD-4 — финальный проход не ждёт последний CSS (P6, M)

**Depends on:** — (LOAD-2 и PERF-INVALIDATION S1/S2 в `main`). **Files:**
`crates/shell/src/page_pipeline.rs` (`build_page_cascade` :725, фазы `parse_and_layout`),
`crates/shell/src/stylesheets.rs` (`load_linked_stylesheets` :101).

1. Фетч `<link rel=stylesheet>` уже параллелен (`parallel_map`, `stylesheets.rs:131`,
   конкатенация в порядке документа), но финальный проход ждёт все листы до экранного
   layout. Остаток записан в §LOAD-2: `final_applied_ms` 8,6 с при CSS +5 с («не
   менялся»); дедлайн 2 с спасает только черновик, финал ждёт до `FETCH_READ_TIMEOUT`
   60 с (`network/src/lib.rs:1242`).
2. В живом режиме строить каскад из пришедших к моменту финала листов (порядок
   документа сохраняется — опоздавший лист встанет на своё место); опоздавший лист по
   приходу запускать инкрементальный рестайл (механизм PERF-INVALIDATION), не полную
   пересборку; `load`/`error` у `<link>` — по факту прихода, как сейчас.
3. Headless (`--screenshot`, `graphic_tests`, `--dump-*`, `--trace-nav`) — прежний
   путь, попиксельно без изменений.

**Done when:** на стенде `load_lab.py --css-ms 5000` (dev-release) финальный кадр
≤ прихода CSS + 1 с, поздний лист применяется инкрементально (спан каскада короче
полного); unit-тест порядка; graphic_tests и `dump_golden.py` без изменений;
`final_applied_s` на корпусе пользователя не хуже базы LOAD-0.

**Сделано (2026-11-17).** Живой режим (`LateCssFeed` из `DynamicImageHookCtx`;
headless, `--deterministic` и dump-режимы получают `None` и прежний путь)
строит каскад только из **заполненных** слотов `PREFETCH_CACHE`
(`PrefetchCache::try_fetch_current` — без ожидания незавершённой загрузки).
Лист в пути даёт `FetchedSheet::Pending`: `subscribe_late_stylesheet` берёт его
через `warm_with` (ждёт прогрев стримингового сканера или заводит свой; байты
разбирает тот же `fetch_stylesheet_for_cascade` → `inline_css_imports`, то есть
кодировка BUG-509, ребейз BUG-1127 и `@import` — как у синхронной ветки), а узел
не попадает ни в `link_outcomes`, ни в CSSOM-реестр
(`build_stylesheet_node_registry(late = true)`). По приходу
`LoadEvent::LateStylesheet` → `Lumen::apply_late_stylesheet`: текст встаёт в
`DynamicCssBase::linked`, `linked_dirty` заставляет `refresh_dynamic_css`
пересобрать лист из частей — без сети и без повторного разбора HTML/скриптов
(не полная пересборка); порядок документа даёт `merge_sheet_chunks`, экран
обновляет обычный `relayout` (смена ревизии листа — «широкая дорога»
[ADR-036](../decisions/ADR-036-incremental-restyle-invalidation.md) §3), CSSOM
получает запись на её место в дереве (`update_stylesheet_nodes`), и уже после
раскладки уходит `load`/`error` элемента (`_lumen_deliver_parser_link_events`,
BUG-804). `load` окна лист держит (HTML LS §4.6.7, счётчик
`Lumen::pending_late_css`), кадр — нет. Пока `RenderDone` в пути, лист ждёт в
`Lumen::pending_late_stylesheets` и разбирается сразу после применения страницы
(у `layout_source` ещё каскад предыдущей). Попутно: прогрев стримингового
сканера теперь применяет `upgrade-insecure-requests` до гейта и запроса
(`page_load.rs::feed_preload_and_emit`), как финальный проход, — иначе UIR-страница
качала лист дважды, а LOAD-4 считал уже пришедший лист непришедшим. Стенд и
цифры — [journal](../perf/journal.md) §2026-11-17 LOAD-4.

## LOAD-7 — мёртвый подресурс не держит `load` окна (P6, M)

**Depends on:** —. **Files:** `crates/shell/src/page_load.rs` (`pending_window_load`,
`maybe_fire_window_load`, `unsettled_images` — LOAD-1), `dynamic_image_hook.rs`,
`crates/network/src/lib.rs` (таймауты).

1. Проба: на 5 сайтах корпуса с `ready_s` ≫ `final_applied_s` (ok-ru 45,2/6,4;
   sports-ru 36/17,3; pikabu 21,2/7,0) атрибутировать держателей `ready_s −
   final_applied_s` сетевым журналом и health-log `load_timing`: сетевые держатели
   (запрос без ответа) против JS-шторма флашей — последний это BUG-1800 (P3), его не
   трогать, только записать в отчёт задачи.
2. Сетевые держатели: запрос картинки (и фрейма), не давший ответа к дедлайну
   (порядка 10 с, по образцу `STREAM_CSS_DEADLINE` из LOAD-2), завершается ошибкой:
   `<img>` получает `error`, `ImageRequestLedger`/`pending_window_load`
   освобождаются — окно не ждёт до `FETCH_READ_TIMEOUT` 60 с. Кратные 10 с `ready_s`
   из базы LOAD-0 (auto-ru 10,27; drom 10,2; cian 10,12; rambler 10,23; mos-ru 20,15;
   market-yandex 30,38) — первые кандидаты на проверку.
3. Повторный замер этих сайтов (`perf_audit --mode stability`), запись в
   `docs/perf/journal.md`.

**Done when:** unit-тест — картинка, висящая дольше дедлайна, не задерживает `load`
окна, у `<img>` приходит `error`; на корпусе нет `ready_s` кратных 10 с сверх
`final_applied_s` + дедлайн; статусы сайтов не ухудшаются.

**Сделано (2026-11-18):** дедлайн 10 с от старта сетевого запроса — `ImageRequestLedger.started`
(отметку ставит задача пула, очередь `WARM_POOL` стартом не считается) и метка в
`pending_new_frames`; логика — `page_load/dead_subresource.rs`. Просроченный URL уходит в
`stream_image_errors` (`<img>` получает `error` через `apply_stream_intrinsic_sizes`), фрейм
перестаёт держать `load` (`deferred_frames_busy`), бронь фрейма живёт до ответа потока. Откат —
`LUMEN_NO_LOAD_DEADLINE=1`. Живая проба (5 сайтов, зонд `LUMEN_LOAD_PROBE=1`, снят):
сетевых держателей дольше 2,7 с нет, дедлайн не срабатывает; остаток
`ready_s − final_applied_s` (36–50 с у ok-ru) — очередь задач движкового потока (BUG-1800, P3,
брифом исключён), а не сеть. Цифры — [journal](../perf/journal.md) §2026-11-18 LOAD-7.

## LOAD-9 — одна экранная раскладка на загрузку (P6, M)

**Depends on:** —. **Files:** `crates/shell/src/page_pipeline.rs` (`parse_and_layout`:
layout для скриптов и экранный layout — карта фаз в шапке этого файла),
`crates/shell/src/page_load.rs` (`LoadMarks`, LOAD-0).

1. Финальный проход делает полный layout минимум дважды: для скриптов (геометрия до
   исполнения) и экранный после DOMContentLoaded; на github полных layout-проходов
   три (THREAD-3/4), layout+paint — 70 % времени загрузки (64,3 из 90,8 с headless).
2. Если после script-layout ни DOM, ни набор листов, ни inline-стили не менялись
   (скрипты не мутировали, новых подресурсов, меняющих геометрию, нет) — экранная
   фаза переиспользует дерево прохода, второй полный layout не строится. Изменились —
   как сейчас.
3. Инструменталь: счётчик полных layout-проходов на загрузку рядом с `LoadMarks`
   (LOAD-0), виден в health-log.

**Done when:** unit-тест — одинаковый DOM даёт один layout; на github (dev-release,
`--trace-nav`) спанов `layout` на загрузку меньше на один; graphic_tests без изменений.

**Сделано (2026-11-18):** последнее полное дерево (`screen_layout`) отдаётся экранной фазе, пока
геометрию не перечеркнуло: изменения скриптов (DOM/листы/inline/adopted), мутации обработчиков
`DOMContentLoaded` (флаг грязноты снимается на время диспатча и возвращается как был — иначе «уже
грязно после скриптов» и «грязно от обработчика» не различить), intrinsic `width`/`height` от картинок
и `hide_banners` пакета. Счётчик — `ParsedPage::layout_passes`, health-log `load_timing.layout_passes`.
**Граница ворот «github −1»:** замер `--trace-nav https://github.com` даёт те же 3 прохода (2,84 / 2,05 /
2,93 с), и все три заслужены — скрипты пересобирают каскад и мутируют DOM, `DOMContentLoaded` мутирует
снова, две картинки пишут intrinsic-размеры; по брифу это «изменились — как сейчас». Переиспользование
покрывает неизменный DOM (1 проход, юнит-тест) и пост-скриптовую мутацию (2). Попутно закрыт настоящий
источник лишнего прохода: `document_adopted_fingerprint()` для пустого списка давал ненулевой хеш против
`DynamicCssBase::adopted_fp = 0` — убран. Цифры — [journal](../perf/journal.md) §2026-11-18 LOAD-9.

## LOAD-6 — параллельный матчинг селекторов начального каскада (P6, L)

**Depends on:** —. **Files:** `crates/engine/layout/src/style/` (`cascade.rs`,
`matching.rs`), вызов каскада в `crates/shell/src/page_pipeline.rs`.

1. THREAD-4 (перепись на github): layout-проход 6,5–8,0 с, из них
   `precompute_counters` 83–89 % — почти целиком `compute_style`, внутри `cs_match`
   (матчинг селекторов) 82–92 %; сама геометрия `lay_out` — 68–87 мс (~1 %). Матчинг
   каждого узла независим: кандидатов даёт `RuleIndex` (только чтение), результат
   per-node, порядок каскада в матче не участвует.
2. Срез 1: параллельный матчинг всех узлов на ограниченном пуле (чанки/уровни дерева —
   детерминизм сохраняется); `compute_style` остаётся последовательным. Срез 2 — по
   переписи после среза 1, если `compute_style`/`build_box` стали доминировать:
   распараллелить и их по уровням дерева.
3. Дифференциальный гейт: параллельный каскад байт-в-байт равен последовательному
   (семья `LUMEN_VERIFY_INCR_CASCADE`, стенд `bug935_cascade_bench`).

**Done when:** полный каскад на живом github ≤ 50 % wall time против HEAD на 8
потоках; дифференциал равен; graphic_tests и `dump_golden.py` без изменений; запись
до/после в `docs/perf/journal.md`.

**Сделано (2026-11-18):** `precompute_counters` перед `walk` матчит все элементы composed-дерева
на пуле rayon (`style/pre_match.rs`): на воркере ставится `StyleEnvSnapshot`, ancestor-bloom
восстанавливается по цепочке `FlatTree::parent_of` (без него матчинг платит 2,6–2,9× —
BUG-1800 срез 5), у каждого узла сохраняется `MatchedRule`-запись и флаг `shareable`. Последовательный
`walk` больше не сканирует кандидатов: `compute_style_shareable` находит запись и применяет её той же
«суженной» дорогой, что инкрементальный каскад (`MatchRestriction` + пустой `InvalidatedRules`), а
`match_only` отсекает в предпроходе apply, `::part`/shadow-интерьер и инлайн-декларации — их всё равно
делает `walk` с настоящим унаследованным стилем. `@scope` и shadow-правила в `MatchedRule` не входят и
матчатся в `walk`, как раньше. Откат/A/B — `LUMEN_NO_PRE_MATCH=1`; документы < 64 элементов остаются
последовательными. **Стенд** `bug935_cascade_bench` (2591 элемент, 1504 правила, лучший из 39, один
бинарь, A/B по флагу): 73,5 → 34,5 мс (47 %). **Живой github** (`--trace-nav`, две пары подряд):
сумма `precompute_counters` 19,8 → 7,7 с (39 %; по парам 31 % и 51 % — страница динамическая),
сумма `layout` 13,6 → 6,4 с (47 %). **Дифференциал:** `pre_match_tests` — обе дороги дают одинаковый
`CascadeStyles` на странице с `@media`/`@layer`/`:has()` и на документе с author shadow tree, под
`:hover`/`:focus`/`:focus-visible`; таблица покрывает все элементы и отвергает чужой лист/вьюпорт.
Попутно: `StyleEnvSnapshot` не нёс `KEYBOARD_FOCUS` — воркеры `build_box` каскадировали
`:focus-visible` как «фокус мышью».

## LOAD-8 — preconnect/dns-prefetch исполняется (P6, S)

**Depends on:** —. **Files:** `crates/shell/src/page_pipeline.rs`
(`dispatch_preload_hints` :183, `PreloadHint::Preconnect` :227),
`crates/shell/src/event_sink.rs` :33, `crates/network/src/pool.rs` / `h2/pool.rs`.

1. Сейчас хинт `preconnect`/`dns-prefetch` только попадает в сетевой журнал:
   `Event::SubresourceHintFound` — «not a fetch trigger» (`page_source.rs:617`),
   обработчика нет. Первый подресурс с нового origin платит DNS+TCP+TLS целиком — на
   CDN-тяжёлых порталах (auto-ru, pikabu) это стартовая задержка каждого нового хоста.
2. По хинту греть соединение в процессный пул PERF-13 (`pool::shared_pools`):
   `preconnect` — DNS+TCP+TLS без запроса, `dns-prefetch` — только резолв; соединение
   переиспользуется первым настоящим запросом к origin по обычному ключу пула. ≤ 4
   origin одновременно, чтобы хинты не открывали десятки сокетов.
3. Фолбэк: нет свободного слота/ошибка — тихо пропустить, хинт advisory.

**Done when:** стенд (локальный сервер, `<link rel=preconnect>` + скрипт с того же
origin через 1 с): handshake один (счётчик TCP у сервера), без хинта — два; unit-тест
ключа пула; graphic_tests не затронуты.
