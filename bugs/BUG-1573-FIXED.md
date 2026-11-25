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

Корень — два независимых пути пересборки `LayoutBox`, оба обнуляли `scroll_x/scroll_y` пересобранных скролл-контейнеров, а восстановление существовало только на одном из них и только для страниц с `@container scroll-state()`:

1. **Полный relayout** (`relayout_page` → `compute_layout`): дерево строится с нуля, все скроллеры стартуют с offset 0. `restore_scroll_positions` вызывался только когда `sheet_has_scroll_state_queries` — для всех остальных страниц смещения терялись, а `update_scroll_states` публиковал нули в JS. Фикс: `keep_scroll` берётся из `prev layout_box` безусловно (`crates/shell/src/relayout.rs`); scroll-hints, которые ставил scroll-state путь, без `scroll-state()` запросов инертны.
2. **Инкрементальные relayout** (`compute_layout_incremental`, `compute_layout_incremental_restyle`): graft копирует `scroll_x/y` из prev только для полностью переиспользованных чистых поддеревьев (`incremental.rs`); пересобранный скроллер (свой стиль dirty или dirty-потомок) стартовал с 0, и `restore_scroll_positions` в этих путях не вызывался вовсе. Фикс: `scroll_offsets(prev)` → `restore_scroll_positions(&mut layout, …)` до `paint_ordered_in` в обоих функциях — то же правило, что у полного пути.

Регресс-тест: `scroll_offsets_survive_scroller_own_style_mutation` в `crates/js/src/dom/tests/v8_bug504_scroll_flush.rs` (JS-рантайм-путь; shell-путь покрывается тем же `restore_scroll_positions`).

JS-сторона `style_flush.rs` (`restore_scroll_positions` после layout + scoped re-collect) проверена и не является источником сброса — она восстанавливает offsets из `scroll_states` map; обнуление происходило раньше, на engine-thread, в `relayout_job`.
