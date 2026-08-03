# BUG-529: `window.innerWidth`/`innerHeight`/`outerWidth`/`outerHeight` do
not exist at all

**Статус:** OPEN
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

## Фикс (не сделан)

Add `innerWidth`/`innerHeight` (CSS pixel viewport size, already available
via `_lumen_get_viewport_size()`) and `outerWidth`/`outerHeight` (window
chrome size — can alias to the same viewport size in this single-window
shell, same simplification already used elsewhere for window-chrome-less
properties) getters to the `Window` shim in `crates/js/src/dom.rs`.
