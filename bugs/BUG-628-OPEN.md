# BUG-628: `IntersectionObserver.prototype.takeRecords()` missing entirely; `root`/`rootMargin`/`thresholds` are not exposed as IDL attributes at all

**Renumbered 2026-08-05** from `BUG-625` — collided with another parallel
session's `BUG-625` (chrome font measurer, already pushed to `origin/main`
by P3's BUG-128 branch), resolved while merging this branch back into
`main`.

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7174-7276` — `IntersectionObserver` shim)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`):

```js
typeof IntersectionObserver.prototype.takeRecords   // "undefined"
var o = new IntersectionObserver(function(){});
typeof o.takeRecords                                // "undefined"
o.root                                               // undefined
o.rootMargin                                         // undefined
o.thresholds                                         // undefined
```

`crates/js/src/dom.rs:7180-7201` defines only the constructor plus
`observe`/`unobserve`/`disconnect`. There is no `takeRecords` method and
no getters for `root`/`rootMargin`/`thresholds` — the constructor stores
the raw options object as `this._options` (`dom.rs:7182`) and never
surfaces it back through the spec's read-only IDL attributes.

## Масштаб

This is the single dominant cause of failure in the `intersection-observer`
category: **68 of 143 test files** call `observer.takeRecords()` as a
matter of course (most WPT `intersection-observer` tests concat pending
records via `entries = entries.concat(observer.takeRecords())` right after
`observe()`, to also cover any synchronous-delivery edge case) and get a
`TypeError: observer.takeRecords is not a function`, aborting the test
before it reaches its actual assertions. A further handful
(`observer-attributes.html` and friends) directly assert on
`observer.root`/`.rootMargin`/`.thresholds` and get `undefined` instead of
the constructed values (e.g. `rootMargin` should default to `"0px 0px 0px
0px"`, `thresholds` to `[0]`).

`CAPABILITIES.md:152` lists `IntersectionObserver` under "Observers/Timing
— ✅" — same drift class as BUG-368 (`innerHTML`): the constructor/
`observe`/callback-delivery path works well enough to pass hand-written
smoke tests (`dom.rs:21114-21330`), but roughly half of the spec's IDL
surface (`takeRecords`, three of five read-only attributes) is absent.

## Fix shape

- `takeRecords()`: return and clear a per-observer pending-records queue.
  The current delivery path (`_lumen_deliver_intersection_observers`,
  `dom.rs:7217-7276`) calls `obs._cb(entries, obs)` directly and never
  retains `entries` afterward — needs to also push to
  `this._records` (or similar) so `takeRecords()` can drain it, and the
  callback path should likewise drain-then-deliver so records aren't
  double-reported.
- `root`/`rootMargin`/`thresholds` getters: trivial — surface
  `this._options.root ?? null`, the normalized/serialized `rootMargin`
  string (defaulting to `"0px 0px 0px 0px"`), and
  `Array.isArray(this._options.threshold) ? this._options.threshold.slice()
  : [this._options.threshold ?? 0]` respectively. Ideally computed once in
  the constructor and stored, not recomputed per access.

See also BUG-626 (constructor/`observe()` perform no argument validation)
and BUG-627 (`root` option is accepted but silently ignored) — filed from
the same run, all three symptoms of the same underlying "IntersectionObserver
is a partial Phase-0 stub" gap.
