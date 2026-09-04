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

## Срез 4 (2026-09-04, P3) — overflow-clip-margin's full `[<visual-box> ||
<length [0,∞]>]` grammar, plus a real gate-list bug found in срез 2's four
canon functions

Picked up `overflow-clip-margin`'s box-keyword+`calc()` grammar extension,
срез 2's last-remaining item. The property was `Option<Length>`-only
(`ComputedStyle::overflow_clip_margin`) — Phase 0 supported just the bare
`<length>` half of the grammar, not the `<visual-box>` component
(`content-box | padding-box | border-box`) or the property's actual
percentage-forbidding `<length [0,∞]>` type (a raw `%`, or one nested
anywhere inside `calc()`, is invalid per spec — this is *not* the usual
`<length-percentage>` shape most of this file's other length properties
have).

**New types/functions** (`style/values/misc.rs`, `style/values/length.rs`,
`style/calc.rs`):
- `OverflowClipMarginBox` — the `content-box`/`padding-box`(default)/
  `border-box` triplet, same one-enum-per-property convention as
  `BackgroundClip`/`MaskClip` (deliberately not reused — those enums are a
  different property's grammar, this codebase doesn't share box-keyword
  enums across properties).
- `calc_node_contains_percent` (`calc.rs`) — recursive check for a
  `Length::Percent` leaf anywhere in a `calc()` tree; the first thing in
  this codebase that needed to *reject* a percent-typed calc leaf rather
  than resolve it.
- `parse_overflow_clip_margin`/`canonical_specified_overflow_clip_margin`/
  `overflow_clip_margin_serialize` (`length.rs`) — parses the 1-2-token
  order-independent grammar (own small `split_top_level_ws` — a documented
  near-duplicate of `style::parse::image`'s, same "small enough to
  duplicate rather than couple unrelated modules" call срез 2's JS
  tokenizer made) and serializes with the elision rules confirmed against
  every `test_valid_value`/`test_computed_value` case in
  `overflow-clip-margin{,-computed}.html`: the default `padding-box` never
  prints (length alone), and a zero length paired with an explicit
  `content-box`/`border-box` elides too (box keyword alone).

**Computed-value calc() resolution** (`selector_query.rs`): unlike the
specified-value path (which keeps `calc(...)` text via `length_to_css`),
`overflow-clip-margin-computed.html` expects `calc(0.5em + 100px)` to
resolve to a plain `"108px"` — CSS Values L4: absolute-and-em-only `calc()`
always resolves at computed-value time (em is always known by then; `%` was
already rejected at parse time, so it's never the blocker). New
`overflow_clip_margin_computed_length` calls `CalcNode::resolve` with the
element's real `font_size` and `Size::ZERO` for the viewport — `vh`/`vw`/
`cq*` inside this property's `calc()` stay unresolved (falls back to calc
text) since no viewport is threaded through `computed_style_to_map`, the
same Phase 0 gap every other computed length in this file already has;
not exercised by the vendored WPT file (only `em` appears).

**Real bug found in срез 2's four canon functions, fixed alongside:**
`block-ellipsis`/`continue`/`max-lines`/`line-clamp` had canonicalization
functions registered in `_lumen_canonicalize_longhand`
(`web_api_shim_mid.js`), but `_lumen_make_style`'s `setProperty` — the
function `div.style[prop] = value` actually calls via the Proxy `set` trap,
i.e. the exact path every `test_valid_value`/`test_invalid_value` case in
the vendored WPT files exercises — gates that dispatch behind an explicit
`if (_LUMEN_COLOR_PROPERTIES.hasOwnProperty(key) || ... )` allow-list that
never got the four new keys added. So `div.style['block-ellipsis'] =
'anything'` fell straight through to the raw-passthrough branch and never
called the canon function at all — срез 2's "65/65 match" was verified
against a standalone Node `vm` harness calling the four functions directly,
not this real pipeline. Added all four keys to the gate (plus
`overflow-clip-margin`, this slice's own new entry, which would have had
the identical silent-no-op bug otherwise).

**Paint** (`display_list/walk.rs`): `overflow_clip_margin`'s consumer
(clip-region expansion for `overflow: clip`) already existed and continues
to measure the margin from the padding edge exactly as before — the new
`<visual-box>` component is parsed and stored but not yet wired into *where*
the base clip edge sits (documented inline as a follow-up; no graphic_tests
page uses `overflow-clip-margin` at all, confirmed by `grep -l
"overflow-clip-margin" graphic_tests/*.html` → empty, so there is nothing in
the visual corpus for a box-keyword rendering change to move — the existing
`overflow_clip_margin_expands_clip_region` paint test still exercises only
the length half and is unaffected by this slice).

14 new tests (`style/tests/values.rs`, `selector_query.rs`),
`cargo test -p lumen-layout --lib` 3786/3786 (was 3772 at срез 3),
`cargo test -p lumen-paint --lib overflow_clip` 3/3.
`RUSTC_WRAPPER="" bash scripts/scoped-test.sh` (lumen-js/lumen-layout/
lumen-paint + reverse deps) green except the same two pre-existing
Windows-path-only `lumen-shell` failures срез 3 already documented
(`bug440_get_form_submission_resolves_to_the_target_file`,
`resolve_file_base_drive_letter_is_not_a_scheme` — hardcoded `D:/tmp`
assertions, fail on Linux regardless of branch). `cargo clippy -p
lumen-layout` remains unrunnable on this Linux dev box for the same
pre-existing toolchain-mismatch reason срез 3 hit (`chunks_exact`, 1.98.0 vs
pinned 1.97.0) — reproduces on `main` in files this slice never touched;
`cargo check -p lumen-layout -p lumen-paint --all-targets` is clean, no new
warnings.

**Untouched:** `webkit-box-computed.html` (1), the `::scroll-marker`
cluster (~40) — unchanged from срез 2's assessment, still the only
materially large remaining piece. Remaining scope after this slice: ~41
files (was ~42).

## Срез 5 (2026-09-04, P3) — `display: -webkit-box`/`-webkit-inline-box`
computed-value quirk (`webkit-box-computed.html`)

Picked up срез 2's last documented item: `getComputedStyle().display`'s
special case for the WHATWG Compat §2.1 legacy `-webkit-box`/`-webkit-
inline-box` keywords (CSS Overflow L4 §continue) — confirmed against every
`test_display_computed` case in `webkit-box-computed.html`: `display:
-webkit-box`/`-webkit-inline-box` normally compute AS SPECIFIED (the
literal keyword round-trips), but when `-webkit-box-orient` is `vertical`
**and** the box is actually clamping — either `-webkit-line-clamp`/
`line-clamp` resolves to a definite integer (not `none`/`auto`) or
`continue` is `discard` — the computed value becomes `flow-root` (for
`-webkit-box`) / `inline-block` (for `-webkit-inline-box`) instead. The
file's own comment and its `-webkit-flex`/`flex`/`inline-flex` cases pin
down that this is narrower than "any legacy webkit flex alias": those
already alias straight to `flex`/`inline-flex` and do NOT get the quirk
even under the identical orient+clamp combination.

**Genuinely new work, not a validation-table entry** (as срез 1/2 already
flagged when deferring this): none of `-webkit-box`/`-webkit-inline-box`
(as a `display` value), `-webkit-box-orient`, or `continue` had ANY Rust
representation before this slice — `grep -rniE "webkit-box|box-orient"
crates/engine/layout crates/js` was zero hits, and `continue` was pure
JS-shim CSSOM string validation (срез 2) with no `ComputedStyle` field.
`-webkit-line-clamp`/`line-clamp` already had a real field (`line_clamp`,
BUG-505 pre-history) that needed no changes — its `Some(n)`/`None` shape
already matches "definite integer" vs "none/auto" exactly.

**Added:**
- `Display::WebkitBox`/`WebkitInlineBox` (`style/values/typography.rs`) —
  new variants, deliberately distinct from `Flex`/`InlineFlex` so the quirk
  can be scoped to literally-specified `-webkit-box`/`-webkit-inline-box`.
  Parsed in `apply/layout.rs`'s `"display"` arm; `-webkit-flex`/`-webkit-
  inline-flex` parse as plain aliases straight to `Display::Flex`/
  `InlineFlex` (own arms, no new variant) per the file's own alias
  assertions. Layout treatment: Phase 0, both fall through to the generic
  `BoxKind::Block` default (same as every other unhandled `Display` value)
  — no legacy-flexbox algorithm, confirmed to be **exactly** the box kind
  these elements already got before this slice (see A/B below).
- `WebkitBoxOrient` (`Horizontal`(default)/`Vertical`) and `CssContinue`
  (`Normal`(default)/`Discard`/`Collapse`/`WebkitLegacy`) — new enums +
  `ComputedStyle` fields (`box_orient`/`continue_value`, both non-inherited),
  parsed in `apply/text.rs` next to `line_clamp` (`"-webkit-box-orient"`/
  `"continue"` arms), `css_wide.rs` inherit/initial handling, `SUPPORTED_
  PROPERTIES` (`css-parser/src/lib.rs` — needed for `CSS.supports()`, which
  four of this WPT file's own test blocks gate on: `line-clamp`/`continue`
  weren't in that list at all despite `line-clamp` already having a real
  field since срез 2, an existing coverage gap fixed alongside), and the JS
  shim's `_LUMEN_KEYWORD_PROPERTIES` (`-webkit-box-orient`; `continue` was
  already gated since срез 2/4).
- `webkit_box_computed_display` (`selector_query.rs`) — the quirk itself,
  replacing the old flat `match style.display {...}` in `computed_style_
  to_map`'s `"display"` entry: computes `is_clamping = box_orient ==
  Vertical && (line_clamp.is_some() || continue_value == Discard)` once,
  then only `WebkitBox`/`WebkitInlineBox` branch on it: `flow-root`/
  `inline-block` when clamping, else the literal keyword. Every other
  `Display` variant is untouched pass-through.
- `-webkit-box-orient`/`continue` also got their own `computed_style_to_
  map` entries (round-trip, not just feeding the quirk) and a `snapshot.rs`
  arm for the two new `Display` variants (the crate's other exhaustive
  match on `Display`, alongside `computed_style_to_map` — both required by
  the compiler, confirmed no *other* file has a third exhaustive match:
  `cargo check --workspace --all-targets` after the change is clean with
  zero non-exhaustive-match errors anywhere else in the workspace).

**Real risk investigated and ruled out, not just assumed:** two
`graphic_tests` pages (`48-line-clamp.html`, `1000000-final.html`) already
use `display: -webkit-box` + `-webkit-box-orient: vertical` +
`-webkit-line-clamp` for real line-clamp truncation rendering — before this
slice, `-webkit-box` was an *unrecognized* `display` token that silently
left `style.display` at its previous value (`Block`, the UA default for
`<div>`, since nothing else set it), so these pages already rendered as
`BoxKind::Block`. Read through every place `Display` participates in box-
kind selection (`box_tree/build.rs`'s item-container/table/grid/flex
groups, `is_inline_content`/`is_atomic_inline_level` in `inline_build.rs`,
the auto-margin/shrink-to-fit exclusion lists in `layout_dispatch.rs`,
`box_tree/flex.rs`'s own item-content dispatch) — `WebkitBox`/
`WebkitInlineBox` match none of them, so both fall to the same generic
`else { BoxKind::Block }` default as before; line-clamp truncation itself
(`apply_line_clamp`, `layout_dispatch.rs:864`) is keyed purely on
`s.line_clamp`, never on `s.display`, so it was and remains unaffected
either way. Confirmed empirically, not just by code reading, since this
repo's Linux dev box can't run the gdigrab-based `graphic_tests/run.py`
pixel pipeline (`docs/graphic-tests.md`'s documented Linux caveat — same
constraint срез 3/4 hit): built `dev-release` for both this slice's code
and `git stash`-restored pre-slice `main`, ran `--dump-layout`/`--dump-
display-list` on both pages against both builds. `--dump-display-list`
diff is **empty** for both pages (byte-identical). `--dump-layout` diff
shows only the new `display=-webkit-box` debug annotation appearing next
to each `.box` element (`snapshot.rs` now prints the more accurate
`Display` value where before `Display::Block` printed nothing) — every
`rect=(...)` geometry line is otherwise byte-identical between the two
builds. Pixel-neutral, confirmed rather than assumed.

12 new regression tests (`selector_query.rs`), transcribing the WPT file's
own matrix (default/orient-alone/clamp-none/clamp-without-vertical-orient/
explicit-horizontal-orient/vertical-orient+clamp/vertical-orient+continue-
discard/continue-discard-without-vertical-orient/continue-none/-webkit-
inline-box/the three flex-alias non-cases), all against the real cascade
pipeline (`div_computed_map`, not a direct function call) — 12/12 pass.
`cargo test -p lumen-layout --lib` 3799/3799 (was 3786 at срез 4).
`cargo check --workspace --all-targets` clean. `cargo clippy -p lumen-
layout`/`-p lumen-css-parser --all-targets -- -D warnings`: `lumen-css-
parser` clean; `lumen-layout` itself still blocked by the same pre-existing
Linux toolchain-mismatch this bug's срез 3/4 already documented (system
`rustc`/`clippy-driver` 1.98.0 vs the repo's pinned 1.97.0, no `rustup` to
pin it down — reproduces on `main` in files this slice never touched, e.g.
`lumen-image`'s `chunks_exact` lint). `RUSTC_WRAPPER="" bash scripts/
scoped-test.sh` green except the same two pre-existing Windows-path-only
`lumen-shell` failures срез 3/4 already documented (`bug440_get_form_
submission_resolves_to_the_target_file`, `resolve_file_base_drive_letter_
is_not_a_scheme` — hardcoded `D:/tmp` assertions, fail on Linux regardless
of branch; re-confirmed present on `main` too via the A/B above).

**Untouched, unchanged from срез 2-4's assessment:** the `::scroll-marker`
cluster (~40 files: `::scroll-marker`/`::scroll-marker-group`/
`::scroll-button()`/`scroll-target-group` selector recognition + box
generation + interaction model). This is now the **only** remaining piece
of this bug's original 60-file scope — срез 1 already called it "a
materially larger follow-up... should be scoped as its own task once the
selector recognition + basic box generation lands", which still holds:
CSS Overflow L5 is a young draft spec, `grep -rni "scroll-marker\|
scroll-button\|scroll-target-group" crates/ --include=*.rs --include=*.js`
is still zero hits, and nothing in this bug's five slices has touched
selector-grammar/pseudo-element recognition at all. Remaining scope after
this slice: ~40 files (was ~41) — all `::scroll-marker`.

## Срез 6 (2026-09-04, P3) — `::scroll-marker`/`::scroll-marker-group`/
`::scroll-button()` selector-grammar recognition + the `scroll-marker-
group`/`scroll-target-group` supporting properties

Picked up the first half of срез 1's own scoping split for the
`::scroll-marker` cluster: "recognize as valid pseudo-elements in the
selector grammar... actually generating the marker/button boxes and their
interaction model is a materially larger follow-up... should be scoped as
its own task once the selector recognition + basic box generation lands."
This slice does exactly the first half — selector-grammar recognition plus
the two CSS properties that gate a scroll-marker-group's placement/
grouping — and deliberately does **not** attempt box generation, snap-
target iteration, or the click/hover/focus interaction model (still a
separate, materially larger follow-up; see "Untouched" below).

Scoped down from the original 60-file write-up's ~40-file remainder by
reading the actual vendored WPT files first (`tests/wpt/css/css-overflow/
parsing/{scroll-buttons,scroll-markers,scroll-target-group}-*.html`,
`getComputedStyle-scroll-button.html`) rather than re-deriving grammar from
the spec: only 8 of those ~40 files are pure selector-grammar/property-
parsing exercises with no box-generation dependency — the other ~32
(`scroll-markers/*.html`, `scroll-marker-group-{hover,hover-from-marker,
display-none}.html`, `getComputedStyle-scroll-button.html`) all assert on
actual generated boxes, snap-target grouping, or activation/hover/focus
behavior and stay out of scope.

**Selector grammar** (`crates/engine/css-parser/src/parser/selectors.rs`):
- `PseudoElementKind::ScrollMarker`/`ScrollMarkerGroup` — simple pseudo-
  elements, same shape as `::marker`/`::placeholder` (no argument, `_ =>
  true` in `pseudo_element_is_valid`).
- `PseudoElementKind::ScrollButton(String)` — functional pseudo-element,
  same parsing shape as `::picker(select)`/`::highlight(name)`
  (`parse_functional_pseudo_element`'s `"scroll-button"` arm), except the
  argument can also be the bare `*` token (not an ident) — its own small
  branch before falling back to `parse_ident`. Argument validity (`up`/
  `down`/`left`/`right`/`block-start`/`inline-start`/`inline-end`/
  `block-end`/`*`, confirmed exhaustively by `scroll-buttons-{valid,
  invalid}.html`'s own matrix — `north`/`5051`/quoted-string/comma-list/
  empty all rejected) is checked in `pseudo_element_is_valid`, mirroring
  `::picker`'s `arg == "select"` check.
- `::scroll-button()` is a focusable, potentially-disabled control (CSS
  Overflow L5 §scroll-buttons) — `compound_selector_is_valid`'s trailing-
  pseudo-class check gained a `ScrollButton`-specific carve-out allowing
  `:disabled`/`:enabled` after it (on top of the existing generic
  `is_user_action_pseudo_class` allowance, which already covers `:focus`
  for every pseudo-element), confirmed by `scroll-buttons-valid.html`'s own
  `:focus`/`:disabled`/`:enabled` matrix for all eight directions.
- `pseudo_element_name()` (`crates/engine/layout/src/style/pseudo.rs`, the
  single kind↔name source of truth the matcher and `CascadeIndex::
  pseudo_subjects` both go through) gained the three new names — required
  by the compiler (exhaustive match), and means a stylesheet rule actually
  targeting one of these three pseudo-elements is now syntactically
  recognized end-to-end, even though nothing ever generates a matching box
  (same Phase-0 shape `::before`/`::after` had before `inject_pseudo`
  existed) — `querySelector('::scroll-marker')` no longer throws
  `SyntaxError`, it now correctly returns `null` (valid selector, no match),
  same as `::before` always has.

**`scroll-marker-group`/`scroll-target-group` properties** — both needed a
real `ComputedStyle` field, not just JS-shim CSSOM validation
(`block-ellipsis`/`continue`'s срез-2 shape), because both have a
`-computed.html` WPT file that exercises `getComputedStyle()`, CSS-wide
keywords (`initial`/`inherit`/`unset`/`revert`), and CSSOM enumeration —
none of which a validation-only property can answer:
- `ScrollTargetGroup` (`style/values/misc.rs`) — plain `none | auto`, fits
  the existing `ScrollbarWidth`-shaped `#[derive(Default)] enum` +
  `parse`/`to_css` pattern exactly; validated via a one-line
  `_LUMEN_KEYWORD_PROPERTIES` entry in the JS shim (no custom canon
  function needed).
- `ScrollMarkerGroup`/`ScrollMarkerGroupPlacement`/`ScrollMarkerGroupMode`
  (same file) — `none | [before|after] [tabs|links]?`, order-dependent
  (confirmed by `scroll-markers-invalid{,.tentative}.html`: `links after`/
  `tabs before`/`after tab`(typo)/`after, tabs`(comma) all rejected —
  direction must come first, comma-separated forms are invalid, and the
  `tabs`/`links` component is itself a tentative, not-yet-stable extension,
  github.com/w3c/csswg-drafts/issues/12122). The property's own `none`
  initial value is modeled as `Option<ScrollMarkerGroup>` being `None` —
  there's no `before`/`after` to place when the value is `none`, so
  `ScrollMarkerGroup::parse` returns the unusual `Option<Option<Self>>`
  (outer = parse success, inner = the property's own value space) rather
  than this module's usual bare `Option<Self>`. New JS-shim canon function
  `_lumen_css_canonical_scroll_marker_group` (`web_api_shim_mid.js`), added
  to both `_lumen_canonicalize_longhand`'s dispatch and — remembering срез
  4's own gate-list bug — the `setProperty` allow-list gate in the same
  commit, not a follow-up.
- Both wired through the full property pipeline other BUG-505 slices
  established: `apply_declaration` arm (`style/apply/layout.rs`), CSS-wide-
  keyword arm (`style/apply/css_wide.rs`), the three `ComputedStyle`
  literal-construction sites (`computed.rs`'s struct field + `root()`,
  `cascade.rs`'s per-element baseline), and a `computed_style_to_map` entry
  (`selector_query.rs`) — `_lumen_get_computed_style_entries` (the native
  binding backing `getComputedStyle()`'s `Array.from()`/`for...of`
  enumeration, `BUG-483 ч.2`) reads straight off that map, so no separate
  work was needed for the WPT files' "shows up in CSSStyleDeclaration
  enumeration"/".cssText" assertions.

**Verification:** 15/15 hand-transcribed cases from `scroll-markers-
{invalid,computed}{,.tentative}.html`'s `scroll-marker-group` matrix
(8 invalid + 7 valid/computed) traced against `_lumen_css_canonical_
scroll_marker_group` via a standalone Node harness that extracts and evals
the real shim function (срез 2's method, not a reimplementation) — 15/15
match. `cargo test -p lumen-layout --lib` 3808/3808 (was 3799 at срез 5, +9
new: 6 `ScrollMarkerGroup` + 3 `ScrollTargetGroup` parse/computed-map
tests). `cargo test -p lumen-css-parser --lib` 362/362 (+3 new: the invalid-
form matrix, the whitespace-canonicalization round-trip, plus the extended
`valid_selector_list_accepts_ordinary_selectors` list covering all nine
`::scroll-button(<direction>)` forms + `:focus`/`:disabled`/`:enabled`).
`RUSTC_WRAPPER="" bash scripts/scoped-test.sh` green except the same two
pre-existing Windows-path-only `lumen-shell` failures срез 3-5 already
documented (`bug440_get_form_submission_resolves_to_the_target_file`,
`resolve_file_base_drive_letter_is_not_a_scheme` — hardcoded `D:/tmp`
assertions, fail on Linux regardless of branch). `cargo clippy -p lumen-
css-parser --all-targets -- -D warnings` clean; `-p lumen-layout` still
blocked by the same pre-existing Linux toolchain-mismatch срез 3-5 already
documented (system `rustc`/`clippy-driver` 1.98.0 vs the repo's pinned
1.97.0, no `rustup` to pin it down — reproduces on `main` in files this
slice never touched, `lumen-image`'s `chunks_exact` lint); `cargo check
--workspace --all-targets` is clean. Display-list neutrality confirmed
empirically, not assumed: `--dump-layout`/`--dump-display-list` A/B (this
slice's `dev-release` build vs `git stash`-restored pre-slice `main`, same
build) across **all 162** `graphic_tests/*.html` pages — `diff -rq` on both
dump sets is empty. Expected, since neither new property has any consumer
yet and none of the three new pseudo-elements ever matches a real element
(no box-generation code was touched), but shown rather than assumed per
this repo's own rule.

**`.ini` bookkeeping deliberately left untouched**, same as every prior
slice of this bug (`git show --stat` on all five срез-1–5 commits: zero
`.ini` changes despite several of them fully closing a file) — flipping
`expected: FAIL` → removed requires a live wptrunner re-run to certify "0
unexpected", which this dev slot doesn't have (срез 1's own note: no
`.venv`, live `--mcp-live-port` eval hits an unrelated engine issue). The
8 files this slice's own testing gives high confidence are now fully
passing: `parsing/scroll-buttons-{invalid,valid}.html`, `parsing/
scroll-markers-{invalid,computed}{,.tentative}.html`, `parsing/
scroll-target-group-{invalid,computed}.html` — left for the next WPT-RUN
mass-run pass to confirm and flip.

**Untouched, genuinely out of scope** (per this slice's own opening split):
box generation for all three pseudo-elements (nothing calls `compute_
pseudo_element_style`/`inject_pseudo`-equivalent for any of them — they
parse and match nothing, same as `::before` before layout wired it),
scroll-snap-target iteration for `::scroll-marker-group`/`scroll-target-
group`'s actual grouping semantics, `::scroll-button()`'s click-to-scroll
behavior and hit-testing, `getComputedStyle(el, pseudoElt)`'s second-
argument resolution for any of the three (`getComputedStyle-scroll-
button.html` — needs real box generation to answer correctly, not just
selector recognition), and the `scroll-markers/` directory's ~32 remaining
files (event targeting, disposal, focus/hover/activation, container-query
interaction, iframe interaction). Remaining scope after this slice: ~32
files (was ~40) — all in `scroll-markers/`, requiring the box-generation +
interaction-model work срез 1 originally flagged as its own task.
