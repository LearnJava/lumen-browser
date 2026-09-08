# BUG-527: several CSS Media Queries L4/L5 discrete-valued features are
unimplemented, and the boolean-context form (`(feature)` with no `: value`)
is unsupported for every feature, even ones whose value form works

**Статус:** FIXED 2026-09-08 (P3)
**Дата:** 2026-08-03
**Компонент:** css-parser (`crates/engine/css-parser/src/parser/media.rs` —
`parse_media_feature`)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/mediaqueries`

## Механизм

```rust
fn parse_media_feature(s: &str) -> MediaCondition {
    // `feature: value` или просто `feature` (boolean feature, не поддерживаем).
    let Some((key, val)) = s.split_once(':') else {
        return MediaCondition::Unsupported;
    };
    ...
```

Two distinct gaps in the same function, confirmed by grep + live probe:

1. **Boolean context is entirely unimplemented.** Per Media Queries L4
   §4.1, a discrete-valued feature used bare (no `: value`) must be tested
   as "does this feature apply/have a non-`none`/non-`0` value" — e.g.
   `(scripting)` should be true whenever `(scripting: enabled)` would be.
   The code above returns `Unsupported` (→ never matches) for *any* bare
   feature the moment it fails to find a `:`, regardless of whether the
   value form of that same feature is otherwise implemented. Confirmed
   live: `matchMedia('(scripting)').matches` → `false` even though
   `scripting` is enabled and `(scripting: enabled)` matches correctly.
2. **Several features aren't recognized at all, in either form** (`grep`
   confirms zero mentions in `parser.rs` beyond the catch-all): `display-mode`,
   `display-state`, `resizable`, `dynamic-range`, `video-dynamic-range`,
   `update`, `navigation-controls`, `overflow-inline`, `overflow-block`. For
   these the value form is *also* broken, not just the boolean form.

## Симптом

Every "Check that X evaluates to true in the boolean context" test across
`css/mediaqueries` fails (`display-mode.html`, `display-state.tentative.html`,
`dynamic-range.html`, `prefers-color-scheme.html`, `resizable.tentative.html`,
`scripting.html`, `update-media-feature.html`, `navigation-controls.tentative.html`
— 12 files, ~40 subtests this slice), plus every value-form parseability
check for the 9 entirely-missing features above (`overflow-media-features.html`
alone: `overflow-inline: none/scroll`, `overflow-block: none/scroll/paged`
all `MediaCondition::Unsupported`).

## Фикс

Two separate work items done together, since both live in the same
`parse_media_feature`/`MediaFeature` shape:

(a) **Boolean context, generalized.** New `BooleanFeature` enum lists every
already-implemented discrete feature (`scripting`, `prefers-color-scheme`,
`forced-colors`, `inverted-colors`, `prefers-reduced-data`,
`prefers-contrast`, `prefers-reduced-motion`, `prefers-reduced-transparency`,
`orientation`, `hover`/`any-hover`, `pointer`/`any-pointer`, plus the 9 new
features below); `BooleanFeature::matches` re-derives the true/false answer
straight from `MediaContext` per feature (spec-defined "off" state: e.g.
`prefers-reduced-motion` is boolean-true only if the user actually prefers
reduced motion, not merely because the feature is recognized). Kept as its
own small enum — carrying the answer logic once — rather than one
`MediaFeature` variant per feature repeating the same "bare form" shape.
Range features (`width`/`resolution`/`aspect-ratio`/…) are explicitly out
of scope: they need `<`/`<=`/`>`/`>=` comparison grammar, not a discrete
match, and stay `Unsupported` bare (regression-tested).

(b) **9 missing features implemented in both forms.** `display-mode`,
`display-state`, `resizable`, `dynamic-range`, `video-dynamic-range`,
`update`, `navigation-controls`, `overflow-inline`, `overflow-block` — new
`MediaFeature` variants + matching `MediaContext` fields with desktop
defaults (`browser`/`normal`/resizable=true/`standard` dynamic range/`fast`
update/`back-button` navigation/`scroll` overflow — no PWA mode, no HDR, no
print pagination outside the dedicated print path). `dynamic-range` and
`video-dynamic-range`'s boolean form is a spec-mandated exception: it tests
whether HDR is actually available (`high`), not merely whether the feature
is recognized — same status as `none`/`no-preference` elsewhere, so it's
`false` by default even though the value form is known.

19 новых юнит-тестов (value-form parseability для всех 9 фич + boolean
context для них и для всех уже реализованных discrete-фич + сериализация
round-trip). `cargo test -p lumen-css-parser --lib`: 457/457 (+19).
`cargo clippy -p lumen-css-parser --all-targets -- -D warnings`: чист.

Найден P2, WPT-RUN-3 срез 24, 2026-08-03
