# BUG-705: legacy live `Document` collections missing entirely — `document.scripts`/`.images`/`.forms`/`.links`/`.anchors`/`.embeds`/`.plugins`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — none of these seven accessors exist on `document`; `WEB_API_SHIM`)
**Найден:** P2, WPT-VENDOR-web-bundle, 2026-08-09

## Симптом

Live probe (`--mcp-live-port`, `eval`) on a plain page:

```
document.scripts   -> undefined   (not an HTMLCollection)
document.images    -> undefined
document.forms     -> undefined
document.links     -> undefined
document.anchors   -> undefined
document.embeds    -> undefined
document.plugins   -> undefined
document.scripts.length -> TypeError: Cannot read properties of undefined (reading 'length')
```

## Причина

`grep -n "'images'\|'forms'\|'links'\|'scripts'\|'embeds'\|'plugins'\|'anchors'"` over
`crates/js/src/dom.rs` returns zero hits — the `Document` object literal
(`var document = {...}`, `dom.rs:4386`) never defines any of these seven
properties. Unlike `document.applets` ([BUG-606](BUG-606-OPEN.md)), which is
spec-mandated to be an always-empty legacy collection, these seven are
**live, non-obsolete** HTML Standard collections (`§4.3.2 The Document
object`) that reflect actual document content and are used by ordinary
(non-obsolete) test code — `document.forms[0]`, `document.images.length`,
etc. are common patterns, not historical-compat corner cases.

The category that surfaced this (`WPT-VENDOR-web-bundle`, `<script
type="webbundle" resources=… scopes=… credentials=…>`) is otherwise
unaffected — the `<script>` element itself parses fine (`tagName ===
"SCRIPT"`, `type === "webbundle"` reflects correctly, unknown script type
is inertly not executed, matching spec behavior for browsers without
Web Bundle support) — this bug was found one level up, via the generic
"probe the shared container" pattern, not in the category's own API.

## Масштаб

Not measured against a specific WPT corpus (found via live probe, not a
harness run — `web-bundle`'s own 28 ids are 100% blocked by the standing
TLS `UnknownIssuer` gap, so none of its own tests reach this code path).
Likely affects any category whose tests enumerate `document.scripts`/
`.images`/`.forms`/`.links` — plausible dom/nodes or html/dom overlap,
not yet cross-checked against a specific failing subtest list.

Fix: extend `Document`'s live-`HTMLCollection` machinery (already used for
`Element.prototype.children`, `_lumen_make_html_collection`, per
[BUG-310](BUG-310-FIXED.md)) with document-scoped, tag-filtered variants —
`scripts`/`img`/`form`/`a[href]`,`area[href]`/`embed`/`applet`(empty)/
`(embed,object)` respectively per the HTML spec's exact per-collection
filter predicates.
