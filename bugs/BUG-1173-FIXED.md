# BUG-1173: `new URL(x)` без base резолвит относительно `location.href` вместо `TypeError`

**Статус:** FIXED 2026-11-15
**Компонент:** js (`crates/js/src/shim/url_shim.js:273` — конструктор `URL`, `:166` — `_url_resolve`)
**Найден:** P3, при починке [BUG-646](BUG-646-FIXED.md), 2026-09-25
**Исправлено:** P3, 2026-11-15

## Симптом

На странице `https://example.com/` (полный рантайм, `install_dom`):
`new URL("0")`, `new URL("a--b")`, `new URL("../realitive/url")`,
`new URL("/absolute/../path?")`, `new URL("Secure-Payment-Confirmation")` —
все возвращают объект с `protocol === "https:"`, вместо того чтобы бросить
`TypeError`.

Наблюдено как 22 «no throw» в первой версии регрессии BUG-646: валидатор PMI
считал строку URL-based, если `new URL(pmi)` не бросал, и все строки без схемы
из списка WPT `payment-request-ctor-pmi-handling.https.sub.html` прошли как
валидные `https:`-URL.

## Причина

`url_shim.js:256`:

```js
var resolved = _url_resolve(String(href), base ? String(base) : (typeof location !== 'undefined' ? location.href : ''));
```

Когда `base` не передан, подставляется `location.href`. URL Standard
§`new URL(url, base)`: если `base` не передан, парсится `url` **без** base, и
строка без схемы — failure → `TypeError`. Документный base применяется только
там, где спека его явно требует (атрибуты `href`/`src`, `fetch`, `location.assign`
и т.п.), не в конструкторе `URL`.

## Решение

Конструктор исправлен коммитом `cccdcaa74` (GAP-ORIGIN, 2026-09-27): при
`base === undefined` он вызывает `_lumen_parse_url(String(href))` без base, и
пустой `protocol` даёт `TypeError`. Эта задача — обещанный в карточке аудит
вызовов `new URL(x)` с одним аргументом: часть шимов молча полагалась на
старую подстановку `location.href` и после исправления сломалась бы.

Проверены все вызовы `new URL(` в `crates/js` (11 штук с одним аргументом):

- `<input type=url>` — `typeMismatch` (`form_validation.rs:63`,
  `web_api_shim_mid.js:6671`): новый `TypeError` и есть требуемая проверка
  «это абсолютный URL», правки не нужны.
- `new WebTransport(url)` (`webtransport.rs`) — **регрессия от GAP-ORIGIN**:
  спека §6.2 парсит `url` относительно API base URL, поэтому относительный
  URL обязан работать. Шим теперь передаёт base явно
  (`_lumen_document_base_url()`), как и до исправления конструктора.
- `MediaMetadata` artwork `src` (`media_session.rs:93`) — base передавался и
  до этого (`document.baseURI`/`location.href`), поведение не изменилось.
- `PaymentRequest` PMI (`payment_request.rs`) — guard `URL_SCHEME` и его
  комментарий описывали уже несуществующую подстановку `location.href`;
  guard снят, схема без base и так даёт `TypeError` → PMI уходит в ветку
  стандартизованных идентификаторов.
- Остальные (`frame_bridge.rs` URL под-документа, `sanitizer_shim.js`
  `javascript:`-проверка, `url_pattern_shim.js` сборка из `//dummy.test`,
  `web_api_shim_mid_b*.js` сравнение origin, `svg_idl_shape.js`) работают с
  заведомо абсолютными строками или передают base — правок не требуют.

## Тесты

- `crates/js/tests/cases/bug1173_url_ctor_no_base.rs` — все пять строк из
  симптома плюс `''`/`'not-valid'` бросают `TypeError`, абсолютный URL и
  явный base резолвятся; `<input type=url>` с `foo` даёт `typeMismatch`,
  с `https://…` — валиден.
- `crates/js/src/webtransport/tests.rs::constructor_resolves_a_relative_url_against_the_document_base`
  — `new WebTransport('/wt')` на странице `https://example.com/dir/page` не
  бросает `SyntaxError` (без правки шима — бросает).

`cargo test -p lumen-js --features v8-backend --test all -- bug1173` — 2 passed;
`--lib -- webtransport::tests_v8::constructor` — 4 passed;
`--lib -- payment_request` — 12 passed (включая BUG-646 `v8_bug646_*`).
Полный WPT-прогон `url/` не делался — сетевой корпус в этой сессии не поднят.
