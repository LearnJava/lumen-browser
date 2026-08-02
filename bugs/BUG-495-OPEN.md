# BUG-495: `background-position-x`/`background-position-y` standalone longhands entirely unimplemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser/layout (`crates/engine/layout/src/style.rs::apply_declaration`,
`selector_query.rs::computed_style_to_map`)
**Найден:** WPT-RUN-3 срез 9 (`ROADMAP.md`) — массовый прогон `css/css-backgrounds`

## Механизм

`grep -rn "background-position-x\|background_position_x" crates/css-parser/src/
crates/engine/layout/src/` returns **zero matches** anywhere in the
workspace. Only the `background-position` *shorthand* is implemented
(`style.rs:15860`); the two standalone longhands added by CSS Backgrounds
and Borders Level 4 (`background-position-x`, `background-position-y`) have
no match arm in `apply_declaration` and no entry in
`computed_style_to_map` — distinct from [BUG-472](BUG-472-OPEN.md) (which
covers properties that *are* implemented but missing from the computed-style
map): here `CSS.supports('background-position-x', <any value>)` itself
returns `false`, confirming the property is unrecognized at the parse layer,
not just absent from the read-side map.

## Симптом

```
FAIL CSS Transitions: property <background-position-x> from neutral to [80px] at (0) should be [40px]
  - assert_true: 'to' value should be supported expected true got false
FAIL Property background-position-x value '0.5em'
  - assert_true: background-position-x doesn't seem to be supported in the computed style expected true got false
```

Two distinct signals, both confirmed against this slice's structured
wptreport:

1. `CSS.supports(prop, value) === false` for values that are valid per spec
   — `interpolation-testcommon.js`'s `'from'/'to' value should be
   supported` assertion — `animations/background-position-x-interpolation.html`
   (112 subtests) and `animations/background-position-y-interpolation.html`
   (112 subtests), 100% of each file.
2. `getComputedStyle(el).getPropertyValue(prop) === ''` unconditionally —
   `computed-testcommon.js`'s `test_computed_value` — `parsing/
   background-position-{x,y}-computed.html` (19 subtests each) and 8 of
   `parsing/background-computed.html`'s 39 subtests (the rest of that file
   is [BUG-472](BUG-472-OPEN.md), background-image/position/size/clip/
   repeat/origin/attachment/color, all *implemented* properties missing
   from the same map).

`parsing/background-position-{x,y}-{valid,invalid}.html` (13 subtests
total) are **not** attributed to this bug despite touching the same
properties — those fail on [BUG-484](BUG-484-OPEN.md) instead
(`_lumen_make_style`'s inline setter never routes through the parser for
*any* property, implemented or not, so it echoes the raw string back
regardless of whether `background-position-x` itself exists).

## Масштаб находки

5 files / 214 subtests in this slice, confirmed via source grep only for
`css/css-backgrounds` — not surveyed elsewhere in `css/`, but any WPT test
anywhere that names `background-position-x`/`-y` directly (rather than
through the shorthand) will hit the same gap.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-backgrounds/` for the 5
attributed files (`animations/background-position-{x,y}-interpolation.html`,
`parsing/background-position-{x,y}-computed.html`,
`parsing/background-computed.html` — the last shared with BUG-472).
