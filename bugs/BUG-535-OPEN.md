# BUG-535: `ruby-position: alternate` has no layout effect — annotations never flip over/under across stacked `<rtc>`s

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** layout (`crates/engine/layout/src/*` — ruby box layout)
**Найден:** P2, WPT-RUN-3 срез 26 (`css/css-ruby`) — массовый прогон

## Симптом

`ruby-position-alternate.html` sets `ruby-position` to `alternate`/
`alternate over`/`over alternate`/`alternate under`/`under alternate` on a
`<ruby>` with three stacked `<rtc>` annotation containers, then asserts (via
`getBoundingClientRect()`-based geometry helpers `assert_rt_is_over`/
`assert_rt_is_under`, not `getComputedStyle()`) that successive `<rtc>`s
alternate sides: first annotation over the base, second under, third over
again (or the mirrored under/over/under sequence for the `*under*` values).
All 7 subtests fail — the three annotations render in a fixed position
regardless of the `alternate` keyword.

This is distinct from the already-covered `ruby-*` gaps in the same slice:

- [BUG-472](BUG-472-OPEN.md) (computed style map) does **not** explain this
  file — the assertions read box geometry, not `getComputedStyle()` strings,
  and `ruby-position-valid.html`/`ruby-position.html` (parsing/basic
  positioning) both already pass 100%, so `ruby-position` **is** parsed and
  **is** applied for the plain `over`/`under`/`inter-character` values.
- [BUG-484](BUG-484-OPEN.md) (inline style setter validation) does not apply
  either — these are valid values being *set* successfully, just not
  producing the spec'd layout.

Confirmed by direct layout inspection (`lumen --dump-layout` on a minimal
`<ruby>`+`<rtc>` page): stacked `<rtc>` annotations are laid out as plain
stacked blocks in source order, with no alternation logic keyed off the
`ruby-position` computed value or the annotation's position among sibling
`<rtc>`s.

## Что нужно

Implement the CSS Ruby Layout `alternate` keyword: when `ruby-position`
computes to `alternate` (optionally combined with `over`/`under` to set the
starting side), successive ruby annotation containers of a ruby-base must
alternate sides (over/under) instead of all rendering on the same side.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-ruby/` for
`ruby-position-alternate.html`, `expected: FAIL` on all 7 subtests.
