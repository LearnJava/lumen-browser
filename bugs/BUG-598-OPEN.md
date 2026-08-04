# BUG-598: `DataTransfer.types` returns a fresh array on every access instead of a cached `FrozenArray`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:737-739`, `DataTransfer.prototype.types` getter)
**Найден:** P2, WPT-VENDOR-html-editing, 2026-08-04

## Симптом

```
FAIL type's identity - assert_equals: expected ["text/plain"] but got ["text/plain"]
FAIL Relationship between types and items - assert_equals: expected ["text/plain"] but got ["text/plain"]
```
(`dnd/datastore/datatransfer-types.html` -- `assert_equals(dt.types, dt.types)`
using `testharness.js`'s `SameValue`-based comparison, so two visually
identical arrays still fail unless they're the same reference)

## Причина

```js
Object.defineProperty(DataTransfer.prototype, 'types', {
    get: function() { return Object.freeze(this._types.slice()); }
});
```
HTML LS's `DataTransfer.types` is declared `readonly attribute
FrozenArray<DOMString> types` -- by WebIDL's `FrozenArray<T>` semantics, the
getter must return the *same* frozen array object across repeated calls as
long as nothing in the underlying data store list changed, and only produce
a *new* one when an item is added/removed/cleared. `.slice()` re-allocates a
new array on every single access, so `dt.types === dt.types` is always
`false` -- the "same reference until the store changes" contract is entirely
absent; only the "frozen" and "correct contents" parts hold.

## Масштаб

Two subtests in `datatransfer-types.html` fail directly. Any code depending
on `dt.types` being cacheable (e.g. comparing before/after a mutation via
reference equality, as this very WPT file's other passing subtests already
do for the *contents*-changed case) is affected; contents-correctness itself
is not in question, only identity/caching.
