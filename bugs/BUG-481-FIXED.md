# BUG-481: `window.visualViewport` not implemented

**Статус:** FIXED 2026-09-02 (P3)
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL Element.scrollIntoView doesn't scroll a position:fixed element ...
  promise_test: Unhandled rejection with value: object
  "ReferenceError: visualViewport is not defined"
```

`grep -n "visualViewport" crates/js/src/dom.rs` — zero matches. The Visual
Viewport API (`window.visualViewport`, a `VisualViewport` with
`width`/`height`/`offsetLeft`/`offsetTop`/`scale`/`onresize`/`onscroll`) is
entirely absent.

## Масштаб находки

4 subtests, `scrollIntoView-fixed-outside-of-viewport.html`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for the one
attributed file, `expected: FAIL` per the actual run.

## Срез 21 (`css/css-device-adapt`, 2026-08-03)

5 files, 5 subtests, same signature — `window.visualViewport` is `undefined`,
so `window.visualViewport.scale` throws `Cannot read properties of undefined
(reading 'scale')` before the assertion runs:
`viewport-clamp-initial-scale-to-max.tentative.html`,
`viewport-clamp-initial-scale-to-min.tentative.html`,
`viewport-user-scalable-no-clamp-to-max.tentative.html`,
`viewport-user-scalable-no-clamp-to-min.tentative.html`,
`viewport-user-scalable-no-wide-content.tentative.html` — all five test
`<meta name="viewport">` initial/min/max-scale clamping, which this engine
cannot expose without a `VisualViewport` object regardless of whether the
underlying zoom clamping itself is implemented. `.ini` under
`tests/wpt/metadata/css/css-device-adapt/` for all 5 files.

## WPT-VENDOR-visual-viewport (2026-08-09)

Category vendored and run whole (`run_report.py --all --root visual-viewport
--recursive`, 19 selected ids): **11/19 harness OK, 0/22 subtests**. Same
`window.visualViewport` absence, now observed as three distinct failure
shapes depending on where each file's first reference sits relative to
`test()`/`async_test().step()`: plain `FAIL` (`TypeError: Cannot read
properties of undefined`) when wrapped in a step, `TIMEOUT` or `ERROR: done()
was called without first defining any tests` when the reference is at the
file's top level or inside an unwrapped callback and the resulting unhandled
exception aborts the script before `done()` runs. No new bug number filed —
same root cause as the entries above.

## Fixed 2026-09-02 (P3)

Added `window.visualViewport` — `web_api_shim_tail_mc.js`, right after the
`window.scrollTo`/`scrollBy` block it depends on (loads after
`EVENT_TARGET_SHIM`, before the `window`→`globalThis` copy loop in
`web_api_shim_tail_b.js`, so it is bare-reachable as `visualViewport` too). A
`VisualViewport` constructor extends the pure-JS `EventTarget` base (same
pattern as `PictureInPictureWindow`/`XRSession`/…) with non-enumerable
getters:

- `width`/`height` — `_lumen_get_viewport_size()`, the same native
  `matchMedia` already reads.
- `pageTop` — `_lumen_get_page_scroll_y()`, the same native `window.scrollY`
  reads (this is what the originating `scrollIntoView-fixed-outside-of-
  viewport.html` regression asserts stays equal to `window.scrollY` across a
  page scroll).
- `offsetLeft`/`offsetTop`/`pageLeft` — `0`, `scale` — `1`: no pinch-zoom or
  `<meta name=viewport>` scale clamping is modeled, so the visual viewport is
  always identical to the layout viewport.
- `onresize`/`onscroll`/`onscrollend` — declared `null` (not wired to any
  dispatch) so `'onresize' in visualViewport` reads `true`, same convention
  as the `window.onscroll` declaration (BUG-822/834).

Four new regression tests
(`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`):
`visual_viewport_exists_and_extends_event_target`,
`visual_viewport_width_height_track_viewport_size`,
`visual_viewport_page_top_tracks_page_scroll`,
`visual_viewport_offset_and_scale_default_unzoomed`.

**Fixes the originating 4 subtests** (`scrollIntoView-fixed-outside-of-
viewport.html` no longer throws — its assertions are only about
`scrollY`/`pageTop` equality, which now hold).

**Residual — the other two slices in this file stay open, unchanged, as
predicted by their own text** ("regardless of whether the underlying zoom
clamping itself is implemented"): the 5 `css-device-adapt` files now get a
real `visualViewport.scale` reading (`1`) instead of a `TypeError`, but that
reading is not the spec-correct clamped value (`2.0` etc.) — actual `<meta
viewport>` scale-clamping is a separate, much larger feature, out of a P3
point-fix's scope. This is the same root cause [BUG-875](bugs/BUG-875-OPEN.md)
tracks under [`ROADMAP.md`](../ROADMAP.md)'s `GAP-VVPORT` — BUG-875 predates
this fix by a WPT-RUN-6 re-discovery of the identical "`window.visualViewport`
absent" symptom, filed after this bug (which is the earlier, 2026-08-02
discovery) without the duplicate check catching it. `GAP-VVPORT`'s own ask
goes further than this fix (real resize/scroll event dispatch, actual
layout-vs-visual-viewport distinction) — left `planned`, not addressed here.
Not reclassifying/relinking BUG-875 or touching the `GAP-VVPORT` row in this
session: that call affects another role's queue and needs a fresh WPT run to
confirm what of BUG-875's own attributed subtest still fails after this fix.

Gates: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean; `cargo test -p lumen-js --features v8-backend` 3408/3409
passed (one unrelated pre-existing flake, confirmed independent of this
change — `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty`
fails only under full-suite parallel execution, passes in isolation and
single-threaded, both before and after this change; not touched by this fix).
