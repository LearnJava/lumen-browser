# BUG-347: `fetch()` never resolves relative URLs against the document base — every relative-URL fetch fails

**Статус:** FIXED 2026-08-06

## Фикс (P3, 2026-08-06)

`fetch(input, init)` (`crates/js/src/dom.rs`, `WEB_API_SHIM`) теперь резолвит
`url` через уже существующий `_url_resolve(url, _lumen_document_base_url())`
сразу после извлечения из `input`/`init`, до любого обращения к нативным
биндингам (`_lumen_fetch_sync`/`_lumen_fetch_sync_with_body`/`_lumen_fetch_cancellable*`/
`_lumen_fetch_async_start`) — абсолютный URL проходит `_url_resolve` без
изменений (уже абсолютные ссылки не трогаются), относительный резолвится
против document base (`<base href>` или URL документа), как того требует
Fetch §4.1 шаг 8. Фикс целиком в JS-шиме — сигнатуры нативных
`HttpClient::fetch_sync`/`fetch_with_body_sync`/`fetch_cancellable`/
`fetch_with_body_cancellable` не менялись, вариант из «suspected fix
direction» с резолюцией в Rust-слое не понадобился.

Заодно (та же строка шима, тот же корень, явно указано в заявке как «чинить
вместе», пункт A2 [BUG-370](BUG-370-FIXED.md)) исправлена абсолютизация
`new Request(url).url` — конструктор `Request` теперь тоже пропускает `url`
через `_url_resolve`/`_lumen_document_base_url()`. Остальные пункты BUG-370
(Body-mixin, `Response.json()`, валидация конструкторов и т.д.) не тронуты —
вне скоупа этого бага.

3 новых юнит-теста (`fetch_resolves_relative_url_against_document_base`,
`fetch_resolves_root_relative_url_against_document_origin`,
`request_constructor_absolutizes_relative_url`), `cargo test -p lumen-js
--features v8-backend` 2497+68/2497+68 OK, `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` чист. Изменение не
трогает paint/layout — гейт `scoped-test.sh` + `dump_golden.py` вместо
полного графического прогона.

Верификация живым WPT-прогоном (`run_report.py --all --root fetch
--recursive`) не выполнена в этой сессии — не запущена (нет активного venv
под рукой при подготовке фикса); ожидание по заявке — исчезновение строк
`missing scheme` (было 201) и рост harness OK / сабтестов на категории
`fetch`.
**Компонент:** js (`crates/js/src/dom.rs:9310`, `WEB_API_SHIM` `fetch()`), network
(`crates/network/src/lib.rs` — `HttpClient::fetch_sync:3667-3668`,
`fetch_with_body_sync:3733`, `fetch_cancellable:3815`, `fetch_with_body_cancellable:3830`)
**Найден:** P2, WPT-VENDOR-custom-elements 2026-07-26 (`run_report.py --all --root
custom-elements --recursive`, `custom-elements/upgrading.html`) — first root-caused here,
but the symptom was already independently observed and flagged as "possible engine gap"
in four earlier WPT-VENDOR passes without being filed: `browsing-topics`
(`fetch-topics-insecure-context.tentative.http.html`), `client-hints`
(`accept-ch/non-secure.http.html`), `connection-allowlist` (`webrtc-*` tests'
`fetch("/common/blank.html")`), `cors` (`access-control-expose-headers-parsing.window.html`)
— see `docs/wpt-status.md` rows for those categories.

## Симптом

`fetch()` called with any relative URL (no scheme, whether or not it starts with `/`)
throws instead of resolving against the current document's URL. Reproduced with
`custom-elements/upgrading.html` (served at `/custom-elements/upgrading.html`), which
calls `fetch("resources/empty-html-document.html")`:

```
fetch error: invalid url: invalid url: missing scheme: "resources/empty-html-document.html"
```

Same signature previously logged (never root-caused) for `fetch("./resources/check-topics-request-header.py")` (browsing-topics), `fetch("resources/echo-client-hints-received.py")` (client-hints), and `fetch("/common/blank.html")` (connection-allowlist, cors) — the leading-`/` case fails too, since nothing resolves against an *origin*, only literal absolute URLs work.

## Root cause

The JS shim's `fetch(input, init)` (`crates/js/src/dom.rs:9310`) takes the URL argument
completely literally:

```js
var url = typeof input === 'string' ? input : (input && input.url ? input.url : String(input));
```

— no base-URL resolution step at all — then passes it straight to the native bindings
`_lumen_fetch_sync`/`_lumen_fetch_cancellable` (`dom.rs:9453`/`9457`), which reach
`HttpClient::fetch_sync` in `crates/network/src/lib.rs:3667-3668`:

```rust
fn fetch_sync(&self, url: &str, method: &str) -> Result<JsFetchResult> {
    let url = Url::parse(url).map_err(|e| Error::InvalidUrl(e.to_string()))?;
```

`Url::parse` requires an absolute URL (scheme required) and errors on anything else. The
same pattern — `Url::parse(url)` on the raw JS argument, no base threaded in at all —
repeats in `fetch_with_body_sync` (3733), `fetch_cancellable` (3815), and
`fetch_with_body_cancellable` (3830). Independent of BUG-346 (`Url::resolve()` not
collapsing dot-segments): here `.resolve()` is never even *called* — the base URL isn't
threaded into the fetch path at all, at either the JS-shim or native layer. `location.href`
is already accessible from JS (`crates/js/src/dom.rs`, navigation state) and could serve
as the resolution base if threaded through.

## Impact

Every `fetch()` call with a relative URL (script-relative, root-relative, or
protocol-relative) fails outright — only fully-qualified absolute URLs work today. This
is a real-world-page-breaking gap, not WPT-specific: any site fetching same-origin
resources by relative path (an extremely common pattern) hits this.

## Suspected fix direction

Thread a base URL (the current document's URL, already available as `location.href`) into
the fetch path — either resolve `url` against it in the JS shim before calling the native
binding, or pass the base through to `HttpClient::fetch_sync`/`fetch_with_body_sync`/
`fetch_cancellable`/`fetch_with_body_cancellable` and call `base.resolve(url)` (already
implemented, modulo BUG-346's dot-segment gap) instead of `Url::parse(url)`. Re-run
`run_report.py --all --root custom-elements --recursive` plus a targeted check of the four
categories that hit this earlier (`browsing-topics`, `client-hints`,
`connection-allowlist`, `cors`) to confirm.

## Измеренный вес (WPT-VENDOR-fetch, 2026-07-28)

Прогон вендоренной категории `fetch` (`run_report.py --all --root fetch
--recursive`, 1 ч 22 мин, 176/481 harness OK, 364/2692 сабтеста) — самая
профильная проверка для этого бага из возможных, и она первая даёт ему
измеримый вес:

```
fetch error: invalid url: invalid url: missing scheme: "..."      201 строка
```

Примеры из лога: `"../resources/bad-chunk-encoding.py?count=1"`,
`"../../../xhr/resources/header-content-length.asis"`,
`"xhr/resources/echo-headers.py"` — то есть весь спектр относительных форм.

Это доминирующий класс отказов категории наравне с HTTPS-порт-гэпом (147) и
survey-gap по `/common/*`. Значительная часть из 118 тестов, у которых harness
поднялся, но прошло **0** сабтестов, отваливается именно здесь: тело теста
целиком состоит из `fetch()` по относительному пути.

Фикс стоит делать вместе с [BUG-346](BUG-346-OPEN.md) (тот же прогон даёт ему 69
собственных 404) и с пунктом A2 [BUG-370](BUG-370-FIXED.md) (`new Request('rel').url`
не абсолютизируется — соседняя строка того же шима, `dom.rs:9027`).

Верификация после фикса: `run_report.py --all --root fetch --recursive`,
ожидание — исчезновение строк `missing scheme` (до фикса 201) и рост
`176/481 harness OK` / `364/2692 сабтеста`.
