# BUG-475: `scrollWidth`/`scrollHeight`/`scrollTop`/`scrollLeft` are 0 on any element that isn't `overflow: scroll`/`auto`

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6196-6205` — getters read
`_lumen_get_scroll_state(nid)`), layout (`crates/engine/layout/src/lib.rs:1121`
`collect_scroll_containers` — only pushes boxes whose `overflow_x`/`overflow_y`
is `Scroll`/`Auto`), shell (`crates/shell/src/main.rs:10632`
`collect_scroll_containers(lb_ref)` → `js.update_scroll_states(...)`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL scrollWidth with negative margins: display: flow-root; overflow: visible
  assert_greater_than_equal: scrollWidth should be at least padding box width
  expected a number greater than or equal to 100 but got 0
```

`.wrapper` in `scrollWidthHeight-negative-margin-002.html` is static markup
present at load, `overflow` left at its initial value (`visible`) — not a
designated scroll container.

## Причина

`_lumen_get_scroll_state(nid)` (`dom.rs:2435`) reads a `HashMap<NodeId, [x, y,
scroll_width, scroll_height]>` built once per relayout by
`collect_scroll_containers` (`layout/src/lib.rs:1121-1159`), which walks the
box tree and pushes an entry **only** for boxes where `overflow_x`/`overflow_y`
is `Overflow::Scroll`/`Overflow::Auto` (`layout/src/lib.rs:1130-1131`). Every
other element — `overflow: visible`/`hidden`, or any element that never had
`overflow` set — has no entry in the map, so the JS getters fall back to `0`
(`s ? s[2] : 0`).

Per CSSOM View §Extensions to the Element interface,
`scrollWidth`/`scrollHeight` are defined for **every** element regardless of
its own `overflow`: they must return at least the element's padding-box size
(more, if its content overflows even without being independently scrollable —
CSS Overflow §"scrolling area"). Same underlying map, so `scrollTop`/
`scrollLeft` (currently exposed only for actual scroll containers) share the
gap for non-scrollable elements too, though those pass more often because
tests usually only read them on elements that already are `overflow:
scroll`/`auto`.

## Масштаб находки

Largest cluster in this slice: ~600 subtests (300× `scrollWidth should be at
least…`, 300× `scrollHeight should be at least…`) plus several downstream
failures that read `scrollWidth`/`scrollHeight` as a precondition
(`scrollWidthHeight-*`, `table-scroll-props.html`'s scroll-prop half,
`HTMLBody-ScrollArea_quirksmode.html`).

## Что нужно

`collect_scroll_containers` (or a sibling collector) needs to report every
box's own padding-box size regardless of its `overflow` value — either by
pushing an entry for every box (paying the map-size cost) or by having the JS
getter fall back to `_lumen_get_bounding_rect`'s padding-box size when no
scroll-container entry exists, only using the scroll-container entry for the
scrolled position (`scroll_x`/`scroll_y`) and true scrollable extent when it
*is* one.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: FAIL` per the actual run.
