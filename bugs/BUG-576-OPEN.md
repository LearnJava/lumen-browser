# BUG-576: `HTMLOptionsCollection.prototype.add()` missing (`select.options.add()`)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:14124-14128` —
`HTMLOptionsCollection.prototype` only gets `constructor` set, no `add`;
contrast `HTMLSelectElement.prototype.add` at `dom.rs:14401`, which does exist)
**Найден:** P2, WPT-VENDOR-html-semantics-forms, 2026-08-04

## Симптом

```
FAIL add method should add option elements correctly - selly.options.add is not a function
FAIL add method should add option groups correctly - selly.options.add is not a function
FAIL select.add() with an index should work when the target is inside an optgroup. - select.options.add is not a function
```

`select.options.add(element, before)` throws `TypeError: ... is not a
function` — `HTMLOptionsCollection` has no `add` method, even though the
select element's own `.add()` (a spec-mandated mirror of the same operation)
works fine.

## Причина

HTML LS §4.10.7 defines `add(element, before)` on **both**
`HTMLSelectElement` and `HTMLOptionsCollection` (the collection returned by
`select.options`) — they're meant to be interchangeable. `dom.rs:14401`
wires `HTMLSelectElement.prototype.add`, but the mirror method on
`HTMLOptionsCollection.prototype` (`dom.rs:14124-14128`, right after the
collection's constructor is set up) was never added. The existing
`HTMLSelectElement.prototype.add` implementation operates on a select `nid`
resolved via `_lumen_reflect_nid(this)`; `HTMLOptionsCollection` instances
are built by `_lumen_make_nid_collection` (`dom.rs:14329-14335`) and would
need the equivalent owning-select lookup — likely a thin delegation to the
same underlying logic once the collection instance's owning select is known
in the collection object, or an easy win by delegating to the existing
select's implementation if the collection retains a back-reference (needs
checking; not investigated further here, out of scope for a P2 WPT-survey
report).

## Масштаб

Small: **4 subtests in 2 files** (`the-select-element/common-HTMLOptionsCollection-add.html`,
`the-select-element/select-add-optgroup.html`). Narrow blast radius compared
to BUG-574/BUG-575 found in the same run, filed separately since it's a
distinct API surface (one specific missing method vs. a whole property/method
absent from the base `Node`/`Element` interfaces).
