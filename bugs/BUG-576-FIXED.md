# BUG-576: `HTMLOptionsCollection.prototype.add()` missing (`select.options.add()`)

**Статус:** FIXED
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` —
`HTMLOptionsCollection.prototype` only got `constructor` set, no `add`;
contrast `HTMLSelectElement.prototype.add` in the same file, which does exist)
**Найден:** P2, WPT-VENDOR-html-semantics-forms, 2026-08-04
**Исправлен:** P3, 2026-09-15

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

## Исправлено

Added `HTMLOptionsCollection.prototype.add` in
`crates/js/src/shim/web_api_shim_tail_b.js`, delegating to the existing
`HTMLSelectElement.prototype.add` instead of re-implementing the insertion
logic. The open question in "Причина" above — how the collection finds its
owning `<select>` — is answered by threading an owner nid through
`_lumen_make_nid_collection` (`web_api_shim_mid.js`): a new optional 5th
argument stores it on the collection's proxy target under a module-level
`Symbol` key (`_LUMEN_COLLECTION_OWNER_NID`), so it never collides with an
indexed or named collection member. `HTMLSelectElement.prototype.options`
now passes its own nid when building the collection; `add` reads it back
and calls `HTMLSelectElement.prototype.add.call(_lumen_make_element(sel),
element, before)`. The symbol approach also means the owner survives even
when `select.options` is read before any `<option>` exists yet, unlike a
scheme that infers the owner from the collection's own members.

New test file `crates/js/tests/cases/bug576_options_collection_add.rs`
(6 tests): `add` exists as a function on the collection; appends when
`before` is omitted; inserts before a numeric index; inserts before an
`<option>` reference; works on a collection captured before any option
existed; and adds `<optgroup>` children through the same call, matching
`HTMLSelectElement.prototype.add`'s existing handling.

`cargo test -p lumen-js --features v8-backend --test all` green (127/127,
was 121), `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean.
