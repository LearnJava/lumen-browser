# BUG-533: `StaticRange` constructor is entirely missing

**Статус:** FIXED 2026-09-13 (P3)
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — live `Range` global at `~6958`, no `StaticRange` counterpart on either engine)
**Найден:** P2, WPT-RUN-3 срез 26 (`css/css-highlight-api`) — массовый прогон

## Симптом

```
grep -n "\"Range\"\|global\.Range\|function Range" crates/js/src/dom.rs
# dom.rs:6958: function Range() { return _lumen_make_range(0, 0, 0, 0); }
grep -n "StaticRange" crates/js/src/dom.rs crates/js/src/v8_runtime.rs
# ноль совпадений
```

`Range` (live, mutation-tracking) is implemented; `StaticRange` (DOM §5.4, an
immutable snapshot of a start/end boundary pair — the type the CSS Custom
Highlight API's `Highlight` constructor is specified to accept alongside
`Range`) is not defined at all on either engine. `new StaticRange({...})`
throws `ReferenceError: StaticRange is not defined`.

## Масштаб

`grep -rl "StaticRange" tests/wpt/css --include=*.html --include=*.js` → **30
files** in the vendored `css/` tree alone (dominated by `css-highlight-api`,
which is specified entirely around mixed `Range`/`StaticRange` collections).

Two distinct failure shapes depending on where the throw lands relative to
`testharness.js`'s `test()` registration — the same TIMEOUT-vs-FAIL split
already documented for [BUG-485](BUG-485-FIXED.md) (`document.head`):

- **TIMEOUT** — `new StaticRange(...)` sits at the **top level** of the
  `<script>`, before the first `test()`/`promise_test()` call registers, so
  the harness never calls `done()`: `Highlight-iteration.html`,
  `Highlight-iteration-with-modifications.html`.
- **FAIL** (throw survives inside a `test()` callback) —
  `Highlight-multiple-type-attribute.html`,
  `Highlight-setlike-tampered-Set-prototype.html`,
  `HighlightRegistry-highlightsFromPoint-ranges.html`, `highlight-priority.html`,
  and 1 of `HighlightRegistry-highlightsFromPoint.html`'s 7 fails (the other
  6 are [BUG-534](BUG-534-FIXED.md)/[BUG-480](BUG-480-OPEN.md)). Note
  `Highlight-setlike.html`'s single fail is a *different* symptom
  (`.size` missing) — that one is [BUG-534](BUG-534-FIXED.md) only, not this
  bug, despite the similar file name.
- **TIMEOUT via corrupted harness internals** —
  `HighlightRegistry-maplike-tampered-Map-prototype.html` deliberately
  freezes a tampered `Map.prototype` (`delete Map.prototype.size`, `Object.freeze`)
  inside its single `test()`, then constructs
  `new Highlight(new StaticRange({...}))` a few lines later, *before* the
  test's own `restoreMapPrototype()` cleanup runs. The `StaticRange` throw
  aborts the test callback with `Map.prototype` still permanently frozen for
  the rest of the file's execution — `testharness.js`'s own internal
  bookkeeping apparently depends on `Map`, so the harness itself locks up
  rather than cleanly reporting the one `test()` as FAIL. Confirmed by
  reading the file: the tamper call precedes the `StaticRange` construction
  and the `restoreMapPrototype()` cleanup follows it in the same callback
  with no `add_cleanup`.

## Что нужно

Add a `StaticRange` global constructor mirroring `Range`'s shape (DOM §5.4:
takes a `StaticRangeInit` dictionary — `startContainer`/`startOffset`/
`endContainer`/`endOffset` — and exposes the same read-only
`startContainer`/`startOffset`/`endContainer`/`endOffset`/`collapsed`/
`commonAncestorContainer` surface as `AbstractRange`, but without `Range`'s
live-tracking mutation methods). No native binding needed beyond storing the
four snapshot fields — unlike `Range`, a `StaticRange` does not need to
follow DOM mutations.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-highlight-api/` for the
files above; `expected: TIMEOUT`/`FAIL` per actual run.

## Срез P3 2026-09-13 (закрытие)

The row's status had been flipped to `FIXED 2026-09-11` at some earlier point
with no substantiating detail added and no matching commit (`git log --all
-- crates/ | grep -i static.range` — zero hits); `grep -rn "StaticRange"
crates/` on `HEAD` confirmed the constructor still did not exist anywhere.
Discovered while writing regression tests for
[BUG-534](BUG-534-FIXED.md) — its Setlike test files construct
`new StaticRange(...)`, which threw `ReferenceError`.

Implemented in `crates/js/src/shim/web_api_shim_mid.js`, immediately after
`_lumen_make_range`/`Range` (same file, same nid-addressing scheme
`node.__nid__`). Per DOM §5.4/`AbstractRange`, the exposed surface is
`startContainer`/`startOffset`/`endContainer`/`endOffset`/`collapsed` only —
`commonAncestorContainer` belongs to `Range`, not `AbstractRange`, so this
card's original "Что нужно" section overstated the required surface by one
member. The boundary pair is captured once at construction
(`Object.defineProperty(this, '__start_nid__', ...)`, non-writable) and never
rewritten afterwards, matching the spec's "static" semantics — contrast
`Range`, whose native mutation ops rewrite `__start_nid__`/`__end_nid__` in
place.

Verified via 22 end-to-end tests over the real V8 shim
(`crates/js/tests/cases/bug534_highlight_api.rs`) that construct
`StaticRange` instances and feed them through `Highlight`/`HighlightRegistry`,
including the two `*-tampered-*-prototype.html` scenarios this bug's own
symptom section flagged as blocked. `cargo test -p lumen-js --features
v8-backend` green (3632/3632, one pre-existing unrelated flake under
`--test-threads` >1 — `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty`,
passes in isolation). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. Live WPT run not performed (no working
`.venv`/`wss` patch in this slot) — confidence rests on the transcribed unit
tests, not a real wptrunner pass. `.ini` dropped for
`Highlight-iteration.html`, `Highlight-iteration-with-modifications.html`,
`Highlight-multiple-type-attribute.html`, `highlight-priority.html`, and (as
part of BUG-534's slice) five more `css-highlight-api` files whose only
listed failure was this constructor;
`HighlightRegistry-highlightsFromPoint-ranges.html.ini` kept, re-attributed
to BUG-534's remaining `highlightsFromPoint()` hit-testing gap.
