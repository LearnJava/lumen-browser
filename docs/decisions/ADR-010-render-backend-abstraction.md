# ADR-010: RenderBackend abstraction — femtovg now, vello later

## Status

Accepted

## Date

2026-06-03

## Context

The paint crate (`lumen-paint`) currently has a single concrete `Renderer` struct that
talks directly to wgpu. This caused BUG-057: on Windows the Vulkan wgpu backend
produces a double-panic on the first rendered frame (encoder invalidated, then Surface
drop races SurfaceTexture during stack unwind). The workaround — defaulting to DX12 on
Windows — works, but reveals a deeper problem: the codebase is hard-coupled to one GPU
API with no fallback.

Two other forces push toward an abstraction layer:

1. **femtovg** is a stable, purpose-built 2D GPU renderer (OpenGL ES 2.0) that covers
   every `DisplayCommand` natively (paths, gradients, text, clips, layers). It eliminates
   our hand-written WGSL shaders and is available identically on Windows / Linux / macOS.

2. **vello** (Linebender / Google Fonts) is a next-generation compute-based 2D renderer
   that achieves pixel-perfect subpixel coverage without CPU tessellation. Its API is
   still evolving (0.x); premature tight coupling would mean rewriting render code on
   every vello release.

Constraints:
- Must not break the existing CI snapshot gate (`cargo test -p lumen-driver --features cpu-render`).
- Must allow comparing two backends on the same page (validate vello before promoting it).
- Changing backend must not require editing `DisplayCommand` or layout/shell code.
- vello API updates must touch exactly one file in the repository.

## Decision

Introduce a `RenderBackend` trait in `lumen-paint::backend`. All rendering goes through
this trait. Backends live in `lumen-paint::backends::*` and are gated by feature flags.

### Trait (stable contract)

```rust
pub trait RenderBackend: Send {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError>;

    fn resize(&mut self, width: u32, height: u32);
    fn set_scale_factor(&mut self, scale: f64);
    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String>;
    fn clear_images(&mut self);
    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>);

    /// Returns raw RGBA pixels after the last render() call.
    /// Only headless backends need to implement this; windowed ones return None.
    fn screenshot_rgba(&mut self) -> Option<Vec<u8>> { None }
}
```

`DisplayCommand` and all layout types remain unchanged — the trait is the only
coupling point between paint and the rest of the engine.

### Backends and feature flags

```toml
# lumen-paint/Cargo.toml
[features]
default        = ["backend-femtovg"]
backend-wgpu   = ["dep:wgpu"]
backend-femtovg = ["dep:femtovg", "dep:glutin"]
backend-vello  = ["dep:vello", "dep:wgpu"]   # wgpu used as vello's surface layer
backend-cpu    = []                           # tiny-skia, already in lumen-driver
compare        = []                           # enables CompareBackend (needs two other features)
```

```
backends/
    wgpu_backend.rs      feature = "backend-wgpu"
    femtovg_backend.rs   feature = "backend-femtovg"   ← Phase 2 default
    vello_backend.rs     feature = "backend-vello"     ← Phase 3 default
    cpu_backend.rs       feature = "backend-cpu"       ← CI / no-GPU
    compare_backend.rs   feature = "compare"           ← testing only
```

### vello isolation strategy

All vello imports live exclusively in `vello_backend.rs`. The file exposes only
`VelloBackend` which implements `RenderBackend`. When vello releases a breaking API
change, only `vello_backend.rs` is edited. No other file references `vello::*`.

Internal structure of `vello_backend.rs`:

```
translate_to_scene(commands: &[DisplayCommand]) -> vello::Scene   ← vello API here
VelloBackend::render()  →  calls translate_to_scene + submit      ← vello API here
```

Everything above `VelloBackend` (trait impls, shell, driver) sees no vello types.

### Parallel / compare mode

`CompareBackend` holds `primary: Box<dyn RenderBackend>` and
`secondary: Box<dyn RenderBackend>`. On each `render()` call it renders both, takes
screenshots, computes pixel diff percent, and logs discrepancies. Used in
`lumen-driver` compare tests:

```bash
cargo test -p lumen-driver --features compare-femtovg-vello
```

Output per page:
```
01-colors.html      femtovg vs vello  0.1%  ✅
30-css-filter.html  femtovg vs vello  3.2%  ⚠️
```

This is how vello will be validated before being promoted to default.

### Migration path

| Phase | Default backend | Fallback |
|---|---|---|
| Phase 1 (now) | wgpu (DX12 on Win) | cpu |
| Phase 2 | femtovg | wgpu → cpu |
| Phase 3 | vello | femtovg → wgpu → cpu |

Shell constructs `Box<dyn RenderBackend>` from a factory that respects:
1. `LUMEN_BACKEND` env var (`wgpu` / `femtovg` / `vello` / `cpu`)
2. Compiled-in default from feature flags
3. Auto-fallback: if preferred backend fails init, try next in chain

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Stay with wgpu only | Driver-specific crashes (BUG-057); no path to vello |
| OpenGL via `glow` directly | More work than femtovg for same result; femtovg already wraps OpenGL correctly for 2D |
| CPU rendering (softbuffer) only | 10–50× slower on complex pages; unacceptable for production |
| Tight-couple vello now | vello API breaks every few months; would cause constant churn |

## Consequences

- **Positive:** backend is a swap — shell and layout untouched when changing GPU API;
  CI can run cpu backend with zero GPU; femtovg eliminates all hand-written WGSL shaders;
  comparison testing validates vello rigorously before promoting it.
- **Negative / trade-offs:** additional abstraction layer; femtovg adds `glutin` dep;
  initial refactor touches shell + paint crates.
- **Future:** when vello reaches 1.0 and API stabilises, run full compare suite
  (femtovg as reference), then flip default. Remove wgpu as default once femtovg is
  proven in production; keep wgpu as compile-time fallback.
