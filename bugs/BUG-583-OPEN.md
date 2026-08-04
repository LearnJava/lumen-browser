# BUG-583: `<permission>` element (Permission Element API) not implemented at all

**Статус:** OPEN
**Компонент:** dom/js (no `<permission>` tag handling anywhere — grep for
`"permission"`/`'permission'` across `crates/dom`, `crates/html-parser`,
`crates/js/src/dom.rs` returns zero hits; the element falls through to the
generic unknown-tag path, so `document.createElement('permission')`
produces a plain `HTMLUnknownElement`)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - HTMLUserMediaElement is not defined
```

48 occurrences, entirely within `permission-element/`. Also present but
not separately counted: `type-supported-feature-detect.tentative.html`
feature-detects via `HTMLPermissionElement`, which is equally undefined.

## Причина

The Permission Element (a proposed `<permission type="camera">` HTML
element that renders a browser-controlled, unspoofable permission-request
button — currently a Chromium-only origin trial, `.tentative.` throughout
the vendored subtree) has zero engine support: not registered as a known
tag, no `HTMLPermissionElement`/`HTMLUserMediaElement` interface, no
permission-prompt wiring. `<permission>` is treated as an ordinary unknown
element (renders as an anonymous inline box with its text content, no
special behavior), consistent with every other unhandled tag.

## Масштаб

Whole feature, self-contained to `permission-element/`. Genuinely
experimental/non-standard (single-vendor origin trial, not yet a W3C
Candidate Recommendation feature) — flagging for scope triage rather than
implying it should be prioritized; recorded per this track's "no subdirectory
skipped silently" rule.
