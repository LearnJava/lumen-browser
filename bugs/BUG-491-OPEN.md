# BUG-491: CSS Borders and Box Decorations Level 4 draft properties entirely
unimplemented (`corner-shape`, `border-shape`, `box-shadow-{offset,color,blur,
spread,position}`, `border-*-radius` pair shorthands, `border-clip`, `hairline`
keyword)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (`crates/engine/layout/src/style.rs::apply_declaration`,
`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)
**Найден:** WPT-RUN-3 срез 8 (`ROADMAP.md`) — массовый прогон `css/css-borders`

## Механизм

`apply_declaration` (`style.rs:14334`) is one large `match prop { ... }` over
recognized property names, ending in a catch-all `_ => {}` (`style.rs:17355`)
that silently drops any declaration whose property name isn't one of the
matched arms — normal CSS error-recovery behavior for a property the engine
doesn't implement. None of the following draft `css-borders-4` names have a
match arm (confirmed by `grep -rni` across `crates/css-parser/src/`,
`crates/engine/layout/src/`, `crates/engine/paint/src/` — zero hits for any
of them, only unrelated `hairline_aa` rasterizer-internal symbol names):

- `corner-shape` and its 8 physical/logical per-corner longhands
  (`corner-top-left-shape`, `corner-top-right-shape`, `corner-bottom-left-shape`,
  `corner-bottom-right-shape`, `corner-start-start-shape`, `corner-start-end-shape`,
  `corner-end-start-shape`, `corner-end-end-shape`) plus the edge/corner-pair
  shorthands (`corner-top`, `corner-right`, `corner-bottom`, `corner-left`,
  `corner-block-start`, `corner-block-end`, `corner-inline-start`,
  `corner-inline-end`, `corner-top-left`, `corner-top-right`, …) and the
  `border-shape` shorthand that sets `border-radius` + `corner-shape`
  together — `css/css-borders/corner-shape/`, `css/css-borders/border-shape/`,
  `css/css-borders/tentative/parsing/border-shape-*`.
- Standalone `box-shadow` component longhands introduced by this draft:
  `box-shadow-offset`, `box-shadow-color`, `box-shadow-blur`,
  `box-shadow-spread`, `box-shadow-position` — distinct from the `box-shadow`
  *shorthand*, which is already ✅ implemented (`CSS-SPECS.md:198`) —
  `css/css-borders/tentative/parsing/box-shadow-*`.
- Physical/logical corner-pair radius shorthands: `border-top-radius`,
  `border-right-radius`, `border-bottom-radius`, `border-left-radius`,
  `border-block-start-radius`, `border-block-end-radius`,
  `border-inline-start-radius`, `border-inline-end-radius` —
  `css/css-borders/tentative/parsing/border-*-radius-*` (the underlying
  physical longhands `border-top-left-radius` etc. this shorthand expands
  into are already ✅ implemented — this is only the paired shorthand).
- `border-clip` — `css/css-borders/tentative/parsing/border-clip-*`.
- The `hairline` keyword for `<line-width>` (`border-width: hairline`,
  `outline-width: hairline`) — `css/css-borders/border-width-hairline.html`
  (only inferred from source grep this slice; the file's one real failure was
  attributed to BUG-384 instead, see below).

`computed_style_to_map` (`selector_query.rs:625+`) mirrors the same gap on
the read side: none of the above keys are inserted into the map, so
`getComputedStyle()` silently returns `""` for them (same fixed-map
mechanism as BUG-472, but here the underlying `ComputedStyle` struct has no
field for the value at all, not just a missing map entry — implementing
BUG-472's fix alone would not close this).

## Симптом

Three converging signals per property, all confirmed against this slice's
structured wptreport (`tests/wpt/run_report.py`'s `--log-wptreport` JSON,
**not** the raw multi-process wptrunner stdout log — see `docs/wpt-status.md`
→ `css` row, срез 8, "Технический гоча" for why the two disagree):

1. `getComputedStyle(el).getPropertyValue(prop) === ""` unconditionally →
   `computed-testcommon.js`'s `test_computed_value` fails with "`<prop>`
   doesn't seem to be supported in the computed style" — 23 files, ~200
   subtests, the cleanest signal (`corner-computed.html`,
   `corner-shape-computed.html`, `border-shape-computed.html`,
   `border-*-radius-computed.html` ×8, `box-shadow-*-computed.html` ×5,
   `border-shape-ignore-radius-computed.html`,
   `corner-block-start-writing-modes.html`).
2. `CSS.supports(prop, value) === false` for values that are actually valid
   per spec → `interpolation-testcommon.js`'s `'from'/'to' value should be
   supported` assertion fails (`corner-shape-interpolation.html`, 478
   subtests — the single largest subtest cluster in this slice;
   `border-shape/border-shape-animation.html`, 100 subtests) and
   `shorthand-testcommon.js`'s `test_shorthand_value`'s "should not set
   unrelated longhands" sub-check (`assert_true(CSS.supports(property,
   value))`) fails the same way inside several `corner-*-valid.html` files.
3. Rendering-geometry checks against an unimplemented visual effect fail with
   the feature's default no-op result: `corner-shape-outside-left.html`/
   `corner-shape-outside-right.html` (`shape-outside` interaction with
   `corner-shape` — 8 subtests each, `assert_array_approx_equals` expects a
   notch/bevel/round-shaped float exclusion, gets the plain rectangular one).

Two files are **not** attributed to this bug despite touching the same
properties: `border-image-width-interpolation-math-functions.html` tests a
different, non-draft property (`border-image-width`, CSS Backgrounds and
Borders **Level 3**, not this Level 4 draft — filed separately as
[BUG-492](../bugs/BUG-492-OPEN.md), not to conflate a mature spec gap with a
speculative-draft one); `border-width-hairline.html`'s one real failure is
[BUG-384](../bugs/BUG-384-FIXED.md) (named window access on a bare
identifier), not this bug.

## Не в скоупе этого бага

The *-invalid.html "should reject garbage" failures and the *-valid.html
"serialization should be canonical" / "should set `<longhand>`" failures for
these same properties are **not** this bug — they're
[BUG-484](../bugs/BUG-484-OPEN.md) (`_lumen_make_style`'s inline style setter
never routes through the real CSS parser at all, for *any* property,
implemented or not: it always stores the raw string and echoes it back
verbatim). Evidence this is BUG-484 and not a symptom of the property being
unimplemented: 13 `*-valid.html` files in this same slice pass **fully**
(`border-block-end-radius-valid.html` 8/8, all four `box-shadow-*-valid.html`
except `color`, etc.) purely because the WPT author happened to write the
already-canonical serialization as the test input — an accidental round-trip
through BUG-484's raw echo, not real parsing. Implementing this bug (the
properties themselves) would not by itself fix BUG-484's echo/no-validation
behavior for them, and vice versa.

## Масштаб находки

Confirmed via source grep only for `css/css-borders`, one small, closed
surface (a still-draft CSSWG module) — not cross-cutting like BUG-472/477/483
/484/488. No action needed outside this bug's own scope.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-borders/` for the files in
this slice fully or partially explained by this bug (see slice write-up in
`docs/wpt-status.md`).
