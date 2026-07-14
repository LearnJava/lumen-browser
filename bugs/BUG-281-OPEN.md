# BUG-281 — `document`/`Element` DOM-tree shape gaps break React 18 mount (`document.nodeType`, `ownerDocument` identity, `documentElement.tagName`, `namespaceURI`)

**Статус:** OPEN — blocks the Ph3-v8-migration S12 DoD item "React 18 CRA demo loads without JS errors"
**Компонент:** js (`crates/js/src/dom.rs`, `WEB_API_SHIM` — the `document`/`Element`/`Node` JS shim, engine-agnostic)
**Найден:** Ph3-v8-migration S12 (`docs/tasks/ph3-v8-migration.md`), while verifying the S12 DoD item
"React 18 CRA demo loads without JS errors" against a real React 18 UMD production build.

## Симптом

Loading React 18 (`react@18/umd/react.production.min.js` + `react-dom@18/umd/react-dom.production.min.js`)
and calling `ReactDOM.createRoot(container).render(...)` throws:

```
JS runtime error: Cannot read properties of undefined (reading '_reactListening<random>')
```

(the property name has a random suffix — normal React internals, not itself a bug) inside
`react-dom`'s event-delegation setup (`listenToAllSupportedEvents`), before any component renders.

## Причина (confirmed via isolated diagnostic page)

`react-dom`'s root-creation path walks `container` → `ownerDocument` → root-listening-marker checks, which
rely on several `Document`/`Element` properties that Lumen's DOM shim gets wrong:

| Check | Expected (spec) | Lumen actual |
|---|---|---|
| `element.nodeType` | `1` (`ELEMENT_NODE`) | `1` — correct |
| `element.ownerDocument === document` | `true` (same object identity) | **`false`** |
| `element.namespaceURI` | `"http://www.w3.org/1999/xhtml"` for HTML elements | **`undefined`** |
| `document.nodeType` | `9` (`DOCUMENT_NODE`) | **`undefined`** |
| `document.documentElement.tagName` | `"HTML"` | **`"#document"`** (looks like `documentElement` returns the `Document` node itself, or `tagName`'s getter is wrong for it) |

Any one of these being wrong is enough to make react-dom's container/document identity checks fail and
crash inside its event-delegation bootstrap.

## Not V8-specific — reproduces identically under QuickJS

Verified with the exact same diagnostic page and the exact same React 18 build against **both** engines
(`--features v8` default and `--no-default-features --features backend-femtovg,backend-wgpu,quickjs`):
byte-identical symptom on both (`ownerDocument=false`, `namespaceURI=undefined`, `doc.nodeType=undefined`,
`documentElement.tagName=#document`, and the same `_reactListening<random> of undefined` crash). This is a
`WEB_API_SHIM` (`crates/js/src/dom.rs`) gap shared by both JS backends — **not** something introduced by
or specific to the Ph3-v8-migration engine cutover, and not blocked on it either.

## Related but distinct

[BUG-280](BUG-280-OPEN.md) (`window` is a plain object, not the real global object — bare globals set via
`window.x = ...` unreachable) is a **different** gap in the same file, already filed and in progress
(`p2-bug-280-global-object`). Fixing BUG-280 alone does not fix this bug — my repro explicitly referenced
`window.React`/`window.ReactDOM` (not bare identifiers) to isolate this bug from BUG-280's.

## Repro

1. Build `lumen.exe` (`dev-release`), any JS backend.
2. Load an HTML page with `<div id="root"></div>` + React 18 UMD (`react.production.min.js` +
   `react-dom.production.min.js`) inline, then `ReactDOM.createRoot(document.getElementById('root'))`.
3. Or the minimal diagnostic: `console.log(document.nodeType, document.documentElement.tagName,
   document.getElementById('root').ownerDocument === document, document.getElementById('root').namespaceURI)`.

## Что нужно для закрытия

Fix `Document`/`Element` construction in `WEB_API_SHIM` so that: `document.nodeType === 9`,
`document.documentElement` returns the actual `<html>` element node (not `document` itself),
`element.ownerDocument` returns the same `document` object by reference (not a fresh wrapper per call), and
`element.namespaceURI` resolves to `"http://www.w3.org/1999/xhtml"` for HTML-namespace elements (`null` or
the correct namespace for foreign elements/SVG). Re-run the React 18 smoke repro above afterward.
