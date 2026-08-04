# BUG-626: `IntersectionObserver` constructor and `observe()` perform no argument validation — invalid `threshold`/`rootMargin` are silently accepted, `observe(nonElement)` silently no-ops

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:7180-7193` — `IntersectionObserver`
constructor and `.prototype.observe`)
**Найден:** P2, WPT-VENDOR-intersection-observer, 2026-08-05

## Симптом

Confirmed live (`--mcp-live-port`, `eval`) — all four should throw per spec,
none do:

```js
new IntersectionObserver(function(){}, {threshold: [1.1]});     // should throw RangeError (out of [0,1])
new IntersectionObserver(function(){}, {threshold: ["foo"]});   // should throw TypeError (not a double)
new IntersectionObserver(function(){}, {rootMargin: "2em"});    // should throw SyntaxError (only px/% allowed)
var o = new IntersectionObserver(function(){}, {});
o.observe("foo");                                                // should throw TypeError (not an Element)
```

`dom.rs:7180-7185` stores `options` verbatim with no validation at all.
`dom.rs:7186-7193` (`observe`) checks `target.__nid__ === undefined` and
silently `return`s instead of throwing — the WPT test's own comment
(`observer-exceptions.html`) documents the spec-required
`TypeError`/`RangeError`/`SyntaxError` outcomes for each case.

## Масштаб

Reproduced by `intersection-observer/observer-exceptions.html`: all 9
subtests FAIL (`assert_throws_js`/`assert_throws_dom` all report "no
exception thrown" instead of the constructor/`observe()` call actually
throwing). Independent of BUG-628/BUG-627 — this is a missing-validation
gap, not a missing-getter or missing-root-support gap, though it lives in
the same constructor/`observe()` code.

## Fix shape

- Constructor: validate `options.threshold` — if array, every element must
  be a finite number in `[0, 1]` (else `RangeError`) and non-numeric
  entries must throw `TypeError` before the `RangeError` check (per WPT's
  ordering, non-numeric first).
- Validate `options.rootMargin` against the CSS `<length-percentage>`
  grammar restricted to `px`/`%` units, exactly 1-4 components, no
  `calc()`/`!important` — else `DOMException("SYNTAX_ERR")`.
- `observe(target)`: throw `TypeError` when `target` is not an `Element`
  (i.e. not `target instanceof Element` in spec terms — here, no
  `__nid__`/not an element-shaped object) instead of the current silent
  `return`.
