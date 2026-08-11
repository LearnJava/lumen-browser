# BUG-761 — `SensorErrorEvent` не наследуется от `Event`, и `error`-событие ниоткуда не диспатчится

**Статус:** OPEN
**Компонент:** js (`crates/js/src/generic_sensor.rs` — `GENERIC_SENSOR_SHIM`,
конструктор `SensorErrorEvent`: два присваивания `this.type`/`this.error`
без прототипа `Event`; потребитель отсутствует — `dispatchEvent` с
`'error'` в шиме не встречается)
**Найден:** P3, 2026-08-11, живой пробой `--mcp-port`/`eval` при закрытии
[BUG-394](BUG-394-FIXED.md)

## Симптом

```js
var e = new SensorErrorEvent('error', {error: new DOMException('x', 'NotAllowedError')});
SensorErrorEvent.prototype instanceof Event   // → false
e instanceof Event                            // → false
e.bubbles                                     // → undefined
e.target                                      // → undefined
typeof e.stopPropagation                      // → "undefined"
```

W3C Generic Sensor API §11 — `interface SensorErrorEvent : Event`.
Объект, который отдаёт конструктор, не Event ни по цепочке прототипов, ни
по составу: нет ни одного члена интерфейса `Event`
(`bubbles`/`cancelable`/`target`/`currentTarget`/`eventPhase`/
`stopPropagation()`/`preventDefault()`), только два собственных поля.
Тот же класс дефекта, что [BUG-394](BUG-394-FIXED.md) — там базой был не
`EventTarget`, здесь не `Event`.

Вторая половина: событие некому доставить. `error` не диспатчится нигде —
`Sensor.start()` шлёт только `activate`, `_lumen_sensor_deliver_reading`
(заготовка Phase 1) не шлёт ничего. То есть `SensorErrorEvent` сегодня
конструируется исключительно тестом или страницей, а не движком.

## Причина

`SensorErrorEvent` собран в шиме как обычная функция с двумя
присваиваниями — прототип не связан с `Event`, `Event.call(this, type)`
не вызывается. Валидация init-словаря (BUG-393) добавлена в тот же
конструктор, но форму объекта не трогала.

Ограничение, которое надо учесть при починке (см. BUG-393): шим ставится
и standalone (юнит-тесты на голом V8), где глобального `Event` может не
быть — после BUG-394 модуль уже требует `Event`/`EventTarget` на глобале
и без них не ставится вовсе, так что новая проверка не нужна, достаточно
опереться на ту же переменную `EventBase`.

## Как чинить

1. `SensorErrorEvent.prototype = Object.create(EventBase.prototype)`,
   `constructor` обратно на себя, в конструкторе `EventBase.call(this, type, init)`
   вместо `this.type = String(type)` — так `bubbles`/`cancelable` придут
   из init-словаря по спеке, а не пропадут.
2. Порядок шагов WebIDL сохранить: валидация обязательного члена `error`
   (BUG-393) — до вызова базового конструктора или после, но без потери
   `TypeError` ни в одном из пяти уже покрытых случаев (тесты
   `sensor_error_event_*` в `generic_sensor.rs`).
3. Отдельным вопросом — кто шлёт `error`. Пока аппаратного пути нет
   (Phase 0), диспатч появится вместе с `_lumen_sensor_deliver_reading`;
   при этом путь должен идти через тот же `dispatchEvent`, что и
   `activate`, чтобы `onerror` вызывался шагом `on<type>`, а не руками
   (ловушка, снятая в BUG-394).

Регрессия: `new SensorErrorEvent('error', {error: err}) instanceof Event`,
`.bubbles === false`, `.type === 'error'`, `.error === err`, плюс все
существующие `TypeError`-проверки BUG-393 остаются зелёными.

## Связанные

* [[BUG-393]] — валидация init-словаря того же конструктора (FIXED).
* [[BUG-394]] — тот же класс в том же файле: `Sensor` не был `EventTarget`
  (FIXED); ограничение «шим требует глобальные `Event`/`EventTarget`»
  введено там же.
* [[BUG-400]] — `performance` не `EventTarget`; [[BUG-664]] —
  `navigator.connection` не `EventTarget`: то же семейство «объект спеки
  собран литералом вместо интерфейса».
