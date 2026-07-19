# BUG-317: `MutationRecord` не выставлен как глобальный интерфейс

**Renumbered 2026-07-18** from `BUG-315` — collided with the real `BUG-315`
(testharnessreport route + persistent-cache fix, already `FIXED` on
`origin/main`), resolved while merging S6/S7 back into `main`.

**Статус:** FIXED 2026-07-20 (P3)
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S6, курируемый асинхронный DOM-сабсет через `wptrunner`

## Симптом

Интерфейс `MutationRecord` не выставлен на глобальном объекте — `MutationRecord
is not defined`. Колбэк MutationObserver вызывается корректно (асинхронно, через
microtask), но передаваемые записи нельзя проверить через `instanceof
MutationRecord`.

Наблюдаемый провал:

- `dom/nodes/MutationObserver-callback-arguments.html` →
  `Callback is invoked with |this| value of MutationObserver and two arguments`
  → `MutationRecord is not defined` (`expected: FAIL`).

Та же семья, что [BUG-314](BUG-314-FIXED.md) (DOM-конструкторы не выставлены как
глобали).

## Ожидание

DOM Standard §4.3.3: `MutationRecord` доступен как глобальный интерфейс;
записи, передаваемые в колбэк `MutationObserver`, — его экземпляры. Реализовать
в engine-agnostic `WEB_API_SHIM`.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/MutationObserver-callback-arguments.html
```

## Фикс (2026-07-20, P3)

Реализовано в engine-agnostic `WEB_API_SHIM` (`crates/js/src/dom.rs`), паттерн
[BUG-314](BUG-314-FIXED.md):

1. `MutationRecord` добавлен как неконструируемый интерфейс-global — top-level
   `function MutationRecord() { throw new TypeError('Illegal constructor'); }`
   (становится голым идентификатором и свойством `globalThis`) + явное
   `window.MutationRecord = MutationRecord` рядом с сиблингом
   `window.MutationObserver`.
2. Каждая запись, собираемая в `_mo_notify`, получает `MutationRecord.prototype`
   в качестве `[[Prototype]]` через `Object.setPrototypeOf(rec, ...)` перед
   постановкой в очередь — так `record instanceof MutationRecord` выполняется
   (DOM §4.3.3). Собственные data-свойства литерала записи (`type`/`target`/…)
   имеют приоритет над цепочкой прототипа.

Регресс закрыт двумя юнит-тестами (`mutation_record_is_interface_global`,
`mutation_observer_records_are_mutation_record_instances`).

**Остаток (не в этом фиксе):** корректный учёт записей MutationObserver
(дубли/`takeRecords`/subtree) — [BUG-318](BUG-318-OPEN.md); поля-узлы записи
(`addedNodes`/`removedNodes` как `NodeList` из живых узлов, `nextSibling`/
`previousSibling`) остаются упрощёнными.
