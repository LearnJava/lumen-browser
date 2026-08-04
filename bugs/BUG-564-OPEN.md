# BUG-564: `document.fonts.ready` is missing — `FontFaceSet` has no `ready` property at all

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::_lumen_get_fonts` — the `fontSet` object
literal returned for `document.fonts` has `length`/`item`/`entries`/`forEach`/
`Symbol.iterator` but no `ready` property)
**Найден:** P2, WPT-RUN-3 срез 43 (`css/css-anchor-position`), 2026-08-04

## Симптом

Any script that awaits font loading before measuring layout —
`document.fonts.ready.then(() => …)` — throws synchronously:

```
script error: JS runtime error: Cannot read properties of undefined (reading 'then')
```

`document.fonts` itself exists (`typeof document.fonts === 'object'`), but
`document.fonts.ready` evaluates to `undefined` rather than a `Promise`, so
`.then` throws `TypeError`. When this line sits in the last `<script>` block of
a `testharness.js` file with `setup({explicit_done: true})` (or simply as the
only path that would ever call `done()`), the throw prevents `done()` from
ever being scheduled and the harness only completes via its own ~10s
(`--timeout-multiplier`-scaled since the WPT-RUN-3 срез 42 fix) default
file-level timeout — `harness_status: TIMEOUT` with whatever subtests
happened to register on
earlier, non-throwing script blocks (0, 1, or more depending on the file).

Confirmed by direct source read (`crates/js/src/dom.rs`, `_lumen_get_fonts`,
~line 6779) and live `--mcp-live-port` probe against
`css/css-anchor-position/anchor-getComputedStyle-002.html` and
`anchor-name-inline-001.html`: `typeof document.fonts.ready` is `"undefined"`.

## Причина

The CSS Font Loading Module (Level 3, `readonly attribute Promise<FontFaceSet>
ready`) requires `FontFaceSet.ready` — resolved once the set's pending font
loads settle, and already-resolved if there are none pending. Lumen's
`document.fonts` binding (`_lumen_get_fonts` in the JS shim) is a static
snapshot object built once per access (`_lumen_fonts_size`/`_lumen_fonts_get`
native calls, no live loading-state tracking) and simply never adds a `ready`
property to the returned object literal. Two sibling constructs in the same
file already use the exact fallback this needs —
`this.ready = Promise.resolve();` (`dom.rs:8805`) and
`this.ready = Promise.resolve(self);` (`dom.rs:15403`) — so the fix is a
one-line addition to the `fontSet` object literal, not new engine machinery;
Lumen doesn't track in-flight `@font-face` loads through this shim, so a
resolved-Promise stub is the correct minimum (matches upstream engines'
behavior once all fonts referenced by the document have already settled,
which is the state `document.fonts` snapshots at call time here).

## Масштаб

Found via WPT-RUN-3 srez 43's individual triage of `css-anchor-position`'s
remaining TIMEOUT cluster (31 files left unexplained after срез 42): explains
2 of 31 — `anchor-getComputedStyle-002.html` (1/2 subtests had already
registered before the throw) and `anchor-name-inline-001.html` (0 subtests —
the only script block is the throwing one). Not scoped further outside this
category; any test on any page waiting on `document.fonts.ready` will hit the
same throw.
