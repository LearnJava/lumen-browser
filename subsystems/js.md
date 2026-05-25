# lumen-js

Crate providing the `JsRuntime` implementation backed by QuickJS via `rquickjs` v0.11.
Phase 0–1 engine; `rusty_v8` is planned for v1.0+.

## Scope

- `QuickJsRuntime` struct: wraps `rquickjs::Runtime + Context` under a `Mutex`.
- Implements `lumen_core::JsRuntime`: `eval`, `set_global`, `get_global`, `call_function`.
- JSON-compatible value conversion: `JsValue ↔ rquickjs::Value<'js>`.
- Shell wires it in via `features = ["quickjs"]`; without the feature `NullJsRuntime` is used.

## Done

- `QuickJsRuntime` — all four trait methods, 16 tests (eval, globals, function call, round-trip, Send+Sync). 2026-05-20.
- `call_function` dynamic-args workaround: temporary global `__lum_args__` + `fn.apply(null, __lum_args__)` eval. Reason: `rquickjs 0.11` `Function::call` requires fixed-size `IntoArgs` tuples; no `apply()` method.
- `lumen-shell` feature `quickjs` enables `QuickJsRuntime` via `run_scripts_with_dom()`.
- **JS↔DOM bindings Phase 0** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-20.
  - 24 native `_lumen_*` Rust functions exposed to QuickJS.
  - JS Web API shim: `console`, `document`, `window`, `alert`, `setTimeout` (synchronous).
  - DOM read: `getElementById`, `querySelector`, `querySelectorAll`, `getAttribute`, `tagName`, `textContent`, `parentElement`, `children`.
  - DOM write: `setAttribute`, `removeAttribute`, `textContent =`, `innerHTML =`, `createElement`, `createTextNode`, `appendChild`, `removeChild`.
  - `document.title` get/set.
  - Phase 0 querySelector: supports `#id`, `.class`, `tagname`, `*` (no compound selectors).
  - 19 DOM tests + 16 runtime tests = 35 total. All pass.
  - Shell integration: `run_scripts_with_dom` wraps `Document` in `Arc<Mutex<>>`, calls `install_dom`, drops runtime to release Arc clones, recovers `Document`.
- **Fetch API JS shim** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-22.
  - 5 native `_lumen_fetch_*` bindings: `_lumen_fetch_sync`, `_lumen_fetch_get_status`, `_lumen_fetch_get_status_text`, `_lumen_fetch_get_headers`, `_lumen_fetch_get_body`. Shared result via `Arc<Mutex<Option<FetchCache>>>`.
  - `install_dom` now accepts `Option<Arc<dyn JsFetchProvider>>` — `None` makes `fetch()` reject immediately.
  - JS classes: `AbortSignal`, `AbortController`, `Headers`, `Response`, `Request`, `fetch()` global + `window.fetch`.
  - `Response.ok` (200–299), `Response.text()` / `Response.json()` returning Promises, `Headers` case-insensitive get/set/has/delete.
  - `AbortController.abort()` sets `signal.aborted = true`.
  - 109 JS tests (was 35 before). All pass.
- **Web Storage API** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-25.
  - 12 native `_lumen_ls_*` / `_lumen_ss_*` bindings (length, key, get, set, remove, clear for localStorage + sessionStorage).
  - `install_dom` now accepts `ls_store: Option<Arc<Mutex<WebStorage>>>` — `None` → fresh in-memory store.
  - `_lumen_make_storage` JS factory + `localStorage`/`sessionStorage` globals in shim. `length` property via `Object.defineProperty` with getter.
  - `sessionStorage` — fresh `Arc::new(Mutex::new(WebStorage::default()))` per `install_dom` call (page-load isolation).
  - `localStorage` — shared `Arc<Mutex<WebStorage>>` from shell (SOP-partitioned, persists across reloads within session).
  - 8 new tests (getItem/setItem/removeItem/clear/key/length/overwrite/session-isolation). 140 JS tests total. All pass.
- **URL / URLSearchParams / performance / queueMicrotask** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_now_ms()` — native Rust function: `SystemTime::now()` as f64 milliseconds since Unix epoch.
  - `URLSearchParams` (WHATWG URL §5): parse from string/object/array, `get/getAll/set/append/delete/has/sort/size/toString/forEach/keys/values/entries`.
  - `URL` (WHATWG URL §6.1): parse absolute URLs, resolve relative URLs and protocol-relative against a base (dot-segment normalization per RFC 3986 §5.2.4). Properties: `href/protocol/hostname/host/port/pathname/search/hash/origin/username/password/searchParams` (lazy). `URL.createObjectURL` / `revokeObjectURL` stubs.
  - `performance` (W3C HR Time L2): `now()` (DOMHighResTimeStamp, time origin captured at `install_dom` call), `timeOrigin`, stub `mark/measure/getEntriesByName/getEntriesByType/clearMarks/clearMeasures`. Exposed on `window.performance`.
  - `queueMicrotask(fn)` (HTML LS §8.1.4.4): schedules via `Promise.resolve().then(fn)`; throws `TypeError` for non-function.
  - All four APIs exposed on `window.*` via post-literal assignment (avoids `var` hoisting issue with `performance`).
  - 42 new tests. 166 JS tests total. All pass.
- **DOM dirty flag / layout invalidation** (`QuickJsRuntime::dom_dirty: Arc<AtomicBool>`). 2026-05-25.
  - `dom_dirty` set to `true` by all DOM-mutating bindings: `_lumen_set_attr`, `_lumen_remove_attr`, `_lumen_set_text_content`, `_lumen_set_inner_html`, `_lumen_append_child`, `_lumen_remove_child`.
  - `QuickJsRuntime::take_dom_dirty() -> bool` — atomic swap(false); cleared after each rAF pass in the shell.
  - Shell: `PersistentJs::take_dom_dirty()` added to trait; `RedrawRequested` step 6 checks flag and calls `self.relayout()` when set.
  - Result: JS DOM mutations (textContent, setAttribute, appendChild, etc.) now cause an automatic relayout before the next paint, making interactive JS pages reflect DOM changes correctly.
- **Async setTimeout / setInterval / clearTimeout / clearInterval + scheduler.postTask** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_request_wakeup(deadline_ms: f64)` — native Rust function: writes the earliest timer deadline (Unix epoch ms) to `QuickJsRuntime::timer_wakeup: Arc<Mutex<Option<f64>>>`. Stores only the minimum deadline (min-update semantics).
  - JS timer queue (`_lumen_timers`) — plain JS array `{id, fn, deadline, interval}`. `setTimeout`/`setInterval` append; `clearTimeout`/`clearInterval` splice; `_lumen_tick_timers()` drains expired entries, reschedules intervals, runs callbacks, and calls `_lumen_request_wakeup` for the next timer.
  - Shell integration: `PersistentJs::tick_timers()` + `take_timer_wakeup()` — called in `about_to_wait`; if a timer deadline is pending, sets `ControlFlow::WaitUntil` so winit wakes up precisely at the next expiry without polling.
  - `scheduler` (W3C Prioritized Task Scheduling API): `postTask(fn, {priority?, delay?}) → Promise` (delay maps to `setTimeout`; priority ignored — Phase 2); `yield() → Promise` (defers via `setTimeout 0`). Exposed on `window.scheduler`.
  - Old synchronous stubs replaced. Timers are now correctly deferred: `setTimeout(fn, 0); x` evaluates `x` before `fn` runs.
  - 6 new tests (deferred, fires-after-tick, clearTimeout, setInterval repeat, clearInterval, scheduler.postTask). 172 JS tests total. All pass.

- **`requestAnimationFrame` / `cancelAnimationFrame`** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_mark_raf_pending()` native Rust function: sets `QuickJsRuntime::raf_pending: Arc<AtomicBool>` to `true` when JS calls `requestAnimationFrame`.
  - `QuickJsRuntime::take_raf_pending() -> bool` — atomic swap(false); read by shell after each rendering step.
  - JS: `requestAnimationFrame(fn)` queues `{id, fn}` into `_lumen_raf_callbacks`, calls `_lumen_mark_raf_pending()`, returns numeric ID. Returns 0 for non-function argument.
  - JS: `cancelAnimationFrame(id)` splices callback from queue; unknown ID is a no-op.
  - JS: `_lumen_run_raf_callbacks(timestamp_ms)` — snapshot-pattern (splice all, run, new callbacks go to next frame). Returns `true` when any callback ran.
  - Shell: `PersistentJs::run_animation_frame(timestamp_ms)` calls `_lumen_run_raf_callbacks`; `take_raf_pending()` detects animation loops and requests next redraw.
  - Shell integration: in `RedrawRequested` step 5.1 — after Rust rAF, before CSS animation tick; new rAF registered during callbacks automatically triggers next frame.
  - `window.requestAnimationFrame` and `window.cancelAnimationFrame` both wired.
  - 11 new tests (id, sequential ids, non-function→0, mark-pending, snapshot-pattern, recursive-pending, cancel, cancel-unknown, window properties). 183 JS tests total.

## Deferred

- PerformanceObserver API.
- Real MutationObserver / IntersectionObserver triggers (requires DOM mutation events).
- querySelector compound selectors (e.g. `div.class`, `#id > p`).
- `rusty_v8` backend (v1.0+).

## Invariants

- `QuickJsRuntime: Send + Sync` (enforced by `unsafe impl` + `Mutex`).
- `call_function` pollutes the global namespace with `__lum_args__` only transiently — cleaned up with `delete` after each call.
- `from_rq` maps `Type::Undefined` to `JsValue::Null` (not `Undefined`) — matches the trait docs which say "simple JSON-compatible types".
- rquickjs 0.11 `Function::call` takes `IntoArgs` (fixed-size tuples). Dynamic calls must use the eval workaround until rquickjs adds `Function::apply` or `Rest<T>: IntoArgs`.
- DOM shim: `parentElement` and `children` are defined with `enumerable: false` via `Object.defineProperty`. Prevents `from_rq`'s `obj.props()` loop from serializing these cyclic getters → infinite recursion / stack overflow.
- DOM shim: `Option<T>` in rquickjs maps `None → undefined` (not `null`). All nullable-returning native functions are wrapped with `_lumen_u2n(v)` in the shim to convert `undefined → null` as Web API requires.
- `install_dom` must be called before `eval`. Drop the runtime before `Arc::try_unwrap(doc_arc)` — closures hold Arc clones until the runtime is dropped.
- Web Storage closures capture `Arc<Mutex<WebStorage>>` clones — dropped with the runtime. The outer `Arc` in the shell's `ls_storage` map remains the authoritative store; JS mutations are immediately visible in Rust after the closure releases the lock.
