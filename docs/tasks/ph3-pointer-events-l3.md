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

## Definition of done
- [ ] Shell копит промежуточные `pointermove` и флашит покадрово.
- [ ] `getCoalescedEvents()` возвращает реальные промежуточные события
      (порядок по спеку, главное — последнее).
- [ ] `getPredictedEvents()` возвращает экстраполированные точки (или `[]`).
- [ ] press/release/enter/leave корректно флашат буфер (нет потери порядка).
- [ ] Заглушки `dom.rs:3765`, `3739`, `3781` заменены/выверены.
- [ ] Тесты `dom.rs:21554`, `26117` обновлены под реальное поведение.
- [ ] `CAPABILITIES.md` — Pointer Events L3 🟡 → ✅.
