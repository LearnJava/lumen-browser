# BUG-749 — `fetch()` не отправляет заголовки запроса вообще: ни `init.headers`, ни `Request.headers` не доезжают до сети

**Статус:** OPEN
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

## Связанные

- [BUG-370](BUG-370-FIXED.md) — при его закрытии и обнаружен.
- [BUG-369](BUG-369-FIXED.md) — `Headers` как WebIDL-интерфейс: объект, который
  корректно наполняется и никуда не отправляется.
- [BUG-347](BUG-347-FIXED.md) — соседняя строка того же шима (`fetch()` не
  резолвил относительные URL).
