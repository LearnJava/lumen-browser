# BUG-573: `Range.prototype.createContextualFragment` missing

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs::_lumen_make_range` object literal,
`dom.rs:6870`-ish — full method list ends at `intersectsNode`,
`dom.rs:6944`, no `createContextualFragment`)
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
