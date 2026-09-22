# Live engine gaps a probe will walk into

**Read this before writing a probe.** These are open defects, not history: a probe that depends on one of them measures the bug instead of its subject, and a page that goes silent because of one reads exactly like a page whose feature you were testing does not work.

Scope and lifecycle:

- The authoritative list of defects is [`BUGS.md`](../BUGS.md); this page is the subset that **changes how you write a probe**, phrased as "do this instead".
- A line dies here when its bug moves to `BUGS-FIXED.md` — delete it, do not annotate it as fixed.
- Method (what counts as evidence, how to launch a probe, how to read a WPT failure) — [`probe-method.md`](probe-method.md). Engine-side implementation traps — `subsystems/<crate>.md`.

**Drift, 2026-09-23 (sweep queued in `STATUS-P6.md`):** twelve lines below still cite bugs that are already FIXED or DUPLICATE — BUG-577/622/651/685/786/873/874/883/885/926/982/1044 — against the rule above. Re-probe before trusting any of those lines.

Moved out of `CLAUDE.md` on 2026-09-03: the list is only relevant to probe/triage work, and it was being loaded in full at the start of every session regardless of the task.

---

## Harness limits — the probe cannot see what you think it sees

- **`fetch()`/`XMLHttpRequest` do nothing in the headless dump modes and on a `file://` page** — the runtime is handed `fetch_provider = None` and answers `false` without logging, so the failure is indistinguishable from a blocked request. A probe that needs real network must drive a live window (`--mcp-live-port`) and be served over http from the same origin.
- **A `file://` URL passed as the initial CLI page argument does not load** ([BUG-651](../bugs/BUG-651-FIXED.md)) — `PageSource::from_arg` never strips the scheme (the sibling used by JS/BiDi navigation does). Start on `about:blank` and navigate.
- **`AutomationCommand::Screenshot` (`resource://screenshot` over `--mcp-live-port`) cannot see ANY per-frame overlay.** `render_current_page_to_png` rasterizes `self.display_list`, but the caret bar, spellcheck squiggles, validation tooltips and the DS-15 anonymous-profile border are built fresh every frame as local `page_buf`/`anim_dl`/`overlay_buf` values inside `on_redraw_requested` and handed straight to `r.render(…)` — never written back into `self.display_list`. Measured on the page's own already-shipped `<input>` caret (FRAME-7 slice 1): click + `type` through MCP updates the value, but the automation screenshot shows no caret at all. A probe needing to *see* one of these overlays has no automation surface today — the absence is not evidence that the overlay is missing.
- **A headless dump mode is a good JS probe harness, but only for synchronous script** — `lumen --dump-layout <page>` runs the page's scripts and `console.log` lands on stderr prefixed `[JS] `, which makes it the cheapest way to read a DOM-level result without a window. What it will not give you is anything behind a timer: a `setTimeout(…, 0)` callback simply never logs, because the one-shot run finishes first. Silent, so an absent line reads as «the API returned nothing» rather than «the callback never ran» — measured 2026-09-09 while probing [BUG-982](../bugs/BUG-982-FIXED.md). Anything asynchronous needs a live window.
- **A spacer that paints nothing gives the page no scroll** — `content_height` comes from the display list, so `<div style="height:4000px">` leaves `max_scroll()` at 0 and `scrollTo` genuinely does nothing. Give it a background.

## Events and dispatch

- **There is no `navigation` global at all** — `'onnavigate' in navigation` and every other Navigation API feature-detect throws `ReferenceError` before it even gets to answer `false` ([BUG-881](../bugs/BUG-881-OPEN.md)). `'onX' in Y` on `window`/`document` is reliable again since 2026-09-12 ([BUG-874](../bugs/BUG-874-FIXED.md)). Event propagation itself is no longer a gap — the full capture/target/bubble path with `window` as the last hop landed 2026-09-10 ([BUG-873](../bugs/BUG-873-FIXED.md)), and so did `composedPath()` ([BUG-577](../bugs/BUG-577-FIXED.md)); what a probe should still not expect is shadow-tree retargeting, which is unmodelled, so a path crossing a shadow boundary lists the real nodes.

## Navigation, frames and documents

- **`window.open()` and `<a target=_blank>` replace the *calling* document** ([BUG-883](../bugs/BUG-883-OPEN.md)) — the opener's timers never fire again.
- **A frame inserted after the shell's single sub-document pass** (from a `load` handler, a timer, rAF), or a `src` assigned to an already-inserted frame, produces no request at all ([BUG-885](../bugs/BUG-885-FIXED.md)); a frame built by a top-level inline script loads fine. Write frames into the markup with their final URL.
- An entry made by `history.pushState(state, "")` (no URL argument) fires no `popstate` on traversal ([BUG-886](../bugs/BUG-886-OPEN.md)).
- **`.xhtml`/`.xht`/`.svg` parameter entities (`<!ENTITY % …>`), external DTD subset and `SYSTEM` entities are unimplemented** ([BUG-786](../bugs/BUG-786-FIXED.md)/[BUG-685](../bugs/BUG-685-FIXED.md), GAP-XMLDOC закрыт срезом 40 2026-09-16) — everything else in the XML/XHTML path (CDATA, HTML LS §13.2.6.5 foreign content, namespace prefixes, live `xmlns`/`xmlns:*` resolver, `lookupNamespaceURI`/`lookupPrefix`) is done; this residual is stably 0 corpus hits, low priority.

## Resource loading

- **`<img>` fires neither `load` nor `error` on any insertion path, and `img.complete` is `undefined`** ([BUG-630](../bugs/BUG-630-OPEN.md)). Never sequence a probe on an image arriving, and never read a silent `<img>` as evidence that a policy blocked it.
- **`<object data>` and `<embed src>` never fetch** ([BUG-798](../bugs/BUG-798-OPEN.md)); `<input type=image>` and SVG `<image>` fetch but fire no `load`/`error`. A probe needing a subresource should use `<link rel=stylesheet>`, `<script src>` or `fetch()`.
- **No outgoing request carries `Referer` or `Origin`** — not a subresource, not `fetch()`, not a same-origin POST ([BUG-859](../bugs/BUG-859-OPEN.md)), although `docs/plan/privacy.md` promises `strict-origin-when-cross-origin`.

## Media

- **Only an animated GIF decodes as a media resource**, so `canPlayType` answers `""` for mp4/webm/ogg and resource selection ends in `MEDIA_ERR_SRC_NOT_SUPPORTED` **without issuing a request at all** — the server never sees the file (`GAP-MEDIADECODE`, measured 2026-09-01). A probe that waits for `loadstart`/`loadedmetadata`/`canplay`/`play` can only hang; only the `error` half arrives.
- **An `<audio>` `src` is not resolved against the document base**, so a relative URL dies as `MEDIA_ERR_SRC_NOT_SUPPORTED` with no request on the server ([BUG-924](../bugs/BUG-924-OPEN.md)). `<audio>` and `<video>` are two different models — `<audio>` still dispatches synchronously — so run a media probe against both.
- **`<video src="">`/`<audio src="">` never fire `loadstart`** — `<video>` still fires `error`; `<audio>` fires neither event at all ([BUG-955](../bugs/BUG-955-OPEN.md)). A probe arming `loadstart` before its assertions hangs on an empty-string `src`, not just on a missing one.

## DOM / CSSOM surface that is simply absent

- **`DOMException` is not defined inside a dedicated `Worker`** — `new DOMException(…)`, `e instanceof DOMException` and `DOMException.NAME_ERR` throw `ReferenceError` in worker code, while the same page-side code works ([BUG-1066](../bugs/BUG-1066-OPEN.md)). A probe or `.any.js` test that runs in the `worker` global and touches `DOMException` dies at the first use; check the window variant before reading the worker one as an engine result.
- Missing as globals: `StaticRange`, `XSLTProcessor`/`document.evaluate`, `document.forms`/`scripts`/`links`, `document.defaultView` ([BUG-622](../bugs/BUG-622-DUPLICATE.md)). `DOMRect`/`DOMPoint`/`DOMMatrix`/`DOMQuad`/`getClientRects` shipped 2026-09-05 (GAP-GEOM). The CSSOM's write half — `new CSSStyleSheet()`, `insertRule`/`deleteRule`, `adoptedStyleSheets` — shipped 2026-09-06 (CSSOM-5); a shadow root's `adoptedStyleSheets`/own `<style>` still don't reach paint (no shadow-scoped cascade at all).
- **An MCP `click` that misses its target now *fails*, it no longer answers `success`** ([BUG-1044](../bugs/BUG-1044-FIXED.md), 2026-09-11): when the hit test at the resolved point returns a node that is neither the target nor its descendant, `click`/`type` answer `Element click intercepted: point (x, y) hits <tag>#id (node N) instead of the target element (node M)` and dispatch nothing. A probe no longer has to assert on the event *target* to notice a miss — but it does have to expect an error where it used to read `success`, and an element that is covered (overlay, `position:absolute` sibling) is now a hard failure rather than a silent one. (The `<button>`-shaped instance — a control with no CSS width laying out zero-wide, so every click fell through to the parent — is gone since [BUG-926](../bugs/BUG-926-FIXED.md), 2026-09-10.)

## Networking, storage, policy

- **CSP is parsed and never enforced**, and `securitypolicyviolation` is dispatched nowhere ([BUG-811](../bugs/BUG-811-OPEN.md)) — a wait on it can only hang.
- **`sessionStorage` has no quota**, so a `while (true)` filling it hangs the page ([BUG-870](../bugs/BUG-870-OPEN.md)).
- **A leaked IndexedDB connection stalls every later upgrade and delete on that name** — correct per spec, but it means a probe must close its connections or the next test waits forever.

## WPT-specific

- **Any element-targeted `test_driver.*` call can still die before it reaches the executor**: `testdriver-extra.js::get_context` reads `element.ownerDocument.defaultView`, which doesn't exist at all ([BUG-622](../bugs/BUG-622-DUPLICATE.md)) — a probe using `click`/`send_keys`/`action_sequence` on an element sees "Browsing context for element was detached" from the page side, not from Lumen. `click`, `action_sequence`, `send_keys`, `delete_all_cookies` and `generate_test_report` are implemented in the executor now (WPT-RUN-12); `set_permission`/`get_computed_role`/`get_computed_label` are not ([BUG-1014](../bugs/BUG-1014-OPEN.md)) and still reject with `ActionError`.
- **Any element-targeted `test_driver.click`/`send_keys`/`action_sequence` dies with `<path> is not a valid selector`, whether or not the element has an `id`** — the vendored `testdriver-extra.js::get_selector` serialises an id-less element as `:root > *|body:nth-child(2)` ([BUG-1063](../bugs/BUG-1063-OPEN.md): the selector parser rejects every `*|E`/`|E`/`*|*` type selector) and an element with an `id` as `#\61 \62 \63 ` ([BUG-1065](../bugs/BUG-1065-OPEN.md): it rejects every CSS escape, even `#a\bc`). Plain `#abc` and `:root > body:nth-child(2)` parse fine. The whole file ends `ERROR` before its first assertion, so a `focus-navigation` probe measures nothing — read the failure as these two bugs, not as the feature under test (a WPT test page may not be edited to dodge them).
