# BUG-487: `revert-rule` CSS-wide keyword not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser (no `revert-rule` handling found anywhere in
`crates/engine/css-parser/src` or `crates/engine/layout/src`)
**Найден:** WPT-RUN-3 срез 6 (`ROADMAP.md`) — массовый прогон `css/css-cascade`

## Механизм

`revert-rule` (CSS Cascade L5 §`revert-rule`) is a CSS-wide keyword,
alongside `initial`/`inherit`/`unset`/`revert`/`revert-layer`, that rolls a
declaration back to the value it would have had from the previous matching
rule in cascade order (as opposed to `revert`, which rolls back to the next
*origin*, or `revert-layer`, which rolls back to the next *layer*). Grepped
`css-parser` and `layout` for `revert-rule`/`RevertRule`/`revert_rule`: zero
hits in either crate. `revert-layer` (a sibling, more established keyword)
**is** implemented and passes its own dedicated tests elsewhere in this
slice — so this is specifically the `revert-rule` value that's absent, not
the whole revert-family mechanism.

Practically: a declaration using `revert-rule` is presumably parsed as an
invalid value (or silently dropped) rather than being recognized as the
CSS-wide keyword it is, so the declaration never rolls back to the prior
rule — it just fails to apply at all, leaving whatever value the *next*
still-valid rule in cascade order provides (often the property's initial
value, which is why every observed failure is `assert_true: expected true
got false`, not a parse error surfaced to JS).

## Симптом

```
FAIL revert-rule in a custom property | assert_true: expected true got false
FAIL The revert-rule keyword rolls back to the previous rule | assert_true: expected true got false
FAIL Cascade order determines the previous rule, not order of application | assert_true: expected true got false
FAIL The revert-rule keyword can cross layers | assert_true: expected true got false
FAIL Combination of revert-rule and revert-layer | assert_true: expected true got false
```

## Масштаб находки

**4 files / 12 subtests cleanly attributable** in this slice — every
subtest's failure is exactly `assert_true: expected true got false` with no
other message shape, confirming `revert-rule` simply never rolls anything
back: `revert-rule-basic.html` (4), `revert-rule-custom-property.html` (1),
`revert-rule-layer.html` (2), `revert-rule-revert-layer.html` (5).

Two more files are **mixed** — `revert-rule` failures alongside unrelated
[BUG-384](BUG-384-OPEN.md) (named access on Window) failures in the same
file:
- `revert-rule-important.html`: 1 subtest is this bug (`assert_true`), 2 are
  BUG-384 (`test2`/`test3 is not defined`).
- `revert-rule-shadow.html`: 1 subtest is this bug (`assert_true`), 11 are
  BUG-384 (`slotted2`/`host3`…`host12 is not defined`).

## Что нужно

Recognize `revert-rule` as a CSS-wide keyword in `css-parser`'s value
grammar (same class of token as the already-handled `revert`/`revert-layer`)
and, in the cascade resolution in `layout`'s style computation, implement
its rollback semantics: walk cascade order (not origin, not layer) back to
the nearest earlier declaration for the same property that isn't itself
`revert-rule`, and use that value (or the property's initial value if none
exists). The existing `revert-layer` implementation is the closest
reference point for how origin/layer-scoped rollback is already wired.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-cascade/` for all 6
files (4 pure + 2 mixed, `expected: FAIL` on the whole file since the mixed
files have zero passing subtests either way).
