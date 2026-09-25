# BUG-656 — `PresentationRequest` constructor performs no argument validation at all

**Статус:** FIXED 2026-09-26 (P3)
**Компонент:** js (`crates/js/src/presentation_api.rs:102-106` — `PresentationRequest`
constructor in `PRESENTATION_API_SHIM`)
**Найден:** P2, WPT-VENDOR-presentation-api (2026-08-05), category run
(`run_report.py --root presentation-api`, 0/13 harness OK — all `.https.` ids
TIMEOUT on the TLS `UnknownIssuer` gap, no execution) + live probe
(`--mcp-live-port`, since none of the WPT ids could execute)

## Симптом

WPT's `PresentationRequest_error.https.html` (never reaches Lumen through the
runner — TLS gap) asserts five distinct failure modes:

```
new PresentationRequest()                                    → TypeError
new PresentationRequest([])                                  → NotSupportedError
new PresentationRequest('https://@')                          → SyntaxError (invalid URL)
new PresentationRequest('unsupported://example.com')          → NotSupportedError
new PresentationRequest(['presentation.html', 'https://@'])   → SyntaxError
```

Live probe (`--mcp-live-port`, script has no user gesture nor network
dependency, so the TLS gap doesn't apply) reproduces directly:

```json
{
  "no_args": "NO THROW, urls=[]",
  "empty_array": "NO THROW, urls=[]",
  "invalid_url": "NO THROW, urls=[\"https://@\"]",
  "unsupported_scheme": "NO THROW, urls=[\"unsupported://example.com\"]",
  "mixed_valid_invalid": "NO THROW, urls=[\"presentation.html\",\"https://@\"]"
}
```

All five throw nothing — the constructor accepts every input unconditionally.

## Причина

`PresentationRequest` in `presentation_api.rs:102-106`:

```js
function PresentationRequest(urls) {
  // Normalise single string to array per spec §6.3.
  this._urls = Array.isArray(urls) ? urls : (typeof urls === 'string' ? [urls] : []);
  this._listeners = Object.create(null);
}
```

This only normalises the shape (string → 1-element array, anything else →
`[]`); it never checks argument count, never validates each entry as a URL,
and never checks the URL scheme against §6.4's "supported" list. The module's
own doc comment (`presentation_api.rs:14`) documents this as intentional
Phase-0 scope ("no actual display discovery or projection"), but §6.3
construction-time validation is independent of display discovery — it's pure
input checking the spec requires synchronously in the constructor, before any
device interaction.

## Как чинить

Add validation to the constructor per W3C Presentation API §6.3
("constructing a `PresentationRequest`"):
1. Normalise to an array (existing behaviour), but if the input is `undefined`
   → throw `TypeError`.
2. If the array is empty → throw `NotSupportedError`.
3. For each entry: attempt `new URL(entry, baseURL)`; on failure → throw
   `SyntaxError`.
4. If none of the parsed URLs use a scheme this Phase-0 stub could ever
   support (`https:`/`http:` are the only ones any real implementation would
   register) → throw `NotSupportedError`. Since Phase 0 never discovers any
   display, this can conservatively check the scheme is `http:`/`https:`
   without needing a real display-compatibility check.

Regression without WPT: eval the five constructor calls above through
`--mcp-live-port`/`--dump-layout` and assert each throws the documented
exception type/name instead of `NO THROW`.

## Связанные

The category's own WPT run produced zero executed ids — all `.https.`-suffixed
files hit the already-documented TLS `UnknownIssuer` gap (`docs/wpt-status.md`,
recurring across most `.https.`-only categories) before reaching any
JS. `receiving-ua/` and most of `controlling-ua/` are `-manual` tests (24 of 37
files) that `run_report.py` never selects — inherent to this category (display
casting requires a human to observe a second screen), not a vendoring gap.
This bug was found only by reading `presentation_api.rs` directly and
confirming with a live probe, per the "a 🚫-scoped category is not
automatically finding-free" rule
([[reference_wpt_run_report_invocation_recipe]]).

## Починка (2026-09-26, P3)

Конструктор в `presentation_api.rs` валидирует вход по §6.3.1 и WebIDL-перегрузке
`(USVString url)` / `(sequence<USVString> urls)`:

- вызов без `new` или без аргументов → `TypeError`;
- итерируемый объект → последовательность (`String()` на каждый элемент), иначе
  `[String(arg)]`; пустая последовательность → `NotSupportedError`;
- каждый URL разбирается `new URL(u, document.baseURI)` (запасной — `location.href`),
  ошибка → `SyntaxError`;
- в защищённом контексте (`isSecureContext === true`) любой a priori
  unauthenticated URL (`http:`/`ws:` не на loopback) → `SecurityError` —
  это WPT `PresentationRequest_mixedcontent{,_multiple}.https.html`;
- поддерживаются только `http:`/`https:`: неподдерживаемые отбрасываются, если
  рядом есть поддерживаемый (WPT `PresentationRequest_success`), иначе
  `NotSupportedError`. `_urls` хранит разрешённые `href`.

Регрессия: `presentation_api::ctor_validation_tests` — все случаи WPT
`PresentationRequest_error.https.html` плюс success/mixedcontent, через реальный
`install_dom` (настоящие `URL`/`DOMException`, `isSecureContext` от схемы страницы).
Старый тестовый модуль переведён на тот же `install_dom`: голый `V8JsRuntime`
не содержит `URL`, а конструктор теперь от него зависит. WPT-прогон категории
по-прежнему упирается в TLS-гэп `UnknownIssuer` до JS.
