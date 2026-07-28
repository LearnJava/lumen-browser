# BUG-369 — `Headers` не итерируем и не копируется из другого `Headers`: `entries()`/`keys()`/`values()` возвращают массивы вместо итераторов, `Symbol.iterator` отсутствует, `new Headers(headers)` молча даёт пустой набор; плюс нет валидации имён, guard-а запрещённых заголовков и `getSetCookie`, а внутренние `_map`/`_key` торчат наружу

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8845-8880` — конструктор `Headers` и весь его прототип: `_key` 8856, `append` 8857, `set` 8861, `get` 8866, `has` 8871, `delete` 8872, `forEach` 8876, `entries` 8879, `keys` 8880, `values` 8881)
**Найден:** P2, WPT-VENDOR-fetch (2026-07-28), проба `--dump-layout` вне WPT

## Симптом

`Headers` в шиме — обычный ES5-конструктор поверх массива пар `this._map`,
а не WebIDL-интерфейс. Из-за этого он расходится с
[Fetch Standard §2.2](https://fetch.spec.whatwg.org/#headers-class) по восьми
независимым пунктам. Все строки ниже — фактический вывод `--dump-layout`-проб
(`.tmp/probe-fetch.html`, `.tmp/probe-fetch2.html`, `.tmp/probe3.html`),
движок — дефолтный V8.

### 1. `Headers` не итерируем — `Symbol.iterator` отсутствует (главный пункт)

```
Headers[Symbol.iterator]      = undefined
Headers iterable (for..of h)  = THROW TypeError: h is not iterable
Array.from(h)                 = []
```

По спеке `Headers` объявлен как `iterable<ByteString, ByteString>`, то есть
`Symbol.iterator` обязан существовать и быть тем же объектом, что `entries`.
`for (const [name, value] of headers)` — самая распространённая форма обхода
заголовков в коде и в тестах; сейчас она бросает `TypeError`.

Хуже, чем просто «бросает»: `Array.from(h)` возвращает **пустой массив**, а не
ошибку — `Array.from` видит объект с числовым `length`? нет, `Headers` его не
имеет, поэтому array-like-ветка даёт `[]`. То есть код, обходящий заголовки
через `Array.from`, молча получает «заголовков нет» вместо падения
(ср. [[feedback_green_test_can_mask_broken_feature]] — тихий неверный ответ
маскируется лучше, чем исключение).

### 2. `entries()`/`keys()`/`values()` возвращают `Array`, а не итератор

```
Array.from(h.entries())  = [["a","1"]]        // работает — но только потому, что это массив
h.entries() shape        = [object Array] next=undefined selfIter=function
h.keys() result          = ["a"]
hdr keys len=1 isArray=true
```

Спека требует объект-итератор (`next()`, `[Symbol.iterator]() === this`,
`[object Headers Iterator]`). Здесь возвращается свежий `Array`: `.next` нет.
Любой код формы `const it = h.entries(); it.next().value` падает.

### 3. `new Headers(anotherHeaders)` молча даёт пустой набор

```
new Headers(headersInstance) → .get("a") = null      // ожидается "1"
```

Корневая причина видна прямо в конструкторе (`dom.rs:8845-8855`): не-массивный
`init` обрабатывается веткой `typeof init === 'object'` через
`Object.keys(init)`. У `Headers`-инстанса единственное собственное свойство —
служебное `_map` (см. п. 7), поэтому копирование сводится к
`append("_map", <массив пар>)`: исходные заголовки теряются, а вместо них
появляется мусорный заголовок с именем `_map`. Спека же требует отдельной ветки
«init является `Headers`» (копировать его список).

Тот же дефект бьёт по `Response.prototype.clone` (`dom.rs:9008`) и
`Request.prototype.clone` (`dom.rs:9040`) — они передают `this.headers.entries()`
(массив пар, п. 2), что случайно работает, но только пока `entries()` остаётся
массивом; починка п. 2 без починки п. 3 сломает оба `clone()`.

### 4. Нет валидации имени/значения

```
Headers invalid name throws = did not throw (spec: TypeError)
```

`new Headers().append("in valid", "x")` обязан бросить `TypeError` (имя не
является валидным HTTP-токеном). Сейчас заголовок с пробелом в имени тихо
кладётся в `_map`.

### 5. Нет понятия guard — запрещённые заголовки проходят

```
Headers forbidden host    = x.example         // ожидается null: forbidden header name
Request bad method CONNECT = CONNECT          // ожидается TypeError: forbidden method
```

`h.set("Host", …)` на request-guard-заголовках должен молча ничего не делать.
Guard (`immutable`/`request`/`request-no-cors`/`response`/`none`) в шиме
отсутствует как концепция, поэтому и `Response`-заголовки мутабельны там, где
спека требует `immutable` (`Response.error()`, `Response.redirect()`).

### 6. `getSetCookie()` отсутствует

```
Headers getSetCookie = undefined
```

Добавлен в спеку в 2023, единственный корректный способ прочитать несколько
`Set-Cookie`; текущий `get("set-cookie")` склеил бы их через `", "`
(`dom.rs:8866-8870`), что для `Set-Cookie` неверно by design.

### 7. Внутренние слоты `_map` и `_key` — web-visible, перечислимые, записываемые

```
desc(new Headers(),_map)     = enum=true writ=true conf=true value=object
desc(Headers.proto,_key)     = enum=true writ=true conf=true value=function
for..in over new Headers()   = _map,_key,append,set,get,has,delete,forEach,entries,keys,values
JSON.stringify(new Headers({a:"1"})) = {"_map":[["a","1"]]}
```

Спека: у `Headers`-инстанса собственных перечислимых свойств нет вовсе, а методы
лежат на прототипе неперечислимыми — то есть `JSON.stringify(h)` обязан дать
`{}`, а `for..in` — ничего. Сейчас наружу торчит и хранилище, и служебный
нормализатор `_key`, причём записываемые: страница может подменить
`Headers.prototype._key` и изменить регистронезависимость всех заголовков
процесса. Тот же класс дефекта, что `__nid__` в [BUG-367](BUG-367-OPEN.md) и
`navigator.credentials._get_original` в [BUG-366](BUG-366-OPEN.md), — уже третий
объект подряд, где приватное состояние не спрятано.

### 8. `Symbol.toStringTag` отсутствует; конструктор вызывается без `new`

```
Object.prototype.toString.call(new Headers()) = [object Object]   // ожидается [object Headers]
Headers toStringTag = undefined
Headers no-new = did not throw (spec: TypeError)
Headers.length = 1                                                 // ожидается 0
```

`Headers()` без `new` обязан бросить `TypeError` (WebIDL); сейчас вызывается как
обычная функция и молча портит глобальный объект (`this._map = []` на
`globalThis` в sloppy mode). `Headers.length` = 1, тогда как единственный
аргумент опциональный → по WebIDL длина 0.

## Ожидаемое поведение

`Headers` реализован как WebIDL-интерфейс: `Symbol.iterator === entries`,
`entries()`/`keys()`/`values()` возвращают итераторы, ветка копирования из
`Headers` в конструкторе, валидация имени/значения, guard, `getSetCookie()`,
приватное состояние вне досягаемости страницы (замыкание или `WeakMap`, либо
неперечислимое `Symbol`-ключевое поле), `Symbol.toStringTag = "Headers"`,
методы неперечислимы, вызов без `new` бросает `TypeError`.

## Как воспроизвести

```
target/dev-release/lumen.exe --dump-layout .tmp/probe-fetch.html
```

Минимальный кейс:

```html
<script>
  var h = new Headers({ 'X-A': '1' });
  document.title = [
    typeof h[Symbol.iterator],            // undefined → ожидается "function"
    new Headers(h).get('X-A'),            // null      → ожидается "1"
    typeof h.entries().next,              // undefined → ожидается "function"
    JSON.stringify(h)                     // {"_map":…} → ожидается {}
  ].join(' | ');
</script>
```

## Влияние

Вне WPT: `Headers` — публичный API, любой скрипт на любой странице, обходящий
заголовки ответа (`for..of response.headers`) или копирующий их
(`new Headers(res.headers)`), получает исключение либо тихо пустой результат.

Внутри WPT: категория `fetch` (⬜ кандидат по скоупу, вендорена
2026-07-28) прошивает `Headers` насквозь — `fetch/api/headers/` это 20+
отдельных тест-файлов, целиком посвящённых пунктам 1-6 (`headers-basic.any.js`,
`headers-combine.any.js`, `headers-casing.any.js`, `headers-errors.any.js`,
`headers-record.any.js`, `headers-normalize.any.js`, `header-setcookie.any.js`,
`headers-no-cors.any.js`), плюс `Headers` используется как вспомогательный
инструмент почти во всех остальных подкатегориях.

## Связанные

- [BUG-370](BUG-370-OPEN.md) — та же проба, `Request`/`Response`: нет Body-mixin
  на `Request`, `Response.json()`, корректного `Response.error()`/`redirect()`.
- [BUG-367](BUG-367-OPEN.md), [BUG-366](BUG-366-OPEN.md) — тот же класс «внутренний
  слот торчит наружу перечислимым и записываемым», на `Element` и `navigator.credentials`.
