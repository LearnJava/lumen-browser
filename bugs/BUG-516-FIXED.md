# BUG-516: `overscroll-behavior-block`/`-inline` logical longhands not recognized by the parser

**Статус:** FIXED 2026-09-05
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -n "overscroll-behavior-block\|
overscroll-behavior-inline" crates/engine/css-parser/src/lib.rs
crates/engine/layout/src/style.rs` — zero hits; contrast with the physical
`overscroll-behavior-x`/`-y`, which are both present:
`css-parser/src/lib.rs:256-258` lists `overscroll-behavior`/`-x`/`-y` as
known properties, but not `-block`/`-inline`, and `layout/src/style.rs`
only defines `ComputedStyle::overscroll_behavior_x`/`_y` fields — no
`_block`/`_inline` counterparts and no logical→physical resolution entry
for this property family, unlike e.g. `margin-block`/`inset-block` which do
have a `resolve_logical_properties` mapping)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-overscroll-behavior`

## Механизм

CSS Overscroll Behavior L1 §2 defines `overscroll-behavior-block`/`-inline`
as the flow-relative (writing-mode-aware) siblings of `-x`/`-y`, resolving
to the physical properties the same way `margin-block-start` resolves to
`margin-top`/`margin-left` depending on `writing-mode`/`direction`. Unlike
[BUG-472](BUG-472-OPEN.md) (a property parses and stores correctly but its
value never reaches `getComputedStyle`'s resolved-value map), this is one
level deeper: the parser's known-property table doesn't contain the logical
names at all, so a `overscroll-behavior-block: contain` declaration is
dropped as an unrecognized property before it can affect anything,
including the physical `-x`/`-y` fields it should resolve into.

## Симптом

```
FAIL Logical overscroll-behavior maps correctly when element has
  horizontal-tb writing mode
  assert_equals: expected "none" but got ""
FAIL Property overscroll-behavior-block has initial value auto
  assert_true: overscroll-behavior-block doesn't seem to be supported in
  the computed style expected true got false
```

## Масштаб находки

3 files / 15 subtests: `overscroll-behavior-logical.html` (3, the dedicated
flow-relative-mapping test — even `getComputedStyle(el).overscrollBehaviorX`
reads empty, confirming the logical declaration never reached the physical
field at all, not just the resolved-value map), `inheritance.html` (4 of its
8 fails — the `-block`/`-inline` half; the other 4, `-x`/`-y`, are
[BUG-472](BUG-472-OPEN.md)), `parsing/overscroll-behavior-computed.html` (8
of its 16 fails, same split). The remaining `-block`/`-inline` subtests in
`parsing/overscroll-behavior-invalid.html` fail on the separate, generic
inline-`style`-setter gap ([BUG-484](BUG-484-OPEN.md)) instead, since that
gap accepts any string regardless of whether the property name is
recognized.

## Что нужно

Add `overscroll-behavior-block`/`-inline` to the parser's known-property
table, add matching `ComputedStyle` storage (or resolve directly into the
existing `overscroll_behavior_x`/`_y` fields at style-computation time,
mirroring how other logical-property families are resolved against
`writing-mode`/`direction` in `style.rs`), then extend
[BUG-472](BUG-472-OPEN.md)'s `computed_style_to_map` fix to also expose the
two logical names once they resolve correctly.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-overscroll-behavior/` for
all 3 files, `expected: FAIL` per subtest.

## Фикс (2026-09-05)

Added `overscroll-behavior-block`/`-inline` to `SUPPORTED_PROPERTIES`
(`css-parser/src/lib.rs`), two new `ComputedStyle` fields (default `Auto`,
same "field != default means explicitly set" heuristic as every other
logical property in the file), two `apply_declaration` branches
(`style/apply/motion.rs`, next to the existing `-x`/`-y` branches — same
`parse_overscroll_behavior`, no new value grammar), and a new
`resolve_overscroll_behavior_logical_properties` (`style/logical.rs`),
called from `cascade.rs` right after `resolve_overflow_logical_properties` —
same shape and same writing-mode axis-swap as that BUG-505 function
(`-block` → `_y` in `horizontal-tb`, → `_x` in every vertical mode).
`selector_query.rs`'s computed-style map gained the two logical keys,
reading back the resolved physical value with the same axis swap.

**Verified live** (`run_smoke.py`, dev-release build):
`overscroll-behavior-logical.html` 3/3 (was 0/3); `inheritance.html`
`overscroll-behavior-block`/`-inline` "has initial value auto" now pass;
`parsing/overscroll-behavior-computed.html` `overscroll-behavior-block`/
`-inline` value `'auto'` now pass. All three `.ini` updated to drop the
now-passing expectations.

**Correction to the original filing's attribution**: the live rerun showed
the *physical* `-x`/`-y` "value 'auto'" subtests in
`overscroll-behavior-computed.html` and "has initial value auto" in
`inheritance.html` were **also** failing before this fix — not just the
logical half as originally split. They now pass too (unaffected by this
diff — likely a pre-existing `.ini` staleness, not caused by this bug or
its fix). The remaining failures in both files (`contain`/`none`/`chain`
values, "does not inherit") fail identically for physical and logical
forms alike — that shared remainder is [BUG-472](BUG-472-OPEN.md)
(`getComputedStyle` resolved-value gaps, ДОРАБОТКА → CSSOM-3), not
BUG-516; both `.ini` files were re-attributed accordingly.
`parsing/overscroll-behavior-{valid,invalid}.html` were not touched — their
`-block`/`-inline` subtests are governed by [BUG-484](BUG-484-OPEN.md), as
the original filing already noted, and are unaffected by this fix.
