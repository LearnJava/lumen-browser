# BUG-475: `scrollWidth`/`scrollHeight`/`scrollTop`/`scrollLeft` are 0 on any element that isn't `overflow: scroll`/`auto`

**Статус:** FIXED 2026-09-02 (P3)
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

## Fixed 2026-09-02 (P3)

Implemented variant 2 from this bug's own "Что нужно" section: the
`scrollWidth`/`scrollHeight` getters in `crates/js/src/shim/web_api_shim_mid.js`
no longer fall back to `0` when `_lumen_get_scroll_state(nid)` has no entry
(element is not a designated scroll container) — they fall back to the
border-box size from `_lumen_get_bounding_rect(nid)` instead. Border box ⊇
padding box, the same margin `collect_scroll_containers`'s own
`content_width`/`content_height` (`layout/src/lib.rs:1204-1224`) already
relies on as the floor for actual scroll containers, so this satisfies the
spec's "at least padding-box size" requirement without a Rust-side change.
`scrollTop`/`scrollLeft` were left untouched — `0` for a non-scroll-container
was already spec-correct there (no scroll position to report), the shared
`0`-fallback mechanism just happened to also cover them.

Regression test: `scroll_width_height_fall_back_to_bounding_rect_for_non_scroll_container`
(`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`). Confirmed live via
`--mcp-port`/`eval` on a page reproducing `.wrapper` from
`scrollWidthHeight-negative-margin-002.html` (`width:80px; padding:1px 4px
8px 16px; border-width:1px 2px 3px 4px` overridden `border-right-width:50px;
border-bottom-width:40px`; `display:flow-root; overflow:visible`):
`scrollWidth`/`scrollHeight` read `154`/`130` (border box) instead of `0`/`0`
before the fix — `154 ≥ 100` and `130 ≥ 89` (padding-box width/height),
satisfying the exact `assert_greater_than_equal` this bug's own symptom
quoted.

**Residual:** the fix closes exactly this bug's reported symptom (the
`assert_greater_than_equal` "at least padding-box" precondition, and the
general "returns 0 instead of a real number" class) but does not compute the
true CSS Overflow "scrollable overflow area" — the same negative-margin test
also asserts an *exact* `scrollWidth`/`scrollHeight` value for a
non-scroll-container element whose content overflows its own padding box
(e.g. `.inner`'s `margin: -100px`), which border-box alone cannot produce.
Filed separately as [BUG-960](BUG-960-OPEN.md) (needs a per-box
scrollable-overflow-region algorithm — CSS Overflow §Scrollable Overflow —
architecturally bigger than this fix). The 8 `.ini` files referencing this
bug under `tests/wpt/metadata/css/cssom-view/` are intentionally left
untouched — the exact PASS/FAIL split needs a fresh `run_report.py`, not run
in this session.

Gates: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean; `cargo test -p lumen-js --features v8-backend
v8_elem_geometry_scroll` 11/11.
