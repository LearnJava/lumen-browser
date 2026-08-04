# BUG-592: `setHTMLUnsafe`/`getHTML`/`parseHTMLUnsafe` family only implemented on `Element`, missing on `ShadowRoot` and `Document`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:1315-1366` `_lumen_make_shadow_root` -- no `setHTMLUnsafe`/`getHTML` members, contrast the element object literal at `dom.rs:3474-3485` which has both; `Document.parseHTMLUnsafe` static factory does not exist anywhere -- `grep -n "parseHTMLUnsafe" crates/js/src/dom.rs` is empty)
**Найден:** P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Симптом

```
FAIL ShadowRoot: setHTMLUnsafe with no shadowdom. - assert_true: container.setHTMLUnsafe is not a function expected true got false
```
(`html/webappapis/dynamic-markup-insertion/html-unsafe-methods/setHTMLUnsafe.html`,
`Element` variant of the same test passes)

```
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe.html
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe-url.html
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe-encoding.html
```

## Причина

`Element.prototype.setHTMLUnsafe`/`getHTML` (WHATWG HTML LS §14.5) are
implemented once, directly inside the per-element object literal built by
`_lumen_make_element` (`dom.rs:3477`/`3483`). `ShadowRoot` is a separate,
hand-rolled object literal (`_lumen_make_shadow_root`) that does not share
`Element`'s prototype or method table, and was never given the same two
methods, even though the spec places both `setHTMLUnsafe`/`getHTML` on a
shared `Element`/`ShadowRoot` mixin. Separately, `Document.parseHTMLUnsafe`
-- the *static* factory that parses a full HTML document string into a new
detached `Document` -- was never added at all, on either the `document`
object literal or a `Document` constructor.

## Масштаб

9 subtests fail with `container.setHTMLUnsafe is not a function` in
`setHTMLUnsafe.html`/`setHTMLUnsafe-CEReactions.html`/
`setHTMLUnsafe-runScripts.html` (each parameterized over `['Element',
'ShadowRoot']`, so exactly half of each file's assertions). All 8
`Document-parseHTMLUnsafe*.html` files TIMEOUT outright (harness never
reaches a single subtest) since the very first statement calls the missing
static method.
