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

**S0–S12a done (2026-07-14).** V8 is now `lumen-shell`'s default JS engine (ADR-018). Remaining:
**S12b** — remove `rquickjs` entirely (tracked as its own XL slice, see the table below; true scope is
117/130 files under `crates/js/src`, not a single session). **Re-scoped 2026-07-14 (branch
p1-v8-s12b, scoping-only session, no code deleted)** — measured, not a single session even by S12a's
own estimate: **2336 `#[test]` fns live in files that touch rquickjs**, of which **1047 are in
`dom.rs`'s own `mod tests` (lines 12796–26677, ~13.9k lines) constructing `QuickJsRuntime` directly** —
this is the real DOM behavioral suite, no v8-side equivalent exists yet. The remaining ~1250+ tests are
scattered across the 84+22+... already-v8-ported modules (S5–S10) as **per-module `#[cfg(test)]` blocks
that construct a bare `rquickjs::{Context, Runtime}` and call the module's rquickjs-only `install_*`
directly** (e.g. `canvas2d.rs` 31, `webgpu.rs` 29, `worker.rs` 26, `offscreen_canvas.rs` 22,
`webgl_bindings.rs` 21, `tc39_proposals.rs` 51, `subtle_crypto.rs` 39, `filesystem_access.rs` 33,
`temporal_api.rs` 30) — these test the rquickjs *binding/wiring* layer, separate from
`v8_runtime.rs`'s own 33 tests which only smoke-test the v8 side. **Deleting rquickjs code is gated on
deleting-or-porting every one of these tests** — rquickjs is a hard (non-optional) dependency of
`crates/js`, so `QuickJsRuntime` and every rquickjs-based `install_*`/test fn compiles unconditionally
regardless of feature flags today; there is no cfg fence to hide behind. Recommended path (not yet
started): treat each already-v8-ported module as its own small S12b sub-slice (delete the module's
rquickjs `install_*` fn + its bare-`rquickjs::Context` tests, drop the call from
`QuickJsRuntime::install_dom` in `lib.rs`, verify shared pure-Rust logic — if any — is still reachable
from `v8_runtime.rs`'s existing native wrappers) — dozens of small, mechanically-similar, independently
mergeable slices; save `dom.rs`'s 1047-test monolith for last, itself probably needing a further split
by DOM sub-area (cache ~14648–14959, websocket ~15396–16150, storage ~16526, IndexedDB ~19220,
fetch/XHR ~19617–19791, ...). No code changed in this session — see the S12b finding log entry below
for the full breakdown before picking up implementation.

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
| ✅ S12a | **Cutover: default flip + gate cleanup** | shell default `quickjs` → `v8` (`crates/shell/Cargo.toml`); broaden the ~80 generic (non engine-specific) `#[cfg(feature = "quickjs")]` gates to `any(feature = "quickjs", feature = "v8")`; ADR-004 → Superseded, write `ADR-018-v8-cutover.md`; `CAPABILITIES.md` JS row → V8-default. ЗАКРЫТ 2026-07-14 (branch p1-v8-s12). | full graphic-test run green (141/141) | Medium — done |
| ☐ S12b | **Cutover: rquickjs removal** | Remove `rquickjs` dep + all QuickJS-specific code (`QuickJsRuntime`, `QuickPersistentJs`, ~380 dual `install_*` bindings across 117 files in `crates/js/src`, `dom.rs` original `install_primitives`); kill `__lum_args__` workaround (`lib.rs:2126`); remove the `quickjs` Cargo feature; simplify the broadened `any(quickjs, v8)` gates back to unconditional. `navigator.userAgent` → `'Lumen/1.0.0'` (`dom.rs:5916`, version-bump commit only, unrelated to this slice) | `rquickjs` gone from `Cargo.lock`; `cargo test -p lumen-js`/`lumen-shell` green with only the `v8` feature in the dependency graph | High — measured 2026-07-14 (branch p1-v8-s12b, scoping only): 119 files, **2336 `#[test]` fns gated on this deletion** (1047 in `dom.rs`'s own suite, ~1250 more as per-module bare-`rquickjs::Context` tests in the already-v8-ported S5–S10 modules); every file's deletion requires porting-or-justifying its own tests first — genuinely dozens of sub-slices, not "a multi-session effort" but a multi-*week* one; see Findings log "S12b — scoping only" entry for the proposed breakdown |

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
covered by S9/S10 either). **Update (P1-imagebitmap, 2026-07-17): this gap is
now closed** — `offscreen_canvas::install_offscreen_canvas_bindings_v8` ported
all 19 natives (same `into_v8_fnN`/`rt.register_native` pattern as this
slice's `canvas2d`/`webgl_canvas`), so `OffscreenCanvas.getContext('2d')` and
`createImageBitmap`/`ImageBitmapRenderingContext` now work under v8 too.

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
worker thread — `run_worker_thread_v8` only calls the stripped-down
`install_worker_globals_v8`, not the full `install_dom` install list.
(Update, P1-imagebitmap 2026-07-17: `offscreen_canvas.rs` *does* now have a
V8 port — `install_offscreen_canvas_bindings_v8`, wired into `install_dom`'s
install list for the main page context — this note is specifically about
*worker threads*, which still skip it.) A worker script referencing
`OffscreenCanvas` sees `undefined`; `_deserializeTransfers`'s `typeof
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

### S12a — Cutover: default flip + gate cleanup (2026-07-14, branch p1-v8-s12)

Flipped `crates/shell/Cargo.toml`'s `default` from `["backend-femtovg", "backend-wgpu", "quickjs"]` to
`[..., "v8"]`. The migration brief's original S12 conflated two very different sizes of work under one
line — measuring the actual code before touching it found `rquickjs` (not `optional` in
`crates/js/Cargo.toml`) referenced in **117 of 130** files under `crates/js/src` (`dom.rs` alone is 26.7k
lines, with a full parallel QuickJS+V8 implementation per binding module from S3–S11), and **89**
`#[cfg(feature = "quickjs")]` occurrences in `crates/shell/src/main.rs` alone, of which only **7** paired
with an actual engine-specific `#[cfg(feature = "v8"...)]` alternative. Splitting S12 into S12a (this
slice: default flip + make the shell behave correctly under the new default) and S12b (full `rquickjs`
deletion, tracked separately, XL) was the only way to land a working default-V8 shell in one session
without a half-finished deletion sweep on `main`.

**The ~82 other `quickjs`-gated blocks were never "QuickJS-engine-specific"** — they were "is a JS engine
compiled in at all" gates that happened to only name `quickjs` because it predated `v8` as a feature (e.g.
process-global provider wiring: `lumen_js::set_clipboard_provider`/`set_audio_capture_provider`/
`set_wake_lock_provider`/`set_screen_capture_provider`/`set_video_gif_store`/`set_text_track_store`/
`config::global().install_navigator()`, none of which are gated inside `lumen-js` itself; and dozens of
engine-agnostic shell↔JS drains — layout-rect delivery, history/nav-traversal drains, pointer lock, HTML5
DnD, print requests, focus requests, view-transition/scroll-progress drains — all calling only
`PersistentJs` trait methods or `route_eval_js`/`route_task_js`, which both `QuickPersistentJs` and
`V8PersistentJs` implement). Left this way, flipping the default would have **silently regressed** all of
the above under V8 (clipboard/audio/wake-lock/screen-capture/fingerprint-spoofing would simply not wire up;
video-GIF and text-track stores would go unregistered). Fix: broadened these ~82 gates (73 in `main.rs`,
3 in `config.rs`, 4 in `platform/file_dialog.rs`, 1 in `tab_lifecycle/hibernate.rs`) to
`#[cfg(any(feature = "quickjs", feature = "v8"))]` (and the `not(...)`/`cfg_attr` variants), via a small
Python script that skipped any block whose next few lines mentioned `QuickJsRuntime`/`QuickPersistentJs` by
name (the genuinely engine-specific construction sites — `QuickPersistentJs`'s own struct/impl and the two
`match lumen_js::QuickJsRuntime::new() { ... }` blocks — correctly stayed `quickjs`-only, since their
`#[cfg(all(feature = "v8", not(feature = "quickjs")))]` siblings already exist from S4/S4-era work). Found
2 more files this way that `grep -rl 'feature = "quickjs"' crates/` outside `main.rs` turned up
(`config.rs`, `platform/file_dialog.rs`, `tab_lifecycle/hibernate.rs`) plus 2 real compile errors from
symbols whose own *definitions* (not just call sites) were still `quickjs`-gated
(`platform::file_dialog::entries_to_json_with_tokens`, `config::FingerprintProfile::install_navigator`) —
both fixed the same way.

**`lumen-driver`'s `WinitSession::eval()`** (headless automation one-shot eval) was intentionally **not**
touched — it hard-codes `lumen_js::QuickJsRuntime::new()` directly behind its own separate `quickjs` Cargo
feature (`crates/driver/Cargo.toml`), and `lumen-driver` has no `v8` feature at all. This is a real,
pre-existing gap (automation `eval()` still requires `--features quickjs` on the driver crate regardless of
the shell's new default), left as a known follow-up — out of scope here (automation/testing surface, not
default interactive browsing).

**Verification**: `cargo check -p lumen-shell` (default = v8) and `cargo build -p lumen-shell --profile
dev-release` (default) both green — no rust-lld/CRT linker conflict against the combined
rquickjs+v8 dependency graph (the S0 finding's workaround was never needed for a full `lumen-shell` link,
only for a specific `cargo test` invocation apparently no longer hit by S1+). `cargo clippy -p lumen-shell
--all-targets -- -D warnings` clean. `cargo test -p lumen-shell` (dev-release): 1547 + 1 tests green. Full
graphic-test suite (`LUMEN_PROFILE=dev-release python graphic_tests/run.py --continue-on-fail`) against the
new v8-default binary: 141/141 green (first attempt hit the known TEST-00 gdigrab-capture-race flake — all
141 FAILed with "no crop offset"; a bare re-run passed clean, no code involved).

**React 18 CRA DoD item — partially verified, 2 pre-existing bugs found and filed, not fixed here**:
downloaded the real `react@18`/`react-dom@18` UMD production builds (`unpkg.com`) and built a self-contained
smoke-test page. First attempt (bare `React`/`ReactDOM` identifiers, as real `<script>`-tag usage would be)
hit [BUG-280](../../bugs/BUG-280-FIXED.md) (`window` is a plain object, not the real global object — already
filed, P2 in progress at the time, fixed 2026-07-16) — rewrote the test to reference `window.React`/`window.ReactDOM` explicitly (what an
actually-bundled CRA build's webpack closures would do, since they never rely on the browser's bare-global
machinery) to isolate a *different* bug: `ReactDOM.createRoot(...).render(...)` throws inside react-dom's
event-delegation bootstrap (`Cannot read properties of undefined (reading '_reactListening<rnd>')`).
Root-caused via a DOM-shape diagnostic to `document.nodeType === undefined` (should be `9`),
`element.ownerDocument === document` → `false` (identity mismatch), `document.documentElement.tagName ===
"#document"` (should be `"HTML"`), `element.namespaceURI === undefined` (should be the XHTML namespace) —
filed as [BUG-281](../../bugs/BUG-281-FIXED.md) (fixed 2026-07-14, see the bug file). **Confirmed cross-engine**: rebuilt with
`--no-default-features --features backend-femtovg,backend-wgpu,quickjs` and re-ran both diagnostics — byte-
identical symptoms under QuickJS, proving neither bug is caused by or specific to this cutover; both are
pre-existing `WEB_API_SHIM` gaps. V8 itself ran the React 18 bundle's own code (classes, hooks, closures)
without any JS-*language*-level error — every failure was a DOM-shim property/identity gap, not a JS-engine
gap. DoD item stands only partially met: "V8 executes a real React 18 bundle correctly" — yes; "a React 18
app fully mounts with no errors" — no, blocked on BUG-280/BUG-281, tracked as follow-up work independent of
S12b.

### S12b — scoping only, no code deleted (2026-07-14, branch p1-v8-s12b)

Measured the real deletion surface before touching anything, per S12a's own warning that "the true
scope... size this as its own multi-session effort." It's bigger than that note implied:

- `crates/js` has **no `quickjs` feature at all** — `rquickjs` is a hard, non-optional dependency
  (`crates/js/Cargo.toml:36`). The `quickjs`/`v8` features that got flipped in S12a live one level up,
  in `crates/shell/Cargo.toml`, and only select which runtime struct the *shell* constructs.
  `QuickJsRuntime` and every rquickjs-based binding in `crates/js` compile unconditionally today,
  regardless of any feature flag — `cargo test -p lumen-js` (no flags) already runs the full rquickjs
  suite; `cargo test -p lumen-js --features v8-backend` is a separate, additive run, not a replacement.
- `grep -rl 'rquickjs\|QuickJs\|quickjs'` over `crates/js/src` → **119 files** (close to the S12a note's
  117/130). Two heaviest: `dom.rs` (26677 lines) and `v8_runtime.rs` (4695 lines, the v8-side mirror).
- **2336 `#[test]` fns total** across those 119 files. **1047 live in `dom.rs`'s `mod tests`**
  (lines 12796–26677 — more than half the file), each built on a `runtime_with_*(...) -> QuickJsRuntime`
  helper calling `QuickJsRuntime::new()`. This is the actual DOM-behavior regression suite (events,
  forms, storage, IDB, fetch/XHR, cache, websockets, history, scroll...) — `v8_runtime.rs` has no
  equivalent (only 33 tests total, all smoke-level).
- The other ~1250 tests sit in the individual already-v8-ported module files (S5–S10's 84+ modules),
  each with its own small `#[cfg(test)] mod tests` that builds a bare `rquickjs::{Context, Runtime}`
  (not `QuickJsRuntime`) and calls that module's rquickjs-only `install_*` directly — e.g. `canvas2d.rs`
  31 tests via `rquickjs::{Context, Runtime}` + `install_canvas2d_bindings`, similarly `webgpu.rs` (29),
  `worker.rs` (26), `offscreen_canvas.rs` (22), `tc39_proposals.rs` (51), `subtle_crypto.rs` (39),
  `filesystem_access.rs` (33), `temporal_api.rs` (30). These test the rquickjs binding/wiring layer
  specifically — separate from whatever integration coverage the graphic-test suite gives the v8 side.
- **Net conclusion**: rquickjs cannot be removed file-by-file for free — every deletion is gated on
  deciding the fate of that file's own rquickjs-based tests (port to `V8JsRuntime`/v8 compat types, or
  delete with a documented equivalent-coverage justification per CLAUDE.md's "tests not weakened" bar).
  Nothing was deleted this session — this is a scoping pass only, to avoid the "half-finished deletion
  sweep" S12a explicitly flagged as the failure mode to avoid.
- **Proposed slice breakdown for follow-up sessions** (not started): one small S12b-N slice per
  already-v8-ported module (mechanically similar: delete the rquickjs `install_*` fn + its local
  `rquickjs::Context`-based tests, drop the call site in `lib.rs`'s `QuickJsRuntime::install_dom`,
  confirm any pure-Rust logic the module shares with `v8_runtime.rs`'s native wrappers stays reachable),
  batched by module group (S5–S7's 84 simple modules first, S8–S10's stateful hot modules next); save
  `dom.rs`'s 1047-test monolith for a dedicated final slice (or slices split by DOM sub-area — line
  ranges noted above), since it has no v8-side test equivalent to port against yet and needs the most
  careful triage.

### S12b-1 — `badging.rs` (2026-07-14, branch p1-v8-s12b-1-badging)

First concrete slice of the breakdown above, used as a template for the remaining S5–S7 simple
modules. `badging.rs` had no native state (pure JS-shim `eval`), making it the smallest clean
example: deleted the rquickjs `install_badging_bindings` fn + its `use rquickjs::Ctx` + its
4-test `rquickjs::Context`-based `mod tests`; ported equivalent coverage as 4 new tests against
`V8JsRuntime` + `install_badging_bindings_v8` directly (gated `#[cfg(all(test, feature =
"v8-backend"))]`, since that's the only cfg under which the v8 install fn and `BADGING_SHIM`
const compile); dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `pub mod
badging;` stays (still holds the live v8-side fn). Net effect: badging is no longer installed
under the (already non-default) QuickJS runtime — accepted, matches this slice pattern's intent
per the "Proposed slice breakdown" note above. `cargo test -p lumen-js --features v8-backend
badging` — 4/4 green; default-feature `cargo test -p lumen-js badging` — 0 tests (as expected,
module is v8-only now). Repeat this exact shape for the remaining ~83 S5–S7 modules; modules with
thread-local/shared native state (e.g. `canvas2d.rs`) will need extra care porting that state
setup into the v8-side test harness.

### S12b-2 — `async_context.rs` (2026-07-14, branch p1-v8-s12b-2-async-context)

Second slice, same shape as S12b-1: `async_context.rs` is pure JS-shim `eval` (no native
bindings, no state beyond the shim's own closures), the AsyncContext.Variable/Snapshot Phase 0
polyfill. Deleted the rquickjs `install_async_context` fn + `use rquickjs::Ctx` + its 8-test
`rquickjs::{Context, Runtime}`-based `mod tests`; ported equivalent coverage as 8 new tests
against `V8JsRuntime` + `install_async_context_v8` directly (gated `#[cfg(all(test, feature =
"v8-backend"))]`); dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. Two tests
(`context_propagates_through_promise_then`, `promise_catch_and_finally_propagate_context`) relied
on rquickjs's manual `ctx.execute_pending_job()` microtask pump — dropped, since V8 auto-runs its
microtask queue (per S3's `_lumen_drain_microtasks` no-op note); both pass unchanged otherwise.
`cargo test -p lumen-js --features v8-backend async_context` — 8/8 green; default-feature
`cargo test -p lumen-js async_context` — 0 tests (module is v8-only now, as expected). Repeat for
the remaining ~82 S5–S7 modules.

### S12b-3 — `digital_credentials.rs` (2026-07-14, branch p1-v8-s12b-3-digital-credentials)

Third slice, same shape as S12b-1/S12b-2: `digital_credentials.rs` is pure JS-shim `eval` (no
native bindings), the Digital Credentials API Phase 0 stub (`DigitalCredential` class +
`navigator.credentials.get({digital:...})` rejection hook). Deleted the rquickjs
`install_digital_credentials_api` fn + `use rquickjs::Ctx` + its 4-test
`rquickjs::{Context, Runtime}`-based `mod tests`; ported equivalent coverage as 4 new tests
against `V8JsRuntime` + `install_digital_credentials_api_v8` directly (gated `#[cfg(all(test,
feature = "v8-backend"))]`); dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`.
`cargo test -p lumen-js --features v8-backend digital_credentials` — 4/4 green; default-feature
`cargo test -p lumen-js digital_credentials` — 0 tests (module is v8-only now, as expected).
Repeat for the remaining ~81 S5–S7 modules.

### S12b-4 — `battery_bindings.rs` (2026-07-14, branch p1-v8-s12b-4-battery-bindings)

Fourth slice, same shape as S12b-1/2/3: `battery_bindings.rs` is pure JS-shim `eval` (no native
bindings), the Battery Status API disable stub (ADR-007 Layer 4, 9D.4 — `navigator.getBattery`
replaced with a rejected-Promise shim to prevent fingerprinting). Deleted the rquickjs
`install_battery_bindings` fn + `use rquickjs::Ctx` + its 5-test `rquickjs::{Context,
Runtime}`-based `mod tests`; ported equivalent coverage as 5 new tests against `V8JsRuntime` +
`install_battery_bindings_v8` directly (gated `#[cfg(all(test, feature = "v8-backend"))]`);
dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p lumen-js
--features v8-backend battery` — 5/5 green; default-feature `cargo test -p lumen-js battery` — 0
tests (module is v8-only now, as expected). Repeat for the remaining ~80 S5–S7 modules.

### S12b-5 — `attribution_reporting.rs` (2026-07-14, branch p1-v8-s12b-5-attribution-reporting)

Fifth slice, same shape as S12b-1/2/3/4: `attribution_reporting.rs` is pure JS-shim `eval` (no
native bindings), the Privacy Sandbox Attribution Reporting API Phase 0 stub
(`window.attributionReporting.registerSource`/`registerTrigger` no-ops + `attributionSrc` IDL
attribute on `HTMLAnchorElement`/`HTMLImageElement`/`HTMLScriptElement`). Deleted the rquickjs
`install_attribution_reporting_api` fn + `use rquickjs::Ctx` + its 8-test `rquickjs::{Context,
Runtime}`-based `mod tests`; ported equivalent coverage as 8 new tests against `V8JsRuntime` +
`install_attribution_reporting_api_v8` directly (gated `#[cfg(all(test, feature =
"v8-backend"))]`); dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. Module-level
doc comment converted from `///` to `//!` (an empty line after a `///` block with no trailing item
next to it trips clippy's `empty_line_after_doc_comments` once the `use rquickjs::Ctx` line that
used to sit right after it is gone). `cargo test -p lumen-js --features v8-backend
attribution_reporting` — 8/8 green; default-feature build has zero `attribution_reporting` tests
left (module is v8-only now, as expected). Repeat for the remaining ~79 S5–S7 modules.

### S12b-6 — `speculation_rules.rs` (2026-07-14, branch p1-v8-s12b-6-speculation-rules)

Sixth slice, same shape as S12b-1..5: `speculation_rules.rs` is pure JS-shim `eval` (no native
bindings), the Speculation Rules API Phase 0 stub (`document.prerendering`,
`document.getSpeculationRules()` → `[]`, `onprerenderingchange`,
`_lumen_deliver_speculation_rules` no-op hook). Deleted the rquickjs `install_speculation_rules_api`
fn + `use rquickjs::Ctx` + its 4-test `rquickjs::{Context, Runtime}`-based `mod tests`; ported
equivalent coverage as 4 new tests against `V8JsRuntime` + `install_speculation_rules_api_v8`
directly (gated `#[cfg(all(test, feature = "v8-backend"))]`); dropped the call site in `lib.rs`'s
`QuickJsRuntime::install_dom`. `SPECULATION_RULES_SHIM` const also gated `#[cfg(feature =
"v8-backend")]` since nothing else references it once the rquickjs install fn is gone.
`cargo test -p lumen-js --features v8-backend speculation_rules` — 4/4 green; default-feature
`cargo test -p lumen-js speculation_rules` — 0 tests (module is v8-only now, as expected).

**Selection method note** (useful for the next ~78 slices): before picking a module, grep
`crates/js/src/dom.rs` for the module's name — `dom.rs`'s own 1047-test `mod tests` suite runs
through `runtime_with_dom()` → `QuickJsRuntime`, and a handful of modules (e.g. `document_pip.rs`,
whose shim classes are exercised by 7 tests named `document_pip_*` in `dom.rs`) are indirectly
tested there by name even though the module itself has zero `#[cfg(test)]` code. Deleting such a
module's rquickjs install fn silently breaks those `dom.rs` tests (the shim stops being installed
under `QuickJsRuntime`) without touching the module file at all. `document_pip.rs` was rejected as
the S12b-6 candidate for exactly this reason; `speculation_rules.rs` has zero references in
`dom.rs` and was safe. Modules with nonzero `dom.rs` hits need their `dom.rs` test(s) ported or
justified as part of the same slice, not treated as out of scope.

### S12b-7 — `shape_detection.rs` (2026-07-14, branch p1-v8-s12b-7-shape-detection)

Seventh slice, same shape as S12b-1..6: `shape_detection.rs` is pure JS-shim `eval` (no native
bindings), the Shape Detection API Phase 0 stub (`FaceDetector`/`BarcodeDetector`/`TextDetector`
classes, `detect()` always resolves `[]`, `BarcodeDetector.getSupportedFormats()` → `[]`). Its
local `mod tests` was a variant not seen in S12b-1..6: instead of a bare `rquickjs::{Context,
Runtime}`, it built a full `QuickJsRuntime` via `install_dom(...)` and asserted through that —
still safe to delete since it's a self-contained local suite, not one of `dom.rs`'s tests (zero
`shape_detection` hits in `dom.rs`, confirmed via the S12b-6 selection method). Deleted the
rquickjs `install_shape_detection_bindings` fn + its `use rquickjs::Ctx` + the 7-test
`QuickJsRuntime`-based `mod tests`; ported equivalent coverage as 7 new tests against
`V8JsRuntime` + `install_shape_detection_bindings_v8` directly (gated `#[cfg(all(test, feature =
"v8-backend"))]`, matching the `with_badging`-style single-helper pattern from S12b-1); dropped
the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `SHAPE_DETECTION_SHIM` const also
gated `#[cfg(feature = "v8-backend")]` since nothing else references it once the rquickjs install
fn is gone. `cargo test -p lumen-js --features v8-backend shape_detection` — 7/7 green;
default-feature `cargo test -p lumen-js shape_detection` — 0 tests (module is v8-only now, as
expected); `cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both default and
`v8-backend` features.

### S12b-8 — `compute_pressure.rs` (2026-07-14, branch p1-v8-s12b-8-compute-pressure)

Eighth slice, first one selected via the systematic method instead of ad-hoc scanning: cross-
referenced `lib.rs`'s `install_*_bindings` call list against `v8_runtime.rs`'s `install_*_v8` list
(`comm -12` on the two sorted name sets) to get the 52 modules that are already fully v8-ported
with their rquickjs path still present — as opposed to modules like `webtransport.rs`/`contacts.rs`
that turned out to have **no call site at all** (dead code, never wired into either runtime, out of
scope for this slice type). Picked the smallest candidate with zero `dom.rs` cross-references
(`compute_pressure`: 174 lines, 0 `dom.rs` hits) from that 52, skipping the S8-S10 stateful/hot
group (canvas2d, webgpu, worker, webassembly, webcodecs) per the plan's explicit ordering. Same
shape as S12b-1..5: pure JS-shim `eval`, no native state, local `mod tests` built a bare
`rquickjs::{Context, Runtime}`. Deleted the rquickjs `install_compute_pressure_bindings` fn + its
`use rquickjs::Ctx` + the 5-test rquickjs-`Context`-based `mod tests`; ported equivalent coverage
as 5 new tests against `V8JsRuntime` + `install_compute_pressure_bindings_v8` (gated
`#[cfg(all(test, feature = "v8-backend"))]`, `with_compute_pressure` single-helper pattern);
dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `COMPUTE_PRESSURE_SHIM` const
gated `#[cfg(feature = "v8-backend")]`. Converted the leftover top-of-file `///` doc comment to
`//!` module-level doc (clippy `empty_line_after_doc_comments` fires once the `///` block is no
longer immediately followed by the item it documented). `cargo test -p lumen-js --features
v8-backend compute_pressure` — 5/5 green; default-feature `cargo test -p lumen-js
compute_pressure` — 0 tests; `cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both
default and `v8-backend` features.

### S12b-9 — `pip_bindings.rs` (2026-07-14, branch p1-v8-s12b-9-typed-om)

Ninth slice. First candidate picked (`typed_om_api.rs`, 148 lines, `comm -12` shows it fully
v8-ported) turned out to be a trap of exactly the kind S12b-6's note warned about: the file itself
has zero `dom.rs` references, but its *class names* (`CSSStyleValue`/`CSSUnitValue`/etc.) are
exercised by 12 tests in `dom.rs`'s own suite named `css_typed_om_*` — the S12b-6 selection method
(grep `dom.rs` for the module's file-stem) misses this because the dom.rs test names use the
feature name, not the file name. Deleting the rquickjs install call site broke 10 of those 12 tests
(`cargo test -p lumen-js typed_om` — 10 failed) before this was caught; reverted `typed_om_api.rs`
and `lib.rs` in full and picked a different module rather than untangling `dom.rs`'s monolith
mid-slice (that triage is explicitly deferred to `dom.rs`'s own dedicated final slice per the
breakdown note). Lesson for future slices: cross-check `dom.rs` against the candidate's exported
JS class/API names (`grep -oE '(function|class) [A-Z][A-Za-z0-9_]*'` on the candidate file), not
just the file stem.

Picked `pip_bindings.rs` (175 lines) instead: native `_lumen_pip_enter`/`_lumen_pip_exit` hooks
(process-global `Vec<PipRequest>` queue the shell drains each tick to drive the OS PiP window),
already fully v8-ported (`into_v8_fn1` + `register_native`, S5-S7 batch 2). Zero `dom.rs` hits for
either its native names or `PipRequest`. Deleted the rquickjs `install_pip_bindings` fn + its
`use rquickjs::{function::Opt, Ctx, Function}` + the 4-test `rquickjs::{Context, Runtime}`-based
`mod tests`; ported equivalent coverage as 4 new tests against `V8JsRuntime` +
`install_pip_bindings_v8` (gated `#[cfg(all(test, feature = "v8-backend"))]`, `with_pip_bindings`
single-helper pattern); dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. The
process-global queue (`enqueue`/`take_pip_requests`/`PipRequest`) stays unconditional — the shell
(`crates/shell/src/main.rs:10524`) drains it regardless of which JS engine is active. `cargo test -p
lumen-js --features v8-backend pip_bindings` — 4/4 green; default-feature `cargo test -p lumen-js
--lib` (full suite, not just a name filter, to catch `dom.rs` cross-references this time) — 2328/2328
green; `cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both default and
`v8-backend` features; `cargo check -p lumen-shell` (default) green.

### S12b-10 - `topics_api.rs` (2026-07-14, branch p1-v8-s12b-10-topics-api)

Tenth slice, selected via the systematic method: `comm -12` on `lib.rs`'s `install_*` call list vs
`v8_runtime.rs`'s `install_*_v8` list gives 81 remaining fully-v8-ported candidates (post S12b-9);
sorted by file size, `document_pip.rs` (131 lines) and `typed_om_api.rs` (148 lines) skipped as
known traps (S12b-6/S12b-9 findings), `serial.rs` (151 lines) newly disqualified - its file-stem
hits `dom.rs`'s `event_target_dependent_apis_installed` test (`typeof navigator.serial ===
'object'`), and `scroll_snap_events.rs` (179 lines) also disqualified - its `fire_snap_changing`/
`fire_snap_changed` `QuickJsRuntime` methods are exercised directly by 2 `dom.rs` tests
(`fire_snap_changing_dispatches_event`, `fire_snap_changed_exposes_snap_targets`), a trap shape not
caught by grepping class names alone (checked `lib.rs`'s `pub fn fire_*`/`take_*` methods against
the candidate list, confirmed no other remaining candidate has a corresponding `QuickJsRuntime`
method). `topics_api.rs` (187 lines) is clean: zero `dom.rs` hits for the file stem,
`browsingTopics`, or `DeprecatedTopicsButton`. Pure JS-shim `eval` (no native bindings), the Privacy
Sandbox Topics API Phase 0 stub (`document.browsingTopics()` -> `Promise<[]>`,
`DeprecatedTopicsButton` surrogate class for `<button browsingtopics>`). Deleted the rquickjs
`install_topics_api` fn + its `use rquickjs::Ctx` + the 6-test `rquickjs::{Context, Runtime}`-based
`mod tests`; ported equivalent coverage as 6 new tests against `V8JsRuntime` +
`install_topics_api_v8` directly (gated `#[cfg(all(test, feature = "v8-backend"))]`,
`with_topics_api` single-helper pattern); dropped the call site in `lib.rs`'s
`QuickJsRuntime::install_dom`. Top-of-file `///` doc comment converted to `//!` module-level doc
(same `empty_line_after_doc_comments` clippy trigger as S12b-5/S12b-8, since the `use rquickjs::Ctx`
line that used to sit right after it is gone). `cargo test -p lumen-js --features v8-backend
topics_api` - 6/6 green; default-feature `cargo test -p lumen-js --lib` (full suite) - 2322/2322
green; `cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both default and
`v8-backend` features; `cargo check -p lumen-shell` (default) green.

### S12b-11 — `media_capabilities.rs` (2026-07-14, branch p1-v8-s12b-11-media-capabilities)

Eleventh slice, selected via the same systematic method: `comm -12` on `lib.rs`'s `install_*` call
list vs `v8_runtime.rs`'s `install_*_v8` list gives 80 remaining candidates (post S12b-10); sorted
by file size, `document_pip.rs`/`typed_om_api.rs`/`serial.rs`/`scroll_snap_events.rs` skipped as
already-known traps (S12b-6/S12b-9/S12b-10 findings). `media_capabilities.rs` (185 lines) is clean:
zero `dom.rs` hits for the file stem, `MediaCapabilities`, `decodingInfo`, or `encodingInfo`. Pure
JS-shim `eval` (no native bindings), the Media Capabilities API (W3C §5) Phase 0 stub
(`navigator.mediaCapabilities.decodingInfo`/`encodingInfo` always resolve
`{supported:true, smooth:true, powerEfficient:false}`). Deleted the rquickjs
`install_media_capabilities_bindings` fn + its `use rquickjs::Ctx` + the 5-test
`rquickjs::{Context, Runtime}`-based `mod tests`; ported equivalent coverage as 5 new tests against
`V8JsRuntime` + `install_media_capabilities_bindings_v8` directly (gated
`#[cfg(all(test, feature = "v8-backend"))]`, `with_media_capabilities` single-helper pattern);
dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. Top-of-file `///` doc comment
converted to `//!` module-level doc (same pattern as S12b-5/S12b-8/S12b-10). `cargo test -p
lumen-js --features v8-backend media_capabilities` - 5/5 green; default-feature `cargo test -p
lumen-js --lib` (full suite) - 2317/2317 green; `cargo clippy -p lumen-js --all-targets -- -D
warnings` clean on both default and `v8-backend` features; `cargo check -p lumen-shell` (default)
green.

### S12b-12 — `device_sensors.rs` (2026-07-14, branch p1-v8-s12b-12-device-sensors)

Twelfth slice, selected via the same systematic method: comparing `fn install_*_bindings(` defining
sites across `crates/js/src/*.rs` against `fn install_*_bindings_v8(` sites gives 52 remaining
candidates (post S12b-11); sorted by file size, `serial.rs` (151 lines) and
`scroll_snap_events.rs` (179 lines) skipped as already-known traps (S12b-10 findings).
`device_sensors.rs` (202 lines) is clean: zero `dom.rs`/`lib.rs` hits for `DeviceOrientationEvent`,
`DeviceMotionEvent`, or the file stem outside its own module. Pure JS-shim `eval` (no native
bindings), the Device Orientation Event L2/L3 Phase 0 stub (`DeviceOrientationEvent`/
`DeviceMotionEvent` classes with zeroed defaults, `requestPermission()` always resolves
`'granted'`). Unlike S12b-10/11, this module's own top-of-file doc comment was already `//!` (no
conversion needed). Deleted the rquickjs `install_device_sensors_bindings` fn + its
`use rquickjs::Ctx`; gated the shim `const` behind `#[cfg(feature = "v8-backend")]` since it's now
only referenced from the v8 path. The original test helper used a **full** `QuickJsRuntime::install_dom`
(not a bare context + manual shim like S12b-11's DOMException stub) because the shim's classes
`extend Event`, which only exists after the `dom.rs` DOM-core JS is evaluated — ported the helper
1:1 to `V8JsRuntime::install_dom` (same `Document`/`about:blank` args), confirmed via
`v8_runtime.rs`'s own `runtime_with_dom` test helper that `install_dom` is the right call and that
it already wires `install_device_sensors_bindings_v8` (ported earlier, S5-S7). 6 tests ported
1:1 (gated `#[cfg(all(test, feature = "v8-backend"))]`); dropped the call site in `lib.rs`'s
`QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features v8-backend device_sensors` -
6/6 green; default-feature `cargo test -p lumen-js --lib` (full suite) - 2311/2311 green; `cargo
clippy -p lumen-js --all-targets -- -D warnings` clean on both default and `v8-backend` features;
`cargo check -p lumen-shell` (default) green.

### S12b-13 — `document_pip.rs` (2026-07-14, branch p1-v8-s12b-13-document-pip)

Thirteenth slice, same systematic selection: comparing `install_*_bindings(` defining sites
(taking a `Ctx` param) against `install_*_bindings_v8(` sites across `crates/js/src/*.rs` gives
the remaining candidates; sorted by file size, `typed_om_api.rs` (148 lines, S12b-9's rejected
candidate — breaks 10 `dom.rs::css_typed_om_*` tests) skipped, along with the known traps
`serial.rs`/`scroll_snap_events.rs`. `document_pip.rs` (131 lines) is clean by the file-stem
method, but its 7 own tests live inside `dom.rs`'s big `mod tests` (not in the module's own file,
unlike S12b-1..12) — named `document_pip_*`, using the rquickjs-based `runtime_with_dom` helper
(`dom.rs:12896`). Ported all 7 1:1 into `document_pip.rs` itself (gated
`#[cfg(all(test, feature = "v8-backend"))]`) via a local `with_document_pip` helper that mirrors
S12b-12's device_sensors pattern: bare `V8JsRuntime::new()` + full `install_dom` (the shim's
classes `extend EventTarget`/`Event`, which only exist after `dom.rs`'s DOM-core JS runs — same
reason S12b-12 needed the full install, not a bare context). Deleted the rquickjs
`install_document_pip_api` fn, gated `DOCUMENT_PIP_SHIM` behind `#[cfg(feature = "v8-backend")]`
(only referenced from the v8 path now), and dropped the call site in `lib.rs`'s
`QuickJsRuntime::install_dom`. Found one incidental casualty: `dom.rs`'s own
`event_target_dependent_apis_installed` regression test (BUG-067/070, checks that several
`extends EventTarget` shims all install correctly on the rquickjs path) asserted
`typeof documentPictureInPicture === 'object'` — since document_pip no longer installs on the
rquickjs path, removed that clause (and updated the preceding comment's module list); the same
assertion now lives in `document_pip.rs`'s own ported tests, so coverage isn't lost, just
relocated. `cargo test -p lumen-js --features v8-backend document_pip` - 7/7 green; default-feature
`cargo test -p lumen-js --lib` (full suite) - 2304/2304 green (2311 − 7 moved tests); `cargo clippy
-p lumen-js --all-targets -- -D warnings` clean on both default and `v8-backend` features (one
`empty_line_after_doc_comments` trigger fixed — same pattern as S12b-5/8/10/12, module doc
converted to `//!`); `cargo check -p lumen-shell` (default) green.

### S12b-14 — `inert.rs` (2026-07-18, branch p1-v8-s12b-14-inert)

Fourteenth slice, same systematic selection: `comm -12` on the still-present rquickjs
`fn install_*(…Ctx…)` sites vs the `fn install_*_v8(` sites gives the remaining candidates;
sorted by file size, the smallest non-trap candidate is `inert.rs` (200 lines) — the known traps
`typed_om_api.rs` (S12b-9), `serial.rs`/`scroll_snap_events.rs` (S12b-10) sit below it. `inert.rs`
is clean by the file-stem method: **zero `dom.rs` hits** for `inert`, and its call site in
`lib.rs`'s `QuickJsRuntime::install_dom` is a plain one-liner (no `QuickJsRuntime` `fire_*`/`take_*`
method, unlike `scroll_snap_events`). Pure JS-shim `eval` (no native bindings), the
`HTMLElement.prototype.inert` getter/setter (HTML LS §6.7) Phase-0 stub — stores `_inert` on the
element instance and calls a `globalThis._lumen_set_inert(nid, bool)` no-op stub the shell will
wire in Phase 1. Exactly the S12b-1..8 shape (own-file `mod tests`, not `dom.rs`). Deleted the
rquickjs `install_inert_api` fn + its `use rquickjs::Ctx`; gated `INERT_SHIM` behind
`#[cfg(feature = "v8-backend")]` (only referenced from the v8 path now, same as S12b-12/13's SHIM
consts); no `empty_line_after_doc_comments` fix needed — the module doc was already `//!`. Ported
all 8 tests 1:1 to `V8JsRuntime` (bare `V8JsRuntime::new()` + the same HTMLElement-stub eval +
`install_inert_api_v8`, `with_inert_api` single-helper pattern, gated
`#[cfg(all(test, feature = "v8-backend"))]`); dropped the call site in `lib.rs`'s
`QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features v8-backend inert` — 8/8 green;
`cargo check -p lumen-js` on default + `v8-backend` features — green; `cargo clippy -p lumen-js
--all-targets -- -D warnings` clean on both.

### S12b-15 — `download_bindings.rs` (2026-07-18, branch p1-v8-s12b-15-download)

Fifteenth slice, same systematic selection (`comm -12` on still-present rquickjs
`fn install_*(…Ctx…)` sites vs `fn install_*_v8(` sites, sorted by file size): the smallest
non-trap candidate is `download_bindings.rs` (202 lines) — the known traps `typed_om_api.rs`
(S12b-9), `serial.rs`/`scroll_snap_events.rs` (S12b-10) sit below it. Clean by the file-stem
method (**zero `dom.rs` hits** for `download`) with its own-file `mod tests`; call site in
`lib.rs`'s `QuickJsRuntime::install_dom` is a plain one-liner (no `QuickJsRuntime`
`fire_*`/`take_*` method). This module *does* have a native binding
(`_lumen_network_download(url, filename)` → process-global `QUEUE` drained by the shell via
`take_download_requests`), but the rquickjs path was a thin `Function::new` + `ctx.eval` shim
whose V8 twin (`install_download_bindings_v8`, `into_v8_fn2` + `register_native` + the same
`_lumen_download` convenience `eval`) already existed from the S5–S7 batch. Deleted the rquickjs
`install_download_bindings` fn + its `use rquickjs::{Ctx, Function}`; the engine-agnostic
`enqueue`/`take_download_requests`/`DownloadRequest`/`QUEUE` (shell-facing) stay untouched. No
`SHIM` const to gate — the shim is inline in the eval string. Ported all 6 tests 1:1 to
`V8JsRuntime` (bare `V8JsRuntime::new()` + `install_download_bindings_v8`, no `install_dom`
needed since `_lumen_network_download` is a plain global; same process-global `TEST_LOCK` +
`guard()` queue-drain pattern), gated `#[cfg(all(test, feature = "v8-backend"))]`; dropped the
call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features
v8-backend download` — 6/6 green; `cargo check -p lumen-js` on default + `v8-backend` — green;
`cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both.

### S12b-16 — `content_index.rs` (2026-07-20, branch p1-v8-s12b-16-content-index)

Sixteenth slice, same systematic selection (`comm -12` on still-present rquickjs
`fn install_*(…Ctx…)` sites vs `fn install_*_v8(` sites, sorted by file size): after the known
traps `typed_om_api.rs` (148, S12b-9), `serial.rs` (151, S12b-10) and `scroll_snap_events.rs`
(179, S12b-10), the smallest non-trap candidate is `content_index.rs` (203 lines). Clean by the
file-stem method (**zero `dom.rs` hits** for `content_index`/`ContentIndex`) with its own-file
`mod tests`; call site in `lib.rs`'s `QuickJsRuntime::install_dom` is a plain one-liner (no
`QuickJsRuntime` `fire_*`/`take_*` method). Exactly the S12b-1..8 shape: pure JS-shim `eval` (no
native bindings), the Content Index API Level 1 Phase-0 stub (`ContentIndex` class with
`add`/`getAll`/`delete`, wired onto `ServiceWorkerRegistration.prototype.index`; in-memory, no
persistence). Deleted the rquickjs `install_content_index_api` fn + its `use rquickjs::Ctx`; gated
`CONTENT_INDEX_SHIM` behind `#[cfg(feature = "v8-backend")]` (only referenced from the v8 path now,
same as S12b-12/13/14's SHIM consts); no `empty_line_after_doc_comments` fix needed — the module
doc was already `//!`. Ported all 5 tests 1:1 to `V8JsRuntime` (bare `V8JsRuntime::new()` + the
same `ServiceWorkerRegistration`-stub eval + `install_content_index_api_v8`, `with_content_index`
single-helper pattern — no `install_dom` needed since the shim only touches `globalThis` and
`ServiceWorkerRegistration.prototype`), gated `#[cfg(all(test, feature = "v8-backend"))]`; dropped
the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features
v8-backend content_index` — 5/5 green; `cargo check -p lumen-js` (default) — green; `cargo clippy
-p lumen-js --all-targets -- -D warnings` clean on both default and `v8-backend` features.

### S12b-17 — `csp.rs` (2026-07-20, branch p1-v8-s12b-17-csp)

Seventeenth slice, same systematic selection. After `content_index.rs` (S12b-16) the next smallest
non-trap `install_*(…Ctx…)` candidate is `csp.rs` (206 lines). Clean by the file-stem method
(**zero `dom.rs` hits** for `csp`/`SecurityPolicyViolationEvent`/`_lumen_dispatch_csp_violation`)
with its own-file `mod tests`; call site in `lib.rs`'s `QuickJsRuntime::install_dom` is a plain
one-liner (no `QuickJsRuntime` `fire_*`/`take_*` method — the Phase-1 `_lumen_fire_csp_violation`
native does not exist yet). Same S12b-1..8/16 shape: pure JS-shim `eval` (no native bindings), the
CSP3 §7.8 Phase-0 stub (`SecurityPolicyViolationEvent extends Event` + the
`window._lumen_dispatch_csp_violation` dispatch helper). Deleted the rquickjs `install_csp_bindings`
fn + its `use rquickjs::Ctx`; gated `CSP_SHIM` behind `#[cfg(feature = "v8-backend")]` (only
referenced from the v8 path now, as S12b-12/13/14/16). Ported all 6 tests 1:1 to `V8JsRuntime`;
unlike S12b-16 the CSP shim needs `Event`/`window`/`document`/`location`, so `with_csp_api` evals a
minimal DOM stub (matching the old rquickjs test's stub, but assigning on `globalThis`) on a bare
`V8JsRuntime::new()` before `install_csp_bindings_v8` — evals on one runtime share global state so
`_dispatched` persists across the assertion `eval`. Gated `#[cfg(all(test, feature =
"v8-backend"))]`; dropped the call site in `lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p
lumen-js --features v8-backend csp` — 6/6 green; `cargo check -p lumen-js --features v8-backend` —
green; `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` clean. Next
candidate S12b-18 = `webxr.rs` (210), then `permissions_policy.rs` (214), `highlight_api.rs` (215).

### S12b-18 — `permissions_policy.rs` (2026-07-20, branch p1-v8-s12b-18-permissions-policy)

Eighteenth slice. **`webxr.rs` (210), listed as the next candidate at the end of S12b-17, is
disqualified — same trap as `serial.rs` (S12b-10):** the naive file-stem grep (`webxr`/`WebXR`)
only matches comments in `dom.rs`, but `dom.rs`'s `event_target_dependent_apis_installed` test
asserts `typeof navigator.xr === 'object'`, so deleting the rquickjs `install_webxr_bindings` call
site would break that test on the default (quickjs) build. That shared test pins six modules to the
rquickjs path (`navigator.hid`/`usb`/`bluetooth`/`serial`/`xr` + `window.navigation`); each must be
handled as a coordinated cluster (or the shared test refactored) rather than as an independent
single-file slice — deferred. The next non-trap candidate by size is therefore `permissions_policy.rs`
(214 lines).

Clean by the file-stem method (**zero `dom.rs` hits** for `permissions_policy`/`permissionsPolicy`/
`featurePolicy`/`FeaturePolicy` — the `FeaturePolicy` shim does not `extends EventTarget`, so it is
not in the `event_target_dependent_apis_installed` test) with its own-file `mod tests`; call site in
`lib.rs`'s `QuickJsRuntime::install_dom` is a plain block (no `QuickJsRuntime` `fire_*`/`take_*`
method — `_lumen_set_permissions_policy` is a plain global assigned inside the shim, not a native
binding). Same S12b-1..8/16/17 shape: pure JS-shim `eval`, the W3C Permissions Policy §8 Phase-0
stub (`document.featurePolicy` + `document.permissionsPolicy` alias, `allowsFeature`/`features`/
`allowedFeatures`/`getAllowlistForFeature`, and the `_lumen_set_permissions_policy(headerValue)`
header-parse hook). Deleted the rquickjs `install_permissions_policy_bindings` fn + its
`use rquickjs::Ctx`; gated `PERMISSIONS_POLICY_SHIM` behind `#[cfg(feature = "v8-backend")]` (only
referenced from the v8 path now, as S12b-12/13/14/16/17). Ported all 6 tests 1:1 to `V8JsRuntime`;
like S12b-17 the shim needs `window`/`document`, so `with_pp_api` evals a minimal `window = globalThis`
+ `document = {}` stub on a bare `V8JsRuntime::new()` before `install_permissions_policy_bindings_v8`
— evals on one runtime share global state so the internal `_ppStore` persists across the assertion
`eval`. Gated `#[cfg(all(test, feature = "v8-backend"))]`; dropped the call site (comment + block) in
`lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features v8-backend
permissions_policy` — 6/6 green; `cargo check -p lumen-js` (default) + `--features v8-backend` —
green; `cargo clippy -p lumen-js --all-targets -- -D warnings` clean on both default and `v8-backend`
features. Next candidate S12b-19 = `highlight_api.rs` (215).

### S12b-19 — `highlight_api.rs` (2026-07-21, branch p1-v8-s12b-19-highlight-api)

Nineteenth slice, next by size after `permissions_policy.rs`. Clean by the file-stem method
(all `highlight`/`Highlight` hits in `dom.rs` are the unrelated `.highlight` CSS class used by
selector tests, not the CSS Highlight API — no cluster trap). The file's `#[cfg(test)]` block
tests `HighlightRegistry`/`Highlight` (plain Rust structs backing the JS shim) directly, with no
`rquickjs::Ctx` dependency at all, so — unlike every prior slice — there was nothing to port.
Deleted the rquickjs `install_highlight_api_bindings` fn (no `use rquickjs::Ctx` to remove, it
took `&rquickjs::Ctx` inline); gated `HIGHLIGHT_API_SHIM` behind `#[cfg(feature = "v8-backend")]`
(only referenced from `install_highlight_api_bindings_v8` now); dropped the call site (comment +
block) in `lib.rs`'s `QuickJsRuntime::install_dom`. `cargo test -p lumen-js --features v8-backend
highlight_api` — 9/9 green; `cargo check -p lumen-js` (default) + `--features v8-backend` — green;
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` clean. Next
candidate by size: re-audit the `webxr.rs`-cluster deferral from S12b-18 (`navigator.hid`/`usb`/
`bluetooth`/`serial`/`xr` + `window.navigation`, pinned together by `dom.rs`'s
`event_target_dependent_apis_installed`) or pick the next non-trap single file.

### S12b-20 — `pointer_capture.rs` (2026-07-21, branch p1-v8-s12b-20-pointer-capture)

Twentieth slice. Deferred the `webxr.rs` cluster re-audit again; `pointer_capture.rs` (102 lines)
is the next non-trap single file — clean by the file-stem method (zero `pointer`/`Pointer` hits
in `dom.rs`'s `event_target_dependent_apis_installed`; `setPointerCapture`/`releasePointerCapture`/
`hasPointerCapture` are plain `Element.prototype` methods in the shim, not `EventTarget`-derived
globals, so no cluster trap). Ported `install_pointer_capture_bindings` (three natives —
`_lumen_set_capture_state`/`_lumen_release_capture_state`/`_lumen_get_capture_nid`) to
`install_pointer_capture_bindings_v8`, registered via `V8JsRuntime::register_native` instead of
`rquickjs::Function::new`. Unlike the plain `install_v8!` macro slices, this one needs an
extra-arg call site (mirrors `geolocation`/`shared_worker`): `V8JsRuntime` gained its own
`pointer_capture_nid: Arc<Mutex<Option<u32>>>` field plus `pointer_capture_nid()`/
`take_pointer_capture()` accessors, mirroring `QuickJsRuntime`'s fields of the same name so the
shell's `PersistentJs` trait (`V8PersistentJs` in `crates/shell/src/main.rs`) observes the same
state the natives mutate — the pointer-event dispatch routing in `main.rs` was already
engine-agnostic via the trait, so only the V8-side wiring was missing. Deleted the rquickjs
`install_pointer_capture_bindings` fn + its call site in `lib.rs`'s `QuickJsRuntime::install_dom`
(the `QuickJsRuntime::pointer_capture_nid` field/accessors stay — still the live rquickjs-path
state). 4/4 own-file tests ported 1:1 to `V8JsRuntime`. `cargo test -p lumen-js --features
v8-backend pointer_capture` — 7/7 green (4 own-file + 3 pre-existing `dom::tests` pointer-capture
cases); `cargo check -p lumen-js` (default) + `--features v8-backend` — green; `cargo check -p
lumen-shell --no-default-features --features backend-femtovg,v8` — green; `cargo clippy
--workspace --all-targets -- -D warnings` clean; `scripts/scoped-test.sh` — all green (0 failed
across `lumen-ai`/`lumen-bench`/`lumen-bidi-server`/`lumen-driver`/`lumen-js`/`lumen-knowledge`/
`lumen-mcp`/`lumen-network`/`lumen-paint`/`lumen-shell`/`lumen-storage`). Next candidate by size:
`battery_bindings.rs` (95) — clean by the file-stem method (zero `battery`/`Battery` hits in
`dom.rs`), or re-audit the still-deferred `webxr.rs` cluster.

### S12b-21 — `webtransport.rs` (2026-07-21, branch p1-v8-s12b-21-webtransport)

Twenty-first slice. The listed `battery_bindings.rs` (95) candidate turned out to be a false
positive of the `grep -rl rquickjs` selection method: its only `rquickjs` hit is a doc comment
saying "rquickjs side removed in S12b-4" — the file is already 100% V8-only. Same false positive
for `badging.rs` (S12b-1's doc comment) and `pointer_capture.rs` (S12b-20's, just completed).
Re-ran the selection grep filtering for files that still have a live `use rquickjs::Ctx` import;
smallest was `webtransport.rs` (108 lines, WebTransport Phase-0 stub, all operations reject — no
QUIC). Tracing its call site turned up something the prior 20 slices didn't hit:
`install_webtransport_bindings` has **zero callers anywhere in the repo** — not from
`QuickJsRuntime::install_dom` (verified by grepping the full ~700-line function body), not from
`V8JsRuntime::install_dom`, not from shell. `window.WebTransport` is already `undefined` on both
engines today; this was dead code since it was added (`8ed672dc`), never wired in. Per CLAUDE.md
("never target new functionality at the rquickjs path" + don't add features beyond what the task
requires), the correct action for dead code is deletion, not porting — inventing a newly-wired V8
stub for a capability that was never live anywhere would be new functionality disguised as a
migration slice. Deleted `crates/js/src/webtransport.rs` whole (incl. its trivial
`webtransport_stub_exists` test, which only asserted the shim constant was non-empty) and the
`pub mod webtransport;` line in `lib.rs`. Updated `CAPABILITIES.md`/`ROADMAP.md`
(`P3-webtransport`) to stop describing a "stub" that no longer exists. `cargo check -p lumen-js`
(default) + `--features v8-backend` — green; `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. **Selection method update:** grep for a live `use rquickjs::Ctx`
import, not just any `rquickjs` substring, before ranking candidates by size — and always confirm
an actual `install_dom` call site exists before committing to a candidate (`contacts.rs`, 110
lines, confirmed live via `lib.rs:926`'s `contacts::init_contacts_manager` call — good next
candidate).

### Audit before resuming S12b (2026-07-29)

S12b had gone quiet since S12b-21 (2026-07-21) — not reserved by a branch, not tracked in
`STATUS-P1.md`. User-requested full removal (not just default-off) triggered a fresh sweep:
119 files in `crates/js/src` still reference rquickjs, 1139 hits across 140 `.rs` files
workspace-wide, 2336 `#[test]` fns still gated on it (1047 in `dom.rs`'s own `mod tests`).
Architecture confirmed unchanged from the S12b scoping note: each binding module carries
*both* implementations (`install_X` rquickjs + `install_X_v8`) in the same file — removal is
line-by-line surgery per file, not file deletion. `crates/js/Cargo.toml`'s `rquickjs` dep is
still **hard**, not optional (this crate has no `quickjs` feature at all) — cutting the
Cargo-feature gate alone won't drop it from `Cargo.lock`.

Three items block a clean finish and must not be treated as "just another slice":

1. **`crates/driver/src/winit_session.rs:1055-1096`** — `WinitSession::eval()` has only an
   rquickjs implementation; `#[cfg(not(feature = "quickjs"))]` returns an error instead of
   calling `V8JsRuntime`, even though `lumen-driver` already has a `v8` feature (used by
   `session.rs::InProcessSession`). Deleting `quickjs` from `driver` without porting this first
   permanently breaks headless `eval` on that path.
2. ~~**BUG-350** — the ESM stack (`esm.rs`, `QuickJsRuntime::eval_module`) is rquickjs-only;
   `V8JsRuntime` has no `eval_module`/`register_module_source` override, so the trait default
   (`ext.rs::eval_module` → `self.eval(source)`) feeds `export`/`import` through classic-script
   parsing and fails on all 80 vendored WPT files using `type="module"`. Porting this closes
   BUG-350 as a side effect, not a separate bug.~~ **Closed by S12b-23** (2026-07-29) — see
   the findings entry below; the WPT files additionally need module-graph *fetching*
   ([BUG-446](../../bugs/BUG-446-OPEN.md)), which is a separate, engine-independent gap.
3. **`dom.rs`'s `mod tests` monolith** (~12796-26677, 1047 tests) — the only regression
   coverage for a large swath of DOM behavior (events, forms, storage, IndexedDB, fetch/XHR,
   Cache, WebSockets, history, scroll). No V8-side equivalent exists; it needs a docs/tasks
   brief and a per-subarea split, not a single slice.

Also re-confirmed the two known "trap" categories from S12b-9/10/18: modules pinned to a
cluster test (`dom.rs::event_target_dependent_apis_installed` — webxr/serial/hid/usb/
bluetooth/navigation) and modules whose only `rquickjs` hit is a stale doc comment (false
positive for candidate selection, same as S12b-1/4/20/21 above).

Filed as ROADMAP.md rows `P3-v8-s12b-22`..`P3-v8-s12b-25` (the three blockers above plus a
final Cargo.toml/feature-gate/docs cleanup slice, strictly last) and `P3-v8-post-audit`
(sweep `BUGS.md` OPEN rows for QuickJS-era assumptions once S12b-25 lands), queued at the top
of `STATUS-P1.md` ahead of the existing bug-fix queue. Full audit trail — memory
`project_quickjs_full_removal_audit_s12b` (assistant session memory, not part of this repo).

### S12b-22 — `WinitSession::eval` → V8 (2026-07-29, branch p1-v8-s12b-22)

Blocker 1 from the audit above, closed. `crates/driver/src/winit_session.rs` now builds a
one-shot `lumen_js::v8_runtime::V8JsRuntime` (`V8JsRuntime::new` + the same 11-argument
`install_dom` the rquickjs path used) instead of `QuickJsRuntime`; the gate moved from
`#[cfg(feature = "quickjs")]` to `#[cfg(feature = "v8")]`. The **one-shot, DOM-clone**
semantics are deliberately preserved, not upgraded: `WinitSessionState` holds a plain
`Document`, not the `Arc<Mutex<Document>>` a persistent runtime needs, and the persistent
variant already exists as `InProcessSession` (DEVX-5). Making `WinitSession` persistent is a
separate refactor with its own layout/paint write-back question — out of scope here.

The `quickjs` feature was dropped from `crates/driver/Cargo.toml` in the same slice rather
than left for S12b-25: after the port no `#[cfg]` in the crate referenced it, and *no crate in
the workspace ever enabled it* (`shell`, `mcp`, `bidi-server` all take `lumen-driver` with its
defaults, `["cpu-render", "v8"]`). Consequence worth recording: `WinitSession::eval` was
returning `"eval требует пересборку с --features quickjs"` in **every** build the workspace
produces — the SDC-1a capability was documented as working-behind-a-flag but was in fact dead
everywhere, and the two tests covering it in `test_automation_commands.rs`
(`eval_reads_back_dom_state_after_click_and_type`, `eval_runs_plain_expression`) had not been
compiled, let alone run, since they were written. Both are now gated on `v8` and pass on V8
unmodified — the assertions (`getAttribute('checked')` → `"checked"`, `getAttribute('value')`
→ `"Lumen"`, `1 + 1` → `2`) held across the engine swap with no adjustment.

**Lesson for the remaining slices:** a `#[cfg(feature = …)]` on a feature nothing enables is
not a rollback path, it is dead code plus a dead test suite — grep for who actually turns a
feature on before trusting a "still available under `--features X`" line in the docs.

### S12b-23 — ESM (`<script type=module>`) → V8 (2026-07-29, branch p1-v8-s12b-23-esm)

Blocker 2 from the audit above, closed; [BUG-350](../../bugs/BUG-350-FIXED.md) fixed. New
`crates/js/src/v8_esm.rs` drives `script_compiler::compile_module` →
`instantiate_module(resolve_module_callback)` → `evaluate` →
`perform_microtask_checkpoint` (the V8 analogue of the QuickJS
`while ctx.execute_pending_job()` drain), and `impl JsRuntime for V8JsRuntime` now overrides
`eval_module` / `register_module_source`, so the classic-eval trait default in `ext.rs` is no
longer reachable on the default build. `V8JsRuntime::set_import_map` was added and the shell's
V8 branch calls it, retiring the "module scripts fall back to `NotImplemented` until ESM
lands" comment in `run_scripts_with_dom`.

**Why the plumbing diverges from rquickjs, and where it deliberately does not.** rquickjs
takes a `Resolver`/`Loader` pair *by value* (`Runtime::set_loader`), so `QuickJsRuntime` shares
its registries with them through `Arc<Mutex<…>>` fields. V8's `ResolveModuleCallback` is a
captureless `extern "C" fn` — there is no `data` pointer to smuggle state through — so the
registries (sources, compiled modules, `identity_hash → specifier`, page URL, import map) live
in a `thread_local!` on the isolate's dedicated JS thread. That is exactly as scoped as the
QuickJS version (per runtime, per thread), and it is the same pattern S9/S10 already used for
`wasm::v8_bridge` and `HUB_V8`. Specifier *resolution*, by contrast, is shared: extracted from
`LumenResolver::resolve_specifier` into the free function `esm::resolve_specifier_with`, which
both engines call — so import maps, relative-URL joining and the virtual `lumen://inline-N`
base cannot drift apart.

**Two rquickjs-era workarounds that V8 makes unnecessary.** Import attributes
(`import … with { type: 'json' }`) arrive in the resolve callback as a ready `FixedArray`
(`[key, value, source-offset]` per attribute), so the Phase 0 source-stripping preprocessor in
`import_attributes.rs` — written because rquickjs 0.11 cannot parse the clause and its `Loader`
never sees attributes — is simply not on the V8 path. Dynamic `import()` is wired through
`set_host_import_module_dynamically_callback` instead of being driven by the loader trait; its
array is `[key, value]` per attribute (no offset), hence the `stride` parameter in
`declared_type`. `import.meta` deliberately *keeps* the shared source-level transformer
(`import_meta.rs`): the `.url` / `.resolve()` / `.env` shape is Lumen policy, not an engine
capability, and reusing it keeps both engines byte-identical there.

Inline modules were also switched on in the driver's headless path
(`session.rs::run_page_scripts`, after the classic scripts per HTML LS §8.1.3.1). No page in
`graphic_tests/` uses `type="module"`, so the CPU-snapshot gate is untouched.

The rquickjs ESM stack (`esm.rs`'s `Loader`/`Resolver` impls, `QuickJsRuntime::eval_module`)
was **not** removed here — it is still the live path under `--features quickjs`, and its tests
belong to the `dom.rs`/final-cleanup slices (S12b-24/25).

**Remaining gap, filed rather than silently shipped:** nothing in the shell ever calls
`register_module_source`, so a page's `import './helper.js'` resolves to a specifier no one
ever registered and fails with `module '…' not found`. That is not a regression (the rquickjs
`LumenLoader` also only read a pre-populated registry, and the shell never populated it) and
not what BUG-350 described, but it is what the 80 vendored `type="module"` WPT files actually
need — [BUG-446](../../bugs/BUG-446-OPEN.md), with a concrete fix sketch (scan specifiers with
the `import_attributes.rs` lexer, BFS-prefetch through the shell's existing synchronous
subresource path — *not* network I/O inside the synchronous V8 callback).

### S12b-24 — scoping only, no code deleted (2026-07-29, branch p1-v8-s12b-24)

Blocker 3 from the audit above (`dom.rs`'s test monolith), scoped per its own "needs a
docs/tasks brief before start" requirement — same shape as the original S12b scoping-only
session, nothing ported or deleted here.

**Corrected boundaries.** The 2026-07-14 audit's numbers (`~12796-26677`, 1047 tests) are stale
— the file has grown since. Current truth: `#[cfg(test)] mod tests {` opens at
`crates/js/src/dom.rs:15982` and runs to EOF (`:31167`); no nested `mod` sub-blocks. **1113**
`#[test]` fns total, all inside this range; nothing above line 15982 is test code (that's the
`QuickJsRuntime` native-binding registrations plus the engine-agnostic `WEB_API_SHIM` string at
`:3323`, out of scope here).

**Helpers.** 13 `runtime_with_*`/`runtime_deterministic` constructors build the `QuickJsRuntime`
each test runs against. `runtime_with_dom` (`:16035`) dominates — **~976 of 1113 tests** (~88%)
use it directly, so splitting by helper is not a useful axis on its own. The five helpers that
*do* matter for planning are the ones injecting a mock provider trait, since porting them means
confirming `V8JsRuntime::install_dom` accepts the same injection point: `runtime_with_cache_backend`
(`CacheBackend`, 13 tests), `runtime_with_ws`/`runtime_with_mock_ws`/`runtime_with_binary_ws`
(`JsWebSocketProvider`, 23 tests total), `runtime_with_mock_sse` (`JsSseProvider`, 11 tests),
`runtime_with_idb` (`IdbBackend`, 42 tests across in-memory + persistence-across-reload
variants), `runtime_with_fetch`/`runtime_with_abort_fetch`/`runtime_with_blocking_fetch`
(`JsFetchProvider`, 8 tests). None of the mock provider structs themselves (`MockWsProvider`,
`MockSseProvider`, `CaptureFetch`, `BlockingFetch`, `MockCacheBackend`, …) touch `rquickjs`
types — they implement `lumen_core::ext` traits consumed engine-agnostically by `install_dom` —
so they're very likely portable verbatim; only the `QuickJsRuntime::new()`/`.install_dom(...)`
call sites need retargeting. Confirm signature parity against `v8_runtime.rs` before assuming
this holds for all five.

**Subarea breakdown (proposed slicing, not final).** ~30 natural sections by file order, each a
candidate slice of 10-60 tests (exact sub-family counts inside the largest unlabeled block,
`dom.rs:16043-17530`, don't cleanly sum to its own "~55" header count — re-verify line-by-line
when a session actually starts that slice, don't trust the paragraph total blindly):

| Slice area | Lines | ~Tests | Helper(s) |
|---|---|---|---|
| Core DOM basics (console/SVG/self&window/title/body/createElement, Canvas2D 23, query/selector 25, text/attrs 14, `Image` ctor 7, timers/scheduler 12, History API 21) | 16043-17530 | ~55 (sub-families above sum higher — recount at slice start) | `runtime_with_dom`, `runtime_with_url` (History) |
| classList / CSSStyleDeclaration | 17531-17698 | 13 | `runtime_with_dom` |
| Element event dispatch + Event/CustomEvent ctors | 17699-17842 | 12 | `runtime_with_dom` |
| Service Worker + Cache Storage | 17843-18298 | ~30 | `runtime_with_dom` |
| Cache API — SQLite backend | 18299-18508 | 13 | `runtime_with_cache_backend` |
| IME composition + bfcache pageshow/pagehide | 18509-18669 | 14 | `runtime_with_dom` |
| Fetch bindings (Headers/Request/Response/AbortController existence) | 18670-18791 | 14 | `runtime_with_dom` |
| WebSocket ctors/constants + connect-failure; bfcache-blocked-by-ws/es filters | 18792-19134 | ~17 | `runtime_with_ws`, `runtime_with_dom` |
| WebSocket mock session behavior | 18959-19163 | ~12 | `runtime_with_mock_ws` |
| EventSource/SSE | 19135-19569 | 21 | `runtime_with_mock_sse` |
| WebSocket binary mode | 19514-19578 | 4 | `runtime_with_binary_ws` |
| Location/NavigateRequest/history+URL sync | 19570-19945 | ~50 | `runtime_with_url` |
| Web Storage (localStorage/sessionStorage) | 19946-20026 | ~10 | `runtime_with_storage` |
| URLSearchParams + URL | 20027-20163 | ~20 | `runtime_with_dom` |
| Performance API + PerformanceObserver | 20164-20356 | ~19 | `runtime_with_dom` |
| queueMicrotask + rAF/cAF incl. EE-5 vsync batching | 20354-20573 | ~21 | `runtime_with_dom` |
| MutationObserver / ResizeObserver / IntersectionObserver (+ rootMargin cluster at 22200-22298) | 20574-21234, 22200-22298 | ~36 | `runtime_with_dom` |
| ChildNode/ParentNode/ElementTraversal + TreeWalker/NodeIterator | 21235-21656 | ~24 | `runtime_with_dom` |
| matchMedia/MediaQueryList | 21717-21923 | 13 | `runtime_with_dom` |
| Element geometry + scroll events + CSS Scroll Snap | 21924-22099 | ~18 | `runtime_with_dom` |
| Lazy image loading + IO rootMargin math | 22100-22298 | ~14 | `runtime_with_dom` |
| FontFaceSet, Shadow DOM, Custom Elements, `<template>`/DocumentFragment | 22299-22648 | ~42 | `runtime_with_dom` |
| IndexedDB in-memory ops | 22646-23011 | ~34 | `runtime_with_idb` |
| IndexedDB persistence across runtime reload | 23012-23173 | 8 | `runtime_with_idb` (shared backend across two runtimes — see hard-to-port note) |
| FormData API | 23174-23594 | 22 | `runtime_with_dom` |
| Fetch abort/blocking/multipart-body | 23423-23594 | ~8 | `runtime_with_abort_fetch`, `runtime_with_blocking_fetch`, `runtime_with_fetch` |
| Selection + Range + execCommand + contentEditable | 23595-24141 | ~68 | `runtime_with_dom` + `make_selection_doc`/`make_contenteditable_doc` |
| `getComputedStyle()` | 24142-24369 | 16 | `runtime_with_dom` + `make_computed_styles_map` |
| Web Crypto + SubtleCrypto | 24370-24878 | ~40 | `runtime_with_dom` (heavy `_lumen_drain_microtasks`, see below) |
| Trusted Types (two sections) | 24879-24998, 29108-29242 | ~19 | `runtime_with_dom` |
| structuredClone | 24999-25254 | 18 | `runtime_with_dom` |
| btoa/atob, Blob, File, FileReader | 25255-25430 | ~19 | `runtime_with_dom` |
| Page Visibility + readyState/lifecycle | 25431-25610 | ~16 | `runtime_with_dom` |
| sendBeacon + fetch keepalive/priority + `URL.createObjectURL` | 25611-25740 | ~11 | `runtime_with_dom` |
| Event class hierarchy (UIEvent/MouseEvent/KeyboardEvent/PointerEvent dispatch) | 25741-26145 | ~31 | `runtime_with_dom` |
| WHATWG Streams + fetch streaming body | 26146-26686 | ~42 | `runtime_with_dom` (heaviest `_lumen_drain_microtasks` removal, see below) |
| `<details>`/`<dialog>`/`<selectlist>`/Popover incl. `popover=hint` | 26687-27278 | ~62 | `runtime_with_dom` + 5 `make_*_doc` fixture builders |
| Form Constraint Validation API | 27279-27607 | ~31 | `runtime_with_dom` + `make_form_doc` |
| requestIdleCallback, MessageChannel/MessagePort, clipboard/permissions, isSecureContext | 27607-27849 | ~29 | `runtime_with_dom` |
| Web Worker | 27850-28004 | 18 | `runtime_with_dom` |
| gc_collect, deterministic render mode | 28005-28133 | 12 | `runtime_with_dom`, `runtime_deterministic` |
| `window.open()`/opener | 28134-28219 | 9 | `runtime_with_dom` |
| Web Animations API (two sections) | 28220-28421, 30429-30490 | ~25 | `runtime_with_dom` |
| CompressionStream/DecompressionStream | 28422-28637 | ~11 | `runtime_with_dom` (heavy `_lumen_drain_microtasks`) |
| Fullscreen API | 28638-28761 | 10 | `runtime_with_dom` |
| Web Locks + Wake Lock + Network Information + userActivation + Web Share + reportError | 28762-29018 | ~26 | `runtime_with_dom` |
| CSS.supports/escape + Trusted Types #2 + Storage Access + EventTarget cluster + `css.registerProperty` (unlabeled catch-all — split along constituent APIs when porting, don't keep together) | 29019-29517 | ~40 | `runtime_with_dom` |
| Performance/Resource/Navigation Timing L2 | 29517-29735 | ~24 | `runtime_with_dom` |
| CSS Typed OM L1 (+ `bug_281_*` doc-identity tests interleaved at 29788-29820, unrelated, physically misplaced) | 29736-29901 | ~18 | `runtime_with_dom` |
| DOM node count/quota bindings + `chrome.runtime` stub | 29901-30020 | ~10 | `runtime_with_dom` |
| Drag and Drop + window scroll (CSSOM View) + Phase-5 HTML5 APIs (`setHTMLUnsafe`/`getHTML`/`moveBefore`/`checkVisibility`) | 30021-30428 | ~38 | `runtime_with_dom` |
| Pointer Events L3 (capture + coalesced/predicted pointermove) + Pointer Lock | 30491-30768 | ~21 | `runtime_with_dom` |
| DOM Core tail (ProcessingInstruction, CharacterData, prototype chains, DocumentFragment, DocumentType, `DOMImplementation`) | 30769-31167 | ~40 | `runtime_with_dom` |

Naming scheme for follow-up sessions: `S12b-24-<slug>` (e.g. `S12b-24-idb`,
`S12b-24-streams`), branch `p1-v8-s12b-24-<slug>` — mirrors the `S12b-23-esm` pattern, not the
flat `S12b-N` numbering used for the already-finished S5–S10 module sweep (S12b-1..21), so the
two numbering schemes don't collide. File each slice's own ROADMAP.md row when a session actually
picks it up, same as S12b-1..21 were — don't pre-file all ~30 here speculatively.

**Cluster-test finding, confirmed and detailed.** `event_target_dependent_apis_installed`
(`dom.rs:29308`) is the one true cross-subsystem cluster (the earlier audit's finding, now
pinned down): one `&&` chain asserting `navigator.hid`/`.usb`/`.bluetooth`/`.serial`/`.xr` and
`window.navigation` are all objects, purely because they once broke together (all subclass
`EventTarget`). Split into 5-6 single-assertion tests when porting, one per subsystem, and route
each to its owning subarea slice (currently sitting in the unlabeled 29019-29517 catch-all) — a
V8-side regression in any one of them shouldn't hide behind the others. Two more multi-assertion
existence-chain tests exist (`window_exports_all_event_classes` at `:26122`, 17 classes;
`dom_interface_globals_defined` at `:30980`, 10 globals + 3 prototype checks) but both are
benign — every assertion belongs to one coherent family, not cross-subsystem — just worth noting
they'll produce an opaque single pass/fail if something regresses.

**Hard-to-port finding, bigger than the S12b-2 precedent suggested.** `_lumen_drain_microtasks`
(`dom.rs:3024`) is a QuickJS-only native binding that loops `ctx.execute_pending_job()` — V8 has
no equivalent because it auto-drains its microtask queue (same fact S12b-2 already recorded for
AsyncContext, but the scale here is much larger). It's called **85 times** across the suite,
concentrated in SubtleCrypto (~24680-24880), WHATWG Streams (~26190-26680, the heaviest
concentration), MessageChannel/MessagePort (27663-27834), and Compression/DecompressionStream
(28484-28607). Beyond the explicit calls, **at least 8 more tests** (`:24530`, `:24572`,
`:24629`, `:24684`, `:24717`, `:24762`, `:25330`, `:25342`, `:25426`) accept *either* the
resolved value *or* `Null`/`false` as passing, with comments like `// May be false if microtasks
not yet flushed` — written to tolerate QuickJS's synchronous-`eval()`-doesn't-flush-microtasks
behavior. Porting must both drop the dead `_lumen_drain_microtasks()` calls and tighten those
loose assertions to the single deterministic value V8 guarantees (the S12b-2 lesson, at scale).

Two more localized notes: the `idb_persists_across_runtime_reload`/
`local_storage_persists_across_runtimes` pattern (two separate `QuickJsRuntime::new()` instances
sharing one backend) isn't QuickJS-specific, but confirm `V8JsRuntime::new()` tolerates repeated
construction within one test process before porting that cluster — V8 isolate lifecycle can be
pickier than QuickJS contexts about repeated init/teardown. `BlockingFetch`'s real-OS-thread
`sleep`-poll loop (`dom.rs:23460-23479`) never touches `Ctx` and should port unchanged.

### S12b-24-core — first porting slice, "Core DOM basics" (2026-07-29, branch p1-v8-s12b-24-core)

The scoping table's first row, ported whole: **99 tests** (not the row's guessed "~55" — the
per-sub-family counts were the accurate ones, as that row warned) moved out of the QuickJS
monolith into `mod tests::v8_core`, gated `#[cfg(feature = "v8-backend")]`, QuickJS copies
deleted. Families: console/SVG/wrapper identity/`self`&`window`, Canvas 2D,
`getElementById`/`querySelector`/attributes/`textContent`/`Image`, `alert`/`print`, timers +
`scheduler.postTask`, History API.

**Mechanics that the next slices can copy verbatim.** A nested module inside the existing
`mod tests` (not a new file) keeps `use super::*` working, so `make_doc`, the `Arc`/`Mutex`
imports and every other outer helper stay reachable and the diff is a helper-name swap plus a
4-space re-indent. Two twin constructors were enough for this row: `v8_runtime_with_dom` and
`v8_runtime_with_url` — `V8JsRuntime::install_dom`'s signature is argument-for-argument identical
to `QuickJsRuntime::install_dom`, as the scoping note predicted. Helpers whose *only* callers move
into the V8 module must move with them (`test_img_bitmap` did): left behind, they are dead code in
a non-`v8-backend` build and `clippy -D warnings` fails there while passing with the feature on —
so check **both** feature configurations before committing.

**Result: 98 of 99 passed on V8 with zero changes to test bodies.** The families in this row use
only `rt.eval` plus `update_layout_rects`/`flush_canvas_updates`/`register_img_bitmaps`/`take_*`,
all of which `V8JsRuntime` mirrors. That is a useful prior for scheduling the remaining ~1014
tests, but do not generalize it past this row: this slice contains none of the
`_lumen_drain_microtasks` clusters and none of the mock-provider helpers.

**Divergence 1 — a real user-visible defect, [BUG-447](../../bugs/BUG-447-FIXED.md).**
`V8JsRuntime` had no `register_img_bitmaps` at all, and `impl PersistentJs for V8PersistentJs`
(`shell/src/main.rs`) did not override the trait method either — so the shell's call after
`fetch_and_decode_images` hit the trait's no-op default, `img_bitmap_store` stayed empty for the
whole session, and `drawImage(imgElement, …)` silently painted nothing on the default engine.
Fixed in this slice (both halves). Method-set diff of the two `impl PersistentJs` blocks is a
cheap audit worth repeating: it also showed `suspend` (intentionally absent on V8 — DoD item 5)
and `debug_js_heap` (QuickJS-only `TEMP BUG-272` diagnostic) as the only other gaps.

**Divergence 2 — a green QuickJS test that encoded a QuickJS bug.**
`canvas_get_context_webgl_via_2d_shim_is_null` asserted `getContext('webgl') === null` and
explained it as a shim boundary. It is not: `lib.rs::install_dom` evals `webgl_canvas`'s
`WEBGL_SHIM` (line ~748) *before* `dom::install_dom_api` (line ~770) defines `document`, so the
shim's `if (typeof document !== 'undefined' && …)` guard skipped its `document.createElement`
hook and functional WebGL was dead on the whole QuickJS path. `V8JsRuntime::install_dom` evals
`WEB_API_SHIM` first and `webgl_canvas` after, so the hook lands and the call returns the real
software-rasterizer context (probed: `typeof gl.getParameter === 'function'`,
`drawingBufferWidth === 300`, no 2D methods). Test rewritten to the correct expectation and
renamed `canvas_get_context_webgl_returns_functional_context`. No bug filed — the broken path is
rquickjs, which S12b-25 deletes. Expect more of this shape: **an assertion that looks like a
documented limitation may be a fossilized QuickJS defect**, so when a ported test fails, check the
install ordering / engine behavior before "fixing" the port.

### S12b-24-events-cache — second porting slice (2026-07-29, branch p1-v8-s12b-24-events-cache)

Four adjacent rows from the scoping table ported together into a new `mod tests::v8_events_cache`:
classList/CSSStyleDeclaration, Element event dispatch + Event/CustomEvent ctors, Service Worker +
Cache Storage, Cache API SQLite backend. **72 tests**, QuickJS copies deleted. Same mechanics as
`v8_core` (nested module, `use super::*`, twin `v8_runtime_with_*` constructors) — nothing new
needed here, confirming the core slice's prediction that the module-nesting pattern generalizes.

**Two pre-existing OPEN bugs closed as a side effect, not new ones filed.** Porting these tests
meant exercising `_lumen_get_attr`/array-argument natives under V8 for the first time in
`cargo test`, and both hit already-diagnosed-but-unfixed defects head-on:

* [BUG-442](../../bugs/BUG-442-FIXED.md) — `Option::None` mapped to `JsValue::Null` on the V8 side
  instead of `Undefined`, so every bare `_lumen_get_attr(...) !== undefined` presence-check in the
  shim was always true on the default engine. Its own "Как чинить" section flagged two options —
  patch `_lumen_get_attr` alone, or fix the conversion boundary for every native — and warned the
  wider option needs a grep for shim call sites comparing a native's result to `null` directly
  first. Did that grep (`_lumen_[a-z_]+\([^)]*\)\s*(===|!==)\s*null`, whole file): one hit outside
  `dom::tests`, itself defensive (`!== null && !== undefined`). Every `Option<T>`-returning native
  the shim reads is already wrapped in `_lumen_u2n` (`undefined`→`null` normalizer, added in
  BUG-381), so the wider fix was safe: `IntoJsReturn for Option<T>` (`v8_compat.rs`) now returns
  `Undefined`; `jsvalue_to_v8`/`to_v8` keep `Null` and `Undefined` distinct instead of collapsing
  both to V8 `null`.
* [BUG-342](../../bugs/BUG-342-FIXED.md) — `v8_to_jsvalue` collapsed `Array`/`Object` arguments to
  `Null`, so any native taking a `Vec<u8>`/etc. silently saw empty data. Fixed by recursing into
  `is_array()`/`is_object()` the same way the sibling `from_v8` (`v8_runtime.rs`) already did —
  the two converters were not deduplicated, just brought back in sync.

Both fixes verified with a throwaway probe (not committed): `hasAttribute('data-nope') === false`
and `_lumen_sha_digest('SHA-256', [72,101,108,108,111])` producing the correct SHA-256 of `"Hello"`,
both under `V8JsRuntime` directly. Full `cargo test -p lumen-js --features v8-backend` (2569 tests)
stayed green throughout — the shim's blanket `_lumen_u2n` usage is what made the wide fix safe,
not luck. **Lesson for the remaining slices:** porting a sub-area's tests is not just mechanical
relocation — it is the *first real V8 exercise* of whatever natives that sub-area touches, and
prior audits (BUG-442, BUG-342, BUG-447) that diagnosed-but-couldn't-verify-fixed a V8-only defect
are likely to get closed for free the moment the corresponding tests move over. Check `BUGS.md` for
OPEN bugs touching the natives your next slice's helpers use *before* starting the port.

### S12b-24-ws-sse — third porting slice (2026-07-29, branch p1-v8-s12b-24-ws-sse)

Five adjacent scoping-table rows ported together into `mod tests::v8_ws_sse`: IME composition +
bfcache pageshow/pagehide, fetch bindings (Headers/Request/Response/AbortController existence),
WebSocket ctors/constants/connect-failure + the `_lumen_bfcache_blocked` eligibility filters,
WebSocket mock-session behavior + binary mode, EventSource/SSE. **73 tests**, QuickJS copies
deleted; `cargo test -p lumen-js --features v8-backend` 2569/2569 (unchanged total — 73 out, 73
in).

**The mock-provider question the scoping note left open is now answered: yes, verbatim.** This was
the first slice carrying `install_dom` provider injection, and all four mock providers
(`FailWsProvider`, `MockWsProvider` + `MockWsSession`, `MockBinaryWsProvider` +
`MockBinaryWsSession` for `JsWebSocketProvider`; `MockSseProvider` + `MockSseSession` for
`JsSseProvider`) moved without a single line changed — they implement `lumen_core::ext` traits and
never touch an engine type, and `V8JsRuntime::install_dom` takes them at the same argument
positions as `QuickJsRuntime::install_dom`. Only the four `runtime_with_*` constructors needed
retargeting (`v8_runtime_with_ws`/`_mock_ws`/`_mock_sse`/`_binary_ws`). Same for the pump natives:
`_lumen_pump_websockets` and `_lumen_pump_sse` deliver queued `JsWsEvent`/`JsSseEvent` under V8
with no shim or binding change, so the whole event-delivery path (open/message/typed event/
error/server-close-and-reconnect/retry) is exercised as-is. The remaining injection helpers
(`runtime_with_cache_backend` was already proven in S12b-24-events-cache; `runtime_with_idb`,
`runtime_with_fetch`/`_abort_fetch`/`_blocking_fetch`, `runtime_with_storage`) can be scheduled on
this precedent rather than treated as risk.

**Zero engine divergences in this slice** — 73 of 73 passed on the first run with untouched test
bodies. Unlike the two earlier slices this one closed no bugs: the `BUGS.md` pre-check flagged
three OPEN EventSource/Worker defects ([BUG-362](../../bugs/BUG-362-OPEN.md),
[BUG-363](../../bugs/BUG-363-OPEN.md), [BUG-364](../../bugs/BUG-364-OPEN.md)) and none of them are
V8-vs-QuickJS divergences — they are engine-agnostic shim gaps (relative-URL resolution, WebIDL
shape of `EventSource`, `Worker` fetching no script) that the ported suite does not cover on
either engine. Worth stating explicitly so a later reader doesn't mistake "73/73 green" for
"EventSource is spec-conformant": these tests pin Lumen's own Phase-0 SSE plumbing, not the spec.

**One loose assertion tightened (the S12b-2 lesson, applied at last).**
`fetch_without_provider_returns_promise` asserted only that `fetch()` returns a thenable, with a
comment explaining that QuickJS's `eval()` doesn't flush microtasks so the rejection can't be
observed. V8 drains its microtask queue, so the ported test — renamed
`fetch_without_provider_rejects` — now attaches a `.catch` and asserts the flag flipped inside the
same `eval`. Confirmed green, i.e. the no-provider `fetch()` really does reject rather than hang.
Expect ~8 more of this exact shape in the SubtleCrypto/Streams/Compression clusters (line list in
the scoping section above).

### S12b-24-nav-url-storage — fourth porting slice (2026-07-29, branch p1-v8-s12b-24-nav-url-storage)

Three adjacent scoping-table rows ported together into `mod tests::v8_nav_url_storage`:
Location/`NavigateRequest` + fragment navigation + History API (`pushState`/`replaceState`,
`popstate`/`hashchange`), Web Storage (`localStorage`/`sessionStorage`), `URLSearchParams` + `URL`.
**67 tests** (the table guessed ~50 + ~10 + ~20 = ~80 for these three rows — the first row was the
over-count this time, the opposite direction from `v8_core`), QuickJS copies deleted;
`cargo test -p lumen-js --features v8-backend` 2569/2569, unchanged total. Third slice in a row
where the module-nesting mechanics needed nothing new: one more twin constructor
(`v8_runtime_with_storage`) alongside `v8_runtime_with_dom`/`_with_url`.

**Helper-liveness bookkeeping, second occurrence.** This slice took the *last* callers of both
`runtime_with_url` and `runtime_with_storage`, so both definitions had to move with the tests —
the same trap `test_img_bitmap` sprang in `v8_core`, and it stays invisible unless you
`cargo check`/`clippy` the crate **without** `v8-backend` (with the feature on, the leftover is
still "used" by nothing and warns; without it there is no V8 module to justify it at all). Cheap
mechanical check before committing a slice: grep the remaining QuickJS region for each
`runtime_with_*` name the slice touched and confirm the count is zero or non-zero deliberately.

**An open scoping question closed for free.** The scoping note flagged
`idb_persists_across_runtime_reload`/`local_storage_persists_across_runtimes` (two runtimes sharing
one backend) as needing confirmation that `V8JsRuntime::new()` survives repeated construction in
one test process, since "V8 isolate lifecycle can be pickier than QuickJS contexts". The
`local_storage` half of that pattern is in this slice and passes unmodified, so the IndexedDB
reload cluster (8 tests) can be scheduled as ordinary work, not as risk.

**Zero engine divergences, and an explicit non-claim about `URL`.** 67/67 green on the first run
with untouched bodies, no loose microtask-tolerant assertions in this range (the
`_lumen_drain_microtasks` clusters are all downstream). No bugs closed either — but unlike
`v8_ws_sse`, here the pre-check found OPEN defects sitting *directly* on the ported APIs:
[BUG-375](../../bugs/BUG-375-OPEN.md) (only `URL.prototype.href` has a working setter; the other
nine swallow assignment silently), [BUG-346](../../bugs/BUG-346-OPEN.md) (`Url::resolve` keeps
`.`/`..` dot-segments) and the `location`-adjacent
[BUG-359](../../bugs/BUG-359-OPEN.md)/[BUG-358](../../bugs/BUG-358-OPEN.md). All are
engine-agnostic (shim / `lumen-core` / shell), which is exactly why the 19 green `URL` tests do not
touch them: the suite pins Lumen's Phase-0 plumbing, not the spec. Worth recording the reason
`url_resolve_relative_path` (`new URL('../other.html', base).pathname === '/other.html'`) is green
while BUG-346 is open — **there are two independent relative-URL resolvers in the tree**: the JS
`_url_resolve` inside `WEB_API_SHIM`, which `function URL(href, base)` (`dom.rs:10876`) calls and
which *does* collapse dot-segments, and Rust `lumen_core::Url::resolve`, which does not and is what
the shell's `ResourceBase::resolve` uses. Fixing BUG-346 is therefore not "wire the working one in"
blindly, but it does mean a correct reference implementation already exists in-tree. Don't let a
future reader conclude from this slice that `URL`/`location` are conformant.

### S12b-24-perf-observers — fifth porting slice (2026-07-30, branch p1-v8-s12b-24-perf-observers)

Three adjacent scoping-table rows ported together into `mod tests::v8_perf_observers`:
Performance API + PerformanceObserver (incl. the Performance Timeline L2 §6.2.2 single-type
form), `queueMicrotask` + rAF/cAF (incl. the EE-5 vsync batching cluster), and
MutationObserver/ResizeObserver/IntersectionObserver. **71 tests** — the table guessed
~19 + ~21 + ~36 = ~76, the closest any three-row estimate has landed so far — QuickJS copies
deleted; `cargo test -p lumen-js --features v8-backend` 2570/2570 (2569 before: −71 QuickJS,
+72 V8, the extra one explained below).

**The most mechanical slice of the five, and why.** Two properties made it so, both cheap to check
before starting and worth checking on every future slice: (1) the whole range uses exactly one
helper, `runtime_with_dom` — `grep -o "runtime_with_[a-z_]*"` over the extracted block returned
that name and nothing else, so a single `v8_runtime_with_dom` twin sufficed and no helper had to
move (~910 QuickJS callers remain, so the helper-liveness trap from `v8_core`/`v8_nav_url_storage`
could not fire); (2) the six runtime methods the bodies call besides `eval` —
`update_layout_rects`, `update_viewport_size`, `take_raf_pending`, `take_dom_dirty`,
`raf_pending_flag`, `dom_dirty_flag` — all already exist on `V8JsRuntime` with identical
signatures (`v8_runtime.rs:523-566`). 71/71 green on the first run, zero body edits, zero engine
divergences. The `grep -o` on `rt\.[a-z_]*(` over the extracted block is the two-minute
pre-flight that predicted this; run it before assuming a slice needs new plumbing.

**A microtask *coverage gap*, not a loose assertion — the S12b-2 lesson's other shape.** This
range contains no `_lumen_drain_microtasks()` calls and no "may be false if microtasks not yet
flushed" assertions, so by the letter of the scoping note there was nothing to tighten. But the
three ported `queue_microtask_*` tests assert only `typeof queueMicrotask === 'function'` (twice)
and that a non-function argument throws — none observes the callback ever running, because under
QuickJS `eval()` returned with the job queue unprocessed and the effect was unreachable from Rust
without an explicit drain. Under V8 the microtask checkpoint runs after each script, so the actual
scheduling contract is testable, and the slice adds `queue_microtask_callback_runs_after_sync_tail`
(the 72nd test): the callback must *not* run inline at the call site (first `eval` returns
`"sync"`), and must already have run by the start of the next `eval` (`"sync,micro"`). Generalized
rule for the remaining slices: the QuickJS microtask limitation left behind both weakened
assertions *and* silently missing ones — when a ported family touches async ordering, check what
the tests decline to assert, not only what they assert loosely.

**Relevant while reading `_lumen_drain_microtasks` in the ported ranges:** the V8 binding
(`v8_runtime.rs:3611`) is a deliberate no-op stub with a `TODO(v8-s3)` — the compat-layer closure
signature (JsValue-level only) cannot reach the isolate for `perform_microtask_checkpoint`. So the
85 call sites downstream can be deleted rather than reimplemented, but a test that *needs* a
forced flush at a point V8 does not choose one has no primitive available; restructure into two
`eval` calls instead, as above.

### S12b-24-childnode-traversal — sixth porting slice (2026-07-30, branch p1-v8-s12b-24-childnode-traversal)

The ChildNode/ParentNode mixin (`remove`/`before`/`after`/`replaceWith`/`prepend`/
`replaceChildren`), ElementTraversal (`childElementCount`/`firstElementChild`/`lastElementChild`/
`nextElementSibling`/`previousElementSibling`), the live `HTMLCollection` returned by `.children`
(incl. `for-in`/`Object.getOwnPropertyNames`/`hasOwnProperty` enumeration, BUG-323),
`Node.isConnected` (BUG-311), TreeWalker/NodeIterator/`NodeFilter`, `document.adoptNode`/
`importNode`, and the raw `_lumen_get_bounding_rect`/`_lumen_get_viewport_size` bindings — **27
tests** ported into `mod tests::v8_childnode_traversal`, QuickJS copies deleted. `cargo test -p
lumen-js --features v8-backend` stayed at 2570/2570 (1:1 swap, no net count change).

**The scoping table's line numbers for this row were already stale by the time this slice
started — a straight line-number lookup would have missed the target entirely.** The table (see
`S12b-24 — scoping only` above) placed this subarea at `dom.rs:21235-21656`; by 2026-07-30 the
five already-merged slices had deleted their QuickJS ranges from *earlier* in the file (while each
slice's own `mod v8_*` was appended near EOF), shifting everything below upward. The actual content
was found by grepping for the family's own names (`tree_walker`, `node_iterator`,
`element_traversal`, `parent_node`) rather than trusting the stale offset, and turned out to sit at
`dom.rs:16030-16511` instead — immediately preceding the still-unported `window.matchMedia`
section, confirming it as one contiguous, self-contained block. **Rule for the remaining ~24
slices: re-locate every subarea by content (section-header comment or representative test name),
never by the brief's line numbers once even one prior slice has merged.** Real count was 27
against the table's "~24" guess (the recurring `#[test] fn` recount-at-start caveat, again
justified).

Fully mechanical otherwise: single helper (`runtime_with_dom` → `v8_runtime_with_dom` twin),
`update_layout_rects`/`update_viewport_size` already mirrored by `V8JsRuntime`, 27/27 green on the
first run, zero body edits, zero engine divergences.

### S12b-24-matchmedia — seventh porting slice (2026-07-30, branch p1-v8-s12b-24-matchmedia)

`window.matchMedia`/`MediaQueryList` (CSS Media Queries L4 §4.2): constructor, `media`/`matches`,
legacy `addListener`/`removeListener`, `addEventListener('change', …)`/`onchange`,
`prefers-color-scheme`, `MediaQueryListEvent` — **13 tests** ported into `mod tests::v8_matchmedia`,
QuickJS copies deleted. `cargo test -p lumen-js --features v8-backend` stayed at 2570/2570 (1:1
swap). Count matched the scoping table's "13" estimate exactly — the first slice of the ~30 to do
so.

**Confirms the childnode-traversal rule, one link deeper.** Located by content
(`grep matchMedia\|MediaQueryList`), not the stale brief line numbers: the actual range at slice
start was `dom.rs:16030-16236`, sitting immediately after the just-merged ChildNode/ParentNode/
ElementTraversal block — the same adjacency the childnode-traversal slice predicted when it noted
this subarea was "immediately preceding the still-unported `window.matchMedia` section". The
scoping table's *relative order* between adjacent rows still holds even though absolute line
numbers don't; useful as a secondary check when a content grep returns more than one plausible
block.

Fully mechanical: the delivery path (`_lumen_deliver_media_changes`, `dom.rs:10558`) is pure JS
living in the shared `WEB_API_SHIM`, so viewport/color-scheme changes never needed a Rust-side
call — the only runtime methods touched are `rt.eval` and the already-mirrored
`rt.update_viewport_size`. No new plumbing, no helper migration (`runtime_with_dom` keeps its
~900+ remaining QuickJS callers), 13/13 green on the first run, zero body edits, zero engine
divergences.

### S12b-24-elem-geometry-scroll — eighth porting slice (2026-07-30, branch p1-v8-s12b-24-elem-geometry-scroll)

Element geometry API (`getBoundingClientRect`, `offsetWidth`/`offsetHeight`), scroll state
(`scrollLeft`/`scrollTop`/`scrollWidth`/`scrollHeight` via `update_scroll_states`, `scrollTo`/
`scrollBy` via `take_scroll_requests`), scroll events (element non-bubbling `fire_element_scroll`,
window-level `fire_window_scroll`), CSS Scroll Snap L2 `snapchanging`/`snapchanged`
(`fire_snap_changing`/`fire_snap_changed`) — **10 tests** ported into `mod tests::v8_elem_geometry_scroll`,
QuickJS copies deleted. `cargo test -p lumen-js --features v8-backend` stayed at 2570/2570 (1:1 swap).
The brief's "~18" estimate for this row bundled the adjacent "Lazy image loading" subarea in with
it — the actual section (found by content, `dom.rs:16030-16205`, immediately after the matchMedia
block removed by the previous slice) holds only these 10; lazy image loading is its own next slice.

**First slice needing genuinely new plumbing since S12b-24-nav-url-storage's helper move.**
`V8JsRuntime` mirrors nearly every `QuickJsRuntime` method touched by this test cluster
(`update_layout_rects`, `update_scroll_states`, `take_scroll_requests`) but was missing
`fire_element_scroll`/`fire_window_scroll`/`fire_snap_changing`/`fire_snap_changed` entirely — those
four only existed as `QuickJsRuntime` methods in `lib.rs` (dispatching a small `if (typeof
fn==='function') fn(...)` script through `ctx.eval`). Added as inherent `V8JsRuntime` methods in
`v8_runtime.rs` using the same eval-based dispatch, just through the already-public `JsRuntime::eval`
trait method instead of the rquickjs `ctx.with` closure — no new native bindings, no shim changes,
because the target JS functions (`_lumen_fire_scroll_on_element`, `_lumen_fire_window_scroll_event`,
`_lumen_fire_snap_changing`, `_lumen_fire_snap_changed`, `_lumen_make_element`) already live in the
shared `WEB_API_SHIM` and are engine-agnostic. Forgot the `#[cfg(feature = "v8-backend")]` gate on
the new `mod v8_elem_geometry_scroll` on the first pass — caught by running
`cargo clippy -p lumen-js --all-targets -- -D warnings` (no `v8-backend`) before the gate, not just
the `--features v8-backend` variant; every prior `mod v8_*` slice carries this same attribute, worth
checking explicitly rather than assuming it copy-pasted correctly. 10/10 green after the fix, zero
body edits, zero engine divergences.

### S12b-24-lazy-image-io — ninth porting slice (2026-07-30, branch p1-v8-s12b-24-lazy-image-io)

Lazy image loading (`_lumen_init_lazy_images`/`_lumen_deliver_intersection_observers`/
`_lumen_deliver_lazy_images` no-op check via `take_lazy_image_requests`) and IntersectionObserver
`rootMargin` parsing/behavior (`_parse_root_margin`, single/two/four-value CSS shorthand, root
expansion for below-viewport elements) — **11 tests** ported into `mod tests::v8_lazy_image_io`,
QuickJS copies deleted. `cargo test -p lumen-js --features v8-backend` stayed at 2570/2570 (1:1 swap).
Actual section on start — `dom.rs:16030-16227`, immediately after `runtime_with_dom` and right where
the elem-geometry-scroll slice's writeup said it would be (that slice's note about the brief bundling
this subarea in with it was confirmed).

No new plumbing: `V8JsRuntime::update_viewport_size`/`update_layout_rects`/`take_lazy_image_requests`
already existed (mirrored from `lib.rs`, unlike the `fire_*` methods the previous slice had to add),
and `_lumen_init_lazy_images`/`_lumen_deliver_intersection_observers`/`_parse_root_margin` all live in
the shared engine-agnostic `WEB_API_SHIM`. Pure mechanical port: bodies unchanged except
`runtime_with_dom` → `v8_runtime_with_dom`, `super::find_element_by_tag` → `super::super::find_element_by_tag`
(nesting one level deeper than the flat `mod tests`). 11/11 green on the first run, zero engine
divergences. `cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings` clean;
also re-checked default (no `v8-backend`) `cargo check -p lumen-js` clean, learning the gate-forgetting
lesson from the previous slice's writeup without repeating the mistake.

### S12b-24-idb — eleventh porting slice (2026-07-30, branch p1-v8-s12b-24-idb)

IndexedDB in-memory ops (open/upgradeneeded, add/get by keyPath and out-of-line autoIncrement,
put-overwrite, duplicate-key add aborts the transaction with `ConstraintError`, `getAll` with/without
`IDBKeyRange`, delete/clear, index get/getAll, unique-index violation, cursor iterate/reverse/
update/delete, `IDBKeyRange.includes`/`indexedDB.cmp`, version-downgrade error, `deleteDatabase`,
second connection sees data persisted by the first) plus IndexedDB persistence across runtime reload
(Rust-backed `IdbBackend` snapshot: reload restores without re-firing `upgradeneeded`, version is
restored, `Date` values round-trip, a deleted database restores as empty on reload, a read-only
transaction does not re-persist) — **23 tests** ported into `mod tests::v8_idb`, QuickJS copies
(including the `MockIdb` struct and `runtime_with_idb` helper) deleted. Actual section on start —
`dom.rs:16031-16558`, immediately after the fontface-shadow-custom slice's block, ending right before
`// ── FormData API tests ──`. Brief's estimate ("~34" in-memory + "8" persistence = ~42) was stale;
real count for the full IndexedDB section (both subareas together) was 23.

Confirms the open question the nav-url-storage slice's writeup flagged: constructing `V8JsRuntime`
twice in one test process, both instances sharing one backend via `Arc<Mutex<...>>`
(`idb_persists_across_runtime_reload` and friends), works with no isolate-lifecycle issues — same
verdict as `local_storage_persists_across_runtimes` already established for `WebStorage`. New helper
`v8_runtime_with_idb` mirrors `runtime_with_idb` exactly (same `install_dom` positional-argument
order; `V8JsRuntime::install_dom`'s `idb_backend: Option<Arc<dyn IdbBackend>>` parameter sits at the
same position as `QuickJsRuntime::install_dom`'s, confirmed by reading both signatures before
porting). No other plumbing needed — bodies are pure `rt.eval`, unchanged except
`runtime_with_idb`/`runtime_with_dom` → `v8_runtime_with_idb`/`v8_runtime_with_dom`. 23/23 green on
the first run, zero engine divergences. `cargo test -p lumen-js --features v8-backend` stayed at
2570/2570 (−23 QuickJS, +23 V8). `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` and the default-feature `cargo clippy -p lumen-js --all-targets -- -D warnings` both
clean.

### S12b-24-formdata — twelfth porting slice (2026-07-30, branch p1-v8-s12b-24-formdata)

FormData API (append/get/getAll/has/delete/set, keys/values/entries iterators, `forEach`,
`Symbol.iterator`, `_toUrlEncoded`/`_toMultipart` percent-encoding and quote-escaping) plus fetch
body-encoding (`fetch()` POST with a `FormData` body → `multipart/form-data` with a boundary, a
string body → `text/plain`, a `Uint8Array` body → `application/octet-stream`, `Content-Type`
header override) plus `AbortController`/`AbortSignal.timeout` around fetch (the
`_lumen_fetch_cancellable`/`_lumen_fetch_cancellable_with_body` bridge, an in-flight abort during
an async fetch) — **30 tests** ported into `mod tests::v8_formdata`. Actual section on start —
`dom.rs:16030-16450`, immediately after the idb slice's block, ending right before
`// ── Selection API tests ──`. Brief's estimate ("~22") was stale; real count was 30.

One divergence from the mechanical pattern of prior slices: `CaptureFetch`/`FetchCall`/
`runtime_with_fetch` were **not** deleted from the QuickJS region — five still-unported tests
further down the file share that same mock (`send_beacon_with_provider_returns_true`,
`fetch_keepalive_with_provider_fires_request`, `fetch_priority_high_and_low_accepted`,
`fetch_priority_invalid_normalizes_to_auto`, `fetch_response_body_getreader_yields_correct_bytes`
— sendBeacon/fetch-keepalive-priority/WHATWG-Streams areas, none scoped to this slice), so removing
them would have broken those tests' compile. They were kept in place next to `runtime_with_dom`;
`mod v8_formdata` defines its own `CaptureFetch`/`FetchCall`/`v8_runtime_with_fetch` copy, so the
two engines' mocks never mix. `AbortFetch`/`BlockingFetch`/`runtime_with_abort_fetch`/
`runtime_with_blocking_fetch`, by contrast, had no other callers and were deleted normally.
Lesson for future slices: grep a helper's usage across the **whole file**, not just the section
being ported, before deleting it — a shared mock can outlive the section that first defined it.

`V8JsRuntime::install_dom`'s `fetch_provider` parameter already sat at the same position as
`QuickJsRuntime::install_dom`'s — no signature plumbing needed. The one real omission was
forgetting `#[cfg(feature = "v8-backend")]` on the new `mod v8_formdata` itself: without it the
default (QuickJS-only) build failed with `unresolved import crate::v8_runtime` (that module is
itself gated behind the same feature in `lib.rs`) — caught immediately by the default-feature
clippy pass. 30/30 green on the first run, bodies unmodified, zero engine divergences.
`cargo test -p lumen-js --features v8-backend` — 2570/2570 (−30 QuickJS, +30 V8); default-feature
`cargo test -p lumen-js` — 1826/1826 unaffected. Both clippy passes clean.

### S12b-24-selection-range-editing — thirteenth porting slice (2026-07-30, branch p1-v8-s12b-24-selection-range-editing)

Selection API (`window.getSelection()`/`document.getSelection()`, `type`/`rangeCount`/
`isCollapsed`/`toString()`/`removeAllRanges`/`getRangeAt`/`collapseToStart`), Range
(`document.createRange()`, `collapse`/`cloneRange`/`selectNodeContents`/
`compareBoundaryPoints`, `window.Range`), `execCommand` (`bold`/`italic`/unknown/`copy`,
`queryCommandEnabled`/`State`/`Value`/`Supported`, `insertText`, `delete`), and
`contentEditable`/`isContentEditable` including the `_lumen_handle_contenteditable_key`
insert/deleteBackward/deleteForward dispatch plus its `beforeinput`-cancellation and
`input`-event paths — **42 tests** ported into `mod tests::v8_selection_range_editing`. Actual
section on start — `dom.rs:16060-16606`, immediately after the formdata slice's block, ending
right before `// ── window.getComputedStyle() tests ──`. Brief's estimate ("~68") was stale
along with its line range; real count was 42.

One non-obvious finding: `bool_eval`, defined at the top of the block being removed, turned out
to be a shared QuickJS helper — still-unported sections further down the file (details/dialog,
`window.open`, and others) call it too. Deleting it wholesale broke ~111 downstream call sites
across the crate (the compiler even pointed at an unrelated same-named helper in
`filesystem_access.rs` as a decoy). Fix: restored the QuickJS `bool_eval` next to
`runtime_with_dom`/`runtime_with_fetch`, and gave `mod v8_selection_range_editing` its own
`V8JsRuntime`-typed copy — the two never mix. Lesson for future slices, sharper than the
formdata-slice mock lesson: grep a **helper function itself**, not just doc-fixture builders,
across the whole file before deleting it — some helpers are load-bearing for sections far outside
the one being ported. `make_selection_doc`/`make_contenteditable_doc` had no such external
callers and moved cleanly. No other plumbing was needed: the only fixture builder was
`runtime_with_dom` (twin `v8_runtime_with_dom`), and every test body was already
`rt.eval(...)` plus direct `Document`/`NodeData` inspection through the shared
`Arc<Mutex<Document>>`, none of it engine-specific. 42/42 green on the first run, bodies
unmodified, zero engine divergences. `cargo test -p lumen-js --features v8-backend` —
2570/2570 (unchanged: −42 ungated QuickJS, +42 gated V8 — a like-for-like swap under this
feature); default-feature `cargo test -p lumen-js` — 1784/1784 (was 1826, −42, since the V8
module is feature-gated out). Both clippy passes clean.

### S12b-24-computedstyle — fourteenth porting slice (2026-07-30, branch p1-v8-s12b-24-computedstyle)

`window.getComputedStyle()`: `getPropertyValue`, camelCase property access (`fontSize` →
`font-size`), the pseudo-element argument (accepted but ignored — not yet supported),
`null`-element lookups, and value replacement across repeated `update_computed_styles`
calls — **16 tests** ported into `mod tests::v8_computedstyle`. Actual section on start —
`dom.rs:16065-16292`, immediately after the selection-range-editing slice's block, ending
right before `// ─── Web Crypto API tests ───`. Brief's estimate ("~16") was accurate this time.

Fully mechanical: the only fixture builder was `runtime_with_dom` (twin `v8_runtime_with_dom`),
and `V8JsRuntime::update_computed_styles` already mirrors the QuickJS signature
(`HashMap<u32, HashMap<String, String>>`) one-for-one. `make_computed_styles_map`/
`get_main_nid` were both scoped to this section only (grepped across the file before deleting,
per the selection-range-editing lesson) — no downstream callers, so the QuickJS copies were
deleted outright rather than kept as shared helpers. 16/16 green on the first run, bodies
unmodified, zero engine divergences. `cargo test -p lumen-js --features v8-backend` —
2570/2570 (unchanged: −16 ungated QuickJS, +16 gated V8); default-feature
`cargo test -p lumen-js` — 1768/1768 (was 1784, −16). Both clippy passes clean.

### S12b-24-webcrypto — fifteenth porting slice (2026-07-30, branch p1-v8-s12b-24-webcrypto)

Web Crypto API (`crypto.getRandomValues`/`randomUUID`, `crypto.subtle.digest`) plus SubtleCrypto
full API (`generateKey`/`importKey`/`sign`/`encrypt`/`decrypt`/`deriveBits`/`deriveKey` across
HMAC/ECDSA/AES-GCM/AES-CBC/PBKDF2/HKDF) — **18 tests** ported into `mod tests::v8_webcrypto`,
QuickJS copies deleted. Actual section on start — `dom.rs:16065-16462`, immediately after the
just-merged `getComputedStyle()` block and ending right before `url_can_parse_static_method`
(not part of this subarea, left untouched — apparently added to the file after the original
scoping audit and physically adjacent by coincidence, the same "stale line numbers, re-locate by
content" pattern the childnode-traversal slice first flagged). Real count (18) landed far under
the scoping table's combined guess for "Web Crypto + SubtleCrypto" (~40) — the table's estimate
was for the whole row before the file grew past it; treat every remaining `~N` guess as a rough
prior, not a target.

**First plumbing requirement since the fourth slice, and the reason: a literal `TODO` stub.**
`v8_runtime.rs::install_dom` carried `// TODO(v8-s3, out of scope): SubtleCrypto install is
rquickjs-ctx-based (crate::subtle_crypto) — separate future slice.` where the 14 `_lumen_subtle_*`
natives (`generateKey`/`importKey`/`exportKey`/`exportKey_or_err`/`sign`/`verify`/`encrypt`/
`decrypt`/`keyInfo`/`aesCbcEncrypt`/`aesCbcDecrypt`/`aesCtrCrypt`/`deriveBits`/`rsaOaepEncrypt`/
`rsaOaepDecrypt`) should have been. Without them `window.crypto.subtle.generateKey(...)` etc.
threw `ReferenceError` on the default engine — a second, larger instance of the BUG-447 shape
(a whole native surface silently absent on V8), except this one was already flagged rather than
lurking. The port was wrapper-only: every function `crate::subtle_crypto::install_subtle_bindings`
calls (`generate_key`, `sign_data`, `aes_gcm_encrypt`, `aes_cbc_encrypt`, `derive_bits`, …) is
`pub(crate)`, takes only primitives/`Vec<u8>`/`String`, and never touches `rquickjs::Ctx` — the
key registry (`CRYPTO_KEYS`) is a plain `thread_local!`, engine-agnostic by construction. Added a
matching `reg!` block to `V8JsRuntime::install_dom` that calls the same functions directly; the
`reg!` macro's existing arity-5 arm covered `importKey`'s five params without changes. Confirms
the scoping note's "very likely portable verbatim" prediction for provider-trait helpers extends
to native-binding installers too, as long as the underlying logic never captured `Ctx`.

**The `_lumen_drain_microtasks`-adjacent lesson, now at full scale.** Every one of the 18 QuickJS
originals used the loose-assertion shape the scoping note predicted for this cluster: single-eval
setup + a second `eval()` read, matched against either the resolved value or `Null`/pending with a
`// microtasks may not have flushed` comment (some tests even burned a third, discarded `eval()`
call as an extra "pump" — QuickJS's `eval()` never drains the queue no matter how many times you
call it, so the extra call was cargo-culted, not functional). Removed entirely: V8's default
`kAuto` microtasks policy auto-checkpoints after each top-level script, the same fact
`v8_perf_observers`'s `queue_microtask_callback_runs_after_sync_tail` established — so the second
`eval()` in every two-step test now deterministically observes the fully-resolved promise chain
(digest results, HMAC signature lengths, AES-GCM/CBC roundtrip plaintext, PBKDF2/HKDF derived
lengths), and the redundant discarded reads were dropped along with the tolerant branches. One
test (`crypto_subtle_digest_sha256_known_vector`) was *not* tightened: it reads the completion
value as the very last statement of its *own* setup script, before that script's own end-of-eval
checkpoint runs — so it observes pre-resolution state on both engines by construction (proves
"not rejected synchronously", nothing more); left as `assert_eq!(r, JsValue::Null)` with a comment
explaining why, rather than mistaking the tolerant shape for one this slice's rule covers.

18/18 green on the first run after the plumbing landed. `cargo test -p lumen-js --features
v8-backend` — 2570/2570 (unchanged: −18 ungated QuickJS, +18 gated V8); default-feature
`cargo test -p lumen-js` — 1750/1750 (was 1768, −18). Both clippy passes (with and without
`v8-backend`) clean.

### S12b-24-url-abort-clone-blob — sixteenth porting slice (2026-07-30, branch p1-v8-s12b-24-url-abort-clone-blob)

URL static methods (`canParse`/`parse`), `AbortSignal`/`AbortController` (incl. `.any`/
`.timeout`), fetch abort rejection, `structuredClone` (primitives/objects/arrays/Map/Set/
RegExp/circular+shared refs/ArrayBuffer/typed arrays/DataCloneError cases/BigInt), `btoa`/
`atob`, `Blob`, `File`, `FileReader` — **43 tests** ported into `mod tests::v8_url_abort_clone_blob`,
QuickJS copies deleted. Actual section on start — `dom.rs:16065-16727`, immediately after the
webcrypto slice's block and ending right before `// ─── Page Visibility API tests`; the scoping
table's original three rows for this range ("Trusted Types (two sections)", "structuredClone",
"btoa/atob, Blob, File, FileReader") had grown a preceding, unlabeled cluster (URL static methods
+ AbortSignal + fetch-abort, ~8 tests) not in the 2026-07-29 audit — same "file grew, re-locate by
content" pattern every slice since childnode-traversal has hit.

**Trusted Types dropped from this slice — scope correction, not a deferral of convenience.**
Porting the 8 Trusted Types tests hit `Runtime("trustedTypes is not defined")` on every one:
`crate::trusted_types::install_trusted_types_bindings` is called from `QuickJsRuntime::install_dom`
(`dom.rs:3116`) but has no V8 counterpart — `v8_runtime.rs:3856` already carries
`// TODO(v8-s3, out of scope): Trusted Types install is rquickjs-ctx-based (crate::trusted_types)
— separate future slice`, a pre-existing, deliberate scope boundary from the S12b-3 module sweep,
not something this slice's scoping missed. Un-ported the 8 tests back onto `QuickJsRuntime`
(restored in place, same names, right after `bool_eval`) rather than deleting or silently
weakening them — the V8 gap is real: `window.trustedTypes` is `undefined` on the default engine
today. A second, un-ported Trusted Types cluster (`trusted_types_is_defined` and friends,
`dom.rs:~19740`, part of the still-unlabeled "CSS.supports/escape + Trusted Types #2 + Storage
Access + EventTarget" catch-all row) has the same dependency and should move together with this
one when that follow-up slice is scheduled. **The fix itself looks trivial, worth flagging for
whoever picks it up**: `crate::trusted_types::TRUSTED_TYPES_SHIM` is a plain JS string evaluated
via `ctx.eval::<(), _>(...)` — no `rquickjs`-specific type touches it — so wiring it into
`V8JsRuntime::install_dom` (the same way `WEB_API_SHIM` already is, `v8_runtime.rs:3991-4005`)
should not require touching `trusted_types.rs` at all, only the V8 install path. Not attempted
here to keep this slice scoped to test-porting mechanics, not new-feature plumbing.

**Two more loose-assertion tightenings beyond `fetch_rejects_on_aborted_signal`** (which follows
the by-now-standard two-`eval()` split): `blob_text_promise`/`blob_array_buffer_promise` dropped
their `Null`-tolerant `match` for a direct `assert_eq!` against the resolved `String`/`Number`, and
`file_reader_read_as_data_url` dropped its `if let ... else { /* acceptable */ }` no-op branch for
an `assert!`-backed `match` that panics on anything but the resolved data URL. All three are exactly
the "at least 8 more tests accept either the resolved value or Null/false" shape the original
S12b-24 scoping note flagged for the SubtleCrypto/Streams/Compression clusters — confirms it also
reaches Blob/FileReader, not just those three.

43/43 green on the first run once Trusted Types was carved out. `cargo test -p lumen-js --features
v8-backend` — 2570/2570 (unchanged: −43 ungated QuickJS, +43 gated V8; the 8 Trusted Types tests
stayed QuickJS-only throughout, no net change there either); default-feature `cargo test -p
lumen-js` — 1707/1707 (was 1750, −43). Both clippy passes (with and without `v8-backend`) clean.

### S12b-24-page-visibility-beacon — seventeenth porting slice (2026-07-30, branch p1-v8-s12b-24-page-visibility-beacon)

Three adjacent scoping-table rows ported together into `mod tests::v8_page_visibility_beacon`:
Page Visibility API (`document.visibilityState`/`.hidden`, `visibilitychange`, the PH1-15
pause/unpause cluster driven by `set_document_visibility`), `document.readyState` + lifecycle
(`readystatechange`/`DOMContentLoaded`/`load`, forward-only transitions), and the unlabeled
"sendBeacon + fetch keepalive/priority + `URL.createObjectURL`" row (FF-5 fetch priority hints,
Beacon §4, `URL.createObjectURL`/`revokeObjectURL`) — **26 tests**, QuickJS copies deleted. Actual
range on start — `dom.rs:16190-16498`, immediately after the just-merged url-abort-clone-blob block
and ending right before `// ─── Event class hierarchy tests`, exactly where the scoping table
(and the url-abort-clone-blob slice's own note) predicted it.

**Second slice needing genuinely new plumbing since S12b-24-webcrypto.** `QuickJsRuntime` exposes
`set_document_visibility(hidden: bool)` (`lib.rs:1863`) — a thin wrapper that `eval()`s
`_lumen_apply_visibility(bool)`, itself plain JS already living in the shared `WEB_API_SHIM`
(`dom.rs:13248`) — but `V8JsRuntime` had no equivalent method at all, so the 6-test T1
pause/unpause cluster (`set_document_visibility_*`) could not compile against it. Added the mirror
method on `V8JsRuntime` next to `update_computed_styles`/`fire_element_scroll` (`v8_runtime.rs:~656`),
same one-line-`eval` shape, `Mirrors [`crate::QuickJsRuntime::set_document_visibility`]` doc
convention. `_lumen_apply_ready_state` (used directly via `rt.eval(...)` by every `ready_state_*`
test, no Rust-side wrapper) was already plain JS in the shim, needing nothing.

**`CaptureFetch`/`runtime_with_fetch` liveness check, by now routine.** The QuickJS mock fetch
provider at `dom.rs:16034`/`16053` carries a comment flagging it as still used by
"not-yet-ported sendBeacon/fetch-keepalive/fetch-priority/Streams tests" — this slice took the
sendBeacon/keepalive/priority share of that but one Streams test (`dom.rs:~16973` post-slice, the
fetch-streaming-body family, still unported) keeps a live call, so the QuickJS helper stayed in
place; the V8 module got its own private `CaptureFetch`/`v8_runtime_with_fetch` copy, following the
per-module self-containment pattern every prior slice (`v8_formdata`, `v8_url_abort_clone_blob`,
…) already established rather than sharing across `mod v8_*` boundaries.

Zero engine divergences and zero loose-assertion tightenings needed — this range has no
`_lumen_drain_microtasks` calls and no promise bodies read across two `eval()`s; every assertion in
the block was already a plain synchronous read. 26/26 green on the first run, test bodies otherwise
untouched. `cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −26 ungated
QuickJS, +26 gated V8); default-feature `cargo test -p lumen-js` — 1681/1681. Both clippy passes
(with and without `v8-backend`) clean.

### S12b-24-event-classes — eighteenth porting slice (2026-07-30, branch p1-v8-s12b-24-event-classes)

The "Event class hierarchy" row, ported whole into `mod tests::v8_event_classes`: constructors +
`instanceof` chains + field checks for `UIEvent`/`MouseEvent`/`KeyboardEvent`/`InputEvent`/
`FocusEvent`/`WheelEvent`/`PointerEvent`/`AnimationEvent`/`TransitionEvent`/`StorageEvent`/
`PopStateEvent`/`HashChangeEvent`/`ErrorEvent`/`SubmitEvent`/`CompositionEvent`, the
`_lumen_dispatch_mouse_event`/`_lumen_dispatch_key_event`/`_lumen_dispatch_pointer_event`/
`_lumen_dispatch_submit_event` dispatch natives (coordinates, modifier bitmask, bubbling vs
non-bubbling delivery, BUG-437 submit-event `preventDefault`/submitter contract), and the
`window.<EventClass>` existence roll-call — **31 tests** (matched the scoping table's "~31" guess
exactly), QuickJS copies deleted. Actual range on start — `dom.rs:16190-16594`, immediately after
the just-merged page-visibility-beacon block and ending right before
`// ─── WHATWG Streams API tests`, exactly where that slice's own note predicted.

**No plumbing needed — every `_lumen_dispatch_*` native is plain JS in `WEB_API_SHIM`, not a
Rust-side binding.** Unlike the geometry/scroll slice (which had to add `fire_*` methods to
`V8JsRuntime`) or the page-visibility slice (`set_document_visibility`), this range's only runtime
touchpoint is `rt.eval()` through the shared `runtime_with_dom` helper (twin `v8_runtime_with_dom`,
copied verbatim from the prior slice). All four dispatch functions and every `Event` subclass
constructor already live in the engine-agnostic shim string, so the port was a pure
`runtime_with_dom` → `v8_runtime_with_dom` rename with zero body edits.

Zero engine divergences, zero loose-assertion tightenings — no `_lumen_drain_microtasks` calls, no
promise bodies read across two `eval()`s, nothing async in this range at all. 31/31 green on the
first run. `cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −31 ungated
QuickJS, +31 gated V8); default-feature `cargo test -p lumen-js` — 1650/1650 (was 1681, −31). Both
clippy passes (with and without `v8-backend`) clean.

### S12b-24-whatwg-streams — nineteenth porting slice (2026-07-30, branch p1-v8-s12b-24-whatwg-streams)

The "WHATWG Streams API" row, ported whole into `mod tests::v8_whatwg_streams`:
`ReadableStream`/`WritableStream`/`TransformStream` constructors, `getReader`/`getWriter`/
`locked`/`releaseLock`, `tee()`, `pipeTo`/`pipeThrough`, a custom transformer, `Blob.stream()`,
`Response.body`/`bodyUsed` plus the K-3 fetch-streaming-body cluster, `TextDecoderStream`/
`TextEncoderStream` and `TextDecoder`'s `{stream: true}` partial-multibyte-UTF-8 buffering,
`ByteLengthQueuingStrategy`/`CountQueuingStrategy`, and `ReadableStream.from()` — **42 tests**
(matched the scoping table's "~42" guess exactly), QuickJS copies deleted. Actual range on start —
`dom.rs:16190-16730`, immediately after the Trusted Types block (still QuickJS-only, unrelated to
this slice) and ending right before `// ── <details>/<summary> + <dialog> tests`.

**Real plumbing question was promise timing, not Rust.** 16 of the 42 tests ported literally were
red on first run: each read a value set inside a `.then()` callback within the *same* `eval()`
call that scheduled it. That contract was already established by `v8_perf_observers`'s
`queue_microtask_callback_runs_after_sync_tail` test (a microtask never runs before its scheduling
script returns, matching every JS engine's real semantics — V8 is not special here, the QuickJS
originals only "worked" because `_lumen_drain_microtasks()` was a real forced-drain on that engine,
while on V8 it's a documented no-op, `v8_runtime.rs:3672`). Fixed with the same two-`eval()` split
pattern used by `v8_webcrypto`/`v8_url_abort_clone_blob`: a setup `eval()` that schedules the
`.then()`, then a second `eval()` that reads the now-settled state (V8's Auto microtask policy
drains the queue to a fixpoint before returning control to the embedder, so a chained
`.then().then()` — `response_body_reader_done_after_all_chunks` — and a two-step
`TextDecoderStream` test needing a mid-sequence `eval()` between two `writer.write()` calls both
resolve correctly across the split). No engine-level divergence — this is a test-authoring
artifact of the QuickJS suite leaning on a QuickJS-only forced-drain primitive, not a product bug.

Second cleanup: `CaptureFetch`/`runtime_with_fetch` (`dom.rs:16030-16058`, the QuickJS fetch mock)
were deleted outright rather than kept dead. The formdata slice's note said they were "still used
by the not-yet-ported sendBeacon/fetch-keepalive/fetch-priority/Streams tests" — but sendBeacon and
fetch-keepalive/priority were ported away in `S12b-24-page-visibility-beacon`, leaving only
`fetch_response_body_getreader_yields_correct_bytes` (this slice) as the sole remaining caller. The
V8 twin (`v8_runtime_with_fetch`/`CaptureFetch` inside `mod v8_whatwg_streams`) follows the same
copy-verbatim pattern as the other five per-module fetch-mock twins already in the file.

42/42 green after the eval-split fixes. `cargo test -p lumen-js --features v8-backend` —
2570/2570 (unchanged: −42 ungated QuickJS, +42 gated V8); default-feature `cargo test -p lumen-js`
— 1608/1608 (was 1650, −42). Both clippy passes (with and without `v8-backend`) clean.

### S12b-24-details-dialog-popover — twentieth porting slice (2026-07-30, branch p1-v8-s12b-24-details-dialog-popover)

`<toggleAttribute>` + `<details>`/`<summary>` + `<dialog>` (including focus management, HTML LS
§6.6.3) + `<selectlist>` (Open UI Customizable Select §3) + HTML Popover API (WHATWG HTML §6.12,
including `popover=hint`, Popover API L2) — 43 tests ported to `mod v8_details_dialog_popover`,
QuickJS copies deleted. This slice sat right after the still-blocked Trusted Types tests (see the
`S12b-24 — scoping only` comment left at `dom.rs:16030-16040`: Trusted Types needs
`crate::trusted_types::install_trusted_types_bindings` wired into `V8JsRuntime::install_dom` first,
out of scope here — left un-ported, comment now trimmed to just "window.open/etc." since
details/dialog is no longer in that list) — the table's line-range estimates for this row were
already stale by the time this slice started (prior slices' deletions had shifted everything below
them up by thousands of lines), confirmed by grep for the `fn.*details|dialog|selectlist|popover`
test names rather than trusting the recorded `26687-27278` range.

All 43 tests are synchronous (no `.then()`/microtask timing anywhere in this cluster) — first
slice since `S12b-24-matchmedia` with zero `_lumen_drain_microtasks`-pattern breakage, so it ported
as a literal copy: `bool_eval`/`runtime_with_dom` renamed to their `V8JsRuntime` twins, test bodies
(including the raw `_lumen_dispatch_bubble`/`_lumen_dispatch_key_event`/`_lumen_dispatch_mouse_event`
calls and the `_lumen_root_nid`/`_lumen_last_focused_nid` global reads, all engine-agnostic
`WEB_API_SHIM` surface) unchanged. `take_focus_requests()` exists identically on both
`QuickJsRuntime` and `V8JsRuntime` (`lib.rs:1805` / `v8_runtime.rs:711`), so the dialog focus-request
tests also ported without adaptation.

Four `toggle_attribute_*` tests were folded into this slice rather than left as an orphaned
one-off: they sat immediately adjacent (between the Trusted Types block and `make_details_doc`),
were never listed in their own scoping-table row, and were too small to justify a separate slice.

`cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −43 ungated QuickJS, +43
gated V8); default-feature `cargo test -p lumen-js` — 1565/1565 (was 1608, −43). Both clippy passes
(with and without `v8-backend`) clean.

### S12b-24-form-constraint-validation — twenty-first porting slice (2026-07-30, branch p1-v8-s12b-24-form-constraint-validation)

Form Constraint Validation API (`ValidityState`, `validity.{valueMissing,typeMismatch,
patternMismatch,tooLong,tooShort,rangeUnderflow,rangeOverflow,stepMismatch,customError,valid}`,
`checkValidity()`/`reportValidity()` + `invalid` event, `setCustomValidity()`/`validationMessage`,
`form.elements`/`noValidate`) plus the tests that shared its header block: `input.value`/`type`
reflection, the BUG-436 `_lumen_set_field_value` value-shadow sync regression test, and
`HTMLInputElement.showPicker()` — 32 tests (brief estimated ~31) ported to
`mod v8_form_constraint_validation`, QuickJS copies deleted.

Same line-drift finding as the two prior slices: the table's recorded `27279-27607` range was
stale by the time this slice started. Located the block by grepping for `make_form_doc`/
`checkValidity`/`ValidityState` instead — it sat at `dom.rs:16160-16459`, immediately after the
still-blocked Trusted Types section (see `S12b-24-details-dialog-popover` above) and immediately
before `// ── document.caretPositionFromPoint tests`, i.e. right before where
`S12b-24-details-dialog-popover` itself started. All ported slices so far have landed in original
file order once each area's tests are located by content grep rather than by trusting recorded
line numbers.

All 32 tests are synchronous (no `.then()`/microtask timing, no `_lumen_drain_microtasks` calls in
this cluster) — ported as a literal copy: `bool_eval`/`runtime_with_dom` renamed to their
`V8JsRuntime` twins (`v8_runtime_with_dom`), test bodies and the shared `make_form_doc` fixture
unchanged. 32/32 green on the first run, no test bodies needed adjustment.

`cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −32 ungated QuickJS, +32
gated V8); default-feature `cargo test -p lumen-js` — 1533/1533 (was 1565, −32). Both clippy passes
(with and without `v8-backend`) clean.

---

### S12b-24-idle-message-clipboard — twenty-second porting slice (2026-07-30, branch p1-v8-s12b-24-idle-message-clipboard)

`requestIdleCallback`/`cancelIdleCallback`, `MessageChannel`/`MessagePort` (construction,
`postMessage` delivery via `onmessage` and `addEventListener`, structured-clone deep copy,
`close()`/`removeEventListener` suppressing further delivery), `navigator.clipboard`
(`readText`/`writeText` stub), `navigator.permissions.query` (clipboard-read granted, camera
denied, bad descriptor rejects), `window.isSecureContext`/`crossOriginIsolated` — 25 tests ported
to `mod v8_idle_message_clipboard`, QuickJS copies deleted.

Same line-drift finding as every prior slice: located the block by grepping for
`requestIdleCallback`/`MessageChannel`/`navigator.clipboard`/`isSecureContext` rather than trusting
recorded line numbers — it sat at `dom.rs:16188-16429`, immediately after the
`S12b-24-form-constraint-validation` block and immediately before `// ── Web Worker tests`.

Unlike the previous (fully synchronous) slice, 9 of the 25 tests use `.then()`/`.catch()` on
`MessagePort`/`navigator.clipboard`/`navigator.permissions` promises and tripped the by-now-familiar
S12b-2 promise-timing lesson on the first literal-copy run: `_lumen_drain_microtasks()` is a no-op
on V8 (`v8_runtime.rs:3611`) and the automatic microtask checkpoint runs at the end of the *previous*
`eval()` call, not mid-script — so a same-eval read of a `.then()`-assigned variable observes
pre-resolution state. Fixed by splitting each into two `rt.eval()` calls: one that sets up the
promise/message chain (assigning to a script-global var, no `_lumen_drain_microtasks()` call), a
second that reads the var — the same two-step shape used in `v8_webcrypto`/`v8_whatwg_streams`.
16/25 passed unmodified on the first run (bodies with no promise involved, or where the assertion
only needs `count === 0` which holds regardless of delivery timing).

`cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −25 ungated QuickJS, +25
gated V8); default-feature `cargo test -p lumen-js` — 1508/1508 (was 1533, −25). Both clippy passes
(with and without `v8-backend`) clean.

### S12b-24-webworker — twenty-third porting slice (2026-07-30, branch p1-v8-s12b-24-webworker)

`Worker` class existence (bare + `window.Worker`), constructor returning an instance from a
`data:` URL, `postMessage`/`terminate`/`addEventListener` presence, `onmessage` default/getter/
setter, `terminate()` not throwing, and four `pump_workers()`-driven message-roundtrip tests
(plain, `addEventListener('message', …)`, base64 `data:` URL script, `Blob`-backed `URL.
createObjectURL` script) — 13 tests ported to `mod v8_webworker`, QuickJS copies deleted. Found by
grepping for `Web Worker tests`, not trusting recorded line numbers — the section sat at
`dom.rs:16189-16343`, immediately after `S12b-24-idle-message-clipboard`'s block and immediately
before `// ── _lumen_gc_collect tests`. Original scoping table estimated ~18 tests for this area;
actual count was 13 (same recount-at-slice-start drift as every prior slice).

Ported without modification beyond `runtime_with_dom` → `v8_runtime_with_dom`: no `.then()`/
`.catch()` anywhere in this section (the roundtrip tests use a real worker thread plus an explicit
`rt.pump_workers()` call after a `std::thread::sleep`, not a JS promise), so the S12b-2
promise-timing lesson didn't apply here — `V8JsRuntime` already carries full `Worker` support
(`workers`/`worker_messages`/`pump_workers()`) from the S10 hand-port
(`v8_runtime.rs:338-458`), confirmed before starting. First slice since `S12b-24-matchmedia` to
port 13/13 tests verbatim with zero behavioral fixups. One process mistake caught before commit:
forgot the `#[cfg(feature = "v8-backend")]` gate on the new `mod v8_webworker` on the first pass —
without it the module compiles unconditionally and both `cargo test` variants under- and
over-count; added the gate (matching all 22 prior `mod v8_*` blocks) and re-ran both suites to
confirm the expected ∓13 shift before treating the slice as done.

`cargo test -p lumen-js --features v8-backend` — 2570/2570 (unchanged: −13 ungated QuickJS, +13
gated V8); default-feature `cargo test -p lumen-js` — 1495/1495 (was 1508, −13). Both clippy passes
(with and without `v8-backend`) clean.

Note for the next session: `S12b-24-window-anim-compress` (24th slice, ROADMAP.md, merged
2026-07-30 — caretPositionFromPoint/gc_collect/deterministic-mode/window.open/Web Animations/
CompressionStream, 52 tests) landed without its own entry here; this file's slice numbering below
resumes at 25 to match the ROADMAP.md progress count, not the count of entries in this file.

### S12b-24-fullscreen-locks — twenty-fifth porting slice (2026-07-30, branch p1-v8-s12b-24-fullscreen-locks)

Fullscreen API (WHATWG Fullscreen §4) + Web Locks API + Screen Wake Lock stub + Network
Information stub + `navigator.userActivation` + Web Share API stub + `window.reportError()` —
seven adjacent scoping-table sub-families (`Fullscreen API` and `Web Locks + Wake Lock + Network
Information + userActivation + Web Share + reportError`) — **32 tests** (brief estimated ~36
combined) ported to `mod v8_fullscreen_locks`, QuickJS copies deleted. All API surface here is
plain JS in the shared `WEB_API_SHIM`, not native bindings — no `V8JsRuntime` changes needed.

This is the first slice into the single ~209-test tail block flagged in the original scoping note
(`dom.rs:15982` `mod tests` no longer has any nested `mod v8_*` boundary until `mod v8_core`, i.e.
every still-unported family from here down is one contiguous run, not scattered — confirmed by
counting `^    fn ` at 4-space indent between the `runtime_with_dom` helper and `mod v8_core`: 209,
matching the STATUS-P1/ROADMAP "~209 remaining" estimate exactly). Sat immediately after the
still-blocked Trusted Types block (first section, 8 tests, `dom.rs:16038-16157` — left in place,
see the `S12b-24 — scoping only` comment right above it) and immediately before `// ──
CSS.supports() / CSS.escape()`.

Heaviest use yet of the S12b-2 `_lumen_drain_microtasks()`-removal lesson on Promise-heavy code
(11 of 32 tests touch `navigator.locks`/`navigator.wakeLock`/`navigator.share` promise chains,
including nested `.then()`s and a `steal_option_grants_immediately` test with a `new Promise`
executor calling back into `navigator.locks.request` before settling). Two removal shapes, not
one: (1) tests that already called `rt.eval("_lumen_drain_microtasks()")` as its own separate
Rust-level statement between a setup `eval()` and an assertion `eval()` — here the drain line is
simply deleted, since V8's default `MicrotasksPolicy::kAuto` already performed a full checkpoint
at the end of the *setup* `eval()`'s `Script::Run` (`v8_runtime.rs:4504-4529`, confirmed by reading
the impl — no explicit checkpoint call exists or is needed). (2) the four `request_fullscreen_*`/
`exit_fullscreen_*` tests concatenated setup + drain + assertion into *one* JS string via `\`
line-continuations — these needed actual restructuring into two separate `rt.eval()` Rust calls
(drop the drain statement, split the string at that point), because V8's checkpoint only runs at
the `eval()`/`Script::Run` boundary, not between statements inside one script. Getting shape (1)
right for the Locks tests (just delete the middle line, no restructuring) depended on recognizing
they were *already* three separate Rust calls, unlike the fullscreen tests being one call each —
worth checking call-count, not just presence of the drain string, before deciding how to edit each
test.

32/32 passed on the first run, no assertion or timing fixups needed. `cargo test -p lumen-js
--features v8-backend` — 2570/2570 (unchanged: −32 ungated QuickJS, +32 gated V8); default-feature
`cargo test -p lumen-js` — 1411/1411 (down from the 1495 recorded after `S12b-24-webworker`;
delta includes both this slice's −32 and drift from sessions in between that didn't log their
default-feature count here — not re-derived, out of scope for this slice). Both clippy passes
(with and without `v8-backend`) clean.

---

### S12b-24-css-storage-nav-misc — twenty-sixth porting slice (2026-07-30, branch p1-v8-s12b-24-css-storage-nav-misc)

First slice into the flat ~209-test tail block flagged by `S12b-24-fullscreen-locks` (no nested
`mod v8_*` boundary until `mod v8_core`). The single QuickJS-side header comment covering this
chunk — `// ── CSS.supports() / CSS.escape() ──` — turned out to be stale: it labelled a
grab-bag of six unrelated feature clusters that had accreted under it over time with no further
headers, not just CSS.supports/escape. Ported all six, minus the one still-blocked cluster mixed
in among them:

- CSS.supports() / CSS.escape() (12 tests)
- Storage Access API (`document.requestStorageAccess`/`hasStorageAccess`/etc., 4 tests)
- `EventTarget` global + dependent-API installation (3 tests) — this is the
  `event_target_dependent_apis_installed` cluster flagged in the original `S12b-24 — scoping
  only` note (originally at `:29308`, drifted down to `:16811` by the time this slice started as
  earlier slices removed content above it) for splitting into 5-6 tests; split the combined
  assert (`navigator.hid`/`usb`/`bluetooth`/`serial`/`xr`/`window.navigation` all `typeof ===
  'object'` in one `&&` chain) into 6 standalone tests, one per API, for per-API failure
  attribution — kept the two adjacent non-combined `EventTarget` tests (constructibility, dispatch)
  as single tests since they weren't part of the flagged combined assertion
- Navigation API `entries()`/`currentEntry`/`canGoBack`/`canGoForward` + History-fallback
  (`history.go`/`history.length`) (5 tests) — uses `rt.take_nav_updates()`/
  `rt.take_history_traversals()` and the unqualified `NavAction` enum; both already proven
  reachable from a nested `mod v8_*` via `use super::*` (`NavigateRequest` in
  `S12b-24-nav-url-storage`), confirmed working unchanged here
- `CSS.registerProperty()` (7 tests)
- `PerformanceObserver` misc: paint/LCP/layout-shift entry delivery, `takeRecords`, `buffered`,
  `disconnect` (5 tests) — distinct fn names from the already-ported `mod v8_perf_observers`
  (slice 5), no collision

**Second Trusted Types cluster found and left in place.** Mixed in among the above (right after
`css_escape_is_function`, before `storage_access_request_storage_access_exists`) sat an 11-test
Trusted Types cluster (`trusted_types_is_defined`, `createPolicy`/`createHTML`/`createScript`/
`createScriptURL`, default policy, duplicate-name handling, `isHTML`/`isScript`/`isScriptURL`) —
the "~10 tests in a second cluster" the original scoping note guessed was at `:29019-29517` (that
line estimate, like `event_target_dependent_apis_installed`'s, had drifted from earlier slices
removing content above it; actual position by this slice was `:16610-16744`). Same block as the
first Trusted Types cluster (before `mod v8_fullscreen_locks`): blocked on
`V8JsRuntime::install_dom` not calling `crate::trusted_types::install_trusted_types_bindings`.
Left untouched in place, with a new comment pointing back at the first cluster's explanation,
directly after the new `mod v8_css_storage_nav_misc` (so both Trusted Types clusters are now
adjacent to each other, one right before `mod v8_fullscreen_locks` and one right after
`mod v8_css_storage_nav_misc`).

41/41 passed on the first run. `cargo test -p lumen-js --features v8-backend` — 2575/2575 (+5 net:
−36 ungated QuickJS, +41 gated V8, the +5 delta from the 1-test-becomes-6-tests split).
Default-feature `cargo test -p lumen-js` — 1375/1375 (down exactly 36 from the 1411 recorded after
`S12b-24-fullscreen-locks`, matching the QuickJS tests removed). Both clippy passes (with and
without `v8-backend`) clean.

### S12b-24-perf-typedom-node — twenty-seventh porting slice (2026-07-30, branch p1-v8-s12b-24-perf-typedom-node)

First slice cut out of the flat ~99-test remainder after `S12b-24-css-storage-nav-misc` (still no
nested `mod v8_*` boundary between `runtime_with_dom` and `mod v8_core`). Four adjacent clusters —
Resource Timing L2 (E-2), the generic `_lumen_deliver_perf_entry` binding (O-2), Navigation Timing
L2 (II-1), CSS Typed OM L1 (A-3) interleaved with the five BUG-281 document/element identity
regression tests, DOM node-count/quota bindings, and the D-6 `chrome.runtime`/`browser.runtime`
stubs — **42 tests** ported to `mod v8_perf_typedom_node`, QuickJS copies deleted. Located by
grepping for `resource_timing_record_exists_in_entries`/`chrome_runtime_get_url` rather than
trusting any recorded line range (none existed for this tail — it was never itemized in the
original scoping table, only flagged as one contiguous ~209/~141/~99-test remainder by the three
preceding slices): actual range at start was `dom.rs:17007-17510`, immediately after the second
Trusted Types cluster (still QuickJS-only, left in place — same blocker as the first cluster, see
`S12b-24-fullscreen-locks`) and immediately before `// ── HTML5 Drag and Drop API (HTML LS §9.10)`.
All 42 tests are synchronous (no `.then()`/microtask timing anywhere in this cluster), so the
S12b-2 promise-timing lesson didn't apply — ported as a literal copy, `runtime_with_dom` renamed to
`v8_runtime_with_dom`.

**Found and fixed [BUG-457](../../bugs/BUG-457-FIXED.md)**, a real V8/QuickJS marshalling
divergence, not a test-authoring artifact: `dom_create_element_throws_quota_exceeded_when_full`
crashed the whole test process on first run (`STATUS_STACK_BUFFER_OVERRUN`, arena index out of
bounds by ~4 billion) instead of failing an assertion. `_lumen_create_element`/
`_lumen_create_element_ns` (`v8_runtime.rs`) signal "arena full" with a `u32::MAX` sentinel that the
engine-agnostic shim checks via `nid < 0` — a contract that only ever worked by rquickjs accident
(its FFI truncates `u32` through a signed 32-bit intermediate, so `u32::MAX` comes out as `-1`).
`IntoJsReturn for u32` (`v8_compat.rs`) instead widens via `self as f64`, so on V8 the same sentinel
becomes the *positive* `4294967295.0`, the shim's `< 0` check never fires, and `_lumen_make_element`
proceeds to wrap and index a node id 4 billion past the 50 000-slot arena — a panic that then
crosses back through V8's native-callback FFI boundary and aborts the process instead of unwinding
(same "no test can observe this as a red assertion" shape BUG-442/BUG-342 had). Fixed by changing
both bindings' return type from `u32` to `i32` (`nid.index() as i32` / `-1` on error) — `IntoJsReturn
for i32` already does the correct sign-preserving `as f64` conversion, and the shim's `nid < 0`
check needed no change. No `V8JsRuntime`/shim changes otherwise; this class of bug (an
rquickjs-marshalling-specific contract silently wrong on V8, uncovered only once its covering test
left the QuickJS monolith) keeps recurring — see BUG-442/BUG-342/BUG-457, all three found by this
same migration.

42/42 passed after the fix. `cargo test -p lumen-js --features v8-backend` — 2575/2575 (unchanged:
−42 ungated QuickJS, +42 gated V8); default-feature `cargo test -p lumen-js` — 1333/1333 (was 1375,
−42). Both clippy passes (with and without `v8-backend`) clean.

### S12b-24-dragdrop-scroll-pointer — twenty-eighth porting slice (2026-07-30, branch p1-v8-s12b-24-dragdrop-scroll-pointer)

Second slice cut out of the flat tail after `S12b-24-perf-typedom-node` (still no nested `mod v8_*`
boundary before `mod v8_core`). Seven adjacent clusters — HTML5 Drag and Drop API (`DataTransfer`/
`DataTransferItem`/`DataTransferItemList`, `_lumen_dispatch_drag_event`), window scroll API (CSSOM
View §4: `scrollTo`/`scrollBy`/`scroll`, `pageYOffset`, `PrintRequest`), "JJ Phase 5" modern HTML5
APIs (`setHTMLUnsafe`/`getHTML`/`moveBefore`/`checkVisibility`), Web Animations API additional
coverage (`DocumentTimeline`, `playbackRate`, `.ready`, `getAnimations()`), and two adjacent Pointer
Events L3 §4.1 clusters (pointer capture altitude/azimuth + `getCoalescedEvents`/
`getPredictedEvents`, and the real coalesced/predicted `pointermove` batch dispatcher plus
`setPointerCapture`/`releasePointerCapture`/`gotpointercapture`/`lostpointercapture`) — **51 tests**
ported to `mod v8_dragdrop_scroll_pointer`, QuickJS copies deleted. Range at start:
`dom.rs:17526-18183`, immediately after the second Trusted Types cluster (still QuickJS-only, same
blocker as before) and immediately before `// ── Pointer Lock API tests`. All native dispatch
helpers used by this cluster (`_lumen_dispatch_drag_event`, `_lumen_dispatch_pointer_event`,
`_lumen_dispatch_pointer_move_coalesced`, `_lumen_dispatch_capture_event`) turned out to be pure JS
functions already living in the shared, engine-agnostic `WEB_API_SHIM` (`dom.rs:3791-4143`), not
native Rust bindings — so unlike `S12b-24-webcrypto`/`S12b-24-page-visibility-beacon`, no new
`V8JsRuntime` plumbing was needed; `take_page_scroll_requests`/`set_page_scroll_y`/
`take_print_requests`/`PrintRequest` were already mirrored. All 51 tests are synchronous (no
`.then()`/microtask timing in this cluster), so the S12b-2 promise-timing lesson didn't apply —
ported as a literal copy, `runtime_with_dom` renamed to `v8_runtime_with_dom`. 51/51 passed on the
first run, no engine divergences, no bugs found. `cargo test -p lumen-js --features v8-backend` —
2575/2575 (unchanged: −51 ungated QuickJS, +51 gated V8); default-feature `cargo test -p lumen-js`
— 1282/1282 (was 1333, −51). Both clippy passes (with and without `v8-backend`) clean. Remainder
after this slice: ~29 tests (Pointer Lock API cluster, `dom.rs:18184+`), immediately before
`mod v8_core` — the flat tail is now down to a single cluster.

### S12b-24-pointer-lock — twenty-ninth porting slice (2026-07-30, branch p1-v8-s12b-24-pointer-lock)

Final flat-tail cluster, `dom.rs:18199-18686`, immediately before `mod v8_core`. Only the first 6
tests under the `// ── Pointer Lock API tests` comment are actually Pointer Lock API (W3C Pointer
Lock L2 §2-4 + Phase 1: `requestPointerLock`/`exitPointerLock`/`pointerLockElement`/
`pointerlockchange`, locked-`mousemove` movement-delta/`pointermove` dispatch via
`_lumen_dispatch_locked_mousemove`). The remaining 23 are an un-headered grab-bag left behind by
earlier slices' scoping — the section comment named only the first cluster, same "don't trust the
header" gotcha as `S12b-24-css-storage-nav-misc`: `Comment`/`Text` constructors + CharacterData
prototype chain/methods (BUG-313/314/322/325), native element/text wrapper `instanceof` resolution
(BUG-322), and `DocumentType`/`DOMImplementation` (BUG-321/324). All 29 tests ported to
`mod v8_pointer_lock`, QuickJS copies deleted, `runtime_with_dom` renamed to `v8_runtime_with_dom`
per convention. All bodies are synchronous `rt.eval(...)` — no promise/microtask timing, so the
S12b-2 lesson didn't apply. 29/29 passed on the first run, no engine divergences, no bugs found.
`cargo test -p lumen-js --features v8-backend` — 2575/2575 (unchanged: −29 ungated QuickJS, +29
gated V8); default-feature `cargo test -p lumen-js` — 1253/1253 (was 1282, −29). Both clippy passes
(with and without `v8-backend`) clean.

**The flat tail is now empty.** Every test in the former `dom.rs::mod tests` monolith is either
under a nested `mod v8_*` (ported, gated on `v8-backend`) or one of the two Trusted Types clusters
(11 tests total, still QuickJS-only — blocked on `V8JsRuntime::install_dom` not calling
`crate::trusted_types::install_trusted_types_bindings`, `v8_runtime.rs:3856`). S12b-24 itself stays
"in progress" until a dedicated slice wires Trusted Types into V8; that slice is the only remaining
work in this task.

### S12b-24-trusted-types — thirtieth (final) porting slice (2026-07-30, branch p1-v8-s12b-24-trusted-types)

Closes S12b-24 entirely. The "11 tests total" estimate two slices back was wrong — it counted only
the second Trusted Types cluster and missed the first one (8 tests, `dom.rs:16030-16154`, right
before `mod v8_fullscreen_locks`); the real count was **19** across both clusters. Another instance
of the "don't trust the header/count" gotcha that already hit `S12b-24-css-storage-nav-misc` and
`S12b-24-pointer-lock` — should have grepped `#[test]` fn names directly instead of trusting the
prose before starting.

The actual blocker (`crate::trusted_types::install_trusted_types_bindings(ctx: &rquickjs::Ctx)` never
called from `V8JsRuntime::install_dom`) turned out to be mechanical, as predicted: `TRUSTED_TYPES_SHIM`
(now `pub(crate)`, was a private `const` inside `trusted_types.rs`) is plain JS with no
`rquickjs`-specific API, so `V8JsRuntime::install_dom` evaluates it inline via the same
`v8::Script::compile`/`run` pattern already used for `WEB_API_SHIM` and `DOM_EXCEPTION_POLYFILL`,
right after the `WEB_API_SHIM` block. The QuickJS-only `install_trusted_types_bindings` helper is
untouched and keeps serving `QuickJsRuntime::install_dom`.

Both clusters merged into one `mod v8_trusted_types` (placed where the first cluster used to be),
QuickJS copies of all 19 tests deleted. All bodies are synchronous `rt.eval(...)` — no promise/
microtask timing, so the S12b-2 lesson doesn't apply; ported as literal copies. 19/19 passed on the
first run, no engine divergences.

Side cleanup, same pattern as `bool_eval` (`S12b-24-window-anim-compress`) and `runtime_with_url`/
`runtime_with_storage` (`S12b-24-nav-url-storage`): the QuickJS-only `runtime_with_dom` helper in the
outer (ungated) `mod tests` lost its last caller (both Trusted Types clusters were the only remaining
ungated tests calling it) and was deleted outright. Unlike those precedents, `make_doc()` still has
hundreds of live callers — just all of them inside `#[cfg(feature = "v8-backend")]` nested modules
now that the flat tail is empty — so instead of deleting it, `make_doc()` and its outer `mod tests`'s
three `use` lines (`super::*`, `lumen_core::JsRuntime`, `lumen_dom::{Document, NodeData, QualName}`)
were gated with `#[cfg(feature = "v8-backend")]` too, since under a build without that feature they
have zero callers left (fresh `dead_code`/`unused_imports` warnings that didn't exist before this
slice — the previous slice's "flat tail is empty" state was only true because the two Trusted Types
clusters were still keeping these alive under a default build).

`cargo test -p lumen-js --features v8-backend` — 2575/2575 (unchanged: −19 ungated QuickJS, +19
gated V8); default-feature `cargo test -p lumen-js` — 1234/1234 (was 1253, −19). Both clippy passes
(with and without `v8-backend`) clean.

**S12b-24 is now fully closed.** Every test that once lived in the `dom.rs::mod tests` monolith is
under a nested `mod v8_*`, gated on `v8-backend`. The remaining rquickjs-removal work (deleting
`QuickJsRuntime`/`QuickPersistentJs` themselves, the 119-file sweep, `Cargo.toml` feature cleanup)
lives in the parent `P3-v8-s12b` task, not this one.

### S12b-B1 — first group-A deletion batch (2026-08-03, branch p1-s12b-b1)

First batch of the `docs/tasks/p1-s12b-cleanup-queue.md` §3 group-A sweep (5 small modules,
831 combined lines): `trusted_types`, `typed_om_api`, `serial`, `scroll_snap_events`, `webxr`.
All five already had a V8-side install path (`trusted_types` inlined straight into
`v8_runtime.rs` per the S12b-24-trusted-types pattern; the other four via `install_*_v8` +
`install_v8!`), so this was pure deletion, no port.

Three of the five (`typed_om_api`, `serial`, `scroll_snap_events`) were the queue-flagged
"trap" modules whose own-file test count undercounts real coverage — confirmed live: `serial`
and `webxr` each had 4 own-file rquickjs tests, but `dom.rs`'s S12b-24 sanity sweep
(`mod v8_css_storage_nav_misc`) only carried over 1 apiece (the `typeof navigator.X === 'object'`
smoke check). `scroll_snap_events` had 5 own-file tests and **zero** in `dom.rs` — nothing had
ported it at all. Per §2 step 2, ported the missing 10 tests into
`dom.rs::mod v8_css_storage_nav_misc` before deleting anything (test count: +10 net, not
reduced — `serial_get_ports_returns_promise`/`serial_request_port_returns_promise`/
`serial_port_class_exists`, `webxr_is_session_supported_returns_promise_false`/
`webxr_request_session_returns_promise`/`webxr_stub_classes_exist`, and all 5
`scroll_snap_events` tests renamed to their original names). `typed_om_api` needed no test
porting — its coverage already lived fully in `dom.rs::mod v8_perf_typedom_node` from S12b-24.

Removed per module: the rquickjs `install_*` fn, `use rquickjs::Ctx`, the own-file `mod tests`
(rquickjs-only harness, now redundant), and the call from `QuickJsRuntime::install_dom` (`lib.rs`,
4 call sites — `typed_om_api`/`serial`/`scroll_snap_events`/`webxr`; `trusted_types`'s call lived
in `dom.rs::install_primitives` instead, not `lib.rs`). Gated each SHIM const
`#[cfg(feature = "v8-backend")]` since only the V8 install path reads it now (`empty_line_after_doc_comments`
clippy hit on `serial.rs`/`webxr.rs`'s now-orphaned `///` module header — same S12b-5/8/10/12
gotcha, fixed by switching to `//!`). `pub mod` declarations kept in `lib.rs` (V8 fn/consts still
live there, per §2 step 7).

`cargo test -p lumen-js --features v8-backend` — 2584/2584, all green (the 10 ported tests
included); default-feature `cargo test -p lumen-js` — 1221/1221, all green (the 4+4+5
rquickjs-only tests from the 5 modules' own `mod tests` blocks are gone, as expected). Both
clippy passes clean.

**S12b-B2** (2026-08-03): second batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3), 5 more small modules, none of them own-file trap cases (own `mod tests` count already
matched the queue's expected total for all five, so no tests needed backfilling from
`dom.rs`): `soft_navigation` (5 tests), `bluetooth` (7), `eye_dropper` (6), `virtual_keyboard`
(5), `local_font_access` (8) — 31 tests total, all ported in place to
`#[cfg(all(test, feature = "v8-backend"))]` against a bare `V8JsRuntime::new()` (no full
`install_dom`), with `bluetooth`/`virtual_keyboard` re-declaring the minimal
`EventTarget`/`Event`/`DOMException`/`DOMRect` stubs their shims need directly in the test
setup script, same as the original rquickjs harness did.

`cargo test -p lumen-js --features v8-backend`: 2584/2584 (unchanged from S12b-B1 — no test
count drift, only in-place porting). `cargo test -p lumen-js`: 1190/1190 (down from
1221, exactly the 31 removed rquickjs tests; rquickjs suite did not go red). Both clippy
passes clean.

**S12b-B3** (2026-08-03): third batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3), 5 more small modules, none trap cases (own `mod tests` counts matched the queue's
expected totals): `sanitizer` (8 tests), `ua_client_hints` (4), `reporting_api` (6),
`launch_handler` (9), `storage_manager` (10) — 37 total, all ported in place to
`#[cfg(all(test, feature = "v8-backend"))]`. Three modules (`ua_client_hints`,
`reporting_api`, `launch_handler`) still had a leftover `///` (not `//!`) file-header doc
comment from before the rquickjs `use`/fn were removed — same S12b-5/8/10/12/B1
`empty_line_after_doc_comments` gotcha, plus a `doc_lazy_continuation` hit specific to this
batch (rustdoc merges consecutive `///` blocks separated only by a blank line into one doc
string attached to the next item, so a module-level bullet list followed by a blank-then-doc
paragraph reads as an unindented list continuation); fixed the same way, `//!` for all
module-level headers.

`sanitizer` needed a harness fix beyond the mechanical port: its shim is the one case in this
batch **not** IIFE-wrapped (top-level `const DANGEROUS_ATTRS`/`function` declarations), and
`install_dom` already installs it via `install_v8!` (v8_runtime.rs) as part of full DOM setup
— the naive test harness (`install_dom` then a second explicit
`install_sanitizer_bindings_v8` call, copied from the `document_pip`-style full-DOM template)
re-evaluated the same top-level `const`s in one global scope and failed every test with
`Runtime("Identifier 'DANGEROUS_ATTRS' has already been declared")`. Fix: drop the second
call — `install_dom` alone is sufficient, matching `document_pip.rs`'s actual pattern (which
this batch's other full-DOM template copy had gotten right; only `sanitizer.rs` had the extra
call). Worth flagging for future full-`install_dom` batches: check whether the target module
is already wired into `install_dom` via `install_v8!` before writing a harness — if it is,
installing it a second time is only safe when the shim is IIFE-wrapped.

`cargo test -p lumen-js --features v8-backend`: 2584/2584 (unchanged from S12b-B2 — no test
count drift, only in-place porting). `cargo test -p lumen-js`: 1153/1153 (down from 1190,
exactly the 37 removed rquickjs tests; rquickjs suite did not go red). Both clippy passes
clean.

**S12b-B4** (2026-08-03): fourth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3), 5 more small modules, none trap cases (own `mod tests` counts matched the queue's
expected totals): `webhid` (7 tests), `network_log_bindings` (8), `css_properties_values_api`
(7), `scheduler` (5), `paint_worklet` (8) — 35 total. `css_properties_values_api`'s 7 tests
turned out to already be pure-Rust (`RegisteredPropertiesMap`/`RegisteredProperty` struct
tests, no `rquickjs` import in `mod tests` at all — the module's only rquickjs dependency was
the production `install_css_properties_values_api` fn), so they needed no porting, only the
production-side removal; `paint_worklet` split similarly (3 pure-Rust registry tests kept
as-is, 5 JS-integration tests ported). The other three ported in place to
`#[cfg(all(test, feature = "v8-backend"))]` against a bare `V8JsRuntime::new()` (no full
`install_dom`), `webhid` re-declaring the same minimal `EventTarget`/`Event`/`DOMException`
stubs its shim needs, matching `bluetooth.rs`'s S12b-B2 harness; `network_log_bindings`
mirrored `download_bindings.rs`'s existing template exactly (register the native directly, no
DOM stubs needed) — including gating its 2 already-pure-Rust queue tests
(`enqueue_and_take_roundtrips`/`take_clears_queue`) behind `v8-backend` too, matching that
template's precedent rather than leaving them ungated.

Confirmed before touching `scheduler.rs` that it is **not** shadowed by `dom.rs`'s own
simpler `var scheduler` shim (WEB_API_SHIM installs first, `scheduler::install_scheduler_api`
overwrites `globalThis.scheduler` after — same order holds for both engines, `install_dom`
evaluates `WEB_API_SHIM` before running `install_v8!(scheduler::install_scheduler_api_v8)`),
so this is not a G0-style dead/shadowed-module case like `webgl_bindings`/`audio_bindings`.

`webhid` hit the S12b-5/8/10/12/B1 `empty_line_after_doc_comments` gotcha (leftover `///`
file-header before the rquickjs `use`/fn removal) — fixed with `//!`.

`cargo test -p lumen-js --features v8-backend`: 2584/2584 (unchanged from S12b-B3 — no test
count drift, only in-place porting). `cargo test -p lumen-js`: 1128/1128 modulo one
pre-existing, unrelated flake — `screen_capture::tests::null_provider_list_sources_returns_empty_array`
fails under the default parallel run (global provider-singleton race across concurrently
running tests) but passes in isolation and under `--test-threads=1` (full suite 1128/1128
serial); down from 1153, exactly the 25 rquickjs tests actually removed from the default
build (7 `webhid` + 8 `network_log_bindings` + 0 `css_properties_values_api`, already
ungated + 5 `scheduler` + 5 of `paint_worklet`'s 8 — its 3 registry tests stayed ungated).
rquickjs suite did not go red from this batch. Both clippy passes clean.

**S12b-B5** (2026-08-03): fifth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3), 5 small modules: `presentation_api` (6 tests), `screen_orientation` (8), `window_management`
(8), `navigation_api` (0), `speech` (0 in its own `mod tests`) — 22 total, all ported in place to
`#[cfg(all(test, feature = "v8-backend"))]` against a bare `V8JsRuntime::new()` (no full
`install_dom`), reusing each module's existing rquickjs prereq-eval string almost verbatim
(swap `ctx.eval::<bool,_>(...)`/`Context::full` boilerplate for `rt.eval(...)` +
`JsValue::Bool` comparisons).

`speech` was the batch's actual trap: its own `mod tests` had 0 tests, but a **hidden
25-test integration suite** lives in `crates/js/tests/cases/speech_api.rs`, built against
`QuickJsRuntime::install_dom` (not caught by the `dom.rs` file-stem grep from §2 step 2 — the
tests live in a *separate integration test file*, not `dom.rs`). Deleting
`speech::install_speech_bindings`'s call out of `QuickJsRuntime::install_dom` silently broke
all 25 (`speechSynthesis is not defined` etc.) on the next full-suite run, after clippy had
already gone green — the gate order (clippy before test) doesn't catch this, only
`cargo test -p lumen-js --features v8-backend` does. Ported by swapping
`QuickJsRuntime`/`lumen_js::QuickJsRuntime` for `V8JsRuntime`/`lumen_js::v8_runtime::V8JsRuntime`
(same `install_dom` signature, mirrors the QuickJS one per its own doc comment) and adding
`#![cfg(feature = "v8-backend")]` at the file top (mirrors `v8_eval.rs`/`v8_smoke.rs`) so the
default build's `tests/cases/mod.rs` aggregation empties it out instead of trying to compile
against a type that no longer installs the API. **Lesson for remaining batches:** step 2 of
§2 must also `grep -rl "<file_stem>" crates/js/tests/cases/` in addition to `dom.rs` — any
module with a dedicated integration-test file is exactly this trap.

`platform_speak_async`/`platform_speak_blocking` (the OS-TTS thread helpers in `speech.rs`,
shared by both engines' install fns) went dead-code once the rquickjs install fn calling them
was removed — gated all of them behind `#[cfg(feature = "v8-backend")]` too (per-target-OS
`cfg` combined via `all(...)`), otherwise `cargo clippy -p lumen-js --all-targets -- -D
warnings` (no `v8-backend`) fails on unused-function.

`navigation_api.rs` hit the S12b-5/8/10/12/B1/B4 `empty_line_after_doc_comments` gotcha
(leftover `///` file-header before the rquickjs `use`/fn removal, with a blank line before the
next doc block) — fixed by converting the header to `//!` module-doc.

`cargo test -p lumen-js --features v8-backend`: 2584 lib + 68 integration, all green (lib
count unchanged from S12b-B4 — in-place porting only; integration count unchanged too, since
`speech_api.rs`'s 25 tests already existed in this binary before the batch, only the *default*
binary lost them). `cargo test -p lumen-js` (default): 1106 lib (down from 1128, the 22 ported
`mod tests`) + 24 integration (down from 49, the 25 `speech_api.rs` tests now v8-only). rquickjs
suite did not go red from this batch. Both clippy passes clean.

Next in queue: S12b-B6 (`iframe_element`/`url_pattern`/`web_midi`/`surface_api`/`scroll_timeline`).

**S12b-B6** (2026-08-04): sixth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 1 last batch), 5 small modules: `iframe_element` (10 tests), `url_pattern` (5),
`web_midi` (11), `surface_api` (11), `scroll_timeline` (14) — 51 total, all ported in place to
`#[cfg(all(test, feature = "v8-backend"))]` against a bare `V8JsRuntime::new()` (no full
`install_dom`), swapping `ctx.eval::<T,_>(...)`/`Context::full` boilerplate for `rt.eval(...)` +
`JsValue::{Bool,String,Number}` comparisons. Step-2 grep (`grep -n "<file_stem>_" dom.rs`) found
no hidden tests for any of the 5 in `dom.rs` itself.

`surface_api` repeated the S12b-B5 `speech`/`speech_api.rs` trap: its own `mod tests` had 11
tests, but a **19-test integration suite** in `crates/js/tests/cases/no_automation_markers.rs`
also builds `QuickJsRuntime::install_dom` — only 4 of the 19 (`navigator_app_name_is_netscape`,
`navigator_vendor_is_google`, `navigator_plugins_is_object`, `navigator_mime_types_is_object`)
actually depend on `surface_api`'s properties; the other 15 (webdriver/playwright/cdc_/phantom/
domAutomation absence, `isTrusted`) hold regardless of which engine installs DOM, since those
markers are never defined by any install fn (asserting absence, not presence). Ported the whole
file to `V8JsRuntime` + `#![cfg(feature = "v8-backend")]` anyway (matching the S12b-B5 precedent
verbatim, `lumen_js::QuickJsRuntime` → `lumen_js::v8_runtime::V8JsRuntime`) rather than
partial-splitting the file — keeps one file, one engine, and the 15 engine-agnostic assertions
now validate against the default (V8) build per `CLAUDE.md`'s "validate JS work against the
default build" instruction instead of the rollback path. Confirmed by the intermediate
`cargo test -p lumen-js --features v8-backend` run: exactly 4 failures
(`navigator_{app_name,vendor,plugins,mime_types}_*`), matching the predicted surface_api-only
dependency before the file-level fix.

No `empty_line_after_doc_comments` hits except `url_pattern.rs` (leftover `///` file-header
before the rquickjs `use`/fn block, blank line before the next doc — same S12b-5/8/10/12/B1/B4/B5
shape) — fixed by converting the header to `//!`. The other 4 modules already used `//!`
module-doc, no fix needed there.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only) +
68 integration (unchanged — `no_automation_markers.rs`'s 19 tests already ran against
`QuickJsRuntime` in this binary before the batch, now against `V8JsRuntime`, same count).
`cargo test -p lumen-js` (default): 1055 lib (down from 1106, the 51 ported `mod tests`) + 5
integration (down from 24, the 19 `no_automation_markers.rs` tests now v8-only). rquickjs suite
did not go red from this batch. Both clippy passes clean.

Next in queue: S12b-B7 (`web_locks`/`webusb`/`close_watcher`, Полоса 2 first batch).

**S12b-B7** (2026-08-04): seventh batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 2 first batch), 3 medium modules: `web_locks` (6 tests), `webusb` (8), `close_watcher`
(8) — 22 total, all ported in place to `#[cfg(all(test, feature = "v8-backend"))]` against a bare
`V8JsRuntime::new()`, swapping `ctx.eval::<T,_>(...)`/`Context::full` boilerplate for
`rt.eval(...)` + `JsValue::{Bool,String}` comparisons. Step-2 grep (`grep -n "<file_stem>_" dom.rs`)
found no hidden per-module tests for any of the 3 — the one `webusb`-adjacent hit in `dom.rs`
(`event_target_dependent_navigator_usb_installed`) is a generic EventTarget-install-order test
already validating V8, not part of `webusb.rs`'s own suite.

`web_locks`'s shim self-installs `navigator = {}` when absent, so its V8 harness needs no stub
eval at all (`install_web_locks_bindings_v8` called directly on a bare runtime) — none of its 6
tests exercise the `DOMException`-dependent abort path either, so the old rquickjs test's
`DOMException` stub had no V8 equivalent to carry over. `webusb` and `close_watcher` both still
needed the same minimal-stub-`eval()`-before-install pattern as the rquickjs tests (webusb:
`window`/`navigator`/`EventTarget`/`Event`/`DOMException`; close_watcher: `document`/`window`/
`Event`), just re-expressed via `rt.eval(...)` instead of `ctx.eval::<(), _>(...)`.

No `empty_line_after_doc_comments` hits: `web_locks.rs` already had a `//!` module header with no
following blank-line-then-doc-comment shape; `webusb.rs`'s former `///`-then-`use` header was
converted to `//!` as part of the same edit that removed the rquickjs fn (no separate fix needed);
`close_watcher.rs` already used `//!`.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only) + 68
integration (unchanged, no integration-suite modules in this batch). `cargo test -p lumen-js`
(default): 1033 lib (down from 1055, the 22 ported `mod tests`) + 5 integration (unchanged).
rquickjs suite did not go red from this batch. Both clippy passes clean.

Next in queue: S12b-B8 (`gamepad`/`generic_sensor`/`shared_storage`, Полоса 2 second batch).

**S12b-B8** (2026-08-04): eighth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 2 second batch), 3 medium modules: `gamepad` (16 tests), `generic_sensor` (16),
`shared_storage` (13) — 45 total, all ported in place to `#[cfg(all(test, feature =
"v8-backend"))]` against a bare `V8JsRuntime::new()`. Step-2 grep (`grep -n "<file_stem>_"
dom.rs`) found no hidden per-module tests for any of the 3.

`gamepad`'s shim needs `navigator`/`Event`/`window.dispatchEvent` stubs (same shape as the old
rquickjs test), evaluated via one `rt.eval(...)` before `install_gamepad_bindings_v8`.
`generic_sensor` is fully self-contained (`Promise`/`Float32Array`/`Float64Array` only, no
`navigator`/`Event` dependency) — its old rquickjs test used a full `QuickJsRuntime::install_dom`
harness for no reason the shim needs; the V8 port drops that in favor of a bare runtime, matching
the shim's actual dependencies. `shared_storage` is likewise self-contained (`Promise` +
`Symbol.asyncIterator` only).

`shared_storage`'s old rquickjs tests resolved promises via `ctx.execute_pending_job()` draining
in a loop; V8 has no equivalent exposed through `JsRuntime`, so promise-returning assertions
(`get`/`append`/`delete`/`length`/`remainingBudget`/`selectURL`/the two-`next()`-call async-iterator
check) follow the two-`eval()`-call pattern already established in `ua_client_hints.rs`: schedule
the `.then()` in one `rt.eval()`, read the global it wrote in a second `rt.eval()` — V8's default
microtask policy drains the queue automatically between separate top-level script evaluations, no
manual drain needed.

No `empty_line_after_doc_comments` hits: all 3 modules already used `//!` or had their doc header
absorbed into the (now `#[cfg(feature = "v8-backend")]`-gated) function doc comment directly above
`install_*_v8`.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only, same
as B7's reasoning: the old rquickjs `mod tests` were plain `#[cfg(test)]` with no feature gate, so
they already ran under `--features v8-backend` before this batch; net swap is zero) + 68
integration (unchanged, no integration-suite modules in this batch). `cargo test -p lumen-js`
(default): 988 lib (down from 1033, the 45 ported `mod tests`) + 5 integration (unchanged).
rquickjs suite did not go red from this batch (988 passed). Both clippy passes clean.

Next in queue: S12b-B9 (`element_internals`/`video_pip`/`media_session`, Полоса 2 third batch).

**S12b-B9** (2026-08-04): ninth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 2 third batch), 3 medium modules: `element_internals` (7 tests), `video_pip` (11),
`media_session` (15) — 33 total, all ported in place to `#[cfg(all(test, feature =
"v8-backend"))]` against a bare `V8JsRuntime::new()`. Step-2 grep (`grep -n "<file_stem>_"
dom.rs`) found no hidden per-module tests for any of the 3 — same negative result as B7/B8.

All three shims are pure `ctx.eval(SHIM)` with no rquickjs native bindings, so the port is a
mechanical swap of the harness only. `element_internals`'s test stubs (`Event`/`Element`/
`_lumen_set_attr`/`_lumen_remove_attr`/`makeEl`) and `video_pip`'s (`EventTarget`/`Event`/
`document.createElement`) carried over unchanged, just re-expressed via `rt.eval(...)` instead
of `ctx.eval::<(), _>(...)`. `media_session` needed only a bare `navigator`/`window` stub.
`video_pip`'s promise-returning assertions (`requestPictureInPicture()`/`exitPictureInPicture()`
`instanceof Promise`) stayed synchronous — no `.then()`/microtask draining needed, matching the
`webusb`-style pattern from B7 rather than the two-`eval()`-call pattern B8 needed for resolved
values.

`empty_line_after_doc_comments` hit twice: `element_internals.rs` and `video_pip.rs` both had a
`///`-block module header immediately followed by a blank line then the first function's doc
comment — converted both headers to `//!`. `media_session.rs` already used `//!`, no fix needed.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only, same
reasoning as B7/B8: the old rquickjs `mod tests` were plain `#[cfg(test)]` with no feature gate,
so they already ran under `--features v8-backend` before this batch) + 68 integration (unchanged).
`cargo test -p lumen-js` (default): 955 lib (down from 988, the 33 ported `mod tests`) + 5
integration (unchanged). rquickjs suite did not go red from this batch (955 passed). Both clippy
passes clean.

Next in queue: S12b-B10 (`long_animation_frames`/`form_validation`/`wake_lock`, Полоса 2 fourth batch).

**S12b-B10** (2026-08-04): tenth batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 2 fourth batch), 3 medium modules: `long_animation_frames` (10 tests),
`form_validation` (7), `wake_lock` (12) — 29 total, all ported in place to `#[cfg(all(test,
feature = "v8-backend"))]` against a bare `V8JsRuntime::new()`. Step-2 grep
(`grep -n "<file_stem>_" dom.rs`) found no hidden per-module tests for `long_animation_frames`
and `form_validation`, but `wake_lock` had 3 (`wake_lock_request_resolves`,
`wake_lock_release_marks_released`, `wake_lock_unsupported_type_rejects`, `dom.rs:16590-16621`)
— already `#[cfg(feature = "v8-backend")]`-gated V8 coverage from an earlier slice, not
rquickjs-side, so left untouched (same negative-ish result shape as `pip_bindings` in the
procedure note, but here the hidden tests were already V8, nothing to port).

`long_animation_frames` and `form_validation` are pure `ctx.eval(SHIM)` shims, mechanical harness
swap only. `wake_lock` is the first module in Полоса 2 with native bindings
(`__lumen_wake_lock_request`/`__lumen_wake_lock_release`) still to delete on the rquickjs side:
removed `install_native_bindings` (`rquickjs::Function::new` closures) and the rquickjs
`install_wake_lock_bindings`, keeping only `install_wake_lock_bindings_v8` (already present since
S5-S7, registers both natives via `v8_compat::into_v8_fn0` then evals the shim). `get_provider()`
and the `NullWakeLockProvider` import became `#[cfg(feature = "v8-backend")]`-only — they were
referenced solely from the deleted rquickjs installer and the (already feature-gated) V8 installer
and tests; clippy caught the resulting dead-code/unused-import once the rquickjs side was gone.

`empty_line_after_doc_comments`: `form_validation.rs` had an `///`-block header
(`use rquickjs::Ctx;` immediately after, no blank line) — since the `///` doc-comment target
(`use rquickjs::Ctx;`) was deleted along with the rquickjs installer, the header would have
docced the crate-doc-less `pub(crate) fn install_form_validation_bindings_v8` instead of nothing;
converted to `//!` for correctness rather than to dodge a lint (no blank line meant no lint hit
either way). `long_animation_frames.rs` and `wake_lock.rs` already used `//!`.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only, same
reasoning as B7-B9) + 68 integration (unchanged). `cargo test -p lumen-js` (default): 926 lib
(down from 955, the 29 ported `mod tests`) + 5 integration (unchanged). rquickjs suite did not go
red from this batch (926 passed). Both clippy passes clean.

Next in queue: S12b-B11 (`navigator_bindings`/`media_capture`/`screen_capture`, Полоса 2 fifth batch).

**S12b-B11** (2026-08-04): eleventh batch of queue group A (`docs/tasks/p1-s12b-cleanup-queue.md`
§3, Полоса 2 fifth batch), 3 medium modules: `navigator_bindings` (16 tests), `media_capture` (8),
`screen_capture` (11) — 35 total, all ported in place to `#[cfg(all(test, feature =
"v8-backend"))]`. Step-2 grep (`grep -n "<file_stem>_" dom.rs`) found no hidden per-module tests
for any of the 3.

`navigator_bindings` is a pure `ctx.eval(SHIM)` shim; the rquickjs side additionally exposed a
test-only `install_navigator_bindings_with(ctx, profile)` (bypasses the process-global
`NavigatorProfile` to avoid a global-state race between parallel profile tests) with no V8
equivalent — added `install_navigator_bindings_v8_with(rt, profile)` alongside the existing
`install_navigator_bindings_v8` (which now delegates to it with `current_navigator_profile()`) so
the custom-profile tests (`custom_profile_applies_all_fields`, `empty_languages_falls_back_to_en_us`,
`language_with_quote_is_escaped_safely`) have the same isolation on V8.

`media_capture` and `screen_capture` are the second and third Полоса-2 modules with native
bindings left to delete on the rquickjs side (after `wake_lock` in B10) — both bridge a
process-global `Arc<dyn *Provider>` plus a thread-local `CAPTURES: HashMap<u64, Box<dyn
*Handle>>` to five `__lumen_*` natives each. Removed the rquickjs `install_*_bindings`
(`rquickjs::Function::new` closures) from both, keeping only the already-present
`install_*_bindings_v8` (`v8_compat::into_v8_fn{0,1,2,3}` + `register_native`). `get_provider()`
and `NEXT_HANDLE_ID` (plus the `AudioCaptureConfig`/`ScreenCaptureConfig` and
`atomic::{AtomicU64, Ordering}` imports) became `#[cfg(feature = "v8-backend")]`-only in both
files — same dead-code/unused-import fallout as `wake_lock` in B10, for the same reason (both were
referenced solely from the deleted rquickjs installer and the feature-gated V8 installer/tests).

Porting the id-returning tests surfaced a pre-existing race, latent under rquickjs and load-bearing
under V8: `NEXT_HANDLE_ID` is a single process-global `AtomicU64` (not thread-local) shared by every
test in the module, and `PROVIDER` is a process-global `RwLock` set immediately before each
`install_*_bindings_v8` call snapshots it — with `cargo test`'s default parallel threads, a
V8JsRuntime's slower construction widened the window enough that a concurrently-running test's
`set_*_provider` call could land between another test's `set_*_provider` and its `install_*`
snapshot, or the shared counter could hand out ids >1 for a supposedly-first capture. Under
rquickjs this either didn't reproduce or was masked; the original tests already hedged the id
non-determinism with `assert!(id >= 1.0)` (never `== 1.0`) but had no guard against the provider
race. Fixed both: ported the `assert!(id >= 1.0)` semantics as-is (a stray `assert_eq!(id,
JsValue::Number(1.0))` in the first draft failed reproducibly at `id: Number(5.0)`), and added a
per-module `static TEST_LOCK: Mutex<()>` + `guard()` serializing every test that touches
`PROVIDER`/`NEXT_HANDLE_ID` — the same pattern already used by `documentpip_bindings.rs`/
`download_bindings.rs`/`network_log_bindings.rs`/`pip_bindings.rs`/`pointer_lock.rs`/
`video_bindings.rs` for analogous global-state races. 3 reruns of the full filtered suite after the
fix: stable, 0 flakes.

Removing the three rquickjs installers also removed their calls from `QuickJsRuntime::install_dom`
(`lib.rs`) — under the rquickjs (opt-in rollback) path, `navigator`/`screen` fingerprint
normalization, timezone offset patching, and the two native capture bridges are no longer wired at
all; the V8 (default) path is unaffected, its `install_dom` call site in `v8_runtime.rs` was
already separate and unchanged. Consistent with every earlier Group-A batch — rquickjs sheds
Web-API surface batch by batch on its way to full deletion (F1-F4).

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — in-place porting only, same
reasoning as B7-B10) + 68 integration (unchanged). `cargo test -p lumen-js` (default): 891 lib
(down from 926, the 35 ported `mod tests`) + 5 integration (unchanged). rquickjs suite did not go
red from this batch (891 passed). Both clippy passes clean.

Next in queue: S12b-B12 (`geolocation`/`esm`/`idle_detection`, Полоса 2 sixth batch).

### S12b-B12 (`geolocation`, `idle_detection` — `esm` pulled from batch)

`geolocation.rs` (17 tests) and `idle_detection.rs` (17 tests) both fit the
group-A procedure cleanly: `install_*_bindings_v8` already existed for both
(`geolocation.rs:36`, `idle_detection.rs` post-edit), neither has any test
hiding in `dom.rs`. Ported 1:1 against a bare `V8JsRuntime::new()` +
module-local `navigator`/timer/`EventTarget` stubs (same shape as the
rquickjs harness each module already had) — `JsValue::Bool`/`Number`/`String`
comparisons replacing rquickjs's typed `ctx.eval::<T, _>`. `idle_detection`'s
two promise-based tests (`request_permission_returns_granted`,
`start_rejects_threshold_below_60s`) used the two-step-`eval` pattern
(`promise_result` helper, same shape as `shared_storage.rs`) instead of
`execute_pending_job` — V8 checkpoints microtasks at the `eval()` boundary,
not mid-script. Removed both rquickjs installers, their calls from
`QuickJsRuntime::install_dom` (`lib.rs`), and gated `GEO_SHIM`/
`IDLE_DETECTION_SHIM`/`user_idle_ms` (both OS variants) behind
`#[cfg(feature = "v8-backend")]` — same dead-code fallout pattern as B7-B11
(these were referenced only by the deleted rquickjs installer plus the
feature-gated V8 path).

**`esm.rs` does not fit the group-A procedure and was pulled from the batch**
(§7 policy: don't force-fit mid-session, re-queue instead). Unlike every
other module so far, `esm.rs`'s rquickjs-specific surface (`impl Resolver for
LumenResolver`, `impl Loader for LumenLoader`, both rquickjs trait impls) is
not installed via a `install_*_bindings(ctx)` call from
`QuickJsRuntime::install_dom` — it is wired directly into
`QuickJsRuntime::new()`/`js_thread_main()` (`lib.rs`) as the runtime's core
module-loading plumbing: `rt.set_loader(resolver, loader)`, plus four
`QuickJsRuntime` struct fields (`module_registry`, `module_page_url`,
`module_import_map`, `module_types`) and three methods
(`register_module_source`, `set_import_map`, `preprocess_import_attributes`)
that read them. `ImportMap`/`resolve_specifier_with`/`new_registry` etc. are
engine-agnostic and already shared with `v8_esm.rs` (`crate::esm::ImportMap`,
`crate::esm::resolve_specifier_with` — see `v8_esm.rs:32`), so those stay
regardless. Removing the rquickjs `Resolver`/`Loader` impls would require
gutting `QuickJsRuntime`'s ES-module support wholesale (the struct fields,
`js_thread_main`'s `set_loader` call, the three methods above) — that is the
same shape of change as `S12b-F2` (deleting `QuickJsRuntime` itself), not a
standalone batch item, so it belongs there rather than being force-fit here.
`esm` is **not** re-added to Group A's Полоса 2 queue; treat its rquickjs
removal as implicit in F2 and skip it as a separate line item when F2 lands.

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged, in-place
porting) + 68 integration (unchanged). `cargo test -p lumen-js` (default):
857 lib (down from 891, the 34 ported `mod tests`) + 5 integration
(unchanged). rquickjs suite did not go red (857 passed). Both clippy passes
clean.

Next in queue: S12b-B13 (`broadcast_channel`/`webrtc_stub`/`credentials`, Полоса 2 seventh batch).

### S12b-B13 (`broadcast_channel`, `webrtc_stub`, `credentials`)

All three modules fit the group-A procedure cleanly: `install_*_bindings_v8`
already existed for each (`broadcast_channel.rs:150`, `webrtc_stub.rs:34`,
`credentials.rs:74`), no test hid in `dom.rs` (a `webauthn_credentials`-family
grep against `dom.rs` returned nothing). `broadcast_channel` (14 tests) ported
1:1 against `V8JsRuntime::new()` + full `install_dom` (mirrors the pre-existing
`v8_trusted_types` pattern in `dom.rs` — the shim needs `MessageEvent`/
`DOMException` from the core DOM shim); its `pump_broadcast_channels()` twin on
`V8JsRuntime` already existed and is synchronous (`eval` per drain), so no
microtask handling was needed. `webrtc_stub` (17 tests) ported 1:1 against a
bare `V8JsRuntime::new()` + module-local synchronous `setTimeout`/
`queueMicrotask` stubs (same shape as the rquickjs original's `install_stubs`
harness — the shim's `_defer` helper snapshots `typeof queueMicrotask`/
`typeof setTimeout` once at install time, so both engines just fall through to
a same-tick call either way); its one real-Promise test
(`create_offer_resolves_to_offer_type`) used the two-step-`eval` pattern
instead of `execute_pending_job`.

`credentials` (12 tests) split differently from every module so far: 7 of its
tests exercise plain Rust functions (`create`/`get`/`base64url_encode`/etc.)
with **no** `rquickjs` dependency at all — those stayed untouched in the
existing engine-agnostic `mod tests`. Only the 5 `fedcm_*` tests evaluated the
shim through a bare `rquickjs::Ctx` (`with_credentials_shim` helper) — those
moved to a new `mod v8_fedcm` (`#[cfg(all(test, feature = "v8-backend"))]`)
against a bare `V8JsRuntime::new()` + the same `window`/`navigator`/`atob`/
`btoa`/`TextEncoder` stubs. **New hiding spot found**: a fourth test cluster
lived in `crates/js/tests/cases/webauthn_credentials.rs` — a *separate
integration-test crate*, not `dom.rs` and not `credentials.rs`'s own `mod
tests` (same class of trap as S12b-B5/B6's dom.rs-adjacent hides, but one level
further out — grep step 2 of the procedure only covers `dom.rs`, not
`tests/cases/*.rs`; both need checking for modules that ship an end-to-end
integration test). Unlike `v8_smoke.rs`/`v8_eval.rs` (feature-gated from
inception), this file had no `#![cfg(feature = "v8-backend")]` guard — it ran
unconditionally against `QuickJsRuntime`, so removing
`credentials::install_credentials_bindings` broke it outright (`navigator
.credentials` no longer installed under any engine). Fixed by porting all 4
tests to `V8JsRuntime` + full `install_dom` and adding the
`#![cfg(feature = "v8-backend")]` guard used by the file's siblings; the two
`_lumen_drain_microtasks()` calls (both after a `.then()` scheduling a
same-tick resolve/reject) became no-ops removed under the two-step-`eval`
pattern, same as B12/S12b-24.

Removed: 3 rquickjs `install_*_bindings` functions, their calls in
`QuickJsRuntime::install_dom` (`lib.rs`), and `use rquickjs::…` from all three
module files; `BROADCAST_CHANNEL_SHIM`/`WEBRTC_SHIM`/`CREDENTIALS_SHIM` gated
`#[cfg(feature = "v8-backend")]` (now read only by the V8 path). `broadcast_channel`'s
`register`/`post`/`close`/`drain` helpers and `credentials`'s native-binding
functions (`create`/`get`/`uvpa_available`) stay ungated — both engines' native
registrations call them (rquickjs's in `dom.rs`'s `install_primitives` for the
WebAuthn natives specifically, out of scope for this batch — removed wholesale
with `install_primitives` itself in `S12b-F3`).

`cargo test -p lumen-js --features v8-backend`: 2584 lib + 68 integration
(unchanged — `rquickjs` isn't feature-gated, so these 36 lib tests + 4
integration tests already ran under `--features v8-backend` before this batch;
converting them to `V8JsRuntime` ports them in place without changing the
total). `cargo test -p lumen-js` (default): 821 lib (down from 857, -36: the
14+17+5 tests moved to V8-only) + 1 integration (down from 5, -4:
`webauthn_credentials.rs` now empties under the default build). rquickjs suite
did not go red (821+1 passed). Both clippy passes clean.

Next in queue: S12b-B14 (`xhr`/`file_input`, Полоса 2 eighth/final batch).

### S12b-B14 (`xhr`, `file_input` — Полоса 2 final batch)

Last batch of Полоса 2. Both modules fit the group-A procedure cleanly:
`install_xhr_bindings_v8`/`install_file_input_bindings_v8` already existed
(`xhr.rs:44`, `file_input.rs:107`), both wired through the plain `install_v8!`
macro in `v8_runtime.rs` (no extra native-state args, despite the queue's
blanket note that `file_input` "needs full `install_dom`" — like B12's
`geolocation`/`idle_detection`, that turned out unnecessary in practice). No
tests hid in `dom.rs` or `crates/js/tests/cases/*.rs` (grepped both).

`xhr` (17 tests) ported 1:1 against `V8JsRuntime::new()` + full `install_dom`
(the shim needs `Event`/`DOMException`/`FormData`/`Blob`/`TextEncoder`/
`TextDecoder` — same shape as `broadcast_channel`'s S12b-B13 harness). None of
the existing tests call `.send()` (no fetch-triggering coverage existed even
under rquickjs), so no fetch provider was needed in the harness.

`file_input` (18 tests) split like `credentials` did in B13: 4 of its tests
(`register_file_token_unique`, `to_base64_empty`/`_hello`/`_binary`) have no
`rquickjs`/engine dependency at all. Of those, only `register_file_token_unique`
stayed in the plain ungated `mod tests` — `register_file_token`/
`clear_file_registry` are still called unconditionally by production code
(`crates/shell/src/main.rs`, `filesystem_access.rs`) regardless of engine.
`to_base64`/`read_file_bytes_for_token` are **not** in that position: their
only remaining caller after this batch is the V8 install path, so leaving them
ungated left them as dead code in the default (rquickjs) build (`-D
dead_code`, caught by the batch gate's default clippy pass). Fixed by gating
both fns `#[cfg(feature = "v8-backend")]` and moving the `to_base64_*` tests
into the new `mod v8_tests` alongside the JS-shim tests — a variant of the B4
`network_log_bindings` precedent ("gate previously-pure-Rust tests behind
`v8-backend` too, once their only production caller is v8-gated"), but
triggered by a compile error instead of applied proactively; worth checking
for on every future batch where a JS-shim install fn is the sole remaining
caller of a helper that pure-Rust tests also exercise directly. The other 14
`file_input` tests (JS-shim + native-binding tests) ported to
`V8JsRuntime::new()` + local stubs (`Blob`/`_lumen_*`/`btoa`/`atob`), mirroring
the original rquickjs harness exactly (`webhid.rs`-style bare-context
template, not full `install_dom`).

**Wrong-worktree-path near-miss:** mid-batch, all three file edits were made
via the repo-root absolute path (`D:\RustProjects\lumen-browser\crates\js\...`)
instead of the `p1-work` pool-slot path
(`.claude\worktrees\p1-work\crates\js\...`) — both resolve to real files since
the root working tree is itself a live `git worktree` (on `main`), so nothing
errored until `git status` on root showed the 3 files dirty on `main`. Caught
before commit: saved the diff (`git diff -- <3 files> > patch`), `git checkout
--` reverted root to clean `main`, then `git apply --directory=.claude/
worktrees/p1-work patch` replayed the same change onto the correct branch.
Costs one extra clippy+test round-trip if caught late (as here) since the
first gate run silently validated the *unedited* root files, not the intended
change — matches `feedback_worktree_edit_absolute_path` from prior sessions;
the fix is to prefix every edit-tool path with the pool-slot worktree root
explicitly, not just `cd` there for Bash commands.

Removed: 2 rquickjs `install_*_bindings` functions (`xhr::install_xhr_bindings`,
`file_input::install_file_input_bindings`), `file_input`'s standalone
`install_native_bindings` helper, `use rquickjs::…` from both module files, and
both calls from `QuickJsRuntime::install_dom` (`lib.rs`). `XHR_SHIM` gated
`#[cfg(feature = "v8-backend")]` (only the V8 path reads it now); `FILE_INPUT_SHIM`
likewise. `register_file_token`/`clear_file_registry` stay ungated — shell and
`filesystem_access.rs` call them regardless of engine.

`cargo test -p lumen-js --features v8-backend`: 2584 lib + 68 integration
(unchanged — both modules' tests already ran under `--features v8-backend`
before this batch via `#[cfg(test)]`; converting them to `V8JsRuntime` ports
them in place without changing the total). `cargo test -p lumen-js` (default):
787 lib (down from 821, -34: the 17+17 tests moved to V8-only — `register_file_
token_unique` is the one `file_input` test that stayed) + 1 integration
(unchanged). rquickjs suite did not go red (787+1 passed). Both clippy passes
clean.

Полоса 2 (S12b-B7…B14) is now complete — 8/8 batches, all medium modules
migrated.

### S12b-B15 (`web_codecs`, `decorators` — Полоса 3 first batch)

First batch of Полоса 3 (large modules). Both modules already carried a
finished V8 port from the S5-S7 hand-port sweep — `install_webcodecs_bindings_v8`
(`web_codecs.rs`, wired via plain `install_v8!` in `v8_runtime.rs:4283`) and
`install_decorator_shim_v8`/`maybe_transform_decorators_v8` (`decorators.rs`,
wired at `v8_runtime.rs:4196` + the eval-hook call site `v8_runtime.rs:4629`)
— so this batch was pure group-A removal, no new porting work. No tests hid
in `dom.rs` or `crates/js/tests/cases/*.rs` (grepped both).

`web_codecs` (8 tests): removed the rquickjs `install_webcodecs_bindings` and
`use rquickjs::Ctx`; ported the 8 `#[cfg(test)]` tests to
`#[cfg(all(test, feature = "v8-backend"))]` against a bare `V8JsRuntime::new()`
plus a minimal hand-rolled `DOMException` stub (the shim's error classes
`extends DOMException`) — the `webhid.rs` bare-context template, not full
`install_dom`.

`decorators` (10 tests): removed the rquickjs `install_decorator_shim`,
`maybe_transform_decorators`, and `use rquickjs::{Ctx, Function}`; gated the
now-V8-only `DECORATOR_SHIM` const `#[cfg(feature = "v8-backend")]` (matches
B14's `XHR_SHIM` precedent — once the only caller is v8-gated, the const goes
dead under the default build without the same gate). Ported all 10 tests to
`V8JsRuntime::new()` (no DOM needed — the shim only touches `Symbol`/
`Object`/`Error`, all native V8 builtins).

Two extra rquickjs call-site removals outside the two module files, both in
`lib.rs`'s `QuickJsRuntime::install_dom`: `web_codecs::install_webcodecs_bindings`
was called **twice** in the same closure (once after `media_devices`, once
again ~170 lines later after `webgpu` — a pre-existing duplicate-install
quirk, not introduced by this batch; both call sites removed since the
function no longer exists). `decorators::install_decorator_shim` had one call
site (removed). `decorators::maybe_transform_decorators` had two call sites
outside `install_dom` — `QuickJsRuntime::eval_module` and the `JsRuntime for
QuickJsRuntime` `eval()` impl — both removed; the QuickJS eval path no longer
pre-processes `@decorator` syntax (consistent with the shim itself no longer
being installed under QuickJS).

`cargo test -p lumen-js --features v8-backend`: 2584 lib + 68 integration
(unchanged — both modules' tests already ran under `--features v8-backend`
before this batch via `#[cfg(test)]`; converting them to `V8JsRuntime` ports
them in place without changing the total). `cargo test -p lumen-js` (default):
769 lib (down from 787, -18: the 8+10 tests moved to V8-only) + 1 integration
(unchanged). Both clippy passes (`--all-targets`, default and
`--features v8-backend`) clean.

Next in queue: S12b-B16 (`intl_bindings`/`media_devices`, Полоса 3 second
batch, large modules).

### S12b-B16 (`intl_bindings`, `media_devices` — Полоса 3 second batch)

Both modules already carried a finished V8 port from S5-S7
(`install_intl_bindings_v8`/`install_media_devices_bindings_v8`, wired via
`install_v8!` in `v8_runtime.rs:4219`/`4225`) — removed the rquickjs
`install_intl_bindings`/`install_media_devices_bindings` fns, `use
rquickjs::Ctx`, and the two call sites in `lib.rs`'s `QuickJsRuntime::
install_dom`. Gated the now-V8-only `INTL_SHIM`/`MEDIA_DEVICES_SHIM` consts
`#[cfg(feature = "v8-backend")]` (B14/B15 `XHR_SHIM`/`DECORATOR_SHIM`
precedent). No hidden tests in `crates/js/tests/cases/*.rs`.

`intl_bindings` (19 tests) surfaced a real, non-mechanical finding: **V8's
prebuilt binary ships a native, ICU-backed `Intl`** (`typeof Intl.NumberFormat`
is `[native code]` on a bare `V8JsRuntime::new()`, before this module installs
anything). `INTL_SHIM`'s own defer-to-native guard (`if (typeof
global.Intl !== 'undefined' && global.Intl.NumberFormat) return;`) was written
for QuickJS, which never has native `Intl` — under V8 the guard is always
true, so the shim's hand-rolled `resolveLocale` fallback rules are permanently
unreachable; V8's real ICU implementation formats instead. 17/19 ported tests
passed unchanged (the shim was deliberately written to match real CLDR
conventions for `en-US`/`ru-RU`, e.g. non-breaking-space grouping and genitive
month forms, so it agrees with native ICU on well-formed locale tags). 2 tests
failed because they asserted the *shim's own* fallback/negotiation behavior:
requesting the invalid tag `'xx-YY'` returned the OS default locale
(`ru-RU` on this dev machine, not `'en-US'`) — machine-dependent, not safe to
hard-code — and `supportedLocalesOf(['en-US','fr-FR','ru-RU'])` returned all 3
(native ICU recognizes `fr-FR`; the shim's narrow en/ru polyfill would have
filtered it out). Rewrote both as `resolved_options_locale_recognizes_
requested_language`/`supported_locales_of_recognizes_known_locale`, asserting
only invariants true for both native ICU and the shim (see the in-test
comments). Net effect: no engine-agnostic behavior regressed — pages still get
a real `Intl`, now spec-accurate instead of a 2-locale approximation — but the
test suite had encoded shim-specific behavior as if it were the contract.

`media_devices` (24 tests): mechanical port, no surprises — tests used only
`navigator`/`window`/`DOMException` stub prereqs (adapted `install_prereqs` to
take `&V8JsRuntime` and drop the QuickJS-only `var Promise = globalThis.
Promise;` line, since V8's `Promise` is already the global one).

`cargo test -p lumen-js --features v8-backend`: 2584 lib (unchanged — both
modules' tests already ran under `--features v8-backend` before this batch)
+ 68 integration (unchanged). `cargo test -p lumen-js` (default): 726 lib
(down from 769, -43: the 19+24 tests moved to V8-only) + 1 integration
(unchanged). Both clippy passes (`--all-targets`, default and
`--features v8-backend`) clean.

Next in queue: S12b-B17 (`wasm/mod`/`sw_worker`, Полоса 3 third batch).

### S12b-B17 (`wasm/mod`, `sw_worker` — Полоса 3 third batch)

Unlike B14-B16, both modules are native Rust↔JS bridges (not JS-string
shims), and both already had a complete V8 port from earlier slices:
`wasm::v8_bridge` (S9) for the WASM interpreter's host-import/memory bridge,
and `sw_worker::spawn_sw_worker_v8`/`install_sw_globals_v8` (S10) for the
Service Worker execution thread. The batch reduced to deleting the rquickjs
twins and `#[cfg(feature = "v8-backend")]`-gating what became V8-only
(`WEBASSEMBLY_SHIM`, `wasm::{value_to_f64,f64_to_value,coerce_value}`,
`sw_globals_shim`, `base64_encode`/`base64_decode` — B14/B15 precedent).

`wasm::mod.rs`: removed the QuickJS `instantiate`/`JsHost`/`call_typed`/
`func_signature`/`mem_*`/`global_*`/`wasm_value_to_js`/`js_value_to_wasm`/
`js_value_to_i64`/`js_value_to_f64` and the `Persistent<Function>`-keyed
`InstanceEntry`/`instances` half of the top-level `Registry` — it now holds
only the `modules` cache, shared by both backends via `with_module`/
`compile`. `webassembly.rs`: removed `install_webassembly_bindings` and its
six `wasm_*_native` free functions plus `install_native_bindings`; the
`WEBASSEMBLY_SHIM` JS string (backend-agnostic) is unchanged and now only
`eval`'d by `install_webassembly_bindings_v8`.

`dom.rs`'s `install_primitives` (rquickjs DOM install, itself untouched —
scoped for S12b-F3) registers `_lumen_sw_activate_script` directly as a
closure spawning `sw_worker::spawn_sw_worker`; since that rquickjs function
is gone, the registration became a no-op (SW fetch interception is
unavailable under the QuickJS rollback path — not a regression to fix, since
QuickJS is a decaying opt-in path, not a target for new work). The V8 side
(`v8_runtime.rs`) already had its own independent `_lumen_sw_activate_script`
registration calling `spawn_sw_worker_v8` since S10 — unaffected.

Porting the 13 `webassembly.rs` QuickJS tests (`mod tests`) surfaced 11
without a V8 twin (2 — `instantiate_and_call_add`, `i64_import_arg_and_
result_use_bigint` — already had one from S9) and, in porting them, two real
pre-existing V8-backend bugs neither prior slice's minimal 2-test coverage
had exercised:

1. `v8_compat.rs`'s `v8_to_jsvalue` (the generic argument converter every
   `into_v8_fnN` native goes through) didn't recognize a `Uint8Array` as a
   byte sequence — it fell through to the generic `is_object()` branch,
   which builds a `JsValue::Object` from the typed array's own property
   names, and `Vec<u8>::from_js_value`/`array_from_js_value` don't know how
   to unwrap an object into a byte vec (single-element-vec fallback → "arg[2]:
   expected number"). Any native taking `Vec<u8>` from a JS `Uint8Array`
   argument was affected, not just WASM — fixed once, generically, by adding
   a `val.is_uint8_array()` branch that reads the bytes via
   `Uint8Array::copy_contents` before the `is_object()` fallback.
2. `__lumen_wasm_mem_buffer` was registered through the generic
   `into_v8_fn1`/`Vec<u8>` path, which returns a plain `JsValue::Array` — not
   a real `ArrayBuffer`. The shim assigns this return value directly to
   `mem._buf` (the JS `Memory.buffer` backing) and relies on `new
   Int32Array(mem._buf)`/`new Uint8Array(mem._buf)` sharing storage with it
   (U-4b's whole point); a plain array has no byte-level storage to alias, so
   every HEAP-view coherence test (write-then-read-back, JS-write-visible-to-
   WASM, buffer-identity-across-calls, grow) silently no-op'd. Fixed by
   giving `__lumen_wasm_mem_buffer` its own scoped native
   (`wasm_mem_buffer_native_v8`) that builds a real `v8::ArrayBuffer` via
   `ArrayBuffer::new` + writing through its `BackingStore`'s `&[Cell<u8>]`
   view — mirroring the removed rquickjs original's `ArrayBuffer::new_copy`.

`cargo test -p lumen-js --features v8-backend`: 2579 lib (down from 2584,
-5: the old 16 QuickJS-only wasm/sw_worker tests are gone, the V8-only tests
grew from 5 to 16 — 13 webassembly `tests_v8` + 3 sw_worker `tests_v8`,
unchanged). `cargo test -p lumen-js` (default): 710 lib (down from 726, -16:
the 13+3 QuickJS-only tests removed, nothing added). Both clippy passes
(`--all-targets`, default and `--features v8-backend`) clean; `lumen-shell`
checked clean under default (`v8`), `quickjs` (both default features on,
`quickjs` wins per the `#[cfg]` priority), and
`--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
(pure rollback build).

Next in queue: S12b-B18 (`es2026_proposals`/`shared_worker`, Полоса 3 fourth
batch).

### S12b-B18 (`es2026_proposals`, `shared_worker` — Полоса 3 fourth batch)

Both modules already had a complete V8 port from earlier slices
(`install_es2026_proposals_v8` since S5-S7, `install_shared_worker_bindings_v8`/
`HUB_V8` since S10), so the batch reduced to deleting the rquickjs twins and
porting the tests that only existed as rquickjs `mod tests`.

`es2026_proposals.rs`: removed `install_es2026_proposals(ctx: &Ctx)` and the
`rquickjs::Ctx` import; `FLOAT16_SHIM`/`DISPOSABLE_STACK_SHIM` (both
backend-agnostic strings) are now `#[cfg(feature = "v8-backend")]`-gated since
only the V8 installer evaluates them. `shared_worker.rs`: removed `HUB`,
`hub()`, `connect_shared_worker`, `post_to_shared_worker`,
`close_shared_worker_port`, `install_shared_worker_bindings`,
`run_shared_worker_thread`, `install_shared_worker_globals`, and the
`rquickjs::{Context, Function, Runtime}` import; `SwInMsg`, `SharedWorkerThread`,
and `PORT_COUNTER` — private types with no remaining unconditional caller —
moved behind `#[cfg(feature = "v8-backend")]` alongside their V8 twins
(precedent: B17's `WEBASSEMBLY_SHIM`). `SHARED_WORKER_GLOBAL_SHIM`/
`SHARED_WORKER_SHIM` gated the same way. `lib.rs`'s `install_dom` (QuickJsRuntime)
call sites for both modules were deleted outright (not turned into no-op
closures — B17's `_lumen_sw_activate_script` no-op precedent only applies to
native-binding registrations the JS shim still calls by name; these two are
plain top-level installer calls with no such contract).

Test porting: `es2026_proposals.rs`'s 15 rquickjs tests → `tests_v8`
(1:1 port, `V8JsRuntime`/`JsValue` assertions). `shared_worker.rs`'s 6 rquickjs
tests → 3 new `tests_v8` twins (`v8_port_is_messageport_like`,
`v8_distinct_names_are_isolated`, `v8_drain_messages_empties_outbox`); the
other 3 already had V8 twins from S10. The async-disposal test
(`v8_async_disposable_stack_dispose_async`) relies on V8's default
"auto-run-microtasks-after-each-script" behaviour (documented at
`v8_runtime.rs`'s `_lumen_drain_microtasks` stub): a single `eval()` call is
enough to drain the whole `Promise.resolve().then(...)` chain inside
`disposeAsync()`, so the test can assert the final LIFO-ordered log
(`sync,async`) synchronously instead of polling — no bugs found in this
batch.

`cargo test -p lumen-js --features v8-backend`: 2576 lib (down from 2579, -3:
21 QuickJS-only tests removed, 21 V8 tests present — but 3 of those already
existed pre-batch, so net new is 21-3=18 minus 21 removed = -3). `cargo test
-p lumen-js` (default): 689 lib (down from 710, -21: all 21 QuickJS-only
tests from these two modules removed, nothing added). Both clippy passes
(`--all-targets`, default and `--features v8-backend`) clean; `lumen-shell`
checked clean under default (`v8`), `quickjs` (both default features on,
`quickjs` wins per the `#[cfg]` priority), and
`--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
(pure rollback build).

### S12b-B19 (`notifications_bindings`, `web_audio` — Полоса 3 fifth batch)

Both modules already had a complete V8 port from earlier slices
(`install_notifications_bindings_v8` since S5-S7 batch 3, `install_web_audio_api_v8`
since S5-S7 batch 2), so the batch reduced to deleting the rquickjs twins and
porting the tests that only existed as rquickjs `mod tests`.

`notifications_bindings.rs`: removed `install_notifications_bindings(ctx: &Ctx,
queue: NotificationQueue, allow: bool)`; `NOTIFICATIONS_SHIM` (backend-agnostic
string) is now `#[cfg(feature = "v8-backend")]`-gated since only the V8
installer evaluates it. `web_audio.rs`: removed `install_web_audio_api(ctx:
&Ctx)` and the `rquickjs::Ctx` import; `WEB_AUDIO_SHIM` gated the same way.
`lib.rs`'s `install_dom` (QuickJsRuntime) call sites for both modules had
already been deleted in an earlier, uncommitted pass in this worktree slot —
this batch's session picked up from that partial state and finished the test
port.

Test porting: `notifications_bindings.rs`'s 26 rquickjs tests → `tests_v8`
(1:1 port, `V8JsRuntime`/`JsValue` assertions, using the runtime's own
`take_notification_requests()` instead of the old free-standing
`drain_notifications(&queue)` call). `web_audio.rs`'s 12 rquickjs tests (13
counting the module doc's stale count) → `tests_v8`, 1:1 port. 4 of the 26
notifications tests initially failed: they read a variable written inside a
`Notification.requestPermission().then(...)` / `getNotifications().then(...)`
callback in the *same* `eval()` call that scheduled it — under the old
rquickjs test harness this worked because `setup_dom_stubs` installed a
synchronous Promise stub (executor runs immediately, `.then()` fires inline),
but real V8 Promises run `.then()` as a genuine microtask that hasn't fired
yet when that same `eval()` call returns its final expression. Fixed by
splitting each into a setup `eval()` + a separate read `eval()` — V8
auto-drains pending microtasks between distinct `eval()` calls (same
precedent as B18's `disposeAsync` test, also documented in
`idle_detection.rs`/`shared_storage.rs`/`ua_client_hints.rs`). No bugs found
in the V8 bridge itself — both modules are pure-JS shims aside from the
trivial `_lumen_audio_tick_time` no-op and the three notification natives,
all of which were already on V8.

`cargo test -p lumen-js --features v8-backend`: 2576 lib (unchanged — both
batches' tests were already counted in the running total after S5-S7).
`cargo test -p lumen-js` (default): 650 lib (down from 689, -39: all 39
QuickJS-only tests from these two modules removed, nothing added). Both
clippy passes (`--all-targets`, default and `--features v8-backend`) clean;
`lumen-shell` checked clean under default (`v8`), `quickjs` (both default
features on, `quickjs` wins per the `#[cfg]` priority), and
`--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
(pure rollback build).

### S12b-B20 (`filesystem_access` — Полоса 3 sixth batch)

The module already had a complete V8 port from an earlier slice
(`install_filesystem_access_v8` since S5-S7 batch 2), so the batch reduced to
deleting the rquickjs `install_filesystem_access` twin, its `lib.rs`
`install_dom` call site, and porting its 33 tests (28 JS-shim tests + 5
pure-Rust registry/JSON-helper unit tests) to V8.

Because everything left in the module — the write/dir registries, the OS
file-dialog spawners, and the JSON helpers — is now reachable only through
`install_filesystem_access_v8`, they were individually gated behind
`#[cfg(feature = "v8-backend")]` (combined with the existing per-OS
`target_os` cfgs on the picker functions) rather than left unconditional;
otherwise a default (non-v8) build would trip `dead_code` under
`clippy --all-targets -D warnings`, since nothing outside the gated installer
calls them anymore. The 5 pure-Rust tests (`writable_write_accumulates`,
`writable_close_writes_file`, `json_escape_quotes`, `json_escape_backslash`,
`file_entry_json_for_existing_file`) moved into `tests_v8` alongside the
JS-shim tests for the same reason — the functions they exercise no longer
exist without the feature.

Porting surfaced a genuine engine-behavior hazard, not a bug in the bridge:
`showOpenFilePicker()`/`showSaveFilePicker()`/`showDirectoryPicker()` each
call `Promise.resolve().then(fn)` where `fn` invokes the real native picker
(a blocking OS dialog — PowerShell `System.Windows.Forms` on Windows). Under
the old rquickjs test harness this was safe because that engine's `eval()`
never auto-ran pending jobs (same root cause as the two-eval-call pattern
from S12b-B8/B19), so the `.then` callback simply never fired during a test.
Under V8, `eval()` drains the microtask queue before returning — even for a
single top-level statement that only checks `typeof ...().then === 'function'`
— so calling any of the three picker functions inside a test pops a real,
blocking native dialog. Fixed by having `tests_v8::with_fsa()` override the
three `_lumen_show_*_picker` natives with synchronous, non-blocking JS mocks
that resolve the promise with a canned payload instead of touching the OS.

`cargo test -p lumen-js --features v8-backend`: 2576 lib (unchanged — the 33
new `tests_v8` tests exactly replace the 33 old rquickjs `mod tests` tests,
which were unconditionally compiled under `#[cfg(test)]` regardless of
features and so were already counted in the v8-backend total before this
batch). `cargo test -p lumen-js` (default): 617 lib (down from 650, -33: all
33 rquickjs-only tests removed, nothing added since `tests_v8` requires
`v8-backend`). Both clippy passes (`--all-targets`, default and
`--features v8-backend`) clean; `lumen-shell` checked clean under default
(`v8`), `quickjs` (both default features on, `quickjs` wins per the `#[cfg]`
priority), and `--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
(pure rollback build).

Next in queue: S12b-B21 (`webgl_canvas`, `audio_element`, Полоса 3 seventh batch).

---

### S12b-B21 (`webgl_canvas`, `audio_element` — Полоса 3 seventh batch)

Both modules already had complete V8 ports from an earlier slice
(`install_webgl_canvas_v8`, `install_audio_element_bindings_v8` since S8 /
S5-S7 batch 3), so the batch reduced to deleting the two rquickjs twins
(`install_webgl_canvas`, `install_audio_element_bindings` +
`install_native_bindings`), their `lib.rs` `install_dom` call sites, and
porting their 31 tests (13 `webgl_canvas` + 18 `audio_element`, the latter
including the one pure-Rust `set_provider_function_exists` check) to V8.

In `webgl_canvas.rs`, everything reachable only through
`install_webgl_canvas_v8` (the `CONTEXTS`/`NEXT_ID` thread-local registry,
`with_ctx`, `WEBGL_SHIM`) was gated behind `#[cfg(feature = "v8-backend")]`
to avoid `dead_code` under a default (non-v8) build; the `pub use webgl::{...}`
backend-constant re-export stayed unconditional since it's a `pub` item with
no other caller either way. In `audio_element.rs`, `get_provider` (the only
caller of which was the deleted rquickjs installer plus the still-gated V8
one) got the same treatment; `provider_lock`/`set_audio_playback_provider`
stayed unconditional — the shell calls the latter before either engine's
runtime exists.

No engine-behavior hazards this batch (unlike B19/B20's microtask-draining
picker/promise findings) — neither module drives a blocking native dialog or
depends on cross-eval promise resolution order. One adaptation: the V8 test
helper for `audio_element` (`with_audio()`) installs the DOM stub *before*
calling `install_audio_element_bindings_v8`, not after like the old rquickjs
`install_all()` did — the V8 installer registers natives and evals the shim
in one call, and the shim's `document`-interception (createElement patch,
`new Audio()` global) only runs when `document` already exists, so the two
calls can no longer be split.

`cargo test -p lumen-js --features v8-backend`: 2576 lib (unchanged — the 31
new `tests_v8` tests exactly replace the 31 old rquickjs `mod tests` tests).
`cargo test -p lumen-js` (default): 586 lib (down from 617, -31). Both
clippy passes (`--all-targets`, default and `--features v8-backend`) clean;
`lumen-shell` checked clean under default (`v8`),
`--no-default-features --features backend-femtovg,quickjs,v8`, and
`--no-default-features --features backend-femtovg,quickjs` (pure rollback
build); `--dump-layout samples/page.html` runs clean under the default build.

Next in queue: S12b-B22 (`video_bindings`, `webassembly`, Полоса 3 eighth batch).

### S12b-B22: video_bindings, webassembly (Полоса 3, 8/12)

`webassembly.rs` had **no rquickjs code left to remove** — its rquickjs twin
(`install_webassembly_bindings`) and all call sites were already deleted in
S12b-B17, with full `tests_v8` parity (13 tests) landed then too. This batch
found that ahead-of-schedule state, verified it (`grep rquickjs
webassembly.rs` — zero hits outside two historical doc-comment mentions), and
left it untouched.

`video_bindings.rs` had the batch's only real work: its V8 port
(`install_video_bindings_v8`) already existed (Ph3 S5-S7 batch 3), so this
reduced to deleting `install_video_bindings`/`install_native_bindings`
(rquickjs, ~250 lines) + the `use rquickjs::{Ctx, Function, Object}` import,
one call site in `lib.rs` (`QuickJsRuntime::new()`, "Install HTMLVideoElement
stubs" block), and porting its 12 `mod tests` to a new `tests_v8` module
verbatim (same helper-fn pattern as B21's `with_audio()`: build the runtime,
install the minimal `document` stub, then call the V8 installer — order
doesn't matter here since, unlike `audio_element`'s `new Audio()` global, the
video shim only patches existing `<video>` elements + intercepts future
`document.createElement`, no global constructor). `get_text_track_store`/
`get_video_gif_store` imports and the `VIDEO_SHIM` const, now reachable only
from the `#[cfg(feature = "v8-backend")]` installer, were gated the same way
to keep the default (no-`v8-backend`) build warning-free.

No bridge bugs found. `cargo test -p lumen-js --features v8-backend`: 2576
lib (unchanged — 12 new `tests_v8` tests exactly replace the 12 old rquickjs
`mod tests` tests; `webassembly.rs` contributed nothing new since B17).
`cargo test -p lumen-js` (default): 574 lib (down from 586, -12). Both
clippy passes (default and `--features v8-backend`) clean; `lumen-shell`
checked clean under default (`v8`) and `--features quickjs` (rollback build).

Next in queue: S12b-B23 (`dom_parser`, Полоса 3 ninth batch).

### S12b-B23: dom_parser (Полоса 3, 9/12)

`dom_parser.rs`'s V8 port (`install_dom_parser_v8`) already existed (Ph3
S5-S7): the DOMParser/XMLSerializer shim is engine-agnostic pure-JS (a
tokenizer + virtual DOM + CSS selector engine building on `_lumen_get_attr*`/
`_lumen_get_children` natives from `dom.rs`), so this batch reduced to
deleting `install_dom_parser` (rquickjs, the one-line `ctx.eval` wrapper) +
its `use rquickjs::Ctx` import, one call site in `lib.rs`
(`QuickJsRuntime::new()`, the "W3C DOM Parsing and Serialization" block —
its stale comment claiming `svg::install_svg_bindings` "must come after
dom_parser" was also dropped, since dom_parser is no longer installed on
this path at all), and porting all 19 `mod tests` to a new `tests_v8` module
(same helper-fn pattern as B20-B22: `setup()` builds the runtime + minimal
`window`/`navigator`/`document` stubs then calls the V8 installer;
`bool_eval`/`string_eval`/`number_eval` wrap `JsValue` match arms since this
module's tests read strings and array lengths, not just booleans). The
`DOM_PARSER_SHIM` const, now reachable only from the
`#[cfg(feature = "v8-backend")]` installer, was gated the same way to keep
the default (no-`v8-backend`) build warning-free.

No bridge bugs found. `cargo test -p lumen-js --features v8-backend`: 2576
lib (unchanged — 19 new `tests_v8` tests exactly replace the 19 old rquickjs
`mod tests` tests). `cargo test -p lumen-js` (default): 555 lib (down from
574, -19). Both clippy passes (default and `--features v8-backend`) clean;
`lumen-shell` checked clean under default (`v8`),
`--no-default-features --features backend-femtovg,quickjs,v8`, and
`--no-default-features --features backend-femtovg,quickjs` (pure rollback
build); `--dump-layout samples/page.html` runs clean under the default build.

Next in queue: S12b-B24 (`temporal_api`, Полоса 3 tenth batch).

### S12b-B24: temporal_api (Полоса 3, 10/12)

`temporal_api.rs`'s V8 port (`install_temporal_api_v8`) already existed
(Ph3 S5-S7) — the TC39 Temporal shim (`Temporal.PlainDate`/`Instant`/
`Duration`/etc, pure JS, no native bindings) — so this batch reduced to
deleting `install_temporal_api` (rquickjs, the one-line `ctx.eval` wrapper) +
`use rquickjs::Ctx`, one call site in `lib.rs` (`QuickJsRuntime::install_dom`,
the "Install TC39 Temporal API shim" block), and porting all 30 `mod tests`
to a new `tests_v8` module (same helper-fn pattern as B20-B23).

**Bridge finding, not a bug:** V8 ships a native, spec-conformant `Temporal`
implementation. The shim's own defer-to-native guard
(`if (global.Temporal.PlainDate) return;`) always bails under the V8 backend,
so `install_temporal_api_v8` is effectively unreachable dead code today — all
30 ported tests actually exercise native V8 `Temporal`, not `TEMPORAL_SHIM`
(confirmed: `typeof Temporal.PlainDate === 'function'` before the installer
even runs its eval). Two tests assumed shim-only API shape and had to be
rewritten against invariants true of the *reachable* implementation (mirrors
the B16 `Intl.resolveLocale` fallback precedent): `plain_month_day_from_string`
switched from the shim's simplified numeric `.month` to native
`PlainMonthDay`'s real `.monthCode` (string, e.g. `"M06"`) + `.day`;
`timezone_utc_offset` switched from `new Temporal.TimeZone('UTC')` (the
standalone `TimeZone` constructor the shim models, absent from the finalized
spec V8 implements) to `ZonedDateTime.offsetNanoseconds`, which both the shim
and native Temporal support identically. `TEMPORAL_SHIM` gated under
`#[cfg(feature = "v8-backend")]` (same reason as B20-B23: dead_code under the
default no-`v8-backend` build otherwise fails `-D warnings`).

`cargo test -p lumen-js --features v8-backend`: 2576 lib (unchanged — 30 new
`tests_v8` tests exactly replace the 30 old rquickjs `mod tests` tests).
`cargo test -p lumen-js` (default): 525 lib (down from 555, -30). Both
clippy passes (default and `--features v8-backend`) clean; `lumen-shell`
checked clean under default (`v8`) and
`--no-default-features --features backend-femtovg,quickjs` (pure rollback
build); `--dump-layout samples/page.html` runs clean under the default build.

Next in queue: S12b-B25 (`svg`, Полоса 3 eleventh batch).

---

### S12b-B25: svg (Полоса 3, 11/12)

`svg.rs`'s V8 port (`install_svg_bindings_v8`) already existed (Ph3 S5-S7) —
SVG DOM stubs (`SVGElement`/`SVGSVGElement` hierarchy, `SVGRect`/`SVGPoint`/
`SVGLength`/`SVGMatrix` value types, `createElementNS` wiring), pure JS, no
native bindings. This batch reduced to deleting `install_svg_bindings`
(rquickjs, the one-line `ctx.eval` wrapper) + `use rquickjs::Ctx`, one call
site in `lib.rs` (`QuickJsRuntime::install_dom`, the "W3C SVG 2" block), and
porting all 20 `mod tests` to a new `tests_v8` module (same helper-fn pattern
as B20-B24). `SVG_SHIM` gated under `#[cfg(feature = "v8-backend")]` (same
reason as B20-B24: dead_code under the default no-`v8-backend` build otherwise
fails `-D warnings`); the leading module doc comment had to switch from `///`
to `//!` in the same edit — clippy's `empty_line_after_doc_comments` fires
once the `///` block is no longer immediately followed by the item it used to
attach to (`install_svg_bindings`, now deleted), since it then reads as
documenting `install_svg_bindings_v8` with a blank line in between.

No bridge bugs found.

`cargo test -p lumen-js --features v8-backend`: 2576 lib (unchanged — 20 new
`tests_v8` tests exactly replace the 20 old rquickjs `mod tests` tests).
`cargo test -p lumen-js` (default): 505 lib (down from 525, -20). Both clippy
passes (default and `--features v8-backend`) clean; `lumen-shell` checked
clean under default (`v8`) and `--no-default-features --features
backend-femtovg,quickjs` (pure rollback build).

Next in queue: S12b-B26 (`offscreen_canvas`, Полоса 3 twelfth batch).

### S12b-B27: worker (+ довершение offscreen_canvas из B26) (Полоса 4, 1/…)

`worker.rs`'s V8 port (`install_worker_bindings_v8`/`spawn_worker_v8`/
`run_worker_thread_v8`/`install_worker_globals_v8`) already existed (Ph3 S10)
as a full standalone twin — unlike most B-batches, the rquickjs and V8 Worker
implementations were two independent per-thread-runtime constructs (each
worker thread owns either a bare `rquickjs::Runtime`/`Context` or a full
[`crate::v8_runtime::V8JsRuntime`]), not a thin shim reused by both. This
batch deleted the rquickjs side wholesale: `spawn_worker`, `run_worker_thread`,
`install_worker_globals`, `install_worker_bindings` (rquickjs) + their call
site in `lib.rs` (`QuickJsRuntime::install_dom`, the "Install Web Worker
bindings" block) + `use rquickjs::{Context, Function, Runtime}`.
`WORKER_SHIM`/`worker_global_shim`/`b64_encode` gated under `#[cfg(feature =
"v8-backend")]` (only consumer left is the V8 installer; same dead_code
rationale as B20-B25). `WorkerRegistry`/`WorkerMessageQueue`/`WorkerBlobStore`/
`WorkerInMsg` and `post_to_worker`/`terminate_worker`/`drain_messages` stayed
unconditional (engine-agnostic plumbing, still read by both `QuickJsRuntime`
and `V8JsRuntime`).

One bridge finding: `QuickJsRuntime`'s own `workers`/`worker_next_id`/
`worker_blob_store` struct fields (in `lib.rs`, separate from
`V8JsRuntime`'s identically-named fields in `v8_runtime.rs`) became
dead — their only writer was the just-deleted `install_worker_bindings`
call site — so `cargo clippy --workspace` flagged them as "field is never
read". Deleted all three; `worker_messages` stays (still read by
`pump_workers()`, which the shell calls unconditionally for both runtimes —
under `QuickJsRuntime` it now always drains empty, since nothing populates
it, matching this batch's intent of dropping the QuickJS Worker
implementation).

Of `worker.rs`'s 21 rquickjs `mod tests`, 8 were pure Rust (b64/percent-decode/
resolve_import_url, no engine) and stayed; the other 13 (JS-shim install,
importScripts variants, structured-clone transfer, end-to-end postMessage)
were deleted after verifying each was already covered, or porting the 6 not
yet covered, in `tests_v8` (7→13 tests: added
`v8_worker_import_scripts_via_blob_url`,
`v8_worker_terminate_stops_message_delivery`, `v8_import_scripts_multiple_urls`,
`v8_import_scripts_unknown_url_throws`,
`v8_serialize_with_no_transfers_is_standard_json`,
`v8_serialize_with_offscreen_canvas_transfer_embeds_sentinel`). The rquickjs
`import_scripts_blob_url` test (direct `install_worker_globals` + blob-store
call, no thread) has no 1:1 V8 counterpart — its code path is exercised
instead by the ported `v8_worker_import_scripts_via_blob_url` end-to-end
thread test, which is a superset (same `resolve_import_url` blob branch, plus
proves the real worker thread runs it), not a coverage gap.

Per the note left in B26, this batch also finished `offscreen_canvas.rs`:
`worker.rs`'s QuickJS worker thread (`run_worker_thread`, its only live
non-test caller) was the reason `install_offscreen_canvas_bindings`
(rquickjs) survived B26; once that caller was deleted above, so was the
installer, its call site in `lib.rs` (`QuickJsRuntime::install_dom`, "Install
OffscreenCanvas bindings" block), and `use rquickjs::Ctx`. Of its 23 rquickjs
`mod tests`, 9 were pure Rust and stayed (including `reset_state`, moved out
of the deleted "Integration tests via JS bindings" section — it only touches
the `OFFSCREEN_CANVASES`/`DIRTY` thread-locals, no rquickjs dependency); the
other 14 were deleted, already fully duplicated in `tests_v8` since B26 (verified
test-for-test, no gaps).

No functional regression under the default (V8) engine: `run_worker_thread_v8`
never installed `OffscreenCanvas` for worker threads even before this batch
(documented gap, P1-imagebitmap) — this batch only removes the *rquickjs*
worker thread's OffscreenCanvas support, which was unreachable under the
default build anyway.

`cargo test -p lumen-js --features v8-backend`: 2569 lib (down from 2590 —
-27 rquickjs tests deleted, +6 new `tests_v8` tests). `cargo test -p lumen-js`
(default, no features): 478 lib (down from 505, -27: the 13+14 rquickjs tests
removed from `worker.rs`/`offscreen_canvas.rs`). Both clippy passes clean
(`cargo clippy --workspace --all-targets -- -D warnings`, which unifies
`v8-backend` in via `lumen-shell`'s dependency edge); `lumen-shell` checked
clean under default (`v8`) and `--no-default-features --features
backend-femtovg,backend-wgpu,quickjs` (pure rollback build).

Next in queue: S12b-B28 (`tc39_proposals`, Полоса 4).

### S12b-B28: `tc39_proposals` (2026-08-04, branch `p1-s12b-b28`)

The module already had a ready V8 port (`install_tc39_proposals_v8`, S5-S7) —
a pure-JS shim for 11 TC39 Stage-4 proposals (`Object.groupBy`/`Map.groupBy`,
Set methods, `Promise.withResolvers`/`Promise.try`, `Array.fromAsync`,
Iterator Helpers, `Uint8Array` Base64/Hex, `RegExp.escape`, `Error.isError`,
`Atomics.pause`/`Atomics.waitAsync`), each section gated by a native-support
check. The batch removed the rquickjs wrapper (`install_tc39_proposals`) +
its `lib.rs` call site and ported all 51 `mod tests` to `tests_v8`, following
the `setup()`/`bool_eval`/`string_eval`/`number_eval` helper pattern from
B24/B25.

**Finding**: `typeof Atomics.waitAsync === 'function'` is already true
natively under V8 (QuickJS never had it), so the shim's own
`typeof Atomics.waitAsync !== 'function'` guard never installs its FIFO
async-waiter bookkeeping under the default engine — that ~100-line branch of
`TC39_PROPOSALS_SHIM` is dead code under V8, mirroring B24's Temporal
finding. Two of the 51 ported tests (`atomics_wait_async_notify_resolves_ok`,
`atomics_wait_async_bigint64_roundtrip`) originally asserted that a
`Atomics.notify()`-driven `res.value.then(...)` had settled by the time a
*second*, separate `eval()` call read the resulting global — the pattern
that reliably observes microtask-queue drains between top-level `eval()`
calls elsewhere in this migration (B18/B19/B26). It does not hold here:
V8's *native* `Atomics.waitAsync` schedules its continuation as a
platform foreground task (not a plain microtask), and this crate's
`V8JsRuntime` never pumps `v8::Platform::PumpMessageLoop` — confirmed with a
throwaway diagnostic test showing even a `setTimeout`-based promise never
settles across two `eval()` calls here (and `setTimeout` itself is only
defined once the DOM shim installs it, not in a bare `V8JsRuntime`). Both
tests were narrowed to assert only the synchronous return values (`res.async`
flag, `Atomics.notify()`'s wake count) computed before any task/microtask
would need to run — still exercises the real API surface, but no longer
depends on task-queue pumping this harness doesn't do. Whether V8's own
native `waitAsync` promise eventually settles is V8's own well-tested
behavior, not code this crate owns.

No bridge bugs found. `cargo test -p lumen-js --features v8-backend`: 2569
lib (unchanged — 51 new `tests_v8` tests exactly replace 51 old rquickjs
`mod tests`). `cargo test -p lumen-js` (default, no features): 427 lib (down
from 478, -51). `cargo clippy --workspace --all-targets -- -D warnings`
clean (unifies `v8-backend` via `lumen-shell`'s dependency edge).

Next in queue: S12b-B29 (`webgpu`, Полоса 4).

### S12b-B29: `webgpu` (2026-08-04, branch `p1-s12b-b29`)

The module already had a ready V8 port (`install_webgpu_bindings_v8`, S9) —
`navigator.gpu` Phase-0 shim shared with the rquickjs twin, plus real-backend
natives (`_lumen_webgpu_*`) gated behind the `webgpu` feature
(`lumen-paint/backend-wgpu`). The batch removed the rquickjs installer
(`install_webgpu_bindings`) and its `lib.rs` call site, and ported all 28
unique tests from the old rquickjs `mod tests` (some of which required the
`webgpu` feature — real compute/render pipeline round-trips against a real
`wgpu` device) into `tests_v8`, replacing the old 1-test `tests_v8` smoke
skeleton.

No bridge bugs found — the V8 real-backend tests (`v8_real_backend_*`,
gated `#[cfg(feature = "webgpu")]`) pass unmodified against the actual wgpu
device, same as the rquickjs versions they replace.

`cargo test -p lumen-js` (default, no features): 402 lib (down from 427,
-25 — only the 25 non-webgpu-gated rquickjs tests counted here; the 3
`real_backend_*` tests were already `#[cfg(feature = "webgpu")]`-gated out
of a plain default build). `cargo test -p lumen-js --features v8-backend`:
2568 lib (down from 2569, -1 — replaces 26 old tests [25 rquickjs +
1 old tests_v8 skeleton] with 25 new `tests_v8`, since the 3 real-backend
tests need `webgpu` too). `cargo test -p lumen-js --features
v8-backend,webgpu`: 2571 lib, all pass, including the 28/28 module tests
(module count net -1: 29 old tests [28 rquickjs + 1 skeleton] → 28 new).
`cargo clippy -p lumen-js --all-targets --features v8-backend,webgpu -- -D
warnings` clean. `lumen-shell` checked under default (wgpu+v8) and rollback
(`backend-femtovg,backend-wgpu,quickjs`) — both green; the rollback build no
longer installs any WebGPU shim into the QuickJS runtime (rquickjs installer
is gone), matching the pattern of prior S12b batches removing engine-specific
surface from the rollback path one module at a time.

Next in queue: S12b-B30 (`canvas2d`, Полоса 4).

### S12b-B30: `canvas2d` (2026-08-04, branch `p1-s12b-b30`)

The module already had a ready V8 port (`install_canvas2d_bindings_v8`, S8) —
all state (`CANVASES`/`DIRTY`/`GRADIENTS`/`PATTERNS`/`PATHS`/`TRANSFERRED`) is
module-level `thread_local!`, not a `V8JsRuntime` field, so no new runtime
plumbing was needed. The batch removed the rquickjs installer
(`install_canvas2d_bindings`, ~1000 lines) and its `lib.rs` call site, and
ported 26 of the 31 old rquickjs tests into a new `tests_v8` module. The other
5 (`parse_canvas_font_size`/`measure_text_width` × 3 + `present_rgba_writes_
pixels_and_marks_dirty`) are pure Rust with no JS engine involved at all —
kept once in the plain `mod tests`, matching the `offscreen_canvas` (S12b-B26)
precedent of not duplicating engine-agnostic coverage.

No bridge bugs found. Two porting notes:

1. `V8JsRuntime::new()` spawns a dedicated OS thread per runtime, so a bare
   rquickjs-style peek at `CANVASES`/`DIRTY` from the test's own thread would
   read an empty, unrelated `thread_local!` instance. Dirty-buffer assertions
   route through the already-public `V8JsRuntime::flush_canvas_updates()`
   (added earlier for the shell's per-frame canvas upload). Assertions with no
   JS-visible getter native (`line_width`, `global_alpha`, `text_align`,
   `text_baseline` — the clamp/store logic lives in the native itself, not the
   JS shim) needed a new escape hatch: `V8JsRuntime::run_for_test` (`#[cfg(
   test)]`, `v8_runtime.rs`), a generic "run this closure on the JS thread"
   helper mirroring `flush_canvas_updates`'s pattern but not tied to one field.
2. Removing the rquickjs installer left most of `canvas2d.rs`'s private
   helpers (`with_canvas`, `bitmaprenderer_transfer_native`, gradient/pattern/
   path registries and their id counters, hex decoding, `render_text_to_
   canvas`) reachable *only* from the now-`v8-backend`-gated
   `install_canvas2d_bindings_v8` — under a hypothetical build with neither
   `quickjs` nor `v8-backend`, they're dead code. Gated all of them (and their
   now-conditionally-used imports/`thread_local!` entries) under `#[cfg(
   feature = "v8-backend")]` to keep `cargo clippy -p lumen-js --all-targets
   -- -D warnings` (no features) clean *for this module*. Three items
   (`BUNDLED_FONT`, `parse_canvas_font_size`, `measure_text_width`) can't be
   gated the same way — they're needed unconditionally by the pure-Rust tests
   — so they stay flagged dead under that exact no-features build, same as
   `offscreen_canvas.rs`'s own `with_offscreen_canvas` already is (verified:
   11 pre-existing errors on `main` before this batch, all in `offscreen_
   canvas.rs`/`worker.rs`, none of them fixed by their own S12b-B26/B27
   batches). This whole "no-features" build config is a temporary artifact of
   the two-backend transition and disappears once S12b-F1..F4 drop the
   `quickjs` feature entirely — not something this batch can fully close.

`cargo test -p lumen-js` (default, no features): 376 lib (down from 402,
-26 — the 26 ported rquickjs tests are gone, the 5 pure ones stay).
`cargo test -p lumen-js --features v8-backend`: 2568 lib (unchanged — 31
old tests [rquickjs, always compiled] become 31 new [5 pure + 26 `tests_v8`],
net zero). `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` clean. `cargo clippy -p lumen-js --all-targets -- -D warnings`
(no features): pre-existing 11 baseline errors (`offscreen_canvas.rs`/
`worker.rs`) plus the 3 unavoidable canvas2d items noted above — no new
category introduced. `lumen-shell` checked under default (wgpu+v8) and
rollback (`backend-femtovg,quickjs`) — both green; the rollback build no
longer installs Canvas 2D bindings into the QuickJS runtime.

Next in queue: S12b-B31 (`subtle_crypto`, Полоса 4, final batch of group A).

### S12b-B31: `subtle_crypto` (2026-08-04, branch `p1-s12b-b31`)

Final batch of group A. The V8 port already existed (`v8_runtime.rs:3818+`,
wrapper-only `reg!` block calling `crate::subtle_crypto::*` directly, landed in
the fifteenth S12b-24 porting slice). This batch removed the rquickjs
installer (`install_subtle_bindings`, the `Ctx`/`Function` reg! block) and its
`install_primitives` call site in `dom.rs`.

Unlike every prior batch, none of the 39 tests needed porting: they're pure
Rust, calling `generate_key`/`sign_data`/`aes_gcm_encrypt`/etc. directly with
no `Ctx`, no `eval()`, no JS engine at all — the same "already engine-agnostic"
shape `offscreen_canvas` (S12b-B26) found for a handful of its own tests, just
covering the *entire* module here instead of a handful of helpers. No bridge
bugs found.

Removing the installer left the whole implementation (key registry, base64url/
JSON helpers, all 14 `generate_key`/`import_key`/`export_key`/`sign_data`/
`verify_signature`/`aes_gcm_*`/`aes_cbc_*`/`aes_ctr_crypt`/`derive_bits`/
`rsa_oaep_*`/`key_info` entry points, and every private algorithm helper they
call) reachable only from the now-`v8-backend`-gated `v8_runtime.rs` block —
same "dead under a hypothetical no-features build" shape `canvas2d` (S12b-B30)
hit. Unlike canvas2d, *all* of it is also exercised by the plain-Rust `mod
tests`, so plain `#[cfg(feature = "v8-backend")]` (canvas2d's approach) would
have broken `cargo test -p lumen-js` with no `--features` at all. Gated every
item (imports included — a plain `--all-targets` build with no features
compiles the lib target once *without* `cfg(test)` too, where the same imports
go unused) with `#[cfg(any(feature = "v8-backend", test))]` instead: reachable
whenever either the real V8 caller or the test module is being compiled,
compiled away entirely otherwise. Net effect: `cargo clippy -p lumen-js
--all-targets -- -D warnings` (no features) introduces **zero** new dead-code
errors from this batch — a full close, not just a partial one like B30's 3
leftover items, made possible only because this module's whole surface was
already unit-testable without a JS engine.

`cargo test -p lumen-js` (default, no features): 376 lib (unchanged — no
rquickjs-only tests existed to remove). `cargo test -p lumen-js
--features v8-backend`: 2568 lib (unchanged). `cargo clippy -p lumen-js
--all-targets --features v8-backend -- -D warnings` clean. `cargo clippy
-p lumen-js --all-targets -- -D warnings` (no features): same pre-existing
16 baseline errors (`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs`, unchanged
from before this batch) — zero new. `lumen-shell` checked under default
(wgpu+v8) and rollback (`backend-femtovg,backend-wgpu,quickjs`) — both green;
the rollback build no longer installs SubtleCrypto bindings into the QuickJS
runtime.

This closes group A (all "already-had-a-V8-port, just delete rquickjs" batches,
B5…B31). Next in queue: group G (modules with **no** V8 port at all — G1…G7,
`docs/tasks/p1-s12b-cleanup-queue.md`), starting with S12b-G1 (`contacts`,
`background_sync`).

---

### S12b-G1: `contacts`, `background_sync` (2026-08-04, branch `p1-s12b-g1`)

First group G batch (real port, not deletion — neither module had a `_v8`
variant before this). Both are pure `ctx.eval(SHIM)` Phase-0 stubs with no
native `Function::new` registrations, so the port is the "no natives" fast
path from `p1-s12b-cleanup-queue.md` §4: replace `rquickjs::Ctx::eval` with
`lumen_core::ext::JsRuntime::eval`, gate the install fn + shim const behind
`#[cfg(feature = "v8-backend")]`, register via `install_v8!` in
`v8_runtime.rs::install_dom` (alphabetical slot: `background_sync` before
`badging`, `contacts` between `compute_pressure` and `content_index`), then
remove the rquickjs `init_*` call sites from `lib.rs::install_dom` (`contacts`
was at `lib.rs:772`, `background_sync` at `:793`). `background_sync.rs`
references two natives (`_lumen_sw_sync_register`/`_lumen_sw_get_tags`)
guarded by `typeof … === 'function'` — grep confirms neither is registered
anywhere in the codebase (rquickjs or V8 side), so the guard always takes the
JS-only fallback; ported as-is, no native binding needed. 9 tests (4 contacts
+ 5 background_sync) ported to `#[cfg(all(test, feature = "v8-backend"))]`
against bare `V8JsRuntime::new()` + local stubs (`navigator`/`DOMException`
for contacts, `ServiceWorkerRegistration`/the two native stubs for
background_sync) — no `install_dom` needed, same shape as `badging.rs`'s
template. No bridge bugs found; this closes 2 of the 7 modules tracked by
[BUG-549](../../bugs/BUG-549-OPEN.md) (remaining 5 land in G3/G4 per the
queue doc — bug stays OPEN until all 7 are ported).

`cargo test -p lumen-js --features v8-backend`: 2568→2568 lib total this run
(9 new module tests; full-suite count matches the pre-batch B31 baseline
since these two modules previously contributed 0 V8 tests). `cargo test
-p lumen-js` (default, no features): 376→367 lib (-9, the ported rquickjs
tests removed). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. `cargo clippy -p lumen-js --all-targets --
-D warnings` (no features): same pre-existing baseline errors from
`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs` (B26/B27/B30, unrelated to
this batch) — zero new errors from `contacts.rs`/`background_sync.rs`, since
both the install fn and the shim const are `#[cfg(feature = "v8-backend")]`
and every reference to them (including the `mod tests`) is likewise feature-
gated.

---

### S12b-G2: `periodic_sync`, `storage_buckets` (2026-08-04, branch `p1-s12b-g2`)

Second group G batch, same "no natives" fast path as G1. Both modules are
pure `ctx.eval(SHIM)` Phase-0 stubs — `periodic_sync.rs` references two
natives (`_lumen_periodic_sync_register`/`_lumen_periodic_sync_unregister`)
behind a `typeof … === 'function'` guard that never resolves (grep confirms
neither is registered on either engine), ported as-is; `storage_buckets.rs`
has no natives at all. Port: `rquickjs::Ctx::eval` → `lumen_core::ext::
JsRuntime::eval`, install fn + shim const gated `#[cfg(feature =
"v8-backend")]`, registered via `install_v8!` in `v8_runtime.rs::install_dom`
(`periodic_sync` before `permissions_policy`, `storage_buckets` before
`storage_manager`), rquickjs `init_*` call sites removed from
`lib.rs::install_dom` (`periodic_sync` was at `lib.rs:786`, `storage_buckets`
at `:808`). 12 tests (4 periodic_sync + 8 storage_buckets) ported to
`#[cfg(all(test, feature = "v8-backend"))]` against bare `V8JsRuntime::new()`
— `periodic_sync` with a local `ServiceWorkerRegistration` stub (mirrors
`background_sync`'s G1 harness), `storage_buckets` with no stubs at all since
its existing test suite only exercised `StorageBucketManager` directly
(never through `navigator.storageBuckets`), so the `typeof navigator/window
!== 'undefined'` branches in the shim simply no-op under the bare runtime —
same as before the port. No bridge bugs found. Closes 2 more of the 7 modules
tracked by [BUG-549](../../bugs/BUG-549-OPEN.md) (4/7 done; remaining 3 —
`push_api`/`background_fetch`/`payment_request`/`media_stream_recording` are
G3/G4 — bug stays OPEN until all 7 land). `storage_buckets.rs` is also one of
the two modules named in BUG-547 (CAPABILITIES.md overclaim) — the overclaim
itself is not corrected by this batch, only the underlying V8 gap.

`cargo test -p lumen-js --features v8-backend`: +12 new module tests.
`cargo test -p lumen-js` (default, no features): 367→355 lib (-12, the ported
rquickjs tests removed). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. `cargo clippy -p lumen-js --all-targets --
-D warnings` (no features): identical 18-error baseline to pre-batch main
(`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs`, verified byte-for-byte
against main before this batch) — zero new errors from `periodic_sync.rs`/
`storage_buckets.rs`.

---

### S12b-G3: `push_api`, `background_fetch` (2026-08-04, branch `p1-s12b-g3`)

Third group G batch, same "no natives" fast path as G1/G2. `push_api.rs`
references two natives (`_lumen_push_subscribe`/`_lumen_push_unsubscribe`)
and `background_fetch.rs` three (`_lumen_bg_fetch_register`/`_activate`/
`_abort`), all behind `typeof … === 'function'` guards that never resolve —
grep confirms none of the five is registered on either engine — ported
as-is, no native binding needed. Port: `rquickjs::Ctx::eval` →
`lumen_core::ext::JsRuntime::eval`, install fn + shim const gated
`#[cfg(feature = "v8-backend")]`, registered via `install_v8!` in
`v8_runtime.rs::install_dom` (`background_fetch` before `background_sync`,
`push_api` between `presentation_api` and `reporting_api`), rquickjs `init_*`
call sites removed from `lib.rs::install_dom` (`background_fetch` was at
`lib.rs:779`, `push_api` at `:786`). 13 tests (7 push_api + 6
background_fetch) ported to `#[cfg(all(test, feature = "v8-backend"))]`
against bare `V8JsRuntime::new()` with a local `ServiceWorkerRegistration`
stub + the relevant native no-op stubs — same shape as G1/G2's harnesses. No
bridge bugs found. Closes 2 more of the 7 modules tracked by
[BUG-549](../../bugs/BUG-549-OPEN.md) — with G1's `contacts`/`background_sync`
and G2's `periodic_sync`, that's 5/7 done; remaining 2 —
`payment_request`/`media_stream_recording` — are G4 (bug stays OPEN until
all 7 land).

`cargo test -p lumen-js --features v8-backend`: +13 new module tests, full
suite 2568 passing (13 of those are `push_api`/`background_fetch`). `cargo
test -p lumen-js` (default, no features): 355→342 lib (-13, the ported
rquickjs tests removed). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. `cargo clippy -p lumen-js --all-targets --
-D warnings` (no features): identical 18-error baseline to pre-batch main
(`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs`) — zero new errors from
`push_api.rs`/`background_fetch.rs`.

### S12b-G4: `payment_request`, `media_stream_recording` (2026-08-04, branch `p1-s12b-g4`)

Fourth group G batch, same "no natives" fast path as G1-G3. Neither module
references any native binding — both are pure `ctx.eval(SHIM)`. Port:
`rquickjs::Ctx::eval` → `lumen_core::ext::JsRuntime::eval`, install fn + shim
const gated `#[cfg(feature = "v8-backend")]`, registered via `install_v8!` in
`v8_runtime.rs::install_dom` (`media_stream_recording` between
`media_session` and `navigation_api`; `payment_request` between
`paint_worklet` and `periodic_sync`), rquickjs `init_*` call sites removed
from `lib.rs::install_dom`. 14 tests (6 payment_request + 8
media_stream_recording) ported to `#[cfg(all(test, feature = "v8-backend"))]`
against bare `V8JsRuntime::new()` — `payment_request` needs a `window =
globalThis` alias plus a `DOMException` stub (shim checks `typeof window ===
'undefined'` and constructs `DOMException`), `media_stream_recording` needs
`Blob`/`DOMException`/`Date.now()` stubs (matches the original rquickjs test
harnesses). No bridge bugs found. Closes the last 2 of the 7 modules tracked
by [BUG-549](../../bugs/BUG-549-OPEN.md) — group G's port work is done (bug
resolution/CAPABILITIES.md update deferred to a follow-up, not part of this
batch's scope).

`cargo test -p lumen-js --features v8-backend`: +14 new module tests. `cargo
test -p lumen-js` (default, no features): 342→328 lib (-14, the ported
rquickjs tests removed). `cargo clippy -p lumen-js --all-targets --features
v8-backend -- -D warnings` clean. `cargo clippy -p lumen-js --all-targets --
-D warnings` (no features): identical 18-error baseline to pre-batch main
(`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs`) — zero new errors from
`payment_request.rs`/`media_stream_recording.rs`.

### S12b-G5: `view_transitions`, `cookie_store` (2026-08-04, branch `p1-s12b-g5`)

Fifth group G batch — the two remaining modules before the group's final
slot G6 (`cookie_banner`). Unlike G1-G4, the two modules are not uniform:
`cookie_store` is the familiar no-natives fast path (`_lumen_cookie_store_set`/
`_delete` referenced behind a `typeof` guard, never registered on either
engine, ported as-is: `rquickjs::Ctx::eval` → `lumen_core::ext::JsRuntime::eval`,
`install_cookie_store_v8` wired via the plain `install_v8!` macro between
`content_index` and `credentials`). `view_transitions` is not — it owns 3
real natives (`_lumen_vt_begin`/`_lumen_vt_end`/`_lumen_vt_cancel`) that push
into an `Arc<Mutex<Vec<ViewTransitionEvent>>>` the shell drains every
`about_to_wait` to drive the CSS cross-fade, so the "no natives" fast path
doesn't apply. Ported using the stateful-module pattern established by
`fullscreen_requests`/`pointer_capture_nid`: a new `view_transition_events`
field on `V8JsRuntime` (mirrors `crate::QuickJsRuntime`'s field of the same
name), a `pub fn take_view_transition_events()` accessor mirroring
`take_fullscreen_requests`, and `install_view_transition_bindings_v8(rt,
events)` registering the 3 natives via `V8JsRuntime::register_native` +
`v8_compat::into_v8_fn0` (the helper `register_native` exists precisely for
"standalone module ports that need `Function::new`-style natives", per its
own doc comment) — called as an extra-arg site in `install_dom` (between
`video_pip` and `virtual_keyboard`), the same shape as the `pointer_capture`/
`geolocation` calls, not the bare `install_v8!` macro, because the closures
need the runtime-instance `Arc`, not just `&self`.

Found in the same batch, not part of the original G0 triage write-up: the
shell's V8 wrapper (`V8PersistentJs::take_view_transition_events` in
`crates/shell/src/main.rs`) was a hardcoded `Vec::new()` stub with a comment
("View Transitions bindings not ported to V8 yet") — the second half of
BUG-545. Fixed by routing it through the new `V8JsRuntime::take_view_transition_events()`,
mirroring the QuickJS wrapper's implementation one function above it.

19 tests ported to `#[cfg(all(test, feature = "v8-backend"))]`: 11
`view_transitions` (harness: bare `V8JsRuntime::new()` + `var document = {};`
+ `install_view_transition_bindings_v8(&rt, Arc::clone(&events))`, `events`
returned alongside `rt` so tests can inspect the queue directly instead of
going through a drain-native) and 8 `cookie_store` (harness matches
`push_api`/`periodic_sync`: `ServiceWorkerRegistration` + native stubs, then
`install_cookie_store_v8`). No bridge bugs found. Both rquickjs install
functions and their `lib.rs::install_dom` call sites removed (`cookie_store`
was at `lib.rs:772`, `view_transitions` at `:784`) — same accepted
QuickJS-behavior-change side effect as every prior G batch; the
`view_transition_events`/`take_view_transition_events` field+method on
`QuickJsRuntime` itself were left in place (dead once the install call is
gone, but shared shell-side plumbing, not rquickjs-specific — removing them
isn't in scope for a "delete the install call" step).

Closes [BUG-545](../../bugs/BUG-545-FIXED.md) (`view_transitions` — real
functional regression, not a stub-native: `P2-viewtrans`'s engine cross-fade
mechanism was already fine, only the JS trigger and the shell's V8-side
drain stub were missing) and [BUG-546](../../bugs/BUG-546-FIXED.md)
(`cookie_store` — `CAPABILITIES.md` claimed ✅ unconditionally). Both
`CAPABILITIES.md` entries raised from their 🟡 QuickJS-only caveat back to
the main ✅ list.

`cargo test -p lumen-js --features v8-backend`: +19 new module tests (11 +
8), full suite 2568 passing. `cargo test -p lumen-js` (default, no
features): 328→309 lib (-19, the ported rquickjs tests removed). `cargo
clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
clean. `cargo clippy -p lumen-js --all-targets -- -D warnings` (no
features): identical baseline to pre-batch main (`offscreen_canvas.rs`/
`worker.rs`/`canvas2d.rs`) — zero new errors from `cookie_store.rs`/
`view_transitions.rs` (the `use std::sync::{Arc, Mutex}` import needed
`#[cfg(feature = "v8-backend")]` gating too, since it's now only used inside
the V8 install fn and its tests, not unconditionally as under rquickjs).
`cargo check -p lumen-shell` (default, v8) and `cargo check -p lumen-shell
--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
(rollback) both clean, plus `cargo clippy -p lumen-shell -- -D warnings`
(default) clean.

### S12b-G6: `cookie_banner` (2026-08-04, branch `p1-s12b-g6`)

Sixth and final port batch of group G — single module, closing the group's
port work (the two remaining G-triage entries, `webgl_bindings`/
`audio_bindings`, are removal-only and live in `S12b-Asnos1`/`S12b-Asnos2`,
not here). No natives — same fast path as `cookie_store`/G1-G4:
`rquickjs::Ctx::eval(COOKIE_BANNER_SHIM)` → `lumen_core::ext::JsRuntime::eval`,
registered as an extra-arg call site in `V8JsRuntime::install_dom` (between
`content_index` and `cookie_store`, not the bare `install_v8!` macro,
because the enable flag lives on `self`, not `&ctx`) rather than a plain
`install_v8!(cookie_banner::install_cookie_banner_bindings_v8)`. Added a
`cookie_banner_dismiss: AtomicBool` field + `set_cookie_banner_dismiss()`
setter on `V8JsRuntime`, mirroring `QuickJsRuntime`'s field/method of the
same name (this closes the actual regression in BUG-548: the shell's
`KeyCommand::ToggleCookieBannerDismiss` toggle and the `cookie_banner_dismiss`
default were already wired into both `run_scripts_with_dom` call sites in
`crates/shell/src/main.rs`, but only the QuickJS branch ever called
`rt.set_cookie_banner_dismiss(...)` — the V8 branch had no such method to
call, so the toggle silently did nothing on the default build). Both shell
call sites (classic-load `run_scripts_with_dom` and the `Lumen::` navigate
path) now call `rt.set_cookie_banner_dismiss(cookie_banner_dismiss)` on the
V8 branch too.

16 tests: 4 (`consent_selectors_*`) test the pure-Rust `CONSENT_SELECTORS`
constant and needed no engine at all — left ungated in the shared `mod
tests`. The remaining 12 (shim behavior: dismiss/no-match/hidden-element/
display-none/cleanup/interval/observer/selector-ordering/no-MutationObserver
cases) moved to `#[cfg(all(test, feature = "v8-backend"))] mod tests_v8`
against a bare `V8JsRuntime::new()` + a hand-rolled `document`/
`MutationObserver`/timer stub (no full `install_dom` needed — the shim only
touches `document`, `window.getComputedStyle`, timers, and `MutationObserver`).
No bridge bugs found. rquickjs `install_cookie_banner_bindings`/`install`/
`install_with_selectors`/`inject`, the `use rquickjs::Ctx`, and the
`cookie_banner_dismiss: AtomicBool` field + `set_cookie_banner_dismiss()` on
`QuickJsRuntime` all removed from `lib.rs`/`cookie_banner.rs`, along with the
`install` call in `QuickJsRuntime::install_dom`.

Closes [BUG-548](../../bugs/BUG-548-FIXED.md) — not a `CAPABILITIES.md`
overclaim (the feature was never listed there), but a real user-facing
regression: the privacy toggle was a silent no-op on the default build.

`cargo test -p lumen-js --features v8-backend cookie_banner`: 16/16
(4 shared + 12 gated). Full `cargo test -p lumen-js --features v8-backend`:
2568 passing, 0 failed (includes this batch's 12 gated tests — the
G5-to-G6 total held flat rather than growing by 12, i.e. some other
concurrent main-branch change removed roughly the same count from the
v8-backend suite elsewhere in the same window; not investigated further,
out of scope for this batch and not a regression of this change per se —
0 failures either way). `cargo test -p lumen-js` (default): 309→297 (-12,
the 12 tests that moved to the v8-only gate; the 4 shared ones stayed).
Both clippy passes clean: `--features v8-backend --all-targets -- -D
warnings` zero errors; default `--all-targets -- -D warnings` matches the
pre-existing 18-error baseline (`offscreen_canvas.rs`/`worker.rs`/
`canvas2d.rs` dead-code, unrelated to this batch), zero new errors.
`cargo check -p lumen-shell` (default, v8) and `--no-default-features
--features backend-femtovg,backend-wgpu,quickjs` (rollback) both clean.

### S12b-Asnos1: `webgl_bindings` removal (2026-08-04, branch `p1-s12b-asnos1`)

First of the two G-triage removal-only entries (`S12b-Asnos1`/`S12b-Asnos2`,
carved out of the stale `S12b-G7` `ROADMAP.md` row, which still described
`audio_bindings` as needing a "check whether shadowed by `web_audio.rs`"
investigation that the 2026-08-03 G0 triage had already completed and
answered — see `docs/tasks/p1-s12b-cleanup-queue.md` §4). `webgl_bindings.rs`
(564 lines, 21 tests, `install_webgl_bindings` fingerprint-only WebGL shim)
is dead code on **both** engines: `grep -rn "webgl_bindings::" crates/js/src`
found zero call sites outside the module's own tests — not registered in
`QuickJsRuntime::install_dom` (`lib.rs`), not registered in
`v8_runtime.rs::install_dom`, no V8 port ever existed. Fully superseded by
the functional `webgl_canvas.rs` (`install_webgl_canvas_v8`, wired through
`SoftwareWebGl`), which already carries the ADR-007 fingerprint
normalization this module provided. No bug filed — not a migration
regression, just a stale module nobody wired up.

Deleted `crates/js/src/webgl_bindings.rs` outright (no V8 side existed to
keep), dropped `pub mod webgl_bindings;` from `lib.rs`, and rewrote the
dangling intra-doc link `[crate::webgl_bindings]` in `webgl_canvas.rs`'s
module doc comment to plain prose. No `install_*` call site existed on
either engine to remove.

`cargo test -p lumen-js` (default): 297→276 (-21, all in the deleted
module — verified against the pre-change tree via `git stash`).
`cargo test -p lumen-js --features v8-backend`: 2547 passing, 0 failed
(2568→2547, -21, same delta — the module was untested-by-omission on V8,
i.e. its 21 tests only ever ran against the rquickjs side). Both clippy
passes clean: `--features v8-backend --all-targets -- -D warnings` zero
errors; default `--all-targets -- -D warnings` matches the pre-existing
18-error baseline (`offscreen_canvas.rs`/`worker.rs`/`canvas2d.rs`
dead-code, unrelated to this batch), zero new errors.

### S12b-Asnos2: `audio_bindings` removal (2026-08-04, branch `p1-s12b-asnos2`)

Second of the two G-triage removal-only entries. Unlike `webgl_bindings`,
`audio_bindings.rs` (1120 lines, 29 tests, `install_audio_bindings` —
`BaseAudioContext`/`AudioContext`/`OfflineAudioContext` plus the extra node
types `ConvolverNode`/`WaveShaperNode`/`IIRFilterNode`/
`ChannelSplitterNode`/`ChannelMergerNode`/`MediaStreamAudioSourceNode`/
`MediaStreamAudioDestinationNode`/`AudioWorklet` stub/`AudioListener`, plus
ADR-007 Layer 4 anti-fingerprint LCG noise) **was** called from
`QuickJsRuntime::install_dom` (`lib.rs:715-718`) right up to this batch —
[BUG-550](../../bugs/BUG-550-FIXED.md)'s "already dead code on both
engines" premise turned out stale before this batch even started: `S12b-B19`
(landed the same day BUG-550 was filed, `46dfe6774`, *after* BUG-550) removed
the *other* shim, `web_audio::install_web_audio_api` (the rquickjs twin of
the now V8-only `web_audio::install_web_audio_api_v8`), because *it* had a
V8 port and qualified for the standard group-A removal procedure.
`web_audio` had been installed *second* in `QuickJsRuntime::install_dom`,
silently overwriting `globalThis.AudioContext` set by `audio_bindings` —
that's the shadow BUG-550 documented. Once B19 deleted the shadower (not
the shadowed), `audio_bindings` became the *sole* remaining `AudioContext`
installer on the `--features quickjs` (no `v8-backend`) rollback build —
live, not dead, there. Flagged to the user before touching code; user
decision 2026-08-04: delete anyway per the original plan, accepting that
the quickjs-only rollback build loses `AudioContext` entirely (no shim at
all, on either the rich or the simple side) until `QuickJsRuntime` itself
is deleted in `S12b-F2`, next in this same batch sequence. No V8 port ever
existed for `audio_bindings` and none was written — `web_audio.rs` remains
the sole `AudioContext` provider on the default (V8) build, unaffected by
this deletion (it never had a dependency on `audio_bindings`, one-directional
shadow only).

Deleted `crates/js/src/audio_bindings.rs` outright, dropped `pub mod
audio_bindings;` from `lib.rs`, and removed the `install_audio_bindings`
call block (`lib.rs:715-718`, the `audio_seed`/`new_session_seed()` +
`install_audio_bindings` two-liner plus its comment) from
`QuickJsRuntime::install_dom`. `grep -rn "audio_bindings" crates/` after
deletion: zero hits — no tests lived outside the module's own file (unlike
the `pip_bindings`/`typed_om_api` trap), no doc-comment cross-references to
fix (unlike `webgl_canvas.rs`'s intra-doc link in Asnos1).

`cargo test -p lumen-js` (default): 276→247 (-29, all in the deleted
module). `cargo test -p lumen-js --features v8-backend`: 2547→2518 (-29,
same delta — `audio_bindings`'s tests never ran against V8, no port
existed). Both clippy passes clean: `--features v8-backend --all-targets --
-D warnings` zero errors; default `--all-targets -- -D warnings` matches
the pre-existing 18-error baseline (`offscreen_canvas.rs`/`worker.rs`/
`canvas2d.rs` dead-code, unrelated to this batch), zero new errors.
`cargo check -p lumen-shell` (default, v8) and `--no-default-features
--features backend-femtovg,backend-wgpu,quickjs` (rollback) both clean.
Closes [BUG-550](../../bugs/BUG-550-FIXED.md) (premise corrected in the
bug file itself, not just here).

### S12b-F1: remove the `quickjs` shell feature (2026-08-04, branch `p1-s12b-f1`)

Closes out group A/G/Asnos (all 38 batches `done`) by removing the shell-side
`quickjs` Cargo feature and every branch that constructed a `QuickJsRuntime`.
`rquickjs` itself (`QuickJsRuntime`, `install_primitives`, the `lumen-js`
dependency) is untouched — that's S12b-F2..F4, scoped to `crates/js`.

Three deletions in `crates/shell/src/main.rs`:

1. `QuickPersistentJs` struct + its ~50-method `PersistentJs` impl (~305
   lines) — the shell-side adapter wrapping `lumen_js::QuickJsRuntime`.
2. The quickjs arm of `run_scripts_with_dom` (constructs `QuickJsRuntime::
   new()`, installs DOM, evaluates scripts, returns a `QuickPersistentJs`).
3. The quickjs arm of `Lumen::bfcache_thaw` (same construction pattern, for
   the T3-hibernation JS-context rebuild path).

Both matching `#[cfg(all(feature = "v8", not(feature = "quickjs")))]` guards
collapsed to plain `#[cfg(feature = "v8")]` now that there's no `quickjs`
branch left to out-race. ~84 occurrences of `#[cfg(any(feature = "quickjs",
feature = "v8"))]` / `#[cfg(not(any(...)))]` / `#[cfg_attr(not(any(...)),
allow(dead_code))]` simplified to the single-condition form across
`main.rs`, `config.rs`, `tab_lifecycle/hibernate.rs`,
`platform/file_dialog.rs` (all four files touched only by this mechanical
substitution plus stale doc-comment wording — no behavioral change beyond
the deleted quickjs arms). `crates/shell/Cargo.toml`'s `[features]` block
lost the `quickjs` entry and its doc comment; `v8` is now the only JS-engine
feature (`NullJsRuntime` remains the fallback with neither).

`cargo check`/`clippy --all-targets -- -D warnings`/`test` -p lumen-shell
(default, v8): clean, 1565 passed, 0 failed. Not re-verified: the
`--no-default-features --features backend-femtovg,backend-wgpu,quickjs`
rollback combo other batches checked, since the flag no longer exists.

Two findings surfaced, neither fixed here (out of scope for a
feature-flag-removal slice):

- **Pre-existing, not a regression:** `--no-default-features --features
  backend-femtovg,backend-wgpu` (no JS engine at all, `NullJsRuntime`
  fallback) fails to compile — 25× `E0433 cannot find module lumen_js`,
  because several fields/fns (`video_gif_store`, `text_track_store`,
  `take_print_requests`, `handle_print_request`, …) reference `lumen_js::*`
  types unconditionally instead of behind a `#[cfg(feature = "v8")]` gate.
  Confirmed identical (same 25 errors) on `main` *before* this branch —
  this build combination was already broken, this slice didn't touch any
  of the offending lines. Candidate for a new BUG (P3).
- **WebGPU reachability shrinks:** the real `navigator.gpu` backend
  (`lumen-js` feature `webgpu` → `lumen-paint/backend-wgpu`) was only ever
  turned on by shell's `quickjs` feature (`quickjs = ["dep:lumen-js",
  "lumen-js/webgpu"]`); `v8` never enabled it. Default (`v8`) already
  shipped `navigator.gpu` as a pure JS shim before this slice — that part
  is unchanged — but the `--features quickjs` escape hatch to the real
  backend is now gone with no replacement, since no shell feature pairs
  `lumen-js/v8-backend` with `lumen-js/webgpu`. `CAPABILITIES.md` currently
  marks WebGPU "✅ complete" unqualified; that claim needs the owning role
  to either wire `lumen-js/webgpu` into shell's `v8` feature or caveat the
  entry.

Next in queue: S12b-F2 (`lumen-js/lib.rs`: delete `QuickJsRuntime`,
`install_dom`, `rq_err`, the `__lum_args__` workaround, `use rquickjs`).

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
