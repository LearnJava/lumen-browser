# BUG-1573 — `el.style.<свойство> = …` на скроллере (или его потомке, или предке) молча сбрасывает `scrollTop` в 0

**Статус:** FIXED 2026-11-25
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 26, `css/css-page` + `css-animations` + `css-transitions` + `css-shadow` + `css-borders` + `css-scroll-snap`)
**Область:** layout/shell (повторная раскладка после мутации стиля теряет смещение прокрутки вложенного контейнера; точное место не изолировано)
**Закрыт:** 2026-11-25 — оба пути пересборки дерева теперь восстанавливают смещения прокрутки (см. «Разбор» ниже).

## Симптом

Контейнер `overflow:auto` 200×200 с четырьмя детьми по 150 px. После `c.scrollTop = 100` и записи в инлайн-стиль **любого** элемента из цепочки «сам / потомок / предок (`body`)» `scrollTop` становится 0. Сброс виден не сразу: синхронное чтение сразу после записи ещё отдаёт 100, через 50 мс — 0.

## Проба

`tests/wpt/run_smoke.py` + testharness, контейнер `.c{width:200px;height:200px;overflow:auto}`, `.c div{height:150px;width:150px}`; в каждой строке перед записью `scrollTop = 100` и пауза 100 мс, замер через 300 мс:

| действие после `scrollTop = 100` | `scrollTop` у нас | ожидается |
|---|---|---|
| `c.style.color = "red"` | 0 | 100 |
| `c.style.background = "#eee"` | 0 | 100 |
| `c.setAttribute("style", "background:#eee")` | 0 | 100 |
| `c.children[0].style.background = "red"` | 0 | 100 |
| `document.body.style.margin = "3px"` | 0 | 100 |
| `c.className = "c y"` | 100 | 100 |
| `c.setAttribute("data-x", "1")` | 100 | 100 |
| `style` соседнего (не предок) элемента | 100 | 100 |
| вставка постороннего `<p>` в `body` | 100 | 100 |

То есть сбрасывает путь «инлайн-стиль → пересчёт стиля поддерева контейнера», а не любая перераскладка.

## Как найдено

WPT-RUN-14 срез 26: `css/css-scroll-snap/snap-after-relayout/*` и `resnap-on-*` читают `scrollTop` после смены стиля скроллера и получают 0 вместо ожидаемой позиции привязки (`assert_equals: expected N but got N`).

## Что делать

Найти, где после мутации инлайн-стиля пересобирается состояние прокрутки контейнера (`collect_scroll_containers_for_js_state`, `crates/engine/layout/src/lib.rs`, и перенос смещения между раскладками) и сохранять смещение, зажимая его по новому `scrollHeight`, а не обнуляя.

## Как проверить

Проба выше; затем `run_corpus.py --prefixes css/css-scroll-snap/snap-after-relayout`.

## Разбор и фикс (2026-11-25)

Корень — все продюсеры `LayoutBox` пересобирали скролл-контейнеры с offset 0, а восстановление было только на части путей. Живой прогон (стенд `.tmp/scroll_repro.html`, отладочный `LUMEN_DBG_SCROLL`) показал обнуляющий продюсер — **engine-thread `relayout_job`** (`relayout_job.rs` звал `layout_measured_hyp_with_counters`/`layout_mutation_incremental_*` напрямую, мимо обёрток `compute_layout_*`): `scroll_offsets(prev)` там был пуст, потому что предыдущая tree уже была заменена нулевой. Закрыты все пути:

1. **Полный relayout** (`relayout` → `relayout_page`/`relayout_page_keeping_scroll`, `crates/shell/src/relayout.rs`): дерево строится с нуля, все скроллеры стартуют с offset 0. `restore_scroll_positions` вызывался только когда `sheet_has_scroll_state_queries` — для всех остальных страниц смещения терялись, а `update_scroll_states` публиковал нули в JS. Фикс: `keep_scroll` берётся из `prev layout_box` безусловно; scroll-hints без `scroll-state()` запросов инертны.
2. **Инкрементальные relayout на UI-потоке** (`compute_layout_incremental`, `compute_layout_incremental_restyle`, там же): graft копирует `scroll_x/y` из prev только для полностью переиспользованных чистых поддеревьев; пересобранный скроллер стартовал с 0, restore не вызывался. Фикс: `scroll_offsets(prev)` → `restore_scroll_positions` до `paint_ordered_in`.
3. **Engine-thread `relayout_job`** (`crates/shell/src/relayout_job.rs`): четыре продюсера (`layout_metrics_relayout`, `layout_measured_hyp_with_counters`, `layout_mutation_incremental_restyle`, `layout_mutation_incremental_with_counters`) — тот же фикс: `scroll_offsets` снимается с `self.layout_box` до `move` в job-замыкание, `restore_scroll_positions` до `paint_ordered_in` и до js-precollect.
4. **Стриминговый релэйаут при загрузке** (`crates/shell/src/page_load.rs`, `layout_streaming_incremental_*`/`layout_measured_with_counters`): та же схема — offsets из `self.layout_box` до layout, restore после.

Попутно закрыт key-space рассинхрон того же класса: три продюсера `scroll_states` для JS (`js_precollect.rs`, `about_to_wait.rs` drain, `page_load.rs` LoadDone-push) клали записи по `NodeId::index()` (без поколения слота), тогда как `style_flush.rs` и `__nid__` работают по `raw()` — для переиспользованного слота арены запись ложилась не под тот ключ. Все три переведены на `raw()`.

JS-сторона `style_flush.rs` (`restore_scroll_positions` после layout + scoped re-collect) проверена и не является источником сброса — она восстанавливает offsets из `scroll_states` map; обнуление происходило раньше, на engine-thread.
