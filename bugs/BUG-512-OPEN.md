# BUG-512: `forced-color-adjust` CSS property not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -rn "forced-color-adjust\|
forced_color_adjust" crates/` — zero hits anywhere in the workspace)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-forced-color-adjust`

## Механизм

`forced-color-adjust` (CSS Color Adjustment Module Level 1 /
Forced Colors Mode, `https://drafts.csswg.org/css-color-adjust-1/#forced`)
lets a page opt an element out of a user's forced-colors (high-contrast)
mode. Unlike [BUG-511](BUG-511-OPEN.md)'s `link-parameters`, this is a
real, shipped property (Chromium/Firefox/Safari all support it) — a
genuine engine gap, not a never-shipped-draft filing. The property is
entirely absent from `ComputedStyle` and the parser's known-property table.

## Симптом

```
FAIL Property forced-color-adjust has initial value auto
  assert_true: forced-color-adjust doesn't seem to be supported in the
  computed style expected true got false
FAIL Property forced-color-adjust value 'preserve-parent-color'
  assert_true: forced-color-adjust doesn't seem to be supported in the
  computed style expected true got false
```

## Масштаб находки

2 files / 5 subtests: `inheritance.html` (2), `parsing/forced-color-adjust-computed.html`
(3). `parsing/forced-color-adjust-invalid.html` (6 subtests) fails on the
separate, generic inline-`style`-setter gap ([BUG-484](BUG-484-OPEN.md) —
invalid values like `"auto auto"`/`"1"`/`"default"` are accepted instead of
rejected) rather than on this property specifically.
`parsing/forced-color-adjust-valid.html` (3/3) passes: valid values
round-trip through `element.style` even without real validation, per
BUG-484's mechanism.

Since forced-colors mode itself (media feature `forced-colors`, the
system-color palette swap) is a larger, separate subsystem not evidenced
anywhere in this crate either, this property gap likely sits on top of a
wider absence — not investigated further in this slice, scope was the
`css-forced-color-adjust` WPT category only.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-forced-color-adjust/` for
`inheritance.html` and `parsing/forced-color-adjust-computed.html`,
`expected: FAIL` per subtest.
