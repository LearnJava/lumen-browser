# Linux/Vulkan validation — wgpu cross-platform

**Developer:** any (this brief is written for a fresh Claude Code session on a
Linux machine) · **Branch:** `main` · **Crates:**
`lumen-paint`, `lumen-shell`, `lumen-driver`

## Context

The cross-platform wgpu feature split is already on `main` (merged via
`p1-wgpu-vkgl`): `crates/engine/paint/Cargo.toml` selects `dx12+vulkan+gles`
on Windows, `vulkan` on Linux, `metal` on macOS via three
`[target.'cfg(target_os = "...")'.dependencies]` blocks. Full writeup:
[`docs/build-speed.md`](../build-speed.md) §4.2.

**The Vulkan path has never been run on real Linux hardware.** This session's
job is to confirm it works.

## Step 1 — smoke test (do this first, report back before anything else)

```bash
git fetch origin
git checkout main
cargo check -p lumen-paint --features backend-wgpu
cargo clippy -p lumen-paint --features backend-wgpu -- -D warnings
cargo test -p lumen-paint --features backend-wgpu
```

Then run the actual browser against wgpu:

```bash
LUMEN_BACKEND=wgpu cargo run -p lumen-shell -- samples/page.html
```

**What "success" looks like:** window opens, page renders, no panic/adapter
error on stderr. If it fails, capture the full stderr — the failure mode
matters (no adapter found vs. panic vs. wrong pixels vs. crash on present) and
should go straight into a new `BUG-NNN` entry in `BUGS.md` (see `bugs/` dir
for the format), not fixed ad hoc.

## Step 2 — scroll test

Compare femtovg vs wgpu on a scroll-heavy page:

```bash
cargo run -p lumen-shell -- samples/page.html                # femtovg, control group
LUMEN_BACKEND=wgpu cargo run -p lumen-shell -- samples/page.html
```

Scroll both (PageDown/mouse wheel) and visually compare: does content render
correctly while scrolling, no tearing/flicker/missing content, comparable
smoothness. Manual/visual check first — if wgpu is fundamentally broken on
Linux, automated tooling is irrelevant.

**Known gap — the existing automated scroll/pixel-diff harness is
Windows-only:**
[`scripts/scroll_blit_accept.py`](../../scripts/scroll_blit_accept.py)
captures via `ffmpeg gdigrab` (Windows Desktop Duplication API). On Linux this
needs `ffmpeg -f x11grab` (X11) or a `pipewire`/`kmsgrab` equivalent
(Wayland). Port this only if Step 2's manual check passes and pixel coverage
matters.

Headless alternative: `lumen-driver`'s `backend-wgpu` feature does headless
GPU snapshots via `WinitSession` — if it reads back pixels directly from the
GPU surface it should work on Linux without a screen-capture shim.

## Step 3 — perf, only after Steps 1–2 pass

`BUG-274` (`bugs/BUG-274-OPEN.md`) documents wgpu burning ~4× more idle CPU
than femtovg on Windows with DX12. Whether this is DX12-specific or present on
Vulkan too is unknown — worth a quick idle-CPU check (`top`/`htop`, femtovg vs
wgpu, 10 s idle) once the smoke test passes. Informational for whoever picks up
BUG-274 next.

## Out of scope

`p1-exp-wgpu-only` is a **permanent experimental branch, never merged into
main** (user decision 2026-07-08). Do not port anything from it in this session.

## Reporting back

Update the Result section below and commit directly on `main` (this is a
docs-only change), or report results in chat.

### Result (fill in after running Steps 1–2)

- Smoke test (Step 1): _pending_
- Scroll test (Step 2): _pending_
- Idle-CPU spot check (Step 3): _pending_
