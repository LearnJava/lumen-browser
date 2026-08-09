# BUG-505: CSS Overflow Level 3-5 properties and pseudo-elements essentially
unimplemented (`text-overflow`, `line-clamp`/`-webkit-line-clamp`/`max-lines`,
`block-ellipsis`/`continue`, `overflow-clip-margin` edge cases,
`scroll-axis-lock`, `scrollbar-gutter` layout effect, `-webkit-box`,
`::scroll-marker`/`::scroll-marker-group`/`::scroll-button()`,
`scroll-target-group`)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (`crates/engine/layout/src/style.rs::apply_declaration`,
selector matching for `::scroll-marker`/`::scroll-button`)
**Найден:** WPT-RUN-3 срез 11 (`ROADMAP.md`) — массовый прогон `css/css-overflow`

## Механизм

Two distinct symptoms, same underlying cause (no parser/layout support for
this whole cluster of Overflow L3-L5 features):

**1. Longhand properties accept any string with no validation and don't
reach `getComputedStyle()`.** `e.style['text-overflow'] = 'auto'` (an invalid
value per spec) is accepted verbatim instead of being rejected — the same
"unknown property falls through to a raw passthrough setter" shape as
[BUG-484](BUG-484-OPEN.md), but here the property itself
(`text-overflow`, `line-clamp`, `-webkit-line-clamp`, `max-lines`,
`block-ellipsis`, `continue`, `scroll-axis-lock`, `scroll-marker-group`,
`scroll-target-group`) has **no** parser arm at all, not even a validating
one, confirmed by `grep -rni` across `crates/css-parser/src/` and
`crates/engine/layout/src/style.rs` — zero hits for any of these names other
than `overflow`/`overflow-x`/`overflow-y` (which *are* implemented) and
`overflow-clip-margin` (partially — canonical serialization order and the
computed-style path are both missing). `getComputedStyle(el).textOverflow`
etc. consequently reads `""`/`undefined` for all of them
(`*-computed.html` files).

**2. `::scroll-marker`, `::scroll-marker-group`, `::scroll-button()` are not
recognized pseudo-elements/pseudo-classes at all.**
`querySelector('::scroll-button')` doesn't throw `SyntaxError` for the
spec-invalid forms (no selector-syntax validation for this family), and
`document.querySelectorAll('::scroll-button(up)')` on the *valid* form throws
`Cannot read properties of undefined (reading 'append')` — the selector isn't
recognized as a pseudo-element at all, so whatever downstream code expects an
element handle gets `undefined`. Since the pseudo-elements never materialize,
every test that clicks/hovers/focuses/queries a `::scroll-marker` or
`::scroll-button` box, or reads `scroll-marker-group`/`scroll-target-group`
computed values, fails or (when the test also awaits a `MutationObserver`/
focus-change that can now never happen) hangs to a harness-level `TIMEOUT`
with zero registered subtests, the same externally-visible shape as
[BUG-360](BUG-360-FIXED.md) but a different mechanism — no missing pseudo-box
ever fires the awaited condition.

`-webkit-box` (legacy flexbox `display` value, `webkit-box-computed.html`)
and `overflow`'s shorthand computed-style path (`overflow-computed.html`,
`overflow-shorthand-001.html`) are separately confirmed missing the same way.

## Масштаб находки

60 files. Representative groups:

- **Property parsing (`-invalid.html`/`-valid.html`/`-computed.html`, no
  validation + absent from computed style):** `parsing/text-overflow-{invalid,
  computed}.html`, `parsing/line-clamp-{invalid,valid}.html`,
  `parsing/webkit-line-clamp-invalid.html`, `parsing/max-lines-{invalid,
  valid}.html`, `parsing/block-ellipsis-invalid.html`,
  `parsing/continue-invalid.html`, `parsing/scroll-axis-lock-{invalid,
  computed}.html`, `parsing/overflow-clip-margin{,-computed}.html`,
  `parsing/overflow-{invalid,valid,computed}.html`,
  `parsing/scrollbar-gutter-{invalid,valid}.html`,
  `parsing/webkit-box-computed.html`, `inheritance.html` (block-ellipsis),
  `overflow-no-interpolation.html`, `overflow-shorthand-001.html`,
  `logical-overflow-001.html` (`overflow-inline`/`overflow-block` logical
  mapping).
- **`::scroll-marker`/`::scroll-button`/`scroll-target-group` selector +
  pseudo-box:** `parsing/scroll-buttons-{invalid,valid}.html`,
  `parsing/scroll-markers-{invalid,computed}{,.tentative}.html`,
  `parsing/scroll-target-group-{invalid,computed}.html`,
  `parsing/getComputedStyle-scroll-button.html`, `scroll-marker-group-
  {hover,hover-from-marker,display-none}.html`, and 33 files under
  `scroll-markers/` (event targeting, disposal, focus/hover, activation,
  container-query interaction, iframe interaction — `scroll-marker-15.html`,
  `scroll-marker-{event-target,group-{003,012,014},multiple-activation,
  navigation-cycles,target-before-after}.html`,
  `scroll-target-group-{013,014,iframe}.html`,
  `scroll-button-{event-target,disposed-event-target,and-scroll-marker-not-
  in-event-path,buttons-selection}.html`, `scroll-marker-controls-scroll-
  tracking-{001,002,003}.html`, `scroll-pseudo-elements-gcs-cq.html`,
  `html-scroll-marker-target-before-after.html`, `root-scroll-marker-
  activation-in-iframe.html`, `scroll-markers-updated-by-programmatic-
  scroll.html`, `scroll-marker-hover-logical.html`,
  `scroll-marker-group-size-container-query-root.html`.

## Что нужно

1. Add parser arms in `apply_declaration` for `text-overflow`, `line-clamp`,
   `-webkit-line-clamp` (legacy alias), `max-lines`, `block-ellipsis`,
   `continue`, `scroll-axis-lock`, `scrollbar-gutter` (validation only — the
   *layout effect* of `scrollbar-gutter`, i.e. actually reserving space, is a
   separate follow-up once Lumen has any concept of reserved scrollbar
   gutter), plus fix `overflow-clip-margin`'s canonical serialization order
   and wire all of the above into `computed_style_to_map`.
2. Recognize `::scroll-marker`, `::scroll-marker-group`, `::scroll-button()`
   as valid pseudo-elements in the selector grammar (so invalid forms throw
   `SyntaxError` and valid ones resolve to a real, matchable node) — actually
   generating the marker/button boxes and their interaction model
   (`scroll-marker-group`/`scroll-target-group` properties, click/hover/focus,
   `:target-current`) is a materially larger follow-up (CSS Overflow L5 is a
   young draft spec) and should be scoped as its own task once the selector
   recognition + basic box generation lands.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-overflow/` for the 60 files
above, `expected: FAIL`/`TIMEOUT` per the actual run.
