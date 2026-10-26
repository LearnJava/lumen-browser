# BUG-1706 — IndexedDB: слушатель, добавленный внутри обработчика события, получает то же событие (живой массив слушателей при диспетче)

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js (`crates/js/src/shim/idb_shim.js` / `dom.rs` — `_idb_dispatch_request`, `_idb_fire_txn`)

## Симптом

```js
const r = indexedDB.open(name, 1);
r.onupgradeneeded = () => {
  // добавляем слушатель на тот же запрос уже во время диспетча upgradeneeded
  r.addEventListener('upgradeneeded', () => calls++);
};
```

Добавленный слушатель вызывается для того же события (`calls === 1`). По DOM §2.9 (`inner invoke`) список слушателей клонируется перед обходом; слушатели, добавленные во время диспетча, текущим событием не вызываются. Диспетчер шима обходит живой массив.

## Как найдено

WPT-IDB-SCOUT, кластер D: 48 id, 242 сабтеста, сообщение «Expected success event, but got upgradeneeded event». `EventWatcher`, создаваемый внутри `onupgradeneeded` (`promiseForRequest` в `migrateNamedDatabase`), получает то же `upgradeneeded`, и тест ждёт `success`, а видит апгрейд; часто весь тест уходит в TIMEOUT. Потолок +5.6 п.п. категории.

## Что делать

Перед обходом копировать массив слушателей (`slice()`), так же — в `_idb_fire_txn`, `_idb_dispatch_request` и у `IDBDatabase`. Это пересекается с [BUG-914](BUG-914-OPEN.md) (переписывание диспетчера под всплытие) — делать вместе.

## Как проверить

Проба выше: `calls === 0`; `run_corpus.py --prefixes IndexedDB` — id с сообщением «Expected success event» исчезают.
