# BUG-309 — `Element.prototype.setAttributeNS`/`getAttributeNS`/`removeAttributeNS` missing

**Статус:** FIXED 2026-07-19
**Компонент:** js (`crates/js/src/dom.rs`, `WEB_API_SHIM` — engine-agnostic DOM shim)
**Найден:** P2-wpt, 2026-07-18, running the WPT `dom/nodes/Element-hasAttribute.html` test end to end through `wptrunner` after BUG-301 was fixed.

## Симптом

`document.createElement("p").setAttributeNS("foo", "x", "first")` throws
`TypeError: el.setAttributeNS is not a function`. The namespaced attribute
accessors (`setAttributeNS`, `getAttributeNS`, `removeAttributeNS`,
`hasAttributeNS`, `getAttributeNodeNS`, `setAttributeNodeNS`) are absent from
the `Element.prototype` shim (`grep setAttributeNS crates/js/src/dom.rs` → no
matches).

This is the first genuine engine gap surfaced by the now-working WPT harness
(BUG-301 fixed): `dom/nodes/Element-hasAttribute.html` runs to completion and
reports subtest 1 ("hasAttribute should check for attribute presence,
irrespective of namespace") as a real **FAIL**, subtest 2 (case-insensitive
`hasAttribute`) as PASS. The FAIL is recorded as an `expected: FAIL` line in
`tests/wpt/metadata/dom/nodes/Element-hasAttribute.html.ini` (not a weakened
test — the harness genuinely observed it); flip that to PASS once this lands.

## Что нужно для закрытия

Implement the `*NS` attribute methods on `Element` in the shared shim
(`WEB_API_SHIM`, `crates/js/src/dom.rs`). DOM §4.9.2: the namespace argument is
stored but Lumen's attribute model is currently name-only, so at minimum
`setAttributeNS(ns, qualifiedName, value)` should set the attribute under
`qualifiedName` (matching `hasAttribute`/`getAttribute`'s name-based lookup),
`getAttributeNS(ns, localName)` read it back, etc. Add a mirrored V8/QuickJS
test as required for shim changes, and re-run
`tests/wpt/run_smoke.py --binary <lumen> /dom/nodes/Element-hasAttribute.html`
to confirm both subtests PASS, then update the `.ini`.

## Фикс (2026-07-19)

Added the four core namespaced accessors to `Element.prototype` in the shared
`WEB_API_SHIM` (`crates/js/src/dom.rs`), directly after `hasAttribute`:
`getAttributeNS`, `setAttributeNS`, `removeAttributeNS`, `hasAttributeNS`. Lumen's
attribute model is name-only, so the `namespace` argument is accepted but ignored —
each method stores/looks up the attribute under its qualified name, matching the
existing name-based `getAttribute`/`hasAttribute` lookup; `setAttributeNS` also fires
the custom-element `attributeChangedCallback` hook exactly like `setAttribute`.
The Attr-node variants (`getAttributeNodeNS`/`setAttributeNodeNS`) are intentionally
omitted — the base non-`NS` `getAttributeNode`/`setAttributeNode` do not exist in the
shim either (no Attr node objects), so adding only the `NS` forms would be
inconsistent.

Test `attribute_ns_methods_are_name_based` (`crates/js/src/dom.rs`) reproduces WPT
`dom/nodes/Element-hasAttribute.html` §1 and exercises get/has/remove. The
`.ini` expectation file was **removed** (both subtests now PASS, no non-default
expectation remains).
