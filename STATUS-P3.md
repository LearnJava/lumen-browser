ROADMAP.md:118 — done (navapi blocked; last commit `e31a08ee` completed Steps 8–10)
ROADMAP.md:124 — ph3-color-management complete (Steps 1–7 done; remaining OPEN items belong to other feature owners or require hardware verification)
   - Step 1: done — commit `c486539c` (`DisplayColorProfile` trait, Windows GDI `GetICMProfileA`, `Lumen::target_color_space()`).
   - Step 2: done — commit `ff18c93a` (`IccProfile::build_rgb_transform_to(target)` DisplayP3/Rec2020 matrices, `RgbTransform.encode`, `ColorFloat::to_display(target)`).
   - Step 3: done — commit `72a7dfaa` (carry `ColorSpace` in `GradientStop`; `Color` derives `Default`).
   - Step 4: done — commit `510b0e6d` (wgpu renderer selects wide-gamut format `Rgba16Float` for DisplayP3/Rec2020; `Renderer::target_color_space()` getter; `WgpuBackend::target_color_space()`).
   - Step 5: done — commit `510b0e6d` (`set_canvas_background` wired into wgpu backend; `LoadOpChoice::Clear(wgpu::Color)`; sRGB→DisplayP3/Rec2020 gamma+matrix conversion at frame start; `Renderer::wgpu_color_for_canvas_bg`).
   - Step 6: done — commit `510b0e6d` (`decode_to(bytes, target)` added to `lumen-image`; `color_manage_in_place` target-aware; shell threads `self.target_color_space()` through all decode paths; per-pixel P3↔Rec2020 converters added).
   - Step 7: done — `WgpuBackend::is_wide_gamut()`; femtoovg 8-bit sRGB constraint documented.
   All per-crate tests pass (lumen-core 277, lumen-image 152, lumen-paint 813).
   Out-of-scope OPEN bugs (NOT ph3-color-management responsibility):
     - BUG-255/256: Navigation API state serialization + nav-key collision — belongs to `P1-ph3-navapi` feature owner.
     - BUG-257: GDI `GetICMProfileA` NULL-buffer early-return — requires runtime verification on physical P3 monitor; owned by shell/platform/display-color-profile.
     - BUG-252/253: already FIXED 2026-06-27 (clippy dead code + rec2020 OETF sign).
