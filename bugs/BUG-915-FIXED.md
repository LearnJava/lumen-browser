# BUG-915 — IndexedDB бросает обычный `Error` вместо `DOMException`, и `assert_throws_dom` отвергает его

**Статус:** FIXED 2026-09-18 (P3, ветка `p3-bug-915`)
**Заведён:** 2026-08-25 (P1, прогоном WPT при проверке [BUG-841](BUG-841-FIXED.md))
**Область:** `crates/js/src/shim/idb_shim.js` — `_idb_error(name, message)`: `new Error(message)` с присвоенным `name`
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
([BUG-714](BUG-714-FIXED.md)) — так что одной замены `new Error` на
`new DOMException` может не хватить, эти два надо мерить вместе.

## Масштаб

Весь IndexedDB: почти каждый подтест с `assert_throws_dom`. Тот же приём
(`new Error` + `name`) стоит поискать в соседних шимах — он не про IndexedDB,
а про способ бросать ошибки.

## Направление починки (не предписание)

`_idb_error` → `new DOMException(message, name)` с проверкой, что полифил
доступен в scope (шим evaluate-ится и в service-worker-контексте), плюс замер
против BUG-714: `code` заполняет сам полифил по legacy-таблице.

## Исправлено

`_idb_error(name, message)` в `crates/js/src/shim/idb_shim.js` теперь строит
`new DOMException(message || name, name)` вместо `new Error` с подменённым
`.name` — `code` заполняет сам `DOM_EXCEPTION_POLYFILL` по своей
legacy-таблице (`NotFoundError` → 8, `InvalidStateError` → 11 и т.д.), так что
второй половины из BUG-714 не потребовалось: полифил уже совпадал с
WebIDL-формой. Единственный вызов `_idb_error` с не-DOM именем
(`IDBCursor.prototype.advance` бросал `_idb_error('TypeError', …)`, а
`TypeError` не входит в таблицу legacy-имён DOM) заменён на настоящий
`throw new TypeError(...)`.

Новый юнит-тест `idb_errors_are_dom_exceptions_with_legacy_code`
(`crates/js/src/dom/tests/v8_idb.rs`) фиксирует все три свойства сразу:
`e instanceof DOMException`, `e.name === 'NotFoundError'`, `e.code === 8`.
`cargo test -p lumen-js --features v8-backend` — 42/42 IDB-тестов зелёные;
полный прогон крейта 3856/3858 (два предсуществующих флака,
`credentials::tests::create_and_get_through_installed_provider` (BUG-759,
TOCTOU на общем слоте) и `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty`,
оба зелёные при `--test-threads=1`, к этому фиксу не относятся).
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чист; `cargo clippy --workspace --all-targets -- -D warnings` (финальный гейт)
чист.
