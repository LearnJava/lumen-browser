# BUG-492: `border-image` (CSS Backgrounds and Borders Level 3) entirely
unimplemented — not just `border-image-width`

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (`crates/engine/layout/src/style.rs::apply_declaration`)
**Найден:** WPT-RUN-3 срез 8 (`ROADMAP.md`) — массовый прогон `css/css-borders`

## Механизм

`grep -rn "border-image\|border_image" crates/css-parser/src/
crates/engine/layout/src/` returns **zero matches** anywhere in the
workspace — `border-image` and its whole longhand family
(`border-image-source`, `border-image-slice`, `border-image-width`,
`border-image-outset`, `border-image-repeat`) fall through
`apply_declaration`'s catch-all `_ => {}` (`style.rs:17355`), same mechanism
as [BUG-491](../bugs/BUG-491-OPEN.md), but this is a mature, non-draft
module (CSS Backgrounds and Borders Level 3 — the same level as `box-shadow`,
already ✅) rather than the still-draft Level 4 this slice's other properties
belong to, so it's tracked separately: closing BUG-491 would not touch this
gap and vice versa, and `CSS-SPECS.md` currently has no row at all for
`border-image` (neither ✅ nor ⬜) — this is the first WPT signal that it's
missing from the P4 property roadmap entirely.

## Симптом

Only one file in `css/css-borders` exercises `border-image` directly:
`border-image-width-interpolation-math-functions.html` (0/48 subtests). Its
failures split into two independent causes, both already-known-class
mechanisms applied to this newly-discovered property:

- 36 subtests: `CSS.supports('border-image-width', <value>)` returns `false`
  even for the trivially valid value `100` — confirms the property itself is
  unrecognized (this bug), via the same `interpolation-testcommon.js`
  `'from'/'to' value should be supported` assertion BUG-491 hits.
- 12 subtests: `assert_true('animate' in Element.prototype, 'Web Animations
  should be supported')` fails — this is [BUG-463](../bugs/BUG-463-OPEN.md)
  (WAAPI `animate` not installed on `Element.prototype`), unrelated to
  `border-image` itself; extend, not new.

## Масштаб находки

Not scoped further this slice — only the one interpolation test file in
`css/css-borders` touches `border-image`; the property's own dedicated test
directory (`css/css-backgrounds` or similar upstream location) is out of
this slice's `--root css/css-borders` scope and untriaged.

## .ini

Committed `.ini` for `border-image-width-interpolation-math-functions.html`
under `tests/wpt/metadata/css/css-borders/`, header referencing both this bug
and BUG-463.
