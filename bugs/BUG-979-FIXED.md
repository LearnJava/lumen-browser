# BUG-979: `contentWindow`/named-window facade (`winFacade`) is a fixed
IDL-property whitelist, not a live view of the frame's own JS global object —
any global the frame's own script defines is unreachable from the parent

**Статус:** FIXED 2026-09-19 (P3)
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

## Исправлено (2026-09-19, P3)

**Решение согласовано с пользователем явно** (see conversation, 2026-09-19):
between "add a real synchronous cross-thread call" (deviates from the
bridge's own repeatedly-documented "isolate boundary is always async"
invariant — see slices 4/6-10 in `frame_bridge.rs`'s module doc) and "an
async Promise-shaped fallback matching the existing pattern" (doesn't close
the real WPT case, whose result is used the same synchronous turn), the user
picked the former.

Each `V8JsRuntime` already lives on its own dedicated thread and already
accepts blocking cross-thread jobs — `run`/`eval`/`get_global`/
`call_function` all block the calling thread until the target thread's job
completes (`v8_runtime/eval.rs`). A new object-safe trait,
[`FramePeerBridge`](../crates/js/src/frame_peer_bridge.rs), is a thin
wrapper around that existing channel — implemented once, by `V8JsRuntime`
(`peer_global_get`/`peer_global_call`, appended to `v8_runtime/eval.rs`).
Both methods return an envelope `JsValue::Object`
(`{"kind":"value"|"function"|"absent"|"error", …}`) instead of a `JsResult`:
the native-function glue (`v8_compat.rs`'s `into_v8_fnN`) has no path to
turn a Rust `Err` into a thrown JS exception, so failure is encoded as data
and the JS shim throws it itself.

Plumbing the handle from shell to `frame_bridge.rs`'s registry:
- `V8PersistentJs.rt` became `Arc<V8JsRuntime>` (was owned by value) so a
  peer's registry can hold its own clone for the same lifetime it already
  keeps the peer's `Arc<Mutex<Document>>` for.
- `PersistentJs` gained `frame_peer_bridge(&self) -> Option<Arc<dyn
  FramePeerBridge>>`, letting `frames.rs` ask either side (`parent_js`/
  `child_js`) for its own handle without reaching into the concrete
  `V8PersistentJs` type it never sees (only the trait object).
- `register_iframe_document`/`register_parent_document`/
  `register_top_document` gained a `peer` parameter; `frames.rs` only passes
  it when the binding is `accessible` (same-origin) — the same gate
  `.document`/mutation natives already use, since a global read/call is
  strictly more powerful than anything the existing `_lumen_f_*` whitelist
  exposes. `register_top_document`'s peer stays `None` for now (only
  `contentWindow`/`window.parent` — the shapes this bug's WPT repro
  exercises — are wired; `window.top`'s own runtime isn't threaded down
  through `top_doc` yet).
- New natives `_lumen_f_global_get`/`_lumen_f_global_call`
  (`crates/js/src/frame_bridge_globals.rs`, a new file — `frame_bridge.rs`
  was already over the 2000-line cap) resolve `bid` via the bridge's
  existing `resolve_slot`, gate on `binding.accessible`, and clone the peer
  handle before dropping the registry lock (so the blocking call doesn't
  hold up unrelated natives on the same registry).
- `winFacade` is now wrapped in a `Proxy`: the `get` trap defers to real own
  properties first (the fixed IDL set stays untouched, and `Object.prototype`
  methods like `toString` don't pay for a cross-isolate round-trip), and only
  on a genuine miss asks the bridge for the peer's real
  `globalThis[prop]`. A `"function"` envelope becomes a JS wrapper that
  synchronously calls through via `_lumen_f_global_call` and re-throws any
  `"error"` envelope as a real `Error`.

**Reentrancy**, the one real risk the always-async design was avoiding: if
frame A synchronously calls a global function of frame B, and that function
synchronously calls a global of frame A right back, both runtime threads
block on each other's `run()` forever. `enter_call`/`CallGuard`
(`frame_peer_bridge.rs`) guard against exactly that cycle — a global process
set of open `(from_doc_ptr, to_doc_ptr)` edges refuses (returns an error
envelope, does not block) a call from A into B while the reverse edge B→A is
already open.

**Regression caught before commit:** wrapping `w` in a Proxy broke `w.window
=== w`/`.self`/`.frames`/ancestor `.parent`/`.top` (three existing tests) —
those were plain `w.window = w` assignments pointing at the pre-Proxy raw
target. Fixed by making them lazy getters reading `wins[bid]` (the cached
Proxy) instead — safe because nothing reads them before `wins[bid]` is
populated at the end of the same synchronous `winFacade` call.

8 new tests in `frame_bridge_globals::tests`: plain-global read, absent
reads as `"absent"` not a thrown error, a function value reads as the
`"function"` marker, calling a global function and reading its return value
(the direct analogue of the bug's `testWindow.prepareForTest(...)`), a
thrown exception surfaces as an `"error"` envelope, a cross-origin binding
never leaks a global, and one end-to-end test through
`_lumen_frame_content_window` (not the raw natives) confirming both the
Proxy fallback and that the fixed IDL set (`postMessage`/`closed`) still
resolves without a bridge round-trip.

`cargo test -p lumen-js --features v8-backend frame_bridge` 60/60 (frame
bridge + new module), `cargo test -p lumen-shell --features v8 -- frame`
102/102, `cargo clippy --workspace --all-targets -- -D warnings` clean.
JS/bridge only, no pixels touched.

Adjacent [BUG-957](BUG-957-OPEN.md) (bolt `addEventListener`/
`removeEventListener`/`dispatchEvent` onto the bare `winFacade`) stays open —
still a structurally different fix: it needs a callback to cross the isolate
boundary, which this slice does not provide (`JsValue` cannot carry a
function; a listener argument passed through `_lumen_f_global_call` degrades
to an inert object, same as any other non-JSON-shaped argument).
