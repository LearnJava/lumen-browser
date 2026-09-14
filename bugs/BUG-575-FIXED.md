# BUG-575: `Element.prototype.localName` missing entirely

**Статус:** FIXED (закрыто ревизией) 2026-09-14
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_LUMEN_WRAPPER_MEMBERS`
element getter block, `dom.rs` since split into `crates/js/src/dom/`)
**Найден:** P2, WPT-VENDOR-html-semantics-forms, 2026-08-04 (root-caused with a
standalone `--dump-layout` probe outside the WPT run)

## Ревизия P3 2026-09-14

Уже устранено побочно фиксом [BUG-367](BUG-367-FIXED.md) (2026-08-10, шесть
дней после подачи этой заявки) — `localName` добавлен на `_LUMEN_WRAPPER_MEMBERS`
вместе с namespace-aware `tagName`/`prefix` (`web_api_shim_mid.js:6367`,
`get localName() { var nid = this.__nid__; return _lumen_u2n(_lumen_get_local_name(nid)); }`).
Подтверждено новым регрессионным тестом
`dom::tests::v8_core::element_local_name_is_lowercase`
(`document.getElementById('main').localName === 'div'`) — зелёный на чистом
`main`, код обёртки не менялся. `cargo test -p lumen-js --features v8-backend`
зелёный, `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` чист.

## Симптом

Probe (`--dump-layout`, outside WPT):

```html
<form id="f"></form>
<script>
document.title = 'f.localName=[' + f.localName + '] typeof=' + typeof f.localName;
</script>
```
→ `f.localName=[undefined] typeof=undefined` — the property is entirely
absent (not a broken getter returning the wrong case), on every element
tested (`<form>`, `<div>`).

Inside WPT this surfaces in `html/semantics/forms/form-submission-target/`
via the shared `resources/reltester.js` helper:

```js
// reltester.js:53-57
let form = submitter;
if (submitter.localName !== "form") {   // always true: localName is undefined
  form = submitter.form;                // a <form> element has no .form property → undefined
}
form.rel = relTest.rel;                 // TypeError: Cannot set properties of undefined (setting 'rel')
```
`submitter` is itself the `<form>` element, but since `submitter.localName`
reads `undefined` instead of `"form"`, the helper takes the wrong branch and
tries `submitter.form` (undefined for a form element), then crashes setting
`.rel` on it.

## Причина

`tagName`/`nodeName` are wired as getters on the `_lumen_build_element(nid)`
object literal (`dom.rs:5595-5596`, native `_lumen_get_tag_name(nid)`,
upper-case per HTML §2.5.1 for elements in the HTML namespace). `localName`
— DOM §4.9 `Element.localName`, the tag's local name without namespace
prefix, lower-case for HTML elements — was never added alongside them. For
this engine's non-namespace-aware element model, `tagName.toLowerCase()`
would be a correct-enough first cut for HTML elements (the common case
exercised by this report); getting SVG/foreign-namespace `localName`
byte-exact would need the same namespace plumbing `namespaceURI` already
has.

## Масштаб

Directly counted: **14 subtests in 2 files**
(`form-submission-target/rel-base-target.html`,
`form-submission-target/rel-form-target.html`) crash via this exact path.
`.localName` is a very commonly used idiom for lower-case tag-name checks
(this engine's own `tagName` is upper-case-only, so code written against the
spec reaches for `.localName` specifically to avoid a `.toUpperCase()`/
`.toLowerCase()` dance) — likely to resurface as a wider-impact finding in
any WPT category that branches on element type via `.localName` rather than
`.tagName`.
