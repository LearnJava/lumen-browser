# BUG-534: CSS Custom Highlight API — `Highlight`/`HighlightRegistry` are ad-hoc Phase-0 stubs, not the spec's Setlike/Maplike interfaces

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/highlight_api.rs` — `HIGHLIGHT_API_SHIM`, installed via `install_highlight_api_bindings_v8`)
**Найден:** P2, WPT-RUN-3 срез 26 (`css/css-highlight-api`) — массовый прогон

## Симптом

`crates/js/src/highlight_api.rs` (landed as "A-2 CSS Custom Highlight API
Phase 0", already merged to `main` before ROADMAP.md/STATUS-PN.md existed —
no open task tracks a Phase 1) installs a hand-rolled JS shim:

```js
global.Highlight = function Highlight(...ranges) {
  this.priority = 0;
  this.ranges = ranges;
};
if (!global.CSS) global.CSS = {};
global.CSS.highlights = {
  set: function(name, highlight) { ... },
  get: function(name) { ... },
  has: function(name) { ... },
  delete: function(name) { ... },
  clear: function() { ... },
};
```

This is neither of the two interfaces the spec (`css-highlight-api-1`)
actually requires:

- **`Highlight` is not Setlike.** It stores raw `.ranges`/`.priority` fields
  with no `.size`, `.add()`, `.delete()`, `.has()`, `.clear()`, `.entries()`,
  `.keys()`, `.values()`, `.forEach()`, or `Symbol.iterator` — every one of
  those throws `"... is not a function"` or resolves to `undefined`. The
  constructor also silently drops arguments into `.ranges` (a plain array)
  rather than deduplicating/validating them as a real `Set` would. It also
  has no `.type` attribute at all (spec: mutable, defaults to `"highlight"`,
  drives which `HighlightType` enum member the highlight paints as) —
  reading `.type` on a fresh `Highlight` gives `undefined`.
- **`CSS.highlights` is not a `HighlightRegistry` instance.** There is no
  global `HighlightRegistry` constructor at all (`window.HighlightRegistry`
  is `undefined`), so `CSS.highlights instanceof HighlightRegistry` and any
  spec test asserting the constructor's existence fail immediately. The
  object itself only has `set/get/has/delete/clear` — no `.size`, no
  Maplike iteration (`keys/values/entries/forEach/Symbol.iterator`), and no
  `highlightsFromPoint()`.

Confirmed by reading `crates/js/src/highlight_api.rs` directly (no grep
ambiguity — the shim is 40 lines, fully quoted above) and cross-checked
against the failing subtest messages, all of which name the exact missing
member (`CSS.highlights.keys is not a function`,
`CSS.highlights[Symbol.iterator] is not a function`,
`HighlightRegistry is in window got disallowed value undefined`,
`Highlight starts empty expected (number) 0 but got (undefined) undefined`
— i.e. `.size` missing).

## Масштаб

6 files / 30 subtests in `css/css-highlight-api` this slice:
`Highlight-setlike.html` (1 — `.size`), `Highlight-type-attribute.tentative.html`
(1 — `.type`), `HighlightRegistry-iteration.html` (15 —
`keys`/`values`/`Symbol.iterator`/`entries`/`forEach`),
`HighlightRegistry-iteration-with-modifications.html` (6 —
`Symbol.iterator`), `HighlightRegistry-maplike.html` (2 — no global
`HighlightRegistry` + `.size`), `HighlightRegistry-highlightsFromPoint.html`
(5 of its 7 fails — `.highlightsFromPoint` missing; the other 2 are
[BUG-533](BUG-533-OPEN.md)'s `StaticRange` and [BUG-480](BUG-480-OPEN.md)'s
`iframe.contentWindow`, not this bug).

Several other files in this slice show `"StaticRange is not defined"` or
setlike-adjacent symptoms but their *sole* cause this slice is
[BUG-533](BUG-533-OPEN.md) (`Highlight-multiple-type-attribute.html`,
`Highlight-setlike-tampered-Set-prototype.html`,
`HighlightRegistry-highlightsFromPoint-ranges.html`, `highlight-priority.html`)
— kept off this bug's file count to avoid double-attributing the same
subtest to two bugs. Not scoped beyond `css-highlight-api` — this is the
only WPT category built around the Highlight Registry surface.

## Что нужно

Rewrite the shim (or move it to native bindings, matching the pattern used
for other collection-like DOM types) so that:

1. `Highlight` implements the Setlike interface per spec — backed by a real
   `Set`-like store, exposing `size`/`add`/`delete`/`has`/`clear`/`entries`/
   `keys`/`values`/`forEach`/`Symbol.iterator`, all operating over the
   `(Range | StaticRange)` collection (needs [BUG-533](BUG-533-OPEN.md)'s
   `StaticRange` first for full coverage, though the setlike protocol itself
   doesn't strictly require it).
2. A global `HighlightRegistry` constructor exists, and `CSS.highlights` is
   an actual instance of it (`instanceof` must hold) implementing the
   Maplike interface (`size`, `keys/values/entries/forEach/Symbol.iterator`
   alongside the existing `get/set/has/delete/clear`).
3. `CSS.highlights.highlightsFromPoint(x, y, options?)` is implemented,
   returning `HighlightHitResult` entries for highlights present at the
   given viewport point (needs hit-testing against painted highlight ranges,
   not just registry bookkeeping).

The existing Rust-side `HighlightRegistry`/`Highlight` structs in
`highlight_api.rs` (used only by the crate's own unit tests today, never
consulted by the JS shim or by paint) could become the real backing store if
wired through native bindings instead of the current pure-JS closure-based
shim — worth checking during the fix whether paint (custom highlight
rendering) already depends on one representation over the other.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-highlight-api/` for the 6
affected files, `expected: FAIL` per actual subtest.
