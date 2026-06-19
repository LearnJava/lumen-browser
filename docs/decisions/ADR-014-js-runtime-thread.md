# ADR-014: QuickJS runtime on a dedicated thread

## Status

Accepted

## Date

2026-06-19

## Context

The QuickJS runtime (`rquickjs::Runtime` + `Context`) is a C FFI binding and is
`!Send + !Sync`: QuickJS keeps thread-local engine state and has no internal
synchronisation. From the first JS integration the engine was created on, and
driven from, the shell's UI/winit event-loop thread, and `QuickJsRuntime` was
forced across the `JsRuntime: Send + Sync` trait bound with two **unsound**
`unsafe impl Send/Sync` blocks guarded only by an internal `Mutex<Inner>`.

This had two costs:

1. **It blocked moving heavy JS off the UI thread.** The non-blocking page
   pipeline (U-1 stage 2 / BUG-171 — run fetch + JS + layout for the final
   render off the UI thread) could not be built while the runtime was pinned to,
   and only reachable from, the UI thread.
2. **The `unsafe impl` was a latent soundness hole.** It asserted `Send`/`Sync`
   for a type that genuinely is neither; only discipline (always lock the mutex)
   kept it from being unsound.

Web Workers and Shared Workers already proved the correct pattern: each spawns
its **own** QuickJS runtime on a dedicated `std::thread` and communicates over
channels. The main runtime was the last one still living on the UI thread.

## Decision

Move the main QuickJS runtime onto its own dedicated thread, mirroring the
worker model. `QuickJsRuntime` becomes a **handle**:

- A `lumen-js` thread owns `Inner { Runtime, Context }` for its whole life; the
  runtime is created on that thread (it is `!Send`) and dropped on it.
- The handle holds a bounded `SyncSender<JsCommand>` (chosen over `Sender` so the
  handle stays `Sync`, which `JsRuntime` requires) plus the existing
  `Arc<Mutex<…>>` / `Arc<Atomic…>` output channels, which are already `Send`.
- Every QuickJS access flows through one private choke point,
  `QuickJsRuntime::run(f)`, which ships `f` to the JS thread as
  `JsCommand::Run(Box<dyn FnOnce(&Inner) + Send>)` and blocks on a reply channel.
  Because `run` blocks until the job completes, `f` may borrow from the caller's
  stack; the box's lifetime is erased to `'static` behind a single documented
  `unsafe` (sound precisely because the borrow outlives the blocking call). The
  two unsound `unsafe impl Send/Sync` blocks are deleted — the handle is now
  *genuinely* `Send + Sync`.
- `Drop` sends `JsCommand::Shutdown` and joins the thread, giving the runtime an
  explicit, thread-correct teardown.

Behaviour is unchanged for now: `run` blocks the caller, so the UI thread still
waits for JS exactly as before. B-1 only **relocates** the runtime so it *can* be
driven from a non-UI thread; the actual non-blocking pipeline is BUG-171.

State coordinated between the shell and JS that previously relied on the runtime
running on the UI thread was migrated off `thread_local`:

- `pointer_lock` and `file_input` token registry — written by the shell (UI
  thread) and read by JS bindings (JS thread). Converted from `thread_local` to a
  process-global `Mutex` (both are singleton / globally-unique-token state).
- Per-runtime `thread_local` registries that are only ever touched on the JS
  thread (canvas2d, webgl, offscreen, wasm, subtle_crypto, media/screen capture)
  are **kept** — they are now *more* consistent, since every runtime (main +
  workers) owns its own thread.

WASM import `Persistent`s are freed via `wasm::clear_registry()` in the JS
thread's teardown, before `Inner` drops, closing BUG-222 (`list_empty` abort).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep `unsafe impl Send/Sync` + `Mutex<Inner>` | Unsound; and leaves the runtime pinned to the UI thread, blocking BUG-171. |
| Per-call closures captured as `Send + 'static` (clone all needed `Arc`s) | Forces ~30 `Arc` clones into `install_dom` and owning every `&str` arg across 22 call sites — large, error-prone diff for no behavioural gain. The blocking-dispatch borrow trick avoids it. |
| Keep `thread_local` for pointer_lock / file_input, route shell access through the runtime | Adds a per-`DeviceEvent` channel round-trip for mouse-motion and several new public methods + trait plumbing; a global `Mutex` is both cheaper and semantically correct for singleton state. |
| Make the whole runtime async (move `run` to non-blocking) | That is BUG-171's job; doing it here would conflate relocation with the pipeline rework and balloon scope. |

## Consequences

- **Positive:** `QuickJsRuntime` is genuinely `Send + Sync`; the unsound
  `unsafe impl`s are gone. The runtime has an explicit lifecycle on a known
  thread, which unblocks BUG-171 (off-UI-thread pipeline) and closes BUG-222
  (WASM `Persistent` teardown). The threading model is now uniform: every runtime
  owns its thread.
- **Negative / trade-offs:** one extra OS thread per runtime (tabs already paid
  this for workers); every QuickJS call now costs a channel round-trip + a
  thread hop (microseconds, dominated by JS execution); one isolated `unsafe`
  lifetime-erasure remains in `run` (documented, sound by the blocking
  guarantee).
- **Future:** BUG-171 changes the *callers* (the heavy `LoadDone` pipeline) to
  issue `run` jobs from a background thread instead of the UI thread, which is
  what actually makes the page load non-blocking. `run` may later grow a
  non-blocking variant for fire-and-forget jobs.
