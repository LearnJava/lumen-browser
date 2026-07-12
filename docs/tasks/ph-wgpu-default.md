# Ph-wgpu-default — Port `p1-exp-wgpu-only` optimizations, flip default render backend to wgpu

**Developer:** P1 · **Branches:** `p1-wgpu-default-<slice>` (one per stage) · **Size:** XL (staged) · **Crates:** `lumen-paint`, `lumen-shell`, `lumen-font`, `lumen-image`

Decision record: new ADR (number TBD when Phase 3 lands) — supersedes the closing note of
[ADR-010](../decisions/ADR-010-render-backend-abstraction.md) ("remove wgpu as default once
femtovg is proven in production; keep wgpu as compile-time fallback"). User decision
2026-07-12: port the Windows-only perf/reliability work from `p1-exp-wgpu-only` into `main`
while **keeping OpenGL/femtovg** (not removing it, unlike that branch), and make wgpu the
default backend once the port lands.

---

## Why

`p1-exp-wgpu-only` (permanent experimental polygon, never merges into main — user decision
2026-07-08) accumulated 31 commits of real wgpu-path wins: startup backend auto-probe
(works around BUG-057/BUG-274/BUG-275), font-metrics cache, parallel image prefetch +
separable box filter, structural display-list hashing, bbox-scissor filter passes,
viewport-cull, a scroll compositor (persistent strip + blit), static/animated split,
bbox-backdrop offscreen caching, image mip-chain, a texture pool fix for gradient masks,
and skip-identical-frame. That branch also deleted femtovg/glutin entirely — that decision
does **not** carry over here.

BUG-274 (open): wgpu backend burns ~4× more idle CPU than femtovg on Windows + a one-time
~450MB memory spike after start. Root-caused on the exp branch (`b0cc89d8`): DX12
`end_render_pass` costs a fixed ~2.3ms/pass regardless of frame area, ~270 passes/frame,
doesn't amortize. Vulkan is 12× cheaper on the same adapter but white-screens on some Intel
iGPUs (BUG-275, WSI/driver issue) — silent failure, submit succeeds, DWM presents nothing.
wgpu-over-GL (still wgpu's WGSL renderer, not a return to femtovg/glutin) was cheapest of
all three on that machine. **User decision (this task): fix BUG-274 before flipping the
default**, not after.

---

## Facts that shaped the slice order (from investigation, verify against current `main` before each slice — main moves)

- Both `lumen-paint` and `lumen-shell` already compile **both** backends by default
  (`crates/shell/Cargo.toml` — `default = ["backend-femtovg", "backend-wgpu", "quickjs"]`).
  Flipping the default is a `crates/shell/src/backend_factory.rs` runtime change, not a
  Cargo feature change. Today empty `LUMEN_BACKEND` and `"femtovg"` are the literal same
  code path (`create_femtovg_or_wgpu`); flipping the default means giving the empty arm its
  own path while keeping explicit `LUMEN_BACKEND=femtovg` deterministic.
- The DX12-vs-PRIMARY OS check (BUG-057 workaround) is duplicated in **three** places:
  `crates/engine/paint/src/renderer.rs:1630-1638` (windowed), `renderer.rs:1712-1720`
  (headless), `crates/engine/paint/src/webgpu_compute.rs:97-104` (separate `navigator.gpu`
  WebGPU context). All three must move to the probe together.
- `renderer.rs` has moved only +64 lines on `main` since the exp branch diverged
  (`02cc0f44`) — porting into it should be close to mechanical.
- `display_list.rs` is the real risk: `main` independently grew its own scroll-compositor
  track for femtovg (ADR-016, `overlay_partition.rs`, `BeginStickyLayer`/`BeginFixedLayer`
  markers) that the exp branch never saw. The structural-hash and static/animated-split
  slices need new match arms for those variants, and a real three-way merge, not a cherry-pick.
- No femtovg-vs-wgpu pixel-diff tooling exists (`CompareBackend` only works for cpu/vello
  combos — neither `WgpuBackend` nor `FemtovgBackend` implement `screenshot_rgba`). Don't
  build new tooling: `graphic_tests/run.py` captures the real window via `gdigrab`
  regardless of which backend is running — force `LUMEN_BACKEND=wgpu` and reuse the
  existing Edge-reference suite at the existing 0.5% threshold.
- `renderer.rs:6103` (`mask-grad-tex`) still creates a full-surface-size texture fresh on
  every `MaskComposite` every frame, unpooled — confirmed still present on `main`, the
  exp-branch texture-pool fix (slice 10) is real and not redundant with the existing
  ADR-008 `texture_pool.rs`/`layer_cache.rs`/`backdrop_cache.rs` (those aren't wired to this
  call site).

---

## Phases

### Phase 0 — cross-platform base (prerequisite, blocking)
`p1-wgpu-cross-platform` (per-target wgpu features: dx12/vulkan/metal) — waiting on a Linux
smoke+scroll validation report (`docs/tasks/linux-wgpu-validation.md`). Merge into main
first; without it, a wgpu-first default finds no adapter at all on Linux.

### Phase 1 — backend-selection reliability (closes BUG-274/275 before the default flips)
1. Port `crates/engine/paint/src/backend_probe.rs` from `origin/p1-exp-wgpu-only`
   (commit `da60a402`) — self-contained, Windows-only GDI `PrintWindow` presentation check,
   no new Cargo deps. Generalize the static candidate order per Phase 0's per-target
   features (Windows: DX12→Vulkan→GL; Linux: Vulkan→GL; macOS: Metal→GL).
2. Point all three OS-check sites (above) at the same selection function.
3. Fold in the ordering lessons from `b0cc89d8`/`36a4cbaa`/`988b8b51` (DX12 fixed per-pass
   cost; Vulkan cheapest-but-sometimes-white; wgpu-over-GL cheapest overall on that machine
   — re-verify on this machine, don't assume the numbers transfer).
4. Re-measure BUG-274 on `main` with the probe on/off; update `bugs/BUG-274-OPEN.md` /
   BUG-275 status.
5. Gate: idle-CPU protocol from BUG-274 (10s, `LUMEN_FRAME_LOG=1/2`) + `graphic_tests/run.py
   --only 00` smoke with `LUMEN_BACKEND=wgpu`.

### Phase 2 — perf slices, in this order (hard dependency chain, do not reorder)
Each slice: own `p1-wgpu-default-<slice>` worktree/branch, its own `LUMEN_NO_*` kill switch
(default = new behavior, escape hatch = old — same convention as the exp branch), accepted
by `LUMEN_BACKEND=wgpu python graphic_tests/run.py --continue-on-fail` at the existing 0.5%
threshold, compared against the femtovg-default baseline pass rate.

**Correction 2026-07-12:** the order below was re-derived from `git log --oneline --reverse
02cc0f44..origin/p1-exp-wgpu-only` directly (the first pass — done by a sub-agent — reported a
wrong chronology and skipped 4 real commits: `5423a84a`, `3eb396b3`, `ec45c8e6`, `fd2694d0`).
Confirmed while porting: `b298ba03` (skip-identical-frame) actually lands chronologically
*third* on the exp branch, long before the structural hash (`8305de10`) — it has no hard
dependency on that hash being fast, only on *some* `hash_display_list` existing (which it
already does on `main`), so it moves much earlier than originally planned.

1. Skip-identical-frame (`b298ba03`) — `content_generation: u64` + `last_frame_hash` on
   `Renderer`; every content-mutating method bumps `content_generation`, `render()` skips the
   GPU submit entirely on an unchanged hash. Independent — do not gate this on the structural
   hash slice below.
2. Allocation-free Debug-hashing (`5423a84a`) — `HashFmt`/`hash_one_command` reused in
   `display_list_cache.rs`'s `hash_commands` and `tile_grid.rs`'s diff path (was
   `format!("{cmd:?}")` per command per frame). Small, mechanical, same Debug representation
   so hash values are unchanged.
3. Scroll-container in-place DL patch (`3eb396b3`) — `patch_scroll_layer` mutates only the
   scroll offsets in `PushScrollLayer` + scrollbar-thumb rects in-place instead of rebuilding
   the whole display list on every wheel tick (~12×). Touches `display_list.rs`'s existing
   scroll-container path — check for interaction with main's own fixed/sticky markers before
   porting (same caution as slice 5 below, smaller surface).
4. Font-metrics cache (`0013b715`) — `OwnedCmap` (`lumen-font/src/cmap.rs`),
   `FaceMetrics`/`LazyParsedFaces`/`resolve_cache_key` (`renderer.rs`). Independent, low risk.
5. Box filter + parallel image prefetch (`2ff7183e`) — separable `resize_area_avg`
   (`lumen-image/src/lib.rs`), `prefetch_image_resizes_parallel`/`prefetch_faces_parallel`
   (`renderer.rs`). Must land before slice 12 (mip-chain supersedes its CPU path).
6. Structural display-list hash (`8305de10`) — `hash_command_into`/`hash_one_command`
   (`display_list.rs`). **Must add match arms for `BeginStickyLayer`/`EndStickyLayer`/
   `BeginFixedLayer`/`EndFixedLayer`** (main-only variants, not seen on the exp branch) or
   the hash is silently wrong on fixed/sticky pages.
7. bbox-scissor filter passes / `LevelBounds` (`6d13d5be`) — depends on slice 6. Slices 8, 11
   depend on this.
8. Viewport-cull invisible layers (`34a53113`) — depends on slice 7.
9. wgpu scroll compositor, persistent strip+blit (`0dadfb1c`) — large/invasive. Keep as a
   **wgpu-specific** mechanism coexisting with main's already-merged femtovg path
   (`overlay_partition.rs`, ADR-016) — do not try to unify them in this task. Must correctly
   pass through main's fixed/sticky markers. Slices 10, 13, 14 depend on this.
10. Fast-scroll degradation (`ec45c8e6`) — EMA scroll-speed hysteresis (enter ≥48, exit <12
    CSS px/frame) freezes CSS animation/transition ticks + GIF/video-texture updates during a
    fast scroll so the display list stays scroll-stable and hits the slice-9 compositor's
    page-compose fast path instead of a monolithic repaint. Depends on slice 9.
11. Static/animated split compositor (`7d867742`) — **highest-risk slice.** Depends on
    7/8/9. Requires a genuine three-way merge of `display_list.rs` against main's
    independent M3.x work. Plan a dedicated session for this slice alone.
12. bbox-offscreen backdrop filter cache (`3f49e673`) — depends on slice 7.
13. VRAM hygiene (`fd2694d0`) — evicted level-textures return to the pool instead of being
    dropped (band↔window resize flap), band-depth texture cached in `PageBandCache` instead of
    recreated per band-miss. Depends on slice 9's `PageBandCache`. Time-neutral, VRAM-only.
14. Image mip-chain (`5e6905c4`) — port after slice 5; supersedes its CPU-resize path behind
    a kill switch, does not delete slice 5's code.
15. Texture pool for gradient masks (`03b8599d`) — independent. Fixes `renderer.rs:6103`.
    Add a kill switch (none existed on the exp branch — add one for A/B parity).

### Phase 3 — flip the default + docs
1. `backend_factory.rs`: empty `LUMEN_BACKEND` tries wgpu (via the Phase 1 probe) first,
   femtovg as fallback on init failure. Explicit `LUMEN_BACKEND=femtovg` stays deterministic
   — do not touch that arm.
2. New ADR superseding ADR-010's closing note, with the BUG-057/274/275 mitigation and the
   ported perf work as justification; femtovg explicitly retained, not removed.
3. Doc-sync per `CLAUDE.md`'s update matrix: `CAPABILITIES.md`, `subsystems/paint.md`,
   `README.md` (if it names the default backend), `docs/decisions/README.md`, `STATUS-P1.md`.
4. Final gate: full `graphic_tests/run.py` with empty `LUMEN_BACKEND` (the new real default)
   vs. the saved femtovg-default baseline pass rate. New regressions get fixed or become a
   `KNOWN_DEBTOR` with the same justification discipline as existing debtors.

---

## Progress (2026-07-13) — Phase 2 ported in full

Slices 1–6 landed as individual branches (see `git log`: backend-probe, skip-frame,
debug-hash, scroll-patch, font-metrics, box-filter). The remaining exp-branch commits were
then ported **in one batch** (user decision 2026-07-12: port everything first, compile/test
once at the end) on branch `p1-wgpu-default-structhash` via sequential `git cherry-pick -n`
in exp-branch chronological order, resolving conflicts by hand, excluding `EXPERIMENT.md`,
`scripts/exp/*` (PowerShell tooling stays unported) and bug-file renames:
structural hash (`8305de10`) → frame diagnostics (`b0cc89d8` diag part, `36a4cbaa`
LUMEN_PRESENT, `adf81070`+`561dd72d` bench_frames/LUMEN_BENCH stand) → bbox-scissor
(`6d13d5be`) → viewport-cull (`34a53113`) → scroll compositor strip+blit (`0dadfb1c`) →
fast-scroll degradation (`ec45c8e6`) → static/animated split (`7d867742`, incl. the 3-way
merge with main's M0.4 page-offset fast path in `shell/main.rs` — fast path kept for
femtovg, `render_with_anim` wired into the fallback/wgpu path) → bbox-backdrop (`3f49e673`)
→ VRAM hygiene (`fd2694d0`) → image mip-chain (`5e6905c4`) → gradient-mask pool
(`03b8599d`). Single batch gate: paint+shell clippy clean, lumen-paint 1025 tests green,
both backends smoke-tested in a real window, CPU-screenshot pixel parity vs pre-batch main
(max delta 1, ≤0.023% px — box-filter quantization class). Remaining: Phase 0 (Linux),
Phase 1 re-measure (BUG-274), Phase 3 flip + ADR + BUG-276.

## Notes

- `p1-exp-wgpu-only` is read-only source material (`git show origin/p1-exp-wgpu-only:<path>`)
  — never merge it, never branch from it.
- No CI on Linux/macOS in this repo; Phase 0 and any cfg-gated non-Windows code in later
  slices needs manual validation on a real machine (see `docs/tasks/linux-wgpu-validation.md`).

## Finding from the Phase 1 backend-probe slice (2026-07-12) — wgpu does not pass `graphic_tests` today

Running the acceptance gate this brief specifies (`LUMEN_BACKEND=wgpu python graphic_tests/run.py
--only 00`) for the very first time (this suite has only ever been run against the femtovg
default before — `LUMEN_BACKEND=wgpu` was never exercised through it) surfaced:
`TEST-00: FAIL (4.85%) [x:1-1022 y:684-718]`, reproduced twice, while femtovg passes the same
test at `0.00%`. The diff band sits in the bottom ~35px of the 720px-tall viewport, nearly full
width — consistent with either a scrollbar/chrome rendering difference or a capture/calibration
offset specific to the wgpu window, not yet root-caused.

This is **not** a regression from the backend-probe port — that slice only changes which wgpu
adapter/backend gets picked (probe accepted DX12, same backend wgpu always defaulted to on
Windows before this task); no rendering code was touched. It means the wgpu path likely never
had a clean baseline against the Edge references to begin with. **Before trusting any Phase 2
slice's "compare pass-rate against baseline" gate, run the full suite once with
`LUMEN_BACKEND=wgpu` to establish what that baseline actually is** (it is very likely below
100%, unlike the femtovg default) — do not assume parity. Root-causing this specific TEST-00 gap
is a separate, un-scoped follow-up; flag it before Phase 3's final default-flip gate at the
latest.
