# BUG-990 — IndexedDB: у `IDBDatabase.objectStoreNames` нет `contains()`, из-за чего апгрейд схемы падает

**Статус:** OPEN
**Заведён:** 2026-09-04 (живой прогон корпуса «топ-100 зарубежных»)
**Область:** `crates/js/src/shim/*.js` (IndexedDB-шим) — `objectStoreNames` отдаётся как массив/литерал, а не `DOMStringList`
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
