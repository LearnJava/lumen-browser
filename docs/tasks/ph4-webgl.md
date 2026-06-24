# Ph4 — WebGL subset (GLSL execution)

**Developer:** P2 · **Branch:** `p2-ph4-webgl` · **Size:** XL · **Crates:** `lumen-paint`, `lumen-js`

> Roadmap item: **Phase 4 (after 1.0)** — "Подмножество WebGL (по запросам)"
> ([docs/plan/phases.md:142](../plan/phases.md), §7F in [docs/plan/web-apis-shell.md](../plan/web-apis-shell.md)).

---

## Status

**Phase 4 (after-1.0) — partial.** A functional software WebGL 1.0 context already
exists *and a GLSL ES 1.0 interpreter is already wired*. This task closes the
remaining gaps, not "add a shader interpreter from scratch".

> ⚠️ **The roadmap line in `phases.md:142` is STALE.** It says *"GLSL не
> исполняется — плоская заливка цветом из uniform4f"*. That described the
> original task #28 state. Task #34 ([crates/engine/paint/src/glsl.rs](../../crates/engine/paint/src/glsl.rs))
> later added a real lexer + parser + AST + tree-walking interpreter for vertex
> and fragment shaders, with varying interpolation. **Verify against code, not
> against the roadmap text.** Updating that `phases.md` line to reflect reality
> is part of this task's DoD.

So: real GLSL shaders already run. What is missing is (a) a way to make the
rendered framebuffer *visible on the page*, (b) the rest of the GL state machine
(blending/depth/cull/scissor/`drawElements`/real textures), and (c) interpreter
fidelity (user-defined functions, integer/precision semantics, more built-ins).

---

## Goal

Turn the existing software WebGL context from "runs shaders into an off-screen
buffer you can only `readPixels`" into a usable WebGL 1.0 subset:

1. **Composite** the `SoftwareWebGl` drawing buffer onto the page so a `<canvas>`
   with a WebGL context actually displays (today only `readPixels` reads it back).
2. **Complete the GL state machine**: blending (`gl.enable(BLEND)` + `blendFunc`),
   depth test + depth buffer, `gl.clear(DEPTH_BUFFER_BIT)`, face culling, scissor,
   `drawElements` (indexed `ELEMENT_ARRAY_BUFFER`), real (non-1×1) textures with
   filtering/wrap, and the remaining `uniform*` setters.
3. **Raise interpreter fidelity** so real-world demo shaders run: user-defined
   functions (not only `main()`), correct `int` vs `float` rules, perspective-correct
   varyings, missing built-ins, `mat3` as a first-class type.

Out of scope: WebGL 2 (the JS shim aliases `webgl2` → the same v1 context; leave
it aliased), real GPU acceleration, MSAA/anti-aliasing.

---

## Current state

### What works today (verified against code)

**JS bindings** — [crates/js/src/webgl_canvas.rs](../../crates/js/src/webgl_canvas.rs):
`install_webgl_canvas()` ([webgl_canvas.rs:57](../../crates/js/src/webgl_canvas.rs))
registers `_lumen_webgl_*` natives and a JS shim (`WEBGL_SHIM`,
[webgl_canvas.rs:220](../../crates/js/src/webgl_canvas.rs)) that intercepts
`document.createElement('canvas')` and gives `getContext('webgl' | 'webgl2' |
'experimental-webgl')` a functional context object. Contexts live in a
`thread_local` registry keyed by id ([webgl_canvas.rs:35](../../crates/js/src/webgl_canvas.rs)).
Fingerprint normalization (ADR-007 L4) is preserved: `getParameter(VENDOR/RENDERER)`
returns normalized strings, `toDataURL`/`toBlob` stay blank
([webgl_canvas.rs:390](../../crates/js/src/webgl_canvas.rs), [:480](../../crates/js/src/webgl_canvas.rs)).

**Software rasterizer** — [crates/engine/paint/src/webgl.rs](../../crates/engine/paint/src/webgl.rs),
`SoftwareWebGl` ([webgl.rs:114](../../crates/engine/paint/src/webgl.rs)). Implemented:
- Framebuffer RGBA8 top-left origin; `clearColor`/`clear(COLOR_BUFFER_BIT)`
  ([webgl.rs:238](../../crates/engine/paint/src/webgl.rs)); `viewport`.
- Buffers: `createBuffer`/`bindBuffer`/`bufferData` (f32 only)
  ([webgl.rs:255](../../crates/engine/paint/src/webgl.rs)–[:277](../../crates/engine/paint/src/webgl.rs)).
- Shaders/programs: `createShader`/`shaderSource`/`compileShader` (parses GLSL),
  `createProgram`/`attachShader`/`linkProgram`/`useProgram`
  ([webgl.rs:280](../../crates/engine/paint/src/webgl.rs)–[:354](../../crates/engine/paint/src/webgl.rs)).
- Attributes: `getAttribLocation`, `enable/disableVertexAttribArray`,
  `vertexAttribPointer` (FLOAT type assumed, stride/offset in bytes→floats)
  ([webgl.rs:407](../../crates/engine/paint/src/webgl.rs)).
- Uniforms: `uniform1f/2f/3f/4f`, `uniform1i` (sampler binding), `uniformMatrix4fv`
  ([webgl.rs:421](../../crates/engine/paint/src/webgl.rs)–[:465](../../crates/engine/paint/src/webgl.rs)).
- Textures (degenerate): `activeTexture`/`bindTexture`/`texImage2D` — **every
  texture is averaged to a single 1×1 RGBA colour** ([webgl.rs:479](../../crates/engine/paint/src/webgl.rs)).
- `drawArrays` ([webgl.rs:498](../../crates/engine/paint/src/webgl.rs)) for
  POINTS/LINES/LINE_STRIP/TRIANGLES/TRIANGLE_STRIP/TRIANGLE_FAN.
- `readPixels` (full framebuffer readback; JS wrapper flips to bottom-left and
  crops, [webgl_canvas.rs:364](../../crates/js/src/webgl_canvas.rs)).

**GLSL interpreter** — [crates/engine/paint/src/glsl.rs](../../crates/engine/paint/src/glsl.rs)
(task #34). This is the part the roadmap text wrongly says is missing:
- Lexer ([glsl.rs:138](../../crates/engine/paint/src/glsl.rs)), recursive-descent
  parser ([glsl.rs:403](../../crates/engine/paint/src/glsl.rs)), AST
  (`Expr`/`Stmt`, [glsl.rs:342](../../crates/engine/paint/src/glsl.rs)).
- Tree-walking evaluator: `exec_main` ([glsl.rs:977](../../crates/engine/paint/src/glsl.rs)),
  `eval_expr` ([glsl.rs:1091](../../crates/engine/paint/src/glsl.rs)).
- Types `float`/`int`/`bool`/`vec2..4`/`mat4`/`sampler2D` (`Val`,
  [glsl.rs:32](../../crates/engine/paint/src/glsl.rs)); swizzles read+write;
  `if`/`for`/`while`/`return`/`discard`; matrix×vector / matrix×matrix
  ([glsl.rs:1244](../../crates/engine/paint/src/glsl.rs)).
- ~40 built-ins (`mix`/`clamp`/`smoothstep`/`length`/`normalize`/`dot`/`cross`/
  `pow`/trig/`texture2D`/…, [glsl.rs:1315](../../crates/engine/paint/src/glsl.rs)).
- Per-vertex shader execution + barycentric varying interpolation in the
  rasterizer: `draw_arrays_shaded` ([webgl.rs:563](../../crates/engine/paint/src/webgl.rs)),
  `fill_triangle_shaded` ([webgl.rs:720](../../crates/engine/paint/src/webgl.rs)),
  `interp_varyings` ([glsl.rs:1546](../../crates/engine/paint/src/glsl.rs)).

### What does NOT work — the real gaps

1. **Framebuffer is never displayed.** The drawing buffer is only reachable via
   `readPixels`. There is no compositing of `SoftwareWebGl::pixels()`
   ([webgl.rs:207](../../crates/engine/paint/src/webgl.rs)) into the page's canvas
   box / display list, so a WebGL page renders nothing visible. (Contrast: Canvas
   2D *does* paint into the page.) **This is the headline gap for "usable WebGL".**
2. **GLSL fidelity holes** (see [glsl.rs:23](../../crates/engine/paint/src/glsl.rs)
   "Unsupported"): user-defined functions are dropped — only `main()` runs
   ([glsl.rs:514](../../crates/engine/paint/src/glsl.rs), `skip_block`); arrays
   beyond mat columns are skipped ([glsl.rs:826](../../crates/engine/paint/src/glsl.rs));
   `mat3` is widened to `mat4`; `int`/`float` mixing is loose; varyings are
   interpolated affinely (no perspective-correct divide by `w`).
3. **No real textures.** `tex_image_2d_rgba` collapses any image to one averaged
   colour ([webgl.rs:479](../../crates/engine/paint/src/webgl.rs)); `texture2D`
   ignores UVs ([glsl.rs:1358](../../crates/engine/paint/src/glsl.rs)); no
   filtering/wrap/mipmap (`texParameteri`/`generateMipmap` are JS no-ops,
   [webgl_canvas.rs:429](../../crates/js/src/webgl_canvas.rs)).
4. **GL state machine is mostly no-op.** `enable`/`disable`/`blendFunc`/
   `depthFunc`/`scissor`/`pixelStorei` are JS no-ops
   ([webgl_canvas.rs:260](../../crates/js/src/webgl_canvas.rs)–[:266](../../crates/js/src/webgl_canvas.rs)):
   no blending modes (the rasterizer is hard-wired source-over in `blend_pixel`,
   [webgl.rs:881](../../crates/engine/paint/src/webgl.rs)), **no depth buffer / depth
   test** (`clear(DEPTH_BUFFER_BIT)` is a no-op [webgl.rs:56](../../crates/engine/paint/src/webgl.rs)),
   no face culling, no scissor rect.
5. **No `drawElements`.** `gl.drawElements` is a JS no-op
   ([webgl_canvas.rs:361](../../crates/js/src/webgl_canvas.rs));
   `ELEMENT_ARRAY_BUFFER` is accepted but unused ([webgl.rs:265](../../crates/engine/paint/src/webgl.rs)).
6. **Missing uniform setters / state queries.** No `uniform2i/3i/4i`,
   `uniformMatrix2fv/3fv` (mat3 dropped, [webgl_canvas.rs:357](../../crates/js/src/webgl_canvas.rs)),
   `vertexAttrib*f` constant attributes; `getActiveUniform`/`getActiveAttrib`
   absent.

---

## Architecture

### Pipeline (proposed end state)

```
canvas.getContext('webgl')   →  SoftwareWebGl (per-thread registry)        [exists]
  shaderSource/compileShader →  glsl::parse → ParsedShader (AST)           [exists]
  drawArrays / drawElements  →  vertex stage: exec vertex main per vertex  [exists / extend]
                                  → clip-space gl_Position + varyings
                                rasterize primitive (bary)                  [exists]
                                  → per-fragment: depth test + run frag main + blend
                                                                            [extend: depth+blend]
  framebuffer (RGBA8)        →  COMPOSITE into page display list           [PROPOSED — missing]
  readPixels                 →  framebuffer readback                       [exists]
```

### Layers to touch

- **`lumen-paint`** — the engine of this task:
  - `glsl.rs`: interpreter fidelity (user functions, mat3, int rules, perspective
    varyings, more built-ins, real texture sampling signature).
  - `webgl.rs`: GL state (blend equation/func, depth buffer + test, cull, scissor),
    `draw_elements`, 2D texture store with filtering/wrap, per-fragment depth+blend
    in `fill_triangle_shaded`/`blend_pixel`.
- **`lumen-js`** — `webgl_canvas.rs`: un-stub `enable`/`disable`/`blendFunc`/
  `depthFunc`/`scissor`/`drawElements`/`texParameteri`, add missing `uniform*`/
  `vertexAttrib*` natives, forward index buffers, forward full texel data.
- **Compositing (cross-layer, coordinate with P1/P3):** make the WebGL framebuffer
  reach the page. *Proposed:* expose the canvas's `SoftwareWebGl::pixels()` to the
  same canvas-element → display-list path Canvas 2D already uses (grep the Canvas 2D
  bitmap-to-display-list bridge in [crates/js/src/canvas2d.rs](../../crates/js/src/canvas2d.rs)
  and the paint display list [crates/engine/paint/src/display_list.rs](../../crates/engine/paint/src/display_list.rs)).
  `lumen-shell` integration is **P3's** — describe the hook in the commit body and
  file it as a P3 follow-up if a shell change is needed.

### GLSL execution model (already chosen — keep it)

Tree-walking interpreter over the AST, one `ShaderEnv`
([glsl.rs:920](../../crates/engine/paint/src/glsl.rs)) per shader invocation. Do
**not** rewrite to SSA/SIMD as part of this task — correctness and coverage first;
an optimization pass (cache parse, batch fragment evaluation, optional SIMD over a
pixel quad) is the last step and only if a perf test demands it.

---

## Entry points (real file:line; PROPOSED marked)

| Concern | Location |
|---|---|
| JS context install | [crates/js/src/webgl_canvas.rs:57](../../crates/js/src/webgl_canvas.rs) `install_webgl_canvas` |
| JS shim (context object) | [crates/js/src/webgl_canvas.rs:220](../../crates/js/src/webgl_canvas.rs) `WEBGL_SHIM` |
| Rasterizer struct | [crates/engine/paint/src/webgl.rs:114](../../crates/engine/paint/src/webgl.rs) `SoftwareWebGl` |
| `drawArrays` | [crates/engine/paint/src/webgl.rs:498](../../crates/engine/paint/src/webgl.rs) |
| Shaded draw path | [crates/engine/paint/src/webgl.rs:563](../../crates/engine/paint/src/webgl.rs) `draw_arrays_shaded` |
| Per-fragment blend | [crates/engine/paint/src/webgl.rs:881](../../crates/engine/paint/src/webgl.rs) `blend_pixel` |
| 1×1 texture store | [crates/engine/paint/src/webgl.rs:479](../../crates/engine/paint/src/webgl.rs) `tex_image_2d_rgba` |
| GLSL parser | [crates/engine/paint/src/glsl.rs:403](../../crates/engine/paint/src/glsl.rs) `Parser` |
| GLSL evaluator | [crates/engine/paint/src/glsl.rs:1091](../../crates/engine/paint/src/glsl.rs) `eval_expr` |
| GLSL built-ins | [crates/engine/paint/src/glsl.rs:1315](../../crates/engine/paint/src/glsl.rs) `eval_call` |
| Function-body skip (only `main`) | [crates/engine/paint/src/glsl.rs:514](../../crates/engine/paint/src/glsl.rs) |
| **PROPOSED** `draw_elements` | `crates/engine/paint/src/webgl.rs` (new `pub fn`) |
| **PROPOSED** depth buffer + `enable(DEPTH_TEST)`/`depthFunc` | `crates/engine/paint/src/webgl.rs` (new fields + per-fragment test) |
| **PROPOSED** blend state (`enable(BLEND)`/`blendFunc`) | `crates/engine/paint/src/webgl.rs` (new fields, use in `blend_pixel`) |
| **PROPOSED** 2D texture store + sampler | `crates/engine/paint/src/webgl.rs` (replace 1×1) + `glsl.rs` `texture2D` UV sampling |
| **PROPOSED** user-defined GLSL functions | `crates/engine/paint/src/glsl.rs` (`ParsedShader.functions`, call dispatch in `eval_call`) |
| **PROPOSED** framebuffer→page compositing bridge | `crates/js/src/webgl_canvas.rs` + canvas→display-list path (see `canvas2d.rs`) |

---

## Steps

Order: interpreter fidelity → state machine → textures → compositing → optimize.
Each step is its own commit (`cargo clippy -p <crate> -- -D warnings` + crate tests
before committing).

1. **GLSL fidelity — user-defined functions.** Stop dropping non-`main` bodies
   ([glsl.rs:514](../../crates/engine/paint/src/glsl.rs)): parse them into
   `ParsedShader.functions: HashMap<String, FnDef>` (params + body), dispatch from
   `eval_call` ([glsl.rs:1315](../../crates/engine/paint/src/glsl.rs)) with a fresh
   local scope before falling through to built-ins. Unit tests in `glsl.rs`.
2. **GLSL fidelity — types & numerics.** First-class `mat3` (stop widening to
   mat4); correct `int`÷`int`, `mod`, comparison-on-int; make swizzle-on-scalar and
   constructor truncation match GLSL. Add missing common built-ins demos need
   (`faceforward`, `refract`, `matrixCompMult`, vector `min/max/clamp/mod` with
   vector second arg — current `map2` broadcasts scalar only, [glsl.rs:1527](../../crates/engine/paint/src/glsl.rs)).
3. **State machine — blending.** Add blend-enable + `blendFunc(src,dst)` factors
   to `SoftwareWebGl`; honour them in `blend_pixel`
   ([webgl.rs:881](../../crates/engine/paint/src/webgl.rs)) (default keeps current
   source-over). Un-stub JS `enable`/`disable`/`blendFunc`
   ([webgl_canvas.rs:260](../../crates/js/src/webgl_canvas.rs)).
4. **State machine — depth.** Add an f32 depth buffer + `clear(DEPTH_BUFFER_BIT)`
   ([webgl.rs:56](../../crates/engine/paint/src/webgl.rs)) + `enable(DEPTH_TEST)` +
   `depthFunc`; interpolate `gl_Position.z/w` per fragment and test/write in
   `fill_triangle_shaded` ([webgl.rs:720](../../crates/engine/paint/src/webgl.rs)).
5. **State machine — cull + scissor.** `cullFace`/`frontFace` winding test before
   raster; `enable(SCISSOR_TEST)`+`scissor(x,y,w,h)` clip rect in `blend_pixel`.
6. **`drawElements`.** Track `ELEMENT_ARRAY_BUFFER` index data (extend
   `bufferData` to accept int indices / a second store), add
   `SoftwareWebGl::draw_elements(mode, count, type, offset)`, wire JS
   `gl.drawElements` ([webgl_canvas.rs:361](../../crates/js/src/webgl_canvas.rs)).
7. **Real textures.** Replace the 1×1 average ([webgl.rs:479](../../crates/engine/paint/src/webgl.rs))
   with a stored RGBA mip-0 image per texture; implement `texture2D(sampler, uv)`
   with NEAREST/LINEAR + REPEAT/CLAMP from `texParameteri`
   ([webgl_canvas.rs:429](../../crates/js/src/webgl_canvas.rs)); perspective-correct
   varyings (divide interpolants by interpolated `1/w`).
8. **Compositing — make it visible.** Bridge `SoftwareWebGl::pixels()` to the page
   so the canvas displays (mirror the Canvas 2D bitmap→display-list path; see
   [crates/js/src/canvas2d.rs](../../crates/js/src/canvas2d.rs) and
   [display_list.rs](../../crates/engine/paint/src/display_list.rs)). If a
   `lumen-shell` change is required, file it for **P3** and describe the hook in the
   commit body (shell is P3-owned).
9. **Optimize (only if a perf test demands).** Cache parsed shaders (already
   `Arc`'d per shader, [webgl.rs:73](../../crates/engine/paint/src/webgl.rs)); avoid
   per-fragment `HashMap` clones in `run_fragment_at`
   ([webgl.rs:787](../../crates/engine/paint/src/webgl.rs)); consider 2×2 pixel-quad
   batching. Measure with `lumen-bench` before/after; do not micro-optimize blind.

---

## Tests

- **`glsl.rs` unit tests** (extend the existing `#[cfg(test)]` block,
  [glsl.rs:1568](../../crates/engine/paint/src/glsl.rs)): user-defined function call
  returns correct value; `mat3` transform; vector `min/max/mod`; new built-ins.
- **`webgl.rs` unit tests** (extend [webgl.rs:922](../../crates/engine/paint/src/webgl.rs)):
  alpha blending with non-default `blendFunc`; depth test hides a farther triangle;
  `drawElements` paints the same pixels as the equivalent `drawArrays`; textured
  triangle samples the correct texel by UV (not the 1×1 average); scissor clips.
- **`webgl_canvas.rs` integration tests** (extend [webgl_canvas.rs:497](../../crates/js/src/webgl_canvas.rs)):
  a full JS program using `drawElements` + a texture + blending reads back the
  expected pixel via `readPixels`.
- **Visual / compositing:** once step 8 lands, add a `graphic_tests/` page (magenta
  frame pattern, per CLAUDE.md) rendering a simple shaded WebGL triangle, plus a
  `--screenshot` CPU-snapshot check. If it can't hit 0.5% vs Edge due to deferred
  fidelity, file a `BUG-NNN` and add a `KNOWN_DEBTORS` entry — **do not** raise the
  threshold.

---

## Definition of done

- [ ] User-defined GLSL functions execute (not only `main`); `mat3` is first-class;
      vector-arg built-ins work — unit tests in `glsl.rs`.
- [ ] Blending (`enable(BLEND)`+`blendFunc`), depth buffer + test
      (`clear(DEPTH_BUFFER_BIT)`, `enable(DEPTH_TEST)`, `depthFunc`), face culling,
      and scissor are honoured by the rasterizer — unit tests in `webgl.rs`.
- [ ] `drawElements` (indexed) renders; wired end-to-end through the JS shim.
- [ ] Real 2D textures (stored image, NEAREST/LINEAR, REPEAT/CLAMP) with UV
      sampling; perspective-correct varyings.
- [ ] WebGL framebuffer is **composited onto the page** (canvas displays), or the
      cross-layer hook is implemented and the `lumen-shell` piece filed for P3 with
      a precise integration note.
- [ ] `cargo clippy -p lumen-paint --all-targets -- -D warnings` and
      `cargo clippy -p lumen-js --all-targets -- -D warnings` clean;
      `cargo test -p lumen-paint` and `cargo test -p lumen-js` green.
- [ ] Docs synced in the same commits: flip the WebGL row in
      [CAPABILITIES.md](../../CAPABILITIES.md); **correct the stale
      [phases.md:142](../plan/phases.md) line** (GLSL *does* execute — describe the
      remaining subset); update §7F in
      [docs/plan/web-apis-shell.md](../plan/web-apis-shell.md); regenerate
      `SYMBOLS.md` for new public APIs; delete the task's pointer line in
      [STATUS-P2.md](../../STATUS-P2.md).
