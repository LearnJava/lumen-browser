# BUG-774 — `StorageEvent.prototype.initStorageEvent` не делает WebIDL-коэрсию/default-подстановку аргументов

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::StorageEvent.prototype.initStorageEvent`, ~строка 636)
**Найден:** P2, WPT-VENDOR-webstorage, 2026-08-18 — `run_report.py --all --root webstorage --recursive`

## Симптом

Текущая реализация:

```js
StorageEvent.prototype.initStorageEvent = function(type, bubbles, cancelable, key, oldValue, newValue, url, storageArea) {
    this.type = type; this.bubbles = !!bubbles; this.cancelable = !!cancelable;
    this.key = key; this.oldValue = oldValue; this.newValue = newValue;
    this.url = String(url); this.storageArea = storageArea;
};
```

Спека требует WebIDL-коэрсию каждого аргумента по его типу
(`type`/`url` — `DOMString`, коэрсируются через `ToString`; `key`/
`oldValue`/`newValue` — `DOMString?`, коэрсируются через `ToString`, но
**отсутствующий/`undefined` аргумент подставляет дефолт `null`**, а не
проходит `ToString(undefined)`). Текущий код присваивает аргументы
как есть, без какой-либо коэрсии для `type`/`key`/`oldValue`/`newValue`, и
не подставляет дефолты для отсутствующих параметров — параметр, который не
был передан вовсе, остаётся `undefined` вместо `null`.

## Подтверждённые провалы (`tests/wpt/webstorage/event_initstorageevent.window.js`)

- **`initStorageEvent` с 1 аргументом** (`event.initStorageEvent('type')`):
  ожидается `event.key === null` (дефолт для непереданного параметра),
  реально `event.key === undefined` — `this.key = key` присваивает
  буквально `undefined`.
- **С 8 `null`-аргументами**: ожидается `event.type === "null"` (строка,
  `ToString(null)`), реально `event.type === null` (объект) — нет `String()`
  коэрсии для `type`.
- **С 8 `undefined`-аргументами**: ожидается `event.type === "undefined"`
  (явно переданный `undefined` — не то же самое, что «аргумент не передан»,
  коэрсируется в строку), реально `event.type === undefined`.

## Предлагаемый фикс

```js
StorageEvent.prototype.initStorageEvent = function(type, bubbles, cancelable, key, oldValue, newValue, url, storageArea) {
    this.type = String(type);
    this.bubbles = !!bubbles;
    this.cancelable = !!cancelable;
    this.key = (key === undefined) ? null : (key === null ? null : String(key));
    this.oldValue = (oldValue === undefined) ? null : (oldValue === null ? null : String(oldValue));
    this.newValue = (newValue === undefined) ? null : (newValue === null ? null : String(newValue));
    this.url = (url === undefined) ? '' : String(url);
    this.storageArea = (storageArea === undefined) ? null : storageArea;
};
```

(`key`/`oldValue`/`newValue` — `DOMString?`: `null` остаётся `null`,
любое другое значение, включая явный `undefined`, коэрсируется в строку —
спека трактует явно переданный `undefined` как «нет значения» только для
параметров без default-значения в сигнатуре IDL; здесь у всех восьми
параметров есть `= null`/`= ""` default, поэтому `undefined`-аргумент
подставляет тот default, а не коэрсируется в строку `"undefined"` — **важно
свериться с точным текстом IDL перед фиксом**, тестовый файл проверяет оба
режима на разных сабтестах, см. `initStorageEvent with 8 undefined arguments`
против `initStorageEvent with 1 argument`.)

Не расследовано отдельно: тот же класс отсутствующей WebIDL-коэрсии может
присутствовать и в других синтетических конструкторах `dom.rs` — не
проверялось в рамках этой сессии, скоуп ограничен `StorageEvent`.
