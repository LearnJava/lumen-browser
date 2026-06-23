# Ph4 — Mobile (Android NDK)

**Developer:** P1
**Branch:** `p1-ph4-mobile`
**Size:** XL (multi-phase; split into sub-tasks before execution)
**Crates:** `lumen-shell` (platform/), `lumen-paint` (backends/), `lumen-core`

---

## Status

**Phase 4 future item** — not started. Do not begin until Phase 3 is closed and
`lumen-plan.md` marks Phase 4 as active. Listed in `docs/plan/phases.md:143`.

---

## Goal

Port Lumen to mobile, Android first. iOS is noted as a non-goal for the foreseeable
future (see iOS note below).

The primary deliverable is a working Android APK that:
- Opens web pages fetched over the network.
- Renders via OpenGL ES (GLES) through the existing `RenderBackend` trait.
- Accepts touch input and translates it to the engine's scroll/click/key model.
- Has a minimal mobile-adapted tab and address-bar UI (no desktop-style panel clutter).
- Builds reproducibly via `cargo-ndk` + a Gradle wrapper that packages the `.so`.

---

## iOS note

iOS is blocked by Apple policy: third-party browsers on iOS must use WebKit as their
rendering engine (`com.apple.developer.web-browser-engine.WebKit` entitlement required
since iOS 17.4 EU only; still forbidden for global distribution as of 2026). Shipping
Lumen's own engine on iOS for general release is not possible without violating App
Store guidelines. This task is Android-only. iOS is mentioned in `phases.md:143` only
as context; do not scope iOS work here.

---

## Current state (as of Phase 2)

### Windowing / event loop

Lumen uses **winit 0.30** (`Cargo.toml:61`) via the `ApplicationHandler` trait
(`crates/shell/src/main.rs:94`). `EventLoop::<LoadEvent>::with_user_event().build()`
creates the event loop at `main.rs:527`; `event_loop.run_app(&mut app)` starts it at
`main.rs:754`.

winit 0.30 supports Android (via `android-activity` crate) in addition to desktop. The
`ApplicationHandler` API is already the cross-platform entry point. The window creation
path (`main.rs:~7200`) requests a `winit::window::Window` through `ActiveEventLoop`;
this is the same call that becomes an Android `ANativeWindow` on the Android target.

The shell is entirely single-process and desktop-centric. There is no `android_main`
entry point, no Gradle project, and no `cargo-ndk` configuration today.

### Render backend

The render backend is a `Box<dyn RenderBackend>` created by
`crates/shell/src/backend_factory.rs:39` (`create_backend()`). The `RenderBackend`
trait lives in `crates/engine/paint/src/backend.rs:78`.

The factory selects backends via `LUMEN_BACKEND` env var; the Phase 2 default chain
is femtovg → wgpu fallback (`backend_factory.rs:48`).

`FemtovgBackend::new()` creates a GL display via `glutin`:

```
crates/engine/paint/src/backends/femtovg_backend.rs:821
  #[cfg(target_os = "windows")]  → DisplayApiPreference::EglThenWgl
  #[cfg(target_os = "macos")]    → DisplayApiPreference::Cgl
  #[cfg(all(not(...windows), not(...macos)))]  → DisplayApiPreference::Egl
```

On Android, the correct preference is `DisplayApiPreference::Egl` (same as the
Linux/other branch). No `#[cfg(target_os = "android")]` branch is needed here —
the existing `not(windows) && not(macos)` fallback already selects EGL. However,
`glutin` needs the `ANativeWindow` handle from winit; that path has not been tested
on Android and may need minor fixes.

`WgpuBackend` supports Android via the `wgpu::Backends::VULKAN` or
`wgpu::Backends::GLES` adapter selection; wgpu already ships Android support.

Neither femtovg nor wgpu has been instantiated from an Android build in Lumen — this
is untested, not absent.

### Input — touch is absent

The `InputCommand` enum (`crates/shell/src/input/mod.rs:40`) has `Click`, `MouseMove`,
`TypeText`, `Scroll`, `KeyDown`. There is no `Touch` variant.

The shell's event loop handles `MouseScrollDelta` with `TouchPhase` tracking for
touchpad momentum scrolling (`main.rs:271`, `main.rs:9726–9850`). This is **touchpad
gesture** support on desktop, not finger-touch input.

`winit::event::WindowEvent::Touch` (finger touch events from Android/iOS/touchscreen)
is **not handled anywhere** in `main.rs` — a grep for `WindowEvent::Touch` returns
zero matches. Touch input must be added from scratch.

### Platform-specific code in `shell/src/platform/`

The platform modules and their OS coverage:

| Module | Windows | Linux | macOS | Android |
|--------|---------|-------|-------|---------|
| `clipboard.rs` | Win32 `CF_UNICODETEXT` | `wl-copy`/`xclip` | `pbcopy` | no-op (stub exists at line 195) |
| `wake_lock.rs` | `SetThreadExecutionState` | no-op | no-op | no-op needed |
| `file_dialog.rs` | PowerShell `.ShowDialog()` | no-op | no-op | Android intent |
| `screen_capture.rs` | GDI `BitBlt` | no-op | no-op | no-op |
| `notification.rs` | PowerShell balloon | `notify-send` | no-op | no-op |
| `dark_mode.rs` | winit `Theme` query | winit `Theme` query | winit `Theme` query | winit `Theme` query (works) |
| `audio_capture.rs` | — | — | — | — |
| `audio_player.rs` | — | — | — | — |

The `clipboard.rs` already has a `#[cfg(not(any(windows, linux, macos)))]` no-op stub
at line 195 that covers Android implicitly. `wake_lock.rs` and `notification.rs` follow
the same pattern. No module requires Android-specific porting to compile — they
gracefully degrade to no-ops.

### OS-specific code outside platform/

Beyond `platform/`, the shell has scattered `#[cfg(target_os = "windows")]` blocks:

- `download.rs:467,554,573,598` — `%USERPROFILE%` path; the `not(windows)` branch uses
  `$HOME`. Android has no `$HOME`; the download dir logic needs an Android branch
  (`Context.getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS)` or a fixed path).
- `memory_poll.rs:79–91` — Windows/Linux/other dispatch; the `not(windows, linux)`
  stub covers Android (returns `NullMemoryPressureSource`). Fine as-is for Phase 4.
- `notification.rs:63` — `not(windows, linux)` no-op covers Android.
- `network_service.rs:46` — `#[cfg(windows)]` block; affects the network service spawn.
  On Android the network service binary would need to be embedded as a `.so` instead of
  a separate `.exe` — requires a different spawning strategy or inlining the network
  service. This is the most significant OS-specific obstacle outside rendering.
- `main.rs:7234` — `#[cfg(target_os = "windows")]` UIA (accessibility) bridge init.
  Android has its own a11y API (AccessibilityService); this block is simply skipped on
  Android and poses no blocking issue.

### Memory and storage paths

`main.rs:4305` selects the user-data directory:

```rust
let dir = if cfg!(target_os = "windows") {
    // APPDATA / lumen
} else {
    // $HOME/.local/share/lumen
};
```

The `else` branch works on Linux. Android apps must use `Context.getFilesDir()` or
`Context.getExternalFilesDir()`; a `$HOME` path does not exist or is not writable.
An `#[cfg(target_os = "android")]` branch is needed here using
`android_activity::AndroidApp::internal_data_path()`.

### Build system

Today: `cargo build -p lumen-shell --release` produces a Windows/Linux binary.

Android requires:
- `cargo-ndk` (Rust → `.so`)
- NDK toolchain (r25+ recommended for Vulkan / GLES 3.2 headers)
- `android-activity` crate as a winit backend
- A minimal Gradle project that packages the `.so` into an APK

None of these exist in the workspace today.

---

## Architecture

### Event loop — winit `android-activity` backend

winit 0.30 supports Android via the `android-activity` crate. The integration works by:

1. Adding `android-activity` as a `[target.'cfg(target_os = "android")'.dependencies]`
   entry in `crates/shell/Cargo.toml`.
2. Replacing `fn main()` with `#[no_mangle] pub extern "C" fn android_main(app: AndroidApp)`
   (provided by the `android-activity` crate's `#[android_main]` proc-macro).
3. `EventLoop::with_user_event().build()` accepts an `AndroidApp` parameter on Android;
   the rest of the `ApplicationHandler` impl in `main.rs` is cross-platform and
   unchanged.

### Render backend — GLES via femtovg

`FemtovgBackend` uses glutin, which supports EGL on Android. The existing
`DisplayApiPreference::Egl` branch (`femtovg_backend.rs:826`) applies directly.

The `ANativeWindow` handle passed by winit's Android backend to `window_handle()`
is understood by `glutin-winit`. No backend code changes are anticipated for the
happy path; testing against a physical device or emulator will surface any issues.

For fallback: `WgpuBackend` supports Android via Vulkan (API 28+) or ANGLE-GLES.
If femtovg initialization fails on older Android versions, the existing
`create_femtovg_or_wgpu` fallback chain in `backend_factory.rs:80` fires naturally.

### Touch input

winit fires `WindowEvent::Touch { id, phase, location, force }` for finger touches.
The current shell does not handle this event. The proposed mapping:

| Touch gesture | Existing `InputCommand` or new action |
|--------------|--------------------------------------|
| Single tap (Ended, no move) | `InputCommand::Click { x, y }` |
| Swipe / pan (Moved) | `InputCommand::Scroll { x, y }` (delta-based) |
| Two-finger pinch | Zoom (new `InputCommand::Zoom { delta }` or inline) |
| Long press | Context menu (new `InputCommand::LongPress`) |

The `TouchPhase::Started/Moved/Ended/Cancelled` enum is already imported
(`main.rs:271`); only the handler arm is missing.

A minimal multi-touch tracking state (gesture recognizer) is needed:
- Single active finger → scroll or click.
- Two fingers → pinch-zoom.
- No complex gesture library needed for Phase 4 v1.

### Network service on Android

On desktop, `lumen-network-service` is a separate `.exe` spawned via
`std::process::Command` (`shell/src/network_service.rs:53`). Android does not allow
spawning child executables from an APK.

Options (proposed, in order of preference):
1. **Inline the network service** on Android: `#[cfg(target_os = "android")]` use
   `MockTransport`-style in-process setup, or compile the network crate directly into
   the shell `.so` without a subprocess.
2. **Embed as a thread**: run the network service event loop on a dedicated Rust thread
   instead of a process. This requires the service to become thread-safe (it already
   uses channels).
3. Keep as a spawned process but bundle the `libnetwork_service.so` as a secondary
   APK component (complex packaging; avoid for Phase 4 v1).

Option 1 or 2 is strongly preferred. The `MockTransport` infrastructure
(`crates/network/src/transport.rs` — from completed task 8E.1) shows that the network
crate can operate without spawning a subprocess.

### Mobile-adapted UI

The current UI (`lumen-shell`) is desktop-first: 48px tab strip, multiple panel
overlays, keyboard-driven hint mode, DevTools inspector, right-click menus.

For Android Phase 4:
- Hide desktop panels behind a `#[cfg(not(target_os = "android"))]` compile gate or
  a runtime `is_mobile()` flag.
- Replace the tab strip with a bottom navigation bar or a scrollable tab drawer.
- Replace the address bar with a centered oval bar (standard Android pattern).
- Remove keyboard-shortcut hint overlay; expose touch-friendly controls instead.
- The `lumen-shell` crate is large (~800 KB source); mobile-specific UI may justify a
  separate `lumen-shell-android` crate (proposed, Phase 4 B).

### APK packaging

Proposed build flow (to be added to `Makefile` or a CI script):

```bash
# 1. Cross-compile the shell .so
cargo ndk -t aarch64-linux-android -o ./android/app/src/main/jniLibs build -p lumen-shell

# 2. Package the APK
cd android && ./gradlew assembleDebug
```

The Gradle project (`android/`) needs:
- `AndroidManifest.xml` — `INTERNET` + `READ_EXTERNAL_STORAGE` permissions.
- `build.gradle` — `minSdkVersion 26` (Android 8.0, API 26: full EGL 1.5 / Vulkan 1.0).
- `strings.xml` — app name "Lumen".
- `NativeActivity` (via `android-activity`) as the sole activity.

### Memory / storage paths on Android

`android-activity` provides `AndroidApp::internal_data_path()` for the app-private
files dir and `AndroidApp::external_data_path()` for external storage. A proposed
`#[cfg(target_os = "android")]` branch in the data-dir resolver (`main.rs:4305`)
passes the `AndroidApp` handle down and calls `internal_data_path()`.

---

## Entry points

### Existing (real file:line)

| File | Line | What |
|------|------|------|
| `crates/shell/src/main.rs:527` | `EventLoop::with_user_event().build()` — entry point to replace with `android_main` on Android |
| `crates/shell/src/main.rs:754` | `event_loop.run_app(&mut app)` — cross-platform; unchanged |
| `crates/shell/src/main.rs:271` | `use winit::event::{..., TouchPhase, ...}` — TouchPhase already imported, Touch event handler missing |
| `crates/shell/src/main.rs:4305` | Data-dir selection — needs `#[cfg(target_os = "android")]` branch |
| `crates/shell/src/backend_factory.rs:39` | `create_backend()` — femtovg EGL path works on Android |
| `crates/engine/paint/src/backends/femtovg_backend.rs:821` | GL display creation — EGL branch (`not(windows) && not(macos)`) covers Android |
| `crates/shell/src/network_service.rs:53` | `Command::new().spawn()` — must become in-process on Android |
| `crates/shell/src/platform/clipboard.rs:195` | `not(windows, linux, macos)` no-op stub — covers Android already |
| `crates/shell/src/platform/wake_lock.rs:85` | `not(windows)` no-op — covers Android |
| `crates/shell/src/platform/notification.rs:63` | `not(windows, linux)` no-op — covers Android |
| `crates/shell/src/download.rs:467` | `%USERPROFILE%` / `$HOME` — needs Android branch |
| `crates/shell/src/input/mod.rs:40` | `InputCommand` enum — needs `Touch` gesture recognition |

### Proposed (new files/code)

| File (proposed) | What |
|-----------------|------|
| `crates/shell/src/input/touch.rs` | Touch gesture recognizer: tap → Click, swipe → Scroll, pinch → Zoom |
| `crates/shell/src/platform/android.rs` | Android-specific helpers: data paths, file picker intent |
| `android/app/build.gradle` | Gradle build descriptor |
| `android/app/src/main/AndroidManifest.xml` | App manifest: INTERNET, NativeActivity |
| `android/app/src/main/res/values/strings.xml` | App name |
| `Makefile` target `android-debug` | `cargo ndk` + `gradlew assembleDebug` |

---

## Steps

### Phase A — Build harness (compile to Android, no window yet)

1. Add `android-activity` as a `[target.'cfg(target_os = "android")'.dependencies]`
   in `crates/shell/Cargo.toml` with winit's `android-activity` feature enabled.
2. Add `#[cfg(target_os = "android")]` entry point in `crates/shell/src/main.rs` using
   `#[android_main]` proc-macro; forward to existing `run_windowed()` logic.
3. Add `aarch64-linux-android` target via `rustup target add`.
4. Create minimal `android/` Gradle skeleton (manifest + `build.gradle` + `jniLibs`
   symlink to `cargo-ndk` output).
5. Verify: `cargo ndk -t aarch64-linux-android build -p lumen-shell` compiles without
   errors (link may fail on missing symbols — acceptable at this stage).

### Phase B — Window + render backend on Android

6. Fix any compile errors from Phase A (likely: `#[cfg]` gates for missing OS APIs,
   platform-specific `unsafe` FFI not available on Android).
7. Confirm `FemtovgBackend::new()` succeeds on an Android emulator (API 26+). If it
   fails, debug glutin EGL surface negotiation; fall back to `WgpuBackend` if needed.
8. Render a white page (`DisplayCommand::FillRect` background) — first successful
   frame on Android.
9. Add `#[cfg(target_os = "android")]` branch in `main.rs:4305` for internal data
   path using `android_activity::AndroidApp::internal_data_path()`.

### Phase C — Network on Android

10. Gate `network_service.rs:53` (`Command::new().spawn()`) with
    `#[cfg(not(target_os = "android"))]`.
11. Add `#[cfg(target_os = "android")]` inline-init path: start the network service
    event loop on a dedicated `std::thread::spawn` thread inside the same process.
12. Verify: `lumen` on Android can fetch `http://example.com` and display it.

### Phase D — Touch input

13. Add `WindowEvent::Touch` arm in `main.rs` event handler, calling
    `crates/shell/src/input/touch.rs::TouchGestureRecognizer::feed()`.
14. Implement `TouchGestureRecognizer`: single-finger tap → `InputCommand::Click`;
    single-finger pan → `InputCommand::Scroll` with velocity; two-finger pinch →
    `zoom::apply_zoom_delta()` (inline, no new InputCommand required for Phase 4 v1).
15. Verify: tapping a link navigates; scrolling a long page works with momentum.

### Phase E — Mobile UI

16. Add `fn is_android() -> bool { cfg!(target_os = "android") }` helper in
    `crates/shell/src/main.rs`.
17. Gate desktop-only panels (DevTools, sidebar, tree-tabs, split-view, hint-overlay)
    with `if !is_android()` at their construction sites.
18. Add a minimal bottom-bar address bar for Android: text input + Go button rendered
    as `DisplayCommand::FillRect` / `DrawText` overlays (same mechanism as existing
    `address_bar.rs` but repositioned).
19. Produce a signed debug APK (`gradlew assembleDebug`) and test manually on a
    physical device.

### Phase F — Polish and CI

20. Add `android-debug` target to `Makefile` or `scripts/build-android.sh`.
21. Document the Android build in `README.md` under a "Mobile" section.
22. Update `CAPABILITIES.md`: add Android row.
23. File an ADR: `docs/decisions/ADR-016-android-port.md`.

---

## Risks

1. **`android-activity` + winit version compatibility.** winit 0.30 requires
   `android-activity 0.6`. If a dependency already pulls in an incompatible version,
   `Cargo.lock` will flag it. Check with `cargo tree -p android-activity` after adding.

2. **glutin EGL surface on emulator.** Android emulators (especially `x86_64` images
   with SwiftShader) sometimes have broken EGL surface negotiation. Test on an `arm64-v8a`
   hardware device early; do not rely solely on emulator results.

3. **Network service subprocess forbidden on Android.** `std::process::Command::new(...).spawn()`
   on Android silently fails or panics depending on the kernel configuration. The inline-
   thread approach (Phase C) must land before any integration test on a real device.

4. **Data-dir path before `AndroidApp` is available.** The `AndroidApp` handle is only
   accessible after `android_main` fires. Any code that calls the data-dir resolver at
   module-init time (static initializers, lazy statics) will panic on Android. Audit
   `lumen_storage` init paths.

5. **Binary size.** A typical Rust Android APK with wgpu or femtovg is 20–35 MB
   uncompressed `.so`. This is acceptable for Phase 4 v1 but may need investigation
   if Play Store size limits become relevant.

6. **No automated CI on Android today.** Graphic tests use `gdigrab` (Windows-only)
   and cannot run in a standard Android emulator CI. A separate emulator-based smoke
   test (launch + fetch + render first frame) is sufficient for Phase 4 CI gates.

7. **Storage API changes.** Android 10+ (API 29) restricts direct filesystem access;
   `getExternalFilesDir` is required for user-visible files. The `browser_data_dir()`
   convention (`current_exe()/data`) does not apply to Android — the executable is a
   `.so`, not a PATH-locatable binary.

---

## Tests

| Test | What it checks |
|------|----------------|
| `cargo check -p lumen-shell --target aarch64-linux-android` | Codebase compiles for Android target (no link) |
| `cargo ndk -t aarch64-linux-android build -p lumen-shell` | Full `.so` links successfully |
| Manual: launch APK on emulator or device, open `http://example.com` | Page loads and renders |
| Manual: tap a link | Navigation fires via `InputCommand::Click` |
| Manual: swipe page | Scroll works with momentum |
| Manual: type URL, tap Go | Address bar accepts input, navigation starts |
| `cargo test -p lumen-shell` (host, existing) | No regressions on desktop from Android `#[cfg]` additions |
| Graphic tests `python graphic_tests/run.py --build` (Windows host) | Desktop rendering unchanged after all `#[cfg]` gating |

---

## Definition of done

- [ ] `cargo check -p lumen-shell --target aarch64-linux-android` passes with zero errors.
- [ ] `cargo ndk -t aarch64-linux-android build -p lumen-shell --release` produces a `.so`.
- [ ] A debug APK can be installed on an Android 8.0+ device or emulator via `adb install`.
- [ ] Opening `https://example.com` in the APK renders the page.
- [ ] Single-finger tap navigates links; vertical swipe scrolls.
- [ ] All desktop graphic tests still pass (no regression from `#[cfg]` additions).
- [ ] `CAPABILITIES.md` updated: Android row added.
- [ ] `docs/decisions/ADR-016-android-port.md` filed.
- [ ] `README.md` has a "Mobile / Android" build section.
