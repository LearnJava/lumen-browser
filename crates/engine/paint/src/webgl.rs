//! Software WebGL 1.0 backend — the CPU "GPU pipeline" behind
//! `canvas.getContext('webgl')` (task #28, §7F).
//!
//! Lumen's render path is deterministic CPU rasterization (see `cpu_raster`),
//! so WebGL is backed by a software rasterizer rather than a live GPU context.
//! [`SoftwareWebGl`] is a pure-Rust WebGL 1.0 state machine: it owns the
//! framebuffer, vertex buffers, shader/program objects, vertex-attribute
//! pointers, uniform state, and a GLSL ES 1.0 interpreter (task #34).
//!
//! # Fragment colour model
//!
//! Vertex and fragment shaders are parsed and executed by [`crate::glsl`].
//! `drawArrays` runs the vertex shader per vertex (computing `gl_Position` and
//! varyings), then rasterizes primitives by interpolating varyings and running
//! the fragment shader per pixel.
//!
//! Fallback: if no linked program is active (or parsing fails), the rasterizer
//! flat-fills with the most-recent `uniform4f` colour — old behaviour from
//! task #28 — so existing tests continue to pass unchanged.
//!
//! # Coordinate model
//!
//! Attribute 0 is treated as the position attribute and is read as clip-space
//! NDC coordinates in `[-1, 1]`. NDC is mapped through the current viewport to
//! framebuffer pixels with a top-left origin (matching the rest of the paint
//! layer; GL's bottom-left Y is flipped on write).

use std::collections::HashMap;
use std::sync::Arc;

use crate::glsl::{self, exec_main, interp_varyings, Val};

// ── WebGL enum constants (subset actually consumed by the rasterizer) ────────

/// `gl.POINTS` primitive mode.
pub const POINTS: u32 = 0x0000;
/// `gl.LINES` primitive mode.
pub const LINES: u32 = 0x0001;
/// `gl.LINE_STRIP` primitive mode.
pub const LINE_STRIP: u32 = 0x0003;
/// `gl.TRIANGLES` primitive mode.
pub const TRIANGLES: u32 = 0x0004;
/// `gl.TRIANGLE_STRIP` primitive mode.
pub const TRIANGLE_STRIP: u32 = 0x0005;
/// `gl.TRIANGLE_FAN` primitive mode.
pub const TRIANGLE_FAN: u32 = 0x0006;

/// `gl.ARRAY_BUFFER` bind target.
pub const ARRAY_BUFFER: u32 = 0x8892;
/// `gl.ELEMENT_ARRAY_BUFFER` bind target.
pub const ELEMENT_ARRAY_BUFFER: u32 = 0x8893;

/// `gl.COLOR_BUFFER_BIT` clear mask.
pub const COLOR_BUFFER_BIT: u32 = 0x4000;
/// `gl.DEPTH_BUFFER_BIT` clear mask (no-op: software path has no depth buffer).
pub const DEPTH_BUFFER_BIT: u32 = 0x0100;

/// `gl.FRAGMENT_SHADER` shader kind.
pub const FRAGMENT_SHADER: u32 = 0x8B30;
/// `gl.VERTEX_SHADER` shader kind.
pub const VERTEX_SHADER: u32 = 0x8B31;

/// A compiled shader object (source is retained for `getShaderSource`).
#[derive(Debug, Clone)]
struct Shader {
    /// `VERTEX_SHADER` or `FRAGMENT_SHADER`.
    kind: u32,
    /// GLSL source set via `shaderSource`.
    source: String,
    /// Whether `compileShader` was called.
    compiled: bool,
    /// Parsed AST, produced by `compile_shader` via the GLSL interpreter.
    parsed: Option<Arc<glsl::ParsedShader>>,
}

/// A linked program object: a vertex + fragment shader pair.
#[derive(Debug, Clone, Default)]
struct Program {
    /// Attached vertex shader id, if any.
    vertex: Option<u32>,
    /// Attached fragment shader id, if any.
    fragment: Option<u32>,
    /// Whether `linkProgram` was called.
    linked: bool,
    /// Attribute name → location, assigned on `getAttribLocation`.
    attribs: HashMap<String, i32>,
    /// Uniform name → location, assigned on `getUniformLocation`.
    uniforms: HashMap<String, i32>,
    /// Next location to hand out for this program's attribs/uniforms.
    next_location: i32,
}

/// One vertex attribute pointer, as configured by `vertexAttribPointer`.
#[derive(Debug, Clone, Copy, Default)]
struct AttribPointer {
    /// Whether `enableVertexAttribArray` was called for this index.
    enabled: bool,
    /// Buffer id sourced for this attribute (the `ARRAY_BUFFER` binding at the
    /// time `vertexAttribPointer` was called, mirroring WebGL capture rules).
    buffer: u32,
    /// Number of components per vertex (1–4).
    size: usize,
    /// Stride between consecutive vertices, in floats (0 = tightly packed).
    stride_floats: usize,
    /// Offset of the first component, in floats.
    offset_floats: usize,
}

/// Pure-Rust software WebGL 1.0 context.
///
/// One instance backs one `<canvas>` WebGL context. All state is owned here;
/// the JS bindings (`lumen-js::webgl_canvas`) forward WebGL calls 1:1.
#[derive(Debug, Clone)]
pub struct SoftwareWebGl {
    /// Drawing-buffer width in pixels.
    width: u32,
    /// Drawing-buffer height in pixels.
    height: u32,
    /// RGBA8 framebuffer, row-major, top-left origin, `width * height * 4` bytes.
    framebuffer: Vec<u8>,
    /// Clear colour set by `clearColor`, RGBA in `[0, 1]`.
    clear_color: [f32; 4],
    /// Current viewport `(x, y, w, h)` in framebuffer pixels.
    viewport: (i32, i32, i32, i32),
    /// Vertex buffer storage: id → float data uploaded via `bufferData`.
    buffers: HashMap<u32, Vec<f32>>,
    /// Currently bound `ARRAY_BUFFER` id (0 = none).
    bound_array_buffer: u32,
    /// Monotonic id allocator for buffers.
    next_buffer_id: u32,
    /// Shader objects by id.
    shaders: HashMap<u32, Shader>,
    /// Monotonic id allocator for shaders.
    next_shader_id: u32,
    /// Program objects by id.
    programs: HashMap<u32, Program>,
    /// Monotonic id allocator for programs.
    next_program_id: u32,
    /// Currently active program (`useProgram`), 0 = none.
    current_program: u32,
    /// Vertex attribute pointers by attribute index.
    attribs: HashMap<u32, AttribPointer>,
    /// Flat fragment colour (most recent `uniform4f`), RGBA in `[0, 1]`.
    /// Used as the fallback colour when no shader program is active.
    draw_color: [f32; 4],
    /// Uniform values by location, set via `uniform*` calls.
    uniform_vals: HashMap<i32, Val>,
    /// Textures uploaded via `texImage2D`, keyed by a per-context texture id.
    /// Each texture is stored as a 1×1 averaged RGBA value for simple sampling.
    texture_solid: HashMap<u32, [f32; 4]>,
    /// Currently bound texture id per texture unit (unit → texture id).
    bound_textures: HashMap<u32, u32>,
    /// Active texture unit (set by `activeTexture`).
    active_texture_unit: u32,
}

/// Per-vertex output produced by the vertex shader execution.
struct VertexOutput {
    /// Clip-space NDC position `[x, y, z, w]` assigned to `gl_Position`.
    position: [f32; 4],
    /// Varying values written by the vertex shader, keyed by variable name.
    varyings: HashMap<String, Val>,
}

impl SoftwareWebGl {
    /// Create a context with a `width × height` drawing buffer.
    ///
    /// The framebuffer starts fully transparent (`rgba(0,0,0,0)`), matching a
    /// freshly created WebGL drawing buffer before any clear.
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        SoftwareWebGl {
            width: w,
            height: h,
            framebuffer: vec![0u8; (w * h * 4) as usize],
            clear_color: [0.0, 0.0, 0.0, 0.0],
            viewport: (0, 0, w as i32, h as i32),
            buffers: HashMap::new(),
            bound_array_buffer: 0,
            next_buffer_id: 1,
            shaders: HashMap::new(),
            next_shader_id: 1,
            programs: HashMap::new(),
            next_program_id: 1,
            current_program: 0,
            attribs: HashMap::new(),
            draw_color: [1.0, 1.0, 1.0, 1.0],
            uniform_vals: HashMap::new(),
            texture_solid: HashMap::new(),
            bound_textures: HashMap::new(),
            active_texture_unit: 0,
        }
    }

    /// Drawing-buffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Drawing-buffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Borrow the RGBA8 framebuffer (top-left origin, `width*height*4` bytes).
    pub fn pixels(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Read the RGBA pixel at `(x, y)` (top-left origin). Returns
    /// `(0,0,0,0)` for out-of-bounds coordinates.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.framebuffer[i],
            self.framebuffer[i + 1],
            self.framebuffer[i + 2],
            self.framebuffer[i + 3],
        ]
    }

    /// `gl.viewport(x, y, w, h)`.
    pub fn viewport(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.viewport = (x, y, w.max(0), h.max(0));
    }

    /// `gl.clearColor(r, g, b, a)`. Components are clamped to `[0, 1]`.
    pub fn clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [clamp01(r), clamp01(g), clamp01(b), clamp01(a)];
    }

    /// `gl.clear(mask)`. Only `COLOR_BUFFER_BIT` has a visible effect; the
    /// software path has no depth/stencil buffers.
    pub fn clear(&mut self, mask: u32) {
        if mask & COLOR_BUFFER_BIT == 0 {
            return;
        }
        let r = to_u8(self.clear_color[0]);
        let g = to_u8(self.clear_color[1]);
        let b = to_u8(self.clear_color[2]);
        let a = to_u8(self.clear_color[3]);
        for px in self.framebuffer.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = a;
        }
    }

    /// `gl.createBuffer()` → opaque buffer id (never 0).
    pub fn create_buffer(&mut self) -> u32 {
        let id = self.next_buffer_id;
        self.next_buffer_id += 1;
        self.buffers.insert(id, Vec::new());
        id
    }

    /// `gl.bindBuffer(target, buffer)`. `buffer == 0` unbinds. Only
    /// `ARRAY_BUFFER` is tracked; `ELEMENT_ARRAY_BUFFER` is accepted but unused
    /// (indexed `drawElements` is not implemented).
    pub fn bind_buffer(&mut self, target: u32, buffer: u32) {
        if target == ARRAY_BUFFER {
            self.bound_array_buffer = buffer;
        }
    }

    /// `gl.bufferData(target, data, usage)` for float data. Stores `data`
    /// against the currently bound buffer for `target` (only `ARRAY_BUFFER`).
    pub fn buffer_data_f32(&mut self, target: u32, data: Vec<f32>) {
        if target == ARRAY_BUFFER && self.bound_array_buffer != 0 {
            self.buffers.insert(self.bound_array_buffer, data);
        }
    }

    /// `gl.createShader(kind)` → opaque shader id, or 0 for an unknown kind.
    pub fn create_shader(&mut self, kind: u32) -> u32 {
        if kind != VERTEX_SHADER && kind != FRAGMENT_SHADER {
            return 0;
        }
        let id = self.next_shader_id;
        self.next_shader_id += 1;
        self.shaders.insert(
            id,
            Shader { kind, source: String::new(), compiled: false, parsed: None },
        );
        id
    }

    /// `gl.shaderSource(shader, source)`.
    pub fn shader_source(&mut self, shader: u32, source: String) {
        if let Some(s) = self.shaders.get_mut(&shader) {
            s.source = source;
            s.parsed = None; // invalidate cached parse on source change
        }
    }

    /// `gl.compileShader(shader)`. Parses the GLSL source into an AST so
    /// that `drawArrays` can execute vertex and fragment programs.
    pub fn compile_shader(&mut self, shader: u32) {
        if let Some(s) = self.shaders.get_mut(&shader) {
            let parsed = Arc::new(glsl::parse(&s.source));
            s.parsed = Some(parsed);
            s.compiled = true;
        }
    }

    /// `gl.getShaderParameter(shader, COMPILE_STATUS)` — true once compiled.
    pub fn shader_compiled(&self, shader: u32) -> bool {
        self.shaders.get(&shader).map(|s| s.compiled).unwrap_or(false)
    }

    /// `gl.createProgram()` → opaque program id (never 0).
    pub fn create_program(&mut self) -> u32 {
        let id = self.next_program_id;
        self.next_program_id += 1;
        self.programs.insert(id, Program::default());
        id
    }

    /// `gl.attachShader(program, shader)`. Slots the shader by its kind.
    pub fn attach_shader(&mut self, program: u32, shader: u32) {
        let kind = match self.shaders.get(&shader) {
            Some(s) => s.kind,
            None => return,
        };
        if let Some(p) = self.programs.get_mut(&program) {
            match kind {
                VERTEX_SHADER => p.vertex = Some(shader),
                FRAGMENT_SHADER => p.fragment = Some(shader),
                _ => {}
            }
        }
    }

    /// `gl.linkProgram(program)`. Always marks the program linked.
    pub fn link_program(&mut self, program: u32) {
        if let Some(p) = self.programs.get_mut(&program) {
            p.linked = true;
        }
    }

    /// `gl.getProgramParameter(program, LINK_STATUS)` — true once linked.
    pub fn program_linked(&self, program: u32) -> bool {
        self.programs.get(&program).map(|p| p.linked).unwrap_or(false)
    }

    /// `gl.useProgram(program)`. `program == 0` clears the active program.
    pub fn use_program(&mut self, program: u32) {
        self.current_program = program;
    }

    /// `gl.getAttribLocation(program, name)` → stable location (≥ 0), or -1 if
    /// the program is unknown. Locations are assigned lazily and cached.
    pub fn get_attrib_location(&mut self, program: u32, name: &str) -> i32 {
        match self.programs.get_mut(&program) {
            Some(p) => {
                if let Some(loc) = p.attribs.get(name) {
                    return *loc;
                }
                let loc = p.next_location;
                p.next_location += 1;
                p.attribs.insert(name.to_string(), loc);
                loc
            }
            None => -1,
        }
    }

    /// `gl.getUniformLocation(program, name)` → stable location (≥ 0), or -1 if
    /// the program is unknown. Locations are assigned lazily and cached.
    pub fn get_uniform_location(&mut self, program: u32, name: &str) -> i32 {
        match self.programs.get_mut(&program) {
            Some(p) => {
                if let Some(loc) = p.uniforms.get(name) {
                    return *loc;
                }
                let loc = p.next_location;
                p.next_location += 1;
                p.uniforms.insert(name.to_string(), loc);
                loc
            }
            None => -1,
        }
    }

    /// `gl.enableVertexAttribArray(index)`.
    pub fn enable_vertex_attrib_array(&mut self, index: u32) {
        self.attribs.entry(index).or_default().enabled = true;
    }

    /// `gl.disableVertexAttribArray(index)`.
    pub fn disable_vertex_attrib_array(&mut self, index: u32) {
        if let Some(a) = self.attribs.get_mut(&index) {
            a.enabled = false;
        }
    }

    /// `gl.vertexAttribPointer(index, size, type, normalized, stride, offset)`.
    ///
    /// `stride` and `offset` are in **bytes** (WebGL semantics) and assume a
    /// `FLOAT` component type (4 bytes). The current `ARRAY_BUFFER` binding is
    /// captured for this attribute, as in real WebGL.
    pub fn vertex_attrib_pointer(
        &mut self,
        index: u32,
        size: usize,
        stride_bytes: usize,
        offset_bytes: usize,
    ) {
        let entry = self.attribs.entry(index).or_default();
        entry.buffer = self.bound_array_buffer;
        entry.size = size.clamp(1, 4);
        entry.stride_floats = stride_bytes / 4;
        entry.offset_floats = offset_bytes / 4;
    }

    /// `gl.uniform4f(location, x, y, z, w)`.
    pub fn uniform4f(&mut self, location: i32, x: f32, y: f32, z: f32, w: f32) {
        self.draw_color = [clamp01(x), clamp01(y), clamp01(z), clamp01(w)];
        if location >= 0 {
            self.uniform_vals.insert(location, Val::Vec4([x, y, z, w]));
        }
    }

    /// `gl.uniform3f(location, x, y, z)`.
    pub fn uniform3f(&mut self, location: i32, x: f32, y: f32, z: f32) {
        if location >= 0 {
            self.uniform_vals.insert(location, Val::Vec3([x, y, z]));
        }
    }

    /// `gl.uniform2f(location, x, y)`.
    pub fn uniform2f(&mut self, location: i32, x: f32, y: f32) {
        if location >= 0 {
            self.uniform_vals.insert(location, Val::Vec2([x, y]));
        }
    }

    /// `gl.uniform1f(location, x)`.
    pub fn uniform1f(&mut self, location: i32, x: f32) {
        if location >= 0 {
            self.uniform_vals.insert(location, Val::Float(x));
        }
    }

    /// `gl.uniform1i(location, v)`. Used to bind sampler2D to a texture unit.
    pub fn uniform1i(&mut self, location: i32, v: i32) {
        if location >= 0 {
            self.uniform_vals.insert(location, Val::Sampler(v.max(0) as u32));
        }
    }

    /// `gl.uniformMatrix4fv(location, transpose, values)`. Stores a 4×4 float
    /// matrix (column-major, 16 elements).
    pub fn uniform_matrix4fv(&mut self, location: i32, values: &[f32]) {
        if location >= 0 && values.len() >= 16 {
            let mut m = [0f32; 16];
            m.copy_from_slice(&values[..16]);
            self.uniform_vals.insert(location, Val::Mat4(m));
        }
    }

    /// `gl.activeTexture(unit_enum)`. Sets the active texture unit.
    pub fn active_texture(&mut self, unit_enum: u32) {
        self.active_texture_unit = unit_enum.saturating_sub(0x84C0); // GL_TEXTURE0
    }

    /// `gl.bindTexture(target, texture_id)`. Records binding for the active unit.
    pub fn bind_texture(&mut self, _target: u32, texture_id: u32) {
        self.bound_textures.insert(self.active_texture_unit, texture_id);
    }

    /// `gl.texImage2D(…, data)`. Averages pixel data to a 1×1 solid colour for
    /// simple texture sampling in the GLSL interpreter.
    pub fn tex_image_2d_rgba(&mut self, texture_id: u32, width: u32, height: u32, data: &[u8]) {
        if width == 0 || height == 0 || data.len() < (width * height * 4) as usize {
            return;
        }
        let total = (width * height) as usize;
        let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
        for px in data.chunks_exact(4).take(total) {
            r += u32::from(px[0]);
            g += u32::from(px[1]);
            b += u32::from(px[2]);
            a += u32::from(px[3]);
        }
        let n = total as f32 * 255.0;
        self.texture_solid.insert(texture_id, [r as f32/n, g as f32/n, b as f32/n, a as f32/n]);
    }

    /// `gl.drawArrays(mode, first, count)`. Executes vertex and fragment shaders
    /// if a linked program is active; falls back to flat-fill with `draw_color`
    /// when no shader program is present.
    pub fn draw_arrays(&mut self, mode: u32, first: i32, count: i32) {
        if count <= 0 || first < 0 {
            return;
        }
        let first = first as usize;
        let count = count as usize;

        // Try shaded path (returns false → fall through to flat-fill fallback).
        if self.draw_arrays_shaded(mode, first, count) {
            return;
        }

        // ── Flat-fill fallback (task #28 behaviour) ───────────────────────
        let positions = match self.collect_positions(first, count) {
            Some(p) if !p.is_empty() => p,
            _ => return,
        };
        let color = self.draw_color;
        match mode {
            TRIANGLES => {
                for tri in positions.chunks_exact(3) {
                    self.fill_triangle(tri[0], tri[1], tri[2], color);
                }
            }
            TRIANGLE_STRIP => {
                for i in 0..positions.len().saturating_sub(2) {
                    let (a, b, c) = if i % 2 == 0 {
                        (positions[i], positions[i + 1], positions[i + 2])
                    } else {
                        (positions[i + 1], positions[i], positions[i + 2])
                    };
                    self.fill_triangle(a, b, c, color);
                }
            }
            TRIANGLE_FAN => {
                let hub = positions[0];
                for i in 1..positions.len().saturating_sub(1) {
                    self.fill_triangle(hub, positions[i], positions[i + 1], color);
                }
            }
            POINTS => {
                for p in &positions {
                    let (px, py) = self.ndc_to_screen(p.0, p.1);
                    self.blend_pixel(px, py, color);
                }
            }
            LINES => {
                for seg in positions.chunks_exact(2) {
                    self.draw_line(seg[0], seg[1], color);
                }
            }
            LINE_STRIP => {
                for i in 0..positions.len().saturating_sub(1) {
                    self.draw_line(positions[i], positions[i + 1], color);
                }
            }
            _ => {}
        }
    }

    // ── GLSL shaded draw path ────────────────────────────────────────────────

    /// Try to execute the active vertex + fragment program for `drawArrays`.
    /// Returns `false` when no program is active or shaders are not yet parsed,
    /// in which case the caller should fall back to flat-fill.
    fn draw_arrays_shaded(&mut self, mode: u32, first: usize, count: usize) -> bool {
        let prog_id = self.current_program;
        if prog_id == 0 { return false; }

        // Extract Arc-clones of parsed shaders (cheap refcount bumps).
        let (vs_arc, fs_arc, attrib_locs) = {
            let prog = match self.programs.get(&prog_id) { Some(p) => p, None => return false };
            let vs_id = match prog.vertex { Some(id) => id, None => return false };
            let fs_id = match prog.fragment { Some(id) => id, None => return false };
            let vs_arc = match self.shaders.get(&vs_id).and_then(|s| s.parsed.clone()) {
                Some(p) => p, None => return false,
            };
            let fs_arc = match self.shaders.get(&fs_id).and_then(|s| s.parsed.clone()) {
                Some(p) => p, None => return false,
            };
            // location → attribute name (reverse of prog.attribs: name → loc)
            let attrib_locs: HashMap<u32, String> = prog.attribs.iter()
                .map(|(name, &loc)| (loc as u32, name.clone()))
                .collect();
            (vs_arc, fs_arc, attrib_locs)
        };

        // Build uniform name → value map for the active program.
        let uniform_map = self.build_uniform_map(prog_id);

        // Execute vertex shader for each vertex.
        let mut vertices: Vec<VertexOutput> = Vec::with_capacity(count);
        for vi in first..first + count {
            let attrs = self.collect_vertex_attribs(vi, &attrib_locs);
            let mut env = glsl::ShaderEnv::new(&uniform_map);
            // Seed gl_Position from attribute 0 (conventional position attribute)
            // so that shaders that don't write gl_Position explicitly still produce
            // correct clip-space positions — backward-compatible with task #28.
            if let Some(attr0_name) = attrib_locs.get(&0) && let Some(val) = attrs.get(attr0_name) {
                env.position = val.to_vec4();
            }
            env.attributes = attrs;
            exec_main(&vs_arc, &mut env);
            vertices.push(VertexOutput { position: env.position, varyings: env.varyings });
        }

        // Rasterize with fragment shader.
        self.rasterize_shaded(mode, &vertices, &fs_arc, &uniform_map);
        true
    }

    /// Build a `name → Val` uniform map for the given program by looking up
    /// `uniform_vals` (which stores values by location) through the program's
    /// `uniforms` (name → location) table.
    fn build_uniform_map(&self, prog_id: u32) -> HashMap<String, Val> {
        let prog = match self.programs.get(&prog_id) { Some(p) => p, None => return HashMap::new() };
        let mut map = HashMap::with_capacity(prog.uniforms.len());
        for (name, &loc) in &prog.uniforms {
            if let Some(val) = self.uniform_vals.get(&loc) {
                // For sampler2D uniforms, inject the averaged texture colour so
                // that `texture2D(u_sampler, v_uv)` can return a meaningful value.
                let final_val = if let Val::Sampler(unit) = val {
                    let tex_id = self.bound_textures.get(unit).copied().unwrap_or(0);
                    if let Some(&rgba) = self.texture_solid.get(&tex_id) {
                        // Store the sampler handle; also store the resolved colour
                        // under a magic key `__tex_<unit>` so texture2D can find it.
                        map.insert(format!("__tex_{}", unit), Val::Vec4(rgba));
                    }
                    val.clone()
                } else {
                    val.clone()
                };
                map.insert(name.clone(), final_val);
            }
        }
        map
    }

    /// Read all enabled vertex attributes for vertex index `vi`, returning a
    /// map from attribute name to its `Val`.
    fn collect_vertex_attribs(
        &self,
        vi: usize,
        attrib_locs: &HashMap<u32, String>,
    ) -> HashMap<String, Val> {
        let mut out = HashMap::new();
        for (&loc, name) in attrib_locs {
            let ap = match self.attribs.get(&loc) { Some(a) => a, None => continue };
            if !ap.enabled { continue; }
            let data = match self.buffers.get(&ap.buffer) { Some(d) => d, None => continue };
            let stride = if ap.stride_floats == 0 { ap.size } else { ap.stride_floats };
            let base = ap.offset_floats + vi * stride;
            let val = match ap.size {
                1 => data.get(base).map(|&v| Val::Float(v)),
                2 => if base + 1 < data.len() {
                    Some(Val::Vec2([data[base], data[base + 1]]))
                } else { None },
                3 => if base + 2 < data.len() {
                    Some(Val::Vec3([data[base], data[base + 1], data[base + 2]]))
                } else { None },
                _ => if base + 3 < data.len() {
                    Some(Val::Vec4([data[base], data[base + 1], data[base + 2], data[base + 3]]))
                } else { None },
            };
            if let Some(v) = val { out.insert(name.clone(), v); }
        }
        out
    }

    /// Rasterize `vertices` using the fragment shader `fs`.
    fn rasterize_shaded(
        &mut self,
        mode: u32,
        vertices: &[VertexOutput],
        fs: &glsl::ParsedShader,
        uniforms: &HashMap<String, Val>,
    ) {
        match mode {
            TRIANGLES => {
                for tri in vertices.chunks_exact(3) {
                    self.fill_triangle_shaded(&tri[0], &tri[1], &tri[2], fs, uniforms);
                }
            }
            TRIANGLE_STRIP => {
                for i in 0..vertices.len().saturating_sub(2) {
                    let (a, b, c) = if i % 2 == 0 {
                        (&vertices[i], &vertices[i + 1], &vertices[i + 2])
                    } else {
                        (&vertices[i + 1], &vertices[i], &vertices[i + 2])
                    };
                    self.fill_triangle_shaded(a, b, c, fs, uniforms);
                }
            }
            TRIANGLE_FAN => {
                let hub = &vertices[0];
                for i in 1..vertices.len().saturating_sub(1) {
                    self.fill_triangle_shaded(hub, &vertices[i], &vertices[i + 1], fs, uniforms);
                }
            }
            POINTS => {
                for v in vertices {
                    let (px, py) = self.ndc_to_screen(v.position[0], v.position[1]);
                    let (rgba, discard) = self.run_fragment_at(fs, uniforms, &v.varyings);
                    if !discard { self.blend_pixel(px, py, rgba); }
                }
            }
            LINES => {
                for seg in vertices.chunks_exact(2) {
                    self.draw_line_shaded(&seg[0], &seg[1], fs, uniforms);
                }
            }
            LINE_STRIP => {
                for i in 0..vertices.len().saturating_sub(1) {
                    self.draw_line_shaded(&vertices[i], &vertices[i + 1], fs, uniforms);
                }
            }
            _ => {}
        }
    }

    /// Rasterize a triangle using barycentric interpolation of varyings and the
    /// fragment shader.
    fn fill_triangle_shaded(
        &mut self,
        a: &VertexOutput,
        b: &VertexOutput,
        c: &VertexOutput,
        fs: &glsl::ParsedShader,
        uniforms: &HashMap<String, Val>,
    ) {
        let pa = self.ndc_to_screen(a.position[0], a.position[1]);
        let pb = self.ndc_to_screen(b.position[0], b.position[1]);
        let pc = self.ndc_to_screen(c.position[0], c.position[1]);

        let min_x = pa.0.min(pb.0).min(pc.0).floor().max(0.0) as i32;
        let max_x = pa.0.max(pb.0).max(pc.0).ceil().min(self.width as f32) as i32;
        let min_y = pa.1.min(pb.1).min(pc.1).floor().max(0.0) as i32;
        let max_y = pa.1.max(pb.1).max(pc.1).ceil().min(self.height as f32) as i32;

        let area = edge(pa, pb, pc);
        if area.abs() < f32::EPSILON { return; }

        for y in min_y..max_y {
            for x in min_x..max_x {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(pb, pc, p);
                let w1 = edge(pc, pa, p);
                let w2 = edge(pa, pb, p);
                let inside = (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                    || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
                if !inside { continue; }

                // Normalised barycentric weights (λA, λB, λC).
                let total = w0 + w1 + w2;
                let (la, lb, lc) = (w0 / total, w1 / total, w2 / total);
                let varyings = interp_varyings(&a.varyings, &b.varyings, &c.varyings, la, lb, lc);

                let (color, discard) = self.run_fragment_at(fs, uniforms, &varyings);
                if !discard { self.blend_pixel(p.0, p.1, color); }
            }
        }
    }

    /// Rasterize a line segment using the fragment shader at each step.
    fn draw_line_shaded(
        &mut self,
        a: &VertexOutput,
        b: &VertexOutput,
        fs: &glsl::ParsedShader,
        uniforms: &HashMap<String, Val>,
    ) {
        let pa = self.ndc_to_screen(a.position[0], a.position[1]);
        let pb = self.ndc_to_screen(b.position[0], b.position[1]);
        let dx = pb.0 - pa.0;
        let dy = pb.1 - pa.1;
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let varyings = interp_varyings(&a.varyings, &b.varyings, &HashMap::new(),
                                           1.0 - t, t, 0.0);
            let (color, discard) = self.run_fragment_at(fs, uniforms, &varyings);
            if !discard { self.blend_pixel(pa.0 + dx * t, pa.1 + dy * t, color); }
        }
    }

    /// Run the fragment shader at a single sample point. Returns `(color, discard)`.
    ///
    /// `frag_color` is pre-seeded from `self.draw_color` so that shaders that
    /// do not assign `gl_FragColor` fall back to the flat uniform colour.
    fn run_fragment_at(
        &self,
        fs: &glsl::ParsedShader,
        uniforms: &HashMap<String, Val>,
        varyings: &HashMap<String, Val>,
    ) -> ([f32; 4], bool) {
        let mut env = glsl::ShaderEnv::new(uniforms);
        env.frag_color = self.draw_color;
        env.varyings = varyings.clone();
        exec_main(fs, &mut env);
        (env.frag_color, env.discard)
    }

    // ── Internal rasterization ──────────────────────────────────────────────

    /// Gather NDC `(x, y)` for vertices `first..first+count` from attribute 0.
    fn collect_positions(&self, first: usize, count: usize) -> Option<Vec<(f32, f32)>> {
        let attr = self.attribs.get(&0)?;
        if !attr.enabled || attr.size < 2 {
            return None;
        }
        let data = self.buffers.get(&attr.buffer)?;
        let stride = if attr.stride_floats == 0 {
            attr.size
        } else {
            attr.stride_floats
        };
        let mut out = Vec::with_capacity(count);
        for v in first..first + count {
            let base = attr.offset_floats + v * stride;
            if base + 1 >= data.len() {
                break;
            }
            out.push((data[base], data[base + 1]));
        }
        Some(out)
    }

    /// Map NDC `[-1, 1]` to framebuffer pixel coords through the viewport.
    /// GL's bottom-left Y origin is flipped to the framebuffer's top-left.
    fn ndc_to_screen(&self, nx: f32, ny: f32) -> (f32, f32) {
        let (vx, vy, vw, vh) = self.viewport;
        let sx = vx as f32 + (nx * 0.5 + 0.5) * vw as f32;
        let sy = vy as f32 + (1.0 - (ny * 0.5 + 0.5)) * vh as f32;
        (sx, sy)
    }

    /// Flat-fill a triangle given in NDC, blending `color` (source-over).
    fn fill_triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: [f32; 4]) {
        let pa = self.ndc_to_screen(a.0, a.1);
        let pb = self.ndc_to_screen(b.0, b.1);
        let pc = self.ndc_to_screen(c.0, c.1);

        let min_x = pa.0.min(pb.0).min(pc.0).floor().max(0.0) as i32;
        let max_x = pa.0.max(pb.0).max(pc.0).ceil().min(self.width as f32) as i32;
        let min_y = pa.1.min(pb.1).min(pc.1).floor().max(0.0) as i32;
        let max_y = pa.1.max(pb.1).max(pc.1).ceil().min(self.height as f32) as i32;

        let area = edge(pa, pb, pc);
        if area.abs() < f32::EPSILON {
            return;
        }

        for y in min_y..max_y {
            for x in min_x..max_x {
                // Sample at pixel centre.
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(pb, pc, p);
                let w1 = edge(pc, pa, p);
                let w2 = edge(pa, pb, p);
                // Inside if all edge functions share the triangle's winding sign.
                let inside = (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                    || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
                if inside {
                    self.blend_pixel(p.0, p.1, color);
                }
            }
        }
    }

    /// Rasterize a 1px line between two NDC points (DDA), blending `color`.
    fn draw_line(&mut self, a: (f32, f32), b: (f32, f32), color: [f32; 4]) {
        let pa = self.ndc_to_screen(a.0, a.1);
        let pb = self.ndc_to_screen(b.0, b.1);
        let dx = pb.0 - pa.0;
        let dy = pb.1 - pa.1;
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.blend_pixel(pa.0 + dx * t, pa.1 + dy * t, color);
        }
    }

    /// Source-over blend `color` (RGBA `[0,1]`) into the pixel at float coords.
    fn blend_pixel(&mut self, fx: f32, fy: f32, color: [f32; 4]) {
        if fx < 0.0 || fy < 0.0 {
            return;
        }
        let x = fx as u32;
        let y = fy as u32;
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        let sa = clamp01(color[3]);
        if sa <= 0.0 {
            return;
        }
        let inv = 1.0 - sa;
        for (c, &src_c) in color.iter().take(3).enumerate() {
            let src = clamp01(src_c) * 255.0;
            let dst = self.framebuffer[i + c] as f32;
            self.framebuffer[i + c] = (src * sa + dst * inv).round().clamp(0.0, 255.0) as u8;
        }
        let dst_a = self.framebuffer[i + 3] as f32 / 255.0;
        let out_a = sa + dst_a * inv;
        self.framebuffer[i + 3] = to_u8(out_a);
    }
}

/// Signed area of the triangle `(a, b, c)` (edge function for barycentrics).
fn edge(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Clamp a float to `[0, 1]`.
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Convert a `[0, 1]` float to a `0..=255` byte with rounding.
fn to_u8(v: f32) -> u8 {
    (clamp01(v) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upload a tightly-packed vec2 position buffer and wire attribute 0 to it.
    fn setup_positions(gl: &mut SoftwareWebGl, verts: &[f32]) {
        let buf = gl.create_buffer();
        gl.bind_buffer(ARRAY_BUFFER, buf);
        gl.buffer_data_f32(ARRAY_BUFFER, verts.to_vec());
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer(0, 2, 0, 0);
    }

    #[test]
    fn new_buffer_is_transparent() {
        let gl = SoftwareWebGl::new(4, 4);
        assert_eq!(gl.pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(gl.pixels().len(), 4 * 4 * 4);
    }

    #[test]
    fn min_dimensions_are_clamped() {
        let gl = SoftwareWebGl::new(0, 0);
        assert_eq!(gl.width(), 1);
        assert_eq!(gl.height(), 1);
    }

    #[test]
    fn clear_fills_with_clear_color() {
        let mut gl = SoftwareWebGl::new(2, 2);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(COLOR_BUFFER_BIT);
        assert_eq!(gl.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(gl.pixel(1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn clear_without_color_bit_is_noop() {
        let mut gl = SoftwareWebGl::new(2, 2);
        gl.clear_color(1.0, 0.0, 0.0, 1.0);
        gl.clear(DEPTH_BUFFER_BIT);
        assert_eq!(gl.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn clear_color_is_clamped() {
        let mut gl = SoftwareWebGl::new(1, 1);
        gl.clear_color(2.0, -1.0, 0.5, 5.0);
        gl.clear(COLOR_BUFFER_BIT);
        assert_eq!(gl.pixel(0, 0), [255, 0, 128, 255]);
    }

    #[test]
    fn buffer_ids_are_nonzero_and_unique() {
        let mut gl = SoftwareWebGl::new(1, 1);
        let a = gl.create_buffer();
        let b = gl.create_buffer();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn shader_program_lifecycle() {
        let mut gl = SoftwareWebGl::new(1, 1);
        let vs = gl.create_shader(VERTEX_SHADER);
        let fs = gl.create_shader(FRAGMENT_SHADER);
        gl.shader_source(vs, "void main(){}".into());
        gl.compile_shader(vs);
        gl.compile_shader(fs);
        assert!(gl.shader_compiled(vs));
        assert!(gl.shader_compiled(fs));

        let prog = gl.create_program();
        gl.attach_shader(prog, vs);
        gl.attach_shader(prog, fs);
        gl.link_program(prog);
        assert!(gl.program_linked(prog));
        gl.use_program(prog);
    }

    #[test]
    fn unknown_shader_kind_returns_zero() {
        let mut gl = SoftwareWebGl::new(1, 1);
        assert_eq!(gl.create_shader(0x1234), 0);
    }

    #[test]
    fn attrib_and_uniform_locations_are_stable() {
        let mut gl = SoftwareWebGl::new(1, 1);
        let prog = gl.create_program();
        let a1 = gl.get_attrib_location(prog, "a_position");
        let a2 = gl.get_attrib_location(prog, "a_position");
        assert_eq!(a1, a2);
        let u1 = gl.get_uniform_location(prog, "u_color");
        assert_ne!(u1, a1);
        assert_eq!(u1, gl.get_uniform_location(prog, "u_color"));
    }

    #[test]
    fn location_for_unknown_program_is_negative() {
        let mut gl = SoftwareWebGl::new(1, 1);
        assert_eq!(gl.get_attrib_location(999, "x"), -1);
        assert_eq!(gl.get_uniform_location(999, "x"), -1);
    }

    #[test]
    fn fullscreen_triangle_fills_center() {
        // Two clip-space triangles covering the whole quad.
        let mut gl = SoftwareWebGl::new(8, 8);
        let verts = [
            -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, // tri 1
            -1.0, 1.0, 1.0, -1.0, 1.0, 1.0, // tri 2
        ];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 0.0, 1.0, 0.0, 1.0); // opaque green
        gl.draw_arrays(TRIANGLES, 0, 6);
        assert_eq!(gl.pixel(4, 4), [0, 255, 0, 255]);
        assert_eq!(gl.pixel(0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn draw_without_uniform_uses_white() {
        let mut gl = SoftwareWebGl::new(4, 4);
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.draw_arrays(TRIANGLES, 0, 6);
        assert_eq!(gl.pixel(2, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn disabled_attribute_draws_nothing() {
        let mut gl = SoftwareWebGl::new(4, 4);
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0];
        let buf = gl.create_buffer();
        gl.bind_buffer(ARRAY_BUFFER, buf);
        gl.buffer_data_f32(ARRAY_BUFFER, verts.to_vec());
        gl.vertex_attrib_pointer(0, 2, 0, 0); // never enabled
        gl.uniform4f(0, 1.0, 0.0, 0.0, 1.0);
        gl.draw_arrays(TRIANGLES, 0, 3);
        assert_eq!(gl.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn triangle_covers_only_its_half() {
        // Lower-left triangle of an 8x8 buffer.
        let mut gl = SoftwareWebGl::new(8, 8);
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 1.0, 0.0, 0.0, 1.0);
        gl.draw_arrays(TRIANGLES, 0, 3);
        // Bottom-left pixel covered, top-right corner outside the triangle.
        assert_eq!(gl.pixel(0, 7), [255, 0, 0, 255]);
        assert_eq!(gl.pixel(7, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn alpha_blends_source_over() {
        // Single triangle to avoid double coverage on a shared quad diagonal.
        let mut gl = SoftwareWebGl::new(4, 4);
        gl.clear_color(0.0, 0.0, 0.0, 1.0);
        gl.clear(COLOR_BUFFER_BIT);
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 1.0, 1.0, 1.0, 0.5); // 50% white over black
        gl.draw_arrays(TRIANGLES, 0, 3);
        // Bottom-left pixel lies strictly inside the single triangle.
        let p = gl.pixel(0, 3);
        assert_eq!(p[3], 255);
        assert!((120..=136).contains(&(p[0] as i32)), "got {}", p[0]);
    }

    #[test]
    fn triangle_strip_assembles_quad() {
        let mut gl = SoftwareWebGl::new(8, 8);
        // Standard 4-vertex strip covering the quad.
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 0.0, 0.0, 1.0, 1.0);
        gl.draw_arrays(TRIANGLE_STRIP, 0, 4);
        assert_eq!(gl.pixel(4, 4), [0, 0, 255, 255]);
    }

    #[test]
    fn viewport_restricts_drawing() {
        let mut gl = SoftwareWebGl::new(8, 8);
        gl.viewport(0, 0, 4, 4); // top-left quarter only
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 1.0, 0.0, 0.0, 1.0);
        gl.draw_arrays(TRIANGLES, 0, 6);
        assert_eq!(gl.pixel(1, 1), [255, 0, 0, 255]);
        assert_eq!(gl.pixel(6, 6), [0, 0, 0, 0]);
    }

    #[test]
    fn draw_arrays_with_zero_count_is_noop() {
        let mut gl = SoftwareWebGl::new(4, 4);
        let verts = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0];
        setup_positions(&mut gl, &verts);
        gl.uniform4f(0, 1.0, 0.0, 0.0, 1.0);
        gl.draw_arrays(TRIANGLES, 0, 0);
        assert_eq!(gl.pixel(0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn stride_and_offset_are_honored() {
        // Interleaved [x, y, pad] per vertex: stride 12 bytes, offset 0.
        let mut gl = SoftwareWebGl::new(8, 8);
        let verts = [
            -1.0, -1.0, 9.0, 1.0, -1.0, 9.0, -1.0, 1.0, 9.0, -1.0, 1.0, 9.0, 1.0, -1.0, 9.0, 1.0,
            1.0, 9.0,
        ];
        let buf = gl.create_buffer();
        gl.bind_buffer(ARRAY_BUFFER, buf);
        gl.buffer_data_f32(ARRAY_BUFFER, verts.to_vec());
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer(0, 2, 12, 0); // stride 3 floats
        gl.uniform4f(0, 0.0, 1.0, 0.0, 1.0);
        gl.draw_arrays(TRIANGLES, 0, 6);
        assert_eq!(gl.pixel(4, 4), [0, 255, 0, 255]);
    }
}
