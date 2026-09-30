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

## Events and dispatch

- **There is no `navigation` global at all** — `'onnavigate' in navigation` and every other Navigation API feature-detect throws `ReferenceError` before it even gets to answer `false` ([BUG-881](../bugs/BUG-881-OPEN.md)). What a probe should still not expect is shadow-tree retargeting, which is unmodelled, so a path crossing a shadow boundary lists the real nodes.

## Resource loading

- **A script-made `<img>` fires `load`/`error` reliably only via plain `src` in the top-level document** ([BUG-1048](../bugs/BUG-1048-FIXED.md) fixed that path, including a never-inserted `new Image()`). Inside an iframe a *detached* image stays silent (the fetch itself starts at once; [BUG-1148](../bugs/BUG-1148-FIXED.md) covered `srcset`/`<picture>`, hibernation and the iframe fetch, [BUG-1230](../bugs/BUG-1230-OPEN.md)) — never read that silence as evidence that a policy blocked it.
- **An SVG document inside `<object>`/`<embed>` is painted from the original bytes by resvg** (OBJECT-1 slice 5): `getSVGDocument()` returns a live DOM, but mutating it changes nothing on screen, and its `documentElement` is `<html>`, not `<svg>`. `<input type=image>` and SVG `<image>` fetch but fire no `load`/`error`. A probe needing a subresource should use `<link rel=stylesheet>`, `<script src>` or `fetch()`.

## Media

- **Only an animated GIF decodes as a media resource**, so `canPlayType` answers `""` for mp4/webm/ogg and resource selection ends in `MEDIA_ERR_SRC_NOT_SUPPORTED` **without issuing a request at all** — the server never sees the file (`GAP-MEDIADECODE`, measured 2026-09-01). A probe that waits for `loadstart`/`loadedmetadata`/`canplay`/`play` can only hang; only the `error` half arrives.
- **`<video src="">`/`<audio src="">` never fire `loadstart`** — `<video>` still fires `error`; `<audio>` fires neither event at all ([BUG-955](../bugs/BUG-955-OPEN.md)). A probe arming `loadstart` before its assertions hangs on an empty-string `src`, not just on a missing one.

## DOM / CSSOM surface that is simply absent

- **`document.referrer` inside an `<iframe>` is always `''`** ([BUG-1231](../bugs/BUG-1231-OPEN.md)). A probe that logs the referrer there measures this, not its subject.
- **A parser-inserted classic `async` script still runs in document order.** Do not order a probe's steps by `async`. A script-inserted `<script src>` does delay `window` `load` ([BUG-1129](../bugs/BUG-1129-FIXED.md)), and a classic one's own `load` follows its body ([BUG-1128](../bugs/BUG-1128-FIXED.md)); a module one's still hops a microtask.
- Missing as globals: `StaticRange`, `XSLTProcessor`/`document.evaluate`, `document.forms`/`scripts`/`links`. `DOMRect`/`DOMPoint`/`DOMMatrix`/`DOMQuad`/`getClientRects` shipped 2026-09-05 (GAP-GEOM). The CSSOM's write half — `new CSSStyleSheet()`, `insertRule`/`deleteRule`, `adoptedStyleSheets` — shipped 2026-09-06 (CSSOM-5); a shadow root's `adoptedStyleSheets`/own `<style>` still don't reach paint (no shadow-scoped cascade at all).
- **An MCP `click` that misses its target now *fails*, it no longer answers `success`** ([BUG-1044](../bugs/BUG-1044-FIXED.md), 2026-09-11): when the hit test at the resolved point returns a node that is neither the target nor its descendant, `click`/`type` answer `Element click intercepted: point (x, y) hits <tag>#id (node N) instead of the target element (node M)` and dispatch nothing. A probe no longer has to assert on the event *target* to notice a miss — but it does have to expect an error where it used to read `success`, and an element that is covered (overlay, `position:absolute` sibling) is now a hard failure rather than a silent one. (The `<button>`-shaped instance — a control with no CSS width laying out zero-wide, so every click fell through to the parent — is gone since [BUG-926](../bugs/BUG-926-FIXED.md), 2026-09-10.)
- **`getComputedStyle(el)` is empty (`length === 0`, every property `''`) for an element that owns no box** ([BUG-1191](../bugs/BUG-1191-OPEN.md)): an empty `<span>`, anything under `display: none` or `content-visibility: hidden` (closed `<details>` content included). Give the probed element text or `display: block`.
- **Adjacent inline elements with different `background` paint one background** ([BUG-1190](../bugs/BUG-1190-OPEN.md)) — `<span>a</span><span>b</span>` share one fragment painted with the first one's background. A pixel probe of such a pair measures this bug; read element geometry instead.

## Networking, storage, policy

- **`sessionStorage` has no quota**, so a `while (true)` filling it hangs the page ([BUG-870](../bugs/BUG-870-OPEN.md)).
- **A leaked IndexedDB connection stalls every later upgrade and delete on that name** — correct per spec, but it means a probe must close its connections or the next test waits forever.

## WPT-specific

- **`test_driver.Actions()` cannot hover or press Escape**: a `pointerMove` without a following `pointerDown` only records the point (no `mouseover`, no `:hover`), and `keyDown('\uE00C')` is typed as text after a click at the last point instead of arriving as `key === 'Escape'` ([BUG-1194](../bugs/BUG-1194-OPEN.md)). A hover- or hotkey-driven subtest failing under WPT measures this, not the feature — probe the feature with a dispatched `mouseover`/`KeyboardEvent` instead.
- **An id-less element used to die with `<path> is not a valid selector`** (`*|body` path of `testdriver-extra.js::get_selector`) — fixed ([BUG-1063](../bugs/BUG-1063-FIXED.md)); the `shadow-dom`/`pointerevents`/`editing` baselines still record `ERROR` until a WPT regeneration run.
