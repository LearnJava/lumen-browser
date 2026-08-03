# BUG-539: `'<prop>' in getComputedStyle(el)` always returns `false` — Proxy has no `has` trap, likely the dominant cause of "doesn't seem to be supported in the computed style" across the whole WPT-RUN-3 corpus

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs:12772` — `window.getComputedStyle`)
**Найден:** WPT-RUN-3 срез 28 (`ROADMAP.md`) — массовый прогон `css/css-inline`

## Механизм

`window.getComputedStyle` (`dom.rs:12772`) returns `new Proxy({}, handler)`
where `handler` defines only a `get` trap (`dom.rs:12775-12793`). No `has`
trap is defined. Per the JS spec, the `in` operator on a Proxy without a
`has` trap falls back to `Reflect.has(target, prop)` on the **underlying
target** — here an empty object literal `{}` — not through `get`. So
`'anything' in getComputedStyle(el)` is `false` for every single property,
including ones that `getPropertyValue`/bracket access resolve correctly.

Live probe (`--mcp-live-port`):

```
'line-height' in getComputedStyle(el)                        => false
getComputedStyle(el).getPropertyValue('line-height')          => "1.2"   (correct)
'display' in getComputedStyle(el)                             => false   (display is about as core as CSS gets)
```

`display` is fully implemented, present in `computed_style_to_map`
(`selector_query.rs:629`), and returned correctly by `getPropertyValue` — the
`in` check fails regardless of whitelist status. This is a structural gap in
the Proxy itself, not a per-property omission.

## Симптом

`css/support/computed-testcommon.js` and `css/support/inheritance-testcommon.js`
— the two shared helpers essentially every `css/*/computed-*.html` and
`css/*/inheritance.html` file in the whole corpus uses to check "is this
property surfaced in computed style" — gate on exactly this idiom
(`computed-testcommon.js:24,49,169`; `inheritance-testcommon.js:12,42`):

```js
assert_true(property in getComputedStyle(target), property + " doesn't seem to be supported in the computed style");
```

Every one of these assertions fails unconditionally, *before* the test can
even check whether the resolved value is correct — so a property that is
100% correctly implemented and exposed (like `display`) reads exactly the
same as one that's entirely missing. This slice alone (`css/css-inline`)
shows the pattern on `vertical-align`/`line-height`/`text-box`/
`initial-letter(s)`/`dominant-baseline`/`baseline-source`/`baseline-shift`/
`alignment-baseline` — a mix of implemented-but-unlisted
([BUG-537](BUG-537-OPEN.md)-class), genuinely-unimplemented
([BUG-538](BUG-538-OPEN.md)-class, SVG/CSS baseline properties are a further
distinct unimplemented cluster not yet filed), *and* fully-correct properties
that this bug alone breaks. **Every prior WPT-RUN-3 slice that reported
"`<prop>` doesn't seem to be supported in the computed style" findings should
be treated as gated by this bug first** — the property may well be correctly
implemented; re-triage after this is fixed before trusting any such finding's
attribution to a missing/incomplete property implementation.

## Как исправить (не входит в объём P2)

Add a `has` trap to the `handler` object in `window.getComputedStyle`
(`dom.rs:12775`) mirroring the `get` trap's logic: return `true` for the
fixed special-cased members (`getPropertyValue`, `length`, `item`,
`cssText`) plus any kebab-cased property name that
`computed_style_to_map`/`_lumen_get_computed_style` actually recognizes for
`nid` — needs a native query (`_lumen_computed_style_has(nid, prop)` or
exposing the full key set) since the JS side has no visibility into the
Rust-side whitelist today.

## Срез 29 (`css/css-scroll-snap` + `css/css-animations`, 2026-08-03)

Systemic-reach warning above confirmed across two more categories: ~190
subtests total via the same `"<prop> doesn't seem to be supported in the
computed style"` message — `animation-range`/`-start`/`-end` (94),
`animation-name`/`animation` (41), `scroll-padding`/`-block`/`-inline`/
`-top`/`-right`/`-bottom`/`-left` (all 8 logical/physical variants, 58),
`scroll-snap-type`/`scroll-snap-align` (17). Not re-attributing any of
these to a missing-property implementation without re-checking after this
lands — same caution the bug's own title states. `.ini` under
`tests/wpt/metadata/css/css-scroll-snap/` and
`tests/wpt/metadata/css/css-animations/`.

## Срез 30 (spread across 13 categories, 2026-08-03) — largest single-slice weight to date, ~4300 subtests

117 files across `css-color` (20, `color`/`opacity`/`background-color`/
`light-dark()` computed-style checks — `inheritance.html` alone: property
`color` 2 subtests, `opacity` 2, plus the category's own
`inheritance-testcommon.js`-driven files), `css-values` (7,
`object-position`/`scale`/`filter` computed values), `css-anchor-position`
(12, `position-area` — `position-area-computed.html` alone: 632 of its own
subtests, the single largest file this slice), `css-shapes` (14,
`shape-outside`/`clip-path`), `css-text-decor` (15), `css-align` (16),
`css-break` (7), `css-tables` (6), `css-writing-modes` (6), `filter-effects`
(6, `backdrop-filter`), `css-contain` (4), `css-conditional` (2),
`css-view-transitions` (2) — every file via the same
`"<prop> doesn't seem to be supported in the computed style"` message from
`computed-testcommon.js`/`inheritance-testcommon.js`'s shared `in
getComputedStyle(el)` guard. Systemic-reach warning from срез 29 holds:
none of these are re-attributed to a per-property missing-implementation
gap without re-checking after the `has` trap lands. `.ini` under each
category's own `tests/wpt/metadata/css/<category>/`.

## Срез 31 (`css-fonts`/`css-transforms`/`css-ui`/`css-text`/`css-flexbox`, 2026-08-03) — ~800 subtests, dozens of properties

Same pattern confirmed on five more categories, this time dominated by
`css-fonts` (`font`, `font-family`, `font-weight`, `font-style`,
`font-stretch`, `font-width`, `font-synthesis`, `font-feature-settings`,
`font-variant-numeric`/`-ligatures`/`-east-asian`, `font-variation-settings`,
`font-palette` — the single largest file, `variations/font-palette.html`,
309 subtests alone), `css-transforms` (`scale`, `rotate`, `translate`,
`transform-origin`, `perspective-origin`), `css-text` (`white-space`,
`text-wrap`, `text-transform`, `text-indent`, `text-fit`, `text-spacing`,
`letter-spacing`, `word-spacing`, `tab-size`, `hyphenate-limit-chars`,
`text-autospace`), `css-ui` (`cursor`, `caret-color`, `outline-style`), and
`css-flexbox` (`flex`, `flex-basis`, `flex-wrap`). Not re-attributed to a
per-property gap without re-checking after the `has` trap lands — same
caution as all prior slices. `.ini` under each category's own
`tests/wpt/metadata/css/<category>/`.
