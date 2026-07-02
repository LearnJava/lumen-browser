# Ph3 — Migrate JS engine to V8 (rusty_v8)

**Developer:** P1
**Branch:** `p1-ph3-v8-migration`
**Size:** XL (~3000 lines across 6 crates, phased over multiple sessions)
**Crates:** `lumen-js`, `lumen-shell`
**Phase:** 3 (v1.0 — do not start before Phase 2 closes and v0.5.0 ships)

---

## Status

**Phase 3 — unlocked (v0.5.0 shipped), not started. Recommended as the highest-leverage
Phase 3 item after the RP render-parity fixes, but NOT the immediate next step.**

### Audit 2026-07-02 — evidence for/against migrating now

Real-world audit (14 sites vs Edge, headless `--screenshot`) put hard numbers on the JS
problem:

- **github.com did not finish rendering in 280 s.** All ~50 script bundles fetched by 6.6 s,
  then silence — the stall is in JS execution and/or layout, not the network. QuickJS is an
  interpreter with no JIT; on megabytes of modern bundle JS it is 10–100× slower than V8 and
  in practice hangs.
- Static/server-rendered sites (Hacker News, gnu.org, example.com, w3.org) render fine — their
  content does not depend on heavy client JS. QuickJS is adequate there.

**Recommendation: do the render-parity (RP-*) fixes first, V8 second.** Rationale:

1. The *majority* of "renders unlike Edge" defects the audit found are **not** JS-engine
   problems — they are: external SVG not decoded (RP-5), `@media print` link leak (BUG-268),
   no synthetic bold (RP-6), anti-bot 403 (RP-7), and the CPU-rasterizer blow-up (BUG-267).
   None of these get better by swapping the JS engine, and together they account for most of
   what a user sees wrong on a page that *does* load. They are also far cheaper (M/L each vs
   XL for V8).
2. V8 is an **XL, multi-session, high-risk** task (Windows linking, 15–150 MB binary, ~35
   binding modules to port, startup-snapshot work). Starting it before the cheap parity wins
   land means the browser still looks broken on simple sites while a huge migration is in
   flight.
3. But V8 is the **only** fix for the class github.com falls into (heavy SPAs), and it also
   unblocks the long-blocked heap-snapshot task 10C.2. Once RP-* is done, V8 is the single
   biggest remaining lever for "open arbitrary sites like Edge" and should be the flagship
   Phase 3 effort.

**Interim mitigation (optional, before committing to V8):** add a hard JS execution budget /
watchdog so pages like github.com fail gracefully (stop the script, render what parsed)
instead of hanging the render for minutes. Cheap, and improves the worst case regardless of
whether V8 lands. Track under RP-7's sibling or a small shell task if pursued.

Phase 2 target version v0.5.0 has shipped — this task is unlocked. See the RP-* briefs
(`rp5-external-svg-images.md`, `rp6-synthetic-bold-font-match.md`, `rp7-antibot-403.md`) and
`bugs/BUG-267-OPEN.md` / `bugs/BUG-268-OPEN.md` for the work that should precede it.

---

## Goal

Replace the current `rquickjs` (QuickJS) JS engine with V8 via `rusty_v8` (or
the higher-level `deno_core`) behind the existing `JsRuntime` trait, so that
real-world SPAs (React, Vue, Angular, Next.js) run at production speed.

The swap must be invisible to all callers of `PersistentJs` in `lumen-shell` and
to the `JsRuntime` trait consumers in `lumen-core`. No shell plumbing changes;
no new public API.

Secondary benefit: V8 snapshots allow true heap serialization, finally closing
task 10C.2 (full `SuspendedHeap` persistence) which has been blocked since
Phase 0 because QuickJS native-function bindings cannot survive
`JS_WriteObject`/`JS_ReadObject` round-trips.

---

## Prerequisites / motivation

### Why V8

| Concern | QuickJS (current) | V8 (target) |
|---|---|---|
| JIT | None — interpreter only | Turbofan + Sparkplug (10–100× for hot loops) |
| ES version | ES2020 | ES2024+ (V8 ships with Chrome) |
| SPA viability | React works for tiny pages; heavy apps are unusably slow | Production-grade |
| Heap snapshot | Blocked by native bindings (task 10C.2 OPEN) | `v8::ScriptCompiler::CreateCodeCache` + isolate snapshots are designed for this |
| Binary size | ~200 KB (QuickJS C) | ~15–150 MB (shared lib depending on link mode) |
| Windows linking | Simple | Complex (prebuilt `.lib` / Chromium build) |

### Memory: heap serialization blocked

From `crates/js/src/lib.rs:609–620` (`capture_raw_heap`):

> Full QuickJS heap serialisation (globals / closures / object graph via
> `JS_WriteObject`) is task 10C.2 and is blocked by our native-function
> bindings, which cannot be round-tripped through `JS_ReadObject`.

The shell currently drops the JS runtime on hibernation and re-runs inline
`<script>` blocks against the restored DOM
(`crates/shell/src/main.rs:14599–14603`). V8 startup snapshots isolate
native-binding registration from page-state capture, making true heap
round-trips achievable.

### Decision record

ADR-004 (`docs/decisions/ADR-004-js-runtime.md`) explicitly plans this swap:

> Use `rquickjs` (QuickJS) for Phase 0–2. Switch to V8 via `rusty_v8` for
> v1.0+ when SPA support becomes required. The JS engine is isolated behind
> the `JsRuntime` trait in `lumen-core::ext`. Switching implementations is a
> drop-in replacement in `lumen-js` — no API change for callers.

Also: `crates/js/Cargo.toml:32`:

> `# Permanent #5 (§5): JS engine. QuickJS for Phase 0–1; rusty_v8 planned for v1.0+.`

---

## Current state

### Engine

`crates/js/src/lib.rs` — `struct QuickJsRuntime` (line 166).

`rquickjs::Runtime` + `rquickjs::Context` owned on a dedicated `lumen-js`
thread (`js_thread_main`, line 372). All QuickJS access funnelled through a
single `QuickJsRuntime::run()` dispatcher (line 478) via bounded
`SyncSender<JsCommand>`. Per ADR-014.

### The seam: `JsRuntime` trait

`crates/core/src/ext.rs:846` — the only public interface V8 must satisfy:

```rust
pub trait JsRuntime: Send + Sync {
    fn eval(&self, script: &str) -> JsResult<JsValue>;
    fn eval_module(&self, source: &str) -> JsResult<()>;   // default: eval
    fn register_module_source(&self, specifier: &str, source: &str); // default: no-op
    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()>;
    fn get_global(&self, name: &str) -> JsResult<JsValue>;
    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue>;
    fn engine_name(&self) -> &'static str;
    fn pause(&mut self) -> JsResult<()>;         // default: no-op
    fn unpause(&mut self) -> JsResult<()>;       // default: no-op
    fn suspend(&mut self) -> JsResult<SuspendedHeap>;  // default: no-op
    fn resume(snapshot: SuspendedHeap) -> JsResult<Self> where Self: Sized;
}
```

`JsValue` (`ext.rs:936`) is a JSON-compatible enum. No V8 `Local<Value>` or
rquickjs `Value` leaks across the boundary — intentional design constraint.

### The seam: `PersistentJs` trait

`crates/shell/src/main.rs:1729` — the shell's higher-level interface over a
live page runtime (~50 methods). V8 implementation wraps a `V8JsRuntime`
struct that provides all of these, identical to the current `QuickPersistentJs`
wrapper (`main.rs:2076`).

Methods that map directly to JS calls via `eval_js()`:

| PersistentJs method | JS expression called |
|---|---|
| `tick_timers` (`main.rs:2097`) | `_lumen_tick_timers()` |
| `run_animation_frame` | `_lumen_raf_tick(timestamp)` |
| `deliver_layout_observers` | `_lumen_deliver_resize_observers();_lumen_deliver_intersection_observers()` |
| `notify_dom_content_loaded` | `_lumen_fire_dcl()` |
| `notify_window_loaded` | `_lumen_fire_load()` |
| `pump_websockets` | `_lumen_pump_websockets()` |
| `pump_sse` | `if(typeof _lumen_pump_sse==='function')_lumen_pump_sse()` |

Methods backed by `Arc<Mutex<…>>` output queues (readable from outside the JS
thread without `run()`):

- `take_navigate_request` — `nav_out: Arc<Mutex<Option<NavigateRequest>>>`
- `take_console_messages` — `console_messages: Arc<Mutex<Vec<(u8, String)>>>`
- `take_dom_dirty` / `take_raf_pending` — atomic booleans
- `take_timer_wakeup` — `timer_wakeup: Arc<Mutex<Option<f64>>>`
- `pump_workers` — `worker_messages` queue drained via `eval_js`
- `flush_canvas_updates` — canvas2d pixel queue

### DOM bindings inventory

The bulk of the work. `crates/js/src/dom.rs:233` — `install_dom_api()` —
registers ~450 `_lumen_*` native Rust functions, then evaluates `WEB_API_SHIM`
(8000+ lines of JS, `dom.rs:5915+`) that builds `document`, `window`,
`console`, and all Web APIs on top.

Additional binding modules (each calls a QuickJS-specific `install_*(&ctx)`
pattern in `install_dom`, `lib.rs:673–1200`):

- `canvas2d::install_canvas2d_bindings` (Canvas 2D — `ctx` param)
- `webgl_canvas::install_webgl_canvas`
- `worker::install_worker_bindings`
- `subtle_crypto`, `wasm`, `webrtc_stub`, `broadcast_channel`, ~35 more

Each of these must be ported to V8's `FunctionTemplate` / `ObjectTemplate`
equivalent, or wrapped behind a helper trait.

### navigator.userAgent string

`crates/js/src/dom.rs:5916` — the one manually maintained version string:

```js
userAgent: 'Lumen/0.2.0',
```

This must be updated to `Lumen/1.0.0` when Phase 3 ships (per version policy:
Phase 3 → v1.0.0). Do not change it now.

### Worker runtime creation

`crates/js/src/worker.rs` — each `new Worker(url)` spawns a dedicated thread
that creates its own `Runtime` + `Context` from scratch. V8 uses an `Isolate`
per thread with the same startup snapshot. Pattern is compatible.

---

## Architecture

### Core principle: trait behind both engines

The `JsRuntime` trait in `lumen-core::ext` already exists as the boundary.
The V8 work lives entirely in `lumen-js`. No other crate changes.

```
lumen-shell  →  PersistentJs (trait, shell-local)
                   ↓ impl
                V8PersistentJs  (proposed, replaces QuickPersistentJs)
                   wraps
                V8JsRuntime  (proposed, in lumen-js)
                   impl JsRuntime for V8JsRuntime
```

### Isolate / context / snapshot model

V8 requires one `Isolate` per thread (it is `!Send` like QuickJS). The same
channel-dispatch pattern from ADR-014 applies: a dedicated `lumen-v8` thread
owns the `Isolate` + `Context`; the handle holds a `SyncSender<V8Command>`.
`run()` behaves identically to the QuickJS version.

**Startup snapshot.** V8 startup snapshots (`v8::StartupData`) can capture the
state of the heap *after* the engine is initialized but *before* user scripts
run. The correct model:

1. At build time (or first launch): create a "base" snapshot that includes
   all native binding registrations (`FunctionTemplate`, property descriptors)
   and the evaluated `WEB_API_SHIM` JS. Freeze into `startup_snapshot.bin`.
2. Per-page runtime: create an `Isolate` from the snapshot. The context already
   has all globals. Only user scripts need to run.
3. Per-tab `SuspendedHeap`: use `v8::ScriptCompiler::CreateCodeCache` for
   closures; for full object-graph capture, V8 `HeapSnapshot` (Chrome DevTools
   Protocol `HeapProfiler.takeHeapSnapshot`) provides the read path. Write path
   via `v8::ValueSerializer`.

This directly closes task 10C.2 (full heap round-trips).

### Feature flag

Keep the `quickjs` feature flag in `lumen-shell/Cargo.toml` (line 22). Add a
sibling `v8` feature that enables the V8 backend. Both can coexist during the
transition; the final commit removes `quickjs`.

```toml
# Cargo.toml (proposed)
v8 = ["dep:lumen-js", "lumen-js/v8-backend"]
```

### Incremental DOM binding port

Do not port all ~35 modules at once. Proposed order:

1. `dom.rs` primitives + `WEB_API_SHIM` (the JavaScript shim needs zero
   changes — it is engine-agnostic JS evaluated in any V8 context)
2. `canvas2d`, `webgl_canvas` (render-critical)
3. `worker`, `shared_worker` (concurrency)
4. `subtle_crypto`, `wasm` (security-critical)
5. Remaining stubs in alphabetical order (all follow the same pattern)

Each module's V8 port follows a mechanical transformation:

| QuickJS (rquickjs) | V8 (rusty_v8) equivalent |
|---|---|
| `ctx.globals().set(name, fn)` | `context.global().set(scope, name_key, fn_template.get_function(scope))` |
| `Function::new(ctx, \|args\| …)` | `v8::FunctionTemplate::new(scope, callback)` |
| `rquickjs::Array` | `v8::Array` |
| `ctx.eval(script)` | `v8::Script::compile + .run(scope)` |
| `rquickjs::Value` | `v8::Local<v8::Value>` |

---

## Entry points

All file paths relative to the worktree root. Proposed entry points are marked.

### lumen-core (read-only boundary — no changes expected)

- **`crates/core/src/ext.rs:846`** — `pub trait JsRuntime` — the contract V8
  must satisfy. Only change needed: if `JsValue` conversions need helpers, add
  them as free functions in `lumen-js`, not in `lumen-core`.
- **`crates/core/src/ext.rs:908`** — `SuspendedHeap` — unchanged; V8 snapshot
  bytes go in `compressed` field.
- **`crates/core/src/ext.rs:982`** — `NullJsRuntime` — keep as test stub.

### lumen-js (main work)

- **`crates/js/Cargo.toml:36`** — `rquickjs = …` — [PROPOSED] replace with
  `rusty_v8 = "…"` (or `deno_core`) under `[features] v8-backend`. Keep
  `rquickjs` under `[features] quickjs-backend` during transition.
- **`crates/js/src/lib.rs:1`** — module declarations — [PROPOSED] add
  `pub mod v8_runtime;` alongside existing mods; `v8_runtime.rs` contains
  `V8JsRuntime` struct.
- **`crates/js/src/lib.rs:166`** — `struct QuickJsRuntime` — the reference
  implementation to mirror.
- **`crates/js/src/lib.rs:342`** — `struct Inner { _rt, ctx }` — [PROPOSED]
  V8 equivalent: `struct V8Inner { isolate: v8::OwnedIsolate, context: v8::Global<v8::Context> }`.
- **`crates/js/src/lib.rs:372`** — `fn js_thread_main` — [PROPOSED] V8 thread
  main: `fn v8_thread_main(cmd_rx, init_tx)` — same channel protocol, V8 setup
  replaces `Runtime::new()` + `Context::full()`.
- **`crates/js/src/lib.rs:406`** — `impl QuickJsRuntime::new` — [PROPOSED]
  `impl V8JsRuntime::new` initializes V8 platform singleton
  (`v8::Platform::new`), then spawns the thread.
- **`crates/js/src/lib.rs:478`** — `fn run<R, F>` — [PROPOSED] identical
  pattern for `V8JsRuntime::run`: same `SyncSender<V8Command>` + blocking reply.
- **`crates/js/src/lib.rs:644`** — `pub fn install_dom` — [PROPOSED]
  `V8JsRuntime::install_dom` with same signature. Calls V8-ported binding
  modules instead of rquickjs ones.
- **`crates/js/src/lib.rs:2074`** — `impl JsRuntime for QuickJsRuntime` —
  [PROPOSED] sibling `impl JsRuntime for V8JsRuntime` in `v8_runtime.rs`.
- **`crates/js/src/dom.rs:233`** — `pub fn install_dom_api` — [PROPOSED]
  V8 variant: `pub fn install_dom_api_v8(scope, ...)` with identical signature
  except `Ctx<'_>` → `v8::HandleScope<'_>`. The JS shim (`WEB_API_SHIM`,
  `dom.rs:~5915`) is engine-agnostic and evaluates unchanged.

### lumen-shell (adapter only — no logic changes)

- **`crates/shell/Cargo.toml:18`** — `default = […, "quickjs"]` — [PROPOSED]
  change to `"v8"` after full port.
- **`crates/shell/Cargo.toml:22`** — `quickjs = ["dep:lumen-js", …]` —
  [PROPOSED] add sibling `v8 = ["dep:lumen-js", "lumen-js/v8-backend"]`.
- **`crates/shell/src/main.rs:2075`** — `#[cfg(feature = "quickjs")] struct QuickPersistentJs` —
  [PROPOSED] add `#[cfg(feature = "v8")] struct V8PersistentJs { rt: lumen_js::V8JsRuntime }`.
  `impl PersistentJs for V8PersistentJs` is mechanical: same ~50 methods,
  same `eval_js`/Arc-drain pattern.
- **`crates/shell/src/main.rs:4934`** — `lumen_js::QuickJsRuntime::new()` call
  site — [PROPOSED] add `#[cfg(feature = "v8")]` branch constructing
  `lumen_js::V8JsRuntime::new()`.
- **`crates/shell/src/main.rs:14599`** — tab restore — [PROPOSED] once V8
  snapshots work, replace the "re-run scripts" fallback with a true
  `V8JsRuntime::resume(snapshot)` that restores the heap.

---

## Steps

### Phase A: Infrastructure (no user-visible change)

**A1.** Add `v8-backend` feature to `crates/js/Cargo.toml`. Add `rusty_v8` (or
`deno_core`) as an optional dependency under that feature. Keep `rquickjs`
under `quickjs-backend`. Both features disabled by default until A5.

**A2.** Create `crates/js/src/v8_runtime.rs`. Define `V8JsRuntime` (handle
struct), `V8Inner` (thread-owned), `V8Command` enum (Run/Shutdown), and
`v8_thread_main`. Implement the `run()` dispatcher with the same
`unsafe` lifetime-erasure trick as QuickJS (identical semantics, documented
in the same way).

**A3.** Implement `JsRuntime for V8JsRuntime` for the 6 required methods:
`eval`, `set_global`, `get_global`, `call_function`, `engine_name`, `resume`.
Return `Err(JsError::NotImplemented)` for all until A4 wires real V8 calls.
Add `engine_name` → `"v8"`.

**A4.** Make `eval` functional:
- `v8::Script::compile(scope, source, None).unwrap().run(scope)`
- Convert `v8::Local<v8::Value>` → `JsValue` via a helper `from_v8`
- Convert `JsValue` → `v8::Local<v8::Value>` via `to_v8`
- Run the `cargo test -p lumen-js` suite (which currently tests rquickjs) — add
  a mirror test suite tagged `#[cfg(feature = "v8-backend")]`.

**A5.** Add `v8 = ["dep:lumen-js", "lumen-js/v8-backend"]` to shell
`Cargo.toml`. Add `#[cfg(feature = "v8")] struct V8PersistentJs` with only
`eval_js` implemented (rest `todo!()`). Confirm `cargo check -p lumen-shell
--features v8` compiles.

### Phase B: Core DOM bindings

**B1.** Port `crates/js/src/dom.rs::install_dom_api` to V8. Start with
`install_primitives` and the `WEB_API_SHIM` eval. The shim itself is pure JS
and runs unmodified in V8 — only the native function registration changes.

**B2.** Port `install_dom_api` native callbacks (`_lumen_get_attr`,
`_lumen_set_attr`, `_lumen_create_element`, etc. — all registered in
`install_primitives`). Each becomes a `v8::FunctionTemplate`.

**B3.** Wire `V8JsRuntime::install_dom` to call the V8-ported
`install_dom_api_v8`. Confirm `_lumen_tick_timers`, `document.querySelector`,
and `window.location.href` work end-to-end with a test page.

### Phase C: Remaining binding modules

Port the ~35 modules in the order listed in the architecture section. Each
module:

1. Add `fn install_X_v8(scope: &mut v8::HandleScope, ctx: &v8::Local<v8::Context>, ...)`.
2. Call from `V8JsRuntime::install_dom` behind `#[cfg(feature = "v8-backend")]`.
3. Run `cargo test -p lumen-js --features v8-backend` after each module.

### Phase D: Worker runtime

Port `crates/js/src/worker.rs`: replace `rquickjs::Runtime + Context` with
`v8::OwnedIsolate + v8::Global<v8::Context>` in the worker thread. Each
worker isolate loads from the same startup snapshot.

### Phase E: Heap snapshots (closes 10C.2)

**E1.** Build the base startup snapshot: evaluate `WEB_API_SHIM` and all native
binding registrations into a `v8::SnapshotCreator`, call `create_blob()`, save
as `assets/v8-startup.bin` (committed to repo).

**E2.** At runtime, load the snapshot blob and pass to
`v8::Isolate::new(v8::CreateParams::default().snapshot_blob(...))`.

**E3.** Implement `V8JsRuntime::suspend()`:
serialize page-JS objects (closures, globals set by user scripts) using
`v8::ValueSerializer`. Store in `SuspendedHeap.compressed` (zstd).

**E4.** Implement `V8JsRuntime::resume(snapshot)`:
deserialize via `v8::ValueDeserializer`, restore globals. Remove the
"re-run scripts" fallback from `shell/src/main.rs:14599`.

### Phase F: Cleanup

**F1.** Remove `quickjs-backend` feature and `rquickjs` dependency from
`lumen-js`.

**F2.** Remove `#[cfg(feature = "quickjs")]` blocks from `lumen-shell`.

**F3.** Update `crates/js/Cargo.toml` description from "QuickJS implementation"
to "V8 implementation".

**F4.** Update `docs/decisions/ADR-004-js-runtime.md` status to "Superseded",
write `ADR-015-v8-migration.md`.

**F5.** Update `engine_name()` return to `"v8"` (already done in A3).

**F6.** The `navigator.userAgent` string at `dom.rs:5916` should read
`'Lumen/1.0.0'` at Phase 3 ship — update it in the version-bump commit.

---

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| V8 Windows linking complexity | High | Use prebuilt `rusty_v8` crates.io releases (they ship prebuilt `.lib` for MSVC); avoid building V8 from source. Pin the version. |
| Binary size bloat (~15–150 MB) | High | Acceptable for v1.0; document in `README.md`. The `quickjs` feature remains for embedded/CI targets. |
| `v8::Local<'_>` lifetime constraints make porting `run()` dispatcher harder | Medium | V8 requires `HandleScope` on the stack; the blocking-dispatch pattern (`run()` blocks until the job completes) is still correct — the scope lives entirely within the job closure on the JS thread. |
| Startup snapshot invalidated by native binding API changes | Low-Medium | Regenerate `assets/v8-startup.bin` whenever native bindings change (add to CI check: snapshot hash in `CAPABILITIES.md`). |
| `wasm` module (`lumen_js::wasm`) uses QuickJS `Persistent<>` for GC roots | Medium | V8 uses `v8::Global<T>` for the same purpose. Replace one-for-one; the `wasm::clear_registry()` teardown call pattern (`lib.rs:401`) remains. |
| Decorators transformer (`decorators::maybe_transform_decorators`) is QuickJS-specific | Low | The transformer is pure Rust source rewriting; call it before passing source to any engine. No change needed. |
| QuickJS `call_function` `__lum_args__` global workaround (`lib.rs:2126`) | Low (eliminated) | V8 exposes `Function::call_with_args` natively; the workaround is dropped. |

---

## Tests

### New tests required in `crates/js`

- `tests/v8_eval.rs` — basic `eval` / `set_global` / `get_global` / `call_function`
  round-trips, tagged `#[cfg(feature = "v8-backend")]`.
- `tests/v8_dom.rs` — install DOM, run `document.createElement`, `querySelector`
  via QuickJS-compatible helpers.
- `tests/v8_module.rs` — `eval_module` with a relative `import`.
- `tests/v8_snapshot.rs` — `suspend()` → `resume()` round-trip preserves a
  global variable.

### Existing tests to keep passing

- `cargo test -p lumen-js` (rquickjs path) must stay green during the entire
  transition (Phases A–E). Remove only after Phase F cleanup.
- `cargo test -p lumen-shell` — `PersistentJs` trait object tests (if any) must
  pass with both `--features quickjs` and `--features v8`.

---

## Definition of done

1. `cargo build -p lumen-shell --no-default-features --features v8` succeeds on
   Windows (MSVC toolchain) and Linux.
2. `cargo test -p lumen-js --features v8-backend` — all tests pass.
3. `cargo run -p lumen-shell -- samples/page.html` with `--features v8` renders
   the test page (document title, colors, text layout match QuickJS output).
4. React 18 `create-react-app` baseline demo page loads without JS error
   (measured via `take_console_messages`).
5. `PersistentJs::suspend()` + `resume()` round-trip preserves a
   `window.__test = 42` global across tabs (closes 10C.2).
6. `rquickjs` removed from `Cargo.lock`.
7. `ADR-015-v8-migration.md` written and committed.
8. `CAPABILITIES.md` updated: JS engine row → V8.
9. `CSS-SPECS.md` not changed (CSS is unaffected).
10. `navigator.userAgent` updated to `'Lumen/1.0.0'` in `dom.rs:5916`.
