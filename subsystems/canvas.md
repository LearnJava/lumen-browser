# lumen-canvas

HTML Canvas 2D rendering context (`CanvasRenderingContext2D`) — CPU rasterization to an RGBA pixel buffer.

## Scope

Phase 5 implementation: all drawing operations write to an in-process `Vec<u8>` (RGBA8, row-major, top-left origin). The buffer is uploaded to GPU via `Renderer::register_image` and drawn with `DrawImage`.

## Done

### Phase 1 (baseline)
- `Context2D::new(width, height)` — transparent black buffer.
- `fillRect / clearRect / strokeRect` — axis-aligned rectangle ops.
- `beginPath / moveTo / lineTo / closePath / arc` — path accumulation.
- `fill() / stroke()` — rasterize current path with `fillStyle` / `strokeStyle`.
- `globalAlpha` — multiplies the alpha channel on all drawing operations.
- Porter-Duff source-over compositing in `composite_pixel`.
- `clearRect` uses direct write (copy semantics), not source-over.
- `CanvasColor::from_css_str` — colour parsing. Since [BUG-451](../bugs/BUG-451-FIXED.md)
  (2026-08-30) this is a two-line wrapper over `lumen_layout::parse_color`, i.e. the
  cascade's own parser, and **not** a copy: the crate already depended on `lumen-layout`,
  so the duplicate (`#rrggbb`/`#rgb`/`rgb(,,)`/`rgba(,,,)`/19 names, no `hsl()` at all,
  and a slice panic on `'rgb('`) was deleted rather than extended. Anything the cascade
  accepts a canvas now accepts. `None` means "not a colour" — the caller must **keep the
  previous value** (HTML LS §4.12.5.1.3), never fall back to black.
- `CanvasColor::to_css_string` — the §4.12.5.1.3 serialization (`#rrggbb` when opaque,
  `rgba(r, g, b, a)` otherwise). Alpha is stored as a byte but serialized as a 0–1 number,
  so it prints the *shortest decimal that round-trips back to the same byte* (128 → `0.5`,
  not `0.502`); every one of the 256 values is covered by a round-trip test.
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
- `clip()` — rasterizes current path into the 8-bit coverage `clip_mask`; intersects with existing mask.
  - `build_clip_mask(path, w, h)` in `rasterize.rs` — scanline even-odd rasterization.
  - `clip_coverage(x, y)` — multiplied into the source alpha on every pixel write in the rasterizer and the fill methods.
- `draw_image(src_pixels, src_w, src_h, dx, dy, dw, dh)` — scaled blit with CTM + globalAlpha.
- `put_image_data(data, sw, sh, dx, dy)` — direct write bypassing CTM/alpha/clip (spec §4.12.5.1.16).
- `create_image_data(sw, sh) -> Vec<u8>` — zero-filled RGBA8 buffer.
- `fill_text_glyphs(glyphs)` — renders pre-rasterized glyph coverage bitmaps with CTM and globalAlpha.
  - Full `fillText` integration deferred to Phase 4 (requires lumen-font dependency).
- `font` property stored; CSS font string parsing deferred.
- `From<CanvasColor> for PaintSource` — backward-compatible implicit conversion.
- 35 unit tests pass.

### Phase 4 (fillText / strokeText / measureText via lumen-font)
- `fillText / strokeText` — full glyph rasterization via `lumen_font::Rasterizer`.
- `measureText` — real advance widths via `Font::parse` + hmtx table.
- `textAlign / textBaseline` — saved/restored as part of `DrawState`.
- `parse_canvas_font_size` — extracts px size from CSS font string.
- 48 unit tests pass.

### Phase 5 — Path2D (HTML LS §4.12.5.1.5)
- `Path2dData` struct in `path2d.rs` — reusable path object storing segments in user-space coordinates (CTM applied at use-time per spec).
- All CanvasPath mixin methods: `moveTo/lineTo/closePath/bezierCurveTo/quadraticCurveTo/arc/arcTo/ellipse/rect/addPath`.
- `from_svg_str(s)` — parses SVG path data strings: M/m L/l H/h V/v C/c Q/q A/a Z/z with relative→absolute conversion.
- `to_device_space(ctm) -> Vec<PathSegment>` — applies CTM at use-time (spec-compliant).
- `svg_arc_to_lines` — endpoint→centre parameterisation (SVG 1.1 Appendix F.6).
- `Context2D::fill_with_path2d / stroke_with_path2d / clip_with_path2d / is_point_in_path2d`.
- 48 unit tests pass (canvas crate).

### JS bindings (lumen-js `canvas2d.rs` + `dom.rs`)
- All Phase 1–5 canvas ops exposed as `_lumen_canvas2d_*` native functions.
- Phase 3: gradients (linear/radial/conic), patterns, shadow, clip, draw_image, put/createImageData.
- Phase 4: fillText, strokeText, measureText, textAlign, textBaseline, font.
- Phase 5: `_lumen_canvas2d_path2d_*` native functions; PATHS/NEXT_PATH_ID thread-locals.
- Thread-local registries: GRADIENTS, PATTERNS, NEXT_PAINT_ID, PATHS, NEXT_PATH_ID.
- JS `Path2D` class in `dom.rs`: constructor (from svg string or Path2D copy), full prototype.
- `ctx.fill(ruleOrPath)`, `ctx.stroke(path?)`, `ctx.clip(path?)`, `ctx.isPointInPath(path, x, y)`.
- `ellipse` implemented in JS shim (rquickjs max-7-args limitation → save/scale/rotate/arc trick).

### Anti-aliased rasterization ([BUG-099](../bugs/BUG-099-OPEN.md), 2026-07-29)

`rasterize.rs` was a binary-coverage scanline filler: a pixel was either fully painted
or untouched, so every non-axis-aligned edge came out jagged against Edge's smoothed
one. It now computes fractional coverage and scales the source alpha by it.

- `AA_ROWS = 4` vertical sub-scanlines per pixel row; horizontal coverage is exact
  (a span contributes its fractional overlap to the two end pixels), so only the
  vertical axis is sampled. An interior pixel still accumulates exactly `1.0`
  (`4 × 0.25`, both terms exact in binary), so axis-aligned fills are bit-identical
  to the old output — only edges change.
- `fill_path`, `stroke_path` and `build_clip_mask` all funnel through one
  `Scratch::coverage_row`; `build_clip_mask` returns `Vec<u8>` coverage instead of
  `Vec<bool>`, and `Context2D::pixel_allowed` became `clip_coverage(x, y) -> f32`.
- A stroke is rasterized as the **union** of its per-segment quads (spans are merged
  on each sub-scanline) instead of one `fill_quad` per segment. Painting them
  separately composited the join overlap twice — visible as a dark corner under
  `globalAlpha < 1`, and as a seam once the quads gained AA edges.
- `coverage_row` returns the touched column range so the compositing loop walks only
  the covered part of the row, not the full canvas width.

### Line caps and joins ([BUG-099](../bugs/BUG-099-OPEN.md), 2026-07-29)

`lineCap` / `lineJoin` / `miterLimit` were parsed and preserved across
`save()`/`restore()` but never read by `rasterize.rs`, so every stroke was a union of
per-segment quads with butt ends and a `strokeRect` corner kept a notch. `stroke_path`
now reads the three off the context and emits join and cap shapes into the same union.

- `flatten_subpaths` rebuilds the sub-path structure `collect_lines` throws away: a
  sub-path breaks at a `Move` or wherever a segment does not start where the previous
  one ended, and it is *closed* when its last vertex repeats its first. Caps therefore
  land only on genuine open ends — a `closePath`d rectangle gets a join at its seam.
- Joins sit at every interior vertex, including the ones a Bézier/arc tessellation
  invents. Two cutoffs keep that affordable: a wedge under `MIN_JOIN_AREA` (0.05 px²,
  computed as `half² · sin θ / 2`) is skipped outright, and a round/miter bulge under
  `ROUND_FLATNESS` (0.05 px) degrades to the bevel triangle. A thin arc therefore
  costs nothing extra; a thick one still gets its outer edge closed.
- A miter is the kite `(vertex, outer₀, tip, outer₁)` with `tip` at
  `half / cos(θ/2)`; over `miterLimit` it falls back to the bevel triangle. A round
  join or cap is a disc of 8–64 edges scaled by radius — a full disc, not a half one,
  since the inner half is already inside the segment quads and the union absorbs it.
- The existing `line_cap_parse` / `line_join_parse` tests cover `from_str` only and
  stayed green through all of this; the 8 new tests in `rasterize.rs` probe a single
  pixel that miter fills solid, round covers partially and bevel leaves empty.

## Deferred

- Gaussian blur for `shadowBlur > 0`.
- Canvas fingerprint noise (ADR-007) — `set_noise_generator / get_image_data`.
## Invariants

- Pixels are RGBA8, straight alpha throughout (no premultiplied alpha).
- `clearRect` directly zeroes the buffer (does not go through `composite_pixel`).
- `arc()` tessellates to at most 180 segments regardless of radius.
- Gradient sampling is in device pixel space (post-CTM), not spec-correct user space.
- `put_image_data` bypasses CTM, globalAlpha, compositing, and clip (spec §4.12.5.1.16).
- `clip()` intersects with the existing mask (never replaces it outright); coverage is combined with `min`, not a product, so nesting `clip()` calls on the same boundary does not darken it.
- Fill, stroke and clip share one coverage rasterizer — a geometry change must go into `Scratch::coverage_row`, not into a per-operation copy of the scanline loop.
- `Path2dData` stores user-space coordinates; CTM is applied in `to_device_space()` at draw time, not at path-construction time (HTML LS §4.12.5.1.5 invariant).
- `ellipse()` on `Path2dData` is approximated via `arc` with save/scale/rotate (correct for all standard use cases).
