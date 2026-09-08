# BUG-1020: media query clause parser mishandles boolean-context features, unclosed parens and bare-word tokens containing `)`

**Статус:** FIXED 2026-09-07 (P3)
**Дата:** 2026-09-06
**Компонент:** css-parser (`crates/engine/css-parser/src/parser/media.rs` —
`parse_media_clause`, `parse_media_feature`)
**Найден:** P3 2026-09-06, побочно при закрытии [BUG-526](BUG-526-FIXED.md)
(`MediaQueryList.media` serialization)

## Механизм

`parse_media_clause`/`parse_media_feature` are a small hand-rolled parser,
not a CSS-Syntax-conformant tokenizer, and three of its shortcuts produce
wrong results (not just "unsupported", genuinely incorrect) on malformed or
boolean-context input:

1. **No boolean-context media features.** `parse_media_feature` requires a
   `:` (`s.split_once(':')`, `media.rs:661-664`) — a feature referenced by
   name alone in a boolean context (`(color)`, meaning "true if the value
   of `color` is non-zero", CSS Values L4 §Boolean Context) has no `:` and
   is unconditionally `Unsupported`, even though `color` itself is a
   perfectly ordinary, already-relevant media feature.
2. **No unclosed-paren auto-close.** `parse_media_clause`'s paren-content
   scan (`media.rs:562-574`) returns the whole clause as `Unsupported` when
   no closing `)` is found before the string ends. Per CSS Syntax L3
   "consume a component value", an unterminated `(...)` block is still
   consumed up to EOF and treated as implicitly closed — `"(color"` is
   valid syntax equivalent to `"(color)"`, not an error.
3. **Bare-word scan doesn't stop at `)`.** The non-paren branch
   (`media.rs:576-578`) finds the word boundary via
   `find(|c| c.is_whitespace() || c == '(' || c == ',')` — `)` is missing
   from that set, so a media-type-position word followed by a stray `)`
   (`"color)"`, no matching open paren at all) is captured whole,
   including the `)`, as a literal `MediaCondition::MediaType("color)")`
   instead of being recognized as a syntax error.

These three interact in the same WPT file, e.g. `"  color ), ( color"`
should split at the `,` into `"  color )"` (clause 1: bad syntax → `not
all`) and `" ( color"` (clause 2: unclosed boolean-context feature →
`(color)`) — today it becomes a single nonsensical
`MediaCondition::MediaType("color and )")`-shaped mess because none of the
three behaviors are correct.

## Симптом

Confirmed via `parse_media_query(q).serialize()` probe (7 subtests of
`css/mediaqueries/match-media-parsing.html`, all pre-existing parsing
behavior — not a regression, uncovered by [BUG-526](BUG-526-FIXED.md)
giving `.media` a real (non-echo) value for the first time):

| query | expected `.media` | got |
|---|---|---|
| `(color` | `(color)` | `not all` |
| ` (color)` | `(color)` | `not all` |
| ` ( color  )  ` | `(color)` | `not all` |
| ` ( color   ` | `(color)` | `not all` |
| `color)` | `not all` | `color)` |
| `  color)` | `not all` | `color)` |
| `  color ), ( color` | `not all, (color)` | `color and ), not all` |

Matching is affected too, not just serialization: `matchMedia("color)")`
should never match (invalid syntax) but currently matches any page whose
resolved `ctx.media_type` string happens to equal the literal `"color)"`,
which can't happen legitimately, so today it always silently evaluates to
`false` by accident rather than by a correct invalid-syntax rule — fixing
tokenization must preserve that `false`, just for the right reason.

## Масштаб

7 subtests of `match-media-parsing.html`.

## Что нужно

- Add `)` to the bare-word delimiter set in `parse_media_clause`'s non-paren
  branch, and treat a word ending in an unmatched `)` (or a lone `)` with
  no preceding `(` in the clause) as `Unsupported` rather than a literal
  media-type.
- On missing closing `)`, treat the remainder of the input as the paren's
  content instead of failing the whole clause (`input.find(')')` →
  `.unwrap_or(input.len())`, with `input` left empty afterward instead of
  sliced past a nonexistent index).
- Add boolean-context support to `parse_media_feature`: when `val` (from
  `s.split_once(':')`) is absent, treat `s` as a bare feature name; for
  every feature already representable as a range (`color`, `width`,
  `resolution` since [BUG-1019](BUG-1019-FIXED.md) landed, …), boolean context
  means "supported and non-zero" — for `color` specifically (not yet a
  `MediaFeature` variant at all), that's just "supported", since Lumen's
  color depth is fixed.

## .ini

`tests/wpt/metadata/css/mediaqueries/match-media-parsing.html.ini` carried
the 7 subtests above as `expected: FAIL`, attributed to this bug — file
deleted (2026-09-07 fix), the whole file now passes.

## Срез P3 2026-09-07: fixed

All three parser shortcuts fixed in `parse_media_clause`/`parse_media_feature`
(`crates/engine/css-parser/src/parser/media.rs`):

- Unclosed `(...)`: `find_matching_close_paren` returning `None` now falls
  back to `(input.len(), input.len())` instead of failing the whole clause —
  the remainder of the input becomes the feature's content, no closing `)`
  left to skip.
- Bare-word scan: `)` added to the delimiter set; a word immediately
  followed by `)` (including the degenerate empty-word case, a lone stray
  `)`) makes the whole clause `Unsupported` instead of swallowing the `)`
  into a literal `MediaType`.
- Boolean context: `parse_media_feature` now handles the no-`:` case by
  matching the bare name against `"color"` → `MediaFeature::Color` (new
  variant, `matches()` always `true` — Lumen's color depth is fixed,
  `serialize()` emits the bare `"color"`, no `: value`). General
  boolean-context support for range features (`width`/`resolution`/…) is
  **not** in scope here — that is [BUG-527](BUG-527-FIXED.md)'s broader,
  still-open item; a regression test
  (`media_query_boolean_context_width_still_unsupported`) pins the boundary
  so a future BUG-527 fix has to touch that test deliberately, not
  accidentally widen this one.

All 7 targeted subtests pass by construction (traced by hand against the
table in §Симптом); the whole `match-media-parsing.html` file (25 subtests)
was additionally transcribed line-by-line into 6 new unit tests
(`crates/engine/css-parser/src/parser/tests/at_rules.rs`, prefixed
`media_query_parsing_*`/`media_query_boolean_*`) — including the lines that
already passed, so the fix can't be shown to silently regress them.
`cargo test -p lumen-css-parser --lib`: 438/438 (+6). `cargo clippy
-p lumen-css-parser --all-targets -- -D warnings`: чист. No other crate
matches on `MediaFeature` variants (`grep -rn "MediaFeature::"` outside
`css-parser`: zero), so the new `Color` variant needed no other call site
updates. `.ini` deleted (whole file now passes).
