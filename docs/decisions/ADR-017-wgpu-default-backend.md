# ADR-017: wgpu as default render backend (supersedes ADR-010 closing note)

## Status

Accepted

## Date

2026-07-13

## Context

ADR-010 (2026-06-03) established the femtovg → wgpu → cpu fallback chain and closed
with: *"Remove wgpu as default once femtovg is proven in production; keep wgpu as
compile-time fallback."* That note was written before the Phase 3 render-pipeline work
identified performance and reliability requirements that actually favour wgpu over femtovg
as the default.

Three conditions pushed this decision forward in 2026-07:

1. **BUG-274 / BUG-275 mitigated by `backend_probe`.** The idle-CPU regression on DX12
   (fixed per-pass cost, ~2.3 ms/pass, ~270 passes/frame) and the Vulkan white-screen on
   some Intel iGPUs (WSI/driver bug, undetectable from wgpu error scopes) are now handled
   by a real presentation probe (`backend_probe::pick_backend`): a test frame is rendered
   and the DWM-composited result captured via `PrintWindow` — the first candidate whose
   frame is actually visible is selected. On the tested Intel Iris Plus: Vulkan=WHITE,
   GL=WHITE (wgpu-GLES also white-screened), DX12=ok → probe correctly selects DX12.

2. **Phase 2 perf slices ported from `p1-exp-wgpu-only`.** Skip-identical-frame,
   allocation-free debug-hashing, scroll-container in-place DL patch, font-metrics cache,
   box filter + parallel image prefetch, structural display-list hash, bbox-scissor filter
   passes / `LevelBounds`, viewport-cull, scroll compositor (persistent strip+blit),
   fast-scroll degradation, static/animated split compositor, bbox-offscreen backdrop
   filter cache, VRAM hygiene (evicted level textures returned to pool), image mip-chain,
   texture pool for gradient masks. These slices landed on `main` making wgpu competitive
   with femtovg for everyday browsing.

3. **wgpu baseline established.** `LUMEN_BACKEND=wgpu` was run against the full
   `graphic_tests` Edge-reference suite for the first time (2026-07-13): 65 PASS / 38 FAIL
   / 38 DEBTOR (141 total). The 38 FAIL items are tracked in BUG-277 and are pre-existing
   wgpu-vs-Edge differences, not regressions introduced by this flip. The Phase 3 gate
   (BUG-277 resolution) closes them or promotes them to justified `KNOWN_DEBTOR` entries.

femtovg is **not removed** — it remains the explicit `LUMEN_BACKEND=femtovg` path and the
fallback when wgpu init fails.

## Decision

Flip the default backend in `backend_factory.rs`: when `LUMEN_BACKEND` is empty, try
wgpu first (which internally runs `backend_probe::pick_backend` on Windows to select the
best API — Vulkan → GL → DX12); fall back to femtovg on wgpu init failure.

Explicit `LUMEN_BACKEND=femtovg` is unchanged (femtovg → wgpu fallback — the Phase 2
chain).

`LUMEN_BACKEND=wgpu` is unchanged (direct wgpu, no fallback).

Phase 0 (Linux) cross-platform wgpu features are compiled in but the probe is
Windows-only; on Linux the static preference chain (Vulkan → GL) applies directly.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep femtovg as default indefinitely | BUG-274 (idle-CPU) and BUG-275 (white-screen) are now mitigated; Phase 2 perf slices make wgpu viable; delaying further has no upside |
| Flip default after BUG-277 is fully resolved | BUG-277 items are pre-existing differences vs Edge, not introduced by this commit; flipping earlier exposes them for faster triage |
| Remove femtovg entirely (like `p1-exp-wgpu-only`) | femtovg is the stable fallback for GPUs where wgpu probe fails; removing it would lose the safety net on older/unusual hardware |
| Wait for vello (ADR-010 Phase 3 plan) | vello 0.x API still evolving; premature promotion would cause constant churn; wgpu is a better intermediate default |

## Consequences

- **Positive:** wgpu path gets daily real-world exercising (all users on default config);
  probe auto-selects the correct API for each machine; Phase 2 perf work is active for
  all users by default; wide-gamut output surface (Rgba16Float on DisplayP3/Rec.2020)
  available without `LUMEN_BACKEND=wgpu`.
- **Negative / trade-offs:** BUG-277 failures visible to default users until resolved;
  wgpu startup includes probe time (~200–1000 ms one-time on first launch, window briefly
  shows probe-colour frames); idle-CPU on wgpu/DX12 higher than femtovg (~2391 ms/10s vs
  ~219 ms/10s on Intel Iris Plus — confirmed 2026-07-13; tracked in BUG-274).
- **Future:** once BUG-277 is fully resolved (all 38 tests PASS or justified KNOWN_DEBTOR),
  the wgpu default is confirmed stable. If vello reaches 1.0 API stability, revisit
  ADR-010 §Migration path to evaluate a vello default via the same compare-suite gate.
