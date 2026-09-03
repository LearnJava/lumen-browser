# BUG-966: `Selection.setBaseAndExtent()` doesn't apply the cross-tree-scope boundary adjustment when anchor and focus are in different node trees

**Статус:** OPEN
**Дата:** 2026-09-03
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js::setBaseAndExtent` /
`crates/js/src/v8_runtime/install/dom_core.rs::_lumen_set_selection`)
**Найден:** P2, WPT-RUN-6 срез 53, живой пробой

## Механизм

`setBaseAndExtent(aN, aO, fN, fO)` (`web_api_shim_mid.js:7564`) does exactly
one check — that both nodes carry a `__nid__` — then forwards the two raw
node ids straight to the native `_lumen_set_selection` binding
(`dom_core.rs:1013-1034`), which stores `anchor`/`focus` verbatim with no
validation of which node tree either belongs to:

```js
setBaseAndExtent: function(aN, aO, fN, fO) {
    if (!aN || aN.__nid__ === undefined || !fN || fN.__nid__ === undefined) return;
    _lumen_set_selection(aN.__nid__, aO >>> 0, fN.__nid__, fO >>> 0);
},
```

The Selection API spec requires a UA to normalize the boundary points when
`anchorNode` and `focusNode` don't share the same node tree (e.g. one is in
the light DOM, the other inside a shadow tree) — per real-browser behaviour
(referenced by the WPT test as Mozilla bug 1887963), the selection collapses
to the anchor's own tree rather than keeping a `focusNode` whose root differs
from `anchorNode`'s. Lumen has no such step anywhere in the call chain: given
`anchorNode = b` (a light-DOM `<video>`) and `focusNode = c` (a `ShadowRoot`
attached to a sibling `<div>`), `sel.focusNode` reads back as the raw shadow
root instead of being adjusted to `b`.

## Симптом

Confirmed live (`--mcp-live-port`, `.tmp/s53-diag.html`, 2026-09-03,
`main` = `87c97af64`):

```
setBaseAndExtent = ok
anchorNode = ===b            (correct)
focusNode  = #shadow-root    (should also resolve to b — WRONG)
```

Real-world trigger: `tests/wpt/selection/selection-nested-video.html`. Its
`DOMContentLoaded` listener is a bare arrow function, **not** wrapped in
`t.step(...)`, so the thrown `AssertionError` from
`assert_equals(sel.focusNode, b)` propagates as an uncaught exception instead
of a normal `FAIL`. `add_completion_callback` reports the harness as
`status=1` (ERROR) with the sole subtest stuck at status `2` (TIMEOUT,
`t.done()` is never reached) — exactly the shape recorded for this id in the
WPT-RUN-5 corpus snapshot. Stack trace (`--mcp-live-port` stderr):

```
[JS error] Uncaught Error
    at get_stack (<anonymous>:4802:21)
    at new AssertionError (<anonymous>:4795:22)
    at assert (<anonymous>:4779:19)
    at assert_equals (<anonymous>:1598:9)
    at assert_wrapper (<anonymous>:1518:30)
    at Object.<anonymous> (<anonymous>:11:5)
    at Object.dispatchEvent (...)
    at _lumen_apply_ready_state (...)
```

## Масштаб

Any script relying on `Selection.setBaseAndExtent`/cross-shadow-tree
selection semantics gets a `focusNode` (or `anchorNode`, symmetrically) whose
root doesn't match spec — not observed outside this one WPT id so far (no
other committed script exercises cross-tree selection).

## Что нужно

Add the tree-scope check to either the JS shim or `_lumen_set_selection`:
compare `anchor.container`'s root to `focus.container`'s root
(`crates/engine/dom` already exposes a root-of-node walk used elsewhere,
e.g. `Node.getRootNode()`'s implementation) and collapse the boundary point
that's out of tree to the other one's position, per Selection API §4.3.

## Классификация WPT-RUN-6

Attributed via `_exact_id_marker("/selection/selection-nested-video.html")`
in `tests/wpt/timeout_audit.py` (marker `selection-cross-tree-scope-focus`).
