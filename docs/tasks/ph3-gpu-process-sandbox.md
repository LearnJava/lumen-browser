# Ph3 — GPU process / sandbox

**Developer:** P4
**Branch:** `p4-ph3-gpu-sandbox`
**Size:** XL (multi-step; split into sub-tasks before execution)
**Crates:** `lumen-ipc`, `lumen-shell`, `lumen-paint`

---

## Status

**Phase 3 future item** — not started. Do not begin until Phase 2 is closed and
`lumen-plan.md` marks Phase 3 as active. Listed in `docs/plan/phases.md:136`.

---

## Goal

Introduce a process boundary between the browser shell and the GPU/renderer so that
GPU driver crashes, shader bugs, or memory corruption in the renderer cannot take down
the full browser. Then apply an OS-level sandbox to the renderer process on each
supported platform: Windows (AppContainer + Job Object), Linux (seccomp-bpf +
user namespaces), macOS (App Sandbox via entitlements). Extend the existing per-origin
security policy toward site isolation once the process boundary exists.

---

## Current state (as of Phase 2)

### Process model — single-process today

Lumen is a **single-process browser**. All subsystems (shell, layout, paint, JS,
network) run in the same OS process. There is no renderer process.

The one exception is the **network service** (`lumen-network-service`), which is already
a separately spawned child process:

- **Spawn:** `crates/shell/src/network_service.rs:53` — `Command::new(&svc_path).spawn()`
- **Handle:** `NetworkServiceHandle` at `crates/shell/src/network_service.rs:26`
- **Drop:** kills the child at `crates/shell/src/network_service.rs:82`

This is the only existing out-of-process boundary. It is the **template for the GPU
process**: same `Command::spawn` pattern, same `NetworkServiceHandle`-style lifecycle,
same IPC channel.

No OS-level sandbox primitives (seccomp-bpf, AppContainer, Job Object, App Sandbox)
exist anywhere in the Rust source. The grep for `seccomp`, `AppContainer`, `JobObject`,
`sandbox_init`, `landlock` returns zero results.

### IPC layer — TCP loopback, bincode-framed

`crates/ipc/src/lib.rs` — complete IPC transport:

| Symbol | Line | Role |
|--------|------|------|
| `IpcChannel<S>` | `lib.rs:161` | Bidirectional framing: 4-byte LE length + bincode body |
| `IpcServer::bind()` | `lib.rs:215` | OS-assigned loopback port; returns `(server, port)` |
| `IpcServer::accept()` | `lib.rs:226` | Blocks until client connects |
| `IpcClient::connect(port)` | `lib.rs:251` | Shell-side connector |
| `IpcClient::request()` | `lib.rs:261` | Send + block for response |
| `IpcRequest` / `IpcResponse` | `lib.rs:44` / `lib.rs:76` | Current message envelope |

The existing `IpcRequest` variants (`Fetch`, `Ping`, `Shutdown`, `CreateTab`,
`NavigateTab`, `Screenshot`, `CloseTab`) are all network-service or tab-control messages.
**No GPU/render commands exist in the envelope yet.** Adding `GpuRender`, `GpuResize`,
`GpuSurfaceLost` variants is the first IPC work item.

**Phase 1 limitation noted in source:** `lib.rs:11` — "single synchronous connection,
one in-flight request at a time". The GPU process needs pipelined (async) message
dispatch; this is a prerequisite upgrade.

### GPU execution — in-process today

GPU work runs in the same process as the shell. Entry points:

| Symbol | File:Line | What it does |
|--------|-----------|--------------|
| `wgpu::Instance::new()` | `renderer.rs:1582` | Creates D3D12/Vulkan/Metal instance |
| `adapter.request_device()` | `renderer.rs:1600` | Opens GPU device handle |
| `RenderBackend::render()` | `backend.rs:~80` | Trait method called every frame |
| `create_backend()` | `backend_factory.rs:~45` | Selects femtovg / wgpu / vello |
| `Box<dyn RenderBackend>` | `main.rs:7204` | Shell holds the live backend |

Phase 3 plan from `backend.rs:16` comment: "backend-vello = Phase 3 default". The
same Phase 3 window is when the GPU process should land.

### Existing sandbox surface — HTML `<iframe sandbox>` only

`crates/core/src/sandbox.rs` — `SandboxFlags` bitset for HTML `<iframe sandbox>`
attribute parsing (HTML LS §7.6.5). This is a **logical** (page-level) sandbox, not an
OS-level process sandbox. No connection to OS syscall filtering or process isolation.

### Site isolation — design-only

`docs/plan/security-performance.md:13` documents site-isolation intent (§10.3: "site
isolation по eTLD+1"), but there is no implementation. The DOM and cookie store use
origin strings but there is no per-origin process assignment table. The JS shared-worker
code (`crates/js/src/shared_worker.rs:67`) notes "single-origin process" as a comment
without wiring.

### `--ipc-server` driving mode (foundation)

`crates/shell/src/main.rs:912` — `run_ipc_server()`: the shell can be started headless
as a tab-command TCP server. An external controller (e.g. `graphic_tests/run.py`)
opens the browser once and pulls PNG screenshots over TCP. This demonstrates that a
**render-over-IPC** loop is already conceptually present; formalising it into a GPU
process is the architectural step.

---

## Architecture

### Process topology (proposed)

```
┌─────────────────────────────────────────────────────┐
│  Browser shell (lumen-shell)                        │
│  - UI (winit), navigation, storage, JS GC tier,     │
│    address bar, find-in-page, DevTools              │
│  - Holds IPC channels to child processes            │
└────────────┬────────────────────────┬───────────────┘
             │ TCP IPC (loopback)     │ TCP IPC (loopback)
             ▼                        ▼
┌────────────────────────┐  ┌───────────────────────────┐
│  lumen-renderer        │  │  lumen-network-service    │
│  (NEW Phase 3 binary)  │  │  (EXISTS Phase 1)         │
│  - wgpu / femtovg /    │  │  - HTTP/TLS/DNS           │
│    vello rendering     │  │  - No UI / no GPU         │
│  - Layout + paint      │  │  - sandboxed: no net write│
│  - OS sandbox applied  │  │    (already isolated)     │
└────────────────────────┘  └───────────────────────────┘
```

The shell keeps the window surface (`winit::Window`) and passes a raw surface handle to
the renderer process via IPC at startup. The renderer process owns the wgpu `Instance`,
`Adapter`, `Device`, and `Queue`. Each frame the shell sends a `DisplayList` over IPC;
the renderer submits GPU work and returns when the frame is complete (or signals error).

### IPC message extensions (proposed)

Extend `IpcRequest` / `IpcResponse` in `crates/ipc/src/lib.rs`:

```rust
// PROPOSED — not yet in source
IpcRequest::GpuInit { surface_handle: RawWindowHandle, width: u32, height: u32 }
IpcRequest::GpuRender { display_list: Vec<u8> }   // bincode-serialized DisplayList
IpcRequest::GpuResize { width: u32, height: u32 }
IpcRequest::GpuSurfaceLost

IpcResponse::GpuReady
IpcResponse::GpuFrameDone
IpcResponse::GpuError(String)
```

`RawWindowHandle` (from `raw-window-handle` crate, already transitively present via
wgpu / winit) is platform-specific and can be transmitted as a `u64` or `usize`
depending on the platform type.

### Per-OS sandbox (proposed)

#### Windows — AppContainer + Job Object

- Use `windows-sys` crate (already in the dependency graph via winit/wgpu) to:
  - Create an `AppContainer` SID via `CreateAppContainerProfile`.
  - Spawn `lumen-renderer.exe` with `STARTUPINFOEXA.lpAttributeList` containing
    `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` pointing to the AppContainer SID.
  - Wrap the child in a Job Object (`CreateJobObject` + `AssignProcessToJobObject`)
    with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so the renderer dies with the shell.
  - Set mitigation policies via `SetProcessMitigationPolicy`:
    `ProcessDEPPolicy`, `ProcessASLRPolicy`, `ProcessControlFlowGuardPolicy`.
- The renderer process must not hold GPU D3D12 device in elevated context —
  AppContainer already strips most privileges; verify `dxgi` works inside container.

#### Linux — seccomp-bpf + user namespaces

- Use `seccompiler` crate (or raw `prctl`/`seccomp` syscall via `libc`) to install a
  seccomp-bpf filter in the renderer after `GPU::init()` completes.
- Allowed syscall whitelist (initial): `read`, `write`, `mmap`, `munmap`, `futex`,
  `clock_gettime`, `exit_group`, `ioctl` (DRM only), `recvmsg`, `sendmsg` (loopback
  socket). Deny everything else with `SECCOMP_RET_KILL_PROCESS`.
- Optionally enter a user namespace (`CLONE_NEWUSER`) before seccomp to drop
  supplemental groups.
- Landlock (Linux 5.13+) for filesystem: allow only `/dev/dri/*` (GPU device nodes)
  and `/proc/self/*`; deny all other path access.

#### macOS — App Sandbox entitlements

- Add `com.apple.security.app-sandbox` entitlement to `lumen-renderer` target in
  `Entitlements.plist`.
- Grant only `com.apple.security.device.gpu` (GPU access) and `com.apple.security.network.client`
  restricted to loopback.
- Use `sandbox_init(3)` C API via `libc` for a programmatic profile as an alternative
  to the entitlement-only approach (the profile can deny all filesystem except
  the IPC socket path).

### Site isolation extension (proposed, post-sandbox)

Once the renderer process exists, extend site isolation:

1. Add a `RendererProcessMap: HashMap<eTLD+1, RendererProcessHandle>` in the shell.
2. When navigating to a new origin, look up or spawn the corresponding renderer
   process.
3. Cross-origin iframes (`<iframe sandbox>` + `allow-same-origin` absent) get their
   own renderer entry.
4. The `SandboxFlags` bitset (`crates/core/src/sandbox.rs:22`) connects here: an
   opaque-origin iframe gets a fresh renderer process that also has `ORIGIN` restriction
   set.

---

## Entry points

### Existing (real file:line)

| File | Line | What |
|------|------|------|
| `crates/shell/src/network_service.rs:38` | Spawn pattern to copy for GPU process |
| `crates/shell/src/network_service.rs:53` | `Command::new().spawn()` — model for renderer spawn |
| `crates/ipc/src/lib.rs:161` | `IpcChannel<S>` — transport to reuse |
| `crates/ipc/src/lib.rs:44` | `IpcRequest` enum — add GPU variants here |
| `crates/ipc/src/lib.rs:76` | `IpcResponse` enum — add GPU variants here |
| `crates/shell/src/backend_factory.rs:45` | `create_backend()` — moves into renderer binary |
| `crates/engine/paint/src/renderer.rs:1582` | `wgpu::Instance::new()` — moves into renderer |
| `crates/engine/paint/src/renderer.rs:1600` | `adapter.request_device()` — moves into renderer |
| `crates/engine/paint/src/backend.rs:~80` | `RenderBackend` trait — renderer implements it |
| `crates/shell/src/main.rs:7204` | Shell creates backend — becomes IPC client call |
| `crates/shell/src/main.rs:912` | `run_ipc_server()` — proof-of-concept for render-over-IPC |
| `crates/core/src/sandbox.rs:22` | `SandboxFlags` — logical sandbox; connects to process map |

### Proposed (new files/code)

| File (proposed) | What |
|-----------------|------|
| `crates/renderer/src/main.rs` | New binary: `lumen-renderer`; GPU process entry point |
| `crates/renderer/src/sandbox/windows.rs` | AppContainer + Job Object setup |
| `crates/renderer/src/sandbox/linux.rs` | seccomp-bpf + user namespace |
| `crates/renderer/src/sandbox/macos.rs` | `sandbox_init` profile |
| `crates/shell/src/renderer_process.rs` | `RendererProcessHandle` (mirrors `network_service.rs`) |
| `crates/ipc/src/gpu_messages.rs` | `GpuRequest` / `GpuResponse` types |

---

## Steps

### Phase A — Process boundary (no sandbox yet)

1. **New crate `lumen-renderer`** — binary that accepts `GpuInit` over IPC, creates
   the wgpu device, and enters a render loop (`GpuRender` → submit → `GpuFrameDone`).
   - Copy `wgpu::Instance::new()` block from `renderer.rs:1582`.
   - Receive `DisplayList` bytes over IPC; deserialize; call existing render path.
   - Return `GpuFrameDone` when `queue.submit()` returns.

2. **Extend `IpcRequest`/`IpcResponse`** at `crates/ipc/src/lib.rs:44,76` —
   add `GpuInit`, `GpuRender`, `GpuResize`, `GpuSurfaceLost` / `GpuReady`,
   `GpuFrameDone`, `GpuError`.

3. **`RendererProcessHandle`** at `crates/shell/src/renderer_process.rs` —
   mirrors `NetworkServiceHandle` (`spawn` + `Drop`-kill pattern).

4. **Shell wires up the new handle** at `crates/shell/src/main.rs` —
   replace `backend_factory::create_backend()` call (line 7204) with
   `RendererProcessHandle::spawn()` + IPC-backed `RemoteRenderBackend`.

5. **`RemoteRenderBackend`** — implement `RenderBackend` trait (`backend.rs:~80`)
   that serializes `DisplayList` and sends `GpuRender` over IPC instead of calling
   wgpu directly.

6. **Verify** with existing `--ipc-server` graphic-test pipeline: screenshots over IPC
   still match Edge baselines.

### Phase B — Windows AppContainer sandbox

7. In `lumen-renderer/src/sandbox/windows.rs`: call `CreateAppContainerProfile`,
   build `STARTUPINFOEXA`, re-exec self inside the container.
8. Add a `Job Object` around the spawned process in `renderer_process.rs`.
9. Test: renderer still renders TEST-01..TEST-20; process stays alive after 100 frames.

### Phase C — Linux seccomp-bpf sandbox

10. In `lumen-renderer/src/sandbox/linux.rs`: install seccomp filter after GPU init.
11. Add `seccompiler` as a `[target.'cfg(target_os = "linux")'.dependencies]` entry in
    `crates/renderer/Cargo.toml` with justification comment.
12. CI Linux job: run headless render tests with filter active.

### Phase D — macOS App Sandbox

13. Add `Entitlements.plist` for `lumen-renderer` target.
14. Wire `sandbox_init(3)` in `sandbox/macos.rs` via `libc::sandbox_init`.
15. macOS CI: confirm renderer process has no write access to `~/Documents`.

### Phase E — Site isolation extension

16. Add `RendererProcessMap` in shell (see Architecture section).
17. Connect `SandboxFlags::ORIGIN` (`crates/core/src/sandbox.rs:39`) — opaque-origin
    iframes get a dedicated renderer.
18. Integration test: two cross-origin iframes rendered by separate processes; killing
    one does not crash the browser.

---

## Platform matrix

| Platform | Sandbox mechanism | Key API | Status |
|----------|------------------|---------|--------|
| Windows 10+ | AppContainer + Job Object | `windows-sys::Win32::Security` | Proposed |
| Linux 3.17+ | seccomp-bpf | `seccompiler` crate or `libc::prctl` | Proposed |
| Linux 5.13+ | + Landlock FS isolation | `rustix::fs::landlock` | Proposed (additive) |
| macOS 10.15+ | App Sandbox + `sandbox_init` | `libc::sandbox_init` | Proposed |

---

## Risks

1. **`raw-window-handle` is not Send across processes.** A window surface handle is
   a kernel object on Windows (`HWND`) or an X11/Wayland connection on Linux. It can
   be serialized as a raw integer and reconstructed in the renderer, but the renderer
   must not access the `Window` struct — only its handle value.

2. **wgpu DXGI inside AppContainer.** Microsoft documents that DXGI works in
   AppContainer but requires the package SID to have access to the GPU. Test early;
   if blocked, fall back to a restricted Job Object without AppContainer for v1.0.

3. **IPC latency budget.** A `DisplayList` for a complex page can be several hundred KB.
   Loopback TCP adds ~0.1 ms per frame; bincode serialization of `DisplayList` needs
   profiling. If >1 ms per frame, switch to a shared-memory ring buffer for the frame
   payload and keep TCP only for control messages.

4. **seccomp whitelist is brittle.** wgpu on Linux uses DRM ioctls; the exact set of
   allowed ioctls differs between Mesa versions. The initial whitelist must be derived
   from an `strace` run and validated in CI against a pinned Mesa version.

5. **`lumen-renderer` binary not yet in Cargo workspace.** Adding a new binary crate
   requires a `Cargo.toml` entry, a `[[bin]]` section, and updating
   `docs/plan/architecture.md` §3 (dependency graph). Use `/lumen-new-crate` skill.

6. **Phase ordering vs Phase 3 backend switch.** ADR-010 plans vello as the Phase 3
   default backend. The GPU process work and the vello switch should land in the same
   Phase 3 window so the renderer binary never needs to support two GPU APIs at once.

---

## Tests

| Test | What it checks |
|------|----------------|
| `cargo test -p lumen-ipc` (existing) | IPC framing round-trips; extend with GPU message round-trips |
| Graphic tests (`graphic_tests/run.py --build`) | End-to-end: shell + renderer process + Edge screenshot comparison; same 0.5% threshold |
| `renderer_process_survives_gpu_error` (new) | Shell receives `GpuError`, restarts renderer, navigation resumes |
| `renderer_process_killed_does_not_crash_shell` (new) | Kill `lumen-renderer` with SIGKILL; shell shows error tab, does not panic |
| `appcontainer_renderer_cannot_access_filesystem` (new, Windows CI) | Renderer process cannot open `C:\Windows\System32\notepad.exe` |
| `seccomp_renderer_denied_on_open_syscall` (new, Linux CI) | `open("/etc/passwd")` in renderer → SIGKILL from kernel |
| `cross_origin_iframes_use_separate_renderers` (new, Phase E) | Two different eTLD+1 iframes → two distinct renderer PIDs visible via `RendererProcessMap` |

---

## Definition of done

- [ ] `lumen-renderer` binary exists in workspace; `cargo build` produces it.
- [ ] Shell spawns renderer process on startup; all existing graphic tests pass.
- [ ] On Windows: renderer runs inside AppContainer + Job Object; verified by
      `GetAppContainerSid` returning non-null in renderer.
- [ ] On Linux: seccomp filter installed; `strace` confirms denied syscalls return
      `ENOSYS` (or process is killed) rather than succeeding.
- [ ] On macOS: `sandbox_check(2)` in renderer confirms filesystem write is denied.
- [ ] Renderer crash does not crash the shell process; error tab shown.
- [ ] `docs/plan/security-performance.md` §10.3 updated to describe the actual process map.
- [ ] `CAPABILITIES.md` updated: process isolation row changes ⬜ → ✅.
- [ ] New ADR filed: `docs/decisions/ADR-015-gpu-renderer-process.md`.
