# BUG-529: `window.innerWidth`/`innerHeight`/`outerWidth`/`outerHeight` do
not exist at all

**Статус:** FIXED 2026-09-03
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — `Window` shim)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/css-scrollbars`

## Механизм

`grep -rn "innerWidth\|innerHeight\|outerWidth\|outerHeight"
crates/js/src/*.rs` — zero hits anywhere in the JS shim. Confirmed live:
`typeof window.innerWidth` → `"undefined"` (same for the other three).
These are among the oldest and most heavily used `Window` properties on the
web (CSSOM View §5.3) — their total absence is surprising for a browser
this far into Phase 2, and wasn't caught earlier because most of Lumen's
own layout/viewport code reads geometry through
`_lumen_get_viewport_size()` (Rust-side) rather than through the JS-visible
`window.inner*`/`outer*` properties a *page script* would use.

## Симптом

Any WPT test comparing `window.innerWidth`/`innerHeight` against an
element's `offsetWidth`/`clientWidth` (the standard "does this viewport
have a scrollbar" idiom) gets `undefined` on one side of the comparison
and fails with a message like `assert_less_than: viewport has a scrollbar
expected a "undefined" but got a "number"` — this dominates
`css/css-scrollbars`'s per-file scrollbar-geometry checks (11 files/12
subtests this slice: `scrollbar-color-001/002.html`,
`scrollbar-width-005..014.html`, `scrollbar-width-keywords.html`) and is
almost certainly latent in many other already-triaged categories that
happened to route around it (worth a future re-check once fixed).

## Фикс

**FIXED 2026-09-03 (P3).** Added `innerWidth`/`innerHeight`/`outerWidth`/
`outerHeight` getters to the `Window` shim
(`crates/js/src/shim/web_api_shim_tail_mc.js`, right next to the existing
`scrollX`/`scrollY` `Object.defineProperties` block — the shim's text moved
out of `dom.rs` into per-file `.js` consts in SPLIT-JS3, after this bug was
filed). `inner*` reads the CSS-pixel viewport size already available via
`_lumen_get_viewport_size()`; there is no window-chrome model in this
single-window shell, so `outer*` aliases the same viewport size, the same
simplification already used for `visualViewport` a few lines above.

Regression test: `window_inner_and_outer_size_track_viewport_size`
(`crates/js/src/dom/tests/v8_elem_geometry_scroll.rs`), asserting all four
properties track `rt.update_viewport_size(800.0, 600.0)`.
`cargo test -p lumen-js --features v8-backend` and
`cargo clippy -p lumen-js --features v8-backend --all-targets -- -D
warnings` both clean.

Directly unblocks the `css/css-scrollbars` idiom this bug names, and is a
named prerequisite for 4 of the 5 files remaining under
[BUG-504](BUG-504-OPEN.md) (`scrollbar-gutter-propagation-*.html`).
