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
| ☐ S3 | **Core DOM** | Port `install_primitives` (184 `reg!` natives, `dom.rs:401`) via compat layer; eval `WEB_API_SHIM` unchanged; `V8JsRuntime::install_dom` with same signature as QuickJS version | `document.querySelector`, `_lumen_tick_timers`, `window.location.href` work; `samples/page.html` renders under `--features v8-backend` e2e | Medium |
| ☐ S4 | **Shell adapter** | `v8 = ["dep:lumen-js", "lumen-js/v8-backend"]` in shell `Cargo.toml`; `#[cfg(feature = "v8")] struct V8PersistentJs` mirroring `QuickPersistentJs` (~50 methods, mechanical); construction branch at `main.rs:4934` | `cargo run -p lumen-shell --no-default-features --features backend-femtovg,v8 -- samples/page.html` interactive | Low |
| ☐ S5–S7 | **Simple-module batches** | ~90 modules with plain `Function::new` registrations, batches of ~30, via compat layer. Same transformation each — parallel subagents appropriate here. Keep a ported/pending checklist in this file | `cargo test -p lumen-js --features v8-backend` after each batch | Low |
| ☐ S8 | **canvas2d + webgl_canvas** | Hand-port (hot path, 85 rquickjs mentions; pixel queues via `flush_canvas_updates`) | canvas graphic tests pass under v8 feature | Medium |
| ☐ S9 | **wasm + webgpu** | `Persistent<Function>` GC roots → `v8::Global<Function>`; keep the `wasm::clear_registry()` teardown pattern (`lib.rs:401`) | wasm + webgpu test suites green (note: webgpu test flaky under load — rerun before blaming the port) | Medium |
| ☐ S10 | **worker + shared_worker + sw_worker** | Per-thread `Runtime`+`Context` (`worker.rs:293`) → per-thread `OwnedIsolate`; same channel protocol | worker tests green | Medium |
| ☐ S11 | **suspend/resume (partial 10C.2)** | `suspend()`: enumerate own globals set by page scripts, serialize *data* via `v8::ValueSerializer` into `SuspendedHeap.compressed` (zstd, ≤5 MB); `resume()`: `ValueDeserializer` restore. **Closures are NOT serializable (F1) — the re-run-scripts fallback at `main.rs:14599` stays.** Optional: pure-JS-shim startup snapshot (F2), only if cheap | `tests/v8_snapshot.rs`: `window.__test = 42` survives suspend→resume | Low |
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
