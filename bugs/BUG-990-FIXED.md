# BUG-990 — IndexedDB: у `IDBDatabase.objectStoreNames` нет `contains()`, из-за чего апгрейд схемы падает

**Статус:** FIXED 2026-09-05
**Заведён:** 2026-09-04 (живой прогон корпуса «топ-100 зарубежных»)
**Область:** `crates/js/src/shim/idb_shim.js` (IndexedDB-шим) — `objectStoreNames`/`indexNames` отдавались как массив/литерал, а не `DOMStringList`
**Владелец:** P3

## Симптом

Типовой пролог апгрейда схемы падает на любом сайте, который его использует:

```js
db.onupgradeneeded = e => {
  const db = e.target.result;
  if (!db.objectStoreNames.contains('store')) db.createObjectStore('store');
};
```

```
[JS error] IDB onupgradeneeded: TypeError: db.objectStoreNames.contains is not a function
[JS error] IDB open onsuccess: TypeError: r.objectStoreNames.contains is not a function
```

Последствие каскадное — хранилище не создаётся, и следом летит:

```
[unhandled-rejection] NotFoundError: no object store named ufpInfo
[JS] TransactionInactiveError: transaction is not active
```

## Что говорит измерение

Прогон 100 сайтов 2026-09-04: ошибка на **claude.ai** и **weibo.com**
(две независимые реализации), плюс сопутствующие `NotFoundError` на них же.

Не путать с [BUG-842](BUG-842-FIXED.md) (самоперевзводящийся `keep_alive`) — тот
закрыт и про другое.

## Спека

IndexedDB §3.2: `IDBDatabase.objectStoreNames` и `IDBObjectStore.indexNames` —
`DOMStringList`, у которого есть `length`, `item(i)` и **`contains(str)`**.
`DOMStringList` — не `Array`, метода `includes` у него нет, поэтому сайты пишут
именно `contains`.

## Объём

Проверить обе точки — `objectStoreNames` у базы и `indexNames` у стора, — а также
что тип ведёт себя как `DOMStringList` (индексный доступ + `length` + `item` +
`contains`), а не как массив.

## Сырые данные

`.tmp/perf-audit/20260904-150604/results.json` (slug `claude`, `weibo`),
`health.log` (события `console_error`).

## Фикс (2026-09-05, P3)

Добавлен хелпер `_idb_string_list(names)` (`idb_shim.js`) — оборачивает
отсортированный снимок имён в DOMStringList-подобный объект (`length` +
индексный доступ + `item()` + `contains()`), не наследуясь от `Array`.
Подключён во всех трёх точках:

- `IDBDatabase.prototype.objectStoreNames` (геттер, уже пересчитывал список
  из `_data.stores` на каждое обращение — просто оборачивает результат);
- `IDBObjectStore.prototype.indexNames` (тот же паттерн, из `_store.indexes`);
- `IDBTransaction.prototype.objectStoreNames` — раньше было полем-массивом,
  мутируемым напрямую (`createObjectStore` пушил в него новое имя во время
  апгрейда); теперь мутируемое состояние живёт в приватном поле
  `_storeNames`, наружу отдаётся только через геттер-обёртку.

Единственный внутренний потребитель, полагавшийся на Array-семантику
(`.join('/')` в тесте `idb_abort_reverts_upgrade_schema_and_version`),
переписан на `Array.prototype.join.call(...)` — ровно то, что пришлось бы
делать и реальному сайту, и ровно то поведение настоящего `DOMStringList`.

Регресс-тест `idb_object_store_names_and_index_names_expose_dom_string_list`
(`crates/js/src/dom/tests/v8_idb.rs`) проверяет `contains()`/`item()` на
`objectStoreNames` и `indexNames` до и после создания стора/индекса.

`cargo test -p lumen-js --features v8-backend` 3464/3464 (кроме заранее
известного [BUG-997](BUG-997-OPEN.md), не связан с этой правкой),
`cargo clippy --workspace --all-targets -- -D warnings` чисто.
