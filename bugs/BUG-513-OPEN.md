# BUG-513: `text-size-adjust` CSS property not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -rln "text.size.adjust\|
text_size_adjust" crates/` — zero hits anywhere in the workspace)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-size-adjust`

## Механизм

`text-size-adjust` (CSS Text Size Adjustment Module,
`https://drafts.csswg.org/css-size-adjust-1/`) controls whether mobile
browsers auto-inflate text size on narrow viewports; it ships (unprefixed
or as `-webkit-text-size-adjust`) in every major mobile browser engine. The
property is entirely absent from `ComputedStyle` and the parser's
known-property table — neither the unprefixed nor prefixed spelling is
recognized.

## Симптом

```
FAIL Property text-size-adjust has initial value auto
  assert_true: text-size-adjust doesn't seem to be supported in the
  computed style expected true got false
FAIL CSS Transitions: property <text-size-adjust> from neutral to [50%] at
  (0) should be [60%]
  assert_true: 'to' value should be supported expected true got false
```

## Масштаб находки

3 files / 204 subtests — by far the largest single-property finding of this
slice, dominated by one file: `animations/text-size-adjust-interpolation.html`
(196 — the Web Animations interpolation-test harness enumerates every
tested value/timing pair and fails identically at its own "is this property
supported" feature-detect before ever reaching interpolation math),
`inheritance.html` (2), `parsing/text-size-adjust-computed.html` (6).
`parsing/text-size-adjust-invalid.html` (4) and one subtest of
`parsing/text-size-adjust-valid.html` (`calc(10% + 5%)` not canonicalized to
`calc(15%)`) fail on the separate, generic inline-`style`-setter gap
([BUG-484](BUG-484-OPEN.md)) instead.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-size-adjust/` for
`animations/text-size-adjust-interpolation.html`, `inheritance.html`, and
`parsing/text-size-adjust-computed.html`, `expected: FAIL` per subtest.
