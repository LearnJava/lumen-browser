# BUG-577: `Event.prototype.composedPath()` missing entirely

**Статус:** FIXED 2026-09-10 (P6, попутно с [BUG-873](BUG-873-FIXED.md))
**Компонент:** js (`crates/js/src/dom.rs:3342-3358` — `Event.prototype` gets
`preventDefault`/`stopPropagation`/`stopImmediatePropagation` but no
`composedPath`; every other event constructor — `CustomEvent`, `UIEvent`,
`MouseEvent`, `KeyboardEvent`, `InputEvent`, `FocusEvent`, `WheelEvent`, … —
chains its prototype off `Event.prototype` via `Object.create`, so the gap is
inherited by all of them)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - event.composedPath is not a function
```

288 occurrences across the slice's 14 subdirectories — the single largest
error cluster in the run, ahead of the already-tracked
[BUG-574](BUG-574-OPEN.md) `Node.prototype.contains()` gap (178
occurrences in this same slice).

## Причина

`Event.prototype.composedPath()` (DOM §2.8, returns the event's dispatch
path — the sequence of nodes/shadow-hosts the event passed through) is not
implemented on any event type. `Event` (`dom.rs:3342`) only installs
`preventDefault`/`stopPropagation`/`stopImmediatePropagation`
(`dom.rs:3354-3358`); no code path anywhere in `dom.rs` sets
`this._path`/similar during dispatch or defines a `composedPath` accessor.

## Масштаб

Large and cross-cutting: `composedPath()` is called by generic
event-inspection helpers (assertion libraries, shadow-DOM boundary checks,
and — per the `resources/testdriver.js`-shared-helper precedent documented
for BUG-574 — plausibly some of the common WPT test infrastructure itself),
not just by tests specific to one feature area. Given the scale (288 hits in
one 444-id slice, larger even than the `contains()` gap that BUG-574 found
propagating noise into already-closed slices), this is a strong candidate to
also be re-checked against already-closed WPT-VENDOR slices for unexplained
`Unhandled rejection`/generic-assertion noise, the same way BUG-574 flagged
itself for future re-audit. Not investigated further here (P2 WPT-survey
scope, not a fix).

## Починка 2026-09-10 (P6, E2E-4 итерация 5)

Ждала не своей реализации, а чужой: `composedPath()` возвращает путь события,
а пути события в движке не существовало вовсе — были два независимых неполных
обхода (разбор в [BUG-873](BUG-873-FIXED.md)). Как только §2.9-обход появился,
метод стал тремя строчками поверх готовых данных.

`crates/js/src/shim/web_api_shim_head.js`: `_lumen_propagate` кладёт
построенный путь в `event._path` на время одного диспатча и снимает его в
конце, а `Event.prototype.composedPath()` разворачивает его в объекты
(`_lumen_path_target`). Вне диспатча — пустой массив, как и требует спека для
события «не в полёте». Заодно объявлены четыре константы `eventPhase`
(`Event.AT_TARGET` и `e.AT_TARGET` — обе живые идиомы).

Метод определён на `Event.prototype`, поэтому его получают все подтипы разом —
ровно та цепочка `Object.create(Event.prototype)`, из-за которой пробел и был
сквозным.

**Ограничение:** ретаргетинг по composed-дереву не смоделирован, так что путь,
пересекающий границу shadow root, перечисляет настоящие узлы, а не хост. Это
отдельный пробел (теневое дерево в движке в целом не ретаргетится), а не
остаток этого бага.

**Замер:** `composed_path_lists_the_whole_chain_and_is_empty_outside_dispatch`
в `crates/js/src/dom/tests/v8_event_propagation.rs` — из вложенного узла путь
читается как `inner>outer>BODY>HTML>document>window`, у неотправленного
события длина 0. Цена по WPT (288 отказов в срезе
`WPT-VENDOR-html-semantics-misc`) этой правкой не перемерена — прогон среза
принадлежит P2.
