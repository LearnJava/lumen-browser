# BUG-370 — `Request` не имеет Body-mixin вовсе (`text`/`json`/`blob`/`arrayBuffer`/`formData`/`bytes` = `undefined`) и не абсолютизирует `url`; `Response.json()` отсутствует, `Response.error()` отдаёт обычный ответ вместо network error, `Response.redirect()` не ставит `Location`; ни один конструктор не валидирует аргументы

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8908-8925` — конструктор `Response`; `dom.rs:8967-9007` — `_consumeBody` и Body-mixin `Response`; `dom.rs:9008-9024` — `clone`/`error`/`redirect`; `dom.rs:9026-9046` — конструктор `Request` и его единственный метод `clone`)
**Найден:** P2, WPT-VENDOR-fetch (2026-07-28), проба `--dump-layout` вне WPT

## Симптом

`Request` и `Response` в шиме — Phase-0-приближения (в комментарии к `Request`
это признано: «minimal Phase 0 impl», `dom.rs:9025`). Расхождения с
[Fetch Standard §2.4-2.5](https://fetch.spec.whatwg.org/#request-class) ниже
сгруппированы по объектам. Все строки — фактический вывод `--dump-layout`-проб
(`.tmp/probe-fetch.html`, `.tmp/probe-fetch2.html`, `.tmp/probe3.html`),
движок — дефолтный V8.

## A. `Request`

### A1. Body-mixin отсутствует полностью

```
Request body mixin = text=undefined json=undefined blob=undefined
                     arrayBuffer=undefined formData=undefined bytes=undefined
Request.proto members = clone,constructor
Request defaults → bodyUsed=undefined  keepalive=undefined  destination=undefined
```

По спеке `Request` включает mixin `Body` — те же семь членов, что у `Response`
(`body`, `bodyUsed`, `arrayBuffer()`, `blob()`, `bytes()`, `formData()`,
`json()`, `text()`). У `Response` mixin реализован (`dom.rs:8986-9007`), у
`Request` — нет ни одного члена: весь прототип это `clone` + `constructor`.
`new Request('/x', {method:'POST', body:'b'}).text()` бросает
`TypeError: r.text is not a function`. Атрибуты `bodyUsed`, `keepalive`,
`destination` тоже не заведены (возвращают `undefined`, а не `false`/`false`/`""`).

### A2. `Request.url` не абсолютизируется — ИСПРАВЛЕНО вместе с BUG-347 2026-08-06

```
Request.url on "x"        = x                          // было; ожидался абсолютный URL
Request.url on "./rel"    = ./rel                       // было; ожидался абсолютный URL
Request.url on absolute   = https://ex.com/a            // единственный работающий случай
new URL("x", location.href) → работает                  // механизм резолва в шиме ЕСТЬ
```

Конструктор клал аргумент дословно:
`this.url = typeof input === 'string' ? input : (input.url || '')`. Спека требует
распарсить его относительно base URL документа и хранить сериализованный
абсолютный URL. Строка `new URL("x", location.href)` в той же пробе отрабатывает,
то есть нужный механизм в шиме доступен был — это был пропуск, а не отсутствие
инструмента.

**Фикс (P3, 2026-08-06):** конструктор `Request` теперь пропускает `url` через
тот же `_url_resolve(url, _lumen_document_base_url())`, что и `fetch()` — см.
[BUG-347](BUG-347-FIXED.md). Остальные пункты этой заявки (A1 Body-mixin, A3
валидация, ниже) не тронуты этим фиксом.

Это было **пятым независимым сайтом того же семейства**, что
[BUG-346](BUG-346-FIXED.md) (`Url::resolve()` не схлопывает `..`),
[BUG-347](BUG-347-FIXED.md) (`fetch()` не резолвит относительные URL),
[BUG-359](BUG-359-OPEN.md) (`window.open`/`location.href=`),
[BUG-362](BUG-362-OPEN.md) (`EventSource`).

### A3. Нет валидации

```
Request no-new              = did not throw   (spec: TypeError)
Request GET+body throws     = did not throw   (spec: TypeError)
Request bad method CONNECT  = CONNECT         (spec: TypeError, forbidden method)
Request.length = 2                            (spec: 1 — второй аргумент опционален)
```

## B. `Response`

### B1. Статический `Response.json()` отсутствует

```
Response statics = error=function redirect=function json=undefined
```

`Response.json(data, init)` в спеке с 2022 года; сейчас есть только
`Response.prototype.json` (метод разбора тела) — разные вещи.

### B2. `Response.error()` не является network error

```
Response.error() shape = status=0 ok=false type=default url=""
```

`status`/`ok` верные, но `type` обязан быть `"error"`. Именно по `type` код
отличает CORS-отказ от нормального ответа, а `response.type === 'error'` —
канонический тест на network error. Причина прямо в `dom.rs:9016-9018`:
`Response.error` делает `new Response(null, {status:0, statusText:''})`, а
конструктор безусловно ставит `this.type = 'default'` (`dom.rs:8914`) — поле
после конструирования не переопределяется. Тот же пропуск делает результат
мутабельным, тогда как спека требует `immutable`-guard на его заголовках.

### B3. `Response.redirect()` не ставит `Location`

```
Response.redirect("https://e.com/", 301) → status=301 location=null type=default
Response.redirect bad code (200)         → did not throw (spec: RangeError)
```

`dom.rs:9020-9024` выставляет только `status` и `url`. Спека требует положить
сериализованный URL в заголовок `Location` (иначе редирект-ответ бессмысленен)
и бросить `RangeError`, если код не входит в {301,302,303,307,308}.

### B4. Нет валидации статуса и тела

```
Response bad status (1000) throws = did not throw   (spec: RangeError)
Response 204 + body throws        = did not throw   (spec: TypeError)
Response ok range 299/300         = true/false                       // корректно
```

### B5. Нет дефолтного `Content-Type` для тела-строки

```
Response default content-type = null       // ожидается "text/plain;charset=UTF-8"
```

Спека выводит `Content-Type` из типа тела (строка → `text/plain;charset=UTF-8`,
`URLSearchParams` → `application/x-www-form-urlencoded;charset=UTF-8`, `Blob` →
его `type`, `FormData` → `multipart/form-data; boundary=…`). Шим не ставит
ничего.

### B6. `formData()` и `bytes()` отсутствуют в mixin

```
Response.proto members = _consumeBody,arrayBuffer,blob,clone,constructor,json,text
```

Нет `formData()` и `bytes()` (последний добавлен в спеку в 2024).

## C. Общее для обоих (и для `Headers` — см. [BUG-369](BUG-369-OPEN.md))

### C1. Все атрибуты — собственные свойства инстанса, а не геттеры прототипа

```
Request  instance own = body,cache,credentials,headers,integrity,method,mode,
                        redirect,referrer,signal,url
Response instance own = _body,body,bodyUsed,headers,ok,redirected,status,
                        statusText,type,url
Request.proto members = clone,constructor
```

По WebIDL это read-only-атрибуты на прототипе. Сейчас они записываемы
(`req.method = 'DELETE'` меняет запрос), не наследуются и дублируются в памяти на
каждом инстансе. Тот же дефект формы, что у `Element` в
[BUG-367](BUG-367-OPEN.md).

### C2. Внутренние слоты web-visible и перечислимые

```
desc(Response.proto,_consumeBody) = enum=true writ=true conf=true value=function
desc(new Response("x"),_body)     = enum=true writ=true conf=true value=object
JSON.stringify(new Request("/x")) = {"url":"/x","method":"GET","headers":{"_map":[]},
  "body":null,"signal":{"aborted":false,"onabort":null,"_listeners":[]},"mode":"cors",
  "credentials":"same-origin","cache":"default","redirect":"follow",
  "referrer":"about:client","integrity":""}
JSON.stringify(new Response("x")) = THROW TypeError: Converting circular structure
  to JSON --> starting at object with constructor 'ReadableStream' | property
  '_rs_ctrl' -> ... '_stream' closes the circle
```

По спеке `JSON.stringify` на обоих обязан дать `{}`. Вместо этого `Request`
вываливает всю внутреннюю структуру (включая `signal._listeners` — реестр
обработчиков), а `Response` **бросает исключение** из-за циклической ссылки в
незапрятанном `ReadableStream`. Практический эффект вне WPT: любой
`JSON.stringify` объекта, куда попал `Response` (лог, отладочный дамп,
`postMessage`-сериализация вручную), падает.

### C3. `Symbol.toStringTag` отсутствует; конструкторы вызываются без `new`

```
Object.prototype.toString.call(new Request("/x")) = [object Object]   // [object Request]
Object.prototype.toString.call(new Response())    = [object Object]   // [object Response]
Request no-new  = did not throw    Response no-new = did not throw    (spec: TypeError)
```

### C4. `fetch` на глобале — `configurable: false`

```
desc(window,fetch) = enumerable=true writable=true configurable=false
```

WebIDL-операции глобала должны быть `writable: true, enumerable: true,
configurable: true`. Сейчас `fetch` невозможно `delete` или переопределить через
`Object.defineProperty` — ломает полифиллы и тест-шимы, которые подменяют
`fetch` для перехвата запросов.

## Что при этом корректно

Чтобы фикс не сломал работающее: `Request` от `Request` копирует `method`
(`POST`), `method` апперкейсится, `Request.signal` — настоящий `AbortSignal`
(`aborted=false`), `AbortController.abort()` работает и даёт `reason` =
`AbortError`, `AbortSignal.abort/timeout/any` все на месте, `Response.clone()`
не помечает тело использованным, повторный `text()` возвращает отклонённый
промис, `ok` корректен на границах 299/300, `statusText` хранится дословно и
без искажений, `Response.headers` из init читается регистронезависимо,
`Response.body` — настоящий `ReadableStream` с ленивой подкачкой.

## Как воспроизвести

```
target/dev-release/lumen.exe --dump-layout .tmp/probe-fetch.html
target/dev-release/lumen.exe --dump-layout .tmp/probe-fetch2.html
```

Минимальный кейс:

```html
<script>
  document.title = [
    typeof new Request('/x').text,               // undefined → "function"
    new Request('rel').url,                      // "rel"      → абсолютный URL
    typeof Response.json,                        // undefined  → "function"
    Response.error().type,                       // "default"  → "error"
    Response.redirect('https://e.com/',301).headers.get('Location')  // null → URL
  ].join(' | ');
</script>
```

## Влияние

Вне WPT: `Request`/`Response` — публичный API. Отсутствие Body-mixin на
`Request` ломает любой Service-Worker-подобный или прокси-код, читающий тело
запроса; неабсолютный `Request.url` ломает сравнение и логирование запросов;
падающий `JSON.stringify(response)` (C2) бьёт по отладке.

Внутри WPT: категория `fetch` (⬜ кандидат по скоупу, вендорена 2026-07-28,
516 id) — `fetch/api/request/` и `fetch/api/response/` это ~90 тест-файлов,
целиком посвящённых пунктам выше (`request-consume.any.js`,
`request-init-002.any.js`, `request-error.any.js`, `response-init-001.any.js`,
`response-static-json.any.js`, `response-static-error.any.js`,
`response-consume.any.js` и т.д.).

## Связанные

- [BUG-369](BUG-369-OPEN.md) — та же проба, `Headers`: не итерируем, не копируется.
- [BUG-347](BUG-347-FIXED.md) — `fetch()` не резолвит относительные URL; A2 —
  соседняя строка того же шима, исправлена вместе с ним 2026-08-06.
- [BUG-346](BUG-346-FIXED.md), [BUG-359](BUG-359-OPEN.md), [BUG-362](BUG-362-OPEN.md) —
  остальные сайты семейства «относительный URL не резолвится».
- [BUG-367](BUG-367-OPEN.md) — тот же дефект формы (атрибуты на инстансе, внутренние
  слоты наружу) на `Element`.
