# BUG-1512 — Scroll-state container queries (`container-type: scroll-state`, `@container scroll-state(stuck|snapped|scrollable|scrolled)`) не реализованы

**Статус:** OPEN (ДОРАБОТКА → CQ-SCROLL-STATE-3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser/layout (`container-type: scroll-state`, `@container scroll-state(…)`)

## Симптом

`CSS.supports('container-type','scroll-state')` — `true`, но запрос `@container scroll-state(scrollable: bottom){…}` не срабатывает на контейнере с `container-type:scroll-state;overflow:auto` и прокручиваемым содержимым; разбор условия не знает `scroll-state()`, `cssRules` пуст (BUG-1455), все `*-parsing`/`*-serialization` падают. Состояния `stuck`, `snapped`, `scrollable`, `scrolled` (`scroll-state-*`, 34 теста) не вычисляются; для `stuck` нужен `position:sticky` (BUG-1468).

## Проба

Проба (`--mcp`): `.c{container-type:scroll-state;overflow:auto;width:100px;height:50px}` + содержимое 300px + `@container scroll-state(scrollable:bottom){#t{color:green}}` → `black`, ожидается `green`.

## Как найдено

WPT-RUN-14 срез 22: `container-queries/scroll-state/*` — 54 id, 203 из 211 сабтестов (`at-container-{scrollable,scrolled,snapped,stuck}-{parsing,serialization}` — 8 id; `scroll-state-*` — 46 id).

## Что делать

Разбор `scroll-state()` (BUG-1455 для CSSOM), состояния прокрутки контейнера в запросах и их инвалидация по прокрутке.

## Как проверить

`css/css-conditional/container-queries/scroll-state/scroll-state-scrollable-axis.html`.

## Срез 1 (CQ-SCROLL-STATE, 2026-10-21, P1) — влит

- `container-type: [size | inline-size] || scroll-state` (`ComputedStyle::container_scroll_state`); `getComputedStyle` отдаёт `container-type`/`container-name`.
- `@container scroll-state(…)` разбирается и оценивается (`style/container_scroll.rs`): `scrollable` по геометрии и `scroll_x/y` (`scroll_state.rs`), `scrolled`/`stuck`/`snapped` пока всегда «нет».
- Условия трёхзначные (`eval_tri`): неизвестное и его отрицание не применяются; контейнер запроса выбирается цепочкой `ContainerContext::parent`; списки `@container a (…), b (…)`; range-синтаксис.
- Правила `@container` выставляют custom properties; элементы без бокса получают результат через `CounterMap::cq_boxless` + `collect_custom_properties_for`.
- WPT `css/css-conditional/container-queries/scroll-state/`: 39 → 161 из 237 субтестов; `scroll-state-scrollable-axis.html` и `scroll-state-scrolled-programmatic-absolute-scrolls.html` зелёные.
- Остаток — задача CQ-SCROLL-STATE-2 (ROADMAP.md).

## Срез 2 (CQ-SCROLL-STATE-2, 2026-10-24, P3) — влит

- Корень: запросы `scroll-state()` отвечают один раз за проход раскладки, а свежее дерево начинается с нулевых смещений (прежние возвращает `restore_scroll_positions` уже после прохода). Теперь на время раскладки ставятся подсказки (`lumen_layout::install_scroll_hints`/`ScrollHints`: смещения контейнеров по `NodeId::raw()` и смещение страницы), `scroll_state_info` читает их вместо `scroll_x/y` и зажимает по границам прокрутки.
- JS: `FlushHandles::maybe_flush` берёт подсказки из `scroll_states` и позиции страницы (в том числе из ещё не применённого `scrollTo`); `note_scroll_moved` (`_lumen_request_scroll`, `_lumen_request_page_scroll[_x]`, `update_scroll_states`, `set_page_scroll_y/x`) помечает флаш устаревшим и отключает инкрементальный путь — такая таблица проходит раскладку целиком. Тесты `v8_cq_scroll_state_invalidation`.
- Оболочка: `Lumen::relayout` при таблице со `scroll-state()` берёт смещения текущего дерева (`relayout_page_keeping_scroll`: подсказки + `restore_scroll_positions` до сборки списка отображения); `restyle_scroll_state_queries` вызывается из `apply_container_scroll` (колесо), из очереди запросов `scrollTop` (`about_to_wait`) и по смещению страницы, если корень — контейнер (`RedrawRequested`).
- Корень документа (`html`) — контейнер `scroll-state` по вьюпорту (`ViewportRole::Root`); `body` с ушедшим во вьюпорт `overflow` по-прежнему не отвечает.
- Попутно: `el.style.overflow = ''` (и другие 2-значные шорткаты, `block-step`) убирал не все продольные объявления — после `overflow: initial` в `style` оставалось `overflow: initial`.
- WPT `css/css-conditional/container-queries/scroll-state/`: 161 → 174 из 237 субтестов (`scrollable-change`, `scrollable-root`, `scrollable-container-type-change` зелёные).
- Остаток — CQ-SCROLL-STATE-3 (ROADMAP.md): `scrolled`, `stuck`, `snapped`, `scrollable-body-002` (границы прокрутки страницы при `body{height; overflow}`), цели-псевдоэлементы, `var()` в запросе.
