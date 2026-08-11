# BUG-393 — `SensorErrorEvent` конструктор не проверяет обязательный `error` в init dict, не бросает `TypeError`

**Статус:** FIXED 2026-08-11 (P3)
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

## Исправление (P3, 2026-08-11)

`crates/js/src/generic_sensor.rs`, конструктор `SensorErrorEvent` в
`GENERIC_SENSOR_SHIM` — четыре WebIDL-проверки вместо молчаливого `null`:

1. `arguments.length < 2` → `TypeError` (у словаря есть обязательный член,
   значит и сам аргумент обязателен);
2. `init` — `null`/`undefined`/не объект → `TypeError`;
3. `init.error === undefined` (в т.ч. ключа нет вовсе) → `TypeError`
   (обязательный член словаря);
4. `error` не `instanceof DOMException` → `TypeError` (конверсия
   interface-типа).

Проверка (4) обёрнута в `typeof globalThis.DOMException === 'function'`:
шим ставится и standalone — в юнит-тестах без `DOM_EXCEPTION_POLYFILL`
глобального `DOMException` нет, и безусловная проверка отвергала бы вообще
любой `error`. `this.type` теперь приводится к строке (WebIDL `DOMString`),
`this.error` хранит переданный объект как есть (`[SameObject]`).

Никакой другой код `SensorErrorEvent` не конструирует (grep по `crates/`,
`shell/`) — событие только экспортируется в глобал, движок его не
диспатчит, так что ужесточение конструктора ничего внутри не ломает.

### Проверка

* 7 новых юнит-тестов в `generic_sensor.rs` (арность `2`; бросок на
  отсутствующем init, на `42`/`null`/`undefined`, на `{}`/`{error: undefined}`,
  на `{error: {}}`/`{error: 'boom'}`; успешная конструкция с настоящим
  `DOMException` — через crate-visible `DOM_EXCEPTION_POLYFILL`, а не
  самописный двойник). `cargo test -p lumen-js --features v8-backend
  generic_sensor` — 22/22.
* Живая проба `--mcp-port`/`eval` (`.tmp/probe393.py`) на реальной странице:
  `SensorErrorEvent.length === 2`; `new SensorErrorEvent('error')`,
  `…('error', {})`, `…('error', {error: {}})` — все три бросают `TypeError`;
  `new SensorErrorEvent('error', {error: new DOMException('boom',
  'NotReadableError')})` даёт `type === 'error'`, `evt.error === err`,
  `evt.error.name === 'NotReadableError'`.

Сам WPT-тест `SensorErrorEvent-constructor.https.html` по-прежнему
недостижим прогоном (HTTPS-порт-гэп) — обе его проверки воспроизведены
пробой и юнит-тестами вручную.

## Связанные

* [[BUG-394]] — тот же файл, соседняя находка той же пробы: `Sensor` и
  его подклассы не наследуются от глобального `EventTarget`.
* Категория `generic-sensor` — вне скоупа (🚫, аппаратный API), но найдена
  как побочный эффект вендоринга по постоянному решению пользователя
  (класс `accelerometer`/`gamepad`/`fledge` — 🚫-scope не освобождает от
  спек-соответствия уже реализованной части API, см. BUG-392).
