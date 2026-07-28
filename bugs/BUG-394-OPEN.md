# BUG-394 — `Sensor`/подклассы не наследуются от глобального `EventTarget`, `Sensor` конструируется напрямую

**Статус:** OPEN
**Компонент:** js (`crates/js/src/generic_sensor.rs:32-78` — приватный
`_SensorEventTarget` вместо глобального `EventTarget`, `Sensor` без
guard на прямой вызов конструктора)
**Найден:** P2, WPT-VENDOR-generic-sensor (2026-07-28), прямая проба
`--mcp-port`/`eval` (соответствующий тест `idlharness.https.window.js`
недостижим прогоном — HTTPS-порт-гэп)

## Симптом

```js
typeof EventTarget                                        // → "function" — глобальный класс есть
Sensor.prototype instanceof EventTarget                   // → false
(new Accelerometer()) instanceof EventTarget               // → false
(new Accelerometer()).addEventListener
  === EventTarget.prototype.addEventListener               // → false — своя реализация
new Sensor() instanceof Sensor                              // → true, конструктор не бросает
```

По W3C Generic Sensor API §8, `interface Sensor : EventTarget` — все
сенсоры обязаны быть настоящими `EventTarget` (`instanceof EventTarget`
истинно, `dispatchEvent`/`addEventListener` — та же реализация, что и у
остальных объектов платформы, поддержка `capture`/`once`/`passive` и
реальных `Event`-объектов с `bubbles`/`cancelable`/`target` и т.д.).
Кроме того, у `Sensor` в IDL нет операции-конструктора — по WebIDL это
означает, что `new Sensor()` должен бросать `TypeError: Illegal
constructor`; создавать можно только конкретные подклассы
(`Accelerometer`, `Gyroscope`, …), у которых конструктор в спеке есть.

Оба факта проверяет `idlharness.https.window.js` категории (сравнивает
фактическую цепочку прототипов и списки собственных операций каждого
интерфейса с эталонным WebIDL), но этот тест недостижим текущим
прогоном (`.https.` без `testdriver.js` → полный TIMEOUT на
HTTPS-порт-гэпе), поэтому находка видна только прямой пробой.

## Причина

`generic_sensor.rs:32-53` — модуль сам определяет минимальный
`_SensorEventTarget` (свой `_listeners`, свои `addEventListener`/
`removeEventListener`/`dispatchEvent`) вместо того, чтобы наследоваться
от настоящего глобального `EventTarget`, который в шиме уже есть
(`dom.rs:3351`). Комментарий в коде (`generic_sensor.rs:34-35`)
объясняет мотивацию — "избежать зависимости от глобального `EventTarget`,
QuickJS не всегда его предоставляет" — но на дефолтном V8-движке (ADR-018)
`EventTarget` доступен, и код всё равно продолжает использовать
приватный мини-класс.

`Sensor.prototype.constructor = Sensor` (`:78`) не имеет guard'а —
`function Sensor(options) { … }` конструируется как обычная функция,
без проверки "вызван ли напрямую или из подкласса".

## Как чинить

1. Заменить `_SensorEventTarget` на прямое наследование от глобального
   `EventTarget` (`Sensor.prototype = Object.create(EventTarget.prototype)`),
   убрать домашний `_listeners`/`dispatchEvent`/`addEventListener`/
   `removeEventListener` — раз V8 (дефолт) гарантированно предоставляет
   `EventTarget`, мотивация комментария больше не блокирует. Если
   QuickJS-путь (опциональный rollback) не даёт `EventTarget` в момент
   `install_generic_sensor_bindings`, проверить порядок инициализации
   шимов, а не обходить проблему приватным классом — engine-agnostic
   код не должен закладываться на устаревший порядок QuickJS-пути
   (CLAUDE.md: не таргетировать rquickjs новыми фиксами, но и не ломать
   V8-путь ради него).
2. Добавить guard в конструктор `Sensor`: бросать `TypeError` при прямом
   `new Sensor(...)` (проверка `new.target === Sensor`), пропускать
   вызов из подклассов (`new.target !== Sensor`).

Регрессия без WPT: `(new Accelerometer()) instanceof EventTarget === true`,
`new Sensor()` бросает `TypeError`, `new Accelerometer()` — нет.

## Связанные

* [[BUG-393]] — тот же файл, соседняя находка той же пробы:
  `SensorErrorEvent` не валидирует обязательный `error` в init dict.
* Категория `generic-sensor` — вне скоупа (🚫, аппаратный API), но найдена
  как побочный эффект вендоринга по постоянному решению пользователя
  (класс `accelerometer`/`gamepad`/`fledge` — 🚫-scope не освобождает от
  спек-соответствия уже реализованной части API, см. BUG-392).
