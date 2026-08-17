# BUG-370 — `Request` не имеет Body-mixin вовсе (`text`/`json`/`blob`/`arrayBuffer`/`formData`/`bytes` = `undefined`) и не абсолютизирует `url`; `Response.json()` отсутствует, `Response.error()` отдаёт обычный ответ вместо network error, `Response.redirect()` не ставит `Location`; ни один конструктор не валидирует аргументы

**Статус:** FIXED 2026-08-10 (все пункты A/B/C)
**Компонент:** js (`crates/js/src/dom.rs` — блок `Body`/`Response`/`Request` в `WEB_API_SHIM`)
**Найден:** P2, WPT-VENDOR-fetch (2026-07-28), проба `--dump-layout` вне WPT
**Исправлен:** P3 2026-08-10, ветка `p3-bug-370`

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
[BUG-359](BUG-359-FIXED.md) (`window.open`/`location.href=`),
[BUG-362](BUG-362-FIXED.md) (`EventSource`).

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

## C. Общее для обоих (и для `Headers` — см. [BUG-369](BUG-369-FIXED.md))

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
[BUG-367](BUG-367-FIXED.md).

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

## Фикс (P3, 2026-08-10)

Оба класса переписаны одним замыканием в `WEB_API_SHIM` — у них общий Body-mixin
и общая форма приватного состояния (две `WeakMap`, по одной на интерфейс). Точечно
чинить было нечего: пункты C1/C2/C3 — не три дефекта, а один, «объект собран
присваиваниями в `this` вместо интерфейса», и он же порождает половину A и B.

| Пункт | Что сделано |
|---|---|
| A1 | `Request` получил Body-mixin (`body`/`bodyUsed`/`arrayBuffer`/`blob`/`bytes`/`formData`/`json`/`text`) и атрибуты `keepalive`/`destination`. Mixin ставится общей функцией `installBody(proto, stateOf)` на оба прототипа — расхождение между ними теперь структурно невозможно |
| A2 | Уже был закрыт вместе с [BUG-347](BUG-347-FIXED.md) |
| A3 | `new` обязателен, `Request.length === 1`, тело у `GET`/`HEAD` → `TypeError`, метод-не-токен → `TypeError`. Нормализация метода приведена к спеке: апперкейсятся только шесть известных имён, `patch` остаётся строчным (раньше апперкейсилось всё) |
| B1 | Добавлен статический `Response.json(data, init)`; тело передаётся байтами, поэтому `application/json` ставится только когда `init.headers` не задал свой `Content-Type` |
| B2 | `Response.error()` строится напрямую из слотов, минуя конструктор (тот безусловно ставит `type = 'default'`) → `type === 'error'`, `body === null`, заголовки `immutable` |
| B3 | `Response.redirect()` кладёт сериализованный URL в `Location` и бросает `RangeError` вне {301,302,303,307,308}. Список заполняется **до** установки `immutable`-guard-а — иначе собственная запись была бы отвергнута |
| B4 | `RangeError` на статусе вне 200-599, `TypeError` на теле при null-body-статусе (101/103/204/205/304) |
| B5 | `Content-Type` выводится из типа тела (строка, `URLSearchParams`, `Blob`, `FormData`) и только заполняет пробел, не перебивая `init.headers` |
| B6 | `formData()` (разбирает urlencoded и multipart) и `bytes()` дополнили mixin |
| C1 | Все атрибуты — read-only геттеры на прототипе; `Object.getOwnPropertyNames(new Request(...))` теперь пуст, `req.method = 'DELETE'` ничего не меняет |
| C2 | Слоты уехали в `WeakMap`; `JSON.stringify` на обоих даёт `{}` (раньше `Request` вываливал `signal._listeners`, а `Response` **бросал** из-за цикла в незапрятанном `ReadableStream`) |
| C3 | `Symbol.toStringTag` на обоих прототипах |
| C4 | `fetch` на глобале стал `configurable`. Объявление `function fetch()` на верхнем уровне скрипта даёт `configurable: false`, и переопределить такое свойство через `defineProperty` нельзя в принципе — функция объявлена как `_lumen_fetch` и публикуется `defineProperty`-ем; `name`/`length` выставлены вручную, чтобы `fetch.name === 'fetch'` и `fetch.length === 1` остались спековыми |

Побочные изменения, которых потребовала приватность состояния (тот же класс, что в
[BUG-369](BUG-369-FIXED.md) — «спрятал состояние в замыкание, отрезал от него и
легитимных внутренних потребителей»): замыкание выдаёт наружу два внутренних
глобала — `_lumen_response_from_fetch_cache(status, statusText, headers, url)`
(сетевой путь; заменил `Response._fromFetchCache` + внешнее присваивание
`resp.url`, которое теперь молча ничего бы не делало) и `_lumen_body_source(obj)`
(`fetch()` читал `input.body`, а он теперь `ReadableStream`, а не исходная
строка/`FormData`).

Регрессионный тест BUG-703 пришлось переписать: он строил объект через
`Object.create(Response.prototype)` и присваивание приватных полей. Новый вариант
гоняет два `fetch()` с разными телами через мок-провайдер — это сильнее прежнего,
потому что ловит именно перекрёстное чтение общего слота `FetchCache`, а не только
факт чтения очереди.

Изменения формы, не входившие в заявку, но следующие из спеки и потому сделанные
заодно: `new Response()` без тела теперь даёт `body === null` (раньше всегда был
поток над пустым буфером); `Response.redirect(url).url` — `''`, а не переданный URL
(URL уезжает в `Location`); `clone()` на использованном теле бросает `TypeError`.

Тесты: 20 новых в `crates/js/src/dom.rs`, `mod dom::tests::v8_ws_sse` (и один
переписанный в `v8_whatwg_streams`). Прогон — `cargo test -p lumen-js --lib
--features v8-backend`, 2638 зелёных.

**Остаток, найденный по ходу и вынесенный отдельно:** `fetch()` не отправляет
заголовки запроса вообще — ни `init.headers`, ни `Request.headers` никуда не
доезжают, потому что у `JsFetchProvider::fetch_sync(url, method)` попросту нет
параметра заголовков → [BUG-749](BUG-749-FIXED.md).

## Связанные

- [BUG-369](BUG-369-FIXED.md) — та же проба, `Headers`: не итерируем, не копируется.
- [BUG-749](BUG-749-FIXED.md) — найден при этом фиксе: `fetch()` молча теряет
  заголовки запроса (в мосту к Rust нет канала для них).
- [BUG-347](BUG-347-FIXED.md) — `fetch()` не резолвит относительные URL; A2 —
  соседняя строка того же шима, исправлена вместе с ним 2026-08-06.
- [BUG-346](BUG-346-FIXED.md), [BUG-359](BUG-359-FIXED.md), [BUG-362](BUG-362-FIXED.md) —
  остальные сайты семейства «относительный URL не резолвится».
- [BUG-367](BUG-367-FIXED.md) — тот же дефект формы (атрибуты на инстансе, внутренние
  слоты наружу) на `Element`.
