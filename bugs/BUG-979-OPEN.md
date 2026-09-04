# BUG-979: `contentWindow`/named-window facade (`winFacade`) is a fixed
IDL-property whitelist, not a live view of the frame's own JS global object —
any global the frame's own script defines is unreachable from the parent

**Статус:** OPEN
**Дата:** 2026-09-04
**Компонент:** js (`crates/js/src/frame_bridge.rs::winFacade`, lines
1917-2000ish)
**Найден:** P2, WPT-RUN-6 срез 58, живой пробой

## Механизм

`winFacade(bid)` (`frame_bridge.rs:1917`) builds the object returned by
`iframe.contentWindow`, named-window access (`window.someIframeName`),
`window.parent`/`.top`, and `MessageEvent.source` as a bare `w = {}` with a
hardcoded, enumerated set of properties: `document`, `window`/`self`/
`frames` (self-references), `parent`, `top`, `closed`, `length` (always
`0`), `frameElement`, `name`, `location`, `close`, `postMessage`. Every one
of those is either a getter that calls a specific narrow native bridge
function (`_lumen_f_url`, `_lumen_f_attr`, …) or a literal. There is no
fallback path that forwards an arbitrary property read across the isolate
boundary — the facade only ever answers the ~11 names it was built with.

That is enough to fake the handful of cross-origin-safe `WindowProxy` IDL
members the spec defines, but it silently breaks same-origin access to
anything the framed document's own script put on ITS real global object:
top-level `function foo(){}` / `var foo = …` declarations become properties
of the real `window` per spec (HTML LS §8.1.6.1), and any code that reaches
them via `iframe.contentWindow.foo`/`someIframeName.foo` gets `undefined`
instead — same-origin access, which the spec allows unrestricted, is
restricted to the same ~11-name whitelist as cross-origin access.

Related to, but distinct from, [BUG-957](BUG-957-OPEN.md) (same `winFacade`
literal, `w = {}`): BUG-957 is specifically about the three missing
`EventTarget` methods (`addEventListener`/`removeEventListener`/
`dispatchEvent`) and its fix is "bolt these three methods on". This defect is
broader and needs a structurally different fix — a live bridge to the
frame's actual global object (e.g. a native `_lumen_f_global_get(bid, name)`
call on every facade property miss), not a longer property whitelist.

## Симптом

Confirmed live (`--mcp-live-port`, minimal repro — an `<iframe name="subFrameA">`
whose own script sets `window.__mark = "..."`), 2026-09-04, `main` =
`4e745d386`:

```
subFrameA-is-element = false                      // not the element (named access does resolve through the bridge)
subFrameA-tagName = undefined
ifr.contentWindow === subFrameA = true             // same object as contentWindow
Object.keys(subFrameA) = window,self,frames,length,close,postMessage
subFrameA.contentWindow-mark THREW TypeError: Cannot read properties of undefined (reading '__mark')
```

`subFrameA.window` (the facade's own self-reference) exists — so
`subFrameA.window.__mark` doesn't throw "Cannot read properties of
undefined", it answers `undefined` for `__mark` itself, exactly the
"is not a function"/`undefined` shape the real WPT test below hits.

Real-world trigger, run through the corpus's own `testharnessreport.js`
(`serve_wpt_like.py`, matching `run_report.py`'s environment):
`/mediacapture-record/MediaRecorder-destroy-script-execution.html`. All four
of its `async_test`s do `let testWindow = subFrameX.window; testWindow.
prepareForTest(...)`, where `prepareForTest` is a plain top-level function
declared in the framed document
(`mediacapture-record/support/MediaRecorder-iframe.html`). Measured:

```
[JS error] Uncaught TypeError: testWindow.prepareForTest is not a function
[JS error] Uncaught TypeError: subFrameStop.window.prepareForTest is not a function
[JS error] Uncaught TypeError: subFrameAllTrackEnded.window.prepareForTest is not a function
PROBE harness-complete status=1 tests=4 …:3|…:3|…:3|…:1
```

harness-complete only fires at ≈10.5s wall-clock (measured across a clean
run, polling every 100ms) — none of the four `onload` handlers is wrapped in
`t.step`/`t.step_func` (they're plain `iframe.onload = function(e){…}`
assignments), so the thrown `TypeError` becomes an uncaught global error
instead of a caught test-step `FAIL`; none of the four `async_test`s ever
reaches `.done()`, and the harness only reports once its own internal
timeout elapses — the same "hangs until the harness's own internal timeout"
shape as [BUG-968](BUG-968-OPEN.md), not the fast-completion "corpus TIMEOUT
doesn't reproduce live" class documented in
[BUG-961](BUG-961-FIXED.md)/[BUG-963](BUG-963-OPEN.md) for
`console-log-large-array`/`canvas-with-padding`/`a.ping-functionality`.
Matches the WPT-RUN-5/6 corpus TIMEOUT signature for this id.

## Масштаб

Any same-origin cross-frame code that reaches a named global (function,
class, plain variable) the other frame's own script defined — not just
`EventTarget` methods (BUG-957) — breaks the same way. Common WPT idiom:
a support iframe exposes test fixtures (`window.recorder`, `window.control`,
helper functions like `prepareForTest`) for the parent to drive; every one
of those is invisible through `contentWindow`/named-window access today.

## Что нужно

Give `winFacade`'s property-miss path a fallback that queries the frame's
real global object across the isolate boundary (a native `_lumen_f_global_
get(bid, name)` alongside the existing narrow bridge calls), rather than
enumerating more IDL names by hand — the current shape structurally cannot
scale to "any global the page defines".

## Классификация WPT-RUN-6

Attributed via `_exact_id_marker("/mediacapture-record/MediaRecorder-destroy-script-execution.html")`
in `tests/wpt/timeout_audit.py` (marker `frame-facade-missing-page-globals`).
