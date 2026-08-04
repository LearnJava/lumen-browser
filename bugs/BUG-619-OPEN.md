# BUG-619: `inert` on a shadow host doesn't propagate to non-slotted shadow-tree children (ancestor walk never crosses the shadow-root→host boundary)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::_lumen_is_focusable`, ancestor loop at `dom.rs:10415-10419`, specifically `_lumen_get_parent` on line 10418)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`), replicating `inert-in-shadow-dom.html`'s
setup: `<div id="shadow-host" inert>` with `attachShadow({mode:'open'})`, a
`<slot>` appended to the shadow root (so the host's existing light-DOM
`<button id="button-1">` gets slotted), and a second `<button id="button-2">`
created and appended directly into the shadow root (never slotted):

```js
button1.focus(); document.activeElement === button1   // → false (correct: unfocusable)
button2.focus(); document.activeElement === button2   // → true  (WRONG: should be false)
```

`button-1` (slotted content, still a light-DOM child of `shadow-host` in
this engine's tree) correctly becomes unfocusable. `button-2` (an element
whose only parent is the shadow root itself) stays focusable even though
its host carries `inert`.

Matches two WPT tests in this category:
`inert-in-shadow-dom.html` ("inert on Shadow host affects content in
shadow" — the `#button-2` sub-assertion) and `inert-on-slots.html`
("inert inside ShadowRoot affects slotted content", same underlying
shadow-root-child case via a `<slot>` in the shadow tree).

## Причина

`_lumen_is_focusable` (`dom.rs:10411`) walks inertness up the ancestor
chain with a plain light-DOM parent pointer, not a flat-tree walk:

```js
var anc = nid;
for (var guard = 0; guard < 512 && anc !== null && anc !== undefined; guard++) {
    if (_lumen_has_attr(anc, 'inert')) return false;
    anc = _lumen_u2n(_lumen_get_parent(anc));
}
```

For `button-1` (slotted), `_lumen_get_parent` walks straight to
`shadow-host` because the element's actual DOM parent is still the host
(slotting is a rendering-time redirection here, not a re-parent), so the
`inert` check on line 10417 finds it directly — this path happens to work.

For `button-2`, `_lumen_get_parent` walks to the `ShadowRoot` node and then
returns `null`/`undefined` (a shadow root has no DOM parent in the light
tree), so the loop exits *without ever reaching `shadow-host`* and its
`inert` attribute is never consulted. Per HTML LS §6.7 / DOM shadow-tree
semantics, inertness must propagate along the *flat tree*, where a shadow
root's parent-for-this-purpose is its host — `_lumen_get_parent` has no
such shadow-boundary crossing.

## Масштаб

2 files, 2 subtests (`inert-in-shadow-dom.html`,
`inert-on-slots.html`) in this category. Likely a narrow, single-purpose
fix inside `_lumen_is_focusable`'s ancestor loop: when `_lumen_get_parent`
returns nothing but the current node is a `ShadowRoot`, continue the walk
from its host instead of stopping. Not investigated whether other
ancestor-walking code in the shim (e.g. any other §6.7-adjacent check,
composed-path building) has the same shadow-boundary gap — worth grepping
`_lumen_get_parent(` callers if picked up.
