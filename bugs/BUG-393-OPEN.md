# BUG-393 — `SensorErrorEvent` конструктор не проверяет обязательный `error` в init dict, не бросает `TypeError`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/generic_sensor.rs:56-59` — конструктор `SensorErrorEvent`)
**Найден:** P2, WPT-VENDOR-generic-sensor (2026-07-28), тест
`SensorErrorEvent-constructor.https.html` (недостижим прогоном из-за
HTTPS-порт-гэпа, подтверждён прямой пробой `--mcp-port`/`eval`)

## Симптом

Прямая проба на пустой странице:

```js
new SensorErrorEvent('error')
// → SensorErrorEvent { type: 'error', error: null }   (не бросает)
```

Тест категории делает ровно это и ожидает исключение:

```js
test(() => {
  assert_equals(SensorErrorEvent.length, 2);
  assert_throws_js(TypeError, () => new SensorErrorEvent('error'));
}, 'SensorErrorEvent constructor without init dict');
```

По W3C Generic Sensor API §11, `SensorErrorEventInit` объявляет `error`
как **обязательный** член словаря (`required DOMException error`).
WebIDL требует, чтобы конструктор бросал `TypeError`, если обязательный
член словаря отсутствует. Lumen подставляет `null` вместо ошибки и
успешно создаёт событие.

`.length` (второй assert теста) уже совпадает со спекой (`2`), так что
находка узкая — именно отсутствие валидации обязательного члена.

## Причина

`generic_sensor.rs:56-59`:

```js
function SensorErrorEvent(type, init) {
  this.type = type;
  this.error = (init && init.error) ? init.error : null;
}
```

Никакой проверки, что `init` передан и что `init.error` присутствует —
конструктор тихо подставляет `null`.

## Как чинить

Бросать `TypeError`, если `init` отсутствует, `init.error` не передан
(`undefined`), или не является `DOMException`-подобным объектом —
аналогично тому, как WebIDL-биндинги для обязательных членов словаря
обрабатываются в остальном шиме (искать паттерн проверки обязательных
полей init-словаря у других Event-подклассов в `dom.rs`, если он есть,
и переиспользовать).

Регрессия без WPT: `new SensorErrorEvent('error')` должен бросать
`TypeError`; `new SensorErrorEvent('error', {error: new DOMException()})`
должен успешно создавать событие с этим `error`.

## Связанные

* [[BUG-394]] — тот же файл, соседняя находка той же пробы: `Sensor` и
  его подклассы не наследуются от глобального `EventTarget`.
* Категория `generic-sensor` — вне скоупа (🚫, аппаратный API), но найдена
  как побочный эффект вендоринга по постоянному решению пользователя
  (класс `accelerometer`/`gamepad`/`fledge` — 🚫-scope не освобождает от
  спек-соответствия уже реализованной части API, см. BUG-392).
