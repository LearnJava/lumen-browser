# BUG-562: `position-try`/`position-try-fallbacks`/`position-try-order`/`@position-try` (CSS Anchor Positioning's fallback mechanism) are not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** css-parser/layout (no `"position-try"` / `"position-try-fallbacks"` / `"position-try-order"` / `@position-try` handling anywhere in `crates/engine/css-parser/src/` or `crates/engine/layout/src/style.rs`)
**Найден:** P2, WPT-RUN-3 срез 40 (`css/css-anchor-position`), 2026-08-04

## Симптом

Test setup scripts that touch the fallback mechanism (`@position-try` rule
insertion, `CSSPositionTryRule`, `element.style.positionTry*`) throw
synchronously and abort the rest of the top-level `<script>`, so later
`var`/`let` bindings the script would have assigned are simply never
initialized:

```
FAIL Element can be anchor positioned - main is not defined
FAIL top: 1px is allowed in @position-try - style is not defined
FAIL Query condition should be valid: anchored(fallback: any) -
  Cannot read properties of undefined (reading 'rules')
FAIL @position-try --foo { } should be a valid rule -
  Cannot read properties of undefined (reading 'append')
NOTRUN bottom: 1px is allowed in @position-try
```

Downstream `test()`/`promise_test()` closures registered *before* the abort
point still run (testharness.js drains its queue independently), so their
bodies see the never-assigned identifiers as genuine `ReferenceError`s —
hence the wide spread of distinct "`X` is not defined" messages (`main`,
`style`, `anchored`, `scroller`, `target`, `t1`, ...), one per file's own
local setup-variable name. Tests registered *after* the abort point never
run at all (`NOTRUN`). Combined this explains roughly 800 of the
~2860 checks in `css/css-anchor-position`.

## Причина

Verified by direct source read (not inferred from the WPT failures):
`grep -rn '"position-try"\|"position-try-fallbacks"\|"position-try-order"'
crates/engine/layout/src/style.rs` and the equivalent for `css-parser/src/`
both return nothing. Contrast with `crates/engine/layout/src/anchor.rs`'s
own module doc comment, which lists exactly which CSS Anchor Positioning L1
properties are wired (`anchor-name`, `position-anchor`, `inset-area`/
`position-area`, `anchor-scope`, `anchor-size()`) — the fallback vocabulary
(`position-try*`, `@position-try`, `CSSPositionTryRule`) is absent from that
list too. So this is not a partial/buggy implementation; the feature is
simply not started yet. `@position-try` isn't a recognized at-rule (so
`document.styleSheets[0].insertRule('@position-try …')` silently does
nothing or throws, depending on call site) and there is no
`CSSPositionTryRule` type at all.

## Масштаб

Not scoped further this slice — `position-try*`/`@position-try` is a
self-contained CSS Anchor Positioning L1 sub-feature (the "fallback"
mechanism, §4 of the spec) layered on top of the already-implemented
anchor-name/position-anchor/anchor()/position-area core. Implementing it is
a P4 (CSS properties) + parser (new at-rule) task, not a small fix.
