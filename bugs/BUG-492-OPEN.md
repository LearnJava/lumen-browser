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

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02) found the real home
category** — `css/css-backgrounds` is where upstream WPT actually puts the
bulk of `border-image` tests, not `css/css-borders`: **25 files / 1646
subtests**, by far the largest single-bug finding across the whole
WPT-RUN-3 track so far. Every `border-image-{source,slice,width,outset,
repeat}` longhand and the shorthand itself fail uniformly, split across
three shapes already anticipated by sl.8's `## Симптом`:

- **Interpolation/composition, 0 subtests passing per file** — the
  `animations/border-image-{outset,slice,source,width}-{interpolation,
  composition}.html` family (7 files, 1790 subtests alone —
  `border-image-width-interpolation.html` is the single largest failing
  file seen in the track to date at 558 subtests) — `CSS.supports(prop,
  val)` returns `false` for every value, same `interpolation-testcommon.js`
  `'from'/'to' value should be supported` assertion as sl.8, just now
  hitting the properties' true test home.
- **`parsing/border-image*-{computed,invalid,valid}.html`** (15 files) —
  the same `computed_style_to_map`/`_lumen_make_style` symptoms BUG-472/484
  already document for *implemented* properties, but here the root cause is
  simpler: the property doesn't exist at all, so both paths trivially fail.
- **Geometry/reset side effects** — `border-image-repeat_repeatnegx_none_50px.html`
  (element height wrong because `border-image-width: 50px` never applies)
  and `border-image-slice-shorthand-reset.html` (shorthand reset to initial
  never happens because the shorthand itself is unrecognized).

`discrete-no-interpolation.html` (35 of its 77 subtests) additionally
confirms `border-image-repeat` specifically, shared with
[BUG-463](BUG-463-OPEN.md) for the other 42 (WAAPI-not-supported) subtests
in the same file.

## .ini

Committed `.ini` for `border-image-width-interpolation-math-functions.html`
under `tests/wpt/metadata/css/css-borders/`, header referencing both this bug
and BUG-463. Срез 9 added `.ini` under `tests/wpt/metadata/css/css-backgrounds/`
for 25 more files (1646 subtests) — one file (`discrete-no-interpolation.html`)
shares its header with BUG-463.
