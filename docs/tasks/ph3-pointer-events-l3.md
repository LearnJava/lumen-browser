# Задача: Pointer Events L3 — coalesced / predicted events

**Developer:** P1
**Ветка:** `p1-pointerfull`
**Размер:** M
**Крейты:** `lumen-js`, `lumen-shell`

## Goal
Реализовать реальный `getCoalescedEvents()` / `getPredictedEvents()` из Pointer
Events Level 3 §4.1: собирать промежуточные `pointermove`-события, пришедшие
между двумя кадрами, и отдавать их через `getCoalescedEvents()` на «главном»
событии; опционально формировать предсказанные точки для `getPredictedEvents()`.
Сейчас оба метода возвращают пустые/одноэлементные заглушки.

## Current state (сверено с кодом 2026-07-05)
- Базовый L2 `PointerEvent` работает; L3-методы — **заглушки**:
  - `PointerEvent.prototype.getCoalescedEvents = function() { return []; }` и
    `getPredictedEvents = () => []` — `crates/js/src/dom.rs:3310-3311`.
  - В `_lumen_dispatch_pointer_event` (диспетч из shell) на конкретном событии:
    `ev.getCoalescedEvents = () => [ev]` (одноэлементный),
    `ev.getPredictedEvents = () => []` — `dom.rs:3764-3766`.
  - Аналогичные заглушки в других dispatch-путях: `dom.rs:3738-3739`
    (`_lumen_dispatch_*` mouse-derived) и `dom.rs:3780-3781`
    (`_lumen_dispatch_capture_event`).
  - Комментарий прямо фиксирует ограничение: «Level 3: … single event, no
    coalescing» — `dom.rs:3764`.
- Shell шлёт события по одному: `js_pointer_event` формирует строку
  `_lumen_dispatch_pointer_event(nid, type, x, y, button, buttons, mod)` —
  `crates/shell/src/main.rs:12020-12036`. Промежуточные `CursorMoved` **не
  накапливаются** — каждый winit `CursorMoved` (`main.rs:9082`) при движении
  напрямую вызывает одиночный dispatch, без буфера «между кадрами».
- Тесты-заглушки, которые придётся расширить/переписать:
  `dom.rs:21554` (проверяют лишь `Array.isArray`) и
  `dom.rs:26117-26126` (`getCoalescedEvents().length === 1`).

## Entry points
- `crates/shell/src/main.rs:9082` — обработчик `WindowEvent::CursorMoved`
  (источник сырых move-событий; здесь копить буфер).
- `crates/shell/src/main.rs:12025` — `js_pointer_event` (расширить сигнатуру
  вызова, чтобы передать список coalesced-точек).
- `crates/js/src/dom.rs:3747` — `_lumen_dispatch_pointer_event` (принять
  массив coalesced и навесить реальные `getCoalescedEvents`).
- `crates/js/src/dom.rs:3310` — прототипные заглушки (оставить как безопасный
  дефолт для событий, созданных из JS).

## Срезы (декомпозиция)
### Срез 1 — S — буфер coalesced-move в shell
В обработчике `CursorMoved` (`main.rs:9082`) не диспатчить каждое движение сразу,
а копить `(x, y, buttons, mod, timestamp)` в `Vec` на структуре окна. Флашить
буфер один раз за «кадр» — на `RedrawRequested`/`AboutToWait` или при следующем
не-move событии (press/release). Последняя точка буфера — «главное»
`pointermove`, остальные — coalesced. Убедиться, что press/release/enter/leave
флашат буфер до себя (порядок событий не нарушается).

### Срез 2 — S — проброс списка coalesced в JS
Расширить `js_pointer_event` (`main.rs:12025`): сериализовать промежуточные
точки (например JSON-массив `[[x,y,buttons,mod],...]`) и передать доп. аргументом
в `_lumen_dispatch_pointer_event`. Аккуратно с экранированием — сейчас строка
собирается `format!` без JSON (`main.rs:12028`).

### Срез 3 — S — реальный getCoalescedEvents в диспетчере
В `_lumen_dispatch_pointer_event` (`dom.rs:3747`) построить массив `PointerEvent`
из переданных coalesced-точек (каждая — полноценный `PointerEvent` с теми же
`pointerId`/`pointerType`, своими `clientX/Y`), и навесить
`ev.getCoalescedEvents = () => coalescedArray` (главное событие входит последним,
как требует спек §4.1). Заглушку `dom.rs:3765` заменить.

### Срез 4 — S — getPredictedEvents (линейная экстраполяция)
Реализовать простое предсказание: по последним 2-3 coalesced-точкам
экстраполировать 1-2 будущие позиции (линейно по скорости) и вернуть их из
`getPredictedEvents()` как `PointerEvent`-ы. Это допустимая реализация — спек не
требует конкретного алгоритма. При недостатке точек → `[]`.

### Срез 5 — XS — единообразие прочих dispatch-путей
Привести заглушки в `dom.rs:3738-3739` и `dom.rs:3780-3781`
(`_lumen_dispatch_capture_event`) к общему поведению: capture-события coalesced
не имеют (пустой массив корректен), но проверить, что они не ломают контракт
(getCoalescedEvents всегда возвращает массив). Прототипный дефолт `dom.rs:3310`
оставить для JS-созданных событий.

## Tests
- JS-интеграция (`dom.rs`, рядом с `:26117`): при dispatch с несколькими
  coalesced-точками `ev.getCoalescedEvents().length === N`, каждый элемент —
  `PointerEvent`, координаты соответствуют переданным, последний === главное
  событие; `getPredictedEvents()` возвращает массив (длина ≥0).
- Shell-логика буфера: юнит на накопление/флаш (если поддаётся — вынести
  буферизацию в тестируемую функцию), либо ручной сценарий в `graphic_tests`
  с логом `getCoalescedEvents().length` > 1 при быстром движении.
- Регрессия: одиночное движение по-прежнему даёт `length === 1`
  (обновить существующий тест `dom.rs:26125`).

## Progress (2026-07-17) — реализовано полностью

**Обнаружено при сверке с кодом:** бриф (2026-07-05) утверждал, что каждый
`CursorMoved` уже диспетчит одиночный `pointermove`. Это устарело — реальный
код на момент старта задачи диспетчил `pointermove`/`mousemove` только для
инъецированного (`InputCommand::MouseMove`, automation) пути через
`dispatch_mouse_move`; настоящий `CursorMoved` от winit диспетчил только
hover-переходы (`pointerover`/`pointerout`/`pointerenter`/`pointerleave`), а
не сам факт движения. Срез 1 поэтому не просто «добавил буфер», а завёл
первый в принципе flush непрерывного `pointermove` для реального ввода мыши.

- `crates/shell/src/main.rs`: новое поле `Lumen::pending_pointer_moves:
  Vec<(f32, f32)>` (CSS-px, хронологический порядок). `CursorMoved`-обработчик
  кладёт в него каждый сырой сэмпл (в блоке CSS `:hover`-трекинга, после
  панели resize/DnD/Pointer-Lock ранних `return`). Новый метод
  `flush_pointer_moves()` — hit-test по ПОСЛЕДНЕЙ точке буфера (как
  `dispatch_mouse_move`, включая `pointer_capture_nid()`-редирект), сериализует
  весь буфер в JSON-массив `[[x,y],...]` и вызывает
  `_lumen_dispatch_pointer_move_coalesced`; no-op на пустом буфере или если
  последняя точка не попадает ни в один элемент (курсор над chrome).
  Флашится: раз за тик в `about_to_wait` (после ожидания `pending_waits`, до
  инъецированного input) — «раз за кадр»; и до-события в hover-переходе
  (`pointerout`/`leave`/`over`/`enter`), `pointerdown`, `pointerup`,
  `CursorLeft` — везде «flush перед событием», порядок сохранён.
- `crates/js/src/dom.rs`: новая `_lumen_dispatch_pointer_move_coalesced(nid,
  points_json, button, buttons, mod)` — строит один `PointerEvent` на точку,
  главное событие (последняя точка) диспетчится через `_lumen_dispatch_rich`;
  `getCoalescedEvents()` — весь список, главное событие последним (по
  ссылке, не копия). `getPredictedEvents()` — линейная экстраполяция по
  вектору последних двух точек (2 прогнозные точки; < 2 сэмплов → `[]`).
  Зарегистрирована в объекте `window` рядом с сестринскими
  `_lumen_dispatch_*`.
- `_lumen_dispatch_pointer_event` (не-move типы: down/up/enter/leave/over/out
  + одноразовый синтетический pointermove из pointer-lock/automation) и
  `_lumen_dispatch_capture_event` — заглушки `[ev]`/`[]` оставлены как есть:
  они уже были спек-корректны для одиночного/некоалесцируемого события, не
  требовали замены (комментарии уточнены, чтобы это не читалось как TODO).
- 3 новых JS-теста (`dom.rs`, рядом с `pointer_event_get_coalesced_events_returns_array`):
  `pointer_move_coalesced_dispatch_single_point`,
  `pointer_move_coalesced_dispatch_multi_point` (3 точки — коалесцированный
  список верный по порядку, главное событие последнее по ссылке,
  предсказанные точки соответствуют линейной экстраполяции),
  `pointer_move_coalesced_dispatch_empty_batch_is_noop`. Все зелёные на
  default (QuickJS) и `--features v8-backend`.
- Существующие тесты `pointer_event_get_coalesced_events_returns_array`
  (single-event через `_lumen_dispatch_pointer_event`) не менялись — их
  поведение не затронуто (не регрессия). Строки `dom.rs:21554`/`26117` из
  исходного брифа относились к File API тестам — дрейф номеров строк
  (бриф от 2026-07-05), не Pointer Events.

## Definition of done
- [x] Shell копит промежуточные `pointermove` и флашит покадрово.
- [x] `getCoalescedEvents()` возвращает реальные промежуточные события
      (порядок по спеку, главное — последнее).
- [x] `getPredictedEvents()` возвращает экстраполированные точки (или `[]`).
- [x] press/release/enter/leave корректно флашат буфер (нет потери порядка).
- [x] Заглушки `dom.rs` (`_lumen_dispatch_pointer_event`,
      `_lumen_dispatch_capture_event`) выверены — корректны для
      некоалесцируемых типов, не требовали замены; комментарии уточнены.
- [x] Новые тесты на реальную коалесацию/предсказание добавлены и зелёные
      (default + `--features v8-backend`).
- [x] `CAPABILITIES.md` — Pointer Events L3 уточнено (coalesced/predicted).
