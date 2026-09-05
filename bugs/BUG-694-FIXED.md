# BUG-694 — `URLSearchParams` is not iterable and leaks its internal `_p` array through the copy-constructor

**Статус:** FIXED 2026-09-05
**Компонент:** js (`crates/js/src/shim/url_shim.js` — `URLSearchParams` constructor + prototype; was `crates/js/src/dom.rs:7974-8073` at filing time, moved to the shim split before this fix)
**Найден:** P2, WPT-VENDOR-url, 2026-08-09
**Исправлен:** P3, ветка `p3-bug694-urlsearchparams-iterable`, 2026-09-05

## Симптом

Same run as [BUG-693](BUG-693-OPEN.md) (`tests/wpt/url/`, `run_report.py
--all --root url --recursive`, 44 id, 39/44 harness OK). The `URLSearchParams`-
focused files show a second, independent defect class from the URL parser
itself:

- `urlencoded-parser.any.html` — **0/105** (100% failure)
- `urlsearchparams-sort.any.html` — **0/17** (100% failure)
- `urlsearchparams-foreach.any.html` — **1/6**
- `urlsearchparams-delete.any.html` — **2/8**
- `urlsearchparams-constructor.any.html` — **14/27**
- `urlsearchparams-has.any.html` — **2/8**

Dominant message across all of these: `"sp is not iterable"` /
`"searchParams is not iterable"` / `"params is not iterable"` — every test
that does `for (const [k, v] of sp)` or `[...sp]` throws immediately, which
is why `urlencoded-parser` (a spec-conformance suite that round-trips
`URLSearchParams` through iteration) is 0/105 outright rather than partially
failing.

A second, distinct symptom in `urlsearchparams-constructor.any.html`:

```
Basic URLSearchParams construction
  assert_equals: expected "a=b" but got "_p=a%2Cb"
```

`new URLSearchParams(existingSearchParams)` (the copy-constructor form)
serializes to a single pair literally named `_p`, its value the
comma-joined array contents percent-encoded.

## Причина

`URLSearchParams.prototype` (`dom.rs:8010-8073`) defines `append`/`delete`/
`get`/`getAll`/`has`/`set`/`sort`/`toString`/`forEach`/`keys`/`values`/
`entries`/`size`, but:

1. **No `Symbol.iterator`** is defined anywhere on the prototype, so the
   object itself is not iterable at all — `for...of`, spread, and
   `Array.from()` all fail or (worse) silently return `[]` depending on the
   call site, per the same defect class already on record for `Headers`
   ([BUG-369](BUG-369-FIXED.md)) and other collection-like objects
   ([BUG-367](BUG-367-FIXED.md)).
2. **`entries()`/`keys()`/`values()` return plain arrays**, not
   `%ArrayIteratorPrototype%`-shaped iterator objects (no `.next()`), so
   `sp.entries().next` is `undefined` — code written per spec that manually
   drives the iterator (rather than using `for...of`) breaks differently
   from #1.
3. **The constructor's object-form branch (`dom.rs:7996-8000`,
   `typeof init === 'object'`) does `Object.keys(init)`.** When `init` is
   itself a `URLSearchParams` instance (the copy-constructor overload,
   `new URLSearchParams(existingParams)`), its only enumerable own property
   is the internal storage field `_p` — `this._p = []` at construction
   (`dom.rs:7975`) is a plain assignment, never hidden via
   `Object.defineProperty(..., {enumerable: false})`. `Object.keys(sp)` on
   an existing instance therefore returns `['_p']`, and the constructor
   dutifully copies `{_p: init._p}` as if it were a single query parameter
   named `_p` whose value is the raw backing array — exactly the
   `_p=a%2Cb` result observed. Same internal-field-leak pattern as
   `Headers._map` in [BUG-369](BUG-369-FIXED.md).
4. **`delete(name, value)` and `has(name, value)`'s two-argument overload
   (added to the URL Standard alongside the rest of the setlike-adjacent
   API) is not implemented** — both prototype methods take only `name` —
   confirmed by `urlsearchparams-delete.any.html`'s "Two-argument delete()"
   failures (`expected "a=b&a=d" but got ""` — the second arg is ignored,
   so `delete('a', 'c')` deletes every `a` pair instead of only the one
   matching both name and value).

## Масштаб

Every consumer of `URLSearchParams` that iterates it (directly or via
spread/`Array.from`) is affected, not just this category's own tests —
`urlencoded-parser.any.html` demonstrates the failure mode is total (0/105)
once iteration is on the hot path. The copy-constructor leak (#3) additionally
means `new URL(x).searchParams` piped into `new URLSearchParams(...)`
anywhere in page script silently produces a single bogus `_p` parameter
instead of a copy — a correctness bug reachable from ordinary application
code, not just WPT.

## Дальше

Add `URLSearchParams.prototype[Symbol.iterator] = URLSearchParams.prototype.entries`
(post-fix for #2, once `entries()` returns real iterator objects, not
arrays); make `_p` non-enumerable (`Object.defineProperty` in the
constructor, mirroring whatever fix lands for `Headers._map` under
BUG-369 — same pattern, worth fixing together); add the second parameter
to `delete`/`has`. Independent of [BUG-693](BUG-693-OPEN.md) (the URL
*parsing* engine) — this bug is scoped to the `URLSearchParams` object's
own WebIDL shape and does not require the parser fix to land first.

## Исправление (2026-09-05)

Point #3 (the `_p=a%2Cb` leak) had already been closed as a side effect of
[BUG-375](BUG-375-FIXED.md): `_p`/`_url` are defined via
`Object.defineProperty(..., {enumerable: false})` in the constructor, so
`Object.keys(existingParams)` returns `[]`, not `['_p']` — the exact
`"_p=a%2Cb"` string from the original report no longer reproduces. What
remained live was #1, #2 and #4, plus a *new* symptom of the same root cause:
since `Object.keys()` on an existing `URLSearchParams` now returns nothing,
`new URLSearchParams(existingParams)` silently produced an **empty** result
instead of a copy — same defect (init not recognized as a source of pairs),
different manifestation.

`crates/js/src/shim/url_shim.js`:

1. **Iterability.** `URLSearchParams.prototype[Symbol.iterator]` is the very
   same function object as `entries()` (WebIDL `iterable<>`, not a copy) —
   `for (const [k, v] of sp)` and `[...sp]` work.
2. **Real iterators.** `entries()`/`keys()`/`values()` return an object
   created from a shared `_USP_IterProto` with `next()` and
   `[Symbol.iterator]() === this`, `Symbol.toStringTag = 'URLSearchParams
   Iterator'` — the same pattern already used for `Headers` under BUG-369.
3. **Copy-constructor.** The constructor's `Array.isArray(init)` branch was
   generalized to "any object exposing `Symbol.iterator`" (mirroring
   `Headers`' `fill()`), read via `Array.from(init)` and, for each entry,
   `Array.from(entry)`. Because `URLSearchParams` instances are now iterable
   (point 1), `new URLSearchParams(existingParams)` falls into this branch
   automatically and copies the pairs — no `instanceof` special case needed,
   the same mechanism also covers `Map`/any other pairs-iterable passed as
   `init`.
4. **Two-argument `delete`/`has`.** Both check `arguments.length > 1`; when a
   second argument is present, only the pair matching both `name` and
   `String(value)` is removed/matched.

`URLSearchParams.prototype.size` and the string/record `init` forms are
unaffected. Not part of this fix: making the prototype methods themselves
non-enumerable (unlike `Headers`, `URLSearchParams`'s methods were already
plain enumerable assignments before this bug and stay that way — out of this
bug's scope, no WPT failure in the record depends on it).

8 new unit tests in `crates/js/src/dom/tests/v8_nav_url_storage.rs`
(`usp_is_iterable`, `usp_symbol_iterator_is_entries`,
`usp_entries_returns_real_iterator`, `usp_copy_constructor`,
`usp_delete_two_arg`, `usp_has_two_arg`, plus the two-argument success paths).
`cargo test -p lumen-js --features v8-backend` — all `usp_*` tests pass; the
only red test in the crate is the pre-existing, unrelated
[BUG-997](BUG-997-OPEN.md). `cargo clippy -p lumen-js --all-targets
--features v8-backend -- -D warnings` clean.
