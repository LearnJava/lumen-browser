# BUG-1019: `resolution`/`min-resolution`/`max-resolution` media feature not implemented at all — no units, no `calc()`

**Статус:** FIXED 2026-09-07 (P3)
**Дата:** 2026-09-06
**Компонент:** css-parser (`crates/engine/css-parser/src/parser/media.rs` —
`MediaFeature` enum, `parse_media_feature`; shared by `@media` cascade
matching and JS `matchMedia()`, `grep -n resolution crates/engine/css-parser`
is empty)
**Найден:** P3 2026-09-06, побочно при закрытии [BUG-526](BUG-526-FIXED.md)
(`MediaQueryList.media` serialization)

## Механизм

`parse_media_feature`'s `match key.as_str()` has arms for `width`/`height`/
`aspect-ratio`/`orientation`/`prefers-*`/`hover`/`pointer`/… but none for
`resolution`/`min-resolution`/`max-resolution` — the feature falls through
to the default arm and becomes `MediaCondition::Unsupported`. There is no
`MediaFeature::Resolution` variant, no `dppx`/`dpi`/`dpcm` unit parsing, and
no `calc()` expression evaluator anywhere in the module (`grep -n calc
crates/engine/css-parser/src/parser/media.rs` is empty too). This module is
shared by both `@media` stylesheet-rule cascade matching (`crates/engine/
css-parser/src/parser/at_rules.rs`) and JS `matchMedia()`
(`crates/js/src/v8_runtime/install/platform.rs`) — the gap is not
matchMedia-specific.

## Симптом

Any page using `@media (min-resolution: 2dppx)` (the standard high-DPI
media query, extremely common on real sites for `<img>`/background-image
2x/3x asset switching) never matches, regardless of actual device pixel
ratio — the clause is `Unsupported`, `matches()` always returns `false`.
`window.matchMedia('(resolution: 2dppx)').matches` is always `false` too.

Confirmed via `parse_media_query(q).serialize()` probe (7 subtests of
`css/mediaqueries/match-media-parsing.html::test_resolution_parsing`):

| query | expected `.media` | got |
|---|---|---|
| `(min-resolution: calc(1x))` | `(min-resolution: calc(1dppx))` | `not all` |
| `(resolution: calc(2x))` | `(resolution: calc(2dppx))` | `not all` |
| `(max-resolution: calc(7x))` | `(max-resolution: calc(7dppx))` | `not all` |
| `(resolution: calc(1x + 2x))` | `(resolution: calc(3dppx))` | `not all` |
| `(resolution: calc(5x - 2x))` | `(resolution: calc(3dppx))` | `not all` |
| `(resolution: calc(1x * 3))` | `(resolution: calc(3dppx))` | `not all` |
| `(resolution: calc(6x / 2))` | `(resolution: calc(3dppx))` | `not all` |

The non-`calc()` forms (`(min-resolution: 1x)`, `(resolution: 2dppx)`,
`(resolution: 600dpi)`, `(resolution: 77dpcm)`, …) are not separately
listed as WPT failures only because `test_parsing()` without an explicit
`expected` defaults to `expected === query`, which any raw-echo
implementation trivially satisfies — that masked the underlying gap in
`.ini` triage, not evidence it works. Matching (not just serialization) is
equally broken for all of these forms.

## Масштаб

7 subtests of `match-media-parsing.html`, all under
`test_resolution_parsing()`; real-world impact is broader (any
`(resolution: …)`/`(min-resolution: …)`/`(max-resolution: …)` media query on
a live page, for both `@media` cascade and `matchMedia()`).

## Что нужно

- Add `MediaFeature::Resolution(f32)`/`MinResolution(f32)`/
  `MaxResolution(f32)` (canonical unit: `dppx`, matching `<resolution>`'s
  CSS Values L4 canonical unit) plus `x`/`dppx` (alias), `dpi` (÷96),
  `dpcm` (÷2.54/96) conversion into `parse_media_feature`.
- A minimal `calc()` evaluator for the arithmetic forms
  (`calc(1x + 2x)`/`calc(5x - 2x)`/`calc(1x * 3)`/`calc(6x / 2)`) —
  four operators, single-unit operands, no nesting needed by this test
  file; check whether `crates/engine/css-parser` already has a generic
  `<length>`/numeric `calc()` evaluator used elsewhere (e.g. for
  `width: calc(...)`) that this can reuse instead of a bespoke one.
- Wire actual device resolution into `MatchContext` (currently only
  `width`/`height` — `grep -n "struct.*Ctx\|MatchContext"
  crates/engine/css-parser/src/parser/media.rs`) — device pixel ratio is
  already computed somewhere for `devicePixelRatio` (`BUG-1017`'s
  `defaultView` fix references it); reuse that source rather than adding a
  second one.
- Feature-value serialization (`MediaFeature::serialize`, added by
  [BUG-526](BUG-526-FIXED.md)) needs a `Self::Resolution(dppx) =>
  format!("resolution: {dppx}dppx")`-style arm plus a `calc(...)` variant
  that re-serializes the simplified numeric result, once the feature and
  its `calc()` value are represented in the AST instead of collapsing to
  `Unsupported`.

## .ini

`tests/wpt/metadata/css/mediaqueries/match-media-parsing.html.ini` carries
the 7 subtests above as `expected: FAIL`, attributed to this bug.

## Fix (P3, 2026-09-07)

Added `MediaFeature::Resolution`/`MinResolution`/`MaxResolution`, each
wrapping a new `ResolutionValue` (`crates/engine/css-parser/src/parser/
media.rs`) — `Literal(f32)` or `Calc(f32)`, both storing the already-
converted canonical `dppx` amount. The `Calc` vs `Literal` split exists
purely for serialization: Media Queries L4 keeps the `calc(...)` wrapper
even once the expression collapses to a single number
(`(resolution: calc(1x))` serializes as `(resolution: calc(1dppx))`, not
`(resolution: 1dppx)`) — a bare `f32` would have lost that distinction.

**Unit parsing** (`parse_resolution_dppx`): `x`/`dppx` (1:1), `dpi` (÷96 —
96dpi = 1dppx), `dpcm` (×2.54/96 — 1dpcm = 2.54/96 dppx, since 1px =
1/96in = 2.54/96cm). `dppx`/`dpcm` are checked before the bare `x` suffix
strip, since both also end in `x`.

**`calc()` evaluator** (`eval_resolution_calc` + `tokenize_calc` +
`parse_calc_operand`): a minimal left-to-right evaluator handling `+`/`-`/
`*`/`/` with single-level operands (a `<resolution>` for `+`/`-`, a
`<resolution>` or a bare number for `*`/`/`) — the only form the vendored
test exercises (`calc(1x + 2x)`, `calc(5x - 2x)`, `calc(1x * 3)`,
`calc(6x / 2)`). No nested `calc()`/parens support, matching scope. A
generic `<length>`-style calc evaluator was checked for first (per this
bug's own "Что нужно") — none exists in `lumen-css-parser`, and the one in
`lumen-layout` (`style/calc.rs`) can't be reused: `layout` depends on
`css-parser`, not the other way around, so pulling it in would be a
layering violation. Not worth a shared crate for four operators used in
exactly one place.

**Second, independent bug found and fixed along the way**:
`parse_media_clause`'s feature-value scanner took the *first* `)` in the
clause (`input.find(')')`), not the balanced one — for `(resolution:
calc(1x))`, that closed `calc(`'s own parenthesis, truncating the outer
feature's value to `resolution: calc(1x` and losing the trailing `)`
entirely. This broke every feature value containing any nested
parenthesized construct, not just `resolution`'s `calc()` — fixed with a
proper depth-counting `find_matching_close_paren` helper.

**`MediaContext.resolution_dppx: f32`** (default `1.0`) — matching, not
just parsing: `Resolution`/`MinResolution`/`MaxResolution` compare against
this field. Deliberately *not* wired to any live per-window scale factor:
`window.devicePixelRatio` itself is a hardcoded `1` everywhere in the
engine outside the multi-window API (`crates/js/src/window_management.rs`)
— no dynamic scale-factor plumbing exists yet for the engine's single-
window case, so threading a second, currently-always-`1.0` value through
the two JS entry points (`_lumen_match_media`, `_lumen_deliver_media_
changes`) and every shell call site would add surface with zero observable
behavior change today. Also not required by the vendored test: `match-
media-parsing.html::test_resolution_parsing` only asserts `MediaQueryList.
media` (serialization), never `.matches`.

**Verification**: 7 new unit tests in `crates/engine/css-parser/src/
parser/tests/at_rules.rs` transcribing `test_resolution_parsing` verbatim
(units round-trip, `dpi`/`dpcm` conversion, `calc()` keeping its wrapper
after collapsing to one number, the four `calc()` arithmetic forms,
`resolution_dppx`-based matching, unknown-unit → `Unsupported`).
`cargo test -p lumen-css-parser --lib`: 432/432 (was 425, +7). `cargo
clippy -p lumen-css-parser --all-targets -- -D warnings`: clean. No other
crate matches on `MediaFeature` exhaustively (`grep -rn "MediaFeature::"`
outside `media.rs` — zero hits), and every `MediaContext { ... }`
construction site already uses `..Default::default()`, so the new field
needed no call-site changes. No paint/display-list surface touched — no
`dump_golden.py`/graphic-test drift possible. `tests/wpt/metadata/css/
mediaqueries/match-media-parsing.html.ini` updated — the 7 `calc()`
subtests removed; the file's other 7 (BUG-1020, an unrelated boolean-
context/unclosed-paren gap) stay.

**Deliberately out of scope**: wiring a real per-window device-pixel
ratio into `MediaContext.resolution_dppx` (see above — blocked on the
engine not having one to wire, not on this bug); negative-`<resolution>`
rejection (CSS Values L4 marks `<resolution>` non-negative, but no
vendored test in this file exercises it, and the pre-existing sibling
features like `width`/`height` don't validate sign either — same debt
class, not introduced here).
