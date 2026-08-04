# BUG-609: `HTMLOptionsCollection.length` setter doesn't grow `<select>` for valid large values

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `HTMLOptionsCollection.prototype` `length` setter, same object introduced for BUG-383/BUG-576)
**Найден:** P2, WPT-VENDOR-html-misc, 2026-08-04

## Симптом

```
FAIL select options.length too large 3 - assert_equals: Length of <select> should be 100,000 expected 100000 but got 3
FAIL select options.length too large 4 - assert_equals: Manual expansion still works expected 100002 but got 5
```
(`select/options-length-too-large.html`)

## Причина

Per the DOM/HTML `HTMLOptionsCollection` `length` setter algorithm, setting
`select.options.length = N` for an in-range `N` (`0 <= N <= 100000`) must
grow the collection by appending empty `<option>` elements up to length `N`
(and shrink/truncate for smaller in-range `N`). Out-of-range values
(negative, `> 100000`) must be silently ignored, leaving the collection
unchanged. Lumen's setter correctly ignores the three out-of-range cases
(`-1`, `100001`, `Number.MAX_SAFE_INTEGER` — the collection stays at length
3, matching spec), but never implements the growth path: setting the
in-range `100000` leaves the collection at its prior length instead of
appending 99997 new `<option>` elements, and a subsequent manual
`appendChild` off that wrong base (`3` → `5` instead of `100000` → `100002`)
confirms the setter is a pure no-op for growth rather than a partially
correct algorithm.

## Масштаб

Self-contained, 1 file, 2/5 subtests (the 3 out-of-range-rejection subtests
already pass).
