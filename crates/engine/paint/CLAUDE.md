# crates/engine/paint — context for paint / rendering work

Loaded when a session works under `crates/engine/paint/`. Root rules are in [`/CLAUDE.md`](../../../CLAUDE.md); detail in [`docs/graphic-tests.md`](../../../docs/graphic-tests.md).

- **`--screenshot` (CPU, `cpu_raster.rs`) and the live window (wgpu, `renderer.rs`) are independent implementations of every `DisplayCommand`.** A new command must be implemented in both; a match on one proves nothing about the other ([`docs/invariants.md`](../../../docs/invariants.md) §Rendering).
- **Three golden sets drift independently** — the Edge pixel diff (`graphic_tests/run.py`), the deterministic CPU PNGs (`graphic_tests/snapshots/cpu/`) and the textual display-list snapshots (`tests/snapshots/*.snap`, `UPDATE_SNAPSHOTS=1 cargo test -p lumen-paint --test all <name>`). A paint change regenerates the affected ones **in the same commit**, or it turns someone else's gate red.
- **Any paint/scroll timing is meaningless without its wgpu backend** — read the `[wgpu] adapter: … (Vulkan|Dx12)` line in stderr; pin it with `WGPU_BACKEND=vulkan|dx12|gl`. The same adapter measured 116 ms/frame on DX12 and 53 ms on Vulkan.
- The backend is chosen at runtime (wgpu; femtovg only as the init-failure fallback), not by a cargo feature — [ADR-017](../../../docs/decisions/ADR-017-wgpu-default-backend.md).
