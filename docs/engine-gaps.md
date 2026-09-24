# Live engine gaps a probe will walk into

**Read this before writing a probe.** These are open defects, not history: a probe that depends on one of them measures the bug instead of its subject, and a page that goes silent because of one reads exactly like a page whose feature you were testing does not work.

Scope and lifecycle:

- The authoritative list of defects is [`BUGS.md`](../BUGS.md); this page is the subset that **changes how you write a probe**, phrased as "do this instead".
- A line dies here when its bug moves to `BUGS-FIXED.md` — delete it, do not annotate it as fixed.
- Method (what counts as evidence, how to launch a probe, how to read a WPT failure) — [`probe-method.md`](probe-method.md). Engine-side implementation traps — `subsystems/<crate>.md`.

Moved out of `CLAUDE.md` on 2026-09-03: the list is only relevant to probe/triage work, and it was being loaded in full at the start of every session regardless of the task.

---

## Harness limits — the probe cannot see what you think it sees

- **`fetch()`/`XMLHttpRequest` do nothing in the headless dump modes and on a `file://` page** — the runtime is handed `fetch_provider = None` and answers `false` without logging, so the failure is indistinguishable from a blocked request. A probe that needs real network must drive a live window (`--mcp-live-port`) and be served over http from the same origin.
- **`AutomationCommand::Screenshot` (`resource://screenshot` over `--mcp-live-port`) cannot see ANY per-frame overlay.** `render_current_page_to_png` rasterizes `self.display_list`, but the caret bar, spellcheck squiggles, validation tooltips and the DS-15 anonymous-profile border are built fresh every frame as local `page_buf`/`anim_dl`/`overlay_buf` values inside `on_redraw_requested` and handed straight to `r.render(…)` — never written back into `self.display_list`. Measured on the page's own already-shipped `<input>` caret (FRAME-7 slice 1): click + `type` through MCP updates the value, but the automation screenshot shows no caret at all. A probe needing to *see* one of these overlays has no automation surface today — the absence is not evidence that the overlay is missing.
- **A headless dump mode is a good JS probe harness, but only for synchronous script** — `lumen --dump-layout <page>` runs the page's scripts and `console.log` lands on stderr prefixed `[JS] `, which makes it the cheapest way to read a DOM-level result without a window. What it will not give you is anything behind a timer: a `setTimeout(…, 0)` callback simply never logs, because the one-shot run finishes first. Silent, so an absent line reads as «the API returned nothing» rather than «the callback never ran» — measured 2026-09-09 while probing [BUG-982](../bugs/BUG-982-FIXED.md). Anything asynchronous needs a live window.
- **A spacer that paints nothing gives the page no scroll** — `content_height` comes from the display list, so `<div style="height:4000px">` leaves `max_scroll()` at 0 and `scrollTo` genuinely does nothing. Give it a background.

- **MCP `eval` answers `JS context not available` when the engine thread is merely busy** for more than 5 s ([BUG-1145](../bugs/BUG-1145-OPEN.md)) — on a heavy page (cnbc, github) that is a timeout, not a missing context; retry later instead of concluding the page has no JS.

## Events and dispatch

- **There is no `navigation` global at all** — `'onnavigate' in navigation` and every other Navigation API feature-detect throws `ReferenceError` before it even gets to answer `false` ([BUG-881](../bugs/BUG-881-OPEN.md)). What a probe should still not expect is shadow-tree retargeting, which is unmodelled, so a path crossing a shadow boundary lists the real nodes.

## Resource loading

- **`<img>` fires neither `load` nor `error` on any insertion path, and `img.complete` is `undefined`** ([BUG-630](../bugs/BUG-630-OPEN.md)). Never sequence a probe on an image arriving, and never read a silent `<img>` as evidence that a policy blocked it.
- **`<object data>` and `<embed src>` never fetch** ([BUG-798](../bugs/BUG-798-OPEN.md)); `<input type=image>` and SVG `<image>` fetch but fire no `load`/`error`. A probe needing a subresource should use `<link rel=stylesheet>`, `<script src>` or `fetch()`.
- **No outgoing request carries `Referer` or `Origin`** — not a subresource, not `fetch()`, not a same-origin POST ([BUG-859](../bugs/BUG-859-OPEN.md)), although `docs/plan/privacy.md` promises `strict-origin-when-cross-origin`.

## Media

- **Only an animated GIF decodes as a media resource**, so `canPlayType` answers `""` for mp4/webm/ogg and resource selection ends in `MEDIA_ERR_SRC_NOT_SUPPORTED` **without issuing a request at all** — the server never sees the file (`GAP-MEDIADECODE`, measured 2026-09-01). A probe that waits for `loadstart`/`loadedmetadata`/`canplay`/`play` can only hang; only the `error` half arrives.
- **`<video src="">`/`<audio src="">` never fire `loadstart`** — `<video>` still fires `error`; `<audio>` fires neither event at all ([BUG-955](../bugs/BUG-955-OPEN.md)). A probe arming `loadstart` before its assertions hangs on an empty-string `src`, not just on a missing one.

## DOM / CSSOM surface that is simply absent

- **`document.referrer` is `undefined`** ([BUG-1121](../bugs/BUG-1121-OPEN.md)) and **`document.cookie` never stores anything** — reads `''` after every write ([BUG-1119](../bugs/BUG-1119-OPEN.md)). A probe that round-trips state through a cookie or logs the referrer measures these, not its subject.
- **DOM methods live on a hidden wrapper prototype, not on `Element.prototype`/`Node.prototype`** ([BUG-1122](../bugs/BUG-1122-OPEN.md)), and `EventTarget.prototype.addEventListener.call(node)` throws ([BUG-1123](../bugs/BUG-1123-OPEN.md)). Call methods on instances; a feature-detect written as `'x' in Element.prototype` answers `false`.
- **`<script defer>` runs in document order, before later inline scripts** ([BUG-1120](../bugs/BUG-1120-OPEN.md)), and a script-inserted `<script src>` neither delays `window` `load` ([BUG-1129](../bugs/BUG-1129-OPEN.md)) nor gets its `load` right after executing ([BUG-1128](../bugs/BUG-1128-OPEN.md)). Do not order a probe's steps by `defer` or by script `onload`.
- Missing as globals: `StaticRange`, `XSLTProcessor`/`document.evaluate`, `document.forms`/`scripts`/`links`. `DOMRect`/`DOMPoint`/`DOMMatrix`/`DOMQuad`/`getClientRects` shipped 2026-09-05 (GAP-GEOM). The CSSOM's write half — `new CSSStyleSheet()`, `insertRule`/`deleteRule`, `adoptedStyleSheets` — shipped 2026-09-06 (CSSOM-5); a shadow root's `adoptedStyleSheets`/own `<style>` still don't reach paint (no shadow-scoped cascade at all).
- **An MCP `click` that misses its target now *fails*, it no longer answers `success`** ([BUG-1044](../bugs/BUG-1044-FIXED.md), 2026-09-11): when the hit test at the resolved point returns a node that is neither the target nor its descendant, `click`/`type` answer `Element click intercepted: point (x, y) hits <tag>#id (node N) instead of the target element (node M)` and dispatch nothing. A probe no longer has to assert on the event *target* to notice a miss — but it does have to expect an error where it used to read `success`, and an element that is covered (overlay, `position:absolute` sibling) is now a hard failure rather than a silent one. (The `<button>`-shaped instance — a control with no CSS width laying out zero-wide, so every click fell through to the parent — is gone since [BUG-926](../bugs/BUG-926-FIXED.md), 2026-09-10.)

## Networking, storage, policy

- **CSP is parsed and never enforced**, and `securitypolicyviolation` is dispatched nowhere ([BUG-811](../bugs/BUG-811-OPEN.md)) — a wait on it can only hang.
- **`sessionStorage` has no quota**, so a `while (true)` filling it hangs the page ([BUG-870](../bugs/BUG-870-OPEN.md)).
- **A leaked IndexedDB connection stalls every later upgrade and delete on that name** — correct per spec, but it means a probe must close its connections or the next test waits forever.

## WPT-specific

- **Any element-targeted `test_driver.*` call can still die before it reaches the executor**: `set_permission`/`get_computed_role`/`get_computed_label` are not implemented in the executor ([BUG-1014](../bugs/BUG-1014-OPEN.md)) and still reject with `ActionError`; `click`, `action_sequence`, `send_keys`, `delete_all_cookies` and `generate_test_report` are (WPT-RUN-12).
- **An id-less element still dies with `<path> is not a valid selector`** — the vendored `testdriver-extra.js::get_selector` serialises it as `:root > *|body:nth-child(2)` ([BUG-1063](../bugs/BUG-1063-OPEN.md): the selector parser rejects every `*|E`/`|E`/`*|*` type selector). An element **with** an `id` now parses fine (`#\61 \62 \63 ` escapes fixed 2026-09-23, BUG-1065), as does plain `#abc`/`:root > body:nth-child(2)`. A `focus-navigation`-shaped probe on an id-less element still measures nothing — read the failure as BUG-1063, not as the feature under test (a WPT test page may not be edited to dodge it).
