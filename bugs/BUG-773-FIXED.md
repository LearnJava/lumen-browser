# BUG-773 — `localStorage`/`sessionStorage` не реализуют «legacy platform object»: property-style доступ, `for-in`/`Object.keys`, `in`/`delete` не проходят через нативный бэкенд

**Статус:** FIXED 2026-08-24
**Компонент:** js (`crates/js/src/dom.rs::_lumen_make_storage`, ~строка 9055; нативные биндинги `crates/js/src/v8_runtime.rs`, ~строки 3519-3569)
**Найден:** P2, WPT-VENDOR-webstorage, 2026-08-18 — `run_report.py --all --root webstorage --recursive`
**Исправлен:** P1, 2026-08-24

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

## Как исправлено (P1, 2026-08-24)

`_lumen_make_storage` больше не строит плоский объект. Появился настоящий
интерфейс `Storage` (глобальный конструктор, бросающий `TypeError: Illegal
constructor`) с общим прототипом, а сам объект хранилища — `Proxy` над пустым
объектом, чей `[[Prototype]]` — этот прототип.

**Что где живёт**

- `Storage.prototype` — `key`/`getItem`/`setItem`/`removeItem`/`clear` и
  аксессор `length`. По WebIDL члены прототипа интерфейса —
  writable + enumerable + configurable, поэтому обычное присваивание даёт ровно
  нужную форму, а `Object.keys(storage)` их не видит: они не собственные.
  Добавлен `Symbol.toStringTag` = `'Storage'`.
- Привязка «объект → нативные функции бэкенда» — **`WeakMap`**
  (`_lumen_storage_impl`), а не поле на объекте. Любое собственное свойство было
  бы, во-первых, видно странице, во-вторых — **затеняло бы ключ хранилища с тем
  же именем** (см. правило видимости ниже). Методы достают набор через
  `_lumen_storage_of(this)`, вызов на чужом получателе даёт
  `TypeError: Illegal invocation`.
- Ловушки `Proxy`: `get`/`set`/`has`/`deleteProperty`/`getOwnPropertyDescriptor`
  /`defineProperty`/`ownKeys` плюс `preventExtensions`→`false` и
  `setPrototypeOf` в семантике SetImmutablePrototype.

**Три правила спеки, которые легко перепутать местами**

1. **Видимость именованного свойства.** У `Storage` нет
   `[LegacyOverrideBuiltIns]`, поэтому имя, на которое уже отвечает сам объект
   или что-либо в цепочке прототипов, **скрывает** одноимённый ключ хранилища
   при *чтении*. Это то, что оставляет `storage.length` и `storage.clear`
   членами интерфейса после `setItem('length', …)`
   (`storage_functions_not_overwritten.window.js`). В коде — предикат
   `visible(prop)`: `typeof prop === 'string' && !Reflect.has(target, prop) &&
   getItem(prop) !== undefined`.
2. **Именованный сеттер затенению не подчиняется.** `storage[k] = v` уходит в
   `setItem` для любого строкового имени, даже если прототип отвечает на `k`.
   `set.window.js` прямо проверяет, что одноимённый сеттер на прототипе
   **не вызывается** (`unreached_func`), а `getItem(k)` после присваивания
   возвращает новое значение, тогда как `storage[k]` по-прежнему читает
   прототип. Симметрии между `get` и `set` здесь нет.
3. **`Object.defineProperty` — третье написание того же сеттера.** Для строкового
   имени принимается только data-дескриптор (иначе `false` → `TypeError`), и он
   уходит в `setItem(prop, String(desc.value))` (`defineProperty.window.js`).

**Границы, о которых стоит знать**

- Символьные ключи через хуки именованных свойств **не** проходят — WebIDL
  маршрутизирует только строки, поэтому символ остаётся обычным собственным
  свойством цели (`symbol-props.window.js`).
- `ownKeys` возвращает ключи хранилища **плюс** `Reflect.ownKeys(target)`:
  инвариант `Proxy` требует, чтобы в списке были все неконфигурируемые
  собственные свойства цели, а страница может создать такое символьное свойство
  через `Object.defineProperty`.
- `Object.defineProperty(storage, 'k', { value: 'v', configurable: false })`
  через `Proxy` невыразимо: проверка инварианта отвергает неконфигурируемый
  дескриптор для имени, не являющегося настоящим свойством цели. Спека и WPT
  такой комбинации не требуют.
- Арность проверяется явно (`_lumen_storage_arity`) — `getItem()` без аргументов
  обязан бросить `TypeError`, а не прочитать ключ, записанный как `undefined`
  (`missing_arguments.window.js`).

**Гейт**

14 новых юнит-тестов в `crates/js/src/dom.rs` (модуль `v8_nav_url_storage`),
по одному на каждый WPT-файл категории; `cargo test -p lumen-js --features
v8-backend` — 3013 + 74 зелёных, `cargo clippy -p lumen-js --features
v8-backend --all-targets -- -D warnings` чисто.

Живой прогон категории (`run_report.py --all --root webstorage --recursive`,
`dev-release`):

| | до | после |
|---|---|---|
| harness OK | 24/54 | 25/54 |
| сабтесты | 63/1270 | **1229/1277** |

Знаменатель сабтестов вырос (1270 → 1277), потому что файлы, которые раньше
падали на первом же утверждении, теперь досчитывают свои наборы до конца.

**Остаток категории — чужие баги, не этот.** 36 неожиданных результатов
раскладываются на четыре кучи: события `storage` между документами и
`iframe.contentWindow.postMessage`/`watchedNode.addEventListener` —
[BUG-480](BUG-480-OPEN.md) (у `<iframe>` нет отдельного browsing context);
`StorageEvent`-конструктор и `initStorageEvent` — [BUG-774](BUG-774-OPEN.md);
и одна новая находка — [BUG-901](BUG-901-OPEN.md): одиночный суррогат в ключе
или значении превращается в `U+FFFD` на границе с нативным кодом
(`storage_setitem.window.js`, 12 сабтестов), потому что Rust `String` — UTF-8 и
такую строку представить не может. Дефект границы биндингов, а не хранилища.

## Не расследовано в этой сессии

Пересечение с [BUG-480](../bugs/BUG-480-OPEN.md) (`<iframe>` без отдельного
browsing context) — весь кластер `event_*.html`/`document-domain.html`
(межконтекстные `storage`-события через `<iframe>`) TIMEOUT/FAIL по этой
уже заведённой причине, не новая находка. Отдельно заведён
[BUG-774](../bugs/BUG-774-OPEN.md) — `StorageEvent.prototype.initStorageEvent`
не делает WebIDL-коэрсию аргументов.
