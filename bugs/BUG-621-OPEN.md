# BUG-621: `HTMLLabelElement.focus()` doesn't delegate focus to the labeled control

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::HTMLElement.prototype.focus`, `dom.rs:10510-10520`)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`) on `<label id="for-submit" for="submit">
Label for Submit</label><input id="submit" type="submit">`:

```js
var label = document.querySelector('#for-submit');
label.control === document.querySelector('#submit')   // → true  (correct — BUG-383's label graph works)
label.focus();
document.activeElement === document.querySelector('#submit')   // → false (WRONG — should be true)
```

Matches `inert-label-focus.html`'s first subtest, "Calling focus() on an
inert label should still send focus to its target." (FAIL:
`assert_equals: expected object "[object Object]" but got object
"[object Object]"` — both operands stringify identically, masking that
they are in fact different element references).

## Причина

`HTMLElement.prototype.focus` (`dom.rs:10510`) has no special case for
`<label>`:

```js
HTMLElement.prototype.focus = function(options) {
    var nid = this.__nid__;
    if (nid === null || nid === undefined) return;
    if (!_lumen_is_focusable(nid)) return;
    _lumen_request_focus(nid);
    ...
```

Per HTML LS §6.6.3 "focusing steps", when the focus target is a `<label>`
element, focus should be delegated to `label.control` (the labeled form
control) rather than applied to the label itself — a `<label>` is not a
focusable area on its own (it carries no default `tabindex`, and
`_LUMEN_FOCUSABLE_TAGS` almost certainly doesn't list `LABEL`; see
`_lumen_is_focusable` at `dom.rs:10411`). The current implementation
simply checks `_lumen_is_focusable(nid)` on the label itself, finds it
unfocusable, and returns without ever consulting `label.control`
(implemented separately by BUG-383's label/control graph, which is
correctly wired but never consulted here).

## Масштаб

1 file, 1 directly-attributable subtest in this category
(`inert-label-focus.html`'s first test). The file's other two subtests use
`test_driver.click()`/depend on the same delegation and would need
re-verification once fixed. Likely fix: at the top of
`HTMLElement.prototype.focus`, if `this.tagName === 'LABEL'` and
`this.control` is non-null, recurse into `this.control.focus(options)`
instead of operating on `this`.
