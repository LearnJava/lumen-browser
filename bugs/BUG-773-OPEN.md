# BUG-773 — `localStorage`/`sessionStorage` не реализуют «legacy platform object»: property-style доступ, `for-in`/`Object.keys`, `in`/`delete` не проходят через нативный бэкенд

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::_lumen_make_storage`, ~строка 9055; нативные биндинги `crates/js/src/v8_runtime.rs`, ~строки 3519-3569)
**Найден:** P2, WPT-VENDOR-webstorage, 2026-08-18 — `run_report.py --all --root webstorage --recursive`

## Симптом

Спека (`https://html.spec.whatwg.org/multipage/webstorage.html` §8) описывает
`Storage` как «legacy platform object» с именованными свойствами: любое
`storage.foo = "bar"` / `storage["foo"]` / `delete storage.foo` /
`for (k in storage)` / `Object.keys(storage)` должно быть эквивалентно
`setItem`/`getItem`/`removeItem`/перечислению фактических ключей хранилища —
одна и та же операция двумя разными синтаксисами.

В Lumen `_lumen_make_storage` (`dom.rs:9055`) строит `localStorage`/
`sessionStorage` как обычный JS-объект:

```js
function _lumen_make_storage(getLen, getKey, getItem, setItem, removeItem, clear) {
    var obj = {
        key:        function(n) { ... },
        getItem:    function(k) { ... },
        setItem:    function(k, v) { setItem(String(k), String(v)); },
        removeItem: function(k) { removeItem(String(k)); },
        clear:      function() { clear(); }
    };
    Object.defineProperty(obj, 'length', { get: function() { return getLen(); }, ... });
    return obj;
}
```

Нет `Proxy`, нет `get`/`set`/`deleteProperty`/`ownKeys`/`has` ловушек — только
явный `getItem`/`setItem`/`removeItem`/`key`/`clear`/`length` API. Нативные
биндинги (`v8_runtime.rs:3519-3569`, `_lumen_ls_get`/`_lumen_ls_set`/…) тоже не
предоставляют такого механизма — уровня, который мог бы перехватить
произвольный доступ по свойству, не существует вовсе.

Следствие — **две несвязанные плоскости данных на одном объекте**:

- `storage.setItem('foo', 'x')` / `storage.getItem('foo')` / `storage.length`
  идут через нативный Rust-бэкенд (`ls_store`/`ss_store`) — работают верно.
- `storage.foo = 'x'` / `storage['foo']` / `delete storage.foo` /
  `for (k in storage)` / `Object.keys(storage)` — обычные JS-свойства
  объекта-обёртки. Не видны `getItem`/`length`/`key()` и не входят в
  перечисление вместе с «настоящими» ключами.
- Методы (`key`, `getItem`, `setItem`, `removeItem`, `clear`) — собственные
  **перечислимые** свойства каждого инстанса (`obj.key = function...`),
  вместо неперечислимых членов общего `Storage.prototype` — поэтому
  `Object.keys(storage)` подмешивает имена методов к «настоящим» ключам,
  а `window.Storage` (интерфейсный конструктор/прототип) вообще не
  существует.

## Подтверждённые провалы (`tests/wpt/webstorage`, `run_report.py`)

- `storage_enumerate.window.js`: `Object.keys(storage)` после
  `setItem("foo","bar"); storage.baz="quux"; setItem(0,"alpha"); storage[42]="beta"`
  ожидает `["0","42","baz","foo"]`, реально даёт
  `["42","baz","clear","getItem","key","removeItem","setItem"]` — значения из
  `setItem` не видны вовсе, зато протекли имена методов.
- `storage_length.window.js` (`.length (method access)`): `storage["name"]="user1"`
  не меняет `storage.length` (остаётся 0) — bracket-присваивание не долетает
  до `_lumen_ls_set`.
- `storage_string_conversion.window.js` («only stores strings»):
  `storage.a = null; storage.a` возвращает `null` (объект), а не
  WebIDL-коэрсию `"null"` — потому что запись/чтение вообще не проходит через
  `setItem`/`getItem`.
- `storage_removeitem.window.js` (`removeItem(null)`/`removeItem(undefined)`):
  использует `"null" in storage` / `"undefined" in storage` — оператор `in`
  не видит именованных «ключей» вовсе, т.к. это не собственные свойства.
- `symbol-props.window.js` («get with symbol on prototype»): ссылается на
  `Storage.prototype` напрямую — `ReferenceError: Storage is not defined`.

Итого по категории: **24/54 harness OK, 63/1270 subtests** — большая часть
провалов сводится к этому одному корню (остальное — уже задокументированный
[BUG-480](../bugs/BUG-480-OPEN.md), нет отдельного browsing context у
`<iframe>`, см. `docs/wpt-vendor-notes/webstorage.md`).

## Почему это не только тестовый шум

Реальные страницы сплошь и рядом пишут в Storage через bracket/dot-нотацию
(`localStorage.token = x`, устаревшие библиотеки, некоторые полифиллы) —
на Lumen такая запись молча создаёт JS-свойство и никогда не попадает в
персистентный бэкенд, т.е. **данные теряются при следующей загрузке
страницы** без единой ошибки в консоли. Это функциональный дефект, не
только несоответствие спеке.

## Предлагаемый фикс

Заменить `_lumen_make_storage` на класс, обёрнутый в `Proxy` (или, если
устройство рантайма это лучше поддерживает, на нативный V8 named-property
interceptor — см. как уже сделано для `Element`/`Node` в других частях
`v8_runtime.rs`), с ловушками `get`/`set`/`has`/`deleteProperty`/`ownKeys`,
маршрутизирующими произвольные строковые ключи в `getItem`/`setItem`/
`removeItem`/`key(n)`; методы `key`/`getItem`/`setItem`/`removeItem`/`clear`
перенести на общий, неперечислимый `Storage.prototype`; добавить
глобальный конструктор/интерфейсный объект `window.Storage`. Тот же паттерн
уже применялся при переносе других WebIDL-шимов с «плоского объекта» на
настоящий интерфейс — см. [BUG-367](../bugs/BUG-367-FIXED.md).

## Не расследовано в этой сессии

Пересечение с [BUG-480](../bugs/BUG-480-OPEN.md) (`<iframe>` без отдельного
browsing context) — весь кластер `event_*.html`/`document-domain.html`
(межконтекстные `storage`-события через `<iframe>`) TIMEOUT/FAIL по этой
уже заведённой причине, не новая находка. Отдельно заведён
[BUG-774](../bugs/BUG-774-OPEN.md) — `StorageEvent.prototype.initStorageEvent`
не делает WebIDL-коэрсию аргументов.
