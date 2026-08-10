# BUG-369 — `Headers` не итерируем и не копируется из другого `Headers`: `entries()`/`keys()`/`values()` возвращают массивы вместо итераторов, `Symbol.iterator` отсутствует, `new Headers(headers)` молча даёт пустой набор; плюс нет валидации имён, guard-а запрещённых заголовков и `getSetCookie`, а внутренние `_map`/`_key` торчат наружу

**Статус:** FIXED 2026-08-10
**Компонент:** js (`crates/js/src/dom.rs` — конструктор `Headers` и весь его прототип)
**Найден:** P2, WPT-VENDOR-fetch (2026-07-28), проба `--dump-layout` вне WPT
**Исправлен:** P3, ветка `p3-bug-369`, 2026-08-10

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
процесса. Тот же класс дефекта, что `__nid__` в [BUG-367](BUG-367-FIXED.md) и
`navigator.credentials._get_original` в [BUG-366](BUG-366-FIXED.md), — уже третий
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

## Исправление (2026-08-10)

`Headers` переписан как WebIDL-интерфейс. Весь конструктор с прототипом переехал
внутрь IIFE в `WEB_API_SHIM`; список заголовков и guard живут в `WeakMap`,
замкнутой в этом IIFE, — до них нет дороги ни чтением, ни записью, поэтому
`_map`/`_key` исчезли из наблюдаемого мира целиком (не «стали неперечислимыми»,
а именно исчезли).

По пунктам заявки:

1. **Итерируемость.** `Headers.prototype[Symbol.iterator]` — тот же самый объект
   функции, что и `entries` (WebIDL `iterable<>`, не копия), поэтому
   `for (const [k, v] of headers)` и `Array.from(h)` работают.
2. **Итераторы.** `entries()`/`keys()`/`values()` возвращают объект с `next()` и
   `[Symbol.iterator]() === this`, с общим прототипом и
   `Symbol.toStringTag = 'Headers Iterator'`.
3. **Копирование.** `fill()` (Fetch §2.2.5) различает три формы `init`: другой
   `Headers` (определяется по членству в приватной `WeakMap` — не по `instanceof`
   и не по утиной типизации), любой iterable пар и запись-объект.
   `new Headers(anotherHeaders)` копирует список дословно, включая дубликаты имён.
4. **Валидация.** Имя проверяется по RFC 7230 tchar, значение нормализуется
   (обрезка HTTP-пробелов) и проверяется на NUL/CR/LF; нарушение — `TypeError`.
5. **Guard.** Реализованы все пять значений. `Request` получает `request`
   (или `request-no-cors` при `mode: 'no-cors'`), `Response` — `response`,
   `Response.error()`/`Response.redirect()` — `immutable`. Запрещённые
   request-заголовки (список + префиксы `proxy-`/`sec-`) молча игнорируются,
   `set-cookie` не проходит через response-guard, мутация immutable бросает
   `TypeError`. `new Request(url, {method: 'CONNECT'})` бросает `TypeError`.
   Сетевой путь (`Response._fromFetchCache`) наполняет список **до** установки
   guard-а, иначе движок сам бы выбросил `Set-Cookie` из каждого ответа.
6. **`getSetCookie()`** возвращает отдельные значения, а не склейку через `, `.
7. **Приватность.** `JSON.stringify(new Headers({a:'1'}))` → `{}`, `for..in` — пусто,
   методы прототипа неперечислимы.
8. **Брендинг.** `Symbol.toStringTag = 'Headers'`, вызов без `new` бросает
   `TypeError` (`new.target`), `Headers.length === 0` (аргумент читается из
   `arguments`, а не объявляется).

Порядок обхода — спековый «sort and combine» (§2.2.3): имена отсортированы,
одноимённые значения склеены через `, `, кроме `set-cookie`, который даёт по
записи на значение. Этот же порядок использует `forEach`.

Guard не выводится в публичный API, поэтому `Response`/`Request` дотягиваются до
него через два внутренних глобала, назначаемых изнутри замыкания:
`_lumen_headers_new(init, guard)` (guard до наполнения) и
`_lumen_headers_set_guard(h, guard)` (guard после наполнения — сетевой путь и
`clone()`).

Побочно починены два места, которые держались на том, что `entries()` отдавал
массив: `Response.prototype.clone` и `Request.prototype.clone` теперь копируют
список дословно (`new Headers(this.headers)`) и заново вешают guard — через
конструктор `Response` клон терял бы `Set-Cookie` на каждом вызове. И ветка
`fetch()`, определяющая `Content-Type` из `init.headers`: она обходила инициализатор
через `for..in`, а у нового `Headers` собственных перечислимых свойств нет — добавлена
явная ветка `initHeaders instanceof Headers`.

Тесты: 16 новых в `dom.rs` (`mod v8_ws_sse`), по одному-двум на каждый пункт.
Проверено и на живой странице (`--dump-display-list`, дефолтная V8-сборка):
все 16 проб заявки дают спековый результат.

## Не входит в этот фикс

- Фильтрация response-заголовков по CORS (guard `response` здесь блокирует только
  мутацию, а не чтение) — вне движка, у которого нет CORS-слоя.
- `Response.redirect()` по-прежнему пишет `r.url` вместо заголовка `Location` и не
  бросает `RangeError` на не-редиректном статусе; `Response.error()` не выставляет
  `type = 'error'` — это [BUG-370](BUG-370-FIXED.md), который прямо владеет
  «корректным `Response.error()`/`redirect()`».
- Мини-шим `Headers`/`Response` в скоупе service worker
  (`crates/js/src/sw_worker.rs`) — отдельный объект того же класса дефекта,
  выделен в [BUG-748](BUG-748-OPEN.md).

## Связанные

- [BUG-370](BUG-370-FIXED.md) — та же проба, `Request`/`Response`: нет Body-mixin
  на `Request`, `Response.json()`, корректного `Response.error()`/`redirect()`.
- [BUG-694](BUG-694-OPEN.md) — ровно тот же класс на `URLSearchParams`: нет
  `Symbol.iterator`, `entries()` отдаёт массив, копирующий конструктор кладёт
  внутреннее поле `_p` отдельным параметром.
- [BUG-748](BUG-748-OPEN.md) — тот же класс в шиме service worker.
- [BUG-367](BUG-367-FIXED.md), [BUG-366](BUG-366-FIXED.md) — тот же класс «внутренний
  слот торчит наружу перечислимым и записываемым», на `Element` и `navigator.credentials`.
