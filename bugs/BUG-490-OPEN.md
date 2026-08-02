# BUG-490: `getComputedStyle(element, pseudoElt)` ignores the pseudo-element argument entirely

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — `WEB_API_SHIM`, `window.getComputedStyle`
at `dom.rs:12772-12802`, engine-agnostic, shared by both QuickJS and V8 per
`CLAUDE.md`)
**Найден:** WPT-RUN-3 срез 7 (`ROADMAP.md`) — массовый прогон `css/css-display`

## Механизм

`window.getComputedStyle = function(element, pseudoElt) { ... }`
(`dom.rs:12772`) resolves `nid` purely from `element.__nid__` — the
`pseudoElt` parameter is never read anywhere in the function body (Proxy
`get` trap or the no-Proxy fallback). The doc comment above it is explicit:
`// Pseudo-elements are not yet supported (ignored).` (`dom.rs:12771`).
`getComputedStyle(el, '::before')` therefore returns `el`'s own resolved
style, not `::before`'s.

Confirmed live via `--mcp-port` on a page with
`#t1::first-line { display: flex; font-size: 30px }` and no other style on
`#t1`: `getComputedStyle(el, '::first-line').display` → `"block"` (`el`'s
own default block display, i.e. the pseudo argument really is silently
dropped, exactly per the code comment) rather than `"inline"` (the correct
resolved value for the `::first-line` box per CSS Display L3 §placement).

**Open discrepancy, not yet resolved:** the actual wptrunner run of
`display-first-line-001.html`/`display-math-on-pseudo-elements-001.html`
observed the assertion failing with `got ""` (empty string), not `got
"block"` as the code path above predicts and as the live `--mcp-port` probe
reproduces. Both point at the same underlying gap (pseudo styling not
implemented), but the exact wrong-value shape differs between the headless
probe (local-file navigation) and the wptrunner-driven run (HTTP navigation
via the live window / `--bidi-port`) — worth a follow-up slice to pin down
whether that's a second, independent bug (e.g. a load-time race specific to
the live-window navigation path) or an artifact of the probe's local-file
navigation taking a different code path than HTTP navigation.

## Симптом

Any WPT assertion using the two-argument `getComputedStyle(el, pseudoElt)`
form to read a pseudo-element's resolved style fails — either with a wrong
(base-element) value or an empty string, depending on navigation path (see
above). Affects `::before`/`::after`/`::first-line`/`::first-letter`
lookups alike, since all four go through the same ignored parameter.

## Масштаб находки

3 files in this slice (`display-first-line-001.html`,
`display-first-letter-001.html`, `display-math-on-pseudo-elements-001.html`
— the latter's `::before`/`::after` checks). Will recur in any WPT category
that queries a pseudo-element's computed style via the standard two-argument
form — a common idiom for `::before`/`::after`/`::first-line`/
`::first-letter` conformance tests specifically (not pseudo-elements in
general — `::marker`, `::selection` etc. aren't queried this way as often).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-display/` for the 3
attributed files.
