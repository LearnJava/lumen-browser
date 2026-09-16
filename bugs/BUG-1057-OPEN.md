# BUG-1057: `document.all` missing its spec-mandated `[[IsHTMLDDA]]` "unusual behaviors"

**Статус:** OPEN (ДОРАБОТКА → GAP-DOCALLDDA)
**Тип:** ДОРАБОТКА — требует расширения биндинга `rusty_v8` (нативный V8-примитив `MarkAsUndetectable`, которого нет в поверхности крейта), не JS-only правку
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` -- `document.all` absent entirely; `rusty_v8` binding -- no `MarkAsUndetectable` exposed)
**Найден:** P3 при работе над BUG-606, 2026-09-16

## Симптом

```
FAIL 'unusual behaviors' of document.all - assert_true: expected true got false
```
(`obsolete/requirements-for-implementations/other-elements-attributes-and-apis/document-all.html`)

## Причина

HTML LS §obsolete requires `document.all` to be an `HTMLAllCollection` carrying
the `[[IsHTMLDDA]]` internal slot: `typeof document.all === "undefined"`, loose
equality to both `null` and `undefined`, falsy in boolean context, and calling
it as a function returns `undefined`. This is not reproducible from JS alone --
a `Proxy` has no `typeof` trap, so no shim-side object can make `typeof` report
`"undefined"`. Real engines implement it as a genuine internal slot; in V8
terms that is `v8::ObjectTemplate::MarkAsUndetectable()` (confirmed present in
the vendored V8 C++ headers shipped inside the `v8` crate,
`v8-150.1.0/v8/include/v8-template.h:1099`), but the `rusty_v8` Rust binding
(`v8-150.1.0/src/*.rs`) exposes no equivalent -- `grep -rn "undetectable"`
over that directory is empty.

Fixing this requires extending the `rusty_v8` binding itself (upstream PR or
a local patch) so `crates/js`'s native install path can mark an object
undetectable, then wiring `document.all` through it. Not a shim-only,
JS-level fix like the rest of BUG-606's scope, which is why it was split off.

## Масштаб

1 subtest, `document-all.html`. Self-contained -- no other WPT category in
this corpus depends on the DDA slot.
