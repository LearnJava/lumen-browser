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

- Smoke test (Step 1): _pending_
- Scroll test (Step 2): _pending_
- Idle-CPU spot check (Step 3): _pending_
