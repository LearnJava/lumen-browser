# BUG-623: `window.find()` (legacy text-search API) is missing entirely

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `window` object)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`): `typeof window.find` → `"undefined"`.

`inert-and-find-flat-tree.html`'s two subtests both fail with `TypeError:
window.find is not a function` (`window.find('inside shadowroot')` /
`window.find('slotted')`, testing that `window.find` respects the flat
tree when searching text inside a `<dialog>` nested in a shadow root).
`inert-and-find.html` TIMEOUTs on the same call used at the top level of
its script (before any test registers).

## Масштаб

2 files in this category. `window.find` is a long-standing, non-standard
(not in any W3C/WHATWG spec) but widely-implemented legacy API — Firefox
and Safari both support it, Chrome does not by default. Lower priority
than the other findings in this category (it isn't a spec conformance gap
in the strict sense), but it is exercised directly by these two vendored
WPT tests and by any future category testing find-in-page-adjacent
behavior against shadow DOM. Not investigated whether Lumen's user-facing
find-in-page feature (`CAPABILITIES.md` lists find-in-page as ✅) could be
exposed here, or whether this needs its own independent text-search
implementation.
