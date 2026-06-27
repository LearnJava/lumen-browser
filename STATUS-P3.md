1: ROADMAP.md:118 — done (navapi blocked; last commit `e31a08ee` completed Steps 8–10)
2: ROADMAP.md:124 — ph3-color-management in progress
3:    - Step 1: done — commit `c486539c` (`DisplayColorProfile` trait, Windows GDI `GetICMProfileA`, `Lumen::target_color_space()`).
4:    - Step 2: done — commit `ff18c93a` (`IccProfile::build_rgb_transform_to(target)` DisplayP3/Rec2020 matrices, `RgbTransform.encode`, `ColorFloat::to_display(target)`).
5:    - Step 3: done — commit `72a7dfaa` (carry `ColorSpace` in `GradientStop`; `Color` derives `Default`).
6:    - Step 4: done — commit `...` (wgpu renderer selects wide-gamut format `Rgba16Float` for DisplayP3/Rec2020; `Renderer::target_color_space()` getter; `WgpuBackend::target_color_space()`).
7:    - Step 5: done — commit `...` (`set_canvas_background` wired into wgpu backend; `LoadOpChoice::Clear(wgpu::Color)`; sRGB→DisplayP3/Rec2020 gamma+matrix conversion at frame start; `Renderer::wgpu_color_for_canvas_bg`).
8:    - Step 6: done — `decode_to(bytes, target)` added to `lumen-image`; `color_manage_in_place` now target-aware; shell passes `self.target_color_space()` through all decode paths; new per-pixel converters for P3↔Rec2020.
9:    - Step 7: done — `WgpuBackend::is_wide_gamut()`; femtoovg hard-constraint to 8-bit sRGB documented as known limitation.
10:   Steps 3–7 committed; all tests pass (lumen-core 277, lumen-image 152, lumen-paint 813).
