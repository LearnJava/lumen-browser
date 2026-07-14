# Ph3 — Migrate JS engine to V8 (rusty_v8)

**Developer:** P1
**Branch:** one branch per slice: `p1-v8-s<N>` (see Slice plan). Branch existence = slice reservation.
**Size:** XL — **12–13 mergeable slices**, each ≤1 session. NOT a single long-lived branch.
**Crates:** `lumen-js`, `lumen-shell` (adapter only), `lumen-core` (read-only boundary)
**Phase:** 3 (v1.0). Unlocked (v0.5.0 shipped 2026-06-23), not started.

---

## Revision history

- **Rev 2 (2026-07-07)** — full re-analysis against code. Corrected: Phase E was
  technically infeasible as written (ValueSerializer cannot serialize closures;
  startup snapshots with stateful native bindings don't work — see «Hard facts»);
  scale re-measured (~2× the Rev 1 estimate); added the **compat layer** as a
  mandatory prerequisite slice; replaced the monolithic Phase A–F plan with a
  slice plan (S0–S12) merged to main behind a feature flag; resolved open
  decisions (raw `v8` crate, not `deno_core`; remove `quickjs` feature at the end;
  do NOT commit snapshot blobs).
- **Rev 1 (2026-07-02)** — original brief + real-world audit evidence.

## Status

**Not started.** Sequencing recommendation from the 2026-07-02 audit still holds:
**do the render-parity fixes first (RP-5/6/7, BUG-267/268), V8 second.** Most
«renders unlike Edge» defects are not JS-engine problems and are far cheaper
(M/L vs XL). But V8 is the *only* fix for heavy SPAs (github.com never finished
rendering in 280 s — the stall is JS execution, QuickJS has no JIT) and the
single biggest remaining lever for «open arbitrary sites like Edge».

**Interim mitigation (optional, independent of V8):** a hard JS execution
budget/watchdog so pages like github.com fail gracefully (stop script, render
what parsed) instead of hanging for minutes. Cheap; improves the worst case
regardless of whether V8 lands.

---

## Goal

Replace `rquickjs` (QuickJS) with V8 via the **`v8` crate (rusty_v8)** behind the
existing `JsRuntime` trait, so that real-world SPAs (React, Vue, Next.js) run at
production speed. The swap must be invisible to all callers of `PersistentJs` in
`lumen-shell` and to `JsRuntime` consumers in `lumen-core`. No shell plumbing
changes; no new public API.

### Resolved decisions (do not re-litigate)

1. **Raw `v8` crate, NOT `deno_core`.** deno_core imposes its own event loop,
   ops model, and module system; Lumen has all three already (ADR-014
   channel-dispatch thread, `_lumen_*` natives, `register_module_source`).
2. **`quickjs` feature is removed at the end (S12).** Dual maintenance of ~380
   native bindings is a permanent tax; Rev 1's «keep quickjs for embedded/CI»
   is rejected.
3. **No committed snapshot blobs.** V8 snapshot blobs are V8-version-specific;
   a committed `assets/v8-startup.bin` goes stale on every `v8` crate bump.
   If a startup snapshot is ever built (S11, optional), generate it in
   `build.rs` or at first launch — never commit it.
4. **Slices merge to main behind the `v8-backend` feature flag** (disabled by
   default until S12). `dom.rs` and binding modules are actively touched by
   P3/P4; a multi-session branch would bleed conflicts in the 26k-line `dom.rs`.

---

## Hard facts that shaped Rev 2 (verified against code / V8 API)

### F1. `v8::ValueSerializer` cannot serialize closures — 10C.2 closes only PARTIALLY

`ValueSerializer` implements structured clone (same contract as `postMessage`):
functions and closures throw `DataCloneError`. `HeapSnapshot` (DevTools
`HeapProfiler.takeHeapSnapshot`) is read-only diagnostics — there is no restore
path. **Consequence:** `suspend()`/`resume()` can round-trip *data* (globals,
objects, arrays, primitives) but NOT closures. The «re-run inline scripts
against restored DOM» fallback (`crates/shell/src/main.rs:14599`) **stays**
after the migration. Task 10C.2 gets a partial close (data yes, closures no) —
record this honestly in `ROADMAP.md` when S11 lands.

### F2. Startup snapshots with stateful native bindings do not work

A V8 snapshot containing `FunctionTemplate`s with native callbacks requires an
`external_references` table — stable function pointers identical at snapshot
*creation* and *load*. Lumen's natives are Rust closures capturing state
(`install_primitives` in `dom.rs:401` takes **40 `Arc<Mutex<…>>` parameters**);
stateful closures have no stable address and cannot be snapshotted.
**Consequence:** the Rev 1 plan «snapshot after binding registration» is dead.
Startup snapshot, if ever attempted, may contain only the pure-JS
`WEB_API_SHIM` evaluation with natives registered *after* isolate creation —
treat as an optional optimization (S11), not a pillar.

### F3. Measured scale (Rev 1 said «~35 modules, ~3000 lines» — it is ~2× more)

| Metric | Measured (2026-07-07) |
|---|---|
| `crates/js` total | 80 216 lines, ~120 binding modules |
| `install_*` calls in `lib.rs::install_dom` | **97** |
| `reg!(` native registrations in `dom.rs` | **184** |
| `Function::new` registrations in other modules | **192** |
| `rquickjs` mentions in `crates/js/src` | 578 |
| Hot/complex modules needing hand-port | `canvas2d` (85 mentions), `webgpu`, `webgl_canvas`, `wasm` (uses `Persistent<Function>` GC roots, `wasm/mod.rs:53`), `worker` (own Runtime per thread, `worker.rs:293`) |

Realistic diff: 6–10k lines across `lumen-js` + a thin `lumen-shell` adapter.

### F4. The port is NOT a mechanical sed — unless the compat layer exists first

All ~380 registrations rely on rquickjs *typed closures*:

```rust
reg!("_lumen_console_log", move |msg: String| { … });   // dom.rs:452
```

Argument conversion is automatic via rquickjs `FromJs`. A raw V8 callback is
untyped `(scope, FunctionCallbackArguments, ReturnValue)` — every registration
would need hand-written argument unpacking. **Consequence:** slice S2 builds a
compat layer first (own `IntoJsFn` trait for arities 0..7 + a `reg!`-twin macro
over V8 mimicking rquickjs ergonomics). After S2 the module port IS mechanical
and parallelizable across subagents. Without S2, the port drowns.

### F5. The `v8` crate downloads a prebuilt static lib in `build.rs`

Prebuilt `.lib` for MSVC ships via GitHub releases, downloaded at build time.
Interactions to verify on THIS machine before any port work (that is slice S0):
network-at-build, sccache/`RUSTC_WRAPPER` interplay, link success on the
MSVC toolchain, binary size delta (expect +30–50 MB static). Pin the version.

---

## Architecture (unchanged from Rev 1 — still correct)

### The seam: `JsRuntime` trait — `crates/core/src/ext.rs:847`

Required methods: `eval`, `set_global`, `get_global`, `call_function`,
`engine_name`, `resume`. Defaulted: `eval_module`, `register_module_source`,
`pause`, `unpause`, `suspend`. `JsValue` (`ext.rs:936`) is a JSON-compatible
enum — no engine value types cross the boundary (intentional). `SuspendedHeap`
(`ext.rs:913`) — V8 bytes go in `compressed`, unchanged.

### The seam: `PersistentJs` trait — `crates/shell/src/main.rs:1729`

~50 methods, two patterns, both engine-agnostic:
- JS-call methods via `eval_js()`: `tick_timers` → `_lumen_tick_timers()`,
  `run_animation_frame` → `_lumen_raf_tick(ts)`, `notify_dom_content_loaded`,
  `pump_websockets`, `pump_sse`, …
- `Arc<Mutex<…>>` drain methods readable off-thread: `take_navigate_request`,
  `take_console_messages`, `take_dom_dirty`, `take_timer_wakeup`,
  `flush_canvas_updates`, …

V8 adapter `V8PersistentJs` mirrors `QuickPersistentJs` (`main.rs:2076`) —
mechanical.

### Threading model (ADR-014 pattern carries over)

One `Isolate` per thread (V8 is `!Send`, same as QuickJS). Dedicated `lumen-v8`
thread owns `v8::OwnedIsolate` + `v8::Global<v8::Context>`; handle holds
`SyncSender<V8Command>`; `run()` blocks until the job completes on the JS
thread. `HandleScope` lives entirely inside the job closure — the blocking
dispatch pattern is compatible. Mirror `js_thread_main` (`lib.rs:372`) and the
`run()` dispatcher (`lib.rs:478`), including its documented unsafe
lifetime-erasure trick.

### What ports for free

`WEB_API_SHIM` (`dom.rs:5915+`, 8000+ lines of JS building `document`/`window`/
`console` over the natives) is pure engine-agnostic JS — evaluates unchanged in
V8. The decorators transformer (`decorators::maybe_transform_decorators`) is
pure Rust source rewriting — call before any engine. The QuickJS
`__lum_args__` workaround (`lib.rs:2126`) is dropped — V8 calls functions with
args natively.

---

## Slice plan (S0–S12)

Rules: one slice = one session = one branch `p1-v8-s<N>` = one worktree =
green `cargo clippy -p lumen-js --all-targets -- -D warnings` +
`cargo test -p lumen-js` = merge `--no-ff` to main. The `v8-backend` feature
stays off-by-default until S12, so main never breaks. Update THIS file's
checklist after every merge.

| # | Slice | Content | DoD | Risk |
|---|---|---|---|---|
| ✅ S0 | **Build spike** | `v8` as optional dep under `[features] v8-backend` in `crates/js/Cargo.toml`; one smoke test: init platform, create isolate, eval `1+1`. **No porting until this is green.** Record crate version + binary size delta here. | `cargo test -p lumen-js --features v8-backend` green on MSVC; sccache interplay documented | **High** — this is the go/no-go gate |
| ✅ S1 | **Runtime skeleton** | `crates/js/src/v8_runtime.rs`: `V8JsRuntime` (handle), `V8Inner` (thread-owned isolate+context), `V8Command`, `v8_thread_main`, `run()` dispatcher; `impl JsRuntime`: `eval`, `set_global`, `get_global`, `call_function`, `engine_name`→`"v8"`; `from_v8`/`to_v8` ⇄ `JsValue` converters. ЗАКРЫТ 2026-07-13 (p1-v8-s1): 17 тестов зелёные, clippy чистый. | mirror test suite `tests/v8_eval.rs` green ✅ | Medium (`HandleScope` lifetimes in the dispatcher) |
| ✅ S2 | **Compat layer** | `into_v8_fnN` free fns (arities 0..7) + `V8NativeFn` object-safe trait + `OwnedNativeFn` RAII + trampoline + `register_v8_native`; `reg!` macro в `v8_runtime.rs`; 3 console natives как proof; 4 новых теста. ЗАКРЫТ 2026-07-13 (p1-v8-s2). | typed Rust closure registers and is callable from JS with auto-converted args | Medium — **this slice de-risks everything after it** |
| ✅ S3 | **Core DOM** | Port `install_primitives` (184 `reg!` natives, `dom.rs:401`) via compat layer; eval `WEB_API_SHIM` unchanged; `V8JsRuntime::install_dom` with same signature as QuickJS version. ЗАКРЫТ 2026-07-13 (p1-v8-s3): 183/184 natives ported (see subsystems/js.md), `_lumen_drain_microtasks` a no-op stub (V8 auto-runs its microtask queue), 27 тестов зелёные. | `document.querySelector`, `_lumen_tick_timers`, `window.location.href` work; `samples/page.html` renders under `--features v8-backend` e2e | Medium |
| ✅ S4 | **Shell adapter** | `v8 = ["dep:lumen-js", "lumen-js/v8-backend"]` in shell `Cargo.toml`; `#[cfg(feature = "v8")] struct V8PersistentJs` mirroring `QuickPersistentJs` (~50 methods, mechanical); construction branch at `main.rs:4934`. ЗАКРЫТ 2026-07-13 (p1-v8-s4): `V8PersistentJs` implements all `PersistentJs` methods (state-backed ones delegate to `V8JsRuntime`; subsystems not yet ported to V8 — workers, canvas2d, view transitions, notifications — use empty/no-op stubs per slice table above). Both construction sites (initial load + bfcache thaw) mirrored; `quickjs` takes priority at compile time when both features are enabled (see `crates/shell/Cargo.toml` comment). | `cargo run -p lumen-shell --no-default-features --features backend-femtovg,v8 -- samples/page.html` interactive | Low |
| ✅ S5–S7 | **Simple-module batches** | ~90 modules, batches of ~30, via compat layer. Same transformation each — parallel subagents appropriate here. Keep a ported/pending checklist in this file | `cargo test -p lumen-js --features v8-backend` after each batch | Low |

**S5-S7 ported/pending checklist** (2026-07-13, p1-v8-s57, ЗАКРЫТ батчем 3): of the 90
`install_*` call sites in `lib.rs::install_dom` (QuickJS), 85 take a single `ctx: &Ctx`
argument with no extra state — of those, **all 79 + 5 (batch 3's video_bindings +
audio_element) = 84 are ported** (batches 1-3): each got a `#[cfg(feature =
"v8-backend")] pub(crate) fn install_X_v8(rt: &V8JsRuntime) -> JsResult<()>` sibling next
to the rquickjs original (same JS shim(s), `rt.eval(...)` instead of `ctx.eval::<(),
_>(...)`), wired via a `install_v8!` macro at the end of `V8JsRuntime::install_dom` —
**best-effort** (logs + continues on error), mirroring `lib.rs`'s `if let Err(e) = X {
eprintln!(...) }` orchestration, so one broken/partial module can't abort DOM bootstrap
for the rest. Side-fix: added a `DOMException` polyfill (`DOM_EXCEPTION_POLYFILL` in
`v8_runtime.rs`, evaluated before `WEB_API_SHIM`) — quickjs-ng bundles this as a built-in
(part of `Context::full()`'s extras), V8 has zero web-platform globals; without it,
`class X extends DOMException` (used by `web_codecs` and dozens of `WEB_API_SHIM` call
sites already ported in S3) throws `ReferenceError` the instant it's evaluated. Batch 2
also added `V8JsRuntime::register_native` (registers an already-wrapped
`into_v8_fnN` native as a global, for standalone modules that need `Function::new`-style
natives without duplicating `install_dom`'s inline scope/store setup). Batch 3 (2026-07-13,
p1-v8-s57-batch3) ported the 3 modules that carry extra state beyond `&ctx`: added
`broadcast_channels`/`pending_notifications` fields to `V8JsRuntime` (mirroring
`QuickJsRuntime`'s fields of the same name) plus `broadcast_registry()`/
`notification_queue()` accessors and `pump_broadcast_channels()`/
`take_notification_requests()` public methods mirroring the QuickJS API; `geolocation`
needed no new field (`fake_coords` is only baked into an injected JS global, same as the
QuickJS original). All tests green (`cargo test -p lumen-js --features v8-backend`); full
workspace clippy + scoped-test green.

Ported (batch 1, 68): async_context, attribution_reporting, badging, battery_bindings,
bluetooth, close_watcher, compute_pressure, content_index, credentials, csp,
css_properties_values_api, decorators, device_sensors, digital_credentials,
document_pip, dom_parser, element_internals, es2026_proposals, eye_dropper,
form_validation, gamepad, generic_sensor, highlight_api, iframe_element, inert,
intl_bindings, launch_handler, local_font_access, long_animation_frames,
media_capabilities, media_devices, media_session, navigation_api, navigator_bindings,
paint_worklet, permissions_policy, presentation_api, reporting_api, sanitizer,
scheduler, screen_orientation, scroll_snap_events, scroll_timeline, serial,
shape_detection, shared_storage, soft_navigation, speculation_rules, storage_manager,
surface_api, svg, tc39_proposals, temporal_api, topics_api, typed_om_api,
ua_client_hints, url_pattern, video_pip, virtual_keyboard, webhid, web_locks, web_midi,
webrtc_stub, webusb, webxr, window_management, xhr, web_codecs.

Ported (batch 2, 11): download_bindings, filesystem_access, idle_detection,
network_log_bindings, speech, web_audio, file_input, pip_bindings, wake_lock,
media_capture, screen_capture — each via `into_v8_fnN` + `register_native`, JS shims
unchanged.

Ported (batch 3, 5): video_bindings, audio_element (heavier native counts, 13-16
`Function::new` each, still simpler than S8's canvas2d); geolocation, broadcast_channel,
notifications_bindings (extra state params beyond `&ctx` — see `V8JsRuntime` plumbing
above). S5-S7 is now fully closed (84/84 simple modules ported).

**Reserved for later hand-port slices, not S5-S7**: canvas2d, offscreen_canvas,
webgl_canvas (→ S8); webassembly, webgpu (→ S9); worker, shared_worker, sw_worker (→
S10) — these take extra params too but are covered by their own slices below.
| ✅ S8 | **canvas2d + webgl_canvas** | Hand-port (hot path, 85 rquickjs mentions; pixel queues via `flush_canvas_updates`) | canvas graphic tests pass under v8 feature | Medium |
| ✅ S9 | **wasm + webgpu** | `Persistent<Function>` GC roots → `v8::Global<Function>`; keep the `wasm::clear_registry()` teardown pattern (`lib.rs:401`) | wasm + webgpu test suites green (note: webgpu test flaky under load — rerun before blaming the port) | Medium |
| ✅ S10 | **worker + shared_worker + sw_worker** | Per-thread `Runtime`+`Context` (`worker.rs:293`) → per-thread `OwnedIsolate`; same channel protocol | worker tests green | Medium |
| ✅ S11 | **suspend/resume (partial 10C.2)** | `suspend()`: enumerate own globals set by page scripts, serialize *data* via `v8::ValueSerializer` into `SuspendedHeap.compressed` (zstd, ≤5 MB); `resume()`: `ValueDeserializer` restore. **Closures are NOT serializable (F1) — the re-run-scripts fallback at `main.rs:14599` stays.** Optional: pure-JS-shim startup snapshot (F2), only if cheap. ЗАКРЫТ 2026-07-14 (branch p1-v8-s11). | `window.__test = 42` survives suspend→resume ✅ | Low |
| ☐ S12 | **Cutover + cleanup** | shell default `quickjs` → `v8`; remove `rquickjs` dep + `quickjs-backend` code; kill `__lum_args__` workaround; ADR-004 → Superseded, write `ADR-015-v8-migration.md`; `CAPABILITIES.md` JS row → V8; `navigator.userAgent` → `'Lumen/1.0.0'` (`dom.rs:5916`, version-bump commit only); React 18 CRA demo loads without JS errors (via `take_console_messages`) | `rquickjs` gone from `Cargo.lock`; full graphic-test run green | Medium (the flag-flip exposes everything at once) |

### Session protocol for a fresh session picking this up

1. Read this file top to bottom; the slice checklist above is the source of truth.
2. `git branch --list 'p1-v8-*'` — an existing branch means that slice is
   reserved/in progress; continue it in its worktree or pick the next unchecked slice.
3. Worktree: `.claude/worktrees/v8-s<N>/`, branch `p1-v8-s<N>`.
4. Build with dev-release profile for anything heavy; never `--release`.
5. After merge: tick the slice checkbox here, note surprises in the
   «Findings log» below, update `subsystems/js.md` if an invariant changed.

## Findings log (append per slice)

### S0 — Build spike (2026-07-13, branch p1-v8-s0)

**v8 crate version:** 150.1.0 (rusty_v8). `cargo check -p lumen-js --features v8-backend` ✅.

**Two smoke tests pass:** `v8_eval_one_plus_one` (eval `1+1` → 2.0) and `v8_string_round_trip`.

**Windows MSVC gotchas found and solved:**

1. **Symlink privilege (ERROR_PRIVILEGE_NOT_HELD, code 1314).** v8's `build.rs` creates a
   `gn_root` symlink when the cargo target dir and cargo registry are on different drives
   (project on `D:\`, registry on `C:\`). Symlinks on Windows require
   `SeCreateSymbolicLinkPrivilege` (Developer Mode or admin).
   **Workaround:** set `CARGO_TARGET_DIR` to any path on `C:\` before building/testing,
   e.g. `CARGO_TARGET_DIR=C:\tmp\lumen-v8-target`. The v8 pre-built `.lib` (~150 MB) is
   then downloaded to that dir on first build.

2. **CRT conflict: rust-lld + rquickjs + v8 (LNK2019 `__declspec(dllimport) _wassert`).**
   `rquickjs_sys` is compiled as C with DLL-import CRT annotations (`/MD`); `v8` additionally
   links `msvcprt.lib`. `rust-lld` (our default linker) cannot resolve DLL imports for ucrt
   symbols in this mixed configuration (no `ucrtbase.lib` import library present).
   **Workaround:** run tests with the MSVC linker via a wrapper:
   ```
   RUSTFLAGS="-Clinker=C:\tmp\msvc-link.bat" CARGO_TARGET_DIR=C:\tmp\lumen-v8-target \
     cargo test -p lumen-js --features v8-backend
   ```
   where `msvc-link.bat` calls the MSVC `link.exe` from BuildTools.
   **Permanent fix (planned S12):** rquickjs is removed; only v8 remains → no CRT conflict.
   A simpler interim fix if needed before S12: make rquickjs optional under `quickjs-backend`
   feature so the v8 test binary never links it.

**sccache interplay:** sccache caches v8 build output normally. The 150 MB `rusty_v8.lib`
is not rebuilt unless the v8 crate version changes. First-build download takes ~30 s on
a fast connection.

**Go/No-Go verdict: GO.** v8 150.1.0 builds and runs on Windows MSVC x86_64. Porting can begin.

### S8 — canvas2d + webgl_canvas (2026-07-14, branch p1-v8-s8)

Both modules use pattern (b) from S5-S7 (module-level `thread_local!` state —
`CANVASES`/`DIRTY`/`GRADIENTS`/`PATTERNS`/`PATHS`/`TRANSFERRED` in
`canvas2d.rs`, `CONTEXTS`/`NEXT_ID` in `webgl_canvas.rs`), so no new
`V8JsRuntime` fields were needed — same shape as `video_bindings_v8`/
`audio_element_v8`. Arities topped out at 7 (`_lumen_canvas2d_arc`,
`_lumen_webgl_uniform4f` needed only 6); every argument/return type
(`u32`/`i32`/`f64`/`String`/`bool`/`Vec<f64>`/`Vec<u8>`) was already covered
by `v8_compat.rs`'s `FromJsValue`/`IntoJsReturn` impls — **no GC-root
(`v8::Global<Function>`) mechanism was needed for S8**, confirming the
migration brief's F3 note that only S9 (wasm) actually requires one.

`install_canvas2d_bindings_v8` (77 natives) needs no shim `eval` — the
`getContext('2d')` JS shim already lives in `dom.rs::WEB_API_SHIM`, shared by
both engines. `install_webgl_canvas_v8` (34 natives) does need
`rt.eval(WEBGL_SHIM)` since that shim is private to `webgl_canvas.rs`, not
part of `WEB_API_SHIM` — mirrors `geolocation_v8`'s `rt.eval(&format!(...))`
pattern for seeding `_LUMEN_GPU_VENDOR`/`_LUMEN_GPU_RENDERER` globals ahead of
the shim. Both wired into `V8JsRuntime::install_dom` right before the S5-S7
`install_v8!` macro list (webgl before canvas2d, mirroring `lib.rs`'s
ordering). Added `V8JsRuntime::flush_canvas_updates()` (dispatches
`canvas2d::flush_dirty()` on the JS thread via `self.run`, since the dirty
registry is thread-local to that thread) and wired
`V8PersistentJs::flush_canvas_updates` in `shell/main.rs` to it, replacing the
no-op stub from S4.

**`offscreen_canvas.rs` intentionally NOT ported in this slice** — the
ROADMAP task title and DoD only name canvas2d + webgl_canvas, and
`graphic_tests/57-canvas-2d.html` doesn't exercise `transferControlToOffscreen`.
`_lumen_canvas_transfer_control_to_offscreen` still returns a valid
`OffscreenCanvas` id under v8, but `.getContext('2d')` on that offscreen
object won't work until `offscreen_canvas.rs` gets its own V8 port (left as a
known gap, not currently claimed by any slice — `offscreen_canvas` is not
covered by S9/S10 either).

**Verification**: `cargo test -p lumen-js --features v8-backend` — 2399 lib
unit tests (includes the existing rquickjs `canvas2d`/`webgl_canvas` tests,
unaffected) + 68 integration tests, all green; `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` clean. No automated
graphic-test runner exists for the v8 feature (`run.py` isn't parametrized by
JS engine — noted as a gap in the S8 risk assessment); verified manually
instead: `cargo run -p lumen-shell --no-default-features --features
backend-femtovg,v8 -- --dump-display-list graphic_tests/57-canvas-2d.html`
produced a display list **byte-for-byte identical** to the default (QuickJS)
build's dump — same 6 `DrawImage src="canvas:N"` entries at identical
coordinates, confirming `getContext('2d')`, `fillRect`, `arc`, path
fill/stroke, and `drawImage` all execute correctly through the V8 bindings.

### S9 — wasm + webgpu (2026-07-14, branch p1-v8-s9)

`webgpu.rs` confirmed S8's prediction: zero `Persistent` usage, so
`install_webgpu_bindings_v8` ports unchanged through the ergonomic
`into_v8_fnN` compat layer (every native is `f64`/`u32`/`String`/`bool`/
`Vec<u8>`); without the `webgpu` Cargo feature it's just `rt.eval(WEBGPU_SHIM)`
— zero natives, mirroring `webgl_canvas`'s S8 shim-eval pattern.

`webassembly.rs` is the actual GC-root slice. The generic `V8NativeFn`/
`JsValue` compat layer cannot carry a JS `Function` (arrays/functions collapse
to `JsValue::Null` in `v8_to_jsvalue`), so a new parallel mechanism was added:
`v8_compat::V8NativeFnScoped` — a second, object-safe native trait giving raw
`(scope, FunctionCallbackArguments, ReturnValue)` access instead of the
`JsValue` abstraction, with its own trampoline (`native_fn_trampoline_scoped`)
and store (`V8Inner::native_fn_store_scoped`, twin of `native_fn_store`).
`V8JsRuntime::register_native_scoped` mirrors `register_native`. Used for the
5 wasm natives that need it: `__lumen_wasm_compile` (throws `CompileError` on
decode failure — `IntoJsReturn` has no error variant), `__lumen_wasm_instantiate`
(captures the JS import-function array as `Vec<v8::Global<v8::Function>>`),
`__lumen_wasm_call` (may re-enter a host import mid-call, needs a live scope
to invoke the stored `Global`), and `__lumen_wasm_global_get`/`_set` (need
exact `BigInt` for `i64`, which `f64`-only `FromJsValue`/`IntoJsReturn` would
truncate past 2^53).

`wasm::v8_bridge` (new submodule of `wasm/mod.rs`, `#[cfg(feature =
"v8-backend")]`) is a **separate** thread-local instance registry from the
QuickJS one — module ids are shared via the existing (backend-agnostic)
`with_module`/`REGISTRY.modules`, but V8 instances get their own
`next_instance`/`instances` map, so the two backends never collide on ids even
if both features are compiled into the same binary. `JsHost` there implements
`HostImports` by resurrecting a `v8::Local<Function>` from the stored `Global`
via `v8::Local::new(scope, &global)` and calling it with `Function::call` —
confirmed this actually resurrects and invokes correctly (not just compiles)
via a dedicated test, not just a display-list diff (no display-list equivalent
exists for wasm).

`crate::wasm::v8_bridge::clear_registry()` is wired into `v8_thread_main`'s
teardown (right before `inner` drops), mirroring `lib.rs:447`'s
`wasm::clear_registry()` call for QuickJS. Unlike QuickJS, V8's `Global::drop`
safely no-ops on an already-disposed isolate (checks `isolate_liveness`) — so
this isn't a correctness requirement to avoid an abort like the QuickJS
`gc_obj_list` assertion (BUG-222), but it is still the correct, leak-free
teardown order (releases the persistent handle while the isolate can still
process the reset).

**Verification**: `cargo test -p lumen-js --features v8-backend` — 2402 lib
unit tests (2399 existing + 3 new `tests_v8` modules) + 68 integration tests,
all green; same with `--features v8-backend,webgpu` added. `cargo clippy -p
lumen-js --all-targets --features v8-backend[,webgpu] -- -D warnings` clean on
both combinations, and on the default (QuickJS-only) build. The 2 new
`webassembly::tests_v8` tests are the load-bearing proof for this slice: one
exported-call round-trip, and one host-import round-trip reusing the same WASM
bytes as `tests::webassembly_i64_import_arg_and_result_use_bigint` — the
`i64`/`BigInt` host-import test specifically proves the `v8::Global<Function>`
GC-root mechanism resurrects and invokes correctly at runtime, not merely
compiles. `webgpu::tests_v8` adds one shim-smoke test (`navigator.gpu` exists).
`offscreen_canvas`/`worker`/`shared_worker`/`sw_worker` remain unported, per
the S8 note and the S10 slice below.

### S10 — worker + shared_worker + sw_worker (2026-07-14, branch p1-v8-s10)

All three modules spawn a dedicated OS thread per instance holding an
engine-owned JS context — QuickJS's version hand-rolls a bare
`Runtime::new()`/`Context::full()` per thread. The V8 port does **not**
hand-roll a second bare-isolate construct: each thread just constructs a
full `V8JsRuntime::new()` (which already spawns exactly the "one Isolate per
thread" pattern from the S1 threading model) and calls its public `eval`/
`set_global`/`register_native` methods directly — reusing 100% of the
tested S1-S9 machinery instead of duplicating scope/dispatch plumbing. The
outer `std::thread` this creates (one for the worker's own message loop,
plus `V8JsRuntime`'s own internal JS thread) is one thread more per worker
than the QuickJS version, an accepted cost for the risk reduction.

All natives across the three modules are plain `String`/`u32`/`bool`/
`Option<String>` — no `Function` arguments, no `i64`/`BigInt` — **except**
`worker.rs`'s `atob`/`btoa`, which must throw a JS `TypeError` on invalid
input (WHATWG Infra §forgiving-base64); the generic `into_v8_fnN` compat
layer has no error/throw variant, so these two go through
`crate::v8_compat::V8NativeFnScoped` (the S9 scoped-native mechanism),
mirroring `wasm_compile_native_v8`'s reasoning. `shared_worker.rs`'s and
`sw_worker.rs`'s `atob`/`btoa` (or cache-native equivalents) don't throw and
use the plain path.

`WorkerHandle`/`WorkerRegistry`/`WorkerMessageQueue`/`WorkerBlobStore`/
`WorkerInMsg` (worker.rs), `SharedWorkerThread`/`SwInMsg`/
`SharedWorkerOutbox` (shared_worker.rs), and `SwWorkerHandle`/
`SwFetchRequest` (sw_worker.rs, from `lumen_core::ext`) are all
engine-agnostic already (plain channel/JSON plumbing) and reused unchanged
by both backends. `WORKER_SHIM`/`SHARED_WORKER_SHIM` (main-thread classes)
and the worker-thread global-scope shims are pure JS; the QuickJS originals
were refactored to extract these into `worker_global_shim(id)`/
`sw_globals_shim(scope_str)` free functions so both engines eval identical
JS (mechanical extraction, no behavior change — verified by the full
existing QuickJS suite staying green).

`shared_worker.rs` gets a **separate** `HUB_V8` registry (own
identity-keyed thread map), mirroring S9's `wasm::v8_bridge` rationale: only
one engine actually runs per browser process, but a dual-compiled binary
must never let a V8 page's `SharedWorker` connect to an already-running
QuickJS-backed thread (or vice versa) just because they share an identity
key.

`sw_worker.rs` needed **no** `flush_jobs`/`execute_pending_job` equivalent
— V8's microtask queue auto-runs (`MicrotasksPolicy::kAuto`, per the S3
slice notes), so a `Promise` chain started by `_sw_fire_fetch`/
`_sw_fire_event` (e.g. `respondWith(caches.match(...))`) fully drains by the
time `V8JsRuntime::eval` returns. Verified empirically:
`tests_v8::v8_sw_responds_from_cache` reads `_sw_resp_body__` immediately
after firing the fetch event, no manual pump, and passes — the QuickJS
version's `flush_jobs(&rt)` step is not needed under V8.

`offscreen_canvas.rs` is **not** installed inside a V8-backed dedicated
worker thread (same known gap as S8: `offscreen_canvas.rs` has no V8 port).
A worker script referencing `OffscreenCanvas` sees `undefined`;
`_deserializeTransfers`'s `typeof
_lumen_offscreen_canvas_from_image_data !== 'undefined'` guard already
degrades gracefully (passes the raw, non-deserialized data through) since
that check was already in the shared/reused JS shim.

`V8PersistentJs::pump_workers`/`pump_shared_workers` (previously no-op
stubs in `crates/shell/src/main.rs`, explicitly waiting on S10) now delegate
to `V8JsRuntime::pump_workers`/`pump_shared_workers` — new methods mirroring
`QuickJsRuntime`'s of the same name. The pre-existing
`_lumen_sw_activate_script` native (wired in S3's core-DOM block, before
this slice existed) previously called the QuickJS-only `spawn_sw_worker`
regardless of which engine was active — a cross-engine reuse quirk that
predates S10. It now calls `spawn_sw_worker_v8`, so a V8-backend page's
Service Worker actually runs on V8 end-to-end.

**Verification**: `cargo test -p lumen-js --features v8-backend` — 2413 lib
tests (2402 + 11 new: 4 `worker::tests_v8`, 3 `shared_worker::tests_v8`, 3
`sw_worker::tests_v8`), all green; default (QuickJS) suite stays green
(2372 tests, unaffected by the `b64_encode`/`worker_global_shim`/
`sw_globals_shim` extraction refactors). `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` and the default
(QuickJS-only) build both clean.

### S11 — suspend/resume, partial 10C.2 (2026-07-14, branch p1-v8-s11)

Implemented directly against the raw `v8::ValueSerializer`/`ValueDeserializer`
FFI wrapper (`v8` crate 150.1.0) — no higher-level structured-clone helper
exists in this crate version. Both need the `ValueSerializerHelper`/
`ValueDeserializerHelper` extension traits imported (`write_header`/
`write_value`/`read_header`/`read_value` are trait methods, not inherent on
`ValueSerializer`/`ValueDeserializer` — not obvious from the type signatures
alone, `rustc` suggests the fix directly).

**Baseline-diff approach** (not a full heap walk — F2 already ruled that out
for snapshots, and a full walk would also re-capture every DOM native as
"page data"): `V8Inner` gained a `baseline_globals: HashSet<String>` snapshot
of the global object's own-enumerable-non-symbol keys, taken once in
`v8_thread_main` right after `Context::new` — before `install_dom` or any
script runs. `suspend()` re-enumerates the live global object and only
considers keys **absent** from that baseline: this is what keeps
`Object`/`Array`/etc. (and, if `install_dom` ran, the ~380 DOM natives) out of
the capture without an allow-list — only genuinely new bindings are
candidates.

**Per-value probe before commit**: each candidate value is
structured-clone-tested in isolation (`ValueSerializer::write_value` inside a
scratch `TryCatch`) before being copied into the wrapper object that gets the
real, final serialize pass. This is deliberately two-pass rather than
one-shot-and-hope: F1 says closures throw `DataCloneError`, and a single
throwing value partway through a combined-object serialize would have voided
every sibling key already written into the same `ValueSerializer` byte
stream. Testing each value alone first means a page global that happens to be
a function (or holds one internally, e.g. `{ onClick: function(){} }`) is
dropped without taking down `__test`/`__state`/other plain-data siblings —
verified directly by `suspend_drops_closures_but_keeps_sibling_data`.
`LumenValueSerializerImpl`/`LumenValueDeserializerImpl` both use only the
required/default trait methods (`throw_data_clone_error` schedules a JS
`Error` via `Exception::error`, same pattern as the existing
`Exception::type_error` use in `v8_compat.rs`); no `is_host_object`/
`write_host_object` override is needed since nothing here ever serializes a
host object.

**Everything stays inline inside the existing `with_tc!` macro body** (no
extracted `fn foo(tc: &mut ...)` helper) — the concrete pinned-scope type
`with_tc!` produces (`PinnedRef<TryCatch<'scope,'obj,P>>`, three lifetime/type
parameters resolved via the crate's internal `NewTryCatch` associated-type
machinery) has no clean spelling from outside the macro invocation; every
other `JsRuntime` method in this file (`eval`/`set_global`/`get_global`/
`call_function`) follows the same inline-only convention already, this just
extends it.

`compress_heap(&[])` is **not** the empty byte vector — it always frames a
4-byte `HEAP_MAGIC` + zlib-stream header, so `SuspendedHeap::is_empty()` is
never a valid check for "suspend captured nothing"; assert on `resume()`
behavior instead (`typeof __anything === 'undefined'`), not on
`heap.compressed.len()`.

**Verification**: `cargo test -p lumen-js --features v8-backend` — 2419 lib
tests (2413 + 6 new `v8_runtime::tests::suspend_*`/`resume_*`), all green,
covering number/string/array/plain-object round-trip, closure-drop-without-
poisoning-siblings, and the empty-snapshot/empty-capture paths; 68 integration
tests unaffected. `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` and the default (QuickJS-only) build both clean.
No shell-level (`main.rs:14599` `restore_js_context`) integration test was
added — that path is exercised end-to-end by the pre-existing QuickJS
hibernation tests and is out of scope for this slice (DoD is the
`JsRuntime::suspend`/`resume` trait pair, not full tab-lifecycle wiring).

---

## Risks (Rev 2)

| Risk | Likelihood | Mitigation |
|---|---|---|
| `v8` crate fails to link / build.rs download blocked on this machine | High | S0 exists solely to burn this down before any port work |
| Binary size +30–50 MB | Certain | Accept for v1.0; document in README at S12 |
| `HandleScope` lifetimes vs blocking `run()` dispatcher | Medium | Scope lives inside the job closure on the JS thread (ADR-014 pattern); prove in S1 |
| Compat layer can't express some rquickjs signature (e.g. `Ctx` capture, varargs) | Medium | Fallback: raw-callback escape hatch in the macro; hot modules (S8–S10) are hand-ported anyway |
| P3/P4 touch `dom.rs`/modules mid-migration | Medium | Slices merge to main fast; the compat layer confines the diff per module |
| webgpu test flake under load | Known | Re-run `--features webgpu` before blaming the port |
| Perf regression vs QuickJS on tiny pages (V8 startup cost) | Low | Isolate creation ~ms; if visible, lazy-init the JS thread |

## Definition of done (updated from Rev 1)

1. `cargo build -p lumen-shell --no-default-features --features backend-femtovg,v8` succeeds (MSVC).
2. `cargo test -p lumen-js --features v8-backend` green; QuickJS suite stays green until S12.
3. `samples/page.html` renders identically under both engines (pre-S12).
4. React 18 CRA demo loads without JS errors.
5. suspend/resume round-trips **data** globals (`window.__test = 42`); closures explicitly out of scope (F1), fallback retained — 10C.2 partially closed, `ROADMAP.md` updated accordingly.
6. `rquickjs` absent from `Cargo.lock`.
7. `ADR-015-v8-migration.md` committed; ADR-004 marked Superseded.
8. `CAPABILITIES.md` JS engine row → V8; full graphic-test run green.
