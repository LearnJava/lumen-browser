# BUG-476: `offsetLeft`/`offsetTop` return viewport-absolute coordinates instead of offsetParent-relative ones

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6190-6193`)
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
