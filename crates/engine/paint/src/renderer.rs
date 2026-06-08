//! wgpu-растеризатор для display list.
//!
//! Три конвейера:
//! 1. **Fill** — заливка прямоугольников цветом. Вершина = (pos, color),
//!    альфа-блендинг. Используется для backgrounds блоков и border-edge-ей.
//! 2. **Text** — текстурированные квады по глифам из atlas-а.
//!    Вершина = (pos, uv, color), фрагмент сэмплит R8-альфу из atlas-а
//!    и умножает на цвет текста.
//! 3. **Image** — RGBA-texture quad per image. Вершина = (pos, uv), фрагмент
//!    сэмплит per-image `Rgba8Unorm` текстуру. Каждый зарегистрированный
//!    источник (`src`) держит свою `wgpu::Texture` + bind group; общий
//!    sampler. Без cache hit — fallback на светло-серый fill (как раньше).
//!
//! Глифы растеризуются по требованию через `lumen_font::Rasterizer` на
//! **подобранный bin размера** (`size_bin_for(font_size)`). Bin-набор —
//! `SIZE_BINS = [8, 12, 16, 20, 24, 32, 48, 64]`; font_size округляется
//! вверх до ближайшего bin (или до 64 если больше). Display-сторона
//! масштабирует квад в долю `font_size / size_bin` — если font_size совпал
//! с bin-ом (16/24 px), масштаба нет вовсе. Это устраняет blur от линейной
//! интерполяции fixed-size атласа (раньше всё рисовалось на 24 px и потом
//! масштабировалось).

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::ext::{FontProvider, FontStyle as CssFontStyle};
use lumen_core::geom::Rect;
use lumen_font::{
    Bitmap, Cmap, Font, Head, Hhea, Hmtx, Outline, Rasterizer, SystemFontIndex,
    maybe_decode_font,
};
use lumen_image::{correct_rgba_pixels, Image, PixelFormat};
use lumen_layout::{BackgroundRepeat, BackgroundSize, BorderStyle, Color, FilterFn, FontStyle, FontWeight, GradientStop, ImageRendering, Length, Mat4, ObjectFit, ObjectPosition, OutlineStyle, PositionComponent};
use winit::window::Window;

use crate::atlas::{AtlasKey, GlyphAtlas, GlyphEntry};
use crate::display_list::{fit_image_quad, fit_image_rect, BlendMode, CornerRadii, MaskMode};
use crate::fingerprint::GpuFingerprint;
use lumen_image::{resize_area_avg, resize_bilinear};
use crate::DisplayCommand;

/// Размер атласа в пикселях (квадратный). Поднят с 512 до 1024 под
/// multi-size atlas: типичная страница использует 2-3 размера шрифта,
/// что даёт ~3× больше уникальных глифов в кеше.
const ATLAS_DIM: u32 = 1024;

/// Bin размеров растеризации (CSS px). `font_size` округляется до
/// ближайшего bin вверх через `size_bin_for`. Если ≤ 8 — используется
/// bin 8 (нечитаемо иначе всё равно); если > 64 — bin 64 с up-scaling-ом
/// (большие заголовки редки, потеря качества на единичных headline-ах
/// приемлема в Phase 0). При совпадении font_size с bin-ом квад не
/// масштабируется (нет blur).
const SIZE_BINS: [u16; 8] = [8, 12, 16, 20, 24, 32, 48, 64];

/// CSS px → размер растеризации в `SIZE_BINS`. Round-up до ближайшего bin;
/// > последнего bin — клампим к последнему.
fn size_bin_for(font_size: f32) -> u16 {
    // NaN / negative / 0 — недопустимый вход (Phase 0 не должно происходить),
    // клампим к min-bin без panic. INFINITY = «больше любого bin» → max-bin.
    if font_size.is_nan() || font_size <= 0.0 {
        return SIZE_BINS[0];
    }
    if font_size.is_infinite() {
        return SIZE_BINS[SIZE_BINS.len() - 1];
    }
    let target = font_size.ceil() as u16;
    for &bin in &SIZE_BINS {
        if bin >= target {
            return bin;
        }
    }
    SIZE_BINS[SIZE_BINS.len() - 1]
}

/// Конструктор `AtlasKey` из renderer-овых типов. face_id хранится в
/// renderer как `usize`, но atlas использует `u16` (Phase 0 hardcap на
/// число face-ов — тысячи нереалистично, 1-16 типично). Конверсия с
/// `as` ⇒ значения >65535 будут warapped — приемлемо для defensive Phase 0
/// (atlas всё равно перестанет работать задолго до).
fn atlas_key(
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords_hash: u64,
) -> AtlasKey {
    AtlasKey::new((face_id & 0xFFFF) as u16, glyph_id, size_bin, coords_hash)
}

const FILL_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000).
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) color: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    // CSS z: positive=closer. WebGPU: smaller depth=front.
    // ±10000 CSS px → [0,1]: z=0→0.5 (2D, painter's order), z>0→<0.5 (front), z<0→>0.5 (back).
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// SDF-круг: UV (-1..1) из центра; фрагменты за радиусом 1.0 discarded.
/// Anti-aliasing через smoothstep(0.9, 1.0, dist).
/// SDF-круг: Skia-compatible 1px linear AA: coverage = clamp(0.5 + r - dist_px, 0, 1).
/// Quad расширен на 0.5px с каждой стороны, UV=±1 соответствует r+0.5 px от центра.
/// `radius_px` (loc 3) — CSS-радиус точки. Формула совпадает с Skia, что минимизирует
/// разницу с Chrome/Edge (пиксельный pixel-diff для dotted border ≈ sub-pixel noise).
const CIRCLE_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos:       vec2<f32>,
    @location(1) uv:        vec2<f32>,
    @location(2) color:     vec4<f32>,
    @location(3) radius_px: f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>,
    @location(1) color:     vec4<f32>,
    @location(2) radius_px: f32,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    var out: VOut;
    out.clip      = vec4<f32>(ndc, 0.0, 1.0);
    out.uv        = in.uv;
    out.color     = in.color;
    out.radius_px = in.radius_px;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Quad spans (r+0.5) px in each direction from center, so dist_px = |uv| * (r+0.5).
    let dist_px = length(in.uv) * (in.radius_px + 0.5);
    let alpha = clamp(0.5 + in.radius_px - dist_px, 0.0, 1.0);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

/// SDF rounded-rect shader with elliptical per-corner radii.
/// Per-vertex data carries the rect's center, half-size, and two vec4s for
/// horizontal (x) and vertical (y) corner radii, enabling `border-radius: H/V`.
///
/// Vertex layout (matches `RRectVertex`):
///   loc 0  pos       vec2  – screen CSS-px position
///   loc 1  z         f32   – CSS depth px (transform-style: preserve-3d)
///   loc 2  color     vec4  – premultiplied RGBA
///   loc 3  center    vec2  – CSS-px center of the rounded rect
///   loc 4  half_size vec2  – CSS-px half-dimensions (w/2, h/2)
///   loc 5  radii_x   vec4  – horizontal corner radii px: tl, tr, br, bl
///   loc 6  radii_y   vec4  – vertical corner radii px:   tl, tr, br, bl
const RRECT_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos:       vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_rrect_verts.
    @location(1) z:         f32,
    @location(2) color:     vec4<f32>,
    @location(3) center:    vec2<f32>,
    @location(4) half_size: vec2<f32>,
    @location(5) radii_x:   vec4<f32>,
    @location(6) radii_y:   vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color:     vec4<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) center:    vec2<f32>,
    @location(3) half_size: vec2<f32>,
    @location(4) radii_x:   vec4<f32>,
    @location(5) radii_y:   vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip      = vec4<f32>(ndc, depth, 1.0);
    out.color     = in.color;
    out.world_pos = in.pos;
    out.center    = in.center;
    out.half_size = in.half_size;
    out.radii_x   = in.radii_x;
    out.radii_y   = in.radii_y;
    return out;
}

/// SDF for an axis-aligned rounded rectangle with per-corner elliptical radii.
/// `p`         = position relative to rect center.
/// `half_size` = half-dimensions of the rect.
/// `radii_x`   = horizontal corner radii (tl, tr, br, bl).
/// `radii_y`   = vertical  corner radii (tl, tr, br, bl).
///
/// Screen y-axis is DOWN: p.y < 0 = top half, p.y > 0 = bottom half.
/// For circular corners (rx == ry) this degenerates to the standard Quilez SDF.
/// Elliptical corners use a first-order approximation: (|q/r| - 1) * min(rx,ry),
/// which is exact on the ellipse surface and has unit gradient near the boundary.
fn sdf_rrect(p: vec2<f32>, half_size: vec2<f32>, radii_x: vec4<f32>, radii_y: vec4<f32>) -> f32 {
    // Select corner radii based on quadrant (y-down screen space).
    var rx: f32 = radii_x.x; // top-left (default)
    var ry: f32 = radii_y.x;
    if p.x >= 0.0 && p.y <= 0.0 { rx = radii_x.y; ry = radii_y.y; } // top-right
    if p.x >= 0.0 && p.y >  0.0 { rx = radii_x.z; ry = radii_y.z; } // bottom-right
    if p.x <  0.0 && p.y >  0.0 { rx = radii_x.w; ry = radii_y.w; } // bottom-left
    // CSS Backgrounds L3 §5.5 overlap clamp: radius must fit inside half-box.
    rx = min(rx, half_size.x);
    ry = min(ry, half_size.y);
    // Position relative to corner center (both axes clamped to ≥ 0 for corner).
    let q = abs(p) - half_size + vec2<f32>(rx, ry);
    // Inside the straight (non-corner) region.
    if q.x <= 0.0 && q.y <= 0.0 { return max(q.x, q.y); }
    // Sharp corner (degenerate radius): standard box SDF.
    if rx < 0.001 || ry < 0.001 {
        return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0);
    }
    // Only one axis in the corner region.
    if q.x <= 0.0 { return q.y; }
    if q.y <= 0.0 { return q.x; }
    // Both axes in the ellipse corner: first-order ellipse SDF approximation.
    // For rx == ry this is identical to the Quilez circular formula.
    let k = length(q / vec2<f32>(rx, ry));
    return (k - 1.0) * min(rx, ry);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let p = in.world_pos - in.center;
    let d = sdf_rrect(p, in.half_size, in.radii_x, in.radii_y);
    // Sub-pixel anti-aliasing: smoothstep over [-0.5, 0.5] px.
    let alpha = 1.0 - smoothstep(-0.5, 0.5, d);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

const TEXT_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_tex, atlas_smp, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

const IMAGE_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_smp: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) uv: vec2<f32>,
    @location(3) alpha: f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.uv = in.uv;
    out.alpha = in.alpha;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sample = textureSample(image_tex, image_smp, in.uv);
    return vec4<f32>(sample.rgb, sample.a * in.alpha);
}
"#;

/// CSS Images L4 §4 — `cross-fade(A, B, p)` shader.
///
/// Bindings:
/// * group 0 binding 0 — viewport uniform (shared with `image_pipeline`).
/// * group 1 binding 0 — `tex_a` (Rgba8Unorm).
/// * group 1 binding 1 — `tex_b` (Rgba8Unorm).
/// * group 1 binding 2 — shared `sampler` (filtering).
/// * group 1 binding 3 — `CrossFadeParams { progress: f32 }` uniform
///   (padded to 16 bytes for std140 alignment).
///
/// Fragment formula: `mix(sample_a, sample_b, progress)` — straight RGBA
/// interpolation (CSS Images L4 §4.2). Shader emits straight-alpha; pipeline
/// uses `ALPHA_BLENDING` so the GPU performs `SrcAlpha · src + (1-SrcAlpha) · dst`
/// — same convention as `image_pipeline`.
const CROSS_FADE_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};

struct CrossFadeParams {
    // x = progress, yzw = padding (uniform buffer requires 16-byte alignment).
    progress: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex_a: texture_2d<f32>;
@group(1) @binding(1) var tex_b: texture_2d<f32>;
@group(1) @binding(2) var smp: sampler;
@group(1) @binding(3) var<uniform> p: CrossFadeParams;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    var out: VOut;
    // CrossFade is a flat 2D primitive: depth = 0.5 (mid plane), matching how
    // FillVertex maps z = 0.0 → 0.5. preserve-3d transforms are deferred.
    out.clip = vec4<f32>(ndc, 0.5, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(tex_a, smp, in.uv);
    let b = textureSample(tex_b, smp, in.uv);
    let t = clamp(p.progress, 0.0, 1.0);
    // Straight-alpha mix per CSS Images L4 §4.2. Pipeline blend state is
    // ALPHA_BLENDING (SrcAlpha · src + (1-SrcAlpha) · dst), matching
    // image_pipeline — shader emits straight-alpha RGBA, blend stage applies
    // the SrcAlpha multiplication.
    return mix(a, b, t);
}
"#;

const COMPOSITE_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_layer: texture_2d<f32>;
@group(0) @binding(1) var s_layer: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    out.clip = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    out.alpha = in.alpha;
    return out;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_layer, s_layer, in.uv);
    // Off-screen layers accumulate premultiplied-alpha content (ALPHA_BLENDING onto clear).
    // Apply opacity to both rgb and alpha so premultiplied invariant is preserved.
    return vec4<f32>(c.rgb * in.alpha, c.a * in.alpha);
}
"#;

/// CSS Compositing & Blending L1 §8 blend shader.
/// Bindings: 0=t_src (offscreen element), 1=t_dst (copy of parent layer),
/// 2=sampler (shared), 3=blend_mode uniform (u32, padded to 16 bytes).
/// Blend mode u32 mapping: 0=Normal, 1=Multiply, 2=Screen, 3=Overlay,
/// 4=Darken, 5=Lighten, 6=ColorDodge, 7=ColorBurn, 8=HardLight, 9=SoftLight,
/// 10=Difference, 11=Exclusion, 12=Hue, 13=Saturation, 14=Color,
/// 15=Luminosity, 16=PlusLighter.
/// Output is written as pre-composited RGBA (REPLACE blend state).
const BLEND_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var t_dst: texture_2d<f32>;
@group(0) @binding(2) var s_layer: sampler;

struct BlendUniform {
    mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};
@group(0) @binding(3) var<uniform> u: BlendUniform;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    out.clip = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

// ── Luminance / Saturation helpers (non-separable modes) ──────────────
fn lum(c: vec3<f32>) -> f32 {
    return 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var result = c;
    if n < 0.0 {
        result = l + (c - l) * l / (l - n);
    }
    let l2 = lum(result);
    let x2 = max(result.r, max(result.g, result.b));
    if x2 > 1.0 {
        result = l2 + (result - l2) * (1.0 - l2) / (x2 - l2);
    }
    return result;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(c);
    return clip_color(c + d);
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    // Sort components to find min/mid/max indices.
    var result = c;
    // Use if-chains to set min/mid/max channels.
    var cmin: f32; var cmid: f32; var cmax: f32;
    var imin: i32; var imid: i32; var imax: i32;
    let cv = array<f32, 3>(c.r, c.g, c.b);
    // Find indices of min, mid, max by sorting.
    if cv[0] <= cv[1] && cv[0] <= cv[2] {
        imin = 0;
        if cv[1] <= cv[2] { imid = 1; imax = 2; } else { imid = 2; imax = 1; }
    } else if cv[1] <= cv[0] && cv[1] <= cv[2] {
        imin = 1;
        if cv[0] <= cv[2] { imid = 0; imax = 2; } else { imid = 2; imax = 0; }
    } else {
        imin = 2;
        if cv[0] <= cv[1] { imid = 0; imax = 1; } else { imid = 1; imax = 0; }
    }
    cmin = cv[imin]; cmid = cv[imid]; cmax = cv[imax];
    var rmin: f32; var rmid: f32; var rmax: f32;
    if cmax > cmin {
        rmid = (cmid - cmin) * s / (cmax - cmin);
        rmax = s;
    } else {
        rmid = 0.0;
        rmax = 0.0;
    }
    rmin = 0.0;
    // Reconstruct result in original channel order.
    var arr = array<f32, 3>(0.0, 0.0, 0.0);
    arr[imin] = rmin;
    arr[imid] = rmid;
    arr[imax] = rmax;
    return vec3<f32>(arr[0], arr[1], arr[2]);
}

// ── Separable blend functions B(Cs, Cd) ───────────────────────────────
fn blend_channel(mode: u32, cs: f32, cd: f32) -> f32 {
    if mode == 1u { // Multiply
        return cs * cd;
    } else if mode == 2u { // Screen
        return cs + cd - cs * cd;
    } else if mode == 3u { // Overlay
        if cd <= 0.5 { return 2.0 * cs * cd; }
        else { return 1.0 - 2.0 * (1.0 - cs) * (1.0 - cd); }
    } else if mode == 4u { // Darken
        return min(cs, cd);
    } else if mode == 5u { // Lighten
        return max(cs, cd);
    } else if mode == 6u { // ColorDodge
        if cd == 0.0 { return 0.0; }
        else if cs == 1.0 { return 1.0; }
        else { return min(1.0, cd / (1.0 - cs)); }
    } else if mode == 7u { // ColorBurn
        if cd == 1.0 { return 1.0; }
        else if cs == 0.0 { return 0.0; }
        else { return 1.0 - min(1.0, (1.0 - cd) / cs); }
    } else if mode == 8u { // HardLight — Overlay with Cs/Cd swapped
        if cs <= 0.5 { return 2.0 * cs * cd; }
        else { return 1.0 - 2.0 * (1.0 - cs) * (1.0 - cd); }
    } else if mode == 9u { // SoftLight
        if cs <= 0.5 {
            return cd - (1.0 - 2.0 * cs) * cd * (1.0 - cd);
        } else {
            var d: f32;
            if cd <= 0.25 {
                d = ((16.0 * cd - 12.0) * cd + 4.0) * cd;
            } else {
                d = sqrt(cd);
            }
            return cd + (2.0 * cs - 1.0) * (d - cd);
        }
    } else if mode == 10u { // Difference
        return abs(cd - cs);
    } else if mode == 11u { // Exclusion
        return cs + cd - 2.0 * cs * cd;
    } else if mode == 16u { // PlusLighter
        return min(1.0, cs + cd);
    }
    // Normal (0) or unknown — alpha-over handled by compositor formula
    return cs;
}

// ── CSS Compositing L1 §8 general compositing formula ─────────────────
// Co = αs × B(Cs, Cd) + αs × Cd × (1 - αd) + Cd × (1 - αs)
// αo = αs + αd × (1 - αs)
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let src = textureSample(t_src, s_layer, in.uv);
    let dst = textureSample(t_dst, s_layer, in.uv);
    let mode = u.mode;

    // Un-premultiply for blending (wgpu stores straight alpha in offscreen layers).
    var cs = src.rgb;
    var cd = dst.rgb;
    let as_ = src.a;
    let ad = dst.a;

    var blended: vec3<f32>;

    // Non-separable modes operate on full RGB vector.
    if mode == 12u { // Hue: hue of src, sat+lum of dst
        blended = set_lum(set_sat(cs, sat(cd)), lum(cd));
    } else if mode == 13u { // Saturation: sat of src, hue+lum of dst
        blended = set_lum(set_sat(cd, sat(cs)), lum(cd));
    } else if mode == 14u { // Color: hue+sat of src, lum of dst
        blended = set_lum(cs, lum(cd));
    } else if mode == 15u { // Luminosity: lum of src, hue+sat of dst
        blended = set_lum(cd, lum(cs));
    } else {
        // Separable modes — apply per channel.
        blended = vec3<f32>(
            blend_channel(mode, cs.r, cd.r),
            blend_channel(mode, cs.g, cd.g),
            blend_channel(mode, cs.b, cd.b),
        );
    }

    // Full CSS Compositing L1 §8 formula.
    let ao = as_ + ad * (1.0 - as_);
    let co = as_ * blended + as_ * cd * (1.0 - ad) + cd * (1.0 - as_);
    if ao <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(co, ao);
}
"#;

/// CSS Masking L1 §4 — mask composite shader.
/// Group 0: viewport uniform (shared with fill/image pipelines).
/// Group 1: t_layer (offscreen element content), t_mask (mask image), s_layer.
///
/// Fragment output: content_sample.rgba * mask_sample.alpha — mask-mode: alpha.
/// `pos` (pixel space) is converted to NDC the same way as fill/image shaders.
/// `uv_layer` = pos / viewport (auto-derived in vertex shader; not a separate attribute).
/// `uv_mask` = UV within the mask image tile (0..1 per tile instance).
const MASK_COMPOSITE_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@group(1) @binding(0) var t_layer: texture_2d<f32>;
@group(1) @binding(1) var t_mask:  texture_2d<f32>;
@group(1) @binding(2) var s_layer: sampler;

struct VIn {
    @location(0) pos:     vec2<f32>,
    @location(1) uv_mask: vec2<f32>,
};
struct VOut {
    @builtin(position) clip:     vec4<f32>,
    @location(0)       uv_layer: vec2<f32>,
    @location(1)       uv_mask:  vec2<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip     = vec4<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
        0.0, 1.0,
    );
    // uv_layer: sample the offscreen content layer at the same pixel position.
    o.uv_layer = in.pos / u.viewport;
    o.uv_mask  = in.uv_mask;
    return o;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_layer, s_layer, in.uv_layer);
    let m = textureSample(t_mask,  s_layer, in.uv_mask);
    // mask-mode: alpha — CSS Masking L1 §6.2 default for raster images.
    return vec4<f32>(c.rgb, c.a * m.a);
}
"#;

/// CSS Masking L1 §5 — mask-layer composite shader.
///
/// Two fragment entry points sharing one vertex shader:
/// - `fs_alpha`:  mask value = mask.a  (CSS mask-mode: alpha, default)
/// - `fs_luma`:   mask value = luma(mask.rgb) × mask.a  (mask-mode: luminance, ITU-R BT.709)
///
/// Group 0: viewport uniform. Group 1: { t_content, t_mask, s }.
/// `t_content` = scratch copy of the parent layer (element content saved before this pass).
/// `t_mask`    = the mask offscreen layer rendered between PushMaskLayer / PopMaskLayer.
///
/// Vertex: pos (CSS px, location 0) + uv (location 1, = pos/surface_size set at plan time).
/// Blend: REPLACE — overwrites parent layer at element rect without compositing.
/// This is correct because `t_content` already carries the full element alpha.
const MASK_LAYER_SHADER_SRC: &str = r#"
struct Uniforms { viewport: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var t_content: texture_2d<f32>;
@group(1) @binding(1) var t_mask:    texture_2d<f32>;
@group(1) @binding(2) var s:         sampler;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
        0.0, 1.0,
    );
    o.uv = in.uv;
    return o;
}

// mask-mode: alpha — use mask alpha channel directly (CSS Masking L1 §6.2).
@fragment fn fs_alpha(in: VOut) -> @location(0) vec4<f32> {
    let c  = textureSample(t_content, s, in.uv);
    let m  = textureSample(t_mask,    s, in.uv);
    let ma = m.a;
    return vec4<f32>(c.rgb * ma, c.a * ma);
}

// mask-mode: luminance — relative luminance × alpha (CSS Masking L1 §6.1, ITU-R BT.709).
@fragment fn fs_luma(in: VOut) -> @location(0) vec4<f32> {
    let c    = textureSample(t_content, s, in.uv);
    let m    = textureSample(t_mask,    s, in.uv);
    let luma = dot(m.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let ma   = luma * m.a;
    return vec4<f32>(c.rgb * ma, c.a * ma);
}
"#;

/// CSS Filter Effects Module L1 — color filter pipeline.
/// Bindings: 0=t_src (offscreen layer), 1=s_src (sampler), 2=FilterParams uniform.
/// Uses CompositeVertex layout. Blend: ALPHA_BLENDING (composites filtered element over parent).
/// Kind values: 1=Brightness, 2=Contrast, 3=Grayscale, 4=HueRotate(rad), 5=Invert,
/// 6=Opacity, 7=Saturate, 8=Sepia. Kind=0 (Blur) is handled by the blur shader, not here.
const FILTER_SHADER_SRC: &str = r#"
struct FilterEntry {
    kind: u32,
    amount: f32,
    _p0: u32,
    _p1: u32,
}
struct FilterParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    entries: array<FilterEntry, 8>,
}

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> u: FilterParams;

struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) alpha: f32 }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv = in.uv;
    return o;
}

fn apply_filter_fn(c: vec4<f32>, kind: u32, amount: f32) -> vec4<f32> {
    if kind == 1u { // Brightness
        return vec4<f32>(clamp(c.rgb * amount, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }
    if kind == 2u { // Contrast
        return vec4<f32>(clamp((c.rgb - 0.5) * amount + 0.5, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }
    if kind == 3u { // Grayscale
        let lum3 = vec3<f32>(dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
        return vec4<f32>(mix(c.rgb, lum3, amount), c.a);
    }
    if kind == 4u { // HueRotate (amount in radians)
        let cos_a = cos(amount);
        let sin_a = sin(amount);
        let r = dot(c.rgb, vec3<f32>(0.213+0.787*cos_a-0.213*sin_a, 0.715-0.715*cos_a-0.715*sin_a, 0.072-0.072*cos_a+0.928*sin_a));
        let g = dot(c.rgb, vec3<f32>(0.213-0.213*cos_a+0.143*sin_a, 0.715+0.285*cos_a+0.140*sin_a, 0.072-0.072*cos_a-0.283*sin_a));
        let b = dot(c.rgb, vec3<f32>(0.213-0.213*cos_a-0.787*sin_a, 0.715-0.715*cos_a+0.715*sin_a, 0.072+0.928*cos_a+0.072*sin_a));
        return vec4<f32>(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0), c.a);
    }
    if kind == 5u { // Invert
        return vec4<f32>(mix(c.rgb, 1.0 - c.rgb, amount), c.a);
    }
    if kind == 6u { // Opacity
        return vec4<f32>(c.rgb, c.a * amount);
    }
    if kind == 7u { // Saturate
        let r = dot(c.rgb, vec3<f32>(0.213+0.787*amount, 0.715-0.715*amount, 0.072-0.072*amount));
        let g = dot(c.rgb, vec3<f32>(0.213-0.213*amount, 0.715+0.285*amount, 0.072-0.072*amount));
        let b = dot(c.rgb, vec3<f32>(0.213-0.213*amount, 0.715-0.715*amount, 0.072+0.928*amount));
        return vec4<f32>(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0), c.a);
    }
    if kind == 8u { // Sepia
        let sr = clamp(dot(c.rgb, vec3<f32>(0.393, 0.769, 0.189)), 0.0, 1.0);
        let sg = clamp(dot(c.rgb, vec3<f32>(0.349, 0.686, 0.168)), 0.0, 1.0);
        let sb = clamp(dot(c.rgb, vec3<f32>(0.272, 0.534, 0.131)), 0.0, 1.0);
        return vec4<f32>(mix(c.rgb, vec3<f32>(sr, sg, sb), amount), c.a);
    }
    return c;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var c = textureSample(t_src, s_src, in.uv);
    for (var i = 0u; i < u.count; i = i + 1u) {
        c = apply_filter_fn(c, u.entries[i].kind, u.entries[i].amount);
    }
    return c;
}
"#;

/// CSS Filter Effects — separable Gaussian blur shader (one pass: H or V).
/// Bindings: 0=t_src, 1=s_src (linear sampler), 2=BlurParams uniform.
/// Uses CompositeVertex layout. Blend: REPLACE (intermediate buffer pass).
const BLUR_SHADER_SRC: &str = r#"
struct BlurParams {
    sigma: f32,
    direction: u32,   // 0 = horizontal, 1 = vertical
    _p0: u32,
    _p1: u32,
}

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> u: BlurParams;

struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) alpha: f32 }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv = in.uv;
    return o;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sigma = max(u.sigma, 0.001);
    let radius = min(i32(ceil(3.0 * sigma)), 32);
    let dim = vec2<f32>(textureDimensions(t_src));
    let step = select(vec2<f32>(1.0 / dim.x, 0.0), vec2<f32>(0.0, 1.0 / dim.y), u.direction == 1u);
    var sum = vec4<f32>(0.0);
    var weight_total = 0.0;
    for (var i = -radius; i <= radius; i = i + 1) {
        let fi = f32(i);
        let w = exp(-fi * fi / (2.0 * sigma * sigma));
        sum = sum + textureSample(t_src, s_src, in.uv + fi * step) * w;
        weight_total = weight_total + w;
    }
    return sum / weight_total;
}
"#;

/// CSS Images L3 §3.3 — GPU gradient pipeline shader (linear + radial).
///
/// Single shader module handles both kinds via `gp.kind` uniform (0=linear, 1=radial).
///
/// Group 0, binding 0: viewport uniform (shared with fill pipeline).
/// Group 1, binding 0: GradParams uniform — gradient line/center/stops.
///
/// Vertex layout (GradVertex): loc 0 = pos (CSS px), loc 1 = uv [0,1]×[0,1].
/// UV is baked into vertices as normalized rect coordinates; the fragment
/// shader uses UV directly without needing rect bounds in the uniform.
///
/// Linear gradient: p0=(sx,sy), p1=(ex,ey) are gradient-line endpoints
/// in UV space.  t = dot(uv-p0, p1-p0) / |p1-p0|²  (0 at start, 1 at end).
///
/// Radial gradient: p0=(cx,cy) is center in UV space; p1=(rx,ry) are
/// semi-axes (farthest-corner size) in UV space.
/// t = length((uv-p0)/p1)  (0 at center, 1 at ellipse edge).
///
/// Conic gradient (CSS Images L4 §3.7): p0=(cx,cy) is center in UV space;
/// p1=(w,h) is box size in CSS px (for box-space angle calculation);
/// `param0` is starting angle in radians (0 = top, clockwise).
/// t = (atan2(dx_box, -dy_box) - param0) / (2π), wrapped to [0,1].
const GRADIENT_SHADER_SRC: &str = r#"
struct ViewUniforms { viewport: vec2<f32> }
@group(0) @binding(0) var<uniform> vu: ViewUniforms;

struct GradStop {
    color: vec4<f32>,
    pos:   f32,
    _p0:   f32, _p1: f32, _p2: f32,
}
struct GradParams {
    p0:        vec2<f32>,
    p1:        vec2<f32>,
    n_stops:   u32,
    kind:      u32,
    repeating: u32,
    param0:    f32,
    stops: array<GradStop, 16>,
}
@group(1) @binding(0) var<uniform> gp: GradParams;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32> }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / vu.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / vu.viewport.y * 2.0,
    );
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}

fn sample_grad(t_in: f32) -> vec4<f32> {
    if gp.n_stops == 0u { return vec4<f32>(0.0); }
    var t = t_in;
    if gp.repeating != 0u {
        t = t - floor(t);
    } else {
        t = clamp(t, 0.0, 1.0);
    }
    if gp.n_stops == 1u { return gp.stops[0].color; }
    if t <= gp.stops[0].pos { return gp.stops[0].color; }
    let last = gp.n_stops - 1u;
    if t >= gp.stops[last].pos { return gp.stops[last].color; }
    for (var i = 0u; i + 1u < gp.n_stops; i = i + 1u) {
        let a = gp.stops[i];
        let b = gp.stops[i + 1u];
        if t >= a.pos && t <= b.pos {
            let span = b.pos - a.pos;
            let f = select(0.0, (t - a.pos) / span, span > 0.0001);
            return mix(a.color, b.color, f);
        }
    }
    return gp.stops[last].color;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var t: f32;
    if gp.kind == 0u {
        let d = gp.p1 - gp.p0;
        let len_sq = dot(d, d);
        t = select(0.0, dot(in.uv - gp.p0, d) / len_sq, len_sq > 0.0001);
    } else if gp.kind == 1u {
        let rel = (in.uv - gp.p0) / gp.p1;
        t = length(rel);
    } else {
        // Conic: convert UV offset back to box-space pixels so the polar
        // angle is computed in the box coordinate system (CSS spec).
        let dx = (in.uv.x - gp.p0.x) * gp.p1.x;
        let dy = (in.uv.y - gp.p0.y) * gp.p1.y;
        // CSS convention: 0° = top (-y), angles grow clockwise.
        // atan2(dx, -dy) gives the angle measured CW from -y axis.
        let two_pi = 6.2831853;
        let raw = atan2(dx, -dy) - gp.param0;
        let frac = raw / two_pi;
        let norm = frac - floor(frac);  // [0, 1) — one full revolution
        if gp.repeating != 0u && gp.n_stops > 1u {
            // Repeating conic (CSS Images L4 §3.7): stops tile within one
            // revolution such that consecutive iterations align edge-to-edge.
            let last = gp.n_stops - 1u;
            let span = gp.stops[last].pos - gp.stops[0].pos;
            if span > 0.0001 {
                let mod_s = norm - floor(norm / span) * span;
                t = gp.stops[0].pos + mod_s;
            } else {
                t = norm;
            }
        } else {
            t = norm;
        }
    }
    return sample_grad(t);
}
"#;

#[repr(C)]
#[derive(Copy, Clone)]
struct FillVertex {
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D elements;
    /// populated from `project_point_z` for 3D-transformed elements (CSS Transforms L2).
    /// Shader maps this to WebGPU NDC depth [0,1] so `CompareFunction::LessEqual` gives
    /// correct occlusion: closer elements (higher z) have lower depth value and win.
    z: f32,
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct TextVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D text;
    /// populated by `apply_affine_to_verts` via `VertexPos::set_depth` when the
    /// glyph quad is under a 3D CSS transform. Shader maps to WebGPU NDC depth
    /// via the same `0.5 - z/20000` formula as `FillVertex`, so depth testing
    /// is consistent across all vertex types in a `preserve-3d` rendering context.
    z: f32,
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D images;
    /// populated by `apply_affine_to_verts` for 3D-transformed image quads. Same
    /// NDC mapping as `FillVertex`/`TextVertex` for cross-type depth testing.
    z: f32,
    uv: [f32; 2],
    alpha: f32,
}

/// CSS Images L4 §4 — vertex for the two-texture `cross-fade` blend pipeline.
///
/// Layout (16 bytes): `pos[8] + uv[8]`. The quad covers the destination rect
/// with UVs spanning `[0,0]→[1,1]`; both textures are sampled at the same UV
/// (CSS Images L4 §4.1 — images are stretched to the destination, intrinsic
/// sizes do not participate in the blend). No depth field: the shader writes
/// a fixed mid-plane depth (0.5 NDC) and does not currently take part in
/// preserve-3d cross-type sorting.
#[repr(C)]
#[derive(Copy, Clone)]
struct CrossFadeVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// UV in `[0,1]×[0,1]` over the destination rect — applied to both
    /// `tex_a` and `tex_b` (CSS Images L4 §4.1: images stretched to fit dest).
    uv: [f32; 2],
}

/// Вершина для SDF-круга. `uv` — нормализованные координаты (-1..1) от центра
/// (quad расширен на 0.5px в каждую сторону). `radius_px` — CSS-радиус точки.
/// Layout: pos(8) + uv(8) + color(16) + radius_px(4) = 36 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct CircleVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// UV in [-1,1] over the expanded quad (CSS_radius + 0.5 in each direction).
    uv: [f32; 2],
    /// RGBA color.
    color: [f32; 4],
    /// CSS radius of the dot in pixels (= border_width / 2).
    radius_px: f32,
}

/// Вершина для SDF-скруглённого прямоугольника (`RRECT_SHADER_SRC`).
/// `center`/`half_size`/`radii_x`/`radii_y` одинаковы для всех 6 вершин одного quad-а
/// и передаются как interpolants (константны внутри одного треугольника).
/// Layout: pos(8) + z(4) + color(16) + center(8) + half_size(8) + radii_x(16) + radii_y(16) = 76 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct RRectVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D rrect;
    /// populated by `apply_affine_to_rrect_verts` for 3D-transformed quads.
    /// Same NDC mapping as `FillVertex` so border-radius backgrounds participate
    /// correctly in cross-type depth testing under CSS Transforms L2 `preserve-3d`.
    z: f32,
    /// RGBA color (linear premultiplied alpha is handled by blend state).
    color: [f32; 4],
    /// Center of the rounded rect in CSS pixels.
    center: [f32; 2],
    /// Half-dimensions of the rect: (width/2, height/2).
    half_size: [f32; 2],
    /// Horizontal corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 5.
    radii_x: [f32; 4],
    /// Vertical corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 6.
    /// Equal to `radii_x` for circular corners; differs for elliptical (`border-radius: H/V`).
    radii_y: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CompositeVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

/// CSS Masking L1 §4 — вершина mask-composite пайплайна.
/// `pos` — pixel-space (convert to NDC via viewport uniform).
/// `uv_mask` — UV [0,1]×[0,1] в пределах одной плитки mask-изображения.
/// `uv_layer` вычисляется в вершинном шейдере из `pos / viewport`.
#[repr(C)]
#[derive(Copy, Clone)]
struct MaskVertex {
    pos: [f32; 2],
    uv_mask: [f32; 2],
}

/// CPU-side зеркало WGSL `FilterEntry` (kind:u32, amount:f32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct FilterEntryCpu { kind: u32, amount: f32, _p0: u32, _p1: u32 }

/// CPU-side зеркало WGSL `FilterParams` (16 bytes header + 8×FilterEntry = 144 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct FilterParamsCpu {
    count: u32, _pad0: u32, _pad1: u32, _pad2: u32,
    entries: [FilterEntryCpu; 8],
}

/// CPU-side зеркало WGSL `BlurParams` (sigma:f32, direction:u32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct BlurParamsCpu { sigma: f32, direction: u32, _p0: u32, _p1: u32 }

/// CSS Images L3 §3.3 — вершина градиентного пайплайна.
/// `uv` — нормализованные координаты [0,1]×[0,1] внутри прямоугольника градиента,
/// бейкятся в вершины, чтобы фрагментный шейдер не нуждался в размерах rect в uniform.
#[repr(C)]
#[derive(Copy, Clone)]
struct GradVertex {
    /// CSS pixel position.
    pos: [f32; 2],
    /// Normalized rect coords: (0,0)=TL, (1,1)=BR.
    uv: [f32; 2],
}

/// CPU-side зеркало WGSL `GradStop` (color: vec4 + pos: f32 + 12 bytes pad = 32 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct GradStopCpu {
    color: [f32; 4],
    pos: f32,
    _p0: f32, _p1: f32, _p2: f32,
}

/// CPU-side зеркало WGSL `GradParams` (32 bytes header + 16×32 bytes stops = 544 bytes).
/// Используется как uniform buffer для одного DrawLinearGradient/DrawRadialGradient/DrawConicGradient.
#[repr(C)]
#[derive(Copy, Clone)]
struct GradParamsCpu {
    /// Linear: (sx, sy) — gradient-line start in UV [0,1].
    /// Radial: (cx, cy) — center in UV [0,1].
    /// Conic:  (cx, cy) — center in UV [0,1].
    p0: [f32; 2],
    /// Linear: (ex, ey) — gradient-line end (used with p0 start).
    /// Radial: (rx, ry) — farthest-corner semi-axes in UV [0,1].
    /// Conic:  (w, h) — box dimensions in CSS pixels (for box-space angle).
    p1: [f32; 2],
    n_stops: u32,
    /// 0 = linear, 1 = radial, 2 = conic.
    kind: u32,
    /// 0 = clamp, 1 = repeating (wrap t via fract).
    repeating: u32,
    /// Conic: starting angle in radians (0 = top, CW). Unused for linear/radial.
    param0: f32,
    stops: [GradStopCpu; 16],
}

/// Конвертирует `FilterFn` в `FilterEntryCpu` для GPU uniform.
/// Blur (kind=0) передаётся как is; color-filter pass пропускает его по kind.
fn filter_fn_to_entry(f: &FilterFn) -> FilterEntryCpu {
    let (kind, amount) = match f {
        FilterFn::Blur(v)       => (0u32, *v),
        FilterFn::Brightness(v) => (1,    *v),
        FilterFn::Contrast(v)   => (2,    *v),
        FilterFn::Grayscale(v)  => (3,    *v),
        FilterFn::HueRotate(v)  => (4,    *v),
        FilterFn::Invert(v)     => (5,    *v),
        FilterFn::Opacity(v)    => (6,    *v),
        FilterFn::Saturate(v)   => (7,    *v),
        FilterFn::Sepia(v)      => (8,    *v),
    };
    FilterEntryCpu { kind, amount, _p0: 0, _p1: 0 }
}

/// Атомарная команда render-pass-а после сборки display list-а. Каждый
/// DisplayCommand → один (рисующий) DrawOp; PushClipRect/PopClip → отдельные
/// SetScissor (если scissor реально меняется). Render-pass проходит список
/// линейно: SetScissor вызывает `pass.set_scissor_rect`, Fill/Text/Image
/// — соответствующий pipeline + draw на указанный диапазон вершин.
/// `image_batch_idx` индексирует `image_batches[i].bind_group` (Vec на
/// уровне render(), не клонируется в DrawOp).
enum DrawOp {
    SetScissor(DeviceScissor),
    Fill { v_start: u32, v_count: u32 },
    Circle { v_start: u32, v_count: u32 },
    /// SDF rounded-rect draw — uses `rrect_pipeline` + `rrect_vbuf`.
    RRect { v_start: u32, v_count: u32 },
    Text { v_start: u32, v_count: u32 },
    Image { v_start: u32, v_count: u32, image_batch_idx: u32 },
    /// CSS Images L3 §3.3 — linear or radial gradient quad. `grad_batch_idx`
    /// indexes into the per-frame `grad_bind_groups` Vec.
    Gradient { v_start: u32, v_count: u32, grad_batch_idx: u32 },
    /// CSS Images L4 §4 — `cross-fade(A, B, p)` two-texture blend quad.
    /// `cf_batch_idx` indexes into the per-frame `cross_fade_bind_groups` Vec
    /// (one bind group per command: holds both textures + sampler + progress
    /// uniform). Pipeline: `cross_fade_pipeline`.
    CrossFade { v_start: u32, v_count: u32, cf_batch_idx: u32 },
}

/// GPU-ресурсы для одной зарегистрированной картинки. Texture хранит уже
/// декодированные пиксели в формате `Rgba8Unorm` (Gray / GrayA / Rgb
/// конвертируются в Rgba при upload-е); bind group привязан к
/// `image_bind_group_layout` + общему sampler-у renderer-а. Intrinsic
/// dimensions (`width` / `height` в пикселях) хранятся для расчёта
/// `object-fit` / `object-position` на стадии рендеринга.
#[derive(Clone)]
struct GpuImage {
    /// Linear (bilinear) filtered bind group — default for auto/smooth.
    bind_group_linear: wgpu::BindGroup,
    /// Nearest-neighbor filtered bind group — used for pixelated/crisp-edges.
    bind_group_nearest: wgpu::BindGroup,
    /// Texture view (needed for mask-composite bind group creation in render loop).
    view: wgpu::TextureView,
    // texture держим как поле — wgpu освобождает GPU-память когда дропается
    // последняя ссылка; bind_group её не держит.
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

/// GPU-ресурсы одного off-screen opacity layer-а. Создаётся лениво через
/// `ensure_layer_textures`; переиспользуется пока размер surface не меняется.
/// `texture` хранится pub чтобы можно было использовать в
/// `encoder.copy_texture_to_texture` для blend-mode compositing.
pub struct OffscreenLayer {
    /// GPU texture resource.
    pub texture: wgpu::Texture,
    /// Texture view for rendering operations.
    pub view: wgpu::TextureView,
    /// Bind group for composite operations.
    pub bind_group: wgpu::BindGroup,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

/// GPU-снимок слоя, загруженный из CPU-пикселей через
/// `Renderer::upload_layer_snapshot`. Хранит `Rgba8Unorm`-текстуру
/// (COPY_DST | TEXTURE_BINDING) и bind group для `image_bgl`,
/// позволяя рендерить снимок через image-pipeline как позиционированный quad.
///
/// Bind group использует `image_bgl` (а не `composite_bgl`), чтобы
/// переиспользовать существующую image-pipeline с поддержкой rect/alpha.
struct GpuLayerSnapshot {
    // texture держим даже без явного обращения — wgpu освобождает GPU-память
    // когда дропается последняя ссылка; bind_group её не держит.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// Ошибка `Renderer::upload_layer_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotUploadError {
    /// `width == 0` или `height == 0`.
    EmptySnapshot,
    /// Стороны превышают `device.limits().max_texture_dimension_2d`.
    TooLarge { width: u32, height: u32, max: u32 },
    /// `pixels.len() != width * height * 4` (ожидается Rgba8, 4 байта/пиксель).
    InvalidDataSize { expected: usize, actual: usize },
}

impl core::fmt::Display for SnapshotUploadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptySnapshot => write!(f, "пустой снимок (width или height = 0)"),
            Self::TooLarge { width, height, max } => write!(
                f,
                "снимок {width}×{height} превышает предел GPU-текстуры {max}×{max}"
            ),
            Self::InvalidDataSize { expected, actual } => write!(
                f,
                "неверный размер данных снимка: ожидалось {expected} байт, получено {actual}"
            ),
        }
    }
}

impl std::error::Error for SnapshotUploadError {}

/// Ошибка `Renderer::register_image`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRegisterError {
    /// `width == 0` или `height == 0` — wgpu отклоняет такие текстуры
    /// на валидации. Декодер lumen-image тоже не должен такое отдавать
    /// (PNG/JPEG запрещают нулевые размеры), но на всякий случай ловим.
    EmptyImage,
    /// Размер изображения превышает `device.limits().max_texture_dimension_2d`
    /// (на downlevel_defaults — 2048).
    TooLarge {
        width: u32,
        height: u32,
        max: u32,
    },
}

impl core::fmt::Display for ImageRegisterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyImage => write!(f, "пустое изображение (width или height = 0)"),
            Self::TooLarge { width, height, max } => write!(
                f,
                "изображение {width}×{height} превышает предел GPU-текстуры {max}×{max}"
            ),
        }
    }
}

impl std::error::Error for ImageRegisterError {}

/// Закешированная информация о глифе: позиция в атласе + метрики.
///
/// `left` / `top` — в пикселях растеризации (т.е. на размер bin-а из
/// `SIZE_BINS`); сюда влияют только параметры растеризации, не итоговый
/// display-размер. `advance_native` — в font units (`hmtx.advance_width`),
/// масштаб по `font_size / units_per_em` применяется на стороне caller-а.
#[derive(Clone, Copy)]
struct CachedGlyph {
    entry: GlyphEntry,
    left: f32,
    top: f32,
    advance_native: u16,
}

/// Один загруженный face: TTF-байты (parsed on-demand через `Font::parse`).
/// face_id 0 — default (bundled, передан в `Renderer::new`); остальные
/// `face_id` назначаются по мере lazy-загрузки из путей `FaceRecord`.
struct LoadedFace {
    bytes: Vec<u8>,
}

/// Распарсенный face для одного `render()`-вызова: Font + ключевые таблицы.
/// Borrow от `LoadedFace.bytes`.
///
/// Используется в codepoint-cascade: per-char проверяем `cmap.glyph_index`
/// у каждого face-а и выбираем тот, где глиф найден. Rasterizer создаётся
/// per-DrawText (см. `push_text_glyphs`) — size_bin зависит от font-size,
/// который варьируется по командам, а не по face-ам.
struct ParsedFace<'a> {
    font: Font<'a>,
    head: Head,
    hhea: Hhea,
    cmap: Cmap<'a>,
    hmtx: Hmtx<'a>,
}

pub struct Renderer {
    /// Windowed surface; `None` in headless mode (created with `new_headless()`).
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Surface configuration; `None` in headless mode.
    config: Option<wgpu::SurfaceConfiguration>,
    /// Width in physical pixels when headless (`surface = None`); 0 otherwise.
    headless_w: u32,
    /// Height in physical pixels when headless (`surface = None`); 0 otherwise.
    headless_h: u32,
    /// Device-pixel-ratio от winit (`Window::scale_factor`). Surface
    /// сконфигурирован в physical pixels (`config.width/height`), но shader
    /// делит позицию вершины на logical viewport (`config / scale_factor`),
    /// чтобы 1 CSS pixel = `scale_factor` device pixels — корректное
    /// масштабирование на HiDPI без правки display list-а.
    /// Обновляется через [`Renderer::set_scale_factor`] при `ScaleFactorChanged`
    /// событии winit (например, drag окна между мониторами с разной DPI).
    scale_factor: f64,

    /// GPU depth buffer for CSS 3D transforms (`transform-style: preserve-3d`).
    /// Size matches the frame surface; recreated on every `resize()`.
    /// `None` only when both dimensions are zero at construction time.
    // CSS: transform-style — when P4 wires preserve-3d, depth_sorted_child_order()
    // in display_list.rs emits commands back-to-front; the GPU depth test here
    // provides correct occlusion for the rare case of intersecting 3D planes.
    depth_texture: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,

    fill_pipeline: wgpu::RenderPipeline,
    circle_pipeline: wgpu::RenderPipeline,
    /// CSS border-radius SDF pipeline. Uses `RRectVertex` layout.
    rrect_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    /// CSS Images L4 §4 — `cross-fade(A, B, p)` two-texture blend pipeline.
    /// Uses `CrossFadeVertex` layout (pos+uv). Bind group 0 = viewport uniform
    /// (shared with `image_pipeline`); bind group 1 = `cross_fade_bgl`
    /// (tex_a, tex_b, sampler, progress uniform). Blend state: `ALPHA_BLENDING`.
    cross_fade_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the `cross_fade_pipeline` per-quad bindings
    /// (group 1): two textures + sampler + progress uniform.
    cross_fade_bgl: wgpu::BindGroupLayout,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bgl: wgpu::BindGroupLayout,
    blend_pipeline: wgpu::RenderPipeline,
    blend_bgl: wgpu::BindGroupLayout,
    blend_mode_uniform: wgpu::Buffer,
    /// CSS Masking L1 §4 — mask composite pipeline + bind group layout.
    /// Used by PopMask to composite the offscreen layer using a mask image.
    mask_composite_bgl: wgpu::BindGroupLayout,
    mask_composite_pipeline: wgpu::RenderPipeline,
    /// CSS Masking L1 §5 — mask-layer composite pipelines.
    /// Used by PopMaskLayer to apply an arbitrary rendered mask to the parent layer.
    /// `_alpha` samples mask.a; `_luma` converts RGB to luminance × alpha.
    /// Shared BGL with mask_composite (same binding layout: t_content, t_mask, s).
    mask_layer_alpha_pipeline: wgpu::RenderPipeline,
    mask_layer_luma_pipeline: wgpu::RenderPipeline,
    /// CSS Filter Effects L1 — color filter pipeline (grayscale/sepia/brightness/etc.).
    filter_bgl: wgpu::BindGroupLayout,
    filter_pipeline: wgpu::RenderPipeline,
    /// CSS Filter Effects L1 — separable Gaussian blur pipeline (one pass: H or V).
    blur_bgl: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::RenderPipeline,
    blur_uniform: wgpu::Buffer,
    /// CSS Filter Effects L1 §2 — backdrop-filter blit pipeline.
    /// Same shader as `filter_pipeline` but uses REPLACE blend so the filtered
    /// backdrop snapshot overwrites (not composites over) the parent layer at
    /// the bounded element rect.
    backdrop_blit_pipeline: wgpu::RenderPipeline,
    /// Intermediate texture for backdrop-filter: ping-pong target for blur passes
    /// (H: scratch → backdrop_layer; V: backdrop_layer → scratch), and color-filter
    /// target when compositing filtered backdrop back onto parent.
    backdrop_layer: Option<OffscreenLayer>,
    /// CSS Filter Effects L1 §2 — `backdrop-filter` result cache (metadata).
    /// Tracks, per backdrop element ordinal, the content hash of the inputs that
    /// produced the cached filtered texture. Used to skip the blur passes when a
    /// frame's backdrop inputs are unchanged from the previous frame.
    backdrop_cache: crate::backdrop_cache::BackdropCache,
    /// Cached filtered backdrop textures, keyed by the same ordinal as
    /// [`Self::backdrop_cache`]. Each is a full parent-layer-sized snapshot of
    /// the blurred (or, for filter-only backdrops, copied) backdrop region.
    /// Reused across frames on a cache hit; the color-filter pass still runs at
    /// blit time so only the expensive blur is skipped.
    backdrop_cache_textures: HashMap<u32, OffscreenLayer>,
    /// CSS Images L3 §3.3 — linear/radial gradient pipeline.
    gradient_bgl: wgpu::BindGroupLayout,
    gradient_pipeline: wgpu::RenderPipeline,
    scratch_layer: Option<OffscreenLayer>,
    layer_sampler: wgpu::Sampler,
    layer_textures: Vec<OffscreenLayer>,
    surface_format: wgpu::TextureFormat,

    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    atlas_texture: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup,

    image_bgl: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    image_sampler_nearest: wgpu::Sampler,
    /// Декодированные изображения в CPU-памяти. Хранятся для on-demand
    /// ресайза под конкретный layout-размер (CPU bilinear resize).
    raw_images: HashMap<String, Image>,
    /// Cache GPU-текстур: ключ `"src"` (оригинал) или `"src@WxH"` (ресайз).
    /// Заполняется через [`Renderer::register_image`] и лениво при DrawImage.
    images: HashMap<String, GpuImage>,
    /// Cache GPU-снимков слоёв per-id. Заполняется compositor-ом через
    /// [`Renderer::upload_layer_snapshot`] для кеширования неизменных слоёв.
    layer_snapshots: HashMap<u64, GpuLayerSnapshot>,
    /// GPU layer cache with LRU eviction (ADR-008 Phase 2).
    /// Tracks layer textures by stacking context ID + size for off-viewport eviction.
    layer_cache: crate::layer_cache::LayerCache,

    atlas: GlyphAtlas,
    /// Загруженные face-ы. `faces[0]` — default (bundled), используется когда
    /// `font-family` пуст или ни одно имя не нашлось через `FontProvider`.
    /// Остальные добавляются лениво при первом `DrawText` с известной family.
    faces: Vec<LoadedFace>,
    /// `face_id` по абсолютному пути TTF — чтобы не грузить файл повторно.
    face_id_by_path: HashMap<PathBuf, usize>,
    /// Источник лукапа face-ов по `(family, weight, style)`. По умолчанию —
    /// `SystemFontIndex`, который лениво сканирует системные font-директории.
    /// `None` означает «без resolver-а — всегда default face» (для тестов /
    /// headless-режимов).
    font_provider: Option<Arc<dyn FontProvider>>,
    /// Кэш растеризованных глифов: ключ `(face_id, glyph_id, size_bin)`.
    /// `face_id` — глифы у разных face-ов имеют разный glyph_id; `size_bin`
    /// — multi-size atlas (см. `SIZE_BINS`): один и тот же глиф для
    /// font-size 16 и 32 даёт две разные записи (разная растеризация,
    /// разный atlas-rect).
    cached_glyphs: HashMap<AtlasKey, Option<CachedGlyph>>,
    /// In headless mode: the `RENDER_ATTACHMENT | COPY_SRC` texture rendered to
    /// by the most recent `render()` call. Kept alive between `render()` and
    /// `render_to_image()` pixel readback, then dropped.
    pending_readback: Option<wgpu::Texture>,
    /// GPU texture pool for layer recycling (ADR-008 Phase 2).
    /// Maintains free textures keyed by (width, height) for reuse instead of
    /// allocating a new `wgpu::Texture` for each layer.
    texture_pool: crate::texture_pool::TexturePool,
    /// Normalized GPU fingerprint: prevents WebGL renderer/vendor fingerprinting (ADR-007).
    gpu_fingerprint: GpuFingerprint,
}

/// Creates a `Depth32Float` texture + view sized `width×height` for GPU depth testing.
/// Called once in `init_pipelines` and on every `resize`.
fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth-texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

impl Renderer {
    pub fn new(window: Arc<Window>, font_bytes: Vec<u8>) -> Result<Self, Box<dyn Error>> {
        // Валидируем шрифт сразу, чтобы при битом файле не падать в первом кадре.
        Font::parse(&font_bytes).map_err(|e| format!("парсинг шрифта: {e}"))?;
        block_on(Self::new_async(window, font_bytes))
    }

    async fn new_async(
        window: Arc<Window>,
        font_bytes: Vec<u8>,
    ) -> Result<Self, Box<dyn Error>> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        // winit отдаёт inner_size в physical pixels; surface конфигурируем
        // в physical (config.width/height), но viewport uniform в render()
        // делится на scale_factor — это даёт CSS-px координаты в shader-е.
        // Изначальный scale_factor от текущего монитора; обновляется при
        // ScaleFactorChanged-event-е через `set_scale_factor`.
        let scale_factor = window.scale_factor();

        // BUG-057: on Windows the Vulkan backend causes a double-panic on the first
        // rendered frame (encoder invalidated, then Surface drop races SurfaceTexture).
        // DX12 does not exhibit this issue. Default to DX12 on Windows; allow the
        // WGPU_BACKEND env-var to override for debugging / fallback.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: if cfg!(target_os = "windows") {
                wgpu::Backends::DX12
            } else {
                wgpu::Backends::PRIMARY
            },
            ..Default::default()
        }
        .with_env());
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lumen-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let adapter_info = adapter.get_info();
        let gpu_fingerprint = GpuFingerprint::from_adapter_info(&adapter_info);

        Self::init_pipelines(
            device,
            queue,
            format,
            font_bytes,
            Some(surface),
            Some(config),
            0,
            0,
            scale_factor,
            gpu_fingerprint,
        )
    }

    /// Creates a headless `Renderer` for off-screen rendering without a winit window.
    /// Uses wgpu without a surface; renders to an internal `Rgba8Unorm` texture.
    /// Call [`render_to_image`](Self::render_to_image) to get pixels after rendering.
    ///
    /// # Errors
    /// Returns `Err` if no GPU adapter is available or device creation fails.
    pub fn new_headless(font_bytes: Vec<u8>, width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error>> {
        Font::parse(&font_bytes).map_err(|e| format!("парсинг шрифта: {e}"))?;
        block_on(Self::new_headless_async(font_bytes, width, height))
    }

    async fn new_headless_async(
        font_bytes: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Mirror the windowed-mode backend choice (BUG-057).
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: if cfg!(target_os = "windows") {
                wgpu::Backends::DX12
            } else {
                wgpu::Backends::PRIMARY
            },
            ..Default::default()
        }
        .with_env());
        // No surface needed — request adapter without compatible_surface constraint.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lumen-headless-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        // Use Rgba8Unorm: no surface capability query needed, widely supported,
        // and matches lumen_image::PixelFormat::Rgba8 for zero-copy readback.
        let format = wgpu::TextureFormat::Rgba8Unorm;

        let adapter_info = adapter.get_info();
        let gpu_fingerprint = GpuFingerprint::from_adapter_info(&adapter_info);

        Self::init_pipelines(
            device,
            queue,
            format,
            font_bytes,
            None,
            None,
            width.max(1),
            height.max(1),
            1.0,
            gpu_fingerprint,
        )
    }

    /// Общий инициализатор пайплайнов: создаёт все GPU-ресурсы (шейдеры, pipeline-ы,
    /// atlas, samplers). Вызывается как из windowed (`new_async`), так и из headless
    /// (`new_headless_async`) путей.
    #[allow(clippy::too_many_arguments)]
    fn init_pipelines(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        font_bytes: Vec<u8>,
        surface: Option<wgpu::Surface<'static>>,
        config: Option<wgpu::SurfaceConfiguration>,
        headless_w: u32,
        headless_h: u32,
        scale_factor: f64,
        gpu_fingerprint: GpuFingerprint,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // ── Uniform bind group (viewport) — общий для fill и text ──────────
        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform-buf"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform-bg"),
            layout: &uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // ── Atlas texture + sampler + bind group ───────────────────────────
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_DIM,
                height: ATLAS_DIM,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas-bg"),
            layout: &atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        // ── Fill pipeline ─────────────────────────────────────────────────
        let fill_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fill-shader"),
            source: wgpu::ShaderSource::Wgsl(FILL_SHADER_SRC.into()),
        });
        let fill_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fill-layout"),
            bind_group_layouts: &[&uniform_bgl],
            push_constant_ranges: &[],
        });
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fill-pipeline"),
            layout: Some(&fill_layout),
            vertex: wgpu::VertexState {
                module: &fill_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<FillVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0, // pos
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 1, // z (CSS depth px)
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 12,
                            shader_location: 2, // color
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fill_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            // CSS Transforms L2 §6 — depth test for preserve-3d rendering contexts.
            // LessEqual: closer elements (smaller depth) win; equal depth preserves
            // painter's order (last-drawn wins), matching the 2D flat-compositing path.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Circle pipeline ───────────────────────────────────────────────
        let circle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("circle-shader"),
            source: wgpu::ShaderSource::Wgsl(CIRCLE_SHADER_SRC.into()),
        });
        let circle_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("circle-layout"),
            bind_group_layouts: &[&uniform_bgl],
            push_constant_ranges: &[],
        });
        let circle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("circle-pipeline"),
            layout: Some(&circle_layout),
            vertex: wgpu::VertexState {
                module: &circle_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CircleVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 32,
                            shader_location: 3,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &circle_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── RRect (SDF rounded-rect) pipeline ─────────────────────────────
        let rrect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rrect-shader"),
            source: wgpu::ShaderSource::Wgsl(RRECT_SHADER_SRC.into()),
        });
        let rrect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rrect-layout"),
            bind_group_layouts: &[&uniform_bgl],
            push_constant_ranges: &[],
        });
        let rrect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rrect-pipeline"),
            layout: Some(&rrect_layout),
            vertex: wgpu::VertexState {
                module: &rrect_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RRectVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // loc 0: pos (vec2)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        // loc 1: z (f32, CSS depth px)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 1,
                        },
                        // loc 2: color (vec4)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 12,
                            shader_location: 2,
                        },
                        // loc 3: center (vec2)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 28,
                            shader_location: 3,
                        },
                        // loc 4: half_size (vec2)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 36,
                            shader_location: 4,
                        },
                        // loc 5: radii_x (vec4: horizontal tl, tr, br, bl)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 44,
                            shader_location: 5,
                        },
                        // loc 6: radii_y (vec4: vertical tl, tr, br, bl)
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 60,
                            shader_location: 6,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &rrect_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            // CSS Transforms L2 §6 — SDF rounded rects participate in 3D depth
            // testing under preserve-3d. LessEqual matches FillVertex pipeline so
            // border-radius backgrounds occlude correctly under 3D transforms.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Text pipeline ─────────────────────────────────────────────────
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text-shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER_SRC.into()),
        });
        let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text-layout"),
            bind_group_layouts: &[&uniform_bgl, &atlas_bgl],
            push_constant_ranges: &[],
        });
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text-pipeline"),
            layout: Some(&text_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0, // pos
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 1, // z (CSS depth px)
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 12,
                            shader_location: 2, // uv
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 20,
                            shader_location: 3, // color
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            // CSS Transforms L2 §6 — text participates in 3D depth testing under
            // preserve-3d. LessEqual matches FillVertex pipeline so 3D-transformed
            // text occludes/is occluded by background rects consistently.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Image pipeline (RGBA texture-quad, per-image bind group) ──────
        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image-sampler-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let image_sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image-sampler-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image-shader"),
            source: wgpu::ShaderSource::Wgsl(IMAGE_SHADER_SRC.into()),
        });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image-layout"),
            bind_group_layouts: &[&uniform_bgl, &image_bgl],
            push_constant_ranges: &[],
        });
        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image-pipeline"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImageVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0, // pos
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 8,
                            shader_location: 1, // z (CSS depth px)
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 12,
                            shader_location: 2, // uv
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 20,
                            shader_location: 3, // alpha
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            // CSS Transforms L2 §6 — image quads participate in 3D depth testing
            // under preserve-3d. LessEqual matches FillVertex/TextVertex pipelines.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Cross-fade pipeline (CSS Images L4 §4: mix(A, B, p)) ──────────
        // BGL group 1 — two textures + sampler + progress uniform.
        let cross_fade_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cross-fade-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let cross_fade_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cross-fade-shader"),
            source: wgpu::ShaderSource::Wgsl(CROSS_FADE_SHADER_SRC.into()),
        });
        let cross_fade_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cross-fade-layout"),
            bind_group_layouts: &[&uniform_bgl, &cross_fade_bgl],
            push_constant_ranges: &[],
        });
        let cross_fade_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cross-fade-pipeline"),
            layout: Some(&cross_fade_layout),
            vertex: wgpu::VertexState {
                module: &cross_fade_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CrossFadeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0, // pos
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1, // uv
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &cross_fade_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Same blend as image_pipeline — straight-alpha source,
                    // SrcAlpha · src + (1-SrcAlpha) · dst.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            // Cross-fade quads run at fixed mid-plane depth (z = 0.5 NDC in
            // shader) — depth_write_enabled = false so they do not occlude
            // 3D-transformed siblings under preserve-3d.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Composite pipeline (opacity layer → parent target) ────────────
        let composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layer_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite-shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER_SRC.into()),
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite-layout"),
            bind_group_layouts: &[&composite_bgl],
            push_constant_ranges: &[],
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite-pipeline"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied-alpha blend: off-screen layers store premultiplied content.
                    // Shader multiplies rgb*opacity so "one * src + (1-src.a) * dst" is correct.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Blend pipeline (CSS Compositing L1 §8 — two-texture blend) ─────
        // 4 bindings: t_src(0), t_dst(1), sampler(2), blend_mode uniform(3).
        let blend_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blend-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let blend_mode_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blend-mode-uniform"),
            size: 16, // u32 mode + 3 × u32 padding = 16 bytes
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blend-shader"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER_SRC.into()),
        });
        let blend_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blend-layout"),
            bind_group_layouts: &[&blend_bgl],
            push_constant_ranges: &[],
        });
        // REPLACE blend state: shader implements full CSS compositing formula.
        let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blend-pipeline"),
            layout: Some(&blend_layout),
            vertex: wgpu::VertexState {
                module: &blend_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blend_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Mask composite pipeline ──────────────────────────────────────────
        // CSS Masking L1 §4: two-texture composite (content layer + mask image).
        // Group 0 = viewport uniform (reuses uniform_bgl).
        // Group 1 = { t_layer, t_mask, s_layer }.
        let mask_composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mask-composite-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mask_composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask-composite-layout"),
            bind_group_layouts: &[&uniform_bgl, &mask_composite_bgl],
            push_constant_ranges: &[],
        });
        let mask_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mask-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(MASK_COMPOSITE_SHADER_SRC.into()),
        });
        let mask_composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask-composite-pipeline"),
            layout: Some(&mask_composite_layout),
            vertex: wgpu::VertexState {
                module: &mask_composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MaskVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Mask-layer composite pipelines ──────────────────────────────────
        // CSS Masking L1 §5: apply a rendered mask layer to the parent layer.
        // Reuses mask_composite_bgl (same binding layout: t_content, t_mask, s).
        // Two pipelines sharing one shader module: alpha mode and luminance mode.
        // Blend: REPLACE (src_factor=One, dst_factor=Zero) — overwrites parent at element rect.
        let mask_layer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mask-layer-shader"),
            source: wgpu::ShaderSource::Wgsl(MASK_LAYER_SHADER_SRC.into()),
        });
        let mask_layer_vtx_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MaskVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
            ],
        };
        let replace_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let mask_layer_alpha_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask-layer-alpha-pipeline"),
            layout: Some(&mask_composite_layout),
            vertex: wgpu::VertexState {
                module: &mask_layer_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&mask_layer_vtx_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_layer_shader,
                entry_point: Some("fs_alpha"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(replace_blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let mask_layer_luma_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask-layer-luma-pipeline"),
            layout: Some(&mask_composite_layout),
            vertex: wgpu::VertexState {
                module: &mask_layer_shader,
                entry_point: Some("vs_main"),
                buffers: &[mask_layer_vtx_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_layer_shader,
                entry_point: Some("fs_luma"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(replace_blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── CSS Filter pipeline ──────────────────────────────────────────────
        // Group 0: { t_src, s_src, FilterParams uniform }
        let filter_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("filter-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let filter_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter-shader"),
            source: wgpu::ShaderSource::Wgsl(FILTER_SHADER_SRC.into()),
        });
        let filter_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("filter-layout"),
            bind_group_layouts: &[&filter_bgl],
            push_constant_ranges: &[],
        });
        let filter_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("filter-pipeline"),
            layout: Some(&filter_layout),
            vertex: wgpu::VertexState {
                module: &filter_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 16, shader_location: 2 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &filter_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── CSS Blur pipeline ────────────────────────────────────────────────
        // Group 0: { t_src, s_src, BlurParams uniform }
        let blur_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let blur_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur-uniform"),
            size: std::mem::size_of::<BlurParamsCpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur-shader"),
            source: wgpu::ShaderSource::Wgsl(BLUR_SHADER_SRC.into()),
        });
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur-layout"),
            bind_group_layouts: &[&blur_bgl],
            push_constant_ranges: &[],
        });
        let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur-pipeline"),
            layout: Some(&blur_layout),
            vertex: wgpu::VertexState {
                module: &blur_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 16, shader_location: 2 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blur_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Backdrop-filter blit pipeline ────────────────────────────────────
        // Same shader + bind group layout as filter_pipeline, but REPLACE blend.
        // Used to overwrite the parent layer's element-bounds region with the
        // filtered backdrop snapshot (with optional color-matrix filter applied).
        let backdrop_blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("backdrop-blit-pipeline"),
            layout: Some(&filter_layout),
            vertex: wgpu::VertexState {
                module: &filter_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 16, shader_location: 2 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &filter_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ── Gradient pipeline (linear + radial) ──────────────────────────────
        let gradient_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gradient-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let gradient_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gradient-shader"),
            source: wgpu::ShaderSource::Wgsl(GRADIENT_SHADER_SRC.into()),
        });
        let gradient_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gradient-layout"),
            bind_group_layouts: &[&uniform_bgl, &gradient_bgl],
            push_constant_ranges: &[],
        });
        let gradient_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gradient-pipeline"),
            layout: Some(&gradient_layout),
            vertex: wgpu::VertexState {
                module: &gradient_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GradVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &gradient_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let atlas = GlyphAtlas::new(ATLAS_DIM);

        let (depth_texture, depth_view) = {
            let (t, v) = create_depth_texture(&device, headless_w, headless_h);
            (Some(t), Some(v))
        };

        Ok(Self {
            surface,
            device,
            queue,
            config,
            headless_w,
            headless_h,
            scale_factor,
            depth_texture,
            depth_view,
            fill_pipeline,
            circle_pipeline,
            rrect_pipeline,
            text_pipeline,
            image_pipeline,
            cross_fade_pipeline,
            cross_fade_bgl,
            uniform_buffer,
            uniform_bind_group,
            atlas_texture,
            atlas_bind_group,
            image_bgl,
            image_sampler,
            image_sampler_nearest,
            raw_images: HashMap::new(),
            images: HashMap::new(),
            layer_snapshots: HashMap::new(),
            layer_cache: crate::layer_cache::LayerCache::new(),
            composite_pipeline,
            composite_bgl,
            blend_pipeline,
            blend_bgl,
            blend_mode_uniform,
            mask_composite_bgl,
            mask_composite_pipeline,
            mask_layer_alpha_pipeline,
            mask_layer_luma_pipeline,
            filter_bgl,
            filter_pipeline,
            blur_bgl,
            blur_pipeline,
            blur_uniform,
            backdrop_blit_pipeline,
            backdrop_layer: None,
            backdrop_cache: crate::backdrop_cache::BackdropCache::new(),
            backdrop_cache_textures: HashMap::new(),
            gradient_bgl,
            gradient_pipeline,
            scratch_layer: None,
            layer_sampler,
            layer_textures: Vec::new(),
            surface_format: format,
            atlas,
            faces: vec![LoadedFace { bytes: font_bytes }],
            face_id_by_path: HashMap::new(),
            font_provider: Some(Arc::new(SystemFontIndex::new())),
            cached_glyphs: HashMap::new(),
            pending_readback: None,
            texture_pool: crate::texture_pool::TexturePool::new(),
            gpu_fingerprint,
        })
    }

    /// Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
    /// headless-режимов (отключить системный скан). `None` отключает поиск —
    /// рендер всегда использует default face.
    #[must_use]
    pub fn with_font_provider(mut self, provider: Option<Arc<dyn FontProvider>>) -> Self {
        self.font_provider = provider;
        self
    }

    /// Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
    /// чтобы передать `FontRegistry` с @font-face шрифтами после загрузки
    /// страницы (Renderer уже создан, builder-паттерн недоступен).
    pub fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.font_provider = provider;
    }

    /// Эагерно загружает указанные family-имена через текущий `FontProvider`,
    /// чтобы они были доступны для codepoint cascade ещё до первого `DrawText`
    /// с этой family-ой в CSS. Используется shell-ом для прогрева
    /// fallback-цепочки (Noto Color Emoji / Noto Sans CJK / etc.), без
    /// которой эмодзи и CJK на странице без явного `font-family` падают
    /// в `.notdef`. Имена, не найденные в провайдере или с битым TTF, тихо
    /// пропускаются. Берётся weight=400 + style=normal — для fallback-целей
    /// этого достаточно. Идемпотентно: повторный вызов на уже загруженной
    /// family не делает работы благодаря `face_id_by_path` cache-у.
    pub fn preload_fallback_chain(&mut self, families: &[&str]) {
        for name in families {
            let _ = self.resolve_face_id(
                &[(*name).to_string()],
                FontWeight::NORMAL,
                FontStyle::Normal,
            );
        }
    }

    /// Returns the normalized GPU fingerprint (vendor/renderer strings).
    ///
    /// Returns ("WebKit", "Generic GPU") regardless of actual adapter to prevent
    /// WebGL fingerprinting attacks (ADR-007 Layer 4).
    pub fn gpu_fingerprint(&self) -> &GpuFingerprint {
        &self.gpu_fingerprint
    }

    /// Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
    /// Emoji / Noto Sans CJK / Apple Color Emoji / Segoe UI Emoji /
    /// PingFang / Hiragino / Microsoft YaHei / Yu Gothic / Malgun Gothic /
    /// Noto Sans Arabic / Hebrew / Devanagari / Thai). На каждой ОС
    /// найдётся лишь часть имён — остальные тихо пропустятся. Это
    /// разблокирует codepoint-cascade для эмодзи / CJK / RTL / Indic /
    /// Thai на страницах **без явного CSS `font-family`** для этих
    /// скриптов. Вызывается shell-ом один раз после `Renderer::new_async`.
    /// Идемпотентен (preload_fallback_chain → resolve_face_id cache).
    pub fn preload_curated_fallbacks(&mut self) {
        self.preload_fallback_chain(crate::fallback::CURATED_FALLBACK_FAMILIES);
    }

    /// Резолвит `face_id` для `DrawText` с указанным `font-family` списком.
    /// Если `font_provider` есть — перебирает имена в порядке приоритета
    /// (CSS Fonts L4 §3.1), для первого найденного через `pick_face` — лениво
    /// загружает TTF и возвращает `face_id`. Generic CSS-family-ы
    /// (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/`system-ui`)
    /// пропускаются — Phase 0 не имеет per-generic-fallback таблицы; в
    /// конечном итоге падают в default. Если ни одно имя не найдено —
    /// возвращает 0 (default face).
    fn resolve_face_id(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
    ) -> usize {
        let Some(provider) = self.font_provider.clone() else {
            return 0;
        };
        for fam in families {
            let lc = fam.to_lowercase();
            if matches!(
                lc.as_str(),
                "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
            ) {
                continue;
            }
            let css_style = match style {
                FontStyle::Normal => CssFontStyle::Normal,
                FontStyle::Italic => CssFontStyle::Italic,
                FontStyle::Oblique => CssFontStyle::Oblique,
            };
            let Some(rec) = provider.pick_face(fam, weight.0, css_style) else {
                continue;
            };
            if let Some(&id) = self.face_id_by_path.get(&rec.path) {
                return id;
            }
            // @font-face in-memory байты (virtual path) или диск для системных шрифтов.
            let raw = if let Some(mem_bytes) = provider.read_face_bytes(&rec.path) {
                mem_bytes
            } else {
                let Ok(disk_bytes) = std::fs::read(&rec.path) else {
                    continue;
                };
                disk_bytes
            };
            // Transparent WOFF/WOFF2 → sfnt conversion before parsing.
            let bytes = match maybe_decode_font(&raw) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => raw,
                Err(e) => {
                    eprintln!("[font] WOFF decode failed {}: {e}", rec.path.display());
                    continue;
                }
            };
            if let Err(e) = Font::parse(&bytes) {
                eprintln!("[font] parse failed {}: {e}", rec.path.display());
                continue;
            }
            let id = self.faces.len();
            self.faces.push(LoadedFace { bytes });
            self.face_id_by_path.insert(rec.path, id);
            return id;
        }
        0
    }

    /// Регистрирует декодированное изображение в GPU-cache под ключом `src`.
    /// Если ключ уже был — старая запись (и её GPU-texture) заменяется.
    ///
    /// Изображение конвертируется в `Rgba8Unorm` (Gray → серый × 3 + alpha 255,
    /// GrayA → серый × 3 + alpha из канала, Rgb → opaque, Rgba → как есть).
    /// Color management в Phase 0 не делается — sRGB-coded байты идут «как есть».
    ///
    /// # Errors
    /// - [`ImageRegisterError::EmptyImage`] при `width == 0 || height == 0`.
    /// - [`ImageRegisterError::TooLarge`] если стороны превышают
    ///   `device.limits().max_texture_dimension_2d`.
    pub fn register_image(
        &mut self,
        src: String,
        image: &Image,
    ) -> Result<(), ImageRegisterError> {
        if image.width == 0 || image.height == 0 {
            return Err(ImageRegisterError::EmptyImage);
        }
        let max_dim = self.device.limits().max_texture_dimension_2d;
        if image.width > max_dim || image.height > max_dim {
            return Err(ImageRegisterError::TooLarge {
                width: image.width,
                height: image.height,
                max: max_dim,
            });
        }

        // Храним декодированный образ для on-demand resize при DrawImage.
        self.raw_images.insert(src.clone(), image.clone());

        // Загружаем оригинал в GPU (без resize — используется только когда
        // layout-size == intrinsic-size, т.е. для object-fit:none / scale-down
        // на маленьких картинках).
        let mut rgba = convert_to_rgba(image);
        // Apply ICC colour correction before GPU upload so wide-gamut (Display P3,
        // Rec2020) photos render correctly on sRGB displays.
        if let Some(ref profile) = image.icc_profile {
            correct_rgba_pixels(&mut rgba, profile);
        }
        let gi = self.make_gpu_image_entry(&rgba, image.width, image.height);
        self.images.insert(src, gi);
        Ok(())
    }

    /// Вычисляет GPU-ключ без мутации — только `&self`. Используется внутри
    /// render-цикла, где `parsed_faces` держит `&self.faces`.
    /// Предполагается, что нужная текстура уже создана через `ensure_image_gpu_key`.
    fn compute_image_gpu_key(&self, src: &str, box_rect: Rect, fit: ObjectFit, pos: ObjectPosition) -> String {
        self.raw_images.get(src).map(|raw| {
            let placed = fit_image_rect(box_rect, (raw.width, raw.height), fit, pos);
            let tw = placed.width.round().max(1.0) as u32;
            let th = placed.height.round().max(1.0) as u32;
            if tw != raw.width || th != raw.height {
                format!("{src}@{tw}x{th}")
            } else {
                src.to_owned()
            }
        }).unwrap_or_else(|| src.to_owned())
    }

    /// Обеспечивает наличие GPU-текстуры для `src` при отображении в `box_rect`.
    ///
    /// Если `placed`-размер (после object-fit) совпадает с intrinsic — ключ = `src`,
    /// текстура уже есть из `register_image`. Иначе создаёт CPU-bilinear ресайз до
    /// placed-размера, кеширует под `"src@WxH"`. Вызывать до render-цикла.
    fn ensure_image_gpu_key(
        &mut self,
        src: &str,
        box_rect: Rect,
        fit: ObjectFit,
        pos: ObjectPosition,
    ) {
        let resize_target = self.raw_images.get(src).map(|raw| {
            let placed = fit_image_rect(box_rect, (raw.width, raw.height), fit, pos);
            let tw = placed.width.round().max(1.0) as u32;
            let th = placed.height.round().max(1.0) as u32;
            (raw.width, raw.height, tw, th)
        });

        if let Some((iw, ih, tw, th)) = resize_target
            && (tw != iw || th != ih)
        {
            let gpu_key = format!("{src}@{tw}x{th}");
            if !self.images.contains_key(&gpu_key)
                && let Some(raw) = self.raw_images.get(src).cloned()
            {
                let resized = if tw <= raw.width && th <= raw.height {
                    resize_area_avg(&raw, tw, th)
                } else {
                    resize_bilinear(&raw, tw, th)
                };
                let mut rgba = convert_to_rgba(&resized);
                // ICC profile is on the original `raw`; resize_* drops it.
                if let Some(ref profile) = raw.icc_profile {
                    correct_rgba_pixels(&mut rgba, profile);
                }
                let gi = self.make_gpu_image_entry(&rgba, tw, th);
                self.images.insert(gpu_key, gi);
            }
        }
    }

    /// Создаёт `GpuImage` из RGBA8-буфера заданного размера.
    /// `&self` достаточно — мутировать нужно только `images`, это делает caller.
    fn make_gpu_image_entry(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumen-image-texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Не sRGB: surface у нас тоже non-sRGB, fragment пишет linear-байты
            // напрямую. Color management — Phase 3+.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let make_bg = |sampler: &wgpu::Sampler| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("image-bg"),
                layout: &self.image_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let bind_group_linear = make_bg(&self.image_sampler);
        let bind_group_nearest = make_bg(&self.image_sampler_nearest);
        GpuImage { bind_group_linear, bind_group_nearest, view, _texture: texture, width, height }
    }

    /// Снимает регистрацию изображения. После этого `DrawImage` для `src`
    /// снова рисует placeholder fill-quad.
    pub fn unregister_image(&mut self, src: &str) {
        self.raw_images.remove(src);
        // Удаляем оригинал и все кешированные ресайзы ("src@WxH").
        let prefix = format!("{src}@");
        self.images.retain(|k, _| k != src && !k.starts_with(&prefix));
    }

    /// Снимает регистрацию всех картинок (например, при переходе на новую
    /// страницу). GPU-память освобождается при drop-е `GpuImage.texture`.
    pub fn clear_images(&mut self) {
        self.raw_images.clear();
        self.images.clear();
    }

    /// Зарегистрирована ли картинка с таким `src` (для shell-логирования).
    #[must_use]
    pub fn has_image(&self, src: &str) -> bool {
        self.images.contains_key(src)
    }

    // ── Layer snapshot API ────────────────────────────────────────────────

    /// Загружает CPU-пиксели (`Rgba8`, 4 байта/пиксель) как именованный
    /// GPU-снимок слоя. Bind group использует `image_bgl` — снимок рендерится
    /// через image-pipeline как позиционированный quad при
    /// `DisplayCommand::DrawLayerSnapshot`.
    ///
    /// Если снимок с `id` уже существует — старая GPU-память освобождается при
    /// drop-е; новая занимает её место.
    ///
    /// # Errors
    /// - [`SnapshotUploadError::EmptySnapshot`] при нулевой стороне.
    /// - [`SnapshotUploadError::TooLarge`] если стороны превышают предел GPU.
    /// - [`SnapshotUploadError::InvalidDataSize`] если `pixels.len() != width * height * 4`.
    pub fn upload_layer_snapshot(
        &mut self,
        id: u64,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), SnapshotUploadError> {
        if width == 0 || height == 0 {
            return Err(SnapshotUploadError::EmptySnapshot);
        }
        let max_dim = self.device.limits().max_texture_dimension_2d;
        if width > max_dim || height > max_dim {
            return Err(SnapshotUploadError::TooLarge { width, height, max: max_dim });
        }
        let expected = (width as usize) * (height as usize) * 4;
        if pixels.len() != expected {
            return Err(SnapshotUploadError::InvalidDataSize {
                expected,
                actual: pixels.len(),
            });
        }

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer-snapshot"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer-snapshot-bg"),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        });
        self.layer_snapshots.insert(id, GpuLayerSnapshot { _texture: texture, bind_group, width, height });
        Ok(())
    }

    /// Удаляет снимок с `id`. GPU-память освобождается при drop-е.
    pub fn evict_layer_snapshot(&mut self, id: u64) {
        self.layer_snapshots.remove(&id);
    }

    /// Удаляет все снимки (например, при переходе на новую страницу).
    pub fn clear_layer_snapshots(&mut self) {
        self.layer_snapshots.clear();
    }

    /// Зарегистрирован ли снимок с таким `id`.
    #[must_use]
    pub fn has_layer_snapshot(&self, id: u64) -> bool {
        self.layer_snapshots.contains_key(&id)
    }

    /// Получить ссылку на layer cache для статистики / монитора GPU памяти.
    pub fn layer_cache(&self) -> &crate::layer_cache::LayerCache {
        &self.layer_cache
    }

    /// Enables or disables the `backdrop-filter` result cache (CSS Filter
    /// Effects L1 §2). Enabled by default. Disabling frees all cached metadata;
    /// the matching GPU textures are dropped lazily as backdrop elements are
    /// re-rendered (or via [`Self::clear_backdrop_cache`]).
    pub fn set_backdrop_cache_enabled(&mut self, enabled: bool) {
        self.backdrop_cache.set_enabled(enabled);
        if !enabled {
            self.backdrop_cache_textures.clear();
        }
    }

    /// Drops every cached `backdrop-filter` texture and its metadata. The next
    /// frame recomputes each backdrop from scratch.
    pub fn clear_backdrop_cache(&mut self) {
        self.backdrop_cache.clear();
        self.backdrop_cache_textures.clear();
    }

    /// Number of live cached `backdrop-filter` textures (for stats / tests).
    #[must_use]
    pub fn backdrop_cache_len(&self) -> usize {
        self.backdrop_cache.len()
    }

    /// Forwards a memory-pressure signal to the `backdrop-filter` cache and
    /// frees the GPU textures of any entries it evicts (ADR-008 §10D.3 /
    /// §10H). Wire into the shell's `MemoryPressureSource` poll loop.
    pub fn backdrop_cache_on_memory_pressure(
        &mut self,
        level: lumen_core::ext::MemoryPressureLevel,
    ) {
        for ord in self.backdrop_cache.on_memory_pressure(level) {
            self.backdrop_cache_textures.remove(&ord);
        }
    }

    /// Forwards a memory-pressure signal to the glyph atlas so it can evict
    /// cached entries (ADR-008 §10H).  Medium: evict ~50% LRU glyphs.
    /// High: clear entirely.  Wire into the shell's `MemoryPressureSource` poll loop.
    pub fn atlas_on_memory_pressure(&mut self, level: lumen_core::ext::MemoryPressureLevel) {
        self.atlas.on_memory_pressure(level);
    }

    /// Получить мutable ссылку для прямого управления кэшем (advanced usage).
    pub fn layer_cache_mut(&mut self) -> &mut crate::layer_cache::LayerCache {
        &mut self.layer_cache
    }

    /// Отметить layer как используемый текущим render pass.
    /// Обновляет LRU timestamp, предотвращая эвикцию активных layers.
    pub fn access_layer(&mut self, key: crate::layer_cache::LayerKey) {
        self.layer_cache.access(key);
    }

    /// Кэшировать layer слой. Returns `true` if this is a new layer, `false` if updated.
    /// Caller должна убедиться, что layer-текстура выделена в GPU
    /// (обычно через `create_layer_texture`).
    pub fn cache_layer(&mut self, key: crate::layer_cache::LayerKey, memory_bytes: u32) -> bool {
        self.layer_cache.insert(key, memory_bytes)
    }

    /// Return an off-screen layer texture to the pool for recycling (Phase 2 ADR-008).
    /// Used when a layer is no longer needed and its texture can be reused for another layer.
    pub fn return_layer_to_pool(&mut self, layer: OffscreenLayer) {
        let pooled = crate::texture_pool::PooledTexture {
            texture: layer.texture,
            view: layer.view,
            bind_group: layer.bind_group,
            width: layer.width,
            height: layer.height,
        };
        self.texture_pool.release(pooled);
    }

    /// Promote a node to its own GPU layer for `will-change: transform/opacity/filter`.
    ///
    /// Creates a `LayerCache` entry for the node so that subsequent animation ticks
    /// can update only the layer's transform matrix without triggering a full relayout.
    /// // CSS: will-change — P4 wires ComputedStyle.will_change to call this after relayout.
    pub fn promote_layer(
        &mut self,
        node_id: u32,
        width: u32,
        height: u32,
    ) -> crate::layer_cache::LayerKey {
        self.layer_cache.promote_layer(node_id, width, height)
    }

    /// Returns `true` if the given node has a promoted GPU layer.
    pub fn is_layer_promoted(&self, node_id: u32) -> bool {
        self.layer_cache.is_layer_promoted(node_id)
    }

    /// Remove the promoted GPU layer for a node, freeing its cache entry.
    pub fn demote_layer(&mut self, node_id: u32) {
        self.layer_cache.demote_layer(node_id);
    }

    /// Очистить весь layer cache (полная эвикция) и очистить texture pool.
    pub fn clear_layer_cache(&mut self) {
        self.layer_cache.clear();
        self.texture_pool.clear();
    }

    /// Get the number of free textures in the pool (for diagnostics).
    pub fn texture_pool_len(&self) -> usize {
        self.texture_pool.len()
    }

    /// Get the number of free textures of a specific size (for diagnostics).
    pub fn texture_pool_len_for_size(&self, width: u32, height: u32) -> usize {
        self.texture_pool.len_for_size(width, height)
    }

    /// Clear all pooled textures (e.g., when resizing or memory pressure is high).
    pub fn clear_texture_pool(&mut self) {
        self.texture_pool.clear();
    }

    /// Возвращает `(width, height)` снимка, или `None` если `id` не зарегистрирован.
    #[must_use]
    pub fn snapshot_dimensions(&self, id: u64) -> Option<(u32, u32)> {
        self.layer_snapshots.get(&id).map(|s| (s.width, s.height))
    }

    /// Resizes the render target. For windowed mode, reconfigures the wgpu surface.
    /// For headless mode, updates the stored physical dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            if let (Some(surface), Some(config)) =
                (self.surface.as_ref(), self.config.as_mut())
            {
                config.width = width;
                config.height = height;
                surface.configure(&self.device, config);
            } else {
                self.headless_w = width;
                self.headless_h = height;
            }
            self.layer_textures.clear();
            // Clear pooled textures on resize (Phase 2 ADR-008) to avoid size mismatches.
            self.texture_pool.clear();
            // Recreate depth texture to match new surface dimensions.
            let (t, v) = create_depth_texture(&self.device, width, height);
            self.depth_texture = Some(t);
            self.depth_view = Some(v);
        }
    }

    /// Обновить device-pixel-ratio. Вызывается shell-ом по `WindowEvent::ScaleFactorChanged`
    /// (например, при перетаскивании окна между мониторами с разной DPI).
    /// Surface сам не меняется — winit отдаёт новый physical `inner_size`
    /// через `inner_size_writer` отдельно, shell его прокинет в `resize`.
    /// Этот метод лишь обновляет коэффициент, по которому в `render()` физический
    /// размер surface превращается в logical viewport для shader-а.
    /// Значения ≤ 0 игнорируются (защита от broken winit-backend-а).
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        if scale_factor > 0.0 {
            self.scale_factor = scale_factor;
        }
    }

    /// Текущий device-pixel-ratio. Для отладки / тестов (UI обычно его не читает —
    /// shader делает деление сам в render-фазе).
    #[must_use]
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Текущий viewport в **logical** (CSS) пикселях: `physical / scale_factor`.
    /// Используется shell-ом для relayout при Resized.
    #[must_use]
    pub fn viewport_size(&self) -> winit::dpi::LogicalSize<f64> {
        let (w, h) = self.surface_dims();
        winit::dpi::PhysicalSize::new(w, h).to_logical(self.scale_factor)
    }

    /// Returns `(width, height)` in physical pixels: from surface config in windowed
    /// mode, or from `headless_w/h` in headless mode.
    #[must_use]
    fn surface_dims(&self) -> (u32, u32) {
        if let Some(c) = &self.config {
            (c.width, c.height)
        } else {
            (self.headless_w, self.headless_h)
        }
    }

    fn create_layer_texture(&mut self, width: u32, height: u32) -> OffscreenLayer {
        // Try to acquire a texture from the pool before creating a new one (Phase 2).
        if let Some(pooled) = self.texture_pool.acquire(width, height) {
            return OffscreenLayer {
                texture: pooled.texture,
                view: pooled.view,
                bind_group: pooled.bind_group,
                width: pooled.width,
                height: pooled.height,
            };
        }

        // Pool miss: allocate a new texture.
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("opacity-layer"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            // COPY_SRC needed for encoder.copy_texture_to_texture in blend compositing.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("opacity-layer-bg"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                },
            ],
        });
        self.texture_pool.update_size(1); // Track new allocation.
        OffscreenLayer { texture, view, bind_group, width, height }
    }

    /// Создаёт или пересоздаёт `scratch_layer` нужного размера.
    /// Scratch layer используется как destination-copy при blend compositing:
    /// GPU копирует содержимое parent layer туда, shader читает оба текстуры
    /// (src + dst) и вычисляет CSS Compositing L1 §8 формулу.
    fn ensure_scratch_layer(&mut self, width: u32, height: u32) {
        let needs_create = self
            .scratch_layer
            .as_ref()
            .is_none_or(|s| s.width != width || s.height != height);
        if needs_create {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blend-scratch-layer"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                // RENDER_ATTACHMENT: needed for blur V-pass (backdrop_layer → scratch)
                //   and for blend-composite destination.
                // COPY_DST: needed for copy_texture_to_texture (parent → scratch) in
                //   backdrop-filter snapshot capture.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            // scratch_layer bind_group uses composite_bgl (t_src slot) for simplicity;
            // the actual blend bind group is created on-the-fly during composite execution.
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blend-scratch-bg"),
                layout: &self.composite_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                    },
                ],
            });
            self.scratch_layer = Some(OffscreenLayer { texture, view, bind_group, width, height });
        }
    }

    /// Создаёт или пересоздаёт `backdrop_layer` нужного размера.
    /// Используется как ping-pong target для blur-проходов backdrop-filter:
    /// H-проход (scratch → backdrop_layer) и как промежуточный буфер для
    /// color-filter применения.
    fn ensure_backdrop_layer(&mut self, width: u32, height: u32) {
        let needs_create = self
            .backdrop_layer
            .as_ref()
            .is_none_or(|l| l.width != width || l.height != height);
        if needs_create {
            self.backdrop_layer = Some(self.create_layer_texture(width, height));
        }
    }

    /// Ensures a cached backdrop texture of size `width`×`height` exists for
    /// `ordinal`. Returns `true` if it was (re)created — the caller must then
    /// invalidate the matching [`Self::backdrop_cache`] entry, since a resize
    /// discards the previously cached pixels.
    ///
    /// Usage flags: `COPY_DST` (filter-only backdrops copy parent → cache
    /// directly), `RENDER_ATTACHMENT` (blur V-pass writes into the cache), and
    /// `TEXTURE_BINDING` (the blit reads the cache as its source).
    fn ensure_backdrop_cache_texture(&mut self, ordinal: u32, width: u32, height: u32) -> bool {
        let needs_create = self
            .backdrop_cache_textures
            .get(&ordinal)
            .is_none_or(|l| l.width != width || l.height != height);
        if !needs_create {
            return false;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("backdrop-cache-layer"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop-cache-bg"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
            ],
        });
        self.backdrop_cache_textures
            .insert(ordinal, OffscreenLayer { texture, view, bind_group, width, height });
        true
    }

    fn ensure_layer_textures(&mut self, count: usize, width: u32, height: u32) {
        while self.layer_textures.len() < count {
            let t = self.create_layer_texture(width, height);
            self.layer_textures.push(t);
        }
        for i in 0..count {
            if self.layer_textures[i].width != width || self.layer_textures[i].height != height {
                self.layer_textures[i] = self.create_layer_texture(width, height);
            }
        }
    }

    /// Рендерит две полосы display list-а одним кадром:
    /// - `content` — основная страница; ко всем `rect`-ам применяется
    ///   смещение `(-scroll_x, -scroll_y)` (CSS px). Так пользователь
    ///   «прокручивает» документ под фиксированным viewport-ом.
    /// - `overlay` — UI поверх (find-bar и т.п.); рисуется как есть, без
    ///   scroll-смещения. Делает overlay viewport-locked даже когда страница
    ///   прокручена.
    ///
    /// `scroll_y ≥ 0`, `scroll_x ≥ 0`. Negatives caller обязан клампить до 0.
    pub fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), wgpu::SurfaceError> {
        // CSS Filter Effects L1 §2 — backdrop-filter result cache.
        // Compute one content hash per frame, but only when the display list
        // actually contains a backdrop-filter (pages without one pay nothing).
        // Two consecutive frames hashing identically guarantees every backdrop
        // element's filtered output is identical, so the composite step can
        // reuse the cached texture and skip the expensive blur passes.
        let backdrop_frame_hash: Option<u64> = if self.backdrop_cache.is_enabled()
            && crate::display_list::contains_backdrop_filter(content, overlay)
        {
            let (sw, sh) = self.surface_dims();
            Some(crate::display_list::hash_display_list(
                content, overlay, scroll_x, scroll_y, sw, sh,
            ))
        } else {
            None
        };

        // Pre-resolve primary face_id для каждой DrawText-команды +
        // lazy-загрузка новых face-ов до сбора вершин. Делается до парсинга
        // (resolve мутирует self.faces). Resolve бежит по обеим полосам
        // в том же порядке, в котором DrawText встречается в render-loop-е
        // ниже — иначе iter "поедет" и попадёт чужой face_id.
        let mut text_face_ids: Vec<usize> =
            Vec::with_capacity(content.len() + overlay.len());
        for cmd in content.iter().chain(overlay.iter()) {
            if let DisplayCommand::DrawText {
                font_family,
                font_weight,
                font_style,
                ..
            } = cmd
            {
                text_face_ids.push(self.resolve_face_id(font_family, *font_weight, *font_style));
            }
        }
        let mut text_face_iter = text_face_ids.into_iter();

        // PRE-PASS: создаём CPU-ресайз текстуры для DrawImage до того, как
        // parsed_faces займёт &self.faces. Scroll offset не влияет на SIZE
        // (только на position), поэтому используем rect напрямую.
        for cmd in content.iter().chain(overlay.iter()) {
            if let DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. } = cmd {
                self.ensure_image_gpu_key(src, *rect, *object_fit, *object_position);
            }
        }

        // Распарсиваем все loaded faces один раз за кадр. Это нужно для
        // codepoint-cascade: per-char смотрим, есть ли глиф в primary
        // face-е; если нет — пробуем остальные. ParsedFace borrow-ит
        // от &self.faces[i].bytes; lifetime ограничен этим scope-ом.
        let parsed_faces: Vec<Option<ParsedFace<'_>>> = self
            .faces
            .iter()
            .map(|face| {
                let font = Font::parse(&face.bytes).ok()?;
                let head = font.head().ok()?;
                let hhea = font.hhea().ok()?;
                let cmap = font.cmap().ok()?;
                let hmtx = font.hmtx().ok()?;
                Some(ParsedFace { font, head, hhea, cmap, hmtx })
            })
            .collect();

        // ── Сбор вершин ────────────────────────────────────────────────────
        let mut fill_vertices: Vec<FillVertex> = Vec::new();
        let mut circle_vertices: Vec<CircleVertex> = Vec::new();
        let mut rrect_vertices: Vec<RRectVertex> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut image_vertices: Vec<ImageVertex> = Vec::new();
        // Bind groups для image draw-ов в порядке появления. DrawOp::Image
        // хранит индекс в этот Vec вместо клонирования BindGroup в каждый op.
        let mut image_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
        let mut grad_vertices: Vec<GradVertex> = Vec::new();
        // Per-gradient CPU uniform data; index = grad_batch_idx in DrawOp::Gradient.
        let mut grad_params: Vec<GradParamsCpu> = Vec::new();
        // CSS Images L4 §4 — `cross-fade` GPU resources. Vertices form one quad
        // per command; bind groups hold both image textures, sampler and the
        // progress uniform. Index in `cross_fade_bind_groups` = `cf_batch_idx`
        // on `DrawOp::CrossFade`.
        let mut cross_fade_vertices: Vec<CrossFadeVertex> = Vec::new();
        let mut cross_fade_bind_groups: Vec<wgpu::BindGroup> = Vec::new();

        // Ordered draw operations. Каждая рисующая DisplayCommand → один
        // DrawOp в этом списке. SetScissor добавляется при изменении clip-стека.
        // В render-pass обходим список линейно — это сохраняет painter's order
        // между типами команд (fill/image/text больше не идут тремя раздельными
        // блоками — теперь смешаны в исходном порядке появления).
        let mut draw_ops: Vec<DrawOp> = Vec::new();

        // Стек активных clip-rect-ов в CSS-px (после intersection с предыдущими).
        // Пустой стек = full-frame scissor. PushClipRect добавляет пересечение
        // с топом; PopClip снимает.
        let mut clip_stack: Vec<Rect> = Vec::new();

        // Стек активных blend-mode-ов (CSS Compositing & Blending L1 §5).
        // Phase 0: stack отслеживается для корректного баланса Push/Pop;
        // рендеринг всегда использует Normal pipeline (ALPHA_BLENDING).
        // Реальное переключение pipeline по mode — задача 1B.5+.
        let mut blend_mode_stack: Vec<BlendMode> = Vec::new();

        // CSS Transforms L1 §13 — стек активных forward-матриц. Каждый элемент
        // хранит АККУМУЛИРОВАННОЕ произведение (родитель · self), т.е. на топе
        // лежит матрица, готовая к прямому применению к viewport-координатам
        // вершин. На PushTransform — `top.multiply(&new)` (multiplication
        // справа моделирует «применить self до родителя» в column-major
        // конвенции, что соответствует CSS «inner transform applied first»).
        // На PopTransform — сбрасываем топ.
        let mut transform_stack: Vec<Mat4> = Vec::new();

        // Render plan: список батчей и composite-переходов.
        #[derive(Clone, Copy)]
        enum LoadOpChoice { ClearWhite, ClearTransparent, Load }
        struct DrawBatchPlan { target_level: usize, load_op: LoadOpChoice, ops_start: usize, ops_end: usize }
        struct CompositePlan { from_level: usize, comp_v_start: u32, mode: BlendMode }
        // CSS Masking L1 §4: gradient mask spec — stored in plan for render-time GPU pass.
        // For Linear: p0/p1 are UV endpoints (from linear_gradient_uv_endpoints).
        // For Radial: p0=[cx_pct,cy_pct], p1=[1,1] (radial_gradient_uv_params).
        // For Conic:  p0=[cx_pct,cy_pct], p1=[width,height] (box-space atan2).
        #[derive(Clone)]
        enum MaskGradientSpec {
            Linear { params: GradParamsCpu, rect: Rect },
            Radial { params: GradParamsCpu, rect: Rect },
            Conic  { params: GradParamsCpu, rect: Rect },
        }
        // CSS Masking L1 §4: mask composite plan. `from_level` = offscreen level
        // with element content; `mask_src` = key in self.images (image mask).
        // `mask_gradient` = gradient mask rendered to temp surface-size texture.
        // `mask_v_start..mask_v_end` indexes into `mask_vertices`.
        // Box<MaskGradientSpec>: GradParamsCpu is 544 bytes; boxing avoids large-variant warning.
        struct MaskCompositePlan {
            from_level: usize,
            mask_v_start: u32,
            mask_v_end: u32,
            mask_src: Option<String>,
            mask_gradient: Option<Box<MaskGradientSpec>>,
        }
        // CSS Filter Effects L1 — filter composite plan.
        // `from_level` = offscreen layer with element content.
        // `filters` = filter list (may include Blur + color filters).
        // `comp_v_start` = start of 6-vertex fullscreen quad in composite_vertices.
        struct FilterCompositePlan {
            from_level: usize,
            filters: Vec<FilterFn>,
            comp_v_start: u32,
        }
        // CSS Filter Effects L1 §2 / Compositing §13 — backdrop-filter plan.
        // `from_level` = element's offscreen layer (content rendered here).
        // `filters` = backdrop filter list.
        // `comp_v_start` = fullscreen quad (blur passes + element composite).
        // `bounds_v_start` = bounded quad (color-filter blit to parent at element bounds).
        struct BackdropFilterCompositePlan {
            from_level: usize,
            filters: Vec<FilterFn>,
            comp_v_start: u32,
            bounds_v_start: u32,
            /// Stable index among backdrop elements in this frame (paint order).
            /// Cache key for [`Renderer::backdrop_cache`] and the matching texture.
            ordinal: u32,
        }
        // CSS Masking L1 §5 — mask-layer composite plan.
        // `from_level`   = offscreen level where mask content was rendered.
        // `parent_level` = from_level − 1: where element content lives.
        // `ml_v_start/end` indexes into `mask_layer_vertices`.
        // `mode` selects alpha vs. luminance mask compositing.
        struct MaskLayerCompositePlan {
            from_level: usize,
            mode: MaskMode,
            ml_v_start: u32,
            ml_v_end: u32,
        }
        enum RenderPlanItem {
            Draw(DrawBatchPlan),
            Composite(CompositePlan),
            MaskComposite(MaskCompositePlan),
            FilterComposite(FilterCompositePlan),
            BackdropFilterComposite(BackdropFilterCompositePlan),
            MaskLayerComposite(MaskLayerCompositePlan),
        }

        let mut render_plan: Vec<RenderPlanItem> = Vec::new();
        let mut composite_vertices: Vec<CompositeVertex> = Vec::new();
        // Accumulated vertex data for mask composite passes.
        let mut mask_vertices: Vec<MaskVertex> = Vec::new();
        // Accumulated vertex data for mask-layer composite passes (PushMaskLayer/PopMaskLayer).
        let mut mask_layer_vertices: Vec<MaskVertex> = Vec::new();
        // Stack of PushMask params. Pushed by PushMask*, popped by PopMask.
        // Either `src` (image key) or `gradient` is set; never both.
        struct MaskPushInfo {
            src: Option<String>,
            gradient: Option<MaskGradientSpec>,
            size: BackgroundSize,
            position: ObjectPosition,
            repeat: BackgroundRepeat,
            rect: Rect,
        }
        let mut mask_params_stack: Vec<MaskPushInfo> = Vec::new();
        // Stack for PushMaskLayer: (rect, mode). Popped by PopMaskLayer.
        let mut mask_layer_stack: Vec<(Rect, MaskMode)> = Vec::new();

        let mut current_level: usize = 0;
        let mut level_alpha_stack: Vec<f32> = Vec::new();
        // Tracks blend mode per opened offscreen level (for non-Normal PushBlendMode).
        let mut level_blend_mode_stack: Vec<BlendMode> = Vec::new();
        // Tracks filter list per opened offscreen level (for CSS filter compositing).
        let mut filter_stack: Vec<Vec<FilterFn>> = Vec::new();
        // Stack for backdrop-filter: (filter_list, element_bounds_css_px).
        let mut backdrop_filter_stack: Vec<(Vec<FilterFn>, lumen_core::geom::Rect)> = Vec::new();
        // Monotonic counter assigning a stable ordinal to each backdrop element
        // (in paint/pop order) — the key into the backdrop-filter result cache.
        let mut backdrop_ordinal: u32 = 0;
        let mut level_first: Vec<bool> = vec![true];
        let mut batch_start: usize = 0;

        // Текущий выставленный scissor (для дедупликации SetScисsor-команд).
        // None = не выставлен (первый SetScissor нужен в любом случае).
        let mut current_scissor: Option<DeviceScissor> = None;
        let (surface_w, surface_h) = self.surface_dims();

        let dpr_f32 = self.scale_factor.max(1e-6) as f32;

        macro_rules! flush_batch {
            () => {{
                let first = level_first.get(current_level).copied().unwrap_or(false);
                let load_op = if first {
                    if current_level == 0 { LoadOpChoice::ClearWhite } else { LoadOpChoice::ClearTransparent }
                } else {
                    LoadOpChoice::Load
                };
                let has_ops = batch_start < draw_ops.len();
                if has_ops || first {
                    render_plan.push(RenderPlanItem::Draw(DrawBatchPlan {
                        target_level: current_level,
                        load_op,
                        ops_start: batch_start,
                        ops_end: draw_ops.len(),
                    }));
                    if current_level < level_first.len() {
                        level_first[current_level] = false;
                    }
                }
                batch_start = draw_ops.len();
                current_scissor = None;
            }}
        }

        // CSS Positioning L3 §6.3 — position:sticky offset stack.
        // Each BeginStickyLayer pushes a (dy, dx) that clamps scroll for its subtree.
        let viewport_css_h = surface_h as f32 / dpr_f32;
        let viewport_css_w = surface_w as f32 / dpr_f32;
        let mut sticky_stack: Vec<(f32, f32)> = Vec::new();

        let iter_content = content.iter().map(|c| (c, false));
        let iter_overlay = overlay.iter().map(|c| (c, true));
        for (cmd, is_overlay) in iter_content.chain(iter_overlay) {
            let (dy, dx) = if is_overlay {
                (0.0_f32, 0.0_f32)
            } else {
                sticky_stack.last().copied().unwrap_or((-scroll_y, -scroll_x))
            };
            match cmd {
                DisplayCommand::FillRect { rect, color } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let v_start = fill_vertices.len() as u32;
                    push_fill_quad(
                        &mut fill_vertices,
                        translate_rect(*rect, dx, dy),
                        apply_alpha_to_color(color_to_array(color), alpha),
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                DisplayCommand::FillRoundedRect { rect, color, radii } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let r = translate_rect(*rect, dx, dy);
                    let v_start = rrect_vertices.len() as u32;
                    push_rrect_quad(&mut rrect_vertices, r, color_to_array(color), *radii);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_rrect_verts(&mut rrect_vertices[v_start as usize..], m);
                    }
                    let v_count = rrect_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::RRect { v_start, v_count });
                    }
                }
                DisplayCommand::DrawBorder {
                    rect,
                    widths: [wt, wr, wb, wl],
                    colors: [ct, cr, cb, cl],
                    styles: [st, sr, sb, sl],
                    radii,
                } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let r = translate_rect(*rect, dx, dy);
                    let fill_v_start = fill_vertices.len() as u32;
                    let circle_v_start = circle_vertices.len() as u32;

                    if radii.all_zero() {
                        // CSS Backgrounds L3 §6.3 — прямоугольные рёбра без угловых дуг.
                        // Каждая сторона укорочена на corner-квадраты, чтобы dash/dot
                        // паттерн шёл только вдоль прямого участка (как в Chrome/Edge).
                        // Угловые квадраты всегда solid.
                        let ct_arr = apply_alpha_to_color(color_to_array(ct), alpha);
                        let cr_arr = apply_alpha_to_color(color_to_array(cr), alpha);
                        let cb_arr = apply_alpha_to_color(color_to_array(cb), alpha);
                        let cl_arr = apply_alpha_to_color(color_to_array(cl), alpha);

                        // All styles span full box width/height including corners.
                        // Chrome/Edge draws each side at full extent; adjacent sides overlap
                        // at corners, with later-drawn sides overwriting earlier ones.
                        // Rendering order: top → right → bottom → left (left wins at corners).
                        if *wt > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y, r.width, *wt),
                                true, *wt, ct_arr, *st,
                            );
                        }
                        if *wr > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x + r.width - *wr, r.y, *wr, r.height),
                                false, *wr, cr_arr, *sr,
                            );
                        }
                        if *wb > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y + r.height - *wb, r.width, *wb),
                                true, *wb, cb_arr, *sb,
                            );
                        }
                        if *wl > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y, *wl, r.height),
                                false, *wl, cl_arr, *sl,
                            );
                        }
                    } else {
                        // CSS Backgrounds L3 §5 + §6.3 — стороны укорочены у углов;
                        // каждый угол рисуется как дуга-сектор (tessellated arc).
                        // Каждый радиус также ограничен половиной соответствующей стороны.
                        let r_tl = radii.tl.min(r.width / 2.0).min(r.height / 2.0);
                        let r_tr = radii.tr.min(r.width / 2.0).min(r.height / 2.0);
                        let r_br = radii.br.min(r.width / 2.0).min(r.height / 2.0);
                        let r_bl = radii.bl.min(r.width / 2.0).min(r.height / 2.0);
                        let ct_arr = apply_alpha_to_color(color_to_array(ct), alpha);
                        let cr_arr = apply_alpha_to_color(color_to_array(cr), alpha);
                        let cb_arr = apply_alpha_to_color(color_to_array(cb), alpha);
                        let cl_arr = apply_alpha_to_color(color_to_array(cl), alpha);
                        // Top side (shortened by r_tl on left, r_tr on right).
                        if *wt > 0.0 {
                            let x0 = r.x + r_tl;
                            let x1 = r.x + r.width - r_tr;
                            if x1 > x0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(x0, r.y, x1 - x0, *wt),
                                    true, *wt, ct_arr, *st,
                                );
                            }
                        }
                        // Right side (shortened by r_tr on top, r_br on bottom).
                        if *wr > 0.0 {
                            let y0 = r.y + r_tr;
                            let y1 = r.y + r.height - r_br;
                            if y1 > y0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(r.x + r.width - wr, y0, *wr, y1 - y0),
                                    false, *wr, cr_arr, *sr,
                                );
                            }
                        }
                        // Bottom side (shortened by r_br on right, r_bl on left).
                        if *wb > 0.0 {
                            let x0 = r.x + r_bl;
                            let x1 = r.x + r.width - r_br;
                            if x1 > x0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(x0, r.y + r.height - wb, x1 - x0, *wb),
                                    true, *wb, cb_arr, *sb,
                                );
                            }
                        }
                        // Left side (shortened by r_tl on top, r_bl on bottom).
                        if *wl > 0.0 {
                            let y0 = r.y + r_tl;
                            let y1 = r.y + r.height - r_bl;
                            if y1 > y0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(r.x, y0, *wl, y1 - y0),
                                    false, *wl, cl_arr, *sl,
                                );
                            }
                        }
                        // Corner arcs: quarter-annulus for each corner with radius > 0.
                        // TL corner (180°→270° in screen-Y-down coords = left→up).
                        if r_tl > 0.0 {
                            let inner = (r_tl - wt.max(*wl)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r_tl, r.y + r_tl], r_tl, inner, 180.0, 270.0, ct_arr);
                        }
                        // TR corner (270°→360° = up→right).
                        if r_tr > 0.0 {
                            let inner = (r_tr - wt.max(*wr)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r.width - r_tr, r.y + r_tr], r_tr, inner, 270.0, 360.0, ct_arr);
                        }
                        // BR corner (0°→90° = right→down).
                        if r_br > 0.0 {
                            let inner = (r_br - wb.max(*wr)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r.width - r_br, r.y + r.height - r_br], r_br, inner, 0.0, 90.0, cb_arr);
                        }
                        // BL corner (90°→180° = down→left).
                        if r_bl > 0.0 {
                            let inner = (r_bl - wb.max(*wl)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r_bl, r.y + r.height - r_bl], r_bl, inner, 90.0, 180.0, cb_arr);
                        }
                    }

                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[fill_v_start as usize..], m);
                        apply_affine_to_circle_verts(&mut circle_vertices[circle_v_start as usize..], m);
                    }
                    let fill_v_count = fill_vertices.len() as u32 - fill_v_start;
                    if fill_v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start: fill_v_start, v_count: fill_v_count });
                    }
                    let circle_v_count = circle_vertices.len() as u32 - circle_v_start;
                    if circle_v_count > 0 {
                        draw_ops.push(DrawOp::Circle { v_start: circle_v_start, v_count: circle_v_count });
                    }
                }
                DisplayCommand::DrawText {
                    rect,
                    text,
                    font_size,
                    color,
                    font_family: _,
                    font_weight: _,
                    font_style: _,
                    font_variation_axes,
                    tab_size,
                    highlight_name: _,
                } => {
                    let primary_face_id = text_face_iter.next().unwrap_or(0);
                    if parsed_faces
                        .get(primary_face_id)
                        .and_then(|p| p.as_ref())
                        .is_none()
                    {
                        continue;
                    }
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let v_start = text_vertices.len() as u32;
                    push_text_glyphs(
                        &mut text_vertices,
                        translate_rect(*rect, dx, dy),
                        text,
                        *font_size,
                        apply_alpha_to_color(color_to_array(color), alpha),
                        primary_face_id,
                        &parsed_faces,
                        &mut self.atlas,
                        &mut self.cached_glyphs,
                        font_variation_axes,
                        *tab_size,
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut text_vertices[v_start as usize..], m);
                    }
                    let v_count = text_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Text { v_start, v_count });
                    }
                }
                DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
                    // CSS Basic UI L4 §5: outline рисуется СНАРУЖИ box-а.
                    // Outer rect = box + outline-offset (по всем сторонам) +
                    // outline-width (тоже по всем сторонам). Inner граница =
                    // box + outline-offset. `OutlineStyle::Auto` рендерится
                    // как Solid (UA focus ring без дополнительного хвоста);
                    // Dashed/Dotted разворачиваются в pattern из квадратов
                    // через `emit_outline_side`.
                    if *width <= 0.0 {
                        continue;
                    }
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let r = translate_rect(*rect, dx, dy);
                    let inner = Rect::new(
                        r.x - offset,
                        r.y - offset,
                        r.width + 2.0 * offset,
                        r.height + 2.0 * offset,
                    );
                    let w = *width;
                    let c = apply_alpha_to_color(color_to_array(color), alpha);
                    let fill_v_start = fill_vertices.len() as u32;
                    let circle_v_start = circle_vertices.len() as u32;
                    // Top stripe (с "ear" по углам слева/справа).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y - w, inner.width + 2.0 * w, w),
                        true,
                        w,
                        c,
                        *style,
                    );
                    // Bottom stripe (тоже с углами).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y + inner.height, inner.width + 2.0 * w, w),
                        true,
                        w,
                        c,
                        *style,
                    );
                    // Left stripe (между inner.y и inner.y+inner.height,
                    // без углов — они уже в top/bottom).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    // Right stripe.
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x + inner.width, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[fill_v_start as usize..], m);
                        apply_affine_to_circle_verts(&mut circle_vertices[circle_v_start as usize..], m);
                    }
                    let fill_v_count = fill_vertices.len() as u32 - fill_v_start;
                    if fill_v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start: fill_v_start, v_count: fill_v_count });
                    }
                    let circle_v_count = circle_vertices.len() as u32 - circle_v_start;
                    if circle_v_count > 0 {
                        draw_ops.push(DrawOp::Circle { v_start: circle_v_start, v_count: circle_v_count });
                    }
                }
                DisplayCommand::DrawImage {
                    rect,
                    src,
                    alt,
                    object_fit,
                    object_position,
                    image_rendering,
                } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let scrolled = translate_rect(*rect, dx, dy);
                    let fit = *object_fit;
                    let pos = *object_position;

                    // Вычисляем GPU-ключ (текстура уже создана в pre-pass).
                    // GPU делает 1:1 сэмплинг по CPU-bilinear scaled текстуре →
                    // pixel-perfect совпадение с браузерами на одном железе.
                    let gpu_key = self.compute_image_gpu_key(src, scrolled, fit, pos);
                    if let Some(gpu) = self.images.get(&gpu_key) {
                        if let Some((visible, uv_min, uv_max)) = fit_image_quad(
                            scrolled,
                            (gpu.width, gpu.height),
                            fit,
                            pos,
                        ) {
                            let v_start = image_vertices.len() as u32;
                            push_image_quad(&mut image_vertices, visible, uv_min, uv_max, alpha);
                            if let Some(m) = transform_stack.last() {
                                apply_affine_to_verts(
                                    &mut image_vertices[v_start as usize..],
                                    m,
                                );
                            }
                            let v_count = image_vertices.len() as u32 - v_start;
                            let image_batch_idx = image_bind_groups.len() as u32;
                            let bg = if matches!(image_rendering, ImageRendering::Pixelated | ImageRendering::CrispEdges) {
                                gpu.bind_group_nearest.clone()
                            } else {
                                gpu.bind_group_linear.clone()
                            };
                            image_bind_groups.push(bg);
                            draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                        }
                    } else {
                        // Картинку никто не зарегистрировал (fetch не сделан /
                        // декодер упал / неизвестный формат) — fallback на
                        // серый placeholder, чтобы место в layout-е было видно.
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            scrolled,
                            apply_alpha_to_color([0.85, 0.85, 0.85, 1.0], alpha),
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                        // BUG-015: render alt text over the placeholder when the
                        // image fails to load. Uses face 0 (bundled Inter) at 12px.
                        // Only rendered when the box is tall enough for one text line.
                        const BROKEN_FONT_SIZE: f32 = 12.0;
                        const BROKEN_PAD: f32 = 4.0;
                        if !alt.is_empty()
                            && scrolled.height >= BROKEN_FONT_SIZE + 2.0 * BROKEN_PAD
                            && parsed_faces.first().and_then(|p| p.as_ref()).is_some()
                        {
                            let text_rect = Rect::new(
                                scrolled.x + BROKEN_PAD,
                                scrolled.y + BROKEN_PAD,
                                (scrolled.width - 2.0 * BROKEN_PAD).max(0.0),
                                (scrolled.height - 2.0 * BROKEN_PAD).max(0.0),
                            );
                            let t_start = text_vertices.len() as u32;
                            push_text_glyphs(
                                &mut text_vertices,
                                text_rect,
                                alt,
                                BROKEN_FONT_SIZE,
                                apply_alpha_to_color([0.35, 0.35, 0.35, 1.0], alpha),
                                0,
                                &parsed_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                &[],
                                0.0,
                            );
                            if let Some(m) = transform_stack.last() {
                                apply_affine_to_verts(
                                    &mut text_vertices[t_start as usize..],
                                    m,
                                );
                            }
                            let t_count = text_vertices.len() as u32 - t_start;
                            if t_count > 0 {
                                draw_ops.push(DrawOp::Text { v_start: t_start, v_count: t_count });
                            }
                        }
                    }
                }
                // Clip-stack управление. PushClipRect добавляет пересечение
                // с топом (CSS Masking L1 §3 — clip-rect = intersection всех
                // ancestor clip-region-ов). PopClip снимает топ. Scissor для
                // wgpu выставляется лениво — следующая draw-команда вызовет
                // sync_scissor_to_stack.
                DisplayCommand::PushClipRect { rect } => {
                    let scrolled = translate_rect(*rect, dx, dy);
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, scrolled),
                        None => scrolled,
                    };
                    clip_stack.push(new);
                }
                DisplayCommand::PopClip => {
                    clip_stack.pop();
                }
                DisplayCommand::PushOpacity { alpha } => {
                    flush_batch!();
                    level_alpha_stack.push(*alpha);
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PopOpacity => {
                    if !level_alpha_stack.is_empty() {
                        flush_batch!();
                        let layer_alpha = level_alpha_stack.pop().unwrap();
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, layer_alpha);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode: BlendMode::Normal,
                        }));
                        current_level -= 1;
                    }
                }
                // CSS Compositing & Blending L1 §5 — mix-blend-mode compositing.
                // Non-Normal mode: push offscreen level + track blend mode.
                // Normal mode: no offscreen layer needed (pass-through).
                DisplayCommand::PushBlendMode { mode } => {
                    blend_mode_stack.push(*mode);
                    if *mode != BlendMode::Normal {
                        flush_batch!();
                        level_blend_mode_stack.push(*mode);
                        current_level += 1;
                        while level_first.len() <= current_level {
                            level_first.push(true);
                        }
                        level_first[current_level] = true;
                    }
                }
                DisplayCommand::PopBlendMode => {
                    blend_mode_stack.pop();
                    if let Some(mode) = level_blend_mode_stack.pop() {
                        flush_batch!();
                        let comp_v_start = composite_vertices.len() as u32;
                        // alpha=1.0: blend shader handles all compositing math.
                        push_composite_quad(&mut composite_vertices, 1.0);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode,
                        }));
                        current_level -= 1;
                    }
                }
                // CSS Backgrounds L3 §3.3/3.4/3.5 — background-size/position/repeat.
                DisplayCommand::DrawBackgroundImage { rect, origin_rect, src, size, position, repeat, image_rendering } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    // `area`  — paint/clip bounds (background-clip). Tiles are drawn only inside.
                    // `oarea` — positioning area (background-origin). Used for size/position math
                    //           per CSS Backgrounds L3 §3.5/3.5.2.
                    let area  = translate_rect(*rect, dx, dy);
                    let oarea = translate_rect(*origin_rect, dx, dy);
                    let Some(gpu) = self.images.get(src) else { continue };
                    let img_w = gpu.width as f32;
                    let img_h = gpu.height as f32;
                    if img_w <= 0.0 || img_h <= 0.0 { continue; }

                    // Compute tile dimensions from background-size relative to positioning area.
                    let (tile_w, tile_h) = match size {
                        BackgroundSize::Auto => (img_w, img_h),
                        BackgroundSize::Cover => {
                            let s = (oarea.width / img_w).max(oarea.height / img_h);
                            (img_w * s, img_h * s)
                        }
                        BackgroundSize::Contain => {
                            let s = (oarea.width / img_w).min(oarea.height / img_h);
                            (img_w * s, img_h * s)
                        }
                        BackgroundSize::Length(w, h) => {
                            let tw = w.max(1.0);
                            let th = h.unwrap_or_else(|| img_h * (tw / img_w)).max(1.0);
                            (tw, th)
                        }
                    };

                    // Compute first tile origin from background-position relative to positioning area.
                    let off_x = match position.x {
                        PositionComponent::Px(px) => px,
                        PositionComponent::Percent(p) => (oarea.width - tile_w) * p,
                    };
                    let off_y = match position.y {
                        PositionComponent::Px(py) => py,
                        PositionComponent::Percent(p) => (oarea.height - tile_h) * p,
                    };
                    let tile_x0 = oarea.x + off_x;
                    let tile_y0 = oarea.y + off_y;

                    let (tile_x_start, repeat_x, repeat_y) = match repeat {
                        BackgroundRepeat::NoRepeat => (tile_x0, false, false),
                        BackgroundRepeat::RepeatX  => (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, false),
                        BackgroundRepeat::RepeatY  => (tile_x0, false, true),
                        BackgroundRepeat::Repeat | BackgroundRepeat::Round | BackgroundRepeat::Space => {
                            (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, true)
                        }
                    };
                    let tile_y_start = if repeat_y {
                        tile_y0 - (off_y / tile_h).ceil() * tile_h
                    } else {
                        tile_y0
                    };

                    let v_start = image_vertices.len() as u32;
                    let image_batch_idx = image_bind_groups.len() as u32;
                    let bg = if matches!(image_rendering, ImageRendering::Pixelated | ImageRendering::CrispEdges) {
                        gpu.bind_group_nearest.clone()
                    } else {
                        gpu.bind_group_linear.clone()
                    };
                    image_bind_groups.push(bg);

                    // Paint bounds: tiles are clipped to the background-clip area.
                    let x_end = area.x + area.width;
                    let y_end = area.y + area.height;
                    let mut ty = tile_y_start;
                    loop {
                        if ty >= y_end { break; }
                        let mut tx = tile_x_start;
                        loop {
                            if tx >= x_end { break; }
                            // Clip tile to background area; compute partial UVs.
                            let cx = tx.max(area.x);
                            let cy = ty.max(area.y);
                            let cx1 = (tx + tile_w).min(x_end);
                            let cy1 = (ty + tile_h).min(y_end);
                            if cx < cx1 && cy < cy1 {
                                let u0 = (cx - tx) / tile_w;
                                let v0 = (cy - ty) / tile_h;
                                let u1 = (cx1 - tx) / tile_w;
                                let v1 = (cy1 - ty) / tile_h;
                                push_image_quad(&mut image_vertices,
                                    Rect::new(cx, cy, cx1 - cx, cy1 - cy),
                                    [u0, v0], [u1, v1], 1.0);
                            }
                            if !repeat_x { break; }
                            tx += tile_w;
                        }
                        if !repeat_y { break; }
                        ty += tile_h;
                    }
                    let v_count = image_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                    }
                }
                // CSS Images L3 §3.3 — GPU linear gradient pipeline.
                DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 0, *repeating, 0.0);
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                // CSS Images L3 §3.5 — GPU radial gradient pipeline.
                DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = radial_gradient_uv_params(*center_x_pct, *center_y_pct);
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 1, *repeating, 0.0);
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                // CSS Images L4 §3.7 — GPU conic gradient pipeline.
                DisplayCommand::DrawConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    // p0 = center (UV); p1 = box size in CSS px (for box-space angle).
                    let p0 = [*center_x_pct, *center_y_pct];
                    let p1 = [scrolled.width.max(1e-6), scrolled.height.max(1e-6)];
                    let from_angle_rad = from_angle_deg.to_radians();
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 2, *repeating, from_angle_rad);
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    // Снимок рендерится через image-pipeline: UV всегда [0,0]→[1,1]
                    // (весь снимок без object-fit). Если id не зарегистрирован —
                    // команда молча игнорируется (compositor мог вызвать evict).
                    if let Some(snap) = self.layer_snapshots.get(id) {
                        let v_start = image_vertices.len() as u32;
                        push_image_quad(
                            &mut image_vertices,
                            scrolled,
                            [0.0, 0.0],
                            [1.0, 1.0],
                            *alpha,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut image_vertices[v_start as usize..], m);
                        }
                        let v_count = image_vertices.len() as u32 - v_start;
                        let image_batch_idx = image_bind_groups.len() as u32;
                        image_bind_groups.push(snap.bind_group.clone());
                        draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                    }
                }
                // CSS Transforms L1 §13 — пушим matrix умноженную на текущий
                // топ (накопление транcформов вложенных боксов). Топ-матрица
                // применяется ко всем последующим вершинам до парного
                // PopTransform. Сам Push/Pop не флашит batch — transform
                // CPU-side применяется к вершинам, не меняет GPU-pipeline.
                DisplayCommand::PushTransform { matrix } => {
                    let accumulated = match transform_stack.last() {
                        Some(prev) => prev.multiply(matrix),
                        None => *matrix,
                    };
                    transform_stack.push(accumulated);
                }
                DisplayCommand::PopTransform => {
                    transform_stack.pop();
                }
                // CSS Overflow L3 §3.2 — PushScrollLayer: clip to padding-box + translate
                // content by (-scroll_x, -scroll_y). Combines a PushClipRect and a 2D
                // translation on the transform stack; PopScrollLayer unwinds both.
                DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } => {
                    // Clip (same as PushClipRect, accounting for sticky dx/dy).
                    let scrolled_clip = translate_rect(*clip_rect, dx, dy);
                    let new_clip = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, scrolled_clip),
                        None => scrolled_clip,
                    };
                    clip_stack.push(new_clip);
                    // Scroll translate: shift content by -scroll_x, -scroll_y.
                    let scroll_m = Mat4::translation_2d(-scroll_x, -scroll_y);
                    let accumulated = match transform_stack.last() {
                        Some(prev) => prev.multiply(&scroll_m),
                        None => scroll_m,
                    };
                    transform_stack.push(accumulated);
                }
                DisplayCommand::PopScrollLayer => {
                    transform_stack.pop();
                    clip_stack.pop();
                }
                // CSS Masking L1 §4 — PushMask*: open an offscreen layer for the element,
                // and record mask params so PopMask can composite with the mask.
                DisplayCommand::PushMaskImage { rect, src, size, position, repeat, .. } => {
                    flush_batch!();
                    mask_params_stack.push(MaskPushInfo {
                        src: Some(src.clone()),
                        gradient: None,
                        size: *size,
                        position: *position,
                        repeat: *repeat,
                        rect: translate_rect(*rect, dx, dy),
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                // CSS Masking L1 §4 — gradient masks: build GradParamsCpu at plan time;
                // render-time pass renders gradient → surface-size temp texture → use as mask.
                DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 0, *repeating, 0.0);
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Linear { params, rect: scrolled }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PushMaskRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = radial_gradient_uv_params(*center_x_pct, *center_y_pct);
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 1, *repeating, 0.0);
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Radial { params, rect: scrolled }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PushMaskConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let p0 = [*center_x_pct, *center_y_pct];
                    let p1 = [scrolled.width.max(1e-6), scrolled.height.max(1e-6)];
                    let from_angle_rad = from_angle_deg.to_radians();
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 2, *repeating, from_angle_rad);
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Conic { params, rect: scrolled }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PopMask => {
                    flush_batch!();
                    let Some(info) = mask_params_stack.pop() else { continue };
                    let mv_start = mask_vertices.len() as u32;
                    let (mask_src, mask_gradient) = if let Some(src) = &info.src {
                        // Image mask: build tile quads — same tiling logic as DrawBackgroundImage.
                        if let Some(gpu) = self.images.get(src) {
                            let img_w = gpu.width as f32;
                            let img_h = gpu.height as f32;
                            if img_w > 0.0 && img_h > 0.0 {
                                let area = info.rect;
                                let (tile_w, tile_h) = match info.size {
                                    BackgroundSize::Auto => (img_w, img_h),
                                    BackgroundSize::Cover => {
                                        let s = (area.width / img_w).max(area.height / img_h);
                                        (img_w * s, img_h * s)
                                    }
                                    BackgroundSize::Contain => {
                                        let s = (area.width / img_w).min(area.height / img_h);
                                        (img_w * s, img_h * s)
                                    }
                                    BackgroundSize::Length(w, h) => {
                                        let tw = w.max(1.0);
                                        let th = h.unwrap_or_else(|| img_h * (tw / img_w)).max(1.0);
                                        (tw, th)
                                    }
                                };
                                let off_x = match info.position.x {
                                    PositionComponent::Px(px) => px,
                                    PositionComponent::Percent(p) => (area.width - tile_w) * p,
                                };
                                let off_y = match info.position.y {
                                    PositionComponent::Px(py) => py,
                                    PositionComponent::Percent(p) => (area.height - tile_h) * p,
                                };
                                let tile_x0 = area.x + off_x;
                                let tile_y0 = area.y + off_y;
                                let (tile_x_start, repeat_x, repeat_y) = match info.repeat {
                                    BackgroundRepeat::NoRepeat => (tile_x0, false, false),
                                    BackgroundRepeat::RepeatX => (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, false),
                                    BackgroundRepeat::RepeatY => (tile_x0, false, true),
                                    BackgroundRepeat::Repeat | BackgroundRepeat::Round | BackgroundRepeat::Space => {
                                        (tile_x0 - (off_x / tile_w).ceil() * tile_w, true, true)
                                    }
                                };
                                let tile_y_start = if repeat_y {
                                    tile_y0 - (off_y / tile_h).ceil() * tile_h
                                } else {
                                    tile_y0
                                };
                                let x_end = area.x + area.width;
                                let y_end = area.y + area.height;
                                let mut ty = tile_y_start;
                                loop {
                                    if ty >= y_end { break; }
                                    let mut tx = tile_x_start;
                                    loop {
                                        if tx >= x_end { break; }
                                        let cx = tx.max(area.x);
                                        let cy = ty.max(area.y);
                                        let cx1 = (tx + tile_w).min(x_end);
                                        let cy1 = (ty + tile_h).min(y_end);
                                        if cx < cx1 && cy < cy1 {
                                            let u0 = (cx - tx) / tile_w;
                                            let v0 = (cy - ty) / tile_h;
                                            let u1 = (cx1 - tx) / tile_w;
                                            let v1 = (cy1 - ty) / tile_h;
                                            mask_vertices.extend_from_slice(&[
                                                MaskVertex { pos: [cx,  cy ], uv_mask: [u0, v0] },
                                                MaskVertex { pos: [cx1, cy ], uv_mask: [u1, v0] },
                                                MaskVertex { pos: [cx1, cy1], uv_mask: [u1, v1] },
                                                MaskVertex { pos: [cx,  cy ], uv_mask: [u0, v0] },
                                                MaskVertex { pos: [cx1, cy1], uv_mask: [u1, v1] },
                                                MaskVertex { pos: [cx,  cy1], uv_mask: [u0, v1] },
                                            ]);
                                        }
                                        if !repeat_x { break; }
                                        tx += tile_w;
                                    }
                                    if !repeat_y { break; }
                                    ty += tile_h;
                                }
                            }
                        }
                        (Some(src.clone()), None)
                    } else if let Some(grad) = info.gradient.clone() {
                        // Gradient mask: quad covers element rect; uv_mask = pos/surface
                        // so the surface-size gradient texture is sampled at the same coords
                        // as the content layer (uv_layer = pos/viewport).
                        let area = info.rect;
                        let (sw, sh) = (surface_w as f32, surface_h as f32);
                        let (x0, y0) = (area.x, area.y);
                        let (x1, y1) = (area.x + area.width, area.y + area.height);
                        mask_vertices.extend_from_slice(&[
                            MaskVertex { pos: [x0, y0], uv_mask: [x0/sw, y0/sh] },
                            MaskVertex { pos: [x1, y0], uv_mask: [x1/sw, y0/sh] },
                            MaskVertex { pos: [x1, y1], uv_mask: [x1/sw, y1/sh] },
                            MaskVertex { pos: [x0, y0], uv_mask: [x0/sw, y0/sh] },
                            MaskVertex { pos: [x1, y1], uv_mask: [x1/sw, y1/sh] },
                            MaskVertex { pos: [x0, y1], uv_mask: [x0/sw, y1/sh] },
                        ]);
                        (None, Some(Box::new(grad)))
                    } else {
                        (None, None)
                    };
                    let mv_end = mask_vertices.len() as u32;
                    render_plan.push(RenderPlanItem::MaskComposite(MaskCompositePlan {
                        from_level: current_level,
                        mask_v_start: mv_start,
                        mask_v_end: mv_end,
                        mask_src,
                        mask_gradient,
                    }));
                    current_level -= 1;
                }
                // CSS Filter Effects L1 — PushFilter opens an offscreen level;
                // PopFilter composites it onto the parent with filter applied.
                DisplayCommand::PushFilter { filters } => {
                    flush_batch!();
                    filter_stack.push(filters.clone());
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PopFilter => {
                    if let Some(filters) = filter_stack.pop() {
                        flush_batch!();
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        render_plan.push(RenderPlanItem::FilterComposite(FilterCompositePlan {
                            from_level: current_level,
                            filters,
                            comp_v_start,
                        }));
                        current_level -= 1;
                    }
                }
                // CSS Filter Effects L1 §2 — backdrop-filter.
                // Opens a new offscreen level for the element's own content.
                DisplayCommand::PushBackdropFilter { filters, bounds } => {
                    flush_batch!();
                    backdrop_filter_stack.push((filters.clone(), *bounds));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                }
                DisplayCommand::PopBackdropFilter => {
                    if let Some((filters, bounds)) = backdrop_filter_stack.pop() {
                        flush_batch!();
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        let bounds_v_start = composite_vertices.len() as u32;
                        push_bounded_quad(
                            &mut composite_vertices,
                            bounds,
                            surface_w as f32,
                            surface_h as f32,
                            dpr_f32,
                            1.0,
                        );
                        let ordinal = backdrop_ordinal;
                        backdrop_ordinal += 1;
                        render_plan.push(RenderPlanItem::BackdropFilterComposite(
                            BackdropFilterCompositePlan {
                                from_level: current_level,
                                filters,
                                comp_v_start,
                                bounds_v_start,
                                ordinal,
                            },
                        ));
                        current_level -= 1;
                    }
                }
                DisplayCommand::DrawSvgPath { vertices, color } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    for [x, y] in vertices {
                        fill_vertices.push(FillVertex {
                            pos: [x + dx, y + dy],
                            z: 0.0,
                            color: c,
                        });
                    }
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // CSS Positioning L3 §6.3 — position:sticky.
                // Offsets computed above; stack managed here to suppress unused-var warnings.
                DisplayCommand::BeginStickyLayer { flow_rect, top, bottom, left, right } => {
                    if !is_overlay {
                        let sdy = sticky_offset_dy(flow_rect, *top, *bottom, scroll_y, viewport_css_h);
                        let sdx = sticky_offset_dx(flow_rect, *left, *right, scroll_x, viewport_css_w);
                        sticky_stack.push((sdy, sdx));
                    }
                }
                DisplayCommand::EndStickyLayer => {
                    if !is_overlay {
                        sticky_stack.pop();
                    }
                }
                // CSS Masking L1 §5 — PushMaskLayer: open an offscreen layer for mask content.
                // The caller (emit_box) is responsible for ensuring the element content is
                // isolated in the parent layer (e.g. via PushOpacity) before calling this.
                // Mask content renders to the new level; PopMaskLayer applies it to the parent.
                DisplayCommand::PushMaskLayer { rect, mode } => {
                    flush_batch!();
                    mask_layer_stack.push((*rect, *mode));
                    current_level += 1;
                    if level_first.len() <= current_level {
                        level_first.resize(current_level + 1, true);
                    }
                    level_first[current_level] = true;
                }
                // CSS Masking L1 §5 — PopMaskLayer: composite mask layer onto parent.
                // Algorithm:
                //   1. Copy parent layer → scratch (scratch preserves element content).
                //   2. Render pass (REPLACE blend): scratch × mask_value → parent at element rect.
                //      This replaces parent content in the element rect with the masked version.
                DisplayCommand::PopMaskLayer => {
                    flush_batch!();
                    let Some((rect, mode)) = mask_layer_stack.pop() else { continue };
                    let ml_v_start = mask_layer_vertices.len() as u32;
                    // Build a rect quad over the element area. UV = pos / surface_size
                    // so both t_content (scratch, full surface) and t_mask (full surface layer)
                    // are sampled at the same normalised coordinate.
                    let (sw, sh) = (surface_w as f32, surface_h as f32);
                    let scrolled = translate_rect(rect, dx, dy);
                    let (x0, y0) = (scrolled.x, scrolled.y);
                    let (x1, y1) = (scrolled.x + scrolled.width, scrolled.y + scrolled.height);
                    mask_layer_vertices.extend_from_slice(&[
                        MaskVertex { pos: [x0, y0], uv_mask: [x0/sw, y0/sh] },
                        MaskVertex { pos: [x1, y0], uv_mask: [x1/sw, y0/sh] },
                        MaskVertex { pos: [x1, y1], uv_mask: [x1/sw, y1/sh] },
                        MaskVertex { pos: [x0, y0], uv_mask: [x0/sw, y0/sh] },
                        MaskVertex { pos: [x1, y1], uv_mask: [x1/sw, y1/sh] },
                        MaskVertex { pos: [x0, y1], uv_mask: [x0/sw, y1/sh] },
                    ]);
                    let ml_v_end = mask_layer_vertices.len() as u32;
                    render_plan.push(RenderPlanItem::MaskLayerComposite(MaskLayerCompositePlan {
                        from_level: current_level,
                        mode,
                        ml_v_start,
                        ml_v_end,
                    }));
                    current_level -= 1;
                }
                // Scrollbar track + thumb: two fill quads drawn with the current
                // clip/transform stack (parent's, NOT scroll layer's).
                // Colors from `scrollbar-color` (CSS Scrollbars L1 §3).
                DisplayCommand::DrawScrollbar { track_rect, thumb_rect, track_color, thumb_color, .. } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    for (rect, color) in &[(*track_rect, *track_color), (*thumb_rect, *thumb_color)] {
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            translate_rect(*rect, dx, dy),
                            *color,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                    }
                }
                // DevTools box model overlay (7E.3): four semi-transparent layers
                // drawn outside-in. Uses the same fill pipeline as FillRect.
                DisplayCommand::BoxModelOverlay { margin, border, padding, content } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    // Standard DevTools palette (Chrome-matching), ~50% alpha.
                    const MARGIN_COLOR:  [f32; 4] = [0.965, 0.699, 0.420, 0.5]; // #f6b26b
                    const BORDER_COLOR:  [f32; 4] = [1.000, 0.898, 0.600, 0.5]; // #ffe599
                    const PADDING_COLOR: [f32; 4] = [0.576, 0.769, 0.490, 0.5]; // #93c47d
                    const CONTENT_COLOR: [f32; 4] = [0.435, 0.659, 0.863, 0.5]; // #6fa8dc

                    let boxes: &[(Rect, [f32; 4])] = &[
                        (*margin,  MARGIN_COLOR),
                        (*border,  BORDER_COLOR),
                        (*padding, PADDING_COLOR),
                        (*content, CONTENT_COLOR),
                    ];
                    for (rect, color) in boxes {
                        if rect.width <= 0.0 || rect.height <= 0.0 {
                            continue;
                        }
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            translate_rect(*rect, dx, dy),
                            *color,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                    }
                }
                DisplayCommand::PageBreak => {
                    // No-op in on-screen rendering; only meaningful in render_print_pages().
                }
                // CSS Images L4 §4 — cross-fade(A, B, p) two-texture blend.
                // Both `src_a` and `src_b` must already be registered via
                // `register_image`; if either is missing the command is a no-op
                // (matches DrawBackgroundImage convention for unregistered URLs).
                // The quad covers `dest` after scroll translation; both textures
                // sample at the full UV range [0,1]×[0,1] (CSS Images L4 §4.1).
                DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    // Look up both GpuImage entries. Use intrinsic-size key
                    // directly — cross-fade stretches each image to `dest`
                    // through UV sampling, so no CPU resize is needed (object-fit
                    // does not apply to cross-fade per CSS Images L4 §4.1).
                    let Some(gpu_a) = self.images.get(src_a) else { continue };
                    let Some(gpu_b) = self.images.get(src_b) else { continue };
                    let scrolled = translate_rect(*dest, dx, dy);
                    if scrolled.width <= 0.0 || scrolled.height <= 0.0 {
                        continue;
                    }
                    let clamped = progress.clamp(0.0, 1.0);

                    // Per-quad progress uniform (std140-padded to 16 bytes).
                    let params: [f32; 4] = [clamped, 0.0, 0.0, 0.0];
                    let cf_idx = cross_fade_bind_groups.len();
                    let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("cross-fade-ubuf-{cf_idx}")),
                        size: std::mem::size_of_val(&params) as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&ubuf, 0, as_bytes(&params));
                    let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("cross-fade-bg-{cf_idx}")),
                        layout: &self.cross_fade_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&gpu_a.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&gpu_b.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: ubuf.as_entire_binding(),
                            },
                        ],
                    });
                    cross_fade_bind_groups.push(bg);

                    let v_start = cross_fade_vertices.len() as u32;
                    push_cross_fade_quad(&mut cross_fade_vertices, scrolled);
                    let v_count = cross_fade_vertices.len() as u32 - v_start;
                    draw_ops.push(DrawOp::CrossFade {
                        v_start,
                        v_count,
                        cf_batch_idx: cf_idx as u32,
                    });
                }
            }
        }
        flush_batch!();
        let _ = (batch_start, current_scissor); // terminal flush — values not needed after

        // ── Atlas upload (если изменился) ─────────────────────────────────
        if self.atlas.dirty() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                self.atlas.pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.atlas.width()),
                    rows_per_image: Some(self.atlas.height()),
                },
                wgpu::Extent3d {
                    width: self.atlas.width(),
                    height: self.atlas.height(),
                    depth_or_array_layers: 1,
                },
            );
            self.atlas.mark_clean();
        }

        // ── Uniforms ──────────────────────────────────────────────────────
        // Shader делит pos на viewport, чтобы получить clip-space. Surface
        // сконфигурирован в physical pixels, но shader считает в CSS px:
        // viewport = config / scale_factor → 1 CSS px = scale_factor device px.
        // scale_factor=1 — поведение pre-DPR (1:1, обычный 1080p); =2 — 4K с
        // 200% scaling, 16-px CSS текст рендерится на 32 device px.
        // f32 cast терпит небольшую потерю точности — DPR редко > 4.0.
        let dpr = self.scale_factor.max(1e-6) as f32;
        let (dims_w, dims_h) = self.surface_dims();
        let viewport = [
            dims_w as f32 / dpr,
            dims_h as f32 / dpr,
            0.0,
            0.0,
        ];
        self.queue
            .write_buffer(&self.uniform_buffer, 0, as_bytes(&viewport));

        // ── Vertex buffers ────────────────────────────────────────────────
        let fill_vbuf = if fill_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fill-vbuf"),
                size: std::mem::size_of_val(fill_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&fill_vertices));
            Some(buf)
        };
        let circle_vbuf = if circle_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("circle-vbuf"),
                size: std::mem::size_of_val(circle_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&circle_vertices));
            Some(buf)
        };
        let rrect_vbuf = if rrect_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rrect-vbuf"),
                size: std::mem::size_of_val(rrect_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&rrect_vertices));
            Some(buf)
        };
        let text_vbuf = if text_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("text-vbuf"),
                size: std::mem::size_of_val(text_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&text_vertices));
            Some(buf)
        };
        let image_vbuf = if image_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("image-vbuf"),
                size: std::mem::size_of_val(image_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&image_vertices));
            Some(buf)
        };
        let comp_vbuf = if composite_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("comp-vbuf"),
                size: std::mem::size_of_val(composite_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&composite_vertices));
            Some(buf)
        };
        let mask_vbuf = if mask_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mask-vbuf"),
                size: std::mem::size_of_val(mask_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&mask_vertices));
            Some(buf)
        };
        let mask_layer_vbuf = if mask_layer_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mask-layer-vbuf"),
                size: std::mem::size_of_val(mask_layer_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&mask_layer_vertices));
            Some(buf)
        };
        let grad_vbuf = if grad_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grad-vbuf"),
                size: std::mem::size_of_val(grad_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&grad_vertices));
            Some(buf)
        };
        let cross_fade_vbuf = if cross_fade_vertices.is_empty() {
            None
        } else {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cross-fade-vbuf"),
                size: std::mem::size_of_val(cross_fade_vertices.as_slice()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&buf, 0, as_bytes(&cross_fade_vertices));
            Some(buf)
        };
        // One uniform buffer + bind group per gradient draw call (same pattern as image batches).
        let grad_bind_groups: Vec<wgpu::BindGroup> = grad_params
            .iter()
            .enumerate()
            .map(|(i, params)| {
                let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("grad-ubuf-{i}")),
                    size: std::mem::size_of::<GradParamsCpu>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                // SAFETY: GradParamsCpu is #[repr(C)]; casting to bytes is valid.
                self.queue.write_buffer(&ubuf, 0, as_bytes(std::slice::from_ref(params)));
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("grad-bg-{i}")),
                    layout: &self.gradient_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ubuf.as_entire_binding(),
                    }],
                })
            })
            .collect();

        // ── Off-screen textures ───────────────────────────────────────────
        // Blend composites (mode != Normal) also need from_level offscreen layers.
        let max_level = render_plan.iter().fold(0usize, |m, item| match item {
            RenderPlanItem::Draw(b) => m.max(b.target_level),
            RenderPlanItem::Composite(c) => m.max(c.from_level),
            RenderPlanItem::MaskComposite(c) => m.max(c.from_level),
            RenderPlanItem::FilterComposite(c) => m.max(c.from_level),
            RenderPlanItem::BackdropFilterComposite(c) => m.max(c.from_level),
            RenderPlanItem::MaskLayerComposite(c) => m.max(c.from_level),
        });
        if max_level > 0 {
            self.ensure_layer_textures(max_level, surface_w, surface_h);
        }

        // CSS Masking L1 §4 — gradient mask temp textures.
        // Kept alive until after encoder.submit() so GPU commands can safely read them.
        // Each entry corresponds to one MaskComposite plan item with mask_gradient.
        // Populated lazily during the render loop (see MaskComposite handler below).
        let mut temp_grad_textures: Vec<(wgpu::Texture, wgpu::TextureView)> = Vec::new();

        // ── Frame ─────────────────────────────────────────────────────────
        // Windowed: get the next swapchain image from the surface.
        // Headless: create a temporary RGBA8 RENDER_ATTACHMENT|COPY_SRC texture so
        //   render_to_image() can read it back after this call.
        let windowed_frame: Option<wgpu::SurfaceTexture>;
        let headless_tex: Option<wgpu::Texture>;
        let frame_view: wgpu::TextureView;
        if let Some(ref surface) = self.surface {
            let f = surface.get_current_texture()?;
            frame_view = f.texture.create_view(&wgpu::TextureViewDescriptor::default());
            windowed_frame = Some(f);
            headless_tex = None;
        } else {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("headless-frame"),
                size: wgpu::Extent3d {
                    width: surface_w,
                    height: surface_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            frame_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            windowed_frame = None;
            headless_tex = Some(tex);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });

        macro_rules! run_draw_ops {
            ($pass:ident, $start:expr, $end:expr) => {
                for op in &draw_ops[$start..$end] {
                    match op {
                        DrawOp::SetScissor(s) => {
                            if s.is_empty() {
                                $pass.set_scissor_rect(0, 0, 1.min(surface_w), 1.min(surface_h));
                            } else {
                                $pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
                        }
                        DrawOp::Fill { v_start, v_count } => {
                            if let Some(vb) = &fill_vbuf {
                                $pass.set_pipeline(&self.fill_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::Circle { v_start, v_count } => {
                            if let Some(vb) = &circle_vbuf {
                                $pass.set_pipeline(&self.circle_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::RRect { v_start, v_count } => {
                            if let Some(vb) = &rrect_vbuf {
                                $pass.set_pipeline(&self.rrect_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::Text { v_start, v_count } => {
                            if let Some(vb) = &text_vbuf {
                                $pass.set_pipeline(&self.text_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::Image { v_start, v_count, image_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &image_vbuf,
                                image_bind_groups.get(*image_batch_idx as usize),
                            ) {
                                $pass.set_pipeline(&self.image_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_bind_group(1, bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::Gradient { v_start, v_count, grad_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &grad_vbuf,
                                grad_bind_groups.get(*grad_batch_idx as usize),
                            ) {
                                $pass.set_pipeline(&self.gradient_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_bind_group(1, bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                        DrawOp::CrossFade { v_start, v_count, cf_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &cross_fade_vbuf,
                                cross_fade_bind_groups.get(*cf_batch_idx as usize),
                            ) {
                                $pass.set_pipeline(&self.cross_fade_pipeline);
                                $pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                                $pass.set_bind_group(1, bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(..));
                                $pass.draw(*v_start..*v_start + *v_count, 0..1);
                            }
                        }
                    }
                }
            };
        }

        // Per-pass filter param buffers — one per filter/backdrop-filter render pass.
        // Using a single shared buffer caused all passes to see the last write_buffer
        // value (wgpu batches all write_buffer calls before any encoder commands run).
        let mut filter_param_bufs: Vec<wgpu::Buffer> = Vec::new();

        for item in &render_plan {
            match item {
                RenderPlanItem::Draw(batch) => {
                    let target_view = if batch.target_level == 0 {
                        &frame_view
                    } else {
                        &self.layer_textures[batch.target_level - 1].view
                    };
                    let load = match batch.load_op {
                        LoadOpChoice::ClearWhite => wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        LoadOpChoice::ClearTransparent => {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        }
                        LoadOpChoice::Load => wgpu::LoadOp::Load,
                    };
                    // Depth attachment only for the frame surface (level 0).
                    // Off-screen layers don't participate in 3D depth sorting.
                    let depth_attachment = if batch.target_level == 0 {
                        self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations {
                                // Clear depth at frame start (ClearWhite/ClearTransparent);
                                // load otherwise to accumulate depth across same-frame batches
                                // so 3D-sorted elements preserve relative depth ordering.
                                load: if matches!(batch.load_op, LoadOpChoice::Load) {
                                    wgpu::LoadOp::Load
                                } else {
                                    wgpu::LoadOp::Clear(1.0)
                                },
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        })
                    } else {
                        None
                    };
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("draw-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: depth_attachment,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    run_draw_ops!(pass, batch.ops_start, batch.ops_end);
                }
                RenderPlanItem::Composite(comp) => {
                    if let Some(cvb) = &comp_vbuf {
                        // Blend path: non-Normal mode AND parent layer exists.
                        if comp.mode != BlendMode::Normal && comp.from_level > 1 {
                            // Ensure scratch layer before borrowing layer_textures immutably.
                            let dst_layer_idx = comp.from_level - 2;
                            let dst_w = self.layer_textures[dst_layer_idx].width;
                            let dst_h = self.layer_textures[dst_layer_idx].height;
                            self.ensure_scratch_layer(dst_w, dst_h);
                            // Copy dst (parent layer) into scratch before overwriting it.
                            let dst_tex_copy = self.layer_textures[dst_layer_idx].texture.as_image_copy();
                            let scratch_copy = self.scratch_layer.as_ref().unwrap().texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                dst_tex_copy,
                                scratch_copy,
                                wgpu::Extent3d { width: dst_w, height: dst_h, depth_or_array_layers: 1 },
                            );
                            // Write blend mode uniform (u32 mode + 3× u32 padding = 16 bytes).
                            let mode_u32 = blend_mode_to_u32(comp.mode);
                            let uniform_data: [u32; 4] = [mode_u32, 0, 0, 0];
                            self.queue.write_buffer(
                                &self.blend_mode_uniform,
                                0,
                                as_bytes(uniform_data.as_slice()),
                            );
                            // Create per-frame blend bind group (src + scratch + sampler + uniform).
                            let src_view = &self.layer_textures[comp.from_level - 1].view;
                            let scratch_view = &self.scratch_layer.as_ref().unwrap().view;
                            let target_view = &self.layer_textures[comp.from_level - 2].view;
                            let blend_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("blend-bg"),
                                layout: &self.blend_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(src_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(scratch_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 3,
                                        resource: self.blend_mode_uniform.as_entire_binding(),
                                    },
                                ],
                            });
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blend-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.blend_pipeline);
                            pass.set_bind_group(0, &blend_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(comp.comp_v_start..comp.comp_v_start + 6, 0..1);
                        } else {
                            // Normal alpha-blend path (opacity compositing or Normal blend mode).
                            let target_view = if comp.from_level == 1 {
                                &frame_view
                            } else {
                                &self.layer_textures[comp.from_level - 2].view
                            };
                            let src_bg = &self.layer_textures[comp.from_level - 1].bind_group;
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("composite-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.composite_pipeline);
                            pass.set_bind_group(0, src_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(comp.comp_v_start..comp.comp_v_start + 6, 0..1);
                        }
                    }
                }
                // CSS Masking L1 §4 — mask composite.
                // Composites the offscreen element layer onto the parent using the
                // mask as an alpha multiplier (mask-mode: alpha, CSS Masking L1 §6.2).
                RenderPlanItem::MaskComposite(comp) => {
                    let target_view = if comp.from_level == 1 {
                        &frame_view
                    } else if comp.from_level >= 2 {
                        &self.layer_textures[comp.from_level - 2].view
                    } else {
                        continue;
                    };
                    let content_layer_view = &self.layer_textures[comp.from_level - 1].view;

                    // Determine mask texture view: image from cache or rendered gradient.
                    let mask_gpu_image = comp.mask_src.as_ref().and_then(|src| self.images.get(src));
                    let mask_view: Option<&wgpu::TextureView> = if let Some(img) = mask_gpu_image {
                        Some(&img.view)
                    } else if let Some(grad_spec) = &comp.mask_gradient {
                        // Render gradient into a surface-size temp texture and use it as mask.
                        // Gradient rendered in same pixel-coord system as content layer,
                        // so uv_mask = pos/surface (set during plan building) samples correctly.
                        let (grad_params, grad_rect) = match grad_spec.as_ref() {
                            MaskGradientSpec::Linear { params, rect } => (params, rect),
                            MaskGradientSpec::Radial { params, rect } => (params, rect),
                            MaskGradientSpec::Conic  { params, rect } => (params, rect),
                        };
                        let temp_tex = self.device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("mask-grad-tex"),
                            size: wgpu::Extent3d {
                                width: surface_w, height: surface_h, depth_or_array_layers: 1,
                            },
                            mip_level_count: 1, sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: self.surface_format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                 | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        });
                        let temp_view = temp_tex.create_view(&wgpu::TextureViewDescriptor::default());
                        // Write gradient params uniform and build bind group.
                        let grad_ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("mask-grad-ubuf"),
                            size: std::mem::size_of::<GradParamsCpu>() as u64,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        // SAFETY: GradParamsCpu is #[repr(C)].
                        self.queue.write_buffer(&grad_ubuf, 0, as_bytes(std::slice::from_ref(grad_params)));
                        let grad_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("mask-grad-bg"),
                            layout: &self.gradient_bgl,
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: grad_ubuf.as_entire_binding(),
                            }],
                        });
                        // Gradient vertex quad covering the element rect (CSS px coords).
                        let r = grad_rect;
                        let grad_verts: [GradVertex; 6] = [
                            GradVertex { pos: [r.x,           r.y          ], uv: [0.0, 0.0] },
                            GradVertex { pos: [r.x + r.width, r.y          ], uv: [1.0, 0.0] },
                            GradVertex { pos: [r.x + r.width, r.y + r.height], uv: [1.0, 1.0] },
                            GradVertex { pos: [r.x,           r.y          ], uv: [0.0, 0.0] },
                            GradVertex { pos: [r.x + r.width, r.y + r.height], uv: [1.0, 1.0] },
                            GradVertex { pos: [r.x,           r.y + r.height], uv: [0.0, 1.0] },
                        ];
                        let grad_vbuf_m = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("mask-grad-vbuf"),
                            size: std::mem::size_of_val(&grad_verts) as u64,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        self.queue.write_buffer(&grad_vbuf_m, 0, as_bytes(&grad_verts));
                        // Render gradient into temp_tex (cleared to transparent first).
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("mask-grad-render"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &temp_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.gradient_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_bind_group(1, &grad_bg, &[]);
                            pass.set_vertex_buffer(0, grad_vbuf_m.slice(..));
                            pass.draw(0..6, 0..1);
                        }
                        // Store temp texture so it lives until encoder.submit().
                        temp_grad_textures.push((temp_tex, temp_view));
                        Some(&temp_grad_textures.last().unwrap().1)
                    } else {
                        None
                    };

                    if let (Some(mvb), Some(mask_view)) = (&mask_vbuf, mask_view) {
                        // Build per-frame bind group: content layer + mask texture + sampler.
                        let mask_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("mask-composite-bg"),
                            layout: &self.mask_composite_bgl,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(content_layer_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(mask_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                                },
                            ],
                        });
                        let v_count = comp.mask_v_end - comp.mask_v_start;
                        if v_count > 0 {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("mask-composite-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.mask_composite_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_bind_group(1, &mask_bg, &[]);
                            pass.set_vertex_buffer(0, mvb.slice(..));
                            pass.draw(comp.mask_v_start..comp.mask_v_end, 0..1);
                        }
                    } else {
                        // Mask image not registered: fallback — composite content at full opacity.
                        let src_bg = &self.layer_textures[comp.from_level - 1].bind_group;
                        let fallback_verts: [CompositeVertex; 6] = [
                            CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0,  1.0], uv: [1.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha: 1.0 },
                            CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha: 1.0 },
                            CompositeVertex { pos: [-1.0, -1.0], uv: [0.0, 1.0], alpha: 1.0 },
                        ];
                        let fallback_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("mask-fallback-vbuf"),
                            size: std::mem::size_of_val(&fallback_verts) as u64,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        self.queue.write_buffer(&fallback_buf, 0, as_bytes(fallback_verts.as_slice()));
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("mask-fallback-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: target_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.composite_pipeline);
                        pass.set_bind_group(0, src_bg, &[]);
                        pass.set_vertex_buffer(0, fallback_buf.slice(..));
                        pass.draw(0..6, 0..1);
                    }
                }
                // CSS Filter Effects L1 — filter composite.
                // If blur in filter list: two-pass separable Gaussian (H: src→scratch, V: scratch→src).
                // Then color filter pass composites src_level onto parent with ALPHA_BLENDING.
                RenderPlanItem::FilterComposite(plan) => {
                    if plan.from_level == 0 { continue; }
                    let src_layer_idx = plan.from_level - 1;
                    let Some(cvb) = &comp_vbuf else { continue };

                    let blur_sigma = plan.filters.iter().find_map(|f| match f {
                        FilterFn::Blur(s) if *s > 0.0 => Some(*s),
                        _ => None,
                    });

                    if let Some(sigma) = blur_sigma {
                        // Ensure scratch before any immutable borrows of self.
                        let src_w = self.layer_textures[src_layer_idx].width;
                        let src_h = self.layer_textures[src_layer_idx].height;
                        self.ensure_scratch_layer(src_w, src_h);

                        // H pass: src_level → scratch
                        let blur_h = BlurParamsCpu { sigma, direction: 0, _p0: 0, _p1: 0 };
                        self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_h]));
                        let src_view_h = &self.layer_textures[src_layer_idx].view;
                        let scratch_view_h = &self.scratch_layer.as_ref().unwrap().view;
                        let blur_bg_h = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("blur-h-bg"),
                            layout: &self.blur_bgl,
                            entries: &[
                                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view_h) },
                                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                wgpu::BindGroupEntry { binding: 2, resource: self.blur_uniform.as_entire_binding() },
                            ],
                        });
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blur-h-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: scratch_view_h,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.blur_pipeline);
                            pass.set_bind_group(0, &blur_bg_h, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                        }

                        // V pass: scratch → src_level (overwrite with fully blurred result)
                        let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                        self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_v]));
                        let scratch_view_v = &self.scratch_layer.as_ref().unwrap().view;
                        let src_level_view_v = &self.layer_textures[src_layer_idx].view;
                        let blur_bg_v = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("blur-v-bg"),
                            layout: &self.blur_bgl,
                            entries: &[
                                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view_v) },
                                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                wgpu::BindGroupEntry { binding: 2, resource: self.blur_uniform.as_entire_binding() },
                            ],
                        });
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blur-v-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: src_level_view_v,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.blur_pipeline);
                            pass.set_bind_group(0, &blur_bg_v, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                        }
                    }

                    // Color filter pass: src_level → parent (ALPHA_BLENDING).
                    // src_level now has blurred content if blur was applied.
                    let mut entries = [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8];
                    let mut color_count = 0u32;
                    for f in &plan.filters {
                        if !matches!(f, FilterFn::Blur(_)) && (color_count as usize) < 8 {
                            entries[color_count as usize] = filter_fn_to_entry(f);
                            color_count += 1;
                        }
                    }
                    let filter_params = FilterParamsCpu {
                        count: color_count, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries,
                    };
                    let fp_buf = make_filter_param_buf(&self.device, &filter_params);

                    let dst_view = if plan.from_level == 1 {
                        &frame_view
                    } else {
                        &self.layer_textures[plan.from_level - 2].view
                    };
                    let src_view_f = &self.layer_textures[src_layer_idx].view;
                    let filter_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("filter-bg"),
                        layout: &self.filter_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view_f) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                            wgpu::BindGroupEntry { binding: 2, resource: fp_buf.as_entire_binding() },
                        ],
                    });
                    filter_param_bufs.push(fp_buf);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("filter-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: dst_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.filter_pipeline);
                        pass.set_bind_group(0, &filter_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                    }
                }
                // CSS Filter Effects L1 §2 / Compositing §13 — backdrop-filter composite.
                //
                // Execution order:
                //   1. copy parent layer → scratch (GPU texture copy)
                //   2. blur scratch if needed (H: scratch → backdrop_layer, V: backdrop_layer → scratch)
                //   3. blit scratch → parent at bounds with optional color filter (REPLACE blend)
                //   4. composite element layer → parent (ALPHA_BLENDING, same as FilterComposite)
                //
                // Phase 0 limitation: skipped when from_level <= 1 (parent = surface texture,
                // which lacks TEXTURE_BINDING and cannot be used as a copy source).
                RenderPlanItem::BackdropFilterComposite(plan) => {
                    // Need from_level >= 2: parent_idx = from_level - 2 indexes layer_textures.
                    if plan.from_level < 2 { continue; }
                    let Some(cvb) = &comp_vbuf else { continue };

                    let parent_idx = plan.from_level - 2;
                    let parent_w = self.layer_textures[parent_idx].width;
                    let parent_h = self.layer_textures[parent_idx].height;
                    self.ensure_scratch_layer(parent_w, parent_h);
                    self.ensure_backdrop_layer(parent_w, parent_h);
                    // The per-ordinal cache texture is the blit source (always), and on a
                    // cache hit it already holds the previous frame's filtered backdrop.
                    if self.ensure_backdrop_cache_texture(plan.ordinal, parent_w, parent_h) {
                        // A resize discarded the cached pixels — drop the stale hash so it
                        // cannot produce a hit against the fresh (uninitialised) texture.
                        self.backdrop_cache.invalidate(plan.ordinal);
                    }
                    // Cache HIT: the cached texture is unchanged → skip the copy + blur
                    // passes entirely. Disabled cache (`backdrop_frame_hash == None`)
                    // always misses, reproducing the original behaviour.
                    let cache_hit = match backdrop_frame_hash {
                        Some(fh) => self.backdrop_cache.lookup(plan.ordinal, fh),
                        None => false,
                    };

                    let blur_sigma = plan.filters.iter().find_map(|f| match f {
                        FilterFn::Blur(s) if *s > 0.0 => Some(*s),
                        _ => None,
                    });

                    // Ordinals evicted by `store()` whose textures must be freed once the
                    // current element's passes (which borrow the cache map) have ended.
                    let mut evicted_ordinals: Vec<u32> = Vec::new();
                    if !cache_hit {
                        if let Some(sigma) = blur_sigma {
                            // Step 1: copy parent layer → scratch (blur H-pass input).
                            // parent has COPY_SRC, scratch has COPY_DST.
                            let parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            let scratch_copy = self.scratch_layer.as_ref().unwrap().texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                scratch_copy,
                                wgpu::Extent3d { width: parent_w, height: parent_h, depth_or_array_layers: 1 },
                            );

                            // Step 2 H pass: scratch → backdrop_layer (REPLACE).
                            let blur_h = BlurParamsCpu { sigma, direction: 0, _p0: 0, _p1: 0 };
                            self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_h]));
                            let scratch_view_h = &self.scratch_layer.as_ref().unwrap().view;
                            let backdrop_view_h = &self.backdrop_layer.as_ref().unwrap().view;
                            let blur_bg_h = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-h-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view_h) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: self.blur_uniform.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("backdrop-blur-h-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: backdrop_view_h,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(&self.blur_pipeline);
                                pass.set_bind_group(0, &blur_bg_h, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                            // Step 2 V pass: backdrop_layer → CACHE texture (REPLACE).
                            // The blurred result lands in the cache, ready for reuse next frame.
                            let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                            self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_v]));
                            let backdrop_view_v = &self.backdrop_layer.as_ref().unwrap().view;
                            let cache_view_v = &self.backdrop_cache_textures[&plan.ordinal].view;
                            let blur_bg_v = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-v-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(backdrop_view_v) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: self.blur_uniform.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("backdrop-blur-v-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: cache_view_v,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(&self.blur_pipeline);
                                pass.set_bind_group(0, &blur_bg_v, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                        } else {
                            // Filter-only backdrop (no blur): copy parent → cache directly.
                            // parent has COPY_SRC, cache has COPY_DST.
                            let parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            let cache_copy = self.backdrop_cache_textures[&plan.ordinal].texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                cache_copy,
                                wgpu::Extent3d { width: parent_w, height: parent_h, depth_or_array_layers: 1 },
                            );
                        }

                        // Record the freshly produced backdrop in the cache (skipped when
                        // caching is disabled — `backdrop_frame_hash == None`).
                        if let Some(fh) = backdrop_frame_hash {
                            let bytes = parent_w as usize * parent_h as usize * 4;
                            evicted_ordinals = self.backdrop_cache.store(plan.ordinal, fh, bytes);
                        }
                    }

                    // Step 3: blit cache texture → parent at element bounds (REPLACE blend).
                    // Applies color filters (count > 0) or passthrough (count = 0).
                    // Bounded quad ensures only the element's bounds region is overwritten.
                    let mut bd_entries = [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8];
                    let mut bd_color_count = 0u32;
                    for f in &plan.filters {
                        if !matches!(f, FilterFn::Blur(_)) && (bd_color_count as usize) < 8 {
                            bd_entries[bd_color_count as usize] = filter_fn_to_entry(f);
                            bd_color_count += 1;
                        }
                    }
                    let bd_filter_params = FilterParamsCpu {
                        count: bd_color_count, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries: bd_entries,
                    };
                    let bd_fp_buf = make_filter_param_buf(&self.device, &bd_filter_params);
                    let parent_dst_view = &self.layer_textures[parent_idx].view;
                    // Source is the cache texture — holds the blurred (or copied) backdrop,
                    // whether freshly produced this frame or reused from a previous frame.
                    let bd_src_view = &self.backdrop_cache_textures[&plan.ordinal].view;
                    let bd_blit_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("backdrop-blit-bg"),
                        layout: &self.filter_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(bd_src_view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                            wgpu::BindGroupEntry { binding: 2, resource: bd_fp_buf.as_entire_binding() },
                        ],
                    });
                    filter_param_bufs.push(bd_fp_buf);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("backdrop-blit-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_dst_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.backdrop_blit_pipeline);
                        pass.set_bind_group(0, &bd_blit_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.bounds_v_start..plan.bounds_v_start + 6, 0..1);
                    }

                    // Step 4: composite element layer → parent (ALPHA_BLENDING).
                    // This is identical to FilterComposite's color-filter pass but with
                    // count=0 (no element-level filter here; PushFilter handles that separately).
                    let elem_filter_params = FilterParamsCpu {
                        count: 0, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries: [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8],
                    };
                    let elem_fp_buf = make_filter_param_buf(&self.device, &elem_filter_params);
                    let elem_src_view = &self.layer_textures[plan.from_level - 1].view;
                    let elem_filter_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("backdrop-elem-composite-bg"),
                        layout: &self.filter_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(elem_src_view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                            wgpu::BindGroupEntry { binding: 2, resource: elem_fp_buf.as_entire_binding() },
                        ],
                    });
                    filter_param_bufs.push(elem_fp_buf);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("backdrop-elem-composite-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_dst_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.filter_pipeline);
                        pass.set_bind_group(0, &elem_filter_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                    }

                    // Free textures evicted by the cache's budget enforcement now that
                    // the element's passes (which borrowed the cache map) have ended.
                    for ord in evicted_ordinals {
                        self.backdrop_cache_textures.remove(&ord);
                    }
                }

                // CSS Masking L1 §5 — mask-layer composite.
                // Applies the rendered mask layer (from PushMaskLayer/PopMaskLayer)
                // to the parent layer's content.
                //
                // Algorithm:
                //   1. Copy parent layer → scratch (saves element content).
                //   2. REPLACE-blend pass: fragment = scratch × mask_value → parent at rect.
                //
                // Phase 0 limitation: skipped when from_level <= 1 (parent = surface,
                // lacks TEXTURE_BINDING and COPY_SRC).
                RenderPlanItem::MaskLayerComposite(plan) => {
                    if plan.from_level < 2 { continue; }
                    let Some(mlvb) = &mask_layer_vbuf else { continue };
                    let v_count = plan.ml_v_end - plan.ml_v_start;
                    if v_count == 0 { continue; }

                    let parent_idx = plan.from_level - 2;
                    let mask_idx   = plan.from_level - 1;
                    let parent_w = self.layer_textures[parent_idx].width;
                    let parent_h = self.layer_textures[parent_idx].height;
                    self.ensure_scratch_layer(parent_w, parent_h);

                    // Step 1: copy parent → scratch.
                    let parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                    let scratch_copy = self.scratch_layer.as_ref().unwrap().texture.as_image_copy();
                    encoder.copy_texture_to_texture(
                        parent_copy,
                        scratch_copy,
                        wgpu::Extent3d { width: parent_w, height: parent_h, depth_or_array_layers: 1 },
                    );

                    // Step 2: mask-layer composite pass.
                    // Bind group: scratch (content), mask layer (mask), sampler.
                    let scratch_view = &self.scratch_layer.as_ref().unwrap().view;
                    let mask_view    = &self.layer_textures[mask_idx].view;
                    let ml_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("mask-layer-composite-bg"),
                        layout: &self.mask_composite_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(scratch_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(mask_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                            },
                        ],
                    });
                    let parent_view = &self.layer_textures[parent_idx].view;
                    let pipeline = match plan.mode {
                        MaskMode::Alpha     => &self.mask_layer_alpha_pipeline,
                        MaskMode::Luminance => &self.mask_layer_luma_pipeline,
                    };
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("mask-layer-composite-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                        pass.set_bind_group(1, &ml_bg, &[]);
                        pass.set_vertex_buffer(0, mlvb.slice(..));
                        pass.draw(plan.ml_v_start..plan.ml_v_end, 0..1);
                    }
                }
            }
        }

        self.queue.submit([encoder.finish()]);
        if let Some(frame) = windowed_frame {
            frame.present();
        }
        // In headless mode, keep the rendered texture alive for render_to_image().
        self.pending_readback = headless_tex;
        Ok(())
    }

    /// CPU-based rasterization using tiny-skia (feature="cpu-render" only).
    ///
    /// Provides deterministic pixel output on Windows/macOS/Linux for CI testing.
    /// No GPU required; does not depend on wgpu or windowing backend.
    ///
    /// # Errors
    /// Returns `Err` if image creation fails or if display command processing fails.
    #[cfg(feature = "cpu-render")]
    pub fn render_to_image_cpu(
        width: u32,
        height: u32,
        commands: &[crate::DisplayCommand],
        _unused_layers: &[crate::BasicLayer],
        scroll_x: f32,
        scroll_y: f32,
    ) -> Result<lumen_image::Image, Box<dyn std::error::Error>> {
        crate::cpu_raster::rasterize_cpu(width, height, commands, scroll_x, scroll_y)
    }

    /// Render a single `tile_size × tile_size` tile at tile coordinates
    /// `(tile_x, tile_y)` using the CPU rasterizer.
    ///
    /// The display list is culled to only commands that intersect the tile
    /// region before rasterization. Scroll offsets are applied so that the
    /// rendered pixels match what the user would see at that scroll position.
    ///
    /// Tile coordinates are in tile space: CSS pixel `p` is in tile
    /// `(p / tile_size).floor()`. The returned `Image` has dimensions
    /// `tile_size × tile_size` (RGBA8).
    ///
    /// # Errors
    /// Propagates errors from the CPU rasterizer (e.g., invalid display commands).
    // BUG-066: guard was missing; render_tile uses cpu_raster which requires cpu-render.
    #[cfg(feature = "cpu-render")]
    pub fn render_tile(
        content: &[crate::DisplayCommand],
        overlay: &[crate::DisplayCommand],
        scroll_x: f32,
        scroll_y: f32,
        tile_x: i32,
        tile_y: i32,
        tile_size: u32,
    ) -> Result<lumen_image::Image, Box<dyn std::error::Error>> {
        let ts = tile_size as f32;

        // Cull both lanes to commands that touch this tile.
        let culled_content = crate::display_list::cull_display_list(content, tile_x, tile_y, ts);
        let culled_overlay = crate::display_list::cull_display_list(overlay, tile_x, tile_y, ts);

        // Merge both lanes (overlay on top).
        let mut all = culled_content;
        all.extend(culled_overlay);

        // Translate so the tile origin is at (0,0) in the rasterised image.
        // The scroll offset shifts content upward (subtract scroll) so that
        // what is visible at scroll_y appears at y=0.
        let offset_x = scroll_x + tile_x as f32 * ts;
        let offset_y = scroll_y + tile_y as f32 * ts;

        crate::cpu_raster::rasterize_cpu(tile_size, tile_size, &all, offset_x, offset_y)
    }

    // Note: render_to_image for GPU path has different signature:
    // &mut self, commands, scroll_y, scroll_x (3 params after self)

    /// Renders display commands and returns a CPU `Image` (RGBA8).
    ///
    /// Only valid when the renderer was created with [`new_headless`](Self::new_headless).
    /// Calls `render()` internally, then reads back the pixel data from the GPU.
    ///
    /// # Errors
    /// Returns `Err` if called on a windowed renderer, if GPU readback fails, or if
    /// the rendered texture is unavailable.
    pub fn render_to_image(
        &mut self,
        commands: &[crate::DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<lumen_image::Image, Box<dyn std::error::Error>> {
        if self.surface.is_some() {
            return Err(
                "render_to_image() requires headless renderer (created with new_headless())"
                    .into(),
            );
        }

        // Run the render pass; in headless mode, render() stores the texture in pending_readback.
        self.render(commands, &[], scroll_y, scroll_x)
            .map_err(|e| format!("render failed: {e}"))?;

        let tex = self
            .pending_readback
            .take()
            .ok_or("нет pending headless кадра после render()")?;

        let (width, height) = self.surface_dims();

        // Align row stride to COPY_BYTES_PER_ROW_ALIGNMENT (256 bytes).
        let bytes_per_pixel = 4u32; // Rgba8Unorm
        let unpadded_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = unpadded_row.div_ceil(align) * align;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback-buf"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback-encoder"),
            });
        encoder.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);

        // Map the staging buffer synchronously.
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::Wait)?;
        rx.recv()
            .map_err(|_| "readback channel disconnected")?
            .map_err(|e| format!("map_async failed: {e}"))?;

        // Copy pixel rows, stripping the row padding added for alignment.
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        {
            let mapped = slice.get_mapped_range();
            for row in 0..height as usize {
                let start = row * padded_row as usize;
                let end = start + unpadded_row as usize;
                pixels.extend_from_slice(&mapped[start..end]);
            }
        }
        staging.unmap();

        Ok(lumen_image::Image {
            width,
            height,
            format: lumen_image::PixelFormat::Rgba8,
            data: pixels,
            icc_profile: None,
        })
    }

    /// Renders a print display list into one `Image` per page.
    ///
    /// Creates a temporary headless renderer at `page_w × page_h` and calls
    /// `render_to_image` for each page's command slice (separated by `PageBreak`
    /// markers in the input). Returns one `Image` per page, in order.
    ///
    /// Typical usage:
    /// ```ignore
    /// let pages = paginate(&layout_root, &ctx);
    /// let cmds  = build_print_display_list(&pages);
    /// let images = Renderer::render_print_pages(font_bytes, &split_at_page_breaks(cmds), w, h)?;
    /// ```
    ///
    /// # Errors
    /// Returns `Err` if headless renderer initialisation fails or GPU readback fails.
    pub fn render_print_pages(
        font_bytes: Vec<u8>,
        pages: &[Vec<crate::DisplayCommand>],
        page_w: u32,
        page_h: u32,
    ) -> Result<Vec<lumen_image::Image>, Box<dyn std::error::Error>> {
        if pages.is_empty() {
            return Ok(vec![]);
        }
        let mut renderer = Renderer::new_headless(font_bytes, page_w, page_h)?;
        let mut images = Vec::with_capacity(pages.len());
        for page_cmds in pages {
            let img = renderer.render_to_image(page_cmds, 0.0, 0.0)?;
            images.push(img);
        }
        Ok(images)
    }
}

/// CSS Positioning L3 §6.3 — computes the effective `dy` for a sticky-positioned
/// element given its normal-flow Y position (`flow_rect.y`), `scroll_y`, and
/// sticky insets. The element sticks when scrolling would push it past `top` or
/// before the `bottom` limit from the viewport bottom edge.
///
/// Returns the `dy` to apply instead of `-scroll_y` for this layer's content.
fn sticky_offset_dy(
    flow_rect: &lumen_core::geom::Rect,
    top: Option<f32>,
    bottom: Option<f32>,
    scroll_y: f32,
    viewport_h: f32,
) -> f32 {
    let mut dy = -scroll_y;
    // top: clamp screen_y to be at least `top` px from the viewport top.
    if let Some(t) = top {
        let screen_y = flow_rect.y + dy;
        if screen_y < t {
            dy += t - screen_y;
        }
    }
    // bottom: clamp so the element's bottom edge is at most `viewport_h - bottom` from top.
    if let Some(b) = bottom {
        let max_screen_y = viewport_h - b - flow_rect.height;
        let actual_screen_y = flow_rect.y + dy;
        if actual_screen_y > max_screen_y {
            dy -= actual_screen_y - max_screen_y;
        }
    }
    dy
}

/// CSS Positioning L3 §6.3 — same as `sticky_offset_dy` but for the X axis.
fn sticky_offset_dx(
    flow_rect: &lumen_core::geom::Rect,
    left: Option<f32>,
    right: Option<f32>,
    scroll_x: f32,
    viewport_w: f32,
) -> f32 {
    let mut dx = -scroll_x;
    if let Some(l) = left {
        let screen_x = flow_rect.x + dx;
        if screen_x < l {
            dx += l - screen_x;
        }
    }
    if let Some(r) = right {
        let max_screen_x = viewport_w - r - flow_rect.width;
        let actual_screen_x = flow_rect.x + dx;
        if actual_screen_x > max_screen_x {
            dx -= actual_screen_x - max_screen_x;
        }
    }
    dx
}

/// Сдвиг rect-а по Y (CSS px). Используется в `render` для применения
/// scroll-offset-а к page-полосе display list-а; overlay-полоса получает
/// `dy = 0`. Без mutation — Rect: Copy.
fn translate_rect(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

/// Применяет 2D-аффинную матрицу к `pos` вершинам в диапазоне `verts`.
/// CSS Transforms L1 §13 forward-применение: каждая вершина (x,y) переходит
/// в (a·x+c·y+e, b·x+d·y+f), где a..f — 6 компонент 2D affine части Mat4.
/// Z/W колонки игнорируются (Phase 0 — только 2D трансформы).
///
/// Каждый из FillVertex / TextVertex / ImageVertex имеет одинаковый layout
/// в начале (`pos: [f32; 2]`); функция параметризована типом V и читает
/// только `pos`-смещение через trait `VertexPos`.
trait VertexPos {
    fn pos_mut(&mut self) -> &mut [f32; 2];
    /// Set CSS depth in pixels (positive = closer to viewer). Default no-op for vertex
    /// types without a depth field; FillVertex overrides to enable GPU depth testing.
    fn set_depth(&mut self, _z: f32) {}
}

impl VertexPos for FillVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for TextVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for ImageVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for CircleVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for GradVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for RRectVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

fn apply_affine_to_grad_verts(verts: &mut [GradVertex], m: &Mat4) {
    apply_affine_to_verts(verts, m);
}

/// Применяет матрицу `PushTransform` к pos-полям вершин.
///
/// 2D affine (`m.is_2d_affine()`) — быстрый путь: x' = a·x + c·y + e
/// (побитово идентично старому 2D-конвейеру; z остаётся 0.0 по умолчанию).
/// Иначе (CSS Transforms L2: 3D rotate/translate/scale, `perspective()`,
/// `matrix3d`) — полная 4×4 проекция с перспективным делением через
/// `Mat4::project_point_z`: возвращает (x', y', z'), где z' сохраняется
/// через `VertexPos::set_depth` для GPU depth testing.
/// FillVertex/TextVertex/ImageVertex/RRectVertex реализуют set_depth и
/// получают корректную глубину для cross-type occlusion под preserve-3d;
/// CircleVertex и GradVertex используют no-op (depth=0.0, painter's order).
fn apply_affine_to_verts<V: VertexPos>(verts: &mut [V], m: &Mat4) {
    if m.is_2d_affine() {
        let a = m.0[0];
        let b = m.0[1];
        let c = m.0[4];
        let d = m.0[5];
        let e = m.0[12];
        let f = m.0[13];
        for v in verts {
            let p = v.pos_mut();
            let x = p[0];
            let y = p[1];
            p[0] = a * x + c * y + e;
            p[1] = b * x + d * y + f;
            // z stays 0.0 (2D affine: depth=0.5 in shader, painter's order applies)
        }
    } else {
        // CSS Transforms L2 — 3D/perspective transform: preserve z for depth testing.
        for v in verts {
            let (x, y, z) = {
                let p = v.pos_mut();
                m.project_point_z(p[0], p[1], 0.0)
            };
            {
                let p = v.pos_mut();
                p[0] = x;
                p[1] = y;
            }
            v.set_depth(z);
        }
    }
}

/// Эмитирует квад для SDF-круга.
///
/// Quad расширяется на 0.5 CSS-px в каждую сторону от `rect`, чтобы шейдер
/// мог рисовать внешнюю половину 1px AA-полосы (Skia-compatible linear AA).
/// UV = ±1 соответствует CSS_radius + 0.5 px от центра.
fn push_circle_quad(out: &mut Vec<CircleVertex>, rect: Rect, color: [f32; 4]) {
    let radius_px = rect.width * 0.5;
    let x0 = rect.x - 0.5;
    let y0 = rect.y - 0.5;
    let x1 = rect.x + rect.width + 0.5;
    let y1 = rect.y + rect.height + 0.5;
    out.extend_from_slice(&[
        CircleVertex { pos: [x0, y0], uv: [-1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y0], uv: [ 1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y1], uv: [ 1.0,  1.0], color, radius_px },
        CircleVertex { pos: [x0, y0], uv: [-1.0, -1.0], color, radius_px },
        CircleVertex { pos: [x1, y1], uv: [ 1.0,  1.0], color, radius_px },
        CircleVertex { pos: [x0, y1], uv: [-1.0,  1.0], color, radius_px },
    ]);
}

/// Применяет 2D аффинное преобразование к pos-полям CircleVertex.
/// UV-координаты не затрагиваются — они описывают относительную позицию
/// внутри квада, а не мировые координаты.
fn apply_affine_to_circle_verts(verts: &mut [CircleVertex], m: &Mat4) {
    apply_affine_to_verts(verts, m);
}

/// Emits 6 `RRectVertex` (two triangles) for a rounded rect quad.
/// Per-vertex `center`, `half_size`, and `radii` are constant across the quad so
/// the fragment shader can evaluate the SDF at each fragment position.
fn push_rrect_quad(out: &mut Vec<RRectVertex>, rect: Rect, color: [f32; 4], radii: CornerRadii) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    let half_size = [rect.width * 0.5, rect.height * 0.5];
    let radii_x = [radii.tl,   radii.tr,   radii.br,   radii.bl  ];
    let radii_y = [radii.tl_y, radii.tr_y, radii.br_y, radii.bl_y];
    let v = |px: f32, py: f32| RRectVertex { pos: [px, py], z: 0.0, color, center, half_size, radii_x, radii_y };
    out.extend_from_slice(&[
        v(x0, y0), v(x1, y0), v(x1, y1),
        v(x0, y0), v(x1, y1), v(x0, y1),
    ]);
}

/// Applies a `PushTransform` matrix to `RRectVertex::pos` AND `center` fields.
/// `half_size` and `radii` are scale-invariant for Phase 0 (no rotation/scale transforms on layout boxes).
///
/// 2D affine — fast path (z stays 0); 3D/perspective — `Mat4::project_point_z`
/// on pos (writing the projected z into `RRectVertex.z` for GPU depth testing
/// under CSS Transforms L2 `preserve-3d`) and `Mat4::project_point` on center
/// (best-effort: the SDF `half_size`/`radii` stay unprojected, so a rounded
/// rect under perspective keeps uniform corner radii — acceptable Phase-0
/// approximation, same as the no-rotation note above).
fn apply_affine_to_rrect_verts(verts: &mut [RRectVertex], m: &Mat4) {
    if m.is_2d_affine() {
        for v in verts {
            let [px, py] = v.pos;
            v.pos = [
                m.0[0] * px + m.0[4] * py + m.0[12],
                m.0[1] * px + m.0[5] * py + m.0[13],
            ];
            let [cx, cy] = v.center;
            v.center = [
                m.0[0] * cx + m.0[4] * cy + m.0[12],
                m.0[1] * cx + m.0[5] * cy + m.0[13],
            ];
            // z stays unchanged (2D affine: depth=0.5 in shader, painter's order applies)
        }
    } else {
        for v in verts {
            // 3D/perspective: preserve z for cross-type depth testing.
            let (px, py, pz) = m.project_point_z(v.pos[0], v.pos[1], 0.0);
            v.pos = [px, py];
            v.z = pz;
            let (cx, cy) = m.project_point(v.center[0], v.center[1], 0.0);
            v.center = [cx, cy];
        }
    }
}

/// Emits tessellated triangle fan for one border corner arc (quarter-annulus).
/// `center`   = pivot point of the arc (corner center of the rounded rect).
/// `outer_r`  = outer radius (= border-radius value).
/// `inner_r`  = inner radius (= outer_r - border_width, or 0 if border fills the corner).
/// `start_deg`/`end_deg` = sweep in degrees (screen Y-down, clockwise).
/// `color`    = fill color from the adjacent border side.
///
/// Uses 8 segments for smooth Phase 0 quality. Each segment is two triangles
/// forming an annular sector quad.
fn emit_border_arc(
    out: &mut Vec<FillVertex>,
    center: [f32; 2],
    outer_r: f32,
    inner_r: f32,
    start_deg: f32,
    end_deg: f32,
    color: [f32; 4],
) {
    const N: u32 = 8;
    let step = (end_deg - start_deg) / N as f32;
    let [cx, cy] = center;
    for i in 0..N {
        let a0 = (start_deg + i as f32 * step).to_radians();
        let a1 = (start_deg + (i + 1) as f32 * step).to_radians();
        let (s0, c0) = (a0.sin(), a0.cos());
        let (s1, c1) = (a1.sin(), a1.cos());
        // Outer arc vertices.
        let po0 = [cx + outer_r * c0, cy + outer_r * s0];
        let po1 = [cx + outer_r * c1, cy + outer_r * s1];
        // Inner arc vertices (or center if inner_r == 0).
        let pi0 = [cx + inner_r * c0, cy + inner_r * s0];
        let pi1 = [cx + inner_r * c1, cy + inner_r * s1];
        out.extend_from_slice(&[
            FillVertex { pos: po0, z: 0.0, color },
            FillVertex { pos: po1, z: 0.0, color },
            FillVertex { pos: pi1, z: 0.0, color },
            FillVertex { pos: po0, z: 0.0, color },
            FillVertex { pos: pi1, z: 0.0, color },
            FillVertex { pos: pi0, z: 0.0, color },
        ]);
    }
}

/// CSS Images L3 §3.3 — push 6 GradVertex (2 triangles) for `rect`.
/// UV is baked: TL=(0,0), TR=(1,0), BL=(0,1), BR=(1,1).
fn push_grad_quad(out: &mut Vec<GradVertex>, rect: Rect) {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    out.extend_from_slice(&[
        GradVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        GradVertex { pos: [x1, y0], uv: [1.0, 0.0] },
        GradVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        GradVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        GradVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        GradVertex { pos: [x0, y1], uv: [0.0, 1.0] },
    ]);
}

/// CSS Images L3 §3.3 — resolve `GradientStop` positions to normalized [0,1].
///
/// CSS spec: if first/last stop position is unspecified, default to 0/100%.
/// Runs of unspecified positions between explicit ones are evenly distributed.
/// `line_len`: pixel length of gradient line (for `Length::Px` stops).
fn resolve_gradient_stops(stops: &[GradientStop], line_len: f32) -> Vec<(f32, [f32; 4])> {
    if stops.is_empty() {
        return vec![];
    }
    let n = stops.len();
    let mut positions: Vec<Option<f32>> = stops
        .iter()
        .map(|s| {
            s.position.as_ref().map(|l| match l {
                Length::Percent(p) => p / 100.0,
                Length::Px(v) if line_len > 0.0 => v / line_len,
                _ => 0.0,
            })
        })
        .collect();
    if positions[0].is_none() {
        positions[0] = Some(0.0);
    }
    if positions[n - 1].is_none() {
        positions[n - 1] = Some(1.0);
    }
    // Distribute runs of None between two explicit positions.
    let mut i = 0;
    while i < n {
        if positions[i].is_some() {
            i += 1;
            continue;
        }
        let lo_i = i - 1;
        let lo_pos = positions[lo_i].unwrap_or(0.0);
        let mut hi_i = i + 1;
        while hi_i < n && positions[hi_i].is_none() {
            hi_i += 1;
        }
        let hi_pos = positions[hi_i.min(n - 1)].unwrap_or(1.0);
        let gap = (hi_i - lo_i) as f32;
        for (offset, pos) in positions[i..hi_i].iter_mut().enumerate() {
            let t = (i + offset - lo_i) as f32 / gap;
            *pos = Some(lo_pos + (hi_pos - lo_pos) * t);
        }
        i = hi_i;
    }
    stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let pos = positions[i].unwrap_or(0.0);
            let c = s.color;
            let col = [
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                c.a as f32 / 255.0,
            ];
            (pos, col)
        })
        .collect()
}

/// CSS Images L3 §3.4 — compute linear gradient line endpoints in UV [0,1] space.
///
/// Returns (start_uv, end_uv) such that `t = dot(uv-start, end-start)/|end-start|²`
/// gives t=0 at the start-color edge and t=1 at the end-color edge.
///
/// CSS angle convention: 0° = "to top", 90° = "to right", 180° = "to bottom".
/// Box dimensions `w`×`h` in CSS pixels.
fn linear_gradient_uv_endpoints(w: f32, h: f32, angle_deg: f32) -> ([f32; 2], [f32; 2]) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0, 0.5], [1.0, 0.5]);
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos(); // negative because CSS y grows down
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([0.5, 0.5], [0.5, 0.5]);
    }
    let cx = w / 2.0;
    let cy = h / 2.0;
    let sx = (cx - dx * half_len) / w;
    let sy = (cy - dy * half_len) / h;
    let ex = (cx + dx * half_len) / w;
    let ey = (cy + dy * half_len) / h;
    ([sx, sy], [ex, ey])
}

/// CSS Images L3 §3.5 — compute radial gradient center + semi-axes in UV [0,1] space.
///
/// Returns (center_uv, semi_axes_uv) where semi-axes are "farthest-corner" sized:
/// rx = max(cx_frac, 1-cx_frac), ry = max(cy_frac, 1-cy_frac).
fn radial_gradient_uv_params(cx_pct: f32, cy_pct: f32) -> ([f32; 2], [f32; 2]) {
    let rx = cx_pct.max(1.0 - cx_pct).max(1e-6);
    let ry = cy_pct.max(1.0 - cy_pct).max(1e-6);
    ([cx_pct, cy_pct], [rx, ry])
}

/// Build a `GradParamsCpu` uniform from resolved stops + pre-computed UV params.
///
/// `param0` is used by the conic gradient (kind = 2) to pass the starting
/// angle in radians (0 = top, clockwise); for linear/radial it is unused.
fn build_grad_params(
    resolved: &[(f32, [f32; 4])],
    p0: [f32; 2],
    p1: [f32; 2],
    kind: u32,
    repeating: bool,
    param0: f32,
) -> GradParamsCpu {
    let n = resolved.len().min(16);
    let zero_stop = GradStopCpu { color: [0.0; 4], pos: 0.0, _p0: 0.0, _p1: 0.0, _p2: 0.0 };
    let mut stops = [zero_stop; 16];
    for (i, &(pos, col)) in resolved.iter().take(16).enumerate() {
        stops[i] = GradStopCpu { color: col, pos, _p0: 0.0, _p1: 0.0, _p2: 0.0 };
    }
    GradParamsCpu {
        p0,
        p1,
        n_stops: n as u32,
        kind,
        repeating: if repeating { 1 } else { 0 },
        param0,
        stops,
    }
}

fn push_fill_quad(out: &mut Vec<FillVertex>, rect: Rect, color: [f32; 4]) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    out.extend_from_slice(&[
        FillVertex { pos: [x0, y0], z: 0.0, color },
        FillVertex { pos: [x1, y0], z: 0.0, color },
        FillVertex { pos: [x1, y1], z: 0.0, color },
        FillVertex { pos: [x0, y0], z: 0.0, color },
        FillVertex { pos: [x1, y1], z: 0.0, color },
        FillVertex { pos: [x0, y1], z: 0.0, color },
    ]);
}

fn push_image_quad(
    out: &mut Vec<ImageVertex>,
    rect: Rect,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    alpha: f32,
) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let [u0, v0] = uv_min;
    let [u1, v1] = uv_max;
    out.extend_from_slice(&[
        ImageVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y0], z: 0.0, uv: [u1, v0], alpha },
        ImageVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y1], z: 0.0, uv: [u0, v1], alpha },
    ]);
}

/// CSS Images L4 §4 — emit one cross-fade quad covering `rect` with UV
/// `[0,0]→[1,1]`. Vertex order matches `push_image_quad` (two triangles,
/// CCW in window space) so the resulting list runs through the
/// `cross_fade_pipeline` without further reordering.
fn push_cross_fade_quad(out: &mut Vec<CrossFadeVertex>, rect: Rect) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    out.extend_from_slice(&[
        CrossFadeVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        CrossFadeVertex { pos: [x1, y0], uv: [1.0, 0.0] },
        CrossFadeVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        CrossFadeVertex { pos: [x0, y0], uv: [0.0, 0.0] },
        CrossFadeVertex { pos: [x1, y1], uv: [1.0, 1.0] },
        CrossFadeVertex { pos: [x0, y1], uv: [0.0, 1.0] },
    ]);
}

fn push_composite_quad(out: &mut Vec<CompositeVertex>, alpha: f32) {
    out.extend_from_slice(&[
        CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0,  1.0], uv: [1.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha },
        CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha },
        CompositeVertex { pos: [-1.0, -1.0], uv: [0.0, 1.0], alpha },
    ]);
}

/// Pushes 6 vertices for a quad covering only `bounds` (in CSS px) in screen
/// space, sampling from the corresponding UV region of the source texture.
///
/// NDC x = css_x / vw * 2 - 1; NDC y = 1 - css_y / vh * 2 (Y flipped).
/// UV  x = css_x / vw;         UV  y = css_y / vh.
/// `vw = surf_w / dpr`, `vh = surf_h / dpr`.
fn push_bounded_quad(
    out: &mut Vec<CompositeVertex>,
    bounds: lumen_core::geom::Rect,
    surf_w: f32,
    surf_h: f32,
    dpr: f32,
    alpha: f32,
) {
    let vw = surf_w / dpr;
    let vh = surf_h / dpr;
    let x0 = bounds.x / vw * 2.0 - 1.0;
    let x1 = (bounds.x + bounds.width) / vw * 2.0 - 1.0;
    let y0 = 1.0 - bounds.y / vh * 2.0;
    let y1 = 1.0 - (bounds.y + bounds.height) / vh * 2.0;
    let u0 = bounds.x / vw;
    let u1 = (bounds.x + bounds.width) / vw;
    let v0 = bounds.y / vh;
    let v1 = (bounds.y + bounds.height) / vh;
    out.extend_from_slice(&[
        CompositeVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [x1, y0], uv: [u1, v0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        CompositeVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        CompositeVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        CompositeVertex { pos: [x0, y1], uv: [u0, v1], alpha },
    ]);
}

/// Конвертирует декодированное изображение в плотный `Rgba8Unorm`-буфер.
/// Gray → серый × 3, alpha = 255. GrayA → серый × 3, alpha из канала.
/// Rgb → opaque (alpha = 255). Rgba — копия.
fn convert_to_rgba(image: &Image) -> Vec<u8> {
    let pixel_count = (image.width as usize) * (image.height as usize);
    let mut out = Vec::with_capacity(pixel_count * 4);
    match image.format {
        PixelFormat::Gray8 => {
            for &g in &image.data {
                out.extend_from_slice(&[g, g, g, 255]);
            }
        }
        PixelFormat::GrayAlpha8 => {
            for pair in image.data.chunks_exact(2) {
                let g = pair[0];
                let a = pair[1];
                out.extend_from_slice(&[g, g, g, a]);
            }
        }
        PixelFormat::Rgb8 => {
            for triple in image.data.chunks_exact(3) {
                out.extend_from_slice(&[triple[0], triple[1], triple[2], 255]);
            }
        }
        PixelFormat::Rgba8 => {
            out.extend_from_slice(&image.data);
        }
    }
    out
}

/// CSS Fonts L4 §7 + OpenType spec — нормализует user-space variation axes
/// в per-fvar-axis normalized coords `[-1.0, 1.0]`, затем применяет avar.
///
/// Возвращает пустой Vec для non-variable fonts (нет таблицы `fvar`) или
/// если `axes` пустой — renderer тогда использует default-instance.
fn normalize_variation_axes(face: &ParsedFace<'_>, axes: &[([u8; 4], f32)]) -> Vec<f32> {
    if axes.is_empty() {
        return Vec::new();
    }
    let fvar = match face.font.fvar() {
        Ok(f) if f.is_variable() => f,
        _ => return Vec::new(),
    };
    let avar = face.font.avar().unwrap_or_default();
    let mut coords = Vec::with_capacity(fvar.axes.len());
    for (axis_idx, axis) in fvar.axes.iter().enumerate() {
        let user_val = axes
            .iter()
            .find(|(tag, _)| tag == &axis.tag)
            .map_or(axis.default, |(_, v)| *v);
        let clamped = axis.clamp(user_val);
        let linear = if (clamped - axis.default).abs() < f32::EPSILON {
            0.0
        } else if clamped < axis.default {
            let range = axis.default - axis.min;
            if range < f32::EPSILON { 0.0 } else { (clamped - axis.default) / range }
        } else {
            let range = axis.max - axis.default;
            if range < f32::EPSILON { 0.0 } else { (clamped - axis.default) / range }
        };
        coords.push(avar.normalize(axis_idx, linear));
    }
    // CSS Fonts L4 §7.12: opsz injected by display_list builder into font_variation_axes
    // when font-optical-sizing: auto (default). normalize_variation_axes handles it here
    // like any other axis — no special case needed.
    coords
}

#[allow(clippy::too_many_arguments)]
fn push_text_glyphs(
    out: &mut Vec<TextVertex>,
    rect: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    primary_face_id: usize,
    parsed: &[Option<ParsedFace<'_>>],
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    font_variation_axes: &[([u8; 4], f32)],
    tab_size: f32,
) {
    // Multi-size atlas: подбираем bin под font_size, растеризируем глифы
    // на этом bin. Display масштаб = font_size / size_bin — если font_size
    // совпал с bin-ом (12/16/24/32/...) — масштаба нет, текст резкий.
    let size_bin = size_bin_for(font_size);
    let display_scale = font_size / size_bin as f32;

    // Baseline: ascent / (ascent − descent) primary face-а. Для Inter ≈ 0.80.
    // Используем primary для всех глифов в run-е — иначе при смешивании
    // face-ов символы прыгали бы по вертикали.
    let primary = parsed[primary_face_id]
        .as_ref()
        .expect("primary face must be parsed by caller");
    let ascent_ratio = primary.hhea.ascent as f32
        / (primary.hhea.ascent as f32 - primary.hhea.descent as f32);
    let baseline_y = rect.y + font_size * ascent_ratio;

    // Per-char cache на длительность одного DrawText: одни и те же символы
    // в строке («the the the») не нужно пробовать через все face-ы каждый раз.
    let mut char_face_cache: HashMap<char, (usize, u16)> = HashMap::new();
    // Normalized variation coords per face_id — лениво вычисляется при первом
    // обращении к данному face. Нормализация требует fvar+avar из шрифта.
    let mut norm_coords_cache: HashMap<usize, Vec<f32>> = HashMap::new();

    let mut cursor_x = rect.x;
    for ch in text.chars() {
        // CSS Text L3 §10.1 — tab character advances by tab_size pixels.
        if ch == '\t' && tab_size > 0.0 {
            cursor_x += tab_size;
            continue;
        }
        let (face_id, glyph_id) = *char_face_cache
            .entry(ch)
            .or_insert_with(|| pick_face_for_codepoint(ch as u32, primary_face_id, parsed));
        let face = parsed[face_id]
            .as_ref()
            .expect("pick_face_for_codepoint вернул face_id с valid parsed face");
        let advance_scale = font_size / face.head.units_per_em as f32;
        let coords = norm_coords_cache
            .entry(face_id)
            .or_insert_with(|| normalize_variation_axes(face, font_variation_axes));
        let cached_glyph = ensure_glyph(
            cached,
            atlas,
            &face.font,
            &face.hmtx,
            face.head.units_per_em,
            face_id,
            glyph_id,
            size_bin,
            coords,
        );

        if let Some(g) = cached_glyph {
            let bm_left = g.left * display_scale;
            let bm_top = g.top * display_scale;
            let bm_w = g.entry.width as f32 * display_scale;
            let bm_h = g.entry.height as f32 * display_scale;
            let x0 = cursor_x + bm_left;
            let y0 = baseline_y - bm_top;
            let x1 = x0 + bm_w;
            let y1 = y0 + bm_h;
            let u0 = g.entry.atlas_x as f32 / ATLAS_DIM as f32;
            let v0 = g.entry.atlas_y as f32 / ATLAS_DIM as f32;
            let u1 = (g.entry.atlas_x + g.entry.width) as f32 / ATLAS_DIM as f32;
            let v1 = (g.entry.atlas_y + g.entry.height) as f32 / ATLAS_DIM as f32;
            out.extend_from_slice(&[
                TextVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], color },
                TextVertex { pos: [x1, y0], z: 0.0, uv: [u1, v0], color },
                TextVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], color },
                TextVertex { pos: [x0, y0], z: 0.0, uv: [u0, v0], color },
                TextVertex { pos: [x1, y1], z: 0.0, uv: [u1, v1], color },
                TextVertex { pos: [x0, y1], z: 0.0, uv: [u0, v1], color },
            ]);

            cursor_x += g.advance_native as f32 * advance_scale;
        } else {
            // Глиф не отрисовался (composite-fallback, empty или нет места
            // в атласе). Двигаем cursor на advance из выбранного face-а.
            if let Some(adv) = face.hmtx.advance_width(glyph_id) {
                cursor_x += adv as f32 * advance_scale;
            }
        }
    }
}

/// CSS Fonts L4 §5.3 — for each character cascade. Сначала пробуем primary
/// face; если `cmap.glyph_index` возвращает None или Some(0) (= .notdef) —
/// обходим остальные loaded faces. Если ни у кого нет — возвращаем
/// `(primary, 0)` (отрисовать .notdef из primary).
fn pick_face_for_codepoint(
    cp: u32,
    primary_face_id: usize,
    parsed: &[Option<ParsedFace<'_>>],
) -> (usize, u16) {
    if let Some(p) = parsed.get(primary_face_id).and_then(|x| x.as_ref())
        && let Some(gid) = p.cmap.glyph_index(cp).filter(|&g| g != 0)
    {
        return (primary_face_id, gid);
    }
    for (idx, opt) in parsed.iter().enumerate() {
        if idx == primary_face_id {
            continue;
        }
        if let Some(p) = opt.as_ref()
            && let Some(gid) = p.cmap.glyph_index(cp).filter(|&g| g != 0)
        {
            return (idx, gid);
        }
    }
    (primary_face_id, 0)
}

#[allow(clippy::too_many_arguments)]
fn ensure_glyph(
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    atlas: &mut GlyphAtlas,
    font: &Font,
    hmtx: &Hmtx,
    units_per_em: u16,
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords: &[f32],
) -> Option<CachedGlyph> {
    let key = atlas_key(face_id, glyph_id, size_bin, AtlasKey::hash_coords(coords));
    if let Some(&entry) = cached.get(&key) {
        return entry;
    }

    let result = rasterize_and_insert(atlas, font, hmtx, units_per_em, key, coords);
    cached.insert(key, result);
    result
}

fn rasterize_and_insert(
    atlas: &mut GlyphAtlas,
    font: &Font,
    hmtx: &Hmtx,
    units_per_em: u16,
    key: AtlasKey,
    coords: &[f32],
) -> Option<CachedGlyph> {
    // `glyph_resolved_with_coords` разворачивает composite в Simple
    // рекурсивно и применяет gvar deltas в указанной точке пространства
    // осей. Пустой coords (default-instance) → short-circuit на путь
    // `glyph_resolved` (для non-VF шрифтов или CSS без
    // `font-variation-settings`).
    let glyph = font.glyph_resolved_with_coords(key.glyph_id, coords).ok().flatten()?;
    if !matches!(glyph.outline, Outline::Simple(_)) {
        return None;
    }
    let raster = Rasterizer::new(f32::from(key.size_bin), units_per_em);
    let bitmap: Bitmap = raster.rasterize(&glyph)?;
    let entry = atlas.insert(key, &bitmap)?;
    // HVAR delta applied: for variable fonts, advance width varies per axis instance.
    // Font::advance_width_varied falls back to hmtx base when HVAR is absent.
    let advance_native = font.advance_width_varied(key.glyph_id, hmtx, coords);
    Some(CachedGlyph {
        entry,
        left: bitmap.left,
        top: bitmap.top,
        advance_native,
    })
}

fn color_to_array(c: &Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

/// Scissor rect для wgpu в device pixels — все 4 компоненты u32 (× 16-битных,
/// но wgpu принимает u32). `set_scissor_rect(x, y, w, h)` обрезает все
/// последующие fragments в pass-е координатами окна. Пустой scissor
/// (`width=0` или `height=0`) запрещён wgpu и в нашем коде кодируется как
/// «ничего не рисуем» — caller проверяет `is_empty()` и пропускает draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeviceScissor {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DeviceScissor {
    /// Полный фрейм — scissor = вся область surface. wgpu reset = установить
    /// scissor в (0,0,W,H) перед draw.
    pub(crate) fn full(surface_w: u32, surface_h: u32) -> Self {
        Self { x: 0, y: 0, width: surface_w, height: surface_h }
    }

    /// Пустой scissor нельзя задать в wgpu — caller обязан проверить и
    /// пропустить draw. Возвращается из `from_css`, когда clip-rect пуст
    /// (после intersection всё схлопнулось до 0).
    pub(crate) fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Пересечение двух прямоугольников в CSS-px (origin-top-left). Пустое
/// пересечение представляется как `Rect { width: 0.0, height: 0.0 }` —
/// `is_empty_rect` это распознаёт. Используется для combine-логики стека
/// `PushClipRect` (новый scissor = пересечение с текущим), CSS Masking L1 §3.
pub(crate) fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    if x1 <= x0 || y1 <= y0 {
        Rect::new(x0, y0, 0.0, 0.0)
    } else {
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// Активный blend mode из стека (CSS Compositing & Blending L1 §5): топ стека.
/// Пустой стек = `BlendMode::Normal` (стандарт; источник без blend-group).
#[allow(dead_code)]
pub(crate) fn current_blend_mode(blend_mode_stack: &[BlendMode]) -> BlendMode {
    blend_mode_stack.last().copied().unwrap_or(BlendMode::Normal)
}

/// Маппинг `BlendMode` в u32 для WGSL-uniform `blend_mode` в `BLEND_SHADER_SRC`.
/// Значение 0 (Normal) в теории не должно попасть в blend-pipeline (guard
/// `mode != Normal` в compositing path), но обработано как identity для устойчивости.
pub(crate) fn blend_mode_to_u32(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal      => 0,
        BlendMode::Multiply    => 1,
        BlendMode::Screen      => 2,
        BlendMode::Overlay     => 3,
        BlendMode::Darken      => 4,
        BlendMode::Lighten     => 5,
        BlendMode::ColorDodge  => 6,
        BlendMode::ColorBurn   => 7,
        BlendMode::HardLight   => 8,
        BlendMode::SoftLight   => 9,
        BlendMode::Difference  => 10,
        BlendMode::Exclusion   => 11,
        BlendMode::Hue         => 12,
        BlendMode::Saturation  => 13,
        BlendMode::Color       => 14,
        BlendMode::Luminosity  => 15,
        BlendMode::PlusLighter => 16,
    }
}

/// Применяет alpha-multiplier к RGBA-вершине: `color.a *= alpha`. Используется
/// для fill / text вершин перед записью в vbuf. `apply_alpha(c, 1.0) == c`
/// (no-op для opacity:1 — общий путь).
pub(crate) fn apply_alpha_to_color(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

/// Разбивает полосу длиной `total_length` на серию dash-сегментов
/// `(offset, length)` по pattern-у `(dash_len, gap_len)`. Совпадает с
/// Chrome/Edge (Skia): `n = floor(total / period)`, `leading = gap / 2`.
///
/// Возвращает empty при degenerate-входе: `total_length <= 0`,
/// `dash_len <= 0`. При `gap_len <= 0` возвращает один full-length сегмент
/// (= Solid fallback). Если полоса короче одного даша, возвращает один
/// сегмент с offset=0.
pub(crate) fn dash_segments(
    total_length: f32,
    dash_len: f32,
    gap_len: f32,
) -> Vec<(f32, f32)> {
    if total_length <= 0.0 || dash_len <= 0.0 {
        return Vec::new();
    }
    if gap_len <= 0.0 {
        return vec![(0.0, total_length)];
    }
    let period = dash_len + gap_len;
    let n_floor = (total_length / period).floor() as i32;
    let n_dashes = n_floor.max(1) as usize;
    // leading=gap/2 matches Chrome/Edge (Skia) phase offset.
    // For too-short fallback (n_floor<1) start at corner (offset=0).
    let leading = if n_floor >= 1 { gap_len * 0.5 } else { 0.0 };
    let mut out = Vec::with_capacity(n_dashes);
    let mut x = leading;
    for _ in 0..n_dashes {
        let seg_start = x.max(0.0);
        let seg_end = (x + dash_len).min(total_length);
        if seg_end > seg_start {
            out.push((seg_start, seg_end - seg_start));
        }
        x += period;
    }
    out
}

/// Рисует одну сторону border (top / right / bottom / left) с учётом
/// `BorderStyle`. Логика идентична `emit_outline_side` (Solid → один
/// full-rect, Dashed → pattern `(2w, w)`, Dotted → `(w, w)`), но без
/// «угловых ears» (border-стороны останавливаются у corner-ов и
/// overlap-ятся как fill-rect-ы — это нормально пока border-color
/// одинаков с обеих сторон угла). Phase 0 `BorderStyle::None`
/// фильтруется emit-side через `is_visible()`, но обрабатываем для
/// устойчивости.
fn emit_border_side(
    out: &mut Vec<FillVertex>,
    circle_out: &mut Vec<CircleVertex>,
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: BorderStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    match style {
        BorderStyle::Dashed => {
            // Chrome/Edge (Skia): full side width, n=round(total/period), leading=0.
            // Dash=max(6,2w) and gap=max(4,w) reproduce Edge's observed n values:
            //   2px→n=18, 4px→n=15, 8px→n=8, 16px→n=4 on a 180px side.
            // Dash size is fixed (native); only gap (step) is adjusted to anchor the last
            // dash end exactly at total. This matches Skia's dash rendering more closely.
            // Positions use floor() to match Chrome/Edge pixel-snapping behaviour.
            let target_dash = (width * 2.0).max(6.0);
            let target_gap = width.max(4.0);
            let target_period = target_dash + target_gap;
            let n = ((total / target_period).round() as usize).max(1);
            // Step between dash start positions; last dash end is clamped to total.
            let step = if n > 1 { (total - target_dash) / (n - 1) as f32 } else { 0.0 };
            for i in 0..n {
                let offset = (i as f32 * step).floor();
                let seg_end = (offset + target_dash).min(total);
                if seg_end > offset {
                    let seg = if horizontal {
                        Rect::new(side_rect.x + offset, side_rect.y, seg_end - offset, side_rect.height)
                    } else {
                        Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, seg_end - offset)
                    };
                    push_fill_quad(out, seg, color);
                }
            }
        }
        BorderStyle::Dotted => {
            // Chrome/Edge (Skia): n = floor(total/period) + 1 dots evenly distributed.
            // Symmetric placement: floor(i*step) for first half, span-floor((n-1-i)*step)
            // for second half. This matches the symmetric Bresenham pattern Edge uses,
            // where the "short" gaps appear at both ends, all middle gaps are equal.
            //   2px→n=46, 4px→n=23, 8px→n=12, 16px→n=6 on a 180px side.
            // For dot_len ≤ 2px: use fill_quad (rectangle) instead of SDF circle —
            // Chrome/Edge renders thin dotted borders as squares, not antialiased circles.
            let dot_len = width.max(1.0);
            let period = dot_len * 2.0;
            let n = ((total / period).floor() as usize + 1).max(1);
            let span = total - dot_len;
            let step = if n > 1 { span / (n - 1) as f32 } else { 0.0 };
            let mid = if n > 0 { (n - 1) / 2 } else { 0 };
            let use_rect = dot_len <= 2.0;
            for i in 0..n {
                let offset = if i <= mid {
                    (i as f32 * step).floor()
                } else {
                    let j = (n - 1 - i) as f32;
                    span.floor() - (j * step).floor()
                };
                let seg_end = (offset + dot_len).min(total);
                if seg_end > offset {
                    let seg = if horizontal {
                        Rect::new(side_rect.x + offset, side_rect.y, seg_end - offset, side_rect.height)
                    } else {
                        Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, seg_end - offset)
                    };
                    if use_rect {
                        push_fill_quad(out, seg, color);
                    } else {
                        push_circle_quad(circle_out, seg, color);
                    }
                }
            }
        }
        BorderStyle::Double => {
            // CSS Backgrounds L3 §4.2: two solid lines ~1/3 width each, gap ~1/3.
            // Width < 3px: no room for gap, fall back to solid.
            if width < 3.0 {
                push_fill_quad(out, side_rect, color);
                return;
            }
            let line = (width / 3.0).max(1.0);
            let (r1, r2) = if horizontal {
                (
                    Rect::new(side_rect.x, side_rect.y, side_rect.width, line),
                    Rect::new(side_rect.x, side_rect.y + width - line, side_rect.width, line),
                )
            } else {
                (
                    Rect::new(side_rect.x, side_rect.y, line, side_rect.height),
                    Rect::new(side_rect.x + width - line, side_rect.y, line, side_rect.height),
                )
            };
            push_fill_quad(out, r1, color);
            push_fill_quad(out, r2, color);
        }
        BorderStyle::Solid | BorderStyle::None => {
            push_fill_quad(out, side_rect, color);
        }
    }
}

/// Рисует одну сторону outline (top / right / bottom / left) с учётом
/// `OutlineStyle`. `horizontal=true` для top/bottom (даш-pattern идёт
/// по X), `false` для left/right (по Y). `width` — толщина outline
/// (CSS px), используется как dash/dot длина. Для Solid/Auto/None —
/// один full-rect; для Dashed — pattern `(2w, w)`; для Dotted — `(w, w)`.
fn emit_outline_side(
    out: &mut Vec<FillVertex>,
    circle_out: &mut Vec<CircleVertex>,
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: OutlineStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    match style {
        OutlineStyle::Dashed => {
            let dash_len = (width * 3.0).max(1.0);
            let gap_len = width.max(1.0);
            for (offset, len) in dash_segments(total, dash_len, gap_len) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_fill_quad(out, seg, color);
            }
        }
        OutlineStyle::Dotted => {
            let dot_len = width.max(1.0);
            for (offset, len) in dash_segments(total, dot_len, dot_len) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_circle_quad(circle_out, seg, color);
            }
        }
        // Solid / Auto / None — full-length rect.
        OutlineStyle::Solid | OutlineStyle::Auto | OutlineStyle::None => {
            push_fill_quad(out, side_rect, color);
        }
    }
}

/// Перед draw-командой убедиться, что в `ops` стоит актуальный `SetScissor`
/// для текущего `clip_stack` (топ стека = пересечение всех Push-ов).
/// Возвращает `false`, если scissor пуст (`width==0` || `height==0`) — caller
/// обязан пропустить draw, wgpu иначе паникует на set_scissor_rect(0,0,0,0).
/// `current_scissor=None` означает, что `SetScissor` ещё не выставлялся
/// в этом render-loop-е — тогда команда добавляется даже если desired==full
/// (нет гарантии, что предыдущий кадр оставил scissor на полный размер).
fn sync_scissor_to_stack(
    clip_stack: &[Rect],
    current_scissor: &mut Option<DeviceScissor>,
    ops: &mut Vec<DrawOp>,
    dpr: f32,
    surface_w: u32,
    surface_h: u32,
) -> bool {
    let desired = match clip_stack.last() {
        Some(rect) => css_rect_to_device_scissor(*rect, dpr, surface_w, surface_h),
        None => DeviceScissor::full(surface_w, surface_h),
    };
    if Some(desired) != *current_scissor {
        ops.push(DrawOp::SetScissor(desired));
        *current_scissor = Some(desired);
    }
    !desired.is_empty()
}

/// CSS-px rect → device-px scissor с учётом DPR и Y-axis inversion для wgpu.
/// Шейдер у нас работает в CSS px (viewport = surface / dpr); scissor wgpu
/// работает в device px (Y top-left). Округление: внешние границы наружу
/// (`floor` для x/y, `ceil` для right/bottom) — чтобы scissor НЕ обрезал
/// край pixel-perfect содержимого внутри clip-rect-а. Затем clamp в
/// `[0, surface_*]`. Пустой результат — `is_empty()`-флаг.
pub(crate) fn css_rect_to_device_scissor(
    rect: Rect,
    dpr: f32,
    surface_w: u32,
    surface_h: u32,
) -> DeviceScissor {
    let dpr = dpr.max(1e-6);
    let x0 = (rect.x * dpr).floor().max(0.0);
    let y0 = (rect.y * dpr).floor().max(0.0);
    let x1 = ((rect.x + rect.width) * dpr).ceil().max(0.0);
    let y1 = ((rect.y + rect.height) * dpr).ceil().max(0.0);
    let sw = surface_w as f32;
    let sh = surface_h as f32;
    let cx0 = x0.min(sw) as u32;
    let cy0 = y0.min(sh) as u32;
    let cx1 = x1.min(sw) as u32;
    let cy1 = y1.min(sh) as u32;
    DeviceScissor {
        x: cx0,
        y: cy0,
        width: cx1.saturating_sub(cx0),
        height: cy1.saturating_sub(cy0),
    }
}

/// Создаёт отдельный UNIFORM-буфер с параметрами одного filter pass.
/// Каждый filter render pass должен иметь СОБСТВЕННЫЙ буфер, так как
/// wgpu батчит все `queue.write_buffer` перед encoder-командами: записи
/// в один shared буфер переписывают друг друга и все проходы видят
/// только последнее значение.
fn make_filter_param_buf(device: &wgpu::Device, params: &FilterParamsCpu) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("filter-pass-param"),
        size: std::mem::size_of::<FilterParamsCpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().copy_from_slice(as_bytes(std::slice::from_ref(params)));
    buf.unmap();
    buf
}

// SAFETY: T: Copy + #[repr(C)] плюс отсутствие padding-байт делают этот
// каст безопасным. Используется только для POD-типов из этого файла.
fn as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice))
    }
}

/// Маленький block_on, чтобы не тащить tokio/pollster ради двух async-вызовов
/// в `Renderer::new`. На request_adapter / request_device обычно сразу `Ready`.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;

    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_bin_for_exact_match() {
        // Точное совпадение — bin == входу.
        for &bin in &SIZE_BINS {
            assert_eq!(size_bin_for(f32::from(bin)), bin, "bin {bin}");
        }
    }

    #[test]
    fn size_bin_for_rounds_up_to_next_bin() {
        // 9 → 12, 13 → 16, 17 → 20, 25 → 32, 33 → 48.
        assert_eq!(size_bin_for(9.0), 12);
        assert_eq!(size_bin_for(13.0), 16);
        assert_eq!(size_bin_for(17.0), 20);
        assert_eq!(size_bin_for(25.0), 32);
        assert_eq!(size_bin_for(33.0), 48);
        // Дробные: 13.5 → 16 (ceil 14 → bin 16).
        assert_eq!(size_bin_for(13.5), 16);
    }

    #[test]
    fn size_bin_for_below_min_clamps_to_min() {
        // < 8 — bin 8 (нечитаемо иначе).
        assert_eq!(size_bin_for(1.0), 8);
        assert_eq!(size_bin_for(7.0), 8);
        assert_eq!(size_bin_for(0.5), 8);
    }

    #[test]
    fn size_bin_for_above_max_clamps_to_max() {
        // > 64 — bin 64 (с up-scaling-ом для редких headline-ов).
        assert_eq!(size_bin_for(72.0), 64);
        assert_eq!(size_bin_for(120.0), 64);
        assert_eq!(size_bin_for(1000.0), 64);
    }

    #[test]
    fn size_bin_for_invalid_returns_min() {
        // NaN / negative / 0 → bin 8 (минимум, без panic).
        assert_eq!(size_bin_for(f32::NAN), 8);
        assert_eq!(size_bin_for(-1.0), 8);
        assert_eq!(size_bin_for(0.0), 8);
        assert_eq!(size_bin_for(f32::INFINITY), 64);
    }

    #[test]
    fn atlas_key_distinguishes_size_bins() {
        // Один и тот же глиф на двух размерах = два разных ключа.
        let k16 = atlas_key(0, 42, 16, 0);
        let k32 = atlas_key(0, 42, 32, 0);
        assert_ne!(k16, k32);
    }

    #[test]
    fn atlas_key_distinguishes_glyph_ids() {
        let k_a = atlas_key(0, 100, 16, 0);
        let k_b = atlas_key(0, 200, 16, 0);
        assert_ne!(k_a, k_b);
    }

    #[test]
    fn atlas_key_distinguishes_face_ids() {
        let k0 = atlas_key(0, 42, 16, 0);
        let k1 = atlas_key(1, 42, 16, 0);
        assert_ne!(k0, k1);
    }

    #[test]
    fn atlas_key_distinguishes_variation_coords_hashes() {
        // Тот же (face, glyph, size), но разные normalized coords ⇒ разные
        // ключи. Без этого variant glyph перезаписывал бы default-instance
        // в atlas-кеше.
        let k_default = atlas_key(0, 42, 16, 0);
        let k_bold = atlas_key(0, 42, 16, 0xdead_beef_cafe_babe);
        assert_ne!(k_default, k_bold);
    }

    #[test]
    fn atlas_key_is_deterministic() {
        assert_eq!(atlas_key(3, 17, 24, 0), atlas_key(3, 17, 24, 0));
        assert_eq!(atlas_key(3, 17, 24, 42), atlas_key(3, 17, 24, 42));
    }

    // ── Clip stack / scissor ──────────────────────────────────────────────

    #[test]
    fn intersect_rects_overlapping() {
        let a = Rect::new(10.0, 10.0, 50.0, 50.0);
        let b = Rect::new(30.0, 30.0, 50.0, 50.0);
        let i = intersect_rects(a, b);
        assert_eq!(i, Rect::new(30.0, 30.0, 30.0, 30.0));
    }

    #[test]
    fn intersect_rects_b_inside_a() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(20.0, 30.0, 40.0, 50.0);
        assert_eq!(intersect_rects(a, b), b);
    }

    #[test]
    fn intersect_rects_disjoint_returns_zero_size() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        let i = intersect_rects(a, b);
        assert_eq!(i.width, 0.0);
        assert_eq!(i.height, 0.0);
    }

    #[test]
    fn intersect_rects_touching_edges_returns_zero_size() {
        // Касание ребра (x=10 правая граница a == x=10 левая граница b) —
        // пересечение пустое (right strictly > left требуется).
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        let i = intersect_rects(a, b);
        assert_eq!(i.width, 0.0);
        assert_eq!(i.height, 0.0);
    }

    #[test]
    fn css_to_device_scissor_dpr1_exact() {
        // DPR=1, rect полностью в viewport — scissor совпадает с rect.
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        assert_eq!(s, DeviceScissor { x: 10, y: 20, width: 100, height: 50 });
    }

    #[test]
    fn css_to_device_scissor_dpr2_doubles() {
        // DPR=2 — все координаты × 2.
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let s = css_rect_to_device_scissor(r, 2.0, 2048, 1440);
        assert_eq!(s, DeviceScissor { x: 20, y: 40, width: 200, height: 100 });
    }

    #[test]
    fn css_to_device_scissor_fractional_expands_outward() {
        // Дробные координаты: x.floor(), right.ceil() — scissor расширяется
        // наружу, чтобы не обрезать pixel-perfect содержимое внутри.
        let r = Rect::new(10.3, 20.7, 100.4, 50.1);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        // x.floor() = 10; y.floor() = 20; right.ceil() = 111; bottom.ceil() = 71.
        assert_eq!(s, DeviceScissor { x: 10, y: 20, width: 101, height: 51 });
    }

    #[test]
    fn css_to_device_scissor_clamps_to_surface() {
        // Rect частично за пределами surface — scissor клампается.
        let r = Rect::new(900.0, 600.0, 500.0, 500.0);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        // right = 1400 → clamp to 1024; bottom = 1100 → clamp to 720.
        assert_eq!(s, DeviceScissor { x: 900, y: 600, width: 124, height: 120 });
    }

    #[test]
    fn css_to_device_scissor_negative_origin_clamps_to_zero() {
        // Rect частично слева/сверху surface — origin клампится в 0.
        let r = Rect::new(-50.0, -30.0, 100.0, 60.0);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        // x.floor()=-50 → max(0)=0, right.ceil()=50 → 50; y similar → 30.
        assert_eq!(s, DeviceScissor { x: 0, y: 0, width: 50, height: 30 });
    }

    #[test]
    fn css_to_device_scissor_fully_outside_is_empty() {
        // Rect полностью справа от surface.
        let r = Rect::new(1500.0, 0.0, 100.0, 50.0);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        assert!(s.is_empty());
    }

    #[test]
    fn css_to_device_scissor_zero_rect_is_empty() {
        // Rect с нулевой шириной — пустой scissor.
        let r = Rect::new(10.0, 20.0, 0.0, 50.0);
        let s = css_rect_to_device_scissor(r, 1.0, 1024, 720);
        assert!(s.is_empty());
    }

    #[test]
    fn device_scissor_full_covers_surface() {
        let s = DeviceScissor::full(1024, 720);
        assert_eq!(s, DeviceScissor { x: 0, y: 0, width: 1024, height: 720 });
        assert!(!s.is_empty());
    }

    #[test]
    fn device_scissor_is_empty_detects_zero_dim() {
        assert!(DeviceScissor { x: 0, y: 0, width: 0, height: 10 }.is_empty());
        assert!(DeviceScissor { x: 0, y: 0, width: 10, height: 0 }.is_empty());
        assert!(!DeviceScissor { x: 0, y: 0, width: 1, height: 1 }.is_empty());
    }

    #[test]
    fn sync_scissor_pushes_full_on_empty_stack() {
        let mut current: Option<DeviceScissor> = None;
        let mut ops: Vec<DrawOp> = Vec::new();
        let ok = sync_scissor_to_stack(&[], &mut current, &mut ops, 1.0, 1024, 720);
        assert!(ok);
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], DrawOp::SetScissor(s) if s == DeviceScissor::full(1024, 720)));
        assert_eq!(current, Some(DeviceScissor::full(1024, 720)));
    }

    #[test]
    fn sync_scissor_dedupes_same_scissor() {
        // Первый вызов выставляет full; второй с тем же стеком — не пушит.
        let mut current: Option<DeviceScissor> = None;
        let mut ops: Vec<DrawOp> = Vec::new();
        sync_scissor_to_stack(&[], &mut current, &mut ops, 1.0, 1024, 720);
        let n_after_first = ops.len();
        sync_scissor_to_stack(&[], &mut current, &mut ops, 1.0, 1024, 720);
        assert_eq!(ops.len(), n_after_first, "повторный вызов не должен пушить op");
    }

    #[test]
    fn sync_scissor_pushes_on_stack_change() {
        let mut current: Option<DeviceScissor> = None;
        let mut ops: Vec<DrawOp> = Vec::new();
        sync_scissor_to_stack(&[], &mut current, &mut ops, 1.0, 1024, 720);
        // Стек добавил clip — scissor сужается.
        let stack = vec![Rect::new(100.0, 100.0, 200.0, 200.0)];
        sync_scissor_to_stack(&stack, &mut current, &mut ops, 1.0, 1024, 720);
        assert_eq!(ops.len(), 2);
        assert!(matches!(
            ops[1],
            DrawOp::SetScissor(s) if s == DeviceScissor { x: 100, y: 100, width: 200, height: 200 }
        ));
    }

    #[test]
    fn sync_scissor_returns_false_on_empty_scissor() {
        // Clip полностью за пределами surface — sync возвращает false,
        // caller должен пропустить draw.
        let mut current: Option<DeviceScissor> = None;
        let mut ops: Vec<DrawOp> = Vec::new();
        let stack = vec![Rect::new(2000.0, 2000.0, 100.0, 100.0)];
        let ok = sync_scissor_to_stack(&stack, &mut current, &mut ops, 1.0, 1024, 720);
        assert!(!ok);
    }

    // ── current_blend_mode ───────────────────────────────────────────────

    #[test]
    fn current_blend_mode_empty_stack_is_normal() {
        assert_eq!(current_blend_mode(&[]), BlendMode::Normal);
    }

    #[test]
    fn current_blend_mode_single_push() {
        assert_eq!(current_blend_mode(&[BlendMode::Multiply]), BlendMode::Multiply);
        assert_eq!(current_blend_mode(&[BlendMode::Screen]), BlendMode::Screen);
        assert_eq!(current_blend_mode(&[BlendMode::PlusLighter]), BlendMode::PlusLighter);
    }

    #[test]
    fn current_blend_mode_nested_returns_top() {
        // Вложенные blend-mode-ы: активен самый внутренний (топ стека).
        assert_eq!(
            current_blend_mode(&[BlendMode::Multiply, BlendMode::Screen]),
            BlendMode::Screen
        );
        assert_eq!(
            current_blend_mode(&[BlendMode::Normal, BlendMode::Overlay, BlendMode::Darken]),
            BlendMode::Darken
        );
    }

    #[test]
    fn current_blend_mode_pop_restores_previous() {
        let mut stack = vec![BlendMode::Multiply, BlendMode::Screen];
        assert_eq!(current_blend_mode(&stack), BlendMode::Screen);
        stack.pop();
        assert_eq!(current_blend_mode(&stack), BlendMode::Multiply);
        stack.pop();
        assert_eq!(current_blend_mode(&stack), BlendMode::Normal);
    }

    #[test]
    fn current_blend_mode_normal_on_stack_returns_normal() {
        // Явный Normal на стеке — тот же результат что и пустой стек.
        assert_eq!(current_blend_mode(&[BlendMode::Normal]), BlendMode::Normal);
    }

    #[test]
    fn apply_alpha_to_color_identity() {
        let c = [0.2, 0.3, 0.4, 0.8];
        assert_eq!(apply_alpha_to_color(c, 1.0), c);
    }

    #[test]
    fn apply_alpha_to_color_half() {
        // Цвет (1, 0.5, 0.25, 0.8), alpha=0.5 → alpha-канал × 0.5 = 0.4.
        let out = apply_alpha_to_color([1.0, 0.5, 0.25, 0.8], 0.5);
        assert_eq!(out, [1.0, 0.5, 0.25, 0.4]);
    }

    #[test]
    fn apply_alpha_to_color_zero() {
        // alpha=0 → final-color.a = 0 (полностью прозрачно).
        let out = apply_alpha_to_color([1.0, 0.5, 0.25, 1.0], 0.0);
        assert_eq!(out, [1.0, 0.5, 0.25, 0.0]);
    }

    // ── dash_segments ────────────────────────────────────────────────────

    #[test]
    fn dash_segments_zero_length_returns_empty() {
        assert!(dash_segments(0.0, 4.0, 2.0).is_empty());
        assert!(dash_segments(-5.0, 4.0, 2.0).is_empty());
    }

    #[test]
    fn dash_segments_zero_dash_returns_empty() {
        assert!(dash_segments(10.0, 0.0, 2.0).is_empty());
        assert!(dash_segments(10.0, -1.0, 2.0).is_empty());
    }

    #[test]
    fn dash_segments_zero_gap_returns_single_full() {
        // gap=0 — это solid, не разрывается.
        let segs = dash_segments(10.0, 4.0, 0.0);
        assert_eq!(segs, vec![(0.0, 10.0)]);
    }

    #[test]
    fn dash_segments_exact_fit() {
        // dash=4, gap=2 → period=6; total=10 → floor(10/6)=1 dash;
        // leading=gap/2=1; сегмент: (1, 4).
        let segs = dash_segments(10.0, 4.0, 2.0);
        assert_eq!(segs.len(), 1);
        assert!((segs[0].0 - 1.0).abs() < 1e-6);
        assert!((segs[0].1 - 4.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_centered_leftover() {
        // dash=2, gap=2 → period=4; total=10 → floor(10/4)=2 dashes;
        // leading=gap/2=1; сегменты (1,2),(5,2).
        let segs = dash_segments(10.0, 2.0, 2.0);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], (1.0, 2.0));
        assert_eq!(segs[1], (5.0, 2.0));
    }

    #[test]
    fn dash_segments_with_leftover_centers() {
        // dash=2, gap=2 → period=4; total=11 → floor(11/4)=2 dashes;
        // leading=gap/2=1; segs[0].0=1.0.
        let segs = dash_segments(11.0, 2.0, 2.0);
        assert_eq!(segs.len(), 2);
        assert!((segs[0].0 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_too_short_one_dash() {
        // total=3, dash=4, gap=2 — n_floor=floor(3/6)=0 → max(1)=1;
        // leading=0 (too-short fallback); сегмент (0,3) обрезается до total.
        let segs = dash_segments(3.0, 4.0, 2.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].0, 0.0);
        assert!((segs[0].1 - 3.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_dotted_pattern() {
        // dot_len=2, gap=2 (Dotted width=2): total=10 → floor(10/4)=2 dots;
        // leading=1; dots at (1,2),(5,2).
        let segs = dash_segments(10.0, 2.0, 2.0);
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn dash_segments_count_for_typical_outline() {
        // Outline width=2, dashed: dash=4, gap=2; полоса 100 px.
        // n=floor(100/6)=16 dashes; leading=1.
        let segs = dash_segments(100.0, 4.0, 2.0);
        assert_eq!(segs.len(), 16);
    }

    // ── emit_border_side ──────────────────────────────────────────────────

    fn collect_border_fill_quads(
        side_rect: Rect,
        horizontal: bool,
        width: f32,
        style: BorderStyle,
    ) -> Vec<Rect> {
        let color = [1.0f32; 4];
        let mut fill_verts: Vec<FillVertex> = Vec::new();
        let mut circle_verts: Vec<CircleVertex> = Vec::new();
        emit_border_side(&mut fill_verts, &mut circle_verts, side_rect, horizontal, width, color, style);
        fill_verts
            .chunks(6)
            .map(|v| {
                let xs = v.iter().map(|p| p.pos[0]);
                let ys = v.iter().map(|p| p.pos[1]);
                let x0 = xs.clone().fold(f32::INFINITY, f32::min);
                let x1 = xs.fold(f32::NEG_INFINITY, f32::max);
                let y0 = ys.clone().fold(f32::INFINITY, f32::min);
                let y1 = ys.fold(f32::NEG_INFINITY, f32::max);
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            })
            .collect()
    }

    fn collect_border_circle_quads(
        side_rect: Rect,
        horizontal: bool,
        width: f32,
        style: BorderStyle,
    ) -> Vec<Rect> {
        let color = [1.0f32; 4];
        let mut fill_verts: Vec<FillVertex> = Vec::new();
        let mut circle_verts: Vec<CircleVertex> = Vec::new();
        emit_border_side(&mut fill_verts, &mut circle_verts, side_rect, horizontal, width, color, style);
        circle_verts
            .chunks(6)
            .map(|v| {
                let xs = v.iter().map(|p| p.pos[0]);
                let ys = v.iter().map(|p| p.pos[1]);
                let x0 = xs.clone().fold(f32::INFINITY, f32::min);
                let x1 = xs.fold(f32::NEG_INFINITY, f32::max);
                let y0 = ys.clone().fold(f32::INFINITY, f32::min);
                let y1 = ys.fold(f32::NEG_INFINITY, f32::max);
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            })
            .collect()
    }

    #[test]
    fn emit_border_side_solid_is_single_quad() {
        let r = Rect::new(10.0, 20.0, 100.0, 6.0);
        let quads = collect_border_fill_quads(r, true, 6.0, BorderStyle::Solid);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0], r);
    }

    #[test]
    fn emit_border_side_dashed_produces_multiple_quads() {
        // width=4: target_dash=max(6,8)=8, target_gap=max(5,4)=5, period=13
        // side=100 → n=round(100/13)=8 segments.
        let r = Rect::new(0.0, 0.0, 100.0, 4.0);
        let quads = collect_border_fill_quads(r, true, 4.0, BorderStyle::Dashed);
        assert!(quads.len() > 1, "dashed must produce multiple segments");
        for q in &quads {
            assert_eq!(q.height, 4.0, "all segments must span full border height");
        }
    }

    #[test]
    fn emit_border_side_dotted_circle_segments() {
        // Dotted width≥3 → SDF-circles (circle_verts), not fill quads.
        // width=4 → dot=4, period=8; side=40 → n=floor(40/8)+1=6 dots.
        // Each quad is expanded 0.5px on each side: height = 4+1 = 5.
        let r = Rect::new(0.0, 0.0, 40.0, 4.0);
        let fill_quads = collect_border_fill_quads(r, true, 4.0, BorderStyle::Dotted);
        let circle_quads = collect_border_circle_quads(r, true, 4.0, BorderStyle::Dotted);
        assert_eq!(fill_quads.len(), 0, "dotted width=4 must NOT produce fill quads");
        assert!(circle_quads.len() > 1, "dotted must produce circle quads");
        assert_eq!(circle_quads.len(), 6, "dotted: n=floor(total/period)+1=6");
        for q in &circle_quads {
            assert_eq!(q.height, 5.0, "expanded quad: dot_size + 1 = 5.0");
        }
    }

    #[test]
    fn emit_border_side_dotted_thin_uses_fill_quads() {
        // Dotted width≤2px → fill_quad rectangles (no SDF circles), matching
        // Chrome/Edge behavior of rendering thin dotted borders as squares.
        // width=2 → dot=2, period=4; side=20 → n=floor(20/4)+1=6 quads.
        let r = Rect::new(0.0, 0.0, 20.0, 2.0);
        let fill_quads = collect_border_fill_quads(r, true, 2.0, BorderStyle::Dotted);
        let circle_quads = collect_border_circle_quads(r, true, 2.0, BorderStyle::Dotted);
        assert_eq!(circle_quads.len(), 0, "thin dotted must NOT produce circle quads");
        assert!(fill_quads.len() > 1, "thin dotted must produce fill quads");
        assert_eq!(fill_quads.len(), 6, "thin dotted: n=floor(20/4)+1=6");
    }

    #[test]
    fn emit_border_side_double_two_quads_horizontal() {
        // width=9 → line≈3; two lines at top and bottom of the side_rect.
        let r = Rect::new(0.0, 0.0, 100.0, 9.0);
        let quads = collect_border_fill_quads(r, true, 9.0, BorderStyle::Double);
        assert_eq!(quads.len(), 2, "double = two parallel lines");
        // First line at top edge.
        assert!((quads[0].y - 0.0).abs() < 1e-3, "first line at y=0");
        // Second line at bottom edge.
        let expected_y2 = 9.0 - (9.0 / 3.0_f32).max(1.0);
        assert!((quads[1].y - expected_y2).abs() < 1e-3, "second line at bottom");
        // Both lines span full width.
        assert_eq!(quads[0].width, 100.0);
        assert_eq!(quads[1].width, 100.0);
    }

    #[test]
    fn emit_border_side_double_thin_fallback_to_solid() {
        // width < 3 → solid fallback (no room for gap).
        let r = Rect::new(0.0, 0.0, 100.0, 2.0);
        let quads = collect_border_fill_quads(r, true, 2.0, BorderStyle::Double);
        assert_eq!(quads.len(), 1, "width<3 must fall back to single solid quad");
    }

    #[test]
    fn emit_border_side_double_vertical() {
        // Vertical double border (left/right side).
        let r = Rect::new(0.0, 0.0, 9.0, 100.0);
        let quads = collect_border_fill_quads(r, false, 9.0, BorderStyle::Double);
        assert_eq!(quads.len(), 2, "double vertical = two parallel lines");
        assert!((quads[0].x - 0.0).abs() < 1e-3);
        let expected_x2 = 9.0 - (9.0 / 3.0_f32).max(1.0);
        assert!((quads[1].x - expected_x2).abs() < 1e-3);
        assert_eq!(quads[0].height, 100.0);
        assert_eq!(quads[1].height, 100.0);
    }

    #[test]
    fn apply_alpha_to_color_preserves_rgb() {
        // RGB не трогается (premultiplied alpha — отдельная история; здесь
        // straight alpha с alpha-blending в pipeline).
        let out = apply_alpha_to_color([0.123, 0.456, 0.789, 1.0], 0.5);
        assert_eq!(out[0], 0.123);
        assert_eq!(out[1], 0.456);
        assert_eq!(out[2], 0.789);
        assert_eq!(out[3], 0.5);
    }

    #[test]
    fn sync_scissor_dpr_scales_stack_rect() {
        // Стек хранится в CSS-px; sync переводит в device-px через DPR.
        let mut current: Option<DeviceScissor> = None;
        let mut ops: Vec<DrawOp> = Vec::new();
        let stack = vec![Rect::new(50.0, 50.0, 100.0, 100.0)];
        sync_scissor_to_stack(&stack, &mut current, &mut ops, 2.0, 2048, 1440);
        assert!(matches!(
            ops[0],
            DrawOp::SetScissor(s) if s == DeviceScissor { x: 100, y: 100, width: 200, height: 200 }
        ));
    }

    // ── blend_mode_to_u32 ────────────────────────────────────────────────

    #[test]
    fn blend_mode_to_u32_correct_values() {
        // Значения должны совпадать с маппингом в BLEND_SHADER_SRC.
        assert_eq!(blend_mode_to_u32(BlendMode::Normal),      0);
        assert_eq!(blend_mode_to_u32(BlendMode::Multiply),    1);
        assert_eq!(blend_mode_to_u32(BlendMode::Screen),      2);
        assert_eq!(blend_mode_to_u32(BlendMode::Overlay),     3);
        assert_eq!(blend_mode_to_u32(BlendMode::Darken),      4);
        assert_eq!(blend_mode_to_u32(BlendMode::Lighten),     5);
        assert_eq!(blend_mode_to_u32(BlendMode::ColorDodge),  6);
        assert_eq!(blend_mode_to_u32(BlendMode::ColorBurn),   7);
        assert_eq!(blend_mode_to_u32(BlendMode::HardLight),   8);
        assert_eq!(blend_mode_to_u32(BlendMode::SoftLight),   9);
        assert_eq!(blend_mode_to_u32(BlendMode::Difference),  10);
        assert_eq!(blend_mode_to_u32(BlendMode::Exclusion),   11);
        assert_eq!(blend_mode_to_u32(BlendMode::Hue),         12);
        assert_eq!(blend_mode_to_u32(BlendMode::Saturation),  13);
        assert_eq!(blend_mode_to_u32(BlendMode::Color),       14);
        assert_eq!(blend_mode_to_u32(BlendMode::Luminosity),  15);
        assert_eq!(blend_mode_to_u32(BlendMode::PlusLighter), 16);
    }

    // ── Render plan: PushBlendMode / PopBlendMode level logic ────────────

    /// Симулирует логику render-planning без GPU: применяет список команд
    /// к level + blend_mode стекам, проверяет итоговый уровень.
    fn sim_blend_level(cmds: &[DisplayCommand]) -> (usize, Vec<BlendMode>) {
        let mut current_level: usize = 0;
        let mut blend_mode_stack: Vec<BlendMode> = Vec::new();
        let mut level_blend_mode_stack: Vec<BlendMode> = Vec::new();
        for cmd in cmds {
            match cmd {
                DisplayCommand::PushBlendMode { mode } => {
                    blend_mode_stack.push(*mode);
                    if *mode != BlendMode::Normal {
                        level_blend_mode_stack.push(*mode);
                        current_level += 1;
                    }
                }
                DisplayCommand::PopBlendMode => {
                    blend_mode_stack.pop();
                    if level_blend_mode_stack.pop().is_some() {
                        current_level -= 1;
                    }
                }
                _ => {}
            }
        }
        (current_level, blend_mode_stack)
    }

    #[test]
    fn push_blend_mode_normal_does_not_create_new_level() {
        // PushBlendMode { Normal } — level остаётся 0.
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: BlendMode::Normal },
        ];
        let (level, stack) = sim_blend_level(&cmds);
        assert_eq!(level, 0, "Normal blend mode не должен открывать offscreen level");
        assert_eq!(stack, vec![BlendMode::Normal]);
    }

    #[test]
    fn push_blend_mode_non_normal_creates_new_level() {
        // PushBlendMode { Multiply } — level становится 1.
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: BlendMode::Multiply },
        ];
        let (level, _) = sim_blend_level(&cmds);
        assert_eq!(level, 1, "не-Normal blend mode должен открывать offscreen level");
    }

    #[test]
    fn pop_blend_mode_restores_level() {
        // Push/Pop пары: level возвращается в 0.
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: BlendMode::Screen },
            DisplayCommand::PopBlendMode,
        ];
        let (level, stack) = sim_blend_level(&cmds);
        assert_eq!(level, 0, "после PopBlendMode level должен вернуться в 0");
        assert!(stack.is_empty(), "blend_mode_stack должен быть пуст после Pop");
    }

    // ── vertex transform: 2D fast path vs 3D perspective projection ────────

    fn fv(x: f32, y: f32) -> FillVertex {
        FillVertex { pos: [x, y], z: 0.0, color: [0.0, 0.0, 0.0, 1.0] }
    }

    fn approxf(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn apply_verts_2d_affine_uses_fast_path() {
        // translate(10, 20) · scale(2, 3): точка (4, 5) → (2·4+10, 3·5+20) = (18, 35).
        let m = Mat4::translation_2d(10.0, 20.0).multiply(&Mat4::scale_2d(2.0, 3.0));
        let mut verts = [fv(4.0, 5.0)];
        apply_affine_to_verts(&mut verts, &m);
        assert!(approxf(verts[0].pos[0], 18.0));
        assert!(approxf(verts[0].pos[1], 35.0));
    }

    #[test]
    fn apply_verts_perspective_divides_by_w() {
        // perspective(800) к вершине z=0: w' = 1, без изменений (z=0 в плоскости).
        // Но композиция perspective · translateZ сдвигает вершину по z и даёт
        // перспективное масштабирование. translateZ(+400) → точка на z=400,
        // perspective(800) → w' = 1 − 400/800 = 0.5 → x' = x/0.5 = 2x.
        let m = Mat4::perspective(800.0).multiply(&Mat4::translate_3d(0.0, 0.0, 400.0));
        assert!(!m.is_2d_affine(), "перспективная матрица не 2D affine");
        let mut verts = [fv(100.0, 50.0)];
        apply_affine_to_verts(&mut verts, &m);
        assert!(approxf(verts[0].pos[0], 200.0), "x' = {}", verts[0].pos[0]);
        assert!(approxf(verts[0].pos[1], 100.0), "y' = {}", verts[0].pos[1]);
    }

    #[test]
    fn apply_verts_rotate_y_flattens_x() {
        // rotateY(90°): x' = cos90·x + sin90·z = 0 (z=0). Грань схлопывается по X.
        let m = Mat4::rotate_y(std::f32::consts::FRAC_PI_2);
        let mut verts = [fv(100.0, 50.0)];
        apply_affine_to_verts(&mut verts, &m);
        assert!(approxf(verts[0].pos[0], 0.0), "x' = {}", verts[0].pos[0]);
        assert!(approxf(verts[0].pos[1], 50.0), "y' = {}", verts[0].pos[1]);
    }

    // ── GPU depth buffer: FillVertex.z field ────────────────────────────────

    #[test]
    fn fill_vertex_z_default_zero() {
        // push_fill_quad creates vertices with z=0 (no transform → depth=0.5 in shader).
        let mut out = Vec::new();
        let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
        push_fill_quad(&mut out, rect, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(out.len(), 6);
        for v in &out {
            assert_eq!(v.z, 0.0, "push_fill_quad must produce z=0 vertices");
        }
    }

    #[test]
    fn apply_verts_2d_affine_leaves_z_zero() {
        // 2D affine transform: z stays 0 (no depth change for flat 2D elements).
        let m = Mat4::translation_2d(50.0, 30.0);
        let mut verts = [fv(10.0, 20.0)];
        apply_affine_to_verts(&mut verts, &m);
        assert_eq!(verts[0].z, 0.0, "2D affine must leave z unchanged at 0.0");
    }

    #[test]
    fn apply_verts_rotate_x_sets_depth() {
        // rotateX(90°) on a vertex at y=100, z_in=0: in CSS Y-down convention,
        // rotating +Y toward the viewer moves y=100 → z_out ≈ +100 (closer to viewer).
        // Vertex at y=0 (on the axis) stays at z=0.
        let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
        assert!(!m.is_2d_affine());
        let mut verts = [fv(50.0, 100.0), fv(50.0, 0.0)];
        apply_affine_to_verts(&mut verts, &m);
        // y=100 rotated about X: z_out ≈ +100 (toward viewer in CSS convention)
        assert!(verts[0].z.abs() > 50.0, "rotateX on y=100 should give |z| > 50, got {}", verts[0].z);
        // y=0 (on axis) stays at z=0
        assert!(approxf(verts[1].z, 0.0), "vertex on rotation axis stays at z=0, got {}", verts[1].z);
    }

    #[test]
    fn apply_verts_perspective_sets_depth() {
        // perspective(800) + translateZ(400): w' = 1 - 400/800 = 0.5.
        // z_out = project_point_z(...).2 — should be non-zero, showing z is propagated.
        let m = Mat4::perspective(800.0).multiply(&Mat4::translate_3d(0.0, 0.0, 400.0));
        let mut verts = [fv(0.0, 0.0)];
        apply_affine_to_verts(&mut verts, &m);
        // With translateZ(400) and perspective(800), the z after perspective divide is
        // pz/pw where pw = 0.5 (computed from 1/d · z term). Non-zero depth expected.
        assert!(verts[0].z.abs() > 0.0, "perspective transform must propagate depth, z={}", verts[0].z);
    }

    #[test]
    fn depth_ndc_formula_maps_correctly() {
        // Verify the NDC formula used in the shader: depth = clamp(0.5 - z/20000, 0, 1)
        // z=0 → depth=0.5 (2D elements, painter's order via LessEqual)
        // z=10000 (close) → depth=0.0 (front)
        // z=-10000 (far) → depth=1.0 (back)
        fn depth_ndc(z: f32) -> f32 { (0.5 - z / 20000.0).clamp(0.0, 1.0) }
        assert!((depth_ndc(0.0) - 0.5).abs() < 1e-6);
        assert!((depth_ndc(10000.0) - 0.0).abs() < 1e-6);
        assert!((depth_ndc(-10000.0) - 1.0).abs() < 1e-6);
        // Closer element has smaller depth → wins LessEqual test
        assert!(depth_ndc(100.0) < depth_ndc(-100.0), "closer (positive z) must have smaller NDC depth");
    }

    // ── GPU depth buffer: TextVertex / ImageVertex / RRectVertex.z field ────

    /// `TextVertex` carries a CSS-px depth field so 3D-transformed glyph quads
    /// participate in the same GPU depth test as `FillVertex` (no painter-order
    /// fallback for text under preserve-3d).
    #[test]
    fn text_vertex_carries_depth_field() {
        let mut v = TextVertex { pos: [10.0, 20.0], z: 0.0, uv: [0.0, 0.0], color: [1.0; 4] };
        assert_eq!(v.z, 0.0, "TextVertex z initial value must be 0.0");
        // VertexPos::set_depth must write into the z field.
        v.set_depth(150.0);
        assert!(approxf(v.z, 150.0), "TextVertex set_depth must update z, got {}", v.z);
        // Struct stride matches the wgpu vertex attribute layout
        // (pos 8 + z 4 + uv 8 + color 16 = 36 bytes).
        assert_eq!(std::mem::size_of::<TextVertex>(), 36);
    }

    /// `ImageVertex` carries a CSS-px depth field so 3D-transformed `<img>`
    /// quads occlude correctly against background rects.
    #[test]
    fn image_vertex_carries_depth_field() {
        let mut v = ImageVertex { pos: [5.0, 7.0], z: 0.0, uv: [1.0, 1.0], alpha: 1.0 };
        assert_eq!(v.z, 0.0, "ImageVertex z initial value must be 0.0");
        v.set_depth(-300.0);
        assert!(approxf(v.z, -300.0), "ImageVertex set_depth must update z, got {}", v.z);
        // Struct stride matches wgpu attribute layout
        // (pos 8 + z 4 + uv 8 + alpha 4 = 24 bytes).
        assert_eq!(std::mem::size_of::<ImageVertex>(), 24);
    }

    /// `RRectVertex` (SDF rounded-rect) carries a CSS-px depth field so border-
    /// radius backgrounds participate in cross-type depth testing.
    #[test]
    fn rrect_vertex_carries_depth_field() {
        let mut v = RRectVertex {
            pos: [0.0, 0.0],
            z: 0.0,
            color: [0.0, 0.0, 0.0, 1.0],
            center: [50.0, 50.0],
            half_size: [50.0, 50.0],
            radii_x: [10.0; 4],
            radii_y: [10.0; 4],
        };
        assert_eq!(v.z, 0.0, "RRectVertex z initial value must be 0.0");
        v.set_depth(42.0);
        assert!(approxf(v.z, 42.0), "RRectVertex set_depth must update z, got {}", v.z);
        // Stride matches wgpu attribute layout
        // (pos 8 + z 4 + color 16 + center 8 + half_size 8 + radii_x 16 + radii_y 16 = 76 bytes).
        assert_eq!(std::mem::size_of::<RRectVertex>(), 76);
    }

    /// Constructors emit z=0 for all 6 quad vertices — equivalent to the 2D
    /// painter's-order path (depth=0.5 in shader); 3D transforms override later
    /// via `apply_affine_to_verts` / `apply_affine_to_rrect_verts`.
    #[test]
    fn push_image_quad_emits_zero_depth() {
        let mut out = Vec::new();
        let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
        push_image_quad(&mut out, rect, [0.0, 0.0], [1.0, 1.0], 1.0);
        assert_eq!(out.len(), 6);
        for v in &out {
            assert_eq!(v.z, 0.0, "push_image_quad must produce z=0 vertices");
        }
    }

    /// `push_rrect_quad` similarly emits z=0 for all 6 vertices.
    #[test]
    fn push_rrect_quad_emits_zero_depth() {
        let mut out = Vec::new();
        let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 50.0);
        let radii = CornerRadii {
            tl: 8.0, tr: 8.0, br: 8.0, bl: 8.0,
            tl_y: 8.0, tr_y: 8.0, br_y: 8.0, bl_y: 8.0,
        };
        push_rrect_quad(&mut out, rect, [1.0, 0.0, 0.0, 1.0], radii);
        assert_eq!(out.len(), 6);
        for v in &out {
            assert_eq!(v.z, 0.0, "push_rrect_quad must produce z=0 vertices");
        }
    }

    /// `apply_affine_to_rrect_verts` propagates projected z through the 3D
    /// path (`Mat4::project_point_z`) so border-radius backgrounds get correct
    /// depth values when transformed.
    #[test]
    fn apply_rrect_affine_3d_sets_depth() {
        // rotateX(90°) on a vertex at y=100 should produce non-zero projected z.
        let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
        assert!(!m.is_2d_affine());
        let mut verts = vec![RRectVertex {
            pos: [50.0, 100.0],
            z: 0.0,
            color: [1.0; 4],
            center: [50.0, 100.0],
            half_size: [50.0, 100.0],
            radii_x: [0.0; 4],
            radii_y: [0.0; 4],
        }];
        apply_affine_to_rrect_verts(&mut verts, &m);
        // Same orientation as `apply_verts_rotate_x_sets_depth`: |z| should grow.
        assert!(verts[0].z.abs() > 50.0,
            "rotateX on y=100 must produce |z|>50 in RRectVertex, got {}", verts[0].z);
    }

    /// 2D affine on `apply_affine_to_rrect_verts` must leave z untouched
    /// (fast path identical to the pre-depth pipeline).
    #[test]
    fn apply_rrect_affine_2d_leaves_z_zero() {
        let m = Mat4::translation_2d(20.0, 30.0);
        let mut verts = vec![RRectVertex {
            pos: [0.0, 0.0],
            z: 0.0,
            color: [1.0; 4],
            center: [50.0, 50.0],
            half_size: [50.0, 50.0],
            radii_x: [0.0; 4],
            radii_y: [0.0; 4],
        }];
        apply_affine_to_rrect_verts(&mut verts, &m);
        assert_eq!(verts[0].z, 0.0, "2D affine on rrect must leave z unchanged");
    }

    /// `apply_affine_to_verts` (the generic path used by Text/Image) propagates
    /// projected depth via `VertexPos::set_depth` into TextVertex/ImageVertex.
    #[test]
    fn apply_verts_3d_sets_text_and_image_depth() {
        let m = Mat4::rotate_x(std::f32::consts::FRAC_PI_2);
        let mut text = [TextVertex { pos: [50.0, 100.0], z: 0.0, uv: [0.0, 0.0], color: [1.0; 4] }];
        apply_affine_to_verts(&mut text, &m);
        assert!(text[0].z.abs() > 50.0,
            "rotateX must propagate depth into TextVertex.z, got {}", text[0].z);
        let mut image = [ImageVertex { pos: [50.0, 100.0], z: 0.0, uv: [0.0, 0.0], alpha: 1.0 }];
        apply_affine_to_verts(&mut image, &m);
        assert!(image[0].z.abs() > 50.0,
            "rotateX must propagate depth into ImageVertex.z, got {}", image[0].z);
    }
}
