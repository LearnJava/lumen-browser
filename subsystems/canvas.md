# lumen-canvas

HTML Canvas 2D rendering context (`CanvasRenderingContext2D`) — CPU rasterization to an RGBA pixel buffer.

## Scope

Phase 0 implementation: all drawing operations write to an in-process `Vec<u8>` (RGBA8, row-major, top-left origin). The buffer is uploaded to GPU via `Renderer::register_image` and drawn with `DrawImage`.

## Done

- `Context2D::new(width, height)` — transparent black buffer.
- `fillRect / clearRect / strokeRect` — axis-aligned rectangle ops.
- `beginPath / moveTo / lineTo / closePath / arc` — path accumulation.
- `fill() / stroke()` — rasterize current path with `fillStyle` / `strokeStyle`.
- `globalAlpha` — multiplies the alpha channel on all drawing operations.
- Porter-Duff source-over compositing in `set_pixel`.
- `clearRect` uses direct write (copy semantics), not source-over.
- `CanvasColor::from_css_str` — parses `#rrggbb`, `#rgb`, `rgb()`, `rgba()`, 19 named colors.
- Scanline even-odd fill for closed paths.
- Thick-stroke line rasterization (perpendicular quad, scanline fill).
- `arc()` approximated as polyline (up to 180 segments).
- 11 unit tests.

## Deferred (Phase 1+)

- Gradients, patterns, transforms, clip, ImageData.
- `bezierCurveTo`, `quadraticCurveTo`, rounded rects.
- `fillText`, `strokeText`, `measureText`.
- Shadow (`shadowColor`, `shadowBlur`).
- `getImageData / putImageData`.
- Shell integration (`<canvas>` element → `Context2D` binding via JS).

## Invariants

- Pixels are RGBA8, premultiplied-alpha is **not** used — straight alpha throughout.
- `clearRect` directly zeroes the buffer (does not go through `set_pixel`).
- `arc()` tessellates to at most 180 segments regardless of radius.
