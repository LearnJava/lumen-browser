# Linux/Vulkan validation — wgpu cross-platform fix

**Developer:** any (this brief is written for a fresh Claude Code session on a
Linux machine) · **Branch:** `p1-wgpu-cross-platform` (start here) · **Crates:**
`lumen-paint`, `lumen-shell`, `lumen-driver`

## Context

Commit `eec18b02` on this branch fixed a real bug: wgpu's graphics-backend
feature (`dx12`) was hardcoded for **all** OSes in the root `Cargo.toml`
`[workspace.dependencies]`. DX12 is Windows-only — on Linux, a build with only
`dx12` compiled into wgpu has no working graphics backend at all, so
`LUMEN_BACKEND=wgpu` would fail to find any GPU adapter. Fixed by moving the
feature choice into `crates/engine/paint/Cargo.toml` as three
`[target.'cfg(target_os = "...")'.dependencies]` blocks: `dx12` on Windows,
`vulkan` on Linux, `metal` on macOS. Full writeup:
[`docs/build-speed.md`](../build-speed.md) §4.2 "Уточнение 2026-07-12".

**This was verified compile-clean on Windows only.** Nobody has run the Vulkan
path at all yet — that is this session's job.

## Step 1 — smoke test (do this first, report back before anything else)

```bash
git fetch origin
git checkout p1-wgpu-cross-platform   # or: git worktree add ../lumen-linux-check p1-wgpu-cross-platform
cargo check -p lumen-paint --features backend-wgpu
cargo clippy -p lumen-paint --features backend-wgpu -- -D warnings
cargo test -p lumen-paint --features backend-wgpu
```

Expect all green (this already passed on Windows; the point here is
confirming the `vulkan` feature compiles — the Windows session could not
check this).

Then run the actual browser against wgpu:

```bash
LUMEN_BACKEND=wgpu cargo run -p lumen-shell -- samples/page.html
```

**What "success" looks like:** window opens, page renders, no panic/adapter
error on stderr. If it fails, capture the full stderr — the failure mode
matters (no adapter found vs. panic vs. wrong pixels vs. crash on present) and
should go straight into a new `BUG-NNN` entry in `BUGS.md` (see `bugs/` dir
for the format) rather than being fixed ad hoc, since a P3 developer may need
to pick it up.

## Step 2 — scroll test

Compare the default backend against wgpu on a scroll-heavy page:

```bash
cargo run -p lumen-shell -- samples/page.html                # femtovg, control group
LUMEN_BACKEND=wgpu cargo run -p lumen-shell -- samples/page.html
```

Scroll both (PageDown/mouse wheel) and visually compare: does content render
correctly while scrolling, no tearing/flicker/missing content, comparable
smoothness. This is a **manual/visual** check to start — do not invest in
porting automated tooling until this passes, since if the wgpu path is
fundamentally broken on Linux no amount of tooling matters yet.

**Known gap — the existing automated scroll/pixel-diff harness is
Windows-only and will not run as-is:**
[`scripts/scroll_blit_accept.py`](../../scripts/scroll_blit_accept.py) (used
for M3.2.1c-6 acceptance, see
[`docs/tasks/ph3-render-multithreading.md`](ph3-render-multithreading.md)
line ~1565) captures via `ffmpeg` **gdigrab**, a Windows Desktop Duplication
API input — it does not exist on Linux. `run.py --live` (graphic_tests
harness) has the same gdigrab dependency. If Step 2's manual check passes and
deeper Linux-side pixel-diff coverage becomes worth the investment, the
capture mechanism needs a Linux equivalent (`ffmpeg -f x11grab` under X11, or
a Wayland-specific capture under `pipewire`/`kmsgrab` — check which display
server is in use first) — that is new work, not a drop-in swap.

Headless alternative worth checking first (may avoid the screen-capture
problem entirely): `lumen-driver`'s `backend-wgpu` feature does headless GPU
snapshots via `WinitSession` (`crates/driver/Cargo.toml` — "driver uses
Renderer directly for headless GPU snapshots"). If this reads back pixels
from the GPU surface directly rather than grabbing the OS window, it should
be capture-mechanism-agnostic and might already work on Linux without
porting anything. Worth a spike before writing new x11grab tooling.

## Step 3 — perf, only after Steps 1–2 pass

`BUG-274` (open, `bugs/BUG-274-OPEN.md`) documents wgpu burning ~4× more idle
CPU than femtovg *on Windows*, with cheaper scroll CPU. Whether this
regression is DX12-specific or present on Vulkan too is unknown — worth a
quick idle-CPU comparison (`top`/`htop` on the Lumen process, femtovg vs wgpu,
10s idle) once the smoke test passes. Do not chase this deeply yet; it is
informational for whoever picks up BUG-274 next, not this session's primary
goal.

## Out of scope for this session (separate, bigger track)

`p1-exp-wgpu-only` is a **permanent experimental branch, never merged into
main** (user decision 2026-07-08) — entry point is `EXPERIMENT.md` at that
branch's worktree root. It carries real wins (texture pooling, mip-chain
images, scroll strip+blit, static/animation split — beats Edge on one
synthetic benchmark) but:
- it removed OpenGL entirely (wgpu-only architecture bet, not what this repo
  ships)
- its measurement tooling (`scripts/exp/proc_stats.ps1`,
  `run_warm_frame_bench.ps1`) is **PowerShell, Windows-only**

Do not start porting that branch's optimizations to Linux in this session.
If Steps 1–2 above pass and there is appetite to extend that experimental
track to Linux, that is a separate, explicitly-scoped follow-up (would need
Linux equivalents for the PowerShell measurement scripts first) — ask the
user before taking it on.

## Reporting back

This is a two-machine handoff (Windows session in `.claude/worktrees/wgpu-cross-platform`,
this Linux session working from a fresh clone/checkout of the same branch).
Report results as a commit on `p1-wgpu-cross-platform` (update this file's
"Result" section below) or, if working interactively with the user, plainly
in chat — the Windows-side session doesn't have visibility into this machine
otherwise.

### Result (fill in after running Steps 1–2)

Linux session 2026-07-12: CachyOS (Arch), KDE/Wayland, Intel iGPU (Mesa Vulkan),
Rust 1.97 stable.

- **Smoke test (Step 1): PASS** (after two Linux-only fixes, see below).
  `cargo check` / `cargo clippy -- -D warnings` / `cargo test` for `lumen-paint
  --features backend-wgpu` all green; 1003 + 34 tests passed. `LUMEN_BACKEND=wgpu`
  opens a window on Wayland, finds the Vulkan adapter (direct `create_wgpu` path,
  no fallback), renders `samples/page.html` correctly — verified by a real window
  screenshot (colors, backgrounds, link, list all present; no panic, no adapter
  errors on stderr).
  - **Fix 1 (this branch):** `crates/shell/src/platform/display_color_profile.rs`
    had `pub use NullDisplayColorProfile as PlatformDisplayColorProfile;` without
    the `lumen_core::ext::` path — E0432 on every non-Windows build of
    `lumen-shell` (the `cfg(not(windows))` branch had never been compiled).
    Replaced with a proper non-Windows `PlatformDisplayColorProfile` impl
    (always sRGB) so the `::new()` call site works on all OSes.
  - **Fix 2 (this branch):** 11 new clippy 1.97 lints (`byte_char_slices`,
    `question_mark`) in `lumen-image`/`lumen-dom`/`lumen-layout`/`lumen-paint` —
    pre-existing code, surfaced because `rust-toolchain.toml` pins `channel =
    "stable"` (not a version), and Linux stable is already 1.97. Fixed
    mechanically (`cargo clippy --fix`).
  - Machine note: `.cargo/config.toml` hardcodes `rustc-wrapper = "sccache"`;
    sccache is not installed on this Linux box — worked around per-command with
    `RUSTC_WRAPPER=""`. Consider guarding the wrapper per-OS or documenting.
- **Scroll test (Step 2): PASS (wgpu), femtovg comparison not run yet.**
  Manual gdigrab tooling was not needed: `--mcp-live-port` + the `scroll` tool
  drives the real window, real-pixel capture via KDE `spectacle` on the active
  window (KWin script activates the Lumen window). Scroll on
  `graphic_tests/1000000-final.html` (2013 DOM nodes, 12 images) works under
  wgpu/Vulkan: `window.scrollY` tracks deltas, continuous ±120 px scrolling for
  20 s — no panic, no visual corruption. Note: MCP `screenshot` resource renders
  via `render_to_image_cpu`, i.e. it does NOT capture actual GPU output — real
  wgpu-vs-femtovg pixel comparison needs window capture (spectacle works).
- **Idle-CPU spot check (Step 3): wgpu/Vulkan does NOT show the BUG-274 idle
  regression.** Release build, `1000000-final.html`, vs Chromium (same page,
  isolated profile, CDP-driven wheel events at the same ~7 events/s):

  | | idle CPU (10 s) | scroll CPU (20 s) | memory (PSS) |
  |---|---|---|---|
  | Lumen wgpu/Vulkan (1 proc) | **0.4 %** | 17.2 % | **347 MiB** |
  | Chromium (15 procs, sum) | 0.9 % | **14.1 %** | 460 MiB |

  Lumen wins on idle CPU and memory; loses ~20 % on scroll CPU. Likely because
  every scroll re-renders the full display list while Chromium composites
  pre-rasterized layers. The scroll strip+blit work on `p1-exp-wgpu-only` is the
  known candidate fix — porting it is the separate follow-up track (see "Out of
  scope"). Caveat: Chromium smooth-scrolls wheel events (animation frames), so
  per-event work is not perfectly identical; numbers are indicative, not a gate.

### Scroll-bench matrix (2026-07-12/13, Linux)

Harness ported from the experimental branch: `crates/shell/src/bench_frames.rs`
(`LUMEN_BENCH=scroll:N:W:STEP:PACE`) + `LUMEN_PRESENT` in the wgpu renderer +
Linux runner `scripts/bench_scroll.py` (drives full top-to-bottom-and-back
passes, samples CPU/PSS from /proc, reports median/p95 frame + scroll speed).
Two drivers: `--driver bench` (LUMEN_BENCH, **stalls on Wayland** — see below)
and `--driver mcp` (default; scrolls via the MCP live window, paced to the
renderer by the `[frame]` log — the interactive input path).

Fixed on the way (all found by this validation):
- **Shell never implemented the `RenderBackend` contract for `SurfaceLost`**
  (backend.rs: "shell вызовет resize и повторит кадр") — one lost surface
  permanently killed rendering. Now recovers via swapchain reconfigure.
- **Windowed wgpu created its depth texture from the headless placeholder
  size** — instant validation panic on X11 (no initial Resized there).
- `LUMEN_PRESENT` requests are validated against surface capabilities
  (Wayland has no `Immediate`) with Immediate → Mailbox → Fifo fallback.
- `surface_error_to_render_error` now logs the concrete variant
  (Lost/Outdated/Timeout) — they are indistinguishable downstream.

Numbers (frame = median `[frame] total`; CPU = process average over the run;
Chromium via CDP rAF loop, same pages, same step, isolated profile):

| Page | Lumen wgpu | Lumen femtovg | Chromium (tree) |
|---|---|---|---|
| samples/page.html, step 120 | **0.88 ms**, 2780 px/s, 3.3 %, 98 MiB | 0.67 ms, 2740 px/s, 3.2 %, 135 MiB | vsync 16.7 ms, no scroll (page fits its window), 443 MiB |
| 1000000-final, step 60 | 132 ms → **7.5 fps**, 435 px/s, 18 %, 339 MiB | 478 ms → 2 fps, 123 px/s, 63 %, 280 MiB | 60 fps, 3370 px/s, **93.5 %**, 537 MiB |
| bench-static-scroll, step 200 | **wedged** (BUG-276): ~0.5 fps, SurfaceLost×21/run | 1049 ms → 1 fps, 190 px/s, 21 %, 143 MiB | 60 fps, 11763 px/s, 37 %, 454 MiB |

Reading: wgpu beats femtovg 3.6× on the stress page (Windows exp measurement
was 19×) and matches it on light pages sub-millisecond. Against Chromium the
unoptimized main renderer loses the heavy-page scroll war (7.5 fps vs 60 fps)
while using 5× less CPU and less RAM; the "faster than Chromium" result
(3.9 ms vs 9.5 ms on bench-anim-scroll) belongs to the `p1-exp-wgpu-only`
optimizations (band compositor + anim split) which are NOT in main. Porting
those (bbox-scissor → viewport-cull → strip+blit) is what closes the gap;
BUG-276 tracks the blur-page swapchain wedge they would fix.

**Wayland harness caveat:** the `LUMEN_BENCH` about_to_wait redraw loop stalls
swapchain acquire on KWin/Wayland (Timeout every ~4 frames = swapchain depth)
even when paced and visible; occluded windows get no frame callbacks at all
(the bench then measures nothing). The MCP driver paced to `[frame]` feedback
is the reliable Linux path; `LUMEN_BENCH` remains correct on Windows and for
`--dump`-style use. Root cause of the Wayland acquire starvation not yet
diagnosed — candidate follow-up if the harness is needed there.
