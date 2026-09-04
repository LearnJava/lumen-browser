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

## Срез 2 (2026-09-04, P3) — grammar for line-clamp/max-lines/block-ellipsis/continue

Picked up the "Grammar found, not yet implemented" list from срез 1 above.
All four are pure CSSOM specified-value validation (`div.style[prop] =
value` / `getPropertyValue`) — none of the four vendored WPT files for this
group is a `-computed.html` file, so no ComputedStyle field or layout effect
is required for these tests, same "validate without rendering" shape as
`text-overflow`'s `<string>` form (срез 1).

**Fixed this slice** (`web_api_shim_mid.js`, four new canonical functions
wired into `_lumen_canonicalize_longhand`):
- `block-ellipsis`: `no-ellipsis | ellipsis | <string>`, single token only.
- `continue`: `normal | discard | collapse | -webkit-legacy`, single token
  only (no combos, confirmed by `continue-invalid.html` rejecting every
  two-token pairing including two legacy-compatible keywords together).
- `max-lines`: real grammar (confirmed by the WPT file, not by its own
  meta-assert which says `none | <integer>` and is wrong) is `<integer
  [1,∞]> || auto` — `'none'` itself is invalid; the two-token combo
  canonicalizes integer-first regardless of input order.
- `line-clamp` (unprefixed shorthand, validated separately from
  `-webkit-line-clamp`'s reduced grammar): `none | [<integer [1,∞]> ||
  <'block-ellipsis'>] -webkit-legacy?`. Key finding: the integer component
  reuses `max-lines`'s own two-token `<integer> || auto` grammar verbatim
  (`'8 auto'` round-trips unchanged, `'auto 11'` reorders to `'11 auto'` —
  exactly what `_lumen_css_canonical_max_lines` already does, so
  `_lumen_css_canonical_line_clamp` calls it for its integer/`auto`
  component instead of reimplementing it). Serialization quirks confirmed
  against `line-clamp-valid.html`: `'ellipsis'` alone (no integer/`auto`
  component) canonicalizes to the bare keyword `'auto'` — a third
  non-compositional value alongside `none` — and a plain `ellipsis`
  block-ellipsis token is dropped when an integer/`auto` component is
  present (`'8 ellipsis'` → `'8'`), while `no-ellipsis` and `<string>` are
  kept (`'7 no-ellipsis'` stays, `'9 " etc., etc. "'` stays).
- New quote-aware tokenizer `_lumen_split_top_level_ws_quoted` — the
  existing `_lumen_split_top_level_ws` only tracks paren depth, not quotes,
  so it would wrongly split `'" etc., etc. " 12'` on the spaces inside the
  string; needed because, unlike `block-ellipsis`'s single-token grammar
  (which just regex-matches the whole trimmed value, no tokenizing needed),
  `line-clamp` genuinely combines a quoted-string token with others.
- Verified by hand-transcribing every `test_valid_value`/`test_invalid_value`
  call from all four `parsing/{block-ellipsis,continue,max-lines,
  line-clamp}-{valid,invalid}.html` files (65 cases total: 14 + 13 + 13 + 25)
  into a standalone Node `vm` harness that loads the real shim file and
  calls the four functions directly — 65/65 match the WPT-expected
  serialization. No live wptrunner pass taken (no `.venv` in this slot);
  `scripts/scoped-test.sh` (Rust side, unaffected by this JS-only change)
  is green.
- `-webkit-line-clamp`'s doc-comment updated — it no longer says `line-clamp`
  "stays unvalidated", since this slice gives it its own, separate
  validation function.

**Untouched, unambiguously out of scope for CSSOM-only validation:**
`webkit-box-computed.html` (needs `display`'s actual computed-value
algorithm to special-case `-webkit-box`/`-webkit-inline-box` when paired
with `-webkit-line-clamp`/`line-clamp` — real layout/cascade design work,
not a validation-table entry) and the `::scroll-marker` cluster (~40 files,
selector-grammar + box-generation + interaction model, unchanged from срез
1's assessment).

Remaining scope after this slice: ~44 files (was ~52) — `webkit-box-
computed.html` (1), the `::scroll-marker` cluster (~40), plus `overflow-
clip-margin`'s box-keyword+`calc()` grammar extension and `overflow`
shorthand's own computed-style path (2, not investigated this slice).

## Срез 3 (2026-09-04, P3) — overflow shorthand + overflow-block/overflow-inline
computed-style path, plus a real coerce_overflow_axes bug found along the way

Picked up `overflow-computed.html`/`overflow-shorthand-001.html` from срез 2's
remaining scope. Both files exercise `getComputedStyle(el).overflow` (the
shorthand) and `.overflowX`/`.overflowY` — the CSSOM-2 specified-value side
(`div.style.overflow`) was already fully implemented (`_lumen_expand_
overflow_shorthand`/`_lumen_overflow_shorthand_value`, wired into
`_lumen_make_style`), but `getComputedStyle()` reads straight through the
native `_lumen_get_computed_style` binding with no JS-side shorthand
collapsing at all, so `computed_style_to_map` (Rust) needed the missing
entries.

**Real bug found and fixed:** `coerce_overflow_axes` (`style/adjust.rs`) —
the existing CSS Overflow L3 §2.1 axis-coercion function ("if one axis is
`visible` and the other isn't, `visible` becomes `auto`") was missing the
spec's `clip` exemption entirely: the rule only fires when the *other* axis
is neither `visible` **nor `clip`**, but the old code treated `clip` the
same as `hidden`/`scroll`/`auto`, so `overflow: clip visible` wrongly
computed to `clip auto` instead of the spec/WPT-expected unchanged
`clip visible` (`overflow-computed.html`'s `'clip visible'`/`'visible clip'`
cases, both asserting no change). Fixed with a `forces_auto` closure
excluding `Clip` alongside `Visible`; 2 new regression tests
(`overflow_axis_coercion_clip_does_not_force_auto`,
`overflow_axis_coercion_clip_clip_unchanged`, `style/tests/values.rs`) plus
hand-verification against all 23 `test_computed_value("overflow", ...)`
cases in the WPT file. No graphic-tests page uses `overflow: clip` in any
combination (`grep -rn "overflow[a-z-]*:\s*clip\b" graphic_tests/*.html` →
zero hits), confirmed display-list-neutral by an A/B of `--dump-layout`/
`--dump-display-list` on the 6 overflow-adjacent pages (`git stash` the
change, rebuild, re-dump, `diff -rq` → empty) per this repo's Linux
`dump_golden.py` caveat.

**`overflow` shorthand's computed-value path:** new `"overflow"` entry in
`computed_style_to_map` (`selector_query.rs`) — collapses to one keyword
when the (already axis-coerced) `overflow_x`/`overflow_y` agree, else
`"x y"`, same collapse rule the CSSOM specified-value path already used.

**`overflow-block`/`overflow-inline` (CSS Overflow L3 §logical) implemented
from scratch** — these were genuinely unimplemented (no `ComputedStyle`
field, no parser arm, confirmed by a fresh `grep` before starting). Two new
`ComputedStyle` fields (`overflow_block`/`overflow_inline`, default
`Visible`, same "field != default means explicitly set" heuristic every
other logical property in `style/logical.rs` already uses), a new
`apply_declaration` arm each (`style/apply/layout.rs`, delegating to the
existing `parse_overflow_kw`), `css_wide.rs` initial/inherit/unset handling,
and a new post-cascade resolution pass
`resolve_overflow_logical_properties` (`style/logical.rs`) mapping them onto
`overflow_x`/`overflow_y` — **which physical axis depends on
`writing-mode`**: `overflow-block` → `overflow-y` under `horizontal-tb` but
→ `overflow-x` under any vertical mode (`vertical-rl`/`vertical-lr`/
`sideways-rl`/`sideways-lr`), the swap confirmed against
`css/css-overflow/logical-overflow-001.html`'s own two `writing-mode`
sub-tests. This pass runs *before* `coerce_overflow_axes` in `cascade.rs`
(reordered — it used to run before `resolve_logical_properties`, now runs
before that too) so the axis-pair adjustment sees the final physical pair,
not a stale one. `computed_style_to_map` gained matching `"overflow-block"`/
`"overflow-inline"` entries (reading back the resolved physical value on
whichever axis it mapped to), and the JS shim's CSSOM-2 validation table
(`_LUMEN_KEYWORD_PROPERTIES`, `web_api_shim_mid.js`) gained both keys with
the same 5-keyword grammar as `overflow-x`/`overflow-y` — two stale comments
elsewhere in the same file claiming "the engine has no such properties at
all" were updated to point at the new entries instead of left dangling.

7 new regression tests total (`style/tests/values.rs`, `selector_query.rs`),
`cargo test -p lumen-layout` 3772/3772 (was 3718 at срез 2 — the crate's
headline count also picked up unrelated tests added by other sessions
between срез 2 and this slice). `cargo test -p lumen-shell` scoped run clean
modulo two pre-existing Windows-path-only failures identical on `main`
(`page_pipeline::bug440_get_form_submission_resolves_to_the_target_file`,
`page_pipeline::resolve_file_base_drive_letter_is_not_a_scheme` — both
assert against a hardcoded `D:/tmp` Windows path, fail on Linux regardless
of this change). `cargo clippy -p lumen-layout` itself is currently
unrunnable on this Linux dev box regardless of branch — pre-existing
`chunks_exact`/`dead_code` errors from a newer-than-pinned system
`rustc`/`clippy` (1.98.0 vs the repo's pinned 1.97.0, no `rustup` installed
to pin it down); reproduced identically on `main` with `--no-deps`, in files
this slice never touched (`box_tree/svg.rs`, `style/parse/counters.rs`,
`invariants.rs`) — `cargo check -p lumen-layout` is clean.

**Untouched, unchanged from срез 2's assessment:** `webkit-box-
computed.html` (1), the `::scroll-marker` cluster (~40), `overflow-clip-
margin`'s box-keyword+`calc()` grammar extension (1). Remaining scope after
this slice: ~42 files (was ~44) — `overflow-computed.html`/`overflow-
shorthand-001.html` are done; the `coerce_overflow_axes` fix and the new
`overflow-block`/`overflow-inline` support weren't separately counted files
in the original 60, so the file count moves by exactly the 2 files closed.
