# BUG-749 — `fetch()` не отправляет заголовки запроса вообще: ни `init.headers`, ни `Request.headers` не доезжают до сети

**Статус:** FIXED 2026-08-17
**Компонент:** js (`crates/js/src/dom.rs` — `_lumen_fetch`), core (`crates/core/src/ext.rs::JsFetchProvider` — сигнатуры четырёх методов), network (`crates/network/src/lib.rs::HttpClient::fetch_sync` и соседи)
**Найден:** P3, при закрытии [BUG-370](BUG-370-FIXED.md) 2026-08-10

## Симптом

Страница задаёт заголовки любым из трёх спековых способов — и ни один не
доезжает до сокета:

```js
fetch('/api', { headers: { 'Authorization': 'Bearer t' } });          // заголовка нет на проводе
fetch('/api', { headers: new Headers([['X-CSRF', 'v']]) });           // заголовка нет
fetch(new Request('/api', { headers: { 'Accept': 'application/json' } })); // заголовка нет
```

Это не потеря в шиме и не guard `Headers`: `q.headers.get('x-csrf')`
возвращает значение, объект `Headers` заполнен правильно. Заголовки
теряются на границе JS↔Rust, потому что **границы для них нет**.

## Механизм

`_lumen_fetch` разбирает `init.headers` ровно один раз и ровно ради одного
значения — `Content-Type` для тела запроса (`contentType`). Дальше вызывается
одна из нативных привязок:

```
_lumen_fetch_sync(url, method)
_lumen_fetch_sync_with_body(url, method, contentType, bodyBytes)
_lumen_fetch_cancellable(url, method, timeoutMs)
_lumen_fetch_cancellable_with_body(url, method, contentType, bodyBytes, timeoutMs)
_lumen_fetch_async_start(url, method, contentType, bodyBytes, hasBody)
```

Параметра заголовков нет ни у одной. Он отсутствует и уровнем ниже — в трейте
`lumen_core::ext::JsFetchProvider`:

```rust
fn fetch_sync(&self, url: &str, method: &str) -> Result<JsFetchResult>;
fn fetch_with_body_sync(&self, url: &str, method: &str, content_type: &str, body: &[u8]) -> …
```

То есть `Request.headers`/`init.headers` физически некуда положить: канал
обрывается на самой первой ступени, а не где-то в `HttpClient`.

Симметричная сторона (заголовки **ответа**) работает — `_lumen_fetch_get_headers()`
есть и `Response.headers` из неё наполняется. Дыра ровно односторонняя.

## Как воспроизвести

Нужен сервер, который отражает полученные заголовки (`httpbin`-подобный
`/headers`, или `python -m http.server` с логированием). Пробы через
`--dump-display-list` недостаточно — дефект виден только на проводе:

```js
fetch('http://127.0.0.1:8000/headers', { headers: { 'X-Probe': '1' } })
  .then(function(r) { return r.text(); })
  .then(function(t) { document.title = t; });   // X-Probe в ответе не будет
```

Быстрее — по сигнатуре: `grep -n "fn fetch_sync" crates/core/src/ext.rs`.

## Влияние

Практически любой авторизованный API-вызов с страницы не работает: `Authorization`,
`X-CSRF-Token`, `X-Requested-With`, `Accept: application/json`, кастомные
API-ключи, `If-None-Match`/`If-Modified-Since` в ручном кэшировании — всё
молча отбрасывается, и сервер отвечает 401/403/406 либо отдаёт HTML вместо
JSON. Отладка со стороны страницы вводит в заблуждение: `Headers` заполнен,
`request.headers.get(...)` отвечает верно — то есть страница видит все
признаки того, что заголовки установлены.

Внутри WPT: категория `fetch` — `fetch/api/request/request-headers.any.js`,
`fetch/api/headers/header-values.any.js`, весь `fetch/api/cors/` (preflight
без `Access-Control-Request-Headers` бессмыслен).

## Что чинить

Канал придётся протянуть насквозь, это не однострочник:
`WEB_API_SHIM` → нативные привязки в `v8_runtime.rs` → `JsFetchProvider`
(четыре метода + async-старт) → `HttpClient`. Дефолтный набор заголовков
(User-Agent, Accept, Sec-CH-UA), который сейчас ставит сам `HttpClient`,
должен при этом остаться и правильно смёржиться с пользовательским списком —
именно поэтому передавать нужно список пар, а не готовую строку.

## Как починено (2026-08-17)

Канал протянут насквозь: `WEB_API_SHIM` → пять нативных привязок
(`v8_runtime.rs`) → `JsFetchProvider` → `HttpClient` → `fetch_with_redirect`.

**Не пятый параметр к каждому из четырёх методов, а одна структура.**
`lumen_core::ext::JsFetchRequest` описывает запрос целиком (url, метод,
заголовки, тело, токен отмены), `JsFetchProvider::fetch_request` — единственный
метод, который её принимает. Четыре старых метода умножали два независимых
признака (тело × отменяемость), и пятый вход пришлось бы продевать через все
четыре одинаково — ровно так этот канал и оказался ненаписанным. `HttpClient`
теперь реализует только `fetch_request`, остальные четыре к нему сводятся;
дефолтная реализация в трейте (для тестовых дублей) заголовки роняет, и это
записано в её доке.

**Заголовки вытесняют, а не дублируют.** Author-заголовок уезжает через слот
`extra_request_headers` рядом с `Cookie`/`Origin`/cache-валидаторами, а
`http::build_request_headers` снимает из fingerprint-набора одноимённый
заголовок. Иначе `fetch(url, {headers: {Accept: 'application/json'}})` уехал бы
вторым `Accept` следом за нашим. HTTP/2-путь (`build_h2_headers`) это правило
соблюдал с самого начала — H1 приведён к нему, расхождение H1↔H2 само по себе
отпечаток.

**Проверка стоит на проводе, а не только в шиме.** `build_author_headers`
отбрасывает forbidden-имена (Fetch §4.4.4), имена не-token и значения с CR/LF
(инъекция расщепила бы запрос), а при наличии тела — `Content-Type` (его несёт
`RequestBody`). Guard `Headers` покрывает `fetch()`, но
`XMLHttpRequest.setRequestHeader` пишет в свой объект мимо него — сокет
единственная общая для обоих точка. Заодно `cors::is_forbidden_request_header`
дополнен двумя именами из актуальной редакции спеки (`set-cookie`,
`access-control-request-private-network`).

**Заодно починен XHR** — тот же канал, та же потеря: из всего списка
`setRequestHeader` на провод уходил только `Content-Type`, и то лишь при
наличии тела.

### Чем проверено

Юнит-тесты на обоих концах канала: `lumen-js` — три спековые формы заголовков
(запись / `Headers` / `Request`), вытеснение списка `Request`-а `init.headers`-ами,
guard на готовом `Headers`, XHR; `lumen-network` — четыре проводных теста
(заголовки на сокете, единственный `Accept`, отбрасывание forbidden + CRLF,
неудвоенный `Content-Type`).

Сквозная проверка — живое окно через MCP против локального сервера-отражателя
(`.tmp/echo_headers_server.py`): все пять запросов пришли с ожидаемыми
заголовками, `Accept: application/json` на проводе ровно один.

Гейт на пробе: **страницу надо отдавать по http с того же origin**. С
`file://`-страницы `fetch()` молча падает `TypeError: network error` — в этом
пути JS-рантайм устанавливается без сетевого провайдера
(`shell/src/main.rs`, `fetch_provider = None`), и по симптому это неотличимо от
блокировки запроса.

## Связанные

- [BUG-370](BUG-370-FIXED.md) — при его закрытии и обнаружен.
- [BUG-369](BUG-369-FIXED.md) — `Headers` как WebIDL-интерфейс: объект, который
  корректно наполняется и никуда не отправляется.
- [BUG-347](BUG-347-FIXED.md) — соседняя строка того же шима (`fetch()` не
  резолвил относительные URL).
