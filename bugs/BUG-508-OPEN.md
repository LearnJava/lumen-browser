# BUG-508: CSS Color HDR `dynamic-range-limit` property entirely unimplemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** css-parser/layout — property not recognized anywhere
(`grep -rn "dynamic-range-limit\|dynamic_range_limit" crates/` returns zero hits)
**Найден:** WPT-RUN-3 срез 14 (`ROADMAP.md`) — массовый прогон `css/css-color-hdr`

## Симптом

`dynamic-range-limit` (CSS Color HDR Module Level 1) is not parsed, not
placed in `ComputedStyle`, and does not participate in interpolation/Web
Animations at all — every test that probes the property fails the same way:

- `computed.html` (21/21 FAIL): every candidate keyword/`dynamic-range-limit-mix()`
  value fails `assert_true: dynamic-range-limit doesn't seem to be supported
  in the computed style expected true got false`.
- `inheritance.html` (2/2 FAIL): same assertion for "has initial value" and
  "inherits".
- `interpolation.html` (64/64 FAIL, all four interpolation methods — CSS
  Animations, CSS Transitions ×2 forms, Web Animations — for the same
  underlying reason): `assert_true: 'from' value should be supported
  expected true got false` / `assert_true: Web Animations should be
  supported expected true got false`.

`parsing.html`'s 16 failures (13/29 subtests pass) are a *different*,
already-tracked mechanism entirely — the inline `style` setter never
rejects invalid values ([BUG-484](BUG-484-OPEN.md)) — and are not
attributed to this bug.

## Масштаб находки

3 files / 87 subtests directly attributable to the missing property
(`computed.html` 21, `inheritance.html` 2, `interpolation.html` 64).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-color-hdr/` for
`computed.html`, `inheritance.html`, `interpolation.html` (per-subtest
`expected: FAIL`).
