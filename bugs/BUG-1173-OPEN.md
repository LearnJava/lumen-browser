# BUG-1173: `new URL(x)` без base резолвит относительно `location.href` вместо `TypeError`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/shim/url_shim.js:254` — конструктор `URL`, `:166` — `_url_resolve`)
**Найден:** P3, при починке [BUG-646](BUG-646-FIXED.md), 2026-09-25

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

## Масштаб

Все проверки «это абсолютный URL?» через `try { new URL(v) } catch`:

- `<input type=url>` — `typeMismatch` (`crates/js/src/form_validation.rs:63`,
  `crates/js/src/shim/web_api_shim_mid.js:4828`): значение `foo` проходит как
  валидный URL, форма отправляется без ошибки валидации.
- `new WebTransport(url)` (`crates/js/src/webtransport.rs:427`).
- `MediaMetadata` artwork `src` без base (`crates/js/src/media_session.rs:93`).
- Любой сайт, различающий абсолютный/относительный URL через `new URL(x)`.

## Дальше

Убрать подстановку `location.href` в конструкторе `URL` и прогнать
`url/` WPT + все внутренние вызовы `new URL(x)` с одним аргументом: часть
из них (шим, не страница) может молча полагаться на текущее поведение —
их нужно перевести на `new URL(x, location.href)` явно.
