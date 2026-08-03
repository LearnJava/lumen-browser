# BUG-526: `MediaQueryList.media` doesn't serialize per the Media Queries
serialization algorithm — it just echoes the raw input string verbatim

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs:10537-10543` — `MediaQueryList` constructor)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/mediaqueries`

## Механизм

```js
function MediaQueryList(media) {
    ...
    this.media = String(media == null ? '' : media);   // dom.rs:10540
    ...
}
```

`window.matchMedia(query).media` is spec'd (Media Queries §Serializing a
media query list) to run the query through the parser and re-serialize the
result: normalize/collapse whitespace, replace every invalid media query in
a comma-separated list with the literal string `not all`, drop redundant
parens/spacing, and canonicalize units (e.g. a `resolution` given as `x`
serializes as `dppx`; `calc()` expressions are simplified numerically). The
shim instead stores the constructor argument as-is with no processing at
all — `media` is a pure echo of whatever string was passed to
`matchMedia()`.

## Симптом

Every case in `css/mediaqueries/match-media-parsing.html` that isn't a
already-canonical single valid query fails: whitespace isn't trimmed/
collapsed (`" foo "` → expected `"foo"`, got unchanged), invalid clauses in
a list aren't replaced with `not all` (`",,"` → expected `"not all, not
all, not all"`, got unchanged `",,"`), and `calc()` inside `resolution`
isn't simplified/unit-canonicalized (`calc(1x)` → expected
`calc(1dppx)`, got unchanged `calc(1x)`). 25 subtests in that one file, plus
`aspect-ratio-serialization.html` (`1/3` → expected `1 / 3`, spacing not
added) and `mq-escaped-serialization.html` (1 subtest, CSS escape
normalization) — 27 subtests total this slice.

## Фикс (не сделан)

Implement the serialization algorithm against the already-parsed
`MediaQuery`/`MediaQueryClause`/`MediaFeature` AST (`crates/engine/
css-parser/src/parser.rs`) rather than storing the raw string: add a
`Display`/`to_css_string` impl that walks the parsed clauses, emits `not
all` for any clause containing `MediaCondition::Unsupported`, and
canonicalizes each feature's value (unit, whitespace) on the way out. Wire
that string into `_lumen_match_media`'s Rust side and return it alongside
the boolean match result so the JS constructor can store the serialized
form instead of the raw input.
