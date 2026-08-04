# BUG-618: `HTMLElement.prototype.inert` getter/setter is shadowed by a stale Phase-0 stub — ignores the `inert` content attribute entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/inert.rs::install_inert_api_v8`, `INERT_SHIM` — installed at `crates/js/src/v8_runtime.rs:4307`, after `WEB_API_SHIM`'s own reflection table entry at `crates/js/src/dom.rs:10729`)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`) on `<div id="i1" inert>...</div>`, freshly
parsed markup, no script mutation:

```js
document.getElementById('i1').hasAttribute('inert')   // → true  (attribute is present)
document.getElementById('i1').inert                    // → false (WRONG — should be true)
```

Contrast with the sibling boolean-reflected attribute `hidden` on the same
page, which reads correctly from markup:

```js
document.getElementById('h1').hidden   // → true (correct)
```

Setting `.inert = true` via script does make the getter return `true`
afterwards, so the property "works" for script-driven toggling — the bug is
specifically that it never picks up the attribute's *initial* (or any
attribute-mutation-driven) state.

## Причина

Two independent implementations of the same IDL property are installed in
sequence during `install_dom`, and the second silently wins:

1. `WEB_API_SHIM`'s generic attribute-reflection installer (added by
   BUG-383, 2026-07-29) declares `inert` as a normal boolean-reflected
   global attribute: `dom.rs:10729`, `['inert', 'inert', 'bool']` inside
   `_lumen_install_reflection(HTMLElement.prototype, [...])`. This getter
   correctly derives from `hasAttribute('inert')`, matching `hidden`'s
   correct behaviour.

2. Later in the same `install_dom` sequence (`v8_runtime.rs:4307`,
   `install_v8!(inert::install_inert_api_v8)`), `crates/js/src/inert.rs`
   runs its own `Object.defineProperty(HTMLElement.prototype, 'inert',
   {configurable: true, get: ..., set: ...})` with a getter that reads a
   private per-instance flag instead of the attribute:

   ```js
   get: function get_inert() {
     return this._inert === true;
   },
   set: function set_inert(value) {
     this._inert = Boolean(value);
     if (typeof globalThis._lumen_set_inert === 'function' && ...) {
       globalThis._lumen_set_inert(this.__nid, this._inert);
     }
   },
   ```

   `_inert` starts undefined on every element, including ones with the
   attribute set in markup, so the getter always begins at `false`
   regardless of the DOM. Because the property is `configurable: true`,
   this second `defineProperty` call clobbers the first, correct
   descriptor rather than being blocked by it.

   The module's own doc comment marks this as intentionally provisional —
   `//! Phase 0: the setter stores a flag on the element JS object...
   //! Phase 1 (shell wiring): implement _lumen_set_inert native binding
   //! to call Document::set_attr(node, "inert", "")...` — i.e. `inert.rs`
   predates BUG-383's generic reflection table and was never removed once
   the generic table gained its own (correct) `inert` entry.

   Note the actual *behavioural* effect of `inert` (unfocusability, see
   `_lumen_is_focusable` at `dom.rs:10411-10435`) is driven by
   `_lumen_has_attr(anc, 'inert')` directly, not by this JS-level `.inert`
   getter — so `<div inert>` content is correctly unfocusable even though
   `.inert` reads back `false`, which is precisely why this went
   unnoticed: the property's *side effects* look right, only reading it
   back does not.

## Масштаб

Directly asserted by WPT: `inert-node-is-unfocusable.html`'s "Can get
inert via property" and "Elements inside of inert subtrees return false
when getting 'inert'" subtests exercise exactly this getter, but never
actually ran in this session's category pass — the file errors out earlier
on the unrelated [BUG-462](BUG-462-OPEN.md)/[BUG-574](BUG-574-OPEN.md)
(`Node.prototype.contains` missing, used by vendored `testdriver.js`'s
click helper) before reaching them, so this defect was masked rather than
reported as its own FAIL. Any page/test that reads `.inert` right after
parse (rather than only ever setting it) will observe the same wrong
`false`. Fix: delete `inert.rs`'s `INERT_SHIM` (fully superseded by the
generic reflection table) and wire its Phase-1 `_lumen_set_inert` intent
into the generic bool-reflection setter instead, or simply drop the
`install_v8!(inert::install_inert_api_v8)` call once confirmed redundant.
