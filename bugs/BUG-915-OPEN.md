# BUG-915 — IndexedDB бросает обычный `Error` вместо `DOMException`, и `assert_throws_dom` отвергает его

**Статус:** OPEN
**Заведён:** 2026-08-25 (P1, прогоном WPT при проверке [BUG-841](BUG-841-FIXED.md))
**Область:** `crates/js/src/dom.rs:14615` — `_idb_error(name, message)`: `new Error(message)` с присвоенным `name`
**Владелец:** P1 (`lumen-js`)

## Симптом

```js
try { tx.objectStore('nope'); }
catch (e) {
    e.name;                      // 'InvalidStateError' — верно
    e.code;                      // undefined, спека требует 11
    e instanceof DOMException;   // false
}
```

`assert_throws_dom` из `testharness.js` проверяет и `code`, и принадлежность к
`DOMException`, поэтому подтест валится с формулировкой «threw object … that is
not a DOMException … property "code" is equal to undefined, expected 11» —
хотя проверяемое поведение движка правильное.

## Прямое измерение (2026-08-25, dev-release, `run_smoke.py`)

`/IndexedDB/idbtransaction-objectStore-exception-order.any.html` после
починки BUG-841 перестал быть TIMEOUT (харнесс доходит до конца, `Test OK`),
и его единственный подтест валится **только** на этом: порядок проверок
`InvalidStateError` перед `NotFoundError` уже правильный. В
`/IndexedDB/idbobjectstore_createIndex.any.html` на ту же причину приходится
6 из 13 неожиданных результатов.

## Причина

`_idb_error` — единственный конструктор ошибок всего IndexedDB-шима (≈40 мест:
`InvalidStateError`, `NotFoundError`, `TransactionInactiveError`,
`ConstraintError`, `DataError`, `ReadOnlyError`, `AbortError`, `VersionError`),
и он делает `new Error`. Глобальный `DOMException` в движке есть
(`DOM_EXCEPTION_POLYFILL`, `crates/js/src/v8_runtime.rs`), шим просто им не
пользуется.

Отдельно и раньше: сам полифил не совпадает с WebIDL-формой legacy-исключения
([BUG-714](BUG-714-OPEN.md)) — так что одной замены `new Error` на
`new DOMException` может не хватить, эти два надо мерить вместе.

## Масштаб

Весь IndexedDB: почти каждый подтест с `assert_throws_dom`. Тот же приём
(`new Error` + `name`) стоит поискать в соседних шимах — он не про IndexedDB,
а про способ бросать ошибки.

## Направление починки (не предписание)

`_idb_error` → `new DOMException(message, name)` с проверкой, что полифил
доступен в scope (шим evaluate-ится и в service-worker-контексте), плюс замер
против BUG-714: `code` заполняет сам полифил по legacy-таблице.
