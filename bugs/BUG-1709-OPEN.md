# BUG-1709 — IndexedDB: бинарные ключи (`ArrayBuffer`, typed array, `DataView`) не поддерживаются, `indexedDB.cmp` кидает не `DOMException`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-IDB-SCOUT)
**Область:** js (`crates/js/src/shim/idb_shim.js` — `_idb_is_valid_key`, сравнение ключей)

## Симптом

`indexedDB.cmp(new Uint8Array([1]), …)` и `store.put(v, new Uint8Array(...))` → `DataError: invalid key`; в части путей брошенный объект не является `DOMException` (нет `.code`). Бинарные ключи — часть Indexed DB §2.8 (порядок типов: number < date < string < binary < array).

## Как найдено

WPT-IDB-SCOUT, кластер I: 4 id, 29 сабтестов (`keyorder`, `idbfactory_cmp`, `binary-key`). Потолок +0.43 п.п. категории.

## Что делать

Принять `ArrayBuffer`/`ArrayBufferView` как валидные ключи (копия байтов при конвертации), добавить тип в сравнение и в сериализацию ключей персистентного хранилища; BigInt-ключ в `cmp` — настоящий `DOMException` `DataError`.

## Как проверить

`IndexedDB/keyorder.any.html`, `idbfactory_cmp.any.html` проходят.
