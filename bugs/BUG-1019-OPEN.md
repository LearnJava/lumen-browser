# BUG-1019: `resolution`/`min-resolution`/`max-resolution` media feature not implemented at all — no units, no `calc()`

**Статус:** OPEN
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
