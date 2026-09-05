# BUG-573: `Range.prototype.createContextualFragment` missing

**Статус:** FIXED 2026-09-05
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_range`;
report's original `dom.rs:6870-6944` pointer predates the shim split, method
list lives here now)
**Найден:** P2, WPT-VENDOR-html-semantics-scripting-1, 2026-08-04

## Симптом

```
FAIL scheduler: inline script created with createContextualFragment - range.createContextualFragment is not a function
```

`range.createContextualFragment(htmlString)` throws `TypeError:
range.createContextualFragment is not a function` — the method does not
exist on the `Range` object at all.

## Причина

`_lumen_make_range()` returns a plain object literal implementing a subset
of the `Range` interface (`setStart`/`setEnd`/`selectNodeContents`/
`cloneRange`/`toString`/`deleteContents`/`insertNode`/
`compareBoundaryPoints`/`getBoundingClientRect`/`getClientRects`/`detach`/
`isPointInRange`/`comparePoint`/`intersectsNode` — `dom.rs:6870-6944`).
`createContextualFragment` (DOM Parsing & Serialization spec) was never
added. Note `extractContents`/`cloneContents` are also stubs (return
`null`) and `insertNode` is a naive parent-append rather than a proper
position-aware insert — pre-existing gaps, out of scope for this report,
worth folding into the same fix pass since they share the same object.

## Масштаб

3 subtests in `html/semantics/scripting-1/the-script-element/` (all in the
`scheduler:` inline-script-via-Range test group). Small count here, but
`createContextualFragment` is a commonly used DOM API for HTML-string-to-
fragment parsing outside of `innerHTML` (used by several JS frameworks'
templating paths) — likely to resurface as a wider-impact finding once a
WPT category that exercises `Range`/DOM-parsing more heavily is run.

## Фикс (2026-09-05, P3)

`createContextualFragment(fragmentHtml)` added next to the other `Range`
methods in `_lumen_make_range` (`web_api_shim_mid.js`): parses the string
through the already-existing `_lumen_parse_html_fragment` (same helper
`innerHTML`/`insertAdjacentHTML`/`outerHTML` use elsewhere in this file),
collects the resulting nodes into a fresh `DocumentFragment`
(`_lumen_create_fragment` + `_lumen_append_child`), and returns it wrapped
through `_lumen_make_document_fragment`.

The spec calls for parsing with the range's start node as context element
(affects e.g. how a bare `<td>` parses); `_lumen_parse_html_fragment` has
no context-element parameter, so this reuses the same "body fragment"
approximation `innerHTML`/`insertAdjacentHTML` already make in this file —
not a new gap introduced by this fix.

`extractContents`/`cloneContents` stubs left untouched, as originally
scoped out above.

Verified live via `--mcp-live-port`: `typeof r.createContextualFragment
=== 'function'`, returned fragment has `nodeType === 11`, two child nodes
with expected text/id, and appending the fragment into the live document
works.

`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` clean. `scripts/scoped-test.sh` clean except the pre-existing,
unrelated [BUG-997](BUG-997-OPEN.md).
