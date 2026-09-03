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

## Срез 1 (2026-09-03, P3) — revision + inline-style validation for the
already-implemented longhands

**Revision finding:** `text-overflow`, `-webkit-line-clamp`/`line-clamp`
(the reduced `none | <integer>` shape), `overflow-clip-margin` (bare
`<length>` shape) and `scrollbar-gutter` (full layout effect) were all
independently implemented in `crates/engine/layout/src/style/apply/*.rs`
by other sessions after this bug was filed (2026-08-02) — `CSS-SPECS.md`
already marks all four ✅/effective. What remained of symptom 1 for these
four was narrower than the original write-up: not "property doesn't exist"
but two independent, already-known-shape gaps — (a) `_lumen_make_style`
(CSSOM-2/BUG-484 machinery) had no validation table entry for any of the
four, so `element.style[prop] = <invalid>` was still accepted verbatim
(`text-overflow-invalid.html`, `webkit-line-clamp-invalid.html`,
`scrollbar-gutter-invalid.html`), and (b) `computed_style_to_map`
(CSSOM-3/BUG-472 machinery) had no entry for `line-clamp`/
`-webkit-line-clamp`/`scrollbar-gutter`/`overflow-clip-margin`, so
`getComputedStyle()` answered `""` for all four regardless of the real
layout effect.

**Fixed this slice:**
- `text-overflow`: full grammar (`clip | ellipsis | <string>`) validated in
  the JS shim (`_lumen_css_canonical_text_overflow`,
  `web_api_shim_mid.js`) — the `<string>` custom-marker form is accepted at
  the CSSOM layer per spec even though the cascade (`style/apply/text.rs`)
  doesn't render it yet (Phase 0, unchanged).
- `-webkit-line-clamp`: full grammar (`none | <integer [1,∞]>`) validated
  (`_lumen_css_canonical_webkit_line_clamp`). The unprefixed `line-clamp`
  shorthand is deliberately NOT validated with this same reduced grammar —
  its real grammar additionally accepts `auto`/`<'block-ellipsis'>`/
  `-webkit-legacy`/multi-token combos (see "Grammar found, not yet
  implemented" below); validating it as `none | <integer>` would reject
  values (`auto`, `ellipsis`, quoted strings) that a bare passthrough
  currently accepts and that WPT's `line-clamp-valid.html` expects to
  round-trip.
- `scrollbar-gutter`: full grammar (`auto | stable && both-edges?`)
  validated (`_lumen_css_canonical_scrollbar_gutter`), order-independent
  two-token form included. `ScrollbarGutter::parse`
  (`style/values/misc.rs`) also fixed to accept `"both-edges stable"`
  (reversed order) — previously only the canonical token order parsed,
  so an author stylesheet using the reversed form silently fell back to
  `auto`.
- `computed_style_to_map` (`selector_query.rs`) gained entries for
  `line-clamp`/`-webkit-line-clamp` (`"none"` or the integer as a string),
  `scrollbar-gutter` (three string forms) and `overflow-clip-margin`
  (`length_to_css`, `None` → `"0px"` per the property's initial value).
- `overflow-clip-margin`'s inline-style validation was deliberately left
  alone (see below) — same "partial grammar would regress currently-passing
  keyword-form tests" reasoning as `line-clamp`.
- 8 new tests (`selector_query.rs`, `style/tests/values.rs`); every valid/
  invalid value in `text-overflow-{valid,invalid}.html`,
  `webkit-line-clamp-{valid,invalid}.html` and
  `scrollbar-gutter-{valid,invalid}.html` traced by hand against the new
  canon functions (25 + 10 + 8 cases) — live WPT/`--mcp-live-port` eval
  was attempted but hit an unrelated engine issue (`eval` → "JS context
  not available" even after `wait{condition:document_ready}` on a fresh
  `file://` navigation with `LUMEN_NO_ENGINE_THREAD=1`; not investigated
  further, out of this bug's scope) — no `.venv` in this slot either, so
  no live wptrunner pass was taken.

**Grammar found, not yet implemented (deferred, don't re-derive from
scratch — read the actual WPT files under
`tests/wpt/css/css-overflow/parsing/` before attempting):**
- `line-clamp` shorthand (unprefixed): `none | [<integer [1,∞]> ||
  <'block-ellipsis'>] -webkit-legacy?`. Surprising canonicalization rules
  confirmed by `line-clamp-valid.html`: `'ellipsis'` alone canonicalizes to
  `'auto'`; `'8 ellipsis'` canonicalizes to `'8'` (the default
  block-ellipsis value drops out of the serialization when paired with an
  integer); `'no-ellipsis 10'` reorders to `'10 no-ellipsis'`.
- `max-lines`: **not** `none | <integer>` despite its own meta-assert
  saying so — `max-lines-invalid.html` rejects `'none'` outright, and
  `max-lines-valid.html` accepts `'auto'`, plain integers, AND two-token
  `auto`+integer combos in either order (canonicalizing to integer-first,
  e.g. `'auto 8'` → `'8 auto'`). Real grammar per the tests:
  `<integer [1,∞]> || auto`.
- `block-ellipsis`: `no-ellipsis | ellipsis | <string>` — single token
  only (no combos).
- `continue`: `normal | discard | collapse | -webkit-legacy` — single
  token only.
- `-webkit-box`/`-webkit-inline-box` `display` computed-value interaction:
  `getComputedStyle().display` resolves to `flow-root`/`inline-block` (not
  `-webkit-box` itself) specifically when `-webkit-box-orient: vertical`
  (the default) is paired with a set `-webkit-line-clamp`/`line-clamp` —
  see `webkit-box-computed.html` for the full matrix (12+ cases, including
  the plain `flex`/`-webkit-flex` compat-alias cases that must NOT get
  this special-cased resolution).
- All four require real design work (a proper block-ellipsis/continue
  representation, `-webkit-legacy` modifier, and `display`'s computed-value
  algorithm reading `line-clamp`) — not a validation-table entry — so they
  were left for a future slice rather than attempted here.

**Untouched, unambiguously a materially larger follow-up (per this bug's
own original "What's needed" §2):** `::scroll-marker`/
`::scroll-marker-group`/`::scroll-button()`/`scroll-target-group` selector
recognition + box generation + interaction model — CSS Overflow L5, zero
hits anywhere in the workspace including the JS shim
(`grep -rni "scroll-marker\|scroll-button\|scroll-target-group"
crates/ --include=*.rs --include=*.js` → nothing), ~40 of the original 60
files.

Remaining scope after this slice: ~52 files (was 60) — the `-webkit-box`/
`line-clamp` family cluster (~9 files), the `::scroll-marker` cluster
(~40 files), plus `overflow-clip-margin`'s box-keyword+`calc()` grammar
extension and `overflow` shorthand's own computed-style path
(`overflow-computed.html`/`overflow-shorthand-001.html`, not investigated
this slice).
