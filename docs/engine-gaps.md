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
- **A `file://` URL passed as the initial CLI page argument does not load** ([BUG-651](../bugs/BUG-651-OPEN.md)) — `PageSource::from_arg` never strips the scheme (the sibling used by JS/BiDi navigation does). Start on `about:blank` and navigate.
- **`AutomationCommand::Screenshot` (`resource://screenshot` over `--mcp-live-port`) cannot see ANY per-frame overlay.** `render_current_page_to_png` rasterizes `self.display_list`, but the caret bar, spellcheck squiggles, validation tooltips and the DS-15 anonymous-profile border are built fresh every frame as local `page_buf`/`anim_dl`/`overlay_buf` values inside `on_redraw_requested` and handed straight to `r.render(…)` — never written back into `self.display_list`. Measured on the page's own already-shipped `<input>` caret (FRAME-7 slice 1): click + `type` through MCP updates the value, but the automation screenshot shows no caret at all. A probe needing to *see* one of these overlays has no automation surface today — the absence is not evidence that the overlay is missing.
- **A spacer that paints nothing gives the page no scroll** — `content_height` comes from the display list, so `<div style="height:4000px">` leaves `max_scroll()` at 0 and `scrollTo` genuinely does nothing. Give it a background.

## Events and dispatch

- **An event dispatched from script reaches only the node it was dispatched on** — no ancestor, no `document`, no `window`, in either phase; a native click reaches `document` but not `window`; `event.target` is unset and `eventPhase` is `undefined` ([BUG-873](../bugs/BUG-873-OPEN.md)). Also: `document.on<type> = fn` sticks as a property and is never invoked, and `'onX' in Y` answers `false` although assignment works ([BUG-874](../bugs/BUG-874-OPEN.md)). Listen with `addEventListener` on the target itself; never feature-detect with `'onX' in Y`.
- **`window.postMessage` accepts only the legacy string `targetOrigin`** — `'*'`, the exact origin and `'/'` work; both dictionary forms, the one-argument form and a trailing slash drop the message silently ([BUG-717](../bugs/BUG-717-OPEN.md)). The cheapest way to sequence a probe page therefore needs the literal `'*'`.

## Navigation, frames and documents

- **`window.open()` and `<a target=_blank>` replace the *calling* document** ([BUG-883](../bugs/BUG-883-OPEN.md)) — the opener's timers never fire again.
- **A frame inserted after the shell's single sub-document pass** (from a `load` handler, a timer, rAF), or a `src` assigned to an already-inserted frame, produces no request at all ([BUG-885](../bugs/BUG-885-OPEN.md)); a frame built by a top-level inline script loads fine. Write frames into the markup with their final URL.
- **`javascript:` URLs never execute** anywhere ([BUG-884](../bugs/BUG-884-OPEN.md)); `window.close()` is a no-op and `window.closed`/`name` are `undefined` ([BUG-887](../bugs/BUG-887-OPEN.md)); `document.open()`/`close()` do not exist ([BUG-888](../bugs/BUG-888-OPEN.md)); an entry made by `history.pushState(state, "")` (no URL argument) fires no `popstate` on traversal ([BUG-886](../bugs/BUG-886-OPEN.md)).
- **An `.xhtml`/`.xht`/`.svg` page runs no scripts** — navigation has no XML path at all, so the file is HTML-parsed ([BUG-786](../bugs/BUG-786-OPEN.md)): a prefixed `<h:script src>` is never requested, a self-closing `<script src="…"/>` swallows the rest of the document, and `<![CDATA[` is a syntax error. Never CDATA-wrap a probe's script.

## Resource loading

- **`<img>` fires neither `load` nor `error` on any insertion path, and `img.complete` is `undefined`** ([BUG-630](../bugs/BUG-630-OPEN.md)). Never sequence a probe on an image arriving, and never read a silent `<img>` as evidence that a policy blocked it.
- **`<object data>` and `<embed src>` never fetch** ([BUG-798](../bugs/BUG-798-OPEN.md)); `<input type=image>` and SVG `<image>` fetch but fire no `load`/`error`. A probe needing a subresource should use `<link rel=stylesheet>`, `<script src>` or `fetch()`.
- **A script-side resource path is dead where the parser's works.** An `<img>` created by script (or a parser one given a new `src`) draws nothing through `drawImage` and rejects `createImageBitmap` with «image not yet decoded» — the bitmap store is filled once, by the parse pass ([BUG-938](../bugs/BUG-938-OPEN.md)); a `background-image` assigned from JS is never requested although the cascade shows it ([BUG-939](../bugs/BUG-939-OPEN.md)). Both fetch fine from markup, so a probe that builds its image or its background in JS measures this instead of its subject.
- **No outgoing request carries `Referer` or `Origin`** — not a subresource, not `fetch()`, not a same-origin POST ([BUG-859](../bugs/BUG-859-OPEN.md)), although `docs/plan/privacy.md` promises `strict-origin-when-cross-origin`.

## Media

- **Only an animated GIF decodes as a media resource**, so `canPlayType` answers `""` for mp4/webm/ogg and resource selection ends in `MEDIA_ERR_SRC_NOT_SUPPORTED` **without issuing a request at all** — the server never sees the file (`GAP-MEDIADECODE`, measured 2026-09-01). A probe that waits for `loadstart`/`loadedmetadata`/`canplay`/`play` can only hang; only the `error` half arrives.
- **An `<audio>` `src` is not resolved against the document base**, so a relative URL dies as `MEDIA_ERR_SRC_NOT_SUPPORTED` with no request on the server ([BUG-924](../bugs/BUG-924-OPEN.md)). `<audio>` and `<video>` are two different models — `<audio>` still dispatches synchronously — so run a media probe against both.
- **`<video src="">`/`<audio src="">` never fire `loadstart`** — `<video>` still fires `error`; `<audio>` fires neither event at all ([BUG-955](../bugs/BUG-955-OPEN.md)). A probe arming `loadstart` before its assertions hangs on an empty-string `src`, not just on a missing one.

## DOM / CSSOM surface that is simply absent

- **The CSSOM is read-only**: `document.styleSheets`/`<style>`/`<link>.sheet`/`CSSStyleSheet`/the `CSSRule` hierarchy exist since CSSOM-1 (2026-09-03), but `new CSSStyleSheet()` throws, `insertRule`/`deleteRule` don't exist, and `adoptedStyleSheets` is a plain expando ([BUG-897](../bugs/BUG-897-OPEN.md) / CSSOM-5). Also missing as globals: `DOMRect`/`DOMPoint`/`DOMMatrix`, `StaticRange`, `XSLTProcessor`/`document.evaluate`, `document.forms`/`scripts`/`links`, `getClientRects` ([BUG-478](../bugs/BUG-478-OPEN.md)), `document.defaultView` ([BUG-622](../bugs/BUG-622-OPEN.md)).
- **`Element.matches(':focus'/':focus-visible'/':focus-within')` answers `false`** even when the `:focus` *style* has been applied — the selector-matching path does not resolve dynamic pseudo-classes ([BUG-560](../bugs/BUG-560-OPEN.md)).
- **Shadow trees do not slot**: no slottable is ever assigned, `slotchange` fires nowhere ([BUG-876](../bugs/BUG-876-OPEN.md)), `host.shadowRoot` returns a fresh wrapper per read ([BUG-877](../bugs/BUG-877-OPEN.md)), and a `<script src>` in a shadow root is never requested ([BUG-878](../bugs/BUG-878-OPEN.md)).

## Networking, storage, policy

- **CSP is parsed and never enforced**, and `securitypolicyviolation` is dispatched nowhere ([BUG-811](../bugs/BUG-811-OPEN.md)) — a wait on it can only hang.
- **`new WebSocket(url)` blocks the whole document until the handshake settles** ([BUG-856](../bugs/BUG-856-OPEN.md)), and `send()` throws on anything that is neither string nor buffer ([BUG-862](../bugs/BUG-862-OPEN.md)). Open one only against a server you control.
- **`sessionStorage` has no quota**, so a `while (true)` filling it hangs the page ([BUG-870](../bugs/BUG-870-OPEN.md)).
- **A leaked IndexedDB connection stalls every later upgrade and delete on that name** — correct per spec, but it means a probe must close its connections or the next test waits forever.

## WPT-specific

- **`test_driver.click(element)` cannot work**: `testdriver.js` opens with `element.getClientRects()`, which does not exist, so it throws synchronously — fixing `defaultView` alone unblocks nothing. Of ~30 `test_driver_internal` actions the executor implements two (`click`, `generate_test_report`) ([BUG-810](../bugs/BUG-810-OPEN.md)).
- **`PerformanceObserver.supportedEntryTypes` lists `layout-shift` and never delivers one** ([BUG-809](../bugs/BUG-809-OPEN.md)) — the advertisement is why such tests TIMEOUT instead of failing. The list is a promise about the *type*, not about delivery: check for a call site before believing it.
