# BUG-394 — `Sensor`/подклассы не наследуются от глобального `EventTarget`, `Sensor` конструируется напрямую

**Статус:** FIXED 2026-08-11
**Компонент:** js (`crates/js/src/generic_sensor.rs:32-78` — приватный
`_SensorEventTarget` вместо глобального `EventTarget`, `Sensor` без
guard на прямой вызов конструктора)
**Найден:** P2, WPT-VENDOR-generic-sensor (2026-07-28), прямая проба
`--mcp-port`/`eval` (соответствующий тест `idlharness.https.window.js`
недостижим прогоном — HTTPS-порт-гэп)
**Исправлен:** P3, 2026-08-11

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

## Что сделано (P3, 2026-08-11)

1. **`_SensorEventTarget` удалён целиком.** База — `globalThis.EventTarget`
   (`Sensor.prototype = Object.create(EventTargetBase.prototype)`,
   `EventTargetBase.call(this)` в конструкторе). Мотивация исходного
   комментария снята вместе с самим rquickjs-путём (CLAUDE.md: движок
   один, V8), да и порядок установки её не подтверждал: `install_dom`
   вычисляет `WEB_API_SHIM` до всех `install_v8!`-модулей, так что
   `EventTarget`/`Event` на момент установки этого шима уже на глобале.
2. **Шим отказывается ставиться без `EventTarget`/`Event`** (ранний
   `return`, прецедент `permissions.rs`): половинчатый `Sensor` без базы
   — не то, что стоит подставлять вместо дефекта. Отсутствующий
   `Accelerometer` заваливает feature-detection закрыто, расходящийся с
   платформой — нет. На странице ветка недостижима; она бьёт только
   standalone-установку (юнит-тест на голом V8).
3. **Guard в конструкторе `Sensor`:** `new.target === Sensor` →
   `TypeError('Illegal constructor')`. Подклассы приходят в то же тело
   через `Sensor.call(this, options)`, где `new.target === undefined`, и
   не задеваются.
4. **Тот же guard в `OrientationSensor`** — в заявке не назван, но это
   ровно то же правило WebIDL: у абстрактной базы (W3C Orientation
   Sensor §6) операции-конструктора нет, она есть только у
   `AbsoluteOrientationSensor`/`RelativeOrientationSensor`.
5. **Побочный дефект, который создала бы правка «в лоб»:** `start()`
   вызывал `onactivate` руками *и* потом `dispatchEvent`. У настоящего
   `EventTarget` шаг `on<type>` уже внутри `dispatchEvent`, так что
   обработчик получал бы событие дважды; явный вызов убран. Заодно
   `activate` теперь настоящий `new Event('activate')`, а не литерал
   `{type: 'activate'}`.

### Проверка

8 новых тестов (`cargo test -p lumen-js --features v8-backend generic_sensor`
— 30/30 зелёных): 4 в `generic_sensor.rs` (наследование, запрет прямой
конструкции `Sensor`/`OrientationSensor`, живость подклассов, одно
`activate` на канал) и 4 интеграционных (`dom::tests::v8_generic_sensor`)
— последние идут против **реального** `install_dom`, потому что
собственные тесты модуля заглушают `Event`/`EventTarget` (на голом V8 их
нет) и по построению не могут показать главное: что база — именно
шимовый `EventTarget`, с его реестром слушателей, опцией `once` и шагом
`on<type>`.

Живая проба (`--mcp-port`, headless, `.tmp/bug394_probe.py`) на
`dev-release`-бинарнике:

```
Sensor.prototype instanceof EventTarget       -> true
new Accelerometer() instanceof EventTarget    -> true
addEventListener is EventTarget's             -> true
new Sensor()                                  -> TypeError: Illegal constructor
new OrientationSensor()                       -> TypeError: Illegal constructor
new Accelerometer()                           -> ok (instanceof Sensor)
once option honoured                          -> 1 (у приватного мини-класса было бы 2)
activate                                      -> handler=1 listener=1 isEvent=true target==true
all 8 sensors still constructible             -> all ok
```

### Чего фикс НЕ закрывает

* `Magnetometer_insecure_context.html` (и родня): сенсоры по-прежнему
  доступны в небезопасном контексте — это [BUG-399](BUG-399-OPEN.md)
  (`window.isSecureContext` захардкожен `true`), а не эта заявка;
  фраза в `docs/wpt-vendor-notes/magnetometer.md` («BUG-394 —
  конструкторы не гейтятся по контексту») к предмету BUG-394 отношения
  не имеет.
* Сам `idlharness.https.window.js` так и остаётся недостижимым прогоном
  (HTTPS-порт-гэп) — гейт здесь юнит-тесты и живая проба, а не WPT.
* `SensorErrorEvent` по-прежнему не наследуется от `Event`
  (`instanceof Event` → `false`, нет `bubbles`/`target`/`stopPropagation`),
  и `error`-событие ниоткуда не диспатчится — заведено отдельно как
  [BUG-761](BUG-761-OPEN.md).

## Связанные

* [[BUG-393]] — тот же файл, соседняя находка той же пробы:
  `SensorErrorEvent` не валидирует обязательный `error` в init dict.
* [[BUG-761]] — остаток того же класса в том же файле: `SensorErrorEvent`
  не `Event`.
* [[BUG-386]] / [[BUG-664]] / [[BUG-400]] — тот же класс дефекта в других
  шимах: объект спеки объявлен наследником `EventTarget`, а собран
  литералом или приватным мини-классом.
* Категория `generic-sensor` — вне скоупа (🚫, аппаратный API), но найдена
  как побочный эффект вендоринга по постоянному решению пользователя
  (класс `accelerometer`/`gamepad`/`fledge` — 🚫-scope не освобождает от
  спек-соответствия уже реализованной части API, см. BUG-392).
