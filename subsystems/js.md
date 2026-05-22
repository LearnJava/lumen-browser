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

## Deferred

- Event loop integration (call JS on DOM events).
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
