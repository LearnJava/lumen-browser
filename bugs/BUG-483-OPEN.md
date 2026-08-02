# BUG-483: `getComputedStyle()` Proxy has no `has` trap — `property in getComputedStyle(el)` always `false`

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:12772-12805` — `window.getComputedStyle`)
**Найден:** WPT-RUN-3 срез 5 (`ROADMAP.md`) — массовый прогон `css/css-box`

## Механизм

`window.getComputedStyle()` (`dom.rs:12772`) returns `new Proxy({}, handler)`
where `handler` defines only a `get` trap (`dom.rs:12776-12792`) — no `has`
trap. Per JS semantics, the `in` operator dispatches to the Proxy's `has`
trap, not `get`; with no `has` trap defined, it falls back to
`Reflect.has(target, prop)` against the empty literal `{}` passed as the
Proxy target. So `prop in getComputedStyle(el)` is **unconditionally
`false`** for every property name, regardless of whether `get`/
`getPropertyValue` would actually resolve it.

Confirmed empirically via `--mcp-live-port` live eval (element with inline
`margin-top: 10px`):

```
'margin-top' in getComputedStyle(el)                        → false   (wrong — should be true)
getComputedStyle(el).getPropertyValue('margin-top')          → "10px" (correct — get trap works)
'toString' in getComputedStyle(el)                           → true   (Reflect.has walks the
                                                                          prototype chain, so
                                                                          inherited names still
                                                                          pass — only own/expected
                                                                          CSS property names never do)
```

## Симптом

WPT's `css/support/computed-testcommon.js` — the shared helper behind
`test_computed_value()`/`test_pseudo_computed_value()` — feature-detects
support with exactly this idiom before comparing any value:

```js
assert_true(property in getComputedStyle(target), property + " doesn't seem to be supported in the computed style");
```

This assertion is the **first line** of every generated test, so it throws
immediately — the test never reaches the actual value comparison, even for
properties that are fully implemented and resolve correctly through
`getPropertyValue`/bracket access.

## Масштаб находки

**Universal, not css-box-specific**: `grep -rl computed-testcommon.js
tests/wpt/css --include=*.html | wc -l` → **456 files** in the vendored
`css/` tree alone (457 across the whole corpus) use this helper — every
future WPT-RUN-3 slice will hit this same false-negative wall on any
`*-computed.html`/`inheritance.html` test until it's fixed. This is the
single highest-leverage fix available to the WPT-RUN-3 track right now.

Within this slice (`css/css-box`), 7 files / 79 subtests fail **exclusively**
on this assertion (confirmed by reading each failing subtest's message — all
are the literal "doesn't seem to be supported" text, meaning no other defect
is masked underneath for these particular files): `clear-computed.html` (6),
`float-computed.html` (5), `margin-computed.html` (8),
`padding-computed.html` (13), `margin-trim-computed.html` (20),
`visibility-computed.html` (3), `inheritance.html` (24).

Note: fixing the `has` trap will unblock the *assertion*, not necessarily
make every one of these subtests pass — `margin` (bare shorthand) and
`margin-trim` are not in `computed_style_to_map`
([BUG-472](BUG-472-OPEN.md)'s known gap) and `margin-trim` has no parser
support at all, so those specific subtests will likely surface a second,
already-known or new failure after this fix. Re-triage after landing this
fix rather than assuming it clears the whole file.

## Что нужно

Add a `has` trap to the `handler` object in `dom.rs:12775`, delegating to
the same `_lumen_get_computed_style(nid, kebab-name)` lookup the `get` trap
already uses (i.e. `has: function(target, prop) { ... }` returning `true`
for any property key `_lumen_get_computed_style` resolves to a non-empty
value, plus the fixed set of always-present keys like `getPropertyValue`/
`length`/`item`/`cssText`).

## Second symptom found (WPT-RUN-3 срез 6, `css/css-cascade`, 2026-08-02)

The same stub Proxy also has no `Symbol.iterator`, and its `length`/`item`
are hardcoded (`length` always `0`, `item()` always returns `''` —
`dom.rs:12783-12784`) instead of reflecting the actual set of resolvable
properties. `for (let property of getComputedStyle(el)) { ... }` — the
standard CSSOM-style idiom for enumerating every computed property, used by
`all-prop-revert-layer-noop.html` and `all-prop-revert-noop.html` (16
variant ids total between them, `?include=0`…`7` each) — throws `TypeError:
<obj> is not iterable` synchronously at the top level of each file's
`<script>`, before any `test()` registers, so every variant reports
`TIMEOUT` (`TestRunner hit external timeout`) rather than a clean `FAIL`.
Same root artifact as the `has`-trap gap above (the Proxy target is a bare
`{}`, so it has none of `CSSStyleDeclaration.prototype`'s real shape), just
a different consumer pattern. A real fix — backing the Proxy (or replacing
it with a concrete object) with the property's actual resolved index/name
list — would close both gaps at once: give `length`/`item(i)`/
`Symbol.iterator` the real per-node property list, and derive `has` from
the same list.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-box/` for the 7
attributed files, `expected: FAIL` per the actual run. `tests/wpt/metadata/
css/css-cascade/` gets `expected: TIMEOUT` for the 16 `all-prop-revert-
{layer-,}noop.html?include=N` variants (WPT-RUN-3 срез 6).
