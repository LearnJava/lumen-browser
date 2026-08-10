# BUG-748 — `Headers`/`Response` в скоупе service worker — отдельный мини-шим на объекте: нет `append`/`delete`/`forEach`/итерации, дубликаты имён теряются, внутреннее поле `_h` торчит наружу

**Статус:** OPEN
**Компонент:** js (`crates/js/src/sw_worker.rs` — шим глобального скоупа service worker: `Headers` ~строки 72-80, `Response` ~строки 82-104)
**Найден:** P3, при закрытии [BUG-369](BUG-369-FIXED.md), 2026-08-10

## Симптом

`WEB_API_SHIM` (`crates/js/src/dom.rs`) — не единственное место, где объявлен
`Headers`. У глобального скоупа service worker свой, независимый мини-шим в
`sw_worker.rs`, и он остался в том виде, в каком `Headers` был до BUG-369, только
ещё беднее: хранилище — не список пар, а простой объект.

```js
function Headers(init) { this._h = {}; if (init) { for (var k in init) this._h[k.toLowerCase()] = String(init[k]); } }
Headers.prototype.get = function(n) { return this._h[n.toLowerCase()] || null; };
Headers.prototype.set = function(n, v) { this._h[n.toLowerCase()] = String(v); };
Headers.prototype.has = function(n) { return n.toLowerCase() in this._h; };
```

Отсюда сразу несколько расхождений с Fetch §2.2, каждое из которых в скоупе
страницы уже исправлено:

1. **Нет `append`, `delete`, `forEach`, `entries`/`keys`/`values`,
   `getSetCookie`, `Symbol.iterator`.** Любой обход заголовков внутри
   `sw.js` (`for (const [k,v] of headers)`, `headers.forEach(...)`) падает
   `TypeError`, а `headers.append(...)` — `is not a function`.
2. **Хранилище — объект, а не список пар**, поэтому дубликаты имён невозможны в
   принципе: два `Set-Cookie` схлопываются в один, `get()` не склеивает значения
   через `, `.
3. **`get()` возвращает `null` на пустом значении** (`'' || null`), тогда как
   заголовок с пустым значением присутствует и `has()` про него скажет `true` —
   `get`/`has` расходятся между собой.
4. **Нет валидации имени/значения и нет guard-а** (тот же п.4/п.5 BUG-369).
5. **`_h` — перечислимое собственное свойство**: `JSON.stringify(headers)` даёт
   `{"_h":{…}}`, `for..in` перечисляет `_h`. Более того, `Response.prototype.clone`
   (`sw_worker.rs:103`) прямо завязан на это поле — `{ headers: this.headers._h }`, —
   так что починка приватности требует одновременной починки клона.
6. `Response` в этом шиме тоже урезан: `statusText` по умолчанию `'OK'` вместо
   `''`, `arrayBuffer()` всегда отдаёт пустой буфер, `_body` — строка.

## Ожидаемое поведение

Скоуп service worker получает тот же `Headers`, что и страница. Правильный ход —
не чинить мини-шим по второму разу, а вынести реализацию BUG-369 в один общий
исходник и вклеивать его в оба шима: два независимых `Headers` в одном движке
разъезжаются по определению, что этот баг и демонстрирует.

## Как воспроизвести

Зарегистрировать service worker, у которого в `fetch`-обработчике есть
`for (const pair of event.request.headers)` или `headers.append(...)`, —
обработчик падает `TypeError`. Проба на самом шиме:

```js
// в скоупе service worker
var h = new Headers({ 'X-A': '1' });
typeof h[Symbol.iterator]   // undefined
typeof h.append             // undefined
JSON.stringify(h)           // {"_h":{"x-a":"1"}}
```

## Влияние

Ограничено скоупом service worker, но именно там обход и правка заголовков —
основной сценарий (проксирование запроса, дописывание `Authorization`,
разбор `Set-Cookie`). Для WPT: подкатегории `service-workers/`, использующие
`Headers` внутри воркера.

## Связанные

- [BUG-369](BUG-369-FIXED.md) — тот же дефект в скоупе страницы, закрыт 2026-08-10;
  реализацию оттуда и надо переиспользовать.
- [BUG-694](BUG-694-OPEN.md) — тот же класс на `URLSearchParams`.
