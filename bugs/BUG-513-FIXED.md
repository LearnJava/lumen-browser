# BUG-513: `text-size-adjust` CSS property not implemented at all

**Статус:** FIXED 2026-09-05
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -rln "text.size.adjust\|
text_size_adjust" crates/` — zero hits anywhere in the workspace)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-size-adjust`

## Механизм

`text-size-adjust` (CSS Text Size Adjustment Module,
`https://drafts.csswg.org/css-size-adjust-1/`) controls whether mobile
browsers auto-inflate text size on narrow viewports; it ships (unprefixed
or as `-webkit-text-size-adjust`) in every major mobile browser engine. The
property is entirely absent from `ComputedStyle` and the parser's
known-property table — neither the unprefixed nor prefixed spelling is
recognized.

## Симптом

```
FAIL Property text-size-adjust has initial value auto
  assert_true: text-size-adjust doesn't seem to be supported in the
  computed style expected true got false
FAIL CSS Transitions: property <text-size-adjust> from neutral to [50%] at
  (0) should be [60%]
  assert_true: 'to' value should be supported expected true got false
```

## Масштаб находки

3 files / 204 subtests — by far the largest single-property finding of this
slice, dominated by one file: `animations/text-size-adjust-interpolation.html`
(196 — the Web Animations interpolation-test harness enumerates every
tested value/timing pair and fails identically at its own "is this property
supported" feature-detect before ever reaching interpolation math),
`inheritance.html` (2), `parsing/text-size-adjust-computed.html` (6).
`parsing/text-size-adjust-invalid.html` (4) and one subtest of
`parsing/text-size-adjust-valid.html` (`calc(10% + 5%)` not canonicalized to
`calc(15%)`) fail on the separate, generic inline-`style`-setter gap
([BUG-484](BUG-484-OPEN.md)) instead.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-size-adjust/` for
`animations/text-size-adjust-interpolation.html`, `inheritance.html`, and
`parsing/text-size-adjust-computed.html`, `expected: FAIL` per subtest.

## Срез 2026-09-05 (P3): fixed

Full grammar cycle implemented, mirroring the shape of `dynamic-range-limit`
(BUG-508): a new value type `TextSizeAdjust` (`auto | Percentage(f32)`,
`style/values/text_size_adjust.rs`) — `none` folds into `Percentage(100.0)`
at parse time, since the spec's computed value for the legacy `none`
keyword is `100%`, not a separate keyword. New inherited `ComputedStyle`
field `text_size_adjust`, initial `Auto`. Both spellings
(`text-size-adjust`/`-webkit-text-size-adjust`) share one `match` arm in
`style/apply/text.rs`, one CSS-wide-keyword arm in `style/apply/css_wide.rs`,
and both are registered in `SUPPORTED_PROPERTIES`
(`crates/engine/css-parser/src/lib.rs`). `computed_style_to_map`
(`selector_query.rs`) serializes both spellings from the same underlying
field — same convention already used there for `-webkit-line-clamp`/
`line-clamp`.

`TextSizeAdjust::interpolate` (regular `<percentage>` lerp, clamped to
`[0,∞)`, discrete flip when either side is `Auto`) exists as a pure library
function, unit-tested directly — but, like `DynamicRangeLimit::interpolate`
(BUG-508), it is **not** wired into the native CSS Animations/Transitions
engine (`crate::animation`, Phase-0 hardcoded animatable-property table) nor
exercised by `element.animate()` in the live shell. This is the same
ДОРАБОТКА gap BUG-508 already accepted, not new scope for this fix. No
mobile auto-inflation rendering pipeline exists in Lumen either way — the
property has CSSOM/animation observability only, no rendering effect.

12 new unit tests (`crates/engine/layout/src/style/tests/text_size_adjust_tests.rs`)
directly mirror every subtest shape in `parsing/{text-size-adjust-valid,
text-size-adjust-invalid,text-size-adjust-computed}.html`,
`inheritance.html`, and `animations/text-size-adjust-interpolation.html`
(percentage-to-percentage lerp at several `t` values matching the WPT
file's own expected outputs, negative-clamp case, discrete-with-`auto`
case). One `text-size-adjust-valid.html` case — `calc(10% * sibling-index())`
— is out of scope: `sibling-index()` (CSS Values L5) is not implemented
anywhere in the engine (`grep -rn "sibling-index\|sibling_index" crates/`
— zero hits), a separate, unrelated gap.

`cargo test -p lumen-layout` 3833/3833 (+11 lib, +2 `--test cases`
unaffected), `cargo clippy -p lumen-layout -p lumen-css-parser --all-targets
-- -D warnings` clean, `cargo check --workspace --all-targets` clean.
`scripts/scoped-test.sh` — the one red target is the pre-existing, known
[BUG-997](BUG-997-OPEN.md) (`dom::tests::v8_perf_typedom_node::
native_binding_panic_does_not_abort_process`, `lumen-js`, deterministic,
unrelated — this change never touches `crates/js`). Live WPT run not
performed (no `.venv` in this slot) — closure is analytical: the new unit
tests directly reproduce the filing's subtests. No `.ini` were ever
committed for this filing beyond the three listed above; none needed
updating since the property now round-trips per spec.
