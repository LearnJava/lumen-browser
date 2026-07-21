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
