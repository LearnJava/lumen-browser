# BUG-476: `offsetLeft`/`offsetTop` return viewport-absolute coordinates instead of offsetParent-relative ones

**Статус:** FIXED 2026-09-02 (P3)
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — moved out of
`dom.rs` by SPLIT-JS3, 2026-08-28; symptom line numbers below are from before
that move)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL container: 2
  assert_equals: offsetLeft expected 10 but got 24
```

`offsetTopLeft-border-box.html`: `.target` sits inside `.container`
(`position: relative`, `padding: 2px 10px`, `border-width: 3px 6px`,
`box-sizing: border-box`, placed at viewport x=8). Expected `offsetLeft` is
`10` (the container's padding-left — `.target` is offset from its
`offsetParent`'s padding edge). Lumen returns `24` — exactly
`container_x(8) + border_left(6) + padding_left(10)`, i.e. the target's own
**viewport-absolute** x.

## Причина

```js
get offsetWidth()  { var r = _lumen_get_bounding_rect(nid); return r ? r[2] : 0; },
get offsetHeight() { var r = _lumen_get_bounding_rect(nid); return r ? r[3] : 0; },
get offsetLeft()   { var r = _lumen_get_bounding_rect(nid); return r ? r[0] : 0; },
get offsetTop()    { var r = _lumen_get_bounding_rect(nid); return r ? r[1] : 0; },
```

`offsetWidth`/`offsetHeight` are fine (size doesn't depend on an origin).
`offsetLeft`/`offsetTop` hand back `r[0]`/`r[1]` straight from
`_lumen_get_bounding_rect`, which is the element's viewport-relative
`getBoundingClientRect()` origin — there is no `offsetParent` walk or
subtraction anywhere in this getter. Per CSSOM View §5 (`HTMLElement`
extensions), `offsetLeft`/`offsetTop` must be relative to the nearest
positioned ancestor (`offsetParent`), not the viewport.

## Масштаб находки

~26+16+15+12+5+2+2 subtests across `offsetTopLeft-border-box.html`,
`offsetTop-offsetLeft-nested-offsetParents.html`,
`offsetTopLeft-empty-inline-offset.html`,
`offsetTopLeftInScrollableParent.html`,
`offsetTop-offsetLeft-with-zoom.html`. A minority of cases read exactly `0`
rather than a wrong nonzero number (e.g. inline `<span>` targets) — not fully
root-caused in this slice; may be a second, narrower gap in how
`_lumen_get_bounding_rect` populates entries for inline boxes. Re-run after
fixing the offsetParent math before deciding whether a second bug is needed.

## Что нужно

Walk the ancestor chain from `nid` (matching the `offsetParent` algorithm —
first positioned ancestor, or `<body>`/table cell fallback) and subtract that
ancestor's own border-box origin (plus its border widths) from `r[0]`/`r[1]`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: FAIL` per the actual run.

## Fixed 2026-09-02 (P3)

Implemented the walk this bug's own "Что нужно" section named. Two new
helpers in `web_api_shim_mid.js`:

- `_lumen_offset_parent_nid(nid)` — the CSSOM View §5 `offsetParent`
  algorithm: walks ancestors via `_lumen_get_parent`, returns the nearest one
  whose computed `position` is not `static` (an empty string from
  `_lumen_get_computed_style`, i.e. no entry yet, is treated as `static`, not
  as positioned — a style-less node must never win the walk), or the nearest
  `<body>`/`<td>`/`<th>`/`<table>`. Returns `null` for the root element,
  `<body>` itself, or a `position: fixed` element.
- `_lumen_offset_origin(nid)` — the point `offsetLeft`/`offsetTop` measure
  from: the `offsetParent`'s padding edge (its border-box origin from
  `_lumen_get_bounding_rect` plus its own `border-left-width`/
  `border-top-width`, always published in computed-style px so a plain
  `parseFloat` resolves them), or `[0, 0]` when there is no `offsetParent`.

`offsetLeft`/`offsetTop` now subtract this origin from
`_lumen_get_bounding_rect(nid)`; `<body>` is a separate special case (always
`0`, CSSOM View §5 step 1) checked before the `offsetParent` walk runs at
all — matches the WPT fixture's own container/target math (container border-
box at viewport x=8, `border-left-width:6px` → padding edge at x=14; target
at viewport x=24 → `offsetLeft = 24 - 14 = 10`, the value the symptom quoted
as expected).

Three regression tests
(`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`):
`offset_left_top_relative_to_positioned_ancestor` (a `position: relative`
ancestor closer than `<body>` wins, border-width subtraction verified),
`offset_left_top_falls_back_to_body_when_no_positioned_ancestor` (no
positioned ancestor anywhere — the walk still reaches `<body>`, per spec, not
`null`), `offset_left_top_zero_for_body_itself`.

**Residual:** this bug's own "Масштаб находки" section flagged a second,
unconfirmed gap — a minority of cases (inline `<span>` targets) reading
exactly `0` rather than a wrong nonzero number, suspected to be a narrower
issue in how `_lumen_get_bounding_rect` populates entries for inline boxes,
not the `offsetParent` math this fix addresses. Not re-investigated in this
session — needs a fresh WPT run against the fix to see whether it still
reproduces. The 9 `.ini` files referencing this bug under
`tests/wpt/metadata/css/cssom-view/` are intentionally left untouched, same
protocol as BUG-475's fix: the exact PASS/FAIL split needs a fresh
`run_report.py`, not run in this session.

Gates: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean; `cargo test -p lumen-js --features v8-backend` 3396/3396
(whole crate).
