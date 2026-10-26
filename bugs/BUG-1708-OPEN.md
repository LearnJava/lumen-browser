# BUG-1708 — IndexedDB: `createObjectStore`/`createIndex` принимают невалидный `keyPath`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js (`crates/js/src/shim/idb_shim.js` — проверка `keyPath`)

## Симптом

`db.createObjectStore('s', {keyPath: 'j a'})`, `'a..b'`, `'.a'` проходят без ошибки; по спецификации (valid key path: идентификаторы ECMAScript, разделённые точками) — `SyntaxError` `DOMException`.

## Как найдено

WPT-IDB-SCOUT, кластер G: 3 id, 26 сабтестов (`keypath_invalid`, `idbobjectstore_createIndex` с невалидным путём). Потолок +0.16 п.п. категории.

## Что делать

Валидировать `keyPath` (строка или непустой массив строк): каждый сегмент — `IdentifierName`, либо пустая строка допустима только как целый путь; иначе `SyntaxError`. Пустой массив — `SyntaxError`.

## Как проверить

`IndexedDB/keypath_invalid.any.html` проходит.
