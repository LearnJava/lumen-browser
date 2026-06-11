# lumen-canvas

HTML Canvas 2D rendering context (`CanvasRenderingContext2D`) — CPU rasterization to an RGBA pixel buffer.

## Scope

Phase 3 implementation: all drawing operations write to an in-process `Vec<u8>` (RGBA8, row-major, top-left origin). The buffer is uploaded to GPU via `Renderer::register_image` and drawn with `DrawImage`.

## Done

### Phase 1 (baseline)
- `Context2D::new(width, height)` — transparent black buffer.
- `fillRect / clearRect / strokeRect` — axis-aligned rectangle ops.
- `beginPath / moveTo / lineTo / closePath / arc` — path accumulation.
- `fill() / stroke()` — rasterize current path with `fillStyle` / `strokeStyle`.
- `globalAlpha` — multiplies the alpha channel on all drawing operations.
- Porter-Duff source-over compositing in `composite_pixel`.
- `clearRect` uses direct write (copy semantics), not source-over.
- `CanvasColor::from_css_str` — parses `#rrggbb`, `#rgb`, `rgb()`, `rgba()`, 19 named colors.
- Scanline even-odd fill for closed paths.
- Thick-stroke line rasterization (perpendicular quad, scanline fill).
- `arc()` approximated as polyline (up to 180 segments).

### Phase 2 (state stack + CTM + Bézier + composite)
- `save() / restore()` — full drawing state stack (CTM, styles, compositing, clip, font).
- `translate / rotate / scale / transform / setTransform / resetTransform` — current transformation matrix.
- `bezierCurveTo / quadraticCurveTo / arcTo / ellipse / rect` — extended path operations.
- `globalCompositeOperation` — 16 Porter-Duff + blend modes.
- `lineCap / lineJoin / miterLimit` — stroke style properties.
- `resize(w, h)` — resets buffer and CTM.
- `from_pixels(w, h, pixels)` — constructor from existing buffer.
- Cubic and quadratic Bézier tessellation (32 segments each).
- 35 unit tests.

### Phase 3 (gradients + patterns + shadow + clip + imageData + font stubs)
- `PaintSource` enum replacing `CanvasColor` for `fillStyle` / `strokeStyle`:
  - `PaintSource::Color(CanvasColor)` — solid colour.
  - `PaintSource::Gradient(CanvasGradient)` — linear / radial / conic gradient with colour stops.
  - `PaintSource::Pattern(CanvasPattern)` — tiled image pattern with repeat modes.
- `CanvasGradient` — `createLinearGradient`, `createRadialGradient`, `createConicGradient`.
  - `add_color_stop(offset, color)` — sorted by offset.
  - `sample(x, y)` — device-space pixel sampling via `atan2_approx` (deterministic, no libm).
- `CanvasPattern` — `createPattern(pixels, w, h, RepeatMode)`.
  - Repeat modes: `Repeat`, `RepeatX`, `RepeatY`, `NoRepeat`.
- Shadow rendering: `shadowColor / shadowBlur / shadowOffsetX / shadowOffsetY`.
  - Phase 3: offset-only (no Gaussian blur); blur value stored but not yet applied.
  - `shadow_effective()` skips zero-alpha or zero-offset shadows.
  - `shift_path()` — shifts all path coordinates by (dx, dy) for shadow pass.
- `clip()` — rasterizes current path into boolean `clip_mask`; intersects with existing mask.
  - `build_clip_mask(path, w, h)` in `rasterize.rs` — scanline even-odd rasterization.
  - `pixel_allowed(x, y)` — checked before every pixel write in rasterizer and fill methods.
- `draw_image(src_pixels, src_w, src_h, dx, dy, dw, dh)` — scaled blit with CTM + globalAlpha.
- `put_image_data(data, sw, sh, dx, dy)` — direct write bypassing CTM/alpha/clip (spec §4.12.5.1.16).
- `create_image_data(sw, sh) -> Vec<u8>` — zero-filled RGBA8 buffer.
- `fill_text_glyphs(glyphs)` — renders pre-rasterized glyph coverage bitmaps with CTM and globalAlpha.
  - Full `fillText` integration deferred to Phase 4 (requires lumen-font dependency).
- `font` property stored; CSS font string parsing deferred.
- `From<CanvasColor> for PaintSource` — backward-compatible implicit conversion.
- 35 unit tests pass.

### JS bindings (lumen-js `canvas2d.rs`)
- All Phase 1–3 canvas ops exposed as `_lumen_canvas2d_*` native functions.
- Phase 3 additions: `create_linear/radial/conic_gradient`, `gradient_add_color_stop`,
  `set_fill/stroke_style_gradient`, `create_pattern`, `set_fill/stroke_style_pattern`,
  `set_shadow_color/blur/offset_x/offset_y`, `clip`, `draw_image`, `put_image_data`,
  `create_image_data`, `set_font`, `fill_text` (stub), `measure_text` (rough estimate).
- Thread-local gradient/pattern registries (GRADIENTS, PATTERNS, NEXT_PAINT_ID).
- `offscreen_canvas.rs` updated to use `PaintSource::Color(...)`.
- `decode_hex` helper for `put_image_data` hex-encoded data.

## Deferred (Phase 4+)

- `fillText / strokeText` — full glyph rasterization (needs lumen-font integration in lumen-js).
- `measureText` — spec-correct measurement based on font metrics.
- Gaussian blur for `shadowBlur > 0`.
- `getContext('2d')` shell integration (display-list `DrawImage` upload).
- Canvas fingerprint noise (ADR-007) — `set_noise_generator / get_image_data`.

## Invariants

- Pixels are RGBA8, straight alpha throughout (no premultiplied alpha).
- `clearRect` directly zeroes the buffer (does not go through `composite_pixel`).
- `arc()` tessellates to at most 180 segments regardless of radius.
- Gradient sampling is in device pixel space (post-CTM), not spec-correct user space.
- `put_image_data` bypasses CTM, globalAlpha, compositing, and clip (spec §4.12.5.1.16).
- `clip()` intersects with the existing mask (never replaces it outright).
