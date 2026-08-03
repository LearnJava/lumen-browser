# BUG-527: several CSS Media Queries L4/L5 discrete-valued features are
unimplemented, and the boolean-context form (`(feature)` with no `: value`)
is unsupported for every feature, even ones whose value form works

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (`crates/engine/css-parser/src/parser.rs:2291-2400`
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

## Фикс (не сделан)

Two separate work items: (a) give every already-implemented discrete
feature a boolean-context branch in `parse_media_feature` (bare feature
name → the "has a meaningful/non-initial value" test, per-feature — a
mechanical addition once `split_once` fails, not a redesign), and (b)
implement the 9 missing features' value-form match arms (mostly
straightforward enum matches like the existing `prefers-contrast`/
`scripting` arms; `overflow-inline`/`overflow-block` and
`video-dynamic-range` need a `MediaContext` field to source the answer
from, `display-mode`/`resizable` need shell-provided window-state, `update`
is effectively always `"fast"` for a non-printing document per spec note).
