# BUG-528: a comma-separated media query list where every individual clause
fails to parse evaluates as matching (`@media all`) instead of never
matching (`not all`)

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (`crates/engine/css-parser/src/parser.rs:1664-1672`
`MediaQuery::matches` — the `self.clauses.is_empty() => true` fallback)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/mediaqueries`

## Механизм

```rust
impl MediaQuery {
    /// Пустой query (= `@media all`) — true. ...
    pub fn matches(&self, ctx: &MediaContext) -> bool {
        if self.clauses.is_empty() {
            return true;
        }
        self.clauses.iter().any(|clause| clause.matches(ctx))
    }
}
```

The empty-clauses-means-`@media all` shortcut is correct for a literal
empty query (`@media {}` / `matchMedia('')`), but the query-list *splitter*
apparently also produces zero clauses when every comma-separated entry in a
non-empty string fails to parse into a clause at all (as opposed to
parsing into a clause containing `MediaCondition::Unsupported`, which
correctly evaluates to non-matching). Confirmed live:

```js
matchMedia('overflow-inline').matches                              // false (single bare token alone)
matchMedia('overflow-inline, not all and overflow-inline').matches // true  (same token, comma-listed)
matchMedia('totally-bogus-garbage-xyz').matches                    // false (single bare token alone)
```

A single invalid bare token alone correctly evaluates `false`, but the same
token appearing as one alternative in a *comma-separated list* (here paired
with `not all and overflow-inline`, itself also unparseable) flips the
overall result to `true` — the two failed-to-parse entries apparently never
get pushed onto `self.clauses` as `Unsupported`-bearing clauses, leaving
the list's clause vector empty and triggering the `@media all` fallback.

## Симптом

`css/mediaqueries/overflow-media-features.html`'s `query_should_be_unknown`
helper (`resources/matchmedia-utils.js`) builds exactly this shape
(`` `${query}, not all and ${query}` ``) to probe whether `query` is
recognized at all — for a genuinely unknown bare feature name this makes
every such probe report "known" instead of "unknown". 2 subtests this
slice (`overflow-inline`, `overflow-block`); likely latent in any other
category using the same `matchmedia-utils.js` helper once
[BUG-527](BUG-527-OPEN.md)'s 9 missing features are exercised through it.

## Фикс (не сделан)

Find the comma-list-splitting site that builds `MediaQuery.clauses` and
make a clause that fails to parse push a `MediaQueryClause` containing
`MediaCondition::Unsupported` (which already correctly evaluates to
non-matching per `MediaQueryClause::matches`) instead of being silently
dropped — the `is_empty()` fallback should then only ever trigger for a
truly empty source string.
