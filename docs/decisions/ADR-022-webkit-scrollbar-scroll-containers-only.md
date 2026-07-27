# ADR-022: `::-webkit-scrollbar*` applies to scroll containers only

## Status

Accepted

## Date

2026-07-27

## Context

CC-CSS-1 (2026-07-24) taught Lumen the legacy WebKit scrollbar pseudo-elements
by *translating* them: at the end of `compute_style`, three extra pseudo-element
cascades (`::-webkit-scrollbar`, `-thumb`, `-track`) run and their results are
written onto the standard `scrollbar-width` / `scrollbar-color` fields, which
`lumen_paint::display_list` already honours.

Both of those standard properties are **inherited** (CSS Scrollbars L1 §2). The
translation therefore had two properties the original WebKit feature does not:

* it ran for **every element**, since any element can carry the inherited value;
* a rule matching a *non-scrollable* element leaked its result to every
  descendant, including scrollable ones that matched no rule of their own.

BUG-341's S10 profile measured the cost of the first property on the CC-12
chrome fixture: the three cascades were **55% of `compute_style`** (11.95 ms per
hover pass over 828 elements), because `assets/chrome/chrome.html` writes the
rules *bare* (`::-webkit-scrollbar { … }`, universal subject) so all three
matched on every element. S10 removed the sheets that declare no such rule and
added a reuse fast path, leaving ~5.8 ms plus ~2.4 ms of fast-path bookkeeping;
CC-12's 2 ms budget is still ~8-20× away.

In WebKit itself, `::-webkit-scrollbar` styles the scrollbar of the element it
matches. There is no inheritance to descendants — that is purely an artifact of
Lumen translating a pseudo-element onto standard inherited properties.

## Decision

The translation runs **only for elements that can show a scrollbar**:
`overflow-x` or `overflow-y` is `scroll` or `auto` (the exact condition
`lumen_paint::display_list::emit_scrollbars` and
`box_tree::scrollbar_gutter_{inline,block}` use), plus the root element and
`<body>` unconditionally — those are the conventional target for styling the
*page* scrollbar, and including two elements per document keeps that idiom
working if the viewport scrollbar ever reads its style from them.

`element_can_have_scrollbar` in `crates/engine/layout/src/style.rs` is the single
definition. The standard `scrollbar-width` / `scrollbar-color` properties are
untouched and keep inheriting normally.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep the per-element translation, memoise the parent's pseudo-inheritance base by pointer | Exact and semantics-preserving, but buys only the ~2.4 ms of fast-path bookkeeping — the 5.8 ms of real cascades for elements whose base differs from their parent's stays. |
| Stop treating `scrollbar-width`/`scrollbar-color` as inherited | Would fix the leak at its root, but contradicts CSS Scrollbars L1 §2 and breaks the standard properties for everyone to fix a non-standard one. |
| Store the translated values outside `ComputedStyle`, keyed by node | A parallel, non-inherited channel for two fields; more machinery than the feature is worth, and paint would need a second lookup path. |
| Leave it alone | The largest single item left on `CC12_HOVER`, on a track whose whole purpose is closing that gate. |

## Consequences

- **Positive:** the three cascades run for a handful of elements instead of all
  of them; the translation now matches WebKit's own scoping, so a
  `.panel::-webkit-scrollbar` rule no longer restyles unrelated scrollable
  descendants.
- **Positive:** no visible change for the common idioms — a bare
  `::-webkit-scrollbar` still matches `<body>` and reaches the page through
  ordinary inheritance, and a rule written for a scroll container still matches
  it directly. Lumen's own chrome and both graphic tests (51, 1000000-final)
  style scroll containers directly and are unaffected.
- **Negative / trade-off:** a page that deliberately relied on Lumen's leak —
  a `::-webkit-scrollbar` rule on a non-scrollable ancestor, expecting a
  scrollable descendant to inherit it — loses that. No real-world engine has
  ever behaved that way, so this is a fidelity fix, not a regression, but it is
  a behaviour change and is called out as such (user decision, 2026-07-27).
- **Future:** if a viewport scrollbar is ever painted from the root box's own
  style, revisit whether the unconditional `html`/`body` inclusion is still the
  right shape or should become a real "is the viewport scroll container" test.
