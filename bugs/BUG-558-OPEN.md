# BUG-558: `MouseEvent` is missing the `x`/`y` aliases, and `pageX`/`pageY` (hence `offsetX`/`offsetY`) are frozen at construction instead of live getters over current scroll position

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/dom.rs:3441-3460` — the `MouseEvent` constructor)
**Найден:** P2, WPT-RUN-3 срез 39 (`css/cssom-view`), 2026-08-04

## Симптом

`mouseEvent.html`, 3 subtests:

```
FAIL MouseEvent's x and y must be equal to clientX and clientY.
  assert_equals: expected (number) 10 but got (undefined) undefined
FAIL MouseEvent's pageX and pageY attributes should be the sum of the scroll
  offset and clientX/clientY - assert_equals: expected 5020 but got 20
FAIL MouseEvent's offsetX/offsetY attributes should be the same value as its
  pageX/pageY attributes. - assert_equals: expected 10 but got 0
```

## Причина

Two independent gaps in the `MouseEvent` constructor (`dom.rs:3441-3460`):

1. **No `x`/`y` properties at all.** CSSOM View §5 defines `x`/`y` as plain
   aliases for `clientX`/`clientY`. `grep -n '\.x \|\.y \|this\.x\b'` around
   the constructor shows no such assignment — reading `mouseEvent.x` returns
   plain-object `undefined`.

2. **`pageX`/`pageY` are computed once at construction and then frozen:**
   ```js
   this.pageX = (init && init.pageX != null) ? +init.pageX : this.clientX;
   this.pageY = (init && init.pageY != null) ? +init.pageY : this.clientY;
   ```
   Per CSSOM View §5, `pageX`/`pageY` (when not explicitly given in the
   init dict) must be **live getters**: `clientX + view.scrollX` /
   `clientY + view.scrollY`, evaluated on every read against the event's
   `view`'s *current* scroll position — not a value snapshotted once. The
   WPT test constructs an event, reads `pageY` (10), scrolls the page by
   5000, and reads `pageY` again on the **same** event object expecting
   5020 — proving real implementations recompute on access. Lumen's plain
   assignment means the second read still returns the construction-time
   value.
   `offsetX`/`offsetY` inherit the same problem one level further: they're
   assigned `0` (or the raw `init.offsetX`) instead of being derived from
   `pageX`/`pageY`, so the third subtest (which only checks
   `offsetX === pageX`) fails regardless of whether `pageX` itself is fixed.

## Масштаб находки

3/3 subtests in `mouseEvent.html`. Narrow blast radius observed this slice
(one file), but any WPT/real-world code reading `event.x`/`.y` or comparing
`pageX` across a scroll is affected.

## Что нужно

1. Add `x`/`y` as plain aliases of `clientX`/`clientY` (either assigned
   alongside them, or as getters if `clientX`/`clientY` can themselves
   change post-construction elsewhere in the codebase — check before
   picking plain assignment vs. getter).
2. Turn `pageX`/`pageY` (and the offsetX/offsetY fallback that mirrors them
   in this test's usage) into getters reading `this.clientX +
   (this.view || window).scrollX` / `...scrollY`, only when not explicitly
   overridden via the init dict (per spec, an explicit `init.pageX` should
   still win and stay static — check the exact algorithm before
   implementing to avoid breaking that case).
