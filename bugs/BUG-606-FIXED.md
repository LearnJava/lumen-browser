# BUG-606: legacy no-op APIs missing entirely — `document.clear()`/`captureEvents()`/`releaseEvents()`, `window.captureEvents()`/`releaseEvents()`, `document.all` unusual behaviors, `document.applets`, `HTMLScriptElement.event`/`.htmlFor`

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `document` object literal; `crates/js/src/shim/web_api_shim_tail_b.js` — `window` no-ops, `HTMLScriptElement` reflection table)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL document.clear - document.clear is not a function
FAIL document.captureEvents - document.captureEvents is not a function
FAIL document.releaseEvents - document.releaseEvents is not a function
FAIL window.captureEvents - window.captureEvents is not a function
FAIL window.releaseEvents - window.releaseEvents is not a function
FAIL 'unusual behaviors' of document.all - assert_true: expected true got false
FAIL document.applets should return an empty collection. - assert_true: expected true got false
FAIL event and htmlFor IDL attributes of HTMLScriptElement - assert_equals: expected (string) "" but got (undefined) undefined
```
(`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/{nothing,document-all,document-applets,script-IDL-event-htmlfor}.html`)

## Причина

HTML LS §obsolete requires several historical APIs to keep existing purely
for compatibility, even though they must do nothing (or something
deliberately "wrong" per spec):

- `document.clear()`, `document.captureEvents()`, `document.releaseEvents()`,
  `window.captureEvents()`, `window.releaseEvents()` must exist as callable
  no-op methods returning `undefined`. None of the five exist at all in
  Lumen's shim.
- `document.all` must exist with the documented "unusual behaviors": it's
  an `HTMLAllCollection` that is loosely-`==` to both `null` and
  `undefined`, `typeof document.all === "undefined"`, and it's falsy in
  boolean context — all of which requires a real `[[IsHTMLDDA]]` internal
  slot on the collection object. Lumen has neither `document.all` (falls
  through to plain `undefined`, so most assertions coincidentally pass by
  being genuinely `undefined`) nor the DDA-slot semantics for when it is
  eventually implemented.
- `document.applets` must return an (always-empty, in a spec-compliant
  implementation with no legacy Java-applet support) live `HTMLCollection`.
  Currently missing entirely (`assert_true(collection instanceof
  HTMLCollection)` fails).
- `HTMLScriptElement.event`/`.htmlFor` are legacy IDL attributes reflecting
  the `event`/`for` content attributes verbatim (string, no special
  parsing) — both missing, `script.event`/`script.htmlFor` are `undefined`
  instead of `""`.

## Масштаб

4 self-contained files, ~13 subtests, all under
`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/`.
No other category in this corpus depends on these obsolete symbols.

## Исправлено

Seven of the eight symbols are plain shim additions:

- `document.clear()`/`captureEvents()`/`releaseEvents()` — no-op methods on
  the `document` object literal (`web_api_shim_mid.js`), next to `images`.
- `window.captureEvents()`/`releaseEvents()` — no-op functions in
  `web_api_shim_tail_b.js`, right beside the existing `window.focus`/`blur`
  no-ops (same rationale: feature-detection code calls them
  unconditionally).
- `document.applets` — a live, always-empty `HTMLCollection` built through
  the same `_lumen_make_nid_collection` helper `document.images` already
  uses, with an `idsFn` that returns `[]`.
- `HTMLScriptElement.event`/`.htmlFor` — two new rows
  (`['event','event','string']` / `['htmlFor','for','string']`) in the
  existing declarative reflection table (`_lumen_install_reflection`), same
  mechanism as every other reflected IDL attribute since BUG-383.

`document.all`'s `[[IsHTMLDDA]]` "unusual behaviors" turned out to be a
genuine engine gap, not a shim fix: `typeof document.all === "undefined"`
cannot be reproduced from JS (a `Proxy` has no `typeof` trap), and the
underlying V8 primitive that real engines use
(`v8::ObjectTemplate::MarkAsUndetectable()`, confirmed present in the
vendored V8 C++ headers shipped inside the `v8` crate) is not exposed by the
`rusty_v8` Rust binding (`grep -rn undetectable` over its `src/*.rs` is
empty). Split off as [BUG-1057](BUG-1057-OPEN.md) /
[GAP-DOCALLDDA](../ROADMAP.md) rather than left half-done under this number.

3 new tests in `crates/js/src/dom/tests/v8_bug606_legacy_noop_apis.rs`
(3/3 green). `cargo clippy -p lumen-js --all-targets --features v8-backend
-- -D warnings` чист.
