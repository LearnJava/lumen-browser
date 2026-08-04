# BUG-601: `DOMTokenList` constructor not exposed on `window` — `instanceof DOMTokenList` throws `ReferenceError`, even though the object itself works correctly

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `// ── DOMTokenList (classList) ──` section around line 1191)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL focusGroup IDL attribute is a DOMTokenList - DOMTokenList is not defined
FAIL focusGroup DOMTokenList and focusGroupStart are exposed on SVGElement via the HTMLOrSVGOrMathMLElement mixin - DOMTokenList is not defined
FAIL focusGroup DOMTokenList and focusGroupStart are exposed on MathMLElement via the HTMLOrSVGOrMathMLElement mixin - DOMTokenList is not defined
```
(`focusgroup/tentative/idl-reflection.html`, 3 of 20 subtests — every other
subtest in the same file, exercising `.value`, `.contains()`, `.add()`,
`.remove()`, `.toggle()`, `.supports()`, `[SameObject]` identity and
`PutForwards` on the very same `element.focusGroup` object, **passes**)

## Причина

`element.focusGroup`/`classList` are backed by a real, spec-correct
`DOMTokenList`-shaped object internally (17/20 subtests in this file
confirm every method and the `[SameObject]` contract work), but the
constructor is never assigned as a global — `typeof window.DOMTokenList` is
`"undefined"`. Any test or page script that does `x instanceof DOMTokenList`
or `new DOMTokenList(...)` fails, even though the object it's checking is
functionally correct.

## Масштаб

Narrow and mechanical: add `window.DOMTokenList = <the internal
constructor/class>` (or an equivalent global binding) next to the existing
`DOMTokenList (classList)` implementation. Only the identity-check subtests
fail; no functional regression in `classList`/`focusGroup` themselves.
