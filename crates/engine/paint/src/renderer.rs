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

use lumen_core::ColorSpace;
use lumen_core::ext::{FontProvider, FontStyle as CssFontStyle};
use lumen_core::geom::Rect;
use lumen_font::{
    Bitmap, Font, Head, Hmtx, Outline, OwnedCmap, Rasterizer,
    SystemFontIndex, maybe_decode_font,
};
use lumen_image::{correct_rgba_pixels, Image, PixelFormat};
use lumen_layout::{BackgroundRepeat, BackgroundSize, BorderStyle, Color, FilterFn, FontStyle, FontWeight, GradientStop, ImageRendering, Mat4, ObjectFit, ObjectPosition, OutlineStyle, PositionComponent, style::TextOrientation};
use winit::window::Window;

use crate::atlas::{AtlasKey, GlyphAtlas, GlyphEntry};
use crate::display_list::{
    fit_image_quad, fit_image_rect, space_axis_geometry, BlendMode, CornerRadii, MaskMode,
};
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

/// Mip-каскад картинок (p1-exp-wgpu-only): fullscreen-triangle blit
/// «mip N−1 → mip N». Bilinear-выборка ровно между четырьмя текселями
/// источника = 2×2 box-фильтр — стандартный GPU-даунскейл (так же строит
/// mip-ы Chromium). Bind group — `image_bgl` (texture + sampler).
const MIPGEN_SHADER_SRC: &str = r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_smp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    // Fullscreen triangle: (-1,-1), (3,-1), (-1,3); uv (0,1), (2,1), (0,-1).
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32((vi >> 1u) & 1u) * 4.0 - 1.0;
    var out: VOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x, -y) * 0.5 + 0.5;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(src_tex, src_smp, in.uv);
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

    // Un-premultiply for blending: offscreen layers accumulate PREMULTIPLIED content
    // (each draw is composited onto them with straight-alpha ALPHA_BLENDING, which —
    // starting from a transparent-black clear — leaves rgb scaled by alpha). The CSS
    // Compositing L1 §8 formulas below (and blend_channel) expect straight Cs/Cd, so
    // divide it back out; a fully-transparent source/dest has no meaningful straight
    // color and `as_`/`ad` zero it out later in the compositing formula regardless.
    let as_ = src.a;
    let ad = dst.a;
    var cs = select(src.rgb / as_, vec3<f32>(0.0), as_ <= 0.0);
    var cd = select(dst.rgb / ad, vec3<f32>(0.0), ad <= 0.0);

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

/// Скролл-композитор страницы (EXPERIMENT.md §2, срез 1): персистентная
/// текстура «полосы» документа — вьюпорт плюс запас сверху и снизу,
/// растеризованная в документных координатах (scroll-инвариантно). Пока
/// вьюпорт остаётся внутри полосы и содержимое не меняется, кадр скролла =
/// один blit этой текстуры со сдвигом + overlay, без перерисовки страницы.
struct PageBandCache {
    /// Держит GPU-память полосы (wgpu освобождает её при дропе последней
    /// ссылки; view её не удерживает — как `GpuImage::_texture`).
    _texture: wgpu::Texture,
    /// View полосы — источник blit-а и цель Band-рендера.
    view: wgpu::TextureView,
    /// Scroll-инвариантный ключ содержимого: хэш content-полосы display
    /// list-а при scroll (0,0) + `content_generation` + геометрия полосы.
    /// Урок EXPERIMENT.md п.15: скролл в ключе = промах каждый кадр.
    key: u64,
    /// Y верхнего края полосы в документных CSS px (≥ 0).
    band_top_css: f32,
    /// Ширина текстуры полосы в device px (= ширине surface).
    w_px: u32,
    /// Высота текстуры полосы в device px (surface + 2×запас).
    h_px: u32,
    /// Depth-текстура Band-рендера (обязана совпадать размером с полосой).
    /// Кэшируется вместе с полосой: раньше создавалась заново на каждый
    /// miss (7+ МБ Depth32 на band-размере — чистый churn VRAM).
    depth_t: wgpu::Texture,
    /// View depth-текстуры полосы.
    depth_v: wgpu::TextureView,
}

/// Одноразовая инъекция blit-квада полосы в начало draw-плана level 0
/// следующего `render_impl`-вызова (Compose-путь скролл-композитора).
struct PendingBaseBlit {
    /// Bind group `image_bgl` поверх текстуры полосы (linear sampler).
    bind_group: wgpu::BindGroup,
    /// Смещение полосы относительно viewport-а: `band_top_css - scroll_y` (≤ 0).
    dy_css: f32,
    /// Ширина квада в CSS px (= ширине viewport-а).
    w_css: f32,
    /// Высота квада в CSS px (= высоте полосы).
    h_css: f32,
}

/// Финальная цель одного `render_impl`-вызова.
enum RenderPassMode {
    /// Обычный кадр: present, `FRAMES_RENDERED`, обновление `last_frame_hash`.
    Normal {
        /// Тотальный хэш кадра, посчитанный в `render()` (для skip-identical).
        frame_hash: u64,
    },
    /// Оффскрин-рендер полосы страницы: без present, без счётчиков кадров,
    /// без `last_frame_hash`; размеры «поверхности» = размеры полосы.
    Band {
        /// Цель рендера (view текстуры полосы).
        view: wgpu::TextureView,
        /// Ширина полосы в device px.
        w_px: u32,
        /// Высота полосы в device px.
        h_px: u32,
    },
    /// Композиция кадра из готовой полосы (через `pending_base_blit`) +
    /// overlay: present и `FRAMES_RENDERED`, но `last_frame_hash` обновляет
    /// вызывающий (`render()`) — хэш Compose-аргументов не описывает кадр.
    Compose,
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

/// Один загруженный face: TTF-байты + owned-кэш метрик, построенный один
/// раз при загрузке (образец — cosmic-text `FontSystem`: fontdb парсит
/// метаданные однажды, дальше живут кэши).
/// face_id 0 — default (bundled, передан в `Renderer::new`); остальные
/// `face_id` назначаются по мере lazy-загрузки из путей `FaceRecord`.
struct LoadedFace {
    /// Байты sfnt-шрифта. `Arc<[u8]>` (BUG-272 срез 6): для @font-face-фейсов
    /// это та же аллокация, что лежит в `FontRegistry::bytes_store` — вместо
    /// двух копий одного шрифта (в реестре и здесь) обе стороны разделяют один
    /// буфер через `read_face_bytes` → клон Arc.
    bytes: Arc<[u8]>,
    /// Метрики для горячего текстового пути (cmap-каскад, advance, baseline).
    /// `None` — face не распарсился при загрузке; такие face пропускаются
    /// в каскаде (эквивалент прежнего `Option<ParsedFace>` = None).
    metrics: Option<FaceMetrics>,
}

/// Owned-метрики face-а, независимые от лайфтайма `bytes`. Живут в
/// `LoadedFace` весь срок жизни рендера — снимают необходимость звать
/// `Font::parse` всех face-ов каждый кадр (тёплый кадр экономит 1.4–2.7 мс,
/// холодный — до 200 мс на 1000000-final.html).
struct FaceMetrics {
    /// `head.units_per_em` — масштаб font units → px.
    units_per_em: u16,
    /// `hhea.ascent` — для baseline (ascent ratio).
    ascent: i16,
    /// `hhea.descent` — для baseline (ascent ratio).
    descent: i16,
    /// Owned-копия cmap subtable: codepoint → glyph id без парсинга шрифта.
    cmap: OwnedCmap,
    /// hmtx advance per glyph id (хвост longHorMetric расширен по спеке).
    /// Индекс = glyph id; длина = num_glyphs.
    advances: Box<[u16]>,
}

/// Строит [`FaceMetrics`] по байтам шрифта. Возвращает `None`, если любая
/// из обязательных таблиц не парсится (head/hhea/cmap/hmtx/maxp).
fn build_face_metrics(bytes: &[u8]) -> Option<FaceMetrics> {
    let font = Font::parse(bytes).ok()?;
    let head = font.head().ok()?;
    let hhea = font.hhea().ok()?;
    let cmap = font.cmap().ok()?;
    let hmtx = font.hmtx().ok()?;
    let num_glyphs = font.maxp().ok()?.num_glyphs;
    let advances: Box<[u16]> = (0..num_glyphs)
        .map(|gid| hmtx.advance_width(gid).unwrap_or(0))
        .collect();
    Some(FaceMetrics {
        units_per_em: head.units_per_em,
        ascent: hhea.ascent,
        descent: hhea.descent,
        cmap: cmap.to_owned_cmap(),
        advances,
    })
}

/// Распарсенный face: Font + таблицы для растеризации. Borrow от
/// `LoadedFace.bytes`.
///
/// После введения `FaceMetrics` нужен только на «медленных» путях:
/// растеризация глифа при промахе atlas-кэша и нормализация
/// font-variation-осей (fvar/avar). Тёплый кадр (все глифы в атласе,
/// без variation settings) не парсит ни одного face-а.
struct ParsedFace<'a> {
    font: Font<'a>,
    head: Head,
    hmtx: Hmtx<'a>,
}

/// Ленивый per-frame кэш [`ParsedFace`]-ов: face парсится при первом
/// обращении внутри одного `render()`-вызова (промах атласа / variation
/// axes), повторные обращения бесплатны. На тёплом кадре не создаётся
/// ни одного `ParsedFace`.
struct LazyParsedFaces<'a> {
    faces: &'a [LoadedFace],
    /// Внешний `Option` — «ещё не пробовали», внутренний — результат парсинга.
    parsed: Vec<Option<Option<ParsedFace<'a>>>>,
}

impl<'a> LazyParsedFaces<'a> {
    fn new(faces: &'a [LoadedFace]) -> Self {
        Self { faces, parsed: Vec::new() }
    }

    /// Парсит face `id` при первом обращении; дальше отдаёт кэш.
    fn get(&mut self, id: usize) -> Option<&ParsedFace<'a>> {
        if id >= self.faces.len() {
            return None;
        }
        if self.parsed.len() < self.faces.len() {
            self.parsed.resize_with(self.faces.len(), || None);
        }
        if self.parsed[id].is_none() {
            let attempt = (|| {
                let font = Font::parse(&self.faces[id].bytes).ok()?;
                let head = font.head().ok()?;
                let hmtx = font.hmtx().ok()?;
                Some(ParsedFace { font, head, hmtx })
            })();
            self.parsed[id] = Some(attempt);
        }
        self.parsed[id].as_ref().and_then(|p| p.as_ref())
    }
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
    /// Target color space for wide-gamut output (ph3-color-management Step 4).
    /// Determines the chosen swap-chain format:
    /// `DisplayP3`/`Rec2020` → `Rgba16Float` (or first non-sRGB fallback);
    /// `Srgb` → non-sRGB preferred (existing behaviour).
    target_color_space: ColorSpace,

    /// PILI-CANVAS-BG: sRGB background color (root element's `background-color`)
    /// at the time the current frame started rendering. `None` means use white
    /// (CSS UA default). Used for the LoadOp clear colour at frame start.
    /// Converted from sRGB to `target_color_space` before being passed to the
    /// GPU clear colour (ph3-color-management Step 5).
    canvas_bg: Option<Color>,

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
    /// Blit-каскад mip-цепочки картинок: пасс «mip N−1 → mip N» при
    /// `register_image` (fullscreen triangle, bilinear = 2×2 box).
    mipgen_pipeline: wgpu::RenderPipeline,
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
    /// Кэш depth-текстур под bbox-офскрины (регион ≠ размеру окна/полосы):
    /// пасс с маленьким color-attachment обязан иметь depth того же размера
    /// (валидация wgpu). Ключ — (w, h) в device px; размеры регионов
    /// выровнены до 64 px, так что классов мало. Чистится при переполнении
    /// (> 16 записей) — обычная страница держит 1-3 размера.
    small_depth_cache: HashMap<(u32, u32), wgpu::TextureView>,
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
    /// Skip-identical-frame: поколение контента, не входящего в display list
    /// (картинки/GIF-кадры/снапшоты/шрифты/canvas-bg/промо-слои). Бампается
    /// каждой мутирующей операцией; входит в хэш кадра.
    content_generation: u64,
    /// Хэш последнего успешно отрисованного оконного кадра
    /// (display list + overlay + scroll + размер + `content_generation`).
    /// Совпадение со следующим кадром ⇒ пиксели идентичны ⇒ кадр пропускается.
    last_frame_hash: Option<u64>,
    /// Скролл-композитор страницы (EXPERIMENT.md §2): персистентная полоса
    /// документа. `None` — ещё не рисовалась (или сброшена сменой геометрии).
    page_band: Option<PageBandCache>,
    /// Blit-квад полосы для следующего Compose-рендера. Ставится только
    /// `try_page_compose`, снимается `take()`-ом в начале сбора вершин.
    pending_base_blit: Option<PendingBaseBlit>,
    /// Scroll-инвариантный ключ контента ПРОШЛОГО кадра. Полоса рисуется
    /// только по стабильному контенту (ключ совпал два кадра подряд):
    /// анимация/GIF/стриминг парсера меняют ключ каждый кадр, и рендер
    /// полосы (1.7× выше вьюпорта) там был бы дороже монолита — замерено
    /// 2026-07-10: 511 промахов из 629 кадров, медиана 10.7 → 21 мс.
    last_content_key: Option<u64>,
    /// GPU layer cache with LRU eviction (ADR-008 Phase 2).
    /// Tracks layer textures by stacking context ID + size for off-viewport eviction.
    layer_cache: crate::layer_cache::LayerCache,

    atlas: GlyphAtlas,
    /// Загруженные face-ы. `faces[0]` — default (bundled), используется когда
    /// `font-family` пуст или ни одно имя не нашлось через `FontProvider`.
    /// Остальные добавляются лениво при первом `DrawText` с известной family.
    faces: Vec<LoadedFace>,
    /// `face_id` bundled Golos Text Regular (DS-4) — default chrome UI font,
    /// used by [`Self::resolve_face_id`] when `font_family` is empty (every
    /// chrome `DrawText` call site) or requests reserved family `"Golos Text"`.
    chrome_face_id: Option<usize>,
    /// `face_id` bundled Golos Text Medium (DS-4) — reserved family `"Golos Text Medium"`.
    chrome_face_medium_id: Option<usize>,
    /// `face_id` bundled JetBrains Mono Regular (DS-4) — reserved family
    /// `"JetBrains Mono"`, used for the omnibox URL field and DevTools panels.
    mono_face_id: Option<usize>,
    /// `face_id` по абсолютному пути TTF — чтобы не грузить файл повторно.
    face_id_by_path: HashMap<PathBuf, usize>,
    /// Мемоизация `resolve_face_id`: хэш `(families, weight, style)` →
    /// `face_id`. Без него каждый `DrawText` каждого кадра гонял
    /// `to_lowercase` + `FontProvider::pick_face` (двe Vec-аллокации +
    /// матчинг). Ключ — u64-хэш (SipHash); коллизия теоретически возможна,
    /// но при десятках ключей пренебрежима (та же логика, что skip-frame
    /// hash). Сбрасывается в `set_font_provider` — новый провайдер
    /// (например, FontRegistry с @font-face) меняет ответы резолва.
    resolve_cache: HashMap<u64, usize>,
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
    count_texture_created_labeled("depth-texture", width, height);
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

/// Selects the best swap-chain format for the given `target` color space
/// from the adapter-reported `caps.formats` (ph3-color-management Step 4).
///
/// * `DisplayP3` / `Rec2020` — prefer `Rgba16Float` (wide-gamut linear float),
///   falling back to the first non-sRGB format when the adapter cannot provide it.
/// * `Srgb` — keep the existing non-sRGB preference so the GPU does not
///   perform automatic decode/encode that conflicts with the CPU-side ICC
///   pipeline; fall back to `caps.formats[0]`.
fn select_surface_format(
    caps: &wgpu::SurfaceCapabilities,
    target: ColorSpace,
) -> wgpu::TextureFormat {
    match target {
        ColorSpace::DisplayP3 | ColorSpace::Rec2020 => caps
            .formats
            .iter()
            .find(|f| **f == wgpu::TextureFormat::Rgba16Float)
            .copied()
            .unwrap_or_else(|| {
                caps.formats
                    .iter()
                    .find(|f| !f.is_srgb())
                    .copied()
                    .unwrap_or(caps.formats[0])
            }),
        _ => caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]),
    }
}

/// `true`, если пропуск идентичных кадров отключён (`LUMEN_NO_FRAME_SKIP=1`).
fn frame_skip_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_FRAME_SKIP").is_ok_and(|v| v == "1"))
}

/// BUG-274 diagnostics: total number of `create_texture` calls in this
/// process (all `Renderer`s). Printed by the `LUMEN_FRAME_LOG=2` phase log
/// to correlate pass-end cost with live-resource growth.
pub static TEXTURES_CREATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-274 diagnostics: bump [`TEXTURES_CREATED`].
fn count_texture_created() {
    TEXTURES_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Перепись созданных текстур по `(label, w, h)` — отвечает на вопрос п.23
/// «кто создаёт ~350 текстур за флинг». Заполняется только при
/// `LUMEN_FRAME_LOG=3`; в обычном режиме — один branch поверх счётчика.
type TextureCensusMap = HashMap<(&'static str, u32, u32), u64>;
static TEXTURE_CENSUS: std::sync::OnceLock<std::sync::Mutex<TextureCensusMap>> =
    std::sync::OnceLock::new();

/// Как [`count_texture_created`], но при `LUMEN_FRAME_LOG=3` дополнительно
/// пишет `(label, w, h)` в [`TEXTURE_CENSUS`] (печатается в `alloc:`-блоке).
fn count_texture_created_labeled(label: &'static str, width: u32, height: u32) {
    count_texture_created();
    if crate::frame_log_level() >= 3 {
        let census = TEXTURE_CENSUS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
        if let Ok(mut m) = census.lock() {
            *m.entry((label, width, height)).or_insert(0) += 1;
        }
    }
}

/// BUG-274 diagnostics: wall time spent inside `create_texture` +
/// `create_view` + `create_bind_group` for offscreen layers, in nanoseconds.
///
/// Separates *allocating* a render target from *using* it: if the cold-frame
/// `encode` cost lived in allocation, this counter would carry it. It does not
/// — which is the whole point of measuring before optimizing.
pub static TEXTURE_CREATE_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-274 diagnostics: offscreen-layer texture pool hits.
pub static TEXTURE_POOL_HITS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-274 diagnostics: offscreen-layer texture pool misses (→ fresh allocation).
pub static TEXTURE_POOL_MISSES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Frames that reached the GPU (`render` ran to completion and presented).
pub static FRAMES_RENDERED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Frames dropped by skip-identical-frame (hash matched the last presented frame).
///
/// A benchmark that claims to measure repaints must prove it caused repaints.
/// Without this counter a harness that silently perturbs nothing reports the
/// skip path's timing and looks like a spectacular optimization.
pub static FRAMES_SKIPPED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Reads a diagnostics counter.
pub fn load_counter(c: &std::sync::atomic::AtomicU64) -> u64 {
    c.load(std::sync::atomic::Ordering::Relaxed)
}

/// `true`, если скролл-композитор страницы отключён
/// (`LUMEN_NO_SCROLL_COMPOSITOR=1`). Диагностика: A/B картинки и скорости
/// на одном бинарнике (как `LUMEN_NO_BBOX_SCISSOR`).
fn scroll_compositor_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_SCROLL_COMPOSITOR").is_ok_and(|v| v == "1")
    })
}

/// `true`, если static/animated split скролл-композитора отключён
/// (`LUMEN_NO_ANIM_SPLIT=1`). Диагностика: A/B картинки и скорости на одном
/// бинарнике; при выключенном split анимируемые кадры рисуются монолитом,
/// как до среза.
fn anim_split_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_ANIM_SPLIT").is_ok_and(|v| v == "1")
    })
}

/// `true`, если bbox-scissor фильтр-пассов отключён (`LUMEN_NO_BBOX_SCISSOR=1`).
/// Диагностика: A/B-сравнение картинки и скорости на одном бинарнике.
fn bbox_scissor_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BBOX_SCISSOR").is_ok_and(|v| v == "1")
    })
}

/// `true`, если bbox-офскрины backdrop-фильтра отключены
/// (`LUMEN_NO_BBOX_BACKDROP=1`): ping-pong/кэш-текстуры backdrop-пути
/// создаются размером с родителя, как до среза. Диагностика: A/B-сравнение
/// картинки и скорости на одном бинарнике.
fn bbox_backdrop_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BBOX_BACKDROP").is_ok_and(|v| v == "1")
    })
}

/// `true`, если mip-цепочка картинок отключена (`LUMEN_NO_IMAGE_MIPS=1`):
/// возврат к CPU-ресайзу под каждый placed-размер (`src@WxH`-зоопарк) и
/// nearest-выбору mip-уровня в сэмплере. Диагностика: A/B-сравнение картинки,
/// скорости и памяти на одном бинарнике.
fn image_mips_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_IMAGE_MIPS").is_ok_and(|v| v == "1")
    })
}

/// `true`, если направленный сдвиг полосы скролл-композитора отключён
/// (`LUMEN_NO_BAND_BIAS=1`): полоса рецентрируется симметрично (вьюпорт по
/// центру), как до среза. По умолчанию **включён**: при промахе бо́льшая часть
/// запаса полосы кладётся ПО ходу скролла, поэтому непрерывный скролл проходит
/// дальше до следующего промаха (реже полная переросфинкция полосы). Меняет
/// только ПОЛОЖЕНИЕ полосы, не её содержимое — пиксельно идентично симметрии.
/// Диагностика: A/B скорости/p95 на одном бинарнике.
fn band_bias_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BAND_BIAS").is_ok_and(|v| v == "1")
    })
}

impl Renderer {
    pub fn new(window: Arc<Window>, font_bytes: Vec<u8>, target_color_space: ColorSpace) -> Result<Self, Box<dyn Error>> {
        // Валидируем шрифт сразу, чтобы при битом файле не падать в первом кадре.
        Font::parse(&font_bytes).map_err(|e| format!("парсинг шрифта: {e}"))?;
        block_on(Self::new_async(window, font_bytes, target_color_space))
    }

    async fn new_async(
        window: Arc<Window>,
        font_bytes: Vec<u8>,
        target_color_space: ColorSpace,
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
        // BUG-274: DX12 pays a fixed ~2.3ms CPU cost per render pass regardless of
        // frame area (doesn't amortize) — with ~270 passes/frame this dominates
        // idle CPU. Vulkan avoids it but a subset of Intel iGPUs present a fully
        // white window despite an error-free submit (BUG-275, WSI/driver issue,
        // undetectable from wgpu's own error scopes). `backend_probe::pick_backend`
        // draws a real probe frame and checks actual DWM presentation to pick the
        // first candidate that genuinely works; `None` falls through to the static
        // preference chain below (also used when the probe is disabled or this
        // isn't Windows). `WGPU_BACKEND` env-var still overrides both.
        let probed = crate::backend_probe::pick_backend(&window).await;
        let static_prefs: &[wgpu::Backends] = if cfg!(target_os = "windows") {
            &[wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL]
        } else {
            &[wgpu::Backends::PRIMARY, wgpu::Backends::GL]
        };
        let backend_prefs: Vec<wgpu::Backends> = probed
            .into_iter()
            .chain(static_prefs.iter().copied().filter(|b| Some(*b) != probed))
            .collect();
        let mut picked = None;
        for backends in backend_prefs {
            let instance = wgpu::Instance::new(
                &wgpu::InstanceDescriptor { backends, ..Default::default() }.with_env(),
            );
            let Ok(surface) = instance.create_surface(window.clone()) else {
                continue;
            };
            match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
            {
                Ok(adapter) => {
                    picked = Some((surface, adapter));
                    break;
                }
                Err(_) => continue,
            }
        }
        let (surface, adapter) =
            picked.ok_or("no GPU adapter under any candidate backend (DX12/Vulkan/GL)")?;
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
        let format = select_surface_format(&caps, target_color_space);
        // LUMEN_PRESENT=mailbox|immediate|fifo — эксперимент BUG-274/Vulkan-white:
        // выбор present mode из поддерживаемых драйвером (дефолт Fifo).
        let present_mode = match std::env::var("LUMEN_PRESENT").as_deref() {
            Ok("mailbox") if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) => {
                wgpu::PresentMode::Mailbox
            }
            Ok("immediate") if caps.present_modes.contains(&wgpu::PresentMode::Immediate) => {
                wgpu::PresentMode::Immediate
            }
            _ => wgpu::PresentMode::Fifo,
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let adapter_info = adapter.get_info();
        // BUG-274: имя адаптера в stderr — диагностика «не WARP ли это»
        // (программный растеризатор объясняет аномальный CPU/память).
        if crate::frame_log_enabled() {
            eprintln!(
                "[wgpu] adapter: {} ({:?}, {:?})",
                adapter_info.name, adapter_info.device_type, adapter_info.backend
            );
            eprintln!(
                "[wgpu] surface: format {:?} (of {:?}) alpha {:?} (of {:?}) present {:?}",
                config.format, caps.formats, config.alpha_mode, caps.alpha_modes,
                config.present_mode,
            );
        }
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
            target_color_space,
            gpu_fingerprint,
        )
    }

    /// Creates a headless `Renderer` for off-screen rendering without a winit window.
    /// Uses wgpu without a surface; renders to an internal `Rgba8Unorm` texture.
    /// Call [`render_to_image`](Self::render_to_image) to get pixels after rendering.
    ///
    /// # Errors
    /// Returns `Err` if no GPU adapter is available or device creation fails.
    pub fn new_headless(
        font_bytes: Vec<u8>,
        width: u32,
        height: u32,
        target_color_space: ColorSpace,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Font::parse(&font_bytes).map_err(|e| format!("парсинг шрифта: {e}"))?;
        block_on(Self::new_headless_async(font_bytes, width, height, target_color_space))
    }

    async fn new_headless_async(
        font_bytes: Vec<u8>,
        width: u32,
        height: u32,
        target_color_space: ColorSpace,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Mirror the windowed-mode fallback chain (BUG-057/274/275) minus the
        // probe: headless has no window to verify real presentation against, and
        // callers (tests, `--screenshot`, driver snapshots) need deterministic
        // backend selection run to run. `WGPU_BACKEND` still overrides.
        let backend_prefs: &[wgpu::Backends] = if cfg!(target_os = "windows") {
            &[wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL]
        } else {
            &[wgpu::Backends::PRIMARY, wgpu::Backends::GL]
        };
        // No surface needed — request adapter without compatible_surface constraint.
        let mut picked = None;
        for &backends in backend_prefs {
            let instance = wgpu::Instance::new(
                &wgpu::InstanceDescriptor { backends, ..Default::default() }.with_env(),
            );
            match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
            {
                Ok(adapter) => {
                    picked = Some(adapter);
                    break;
                }
                Err(_) => continue,
            }
        }
        let adapter =
            picked.ok_or("no GPU adapter under any candidate backend (DX12/Vulkan/GL)")?;
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
        // Target color space is recorded for render path queries but headless
        // readback always returns sRGB bytes for snapshot determinism.
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
            target_color_space,
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
        target_color_space: ColorSpace,
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
        count_texture_created_labeled("glyph-atlas", ATLAS_DIM, ATLAS_DIM);
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            // Трилинейный выбор mip-уровня: даунскейл картинок делает GPU по
            // mip-цепочке (см. make_gpu_image_entry_mipped). На 1-mip
            // текстурах (снапшоты, полоса) LOD клампится в 0 — поведение
            // не меняется.
            mipmap_filter: if image_mips_disabled() {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            },
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

        // ── Mipgen pipeline (mip-цепочка картинок) ────────────────────────
        // Пасс «mip N−1 → mip N» без depth и без блендинга: fullscreen
        // triangle пишет bilinear-выборку источника (2×2 box-даунскейл).
        let mipgen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mipgen-shader"),
            source: wgpu::ShaderSource::Wgsl(MIPGEN_SHADER_SRC.into()),
        });
        let mipgen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mipgen-layout"),
            bind_group_layouts: &[&image_bgl],
            push_constant_ranges: &[],
        });
        let mipgen_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mipgen-pipeline"),
            layout: Some(&mipgen_layout),
            vertex: wgpu::VertexState {
                module: &mipgen_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mipgen_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    // Картинки всегда Rgba8Unorm (см. make_gpu_image_entry),
                    // не surface format.
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
                    // Write only RGB — preserve destination alpha so the parent
                    // layer's opacity isn't reduced by blur-edge transparency.
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let atlas = GlyphAtlas::new(ATLAS_DIM);

        // DS-4: bundled chrome UI faces (Golos Text + JetBrains Mono), loaded
        // eagerly right after the default (Inter) face at index 0 — mirrors
        // `FemtovgBackend::new`'s eager `add_font_mem` for the same fonts.
        // A `None` id (metrics failed to parse — shouldn't happen for a
        // bundled, CI-validated asset) just leaves `resolve_face_id` falling
        // back to the default face 0.
        let mut faces = vec![LoadedFace {
            metrics: build_face_metrics(&font_bytes),
            bytes: Arc::from(font_bytes),
        }];
        let push_chrome_face = |faces: &mut Vec<LoadedFace>, bytes: &'static [u8]| {
            build_face_metrics(bytes).map(|metrics| {
                let id = faces.len();
                faces.push(LoadedFace { metrics: Some(metrics), bytes: Arc::from(bytes) });
                id
            })
        };
        let chrome_face_id =
            push_chrome_face(&mut faces, crate::chrome_fonts::GOLOS_TEXT_REGULAR);
        let chrome_face_medium_id =
            push_chrome_face(&mut faces, crate::chrome_fonts::GOLOS_TEXT_MEDIUM);
        let mono_face_id =
            push_chrome_face(&mut faces, crate::chrome_fonts::JETBRAINS_MONO_REGULAR);

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
            mipgen_pipeline,
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
            content_generation: 0,
            last_frame_hash: None,
            page_band: None,
            pending_base_blit: None,
            last_content_key: None,
            layer_cache: crate::layer_cache::LayerCache::new(),
            composite_pipeline,
            composite_bgl,
            blend_pipeline,
            blend_bgl,
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
            small_depth_cache: HashMap::new(),
            backdrop_cache: crate::backdrop_cache::BackdropCache::new(),
            backdrop_cache_textures: HashMap::new(),
            gradient_bgl,
            gradient_pipeline,
            scratch_layer: None,
            layer_sampler,
             layer_textures: Vec::new(),
             surface_format: format,
             target_color_space,
             canvas_bg: None,
             atlas,
            faces,
            chrome_face_id,
            chrome_face_medium_id,
            mono_face_id,
            face_id_by_path: HashMap::new(),
            resolve_cache: HashMap::new(),
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
        self.content_generation = self.content_generation.wrapping_add(1);
        self.font_provider = provider;
        // Новый провайдер может отвечать иначе на те же (families, weight,
        // style) — например, FontRegistry с загруженным @font-face.
        self.resolve_cache.clear();
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
        self.content_generation = self.content_generation.wrapping_add(1);
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
        // DS-4: chrome never queries the CSS FontProvider — every chrome
        // `DrawText` passes an empty `font_family` (page content always has a
        // non-empty one, from the UA/author stylesheet's font-family cascade),
        // so an empty list defaults to the bundled chrome UI face (Golos
        // Text). Reserved bundled family names resolve directly here,
        // independent of whether a `FontProvider` is installed at all.
        if families.is_empty() {
            return self.chrome_face_id.unwrap_or(0);
        }
        for fam in families {
            match fam.as_str() {
                "Golos Text" => return self.chrome_face_id.unwrap_or(0),
                "Golos Text Medium" => {
                    return self.chrome_face_medium_id.or(self.chrome_face_id).unwrap_or(0);
                }
                "JetBrains Mono" => return self.mono_face_id.unwrap_or(0),
                _ => {}
            }
        }
        let Some(provider) = self.font_provider.clone() else {
            return 0;
        };
        // Мемоизация: горячий путь (каждый DrawText каждого кадра) — один
        // hash-lookup без аллокаций вместо to_lowercase + pick_face.
        let cache_key = Self::resolve_cache_key(families, weight, style);
        if let Some(&id) = self.resolve_cache.get(&cache_key) {
            return id;
        }
        let resolved = self.resolve_face_id_uncached(families, weight, style, &provider);
        self.resolve_cache.insert(cache_key, resolved);
        resolved
    }

    /// Ключ мемо-кэша [`Self::resolve_face_id`]: хэш `(families, weight,
    /// style)` без аллокаций. Вынесен, чтобы префетч и резолв считали ключ
    /// одинаково.
    fn resolve_cache_key(families: &[String], weight: FontWeight, style: FontStyle) -> u64 {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for fam in families {
            h.write(fam.as_bytes());
            h.write_u8(0xFF); // разделитель — ["ab","c"] ≠ ["a","bc"]
        }
        h.write_u16(weight.0);
        h.write_u8(match style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
        });
        h.finish()
    }

    /// Generic CSS-family (`serif`/`sans-serif`/…) — резолвится в default,
    /// провайдер не спрашивается (Phase 0 без per-generic-fallback таблицы).
    fn is_generic_family(lowercase_name: &str) -> bool {
        matches!(
            lowercase_name,
            "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
        )
    }

    /// Конверсия paint-стиля в стиль `FontProvider`-а.
    fn css_style_of(style: FontStyle) -> CssFontStyle {
        match style {
            FontStyle::Normal => CssFontStyle::Normal,
            FontStyle::Italic => CssFontStyle::Italic,
            FontStyle::Oblique => CssFontStyle::Oblique,
        }
    }

    /// Параллельная предзагрузка face-ов для всех `DrawText` кадра
    /// (p1-exp-wgpu-only, ярус 1 «вынос загрузки face-ов с render-пути»).
    ///
    /// Раньше первый кадр страницы грузил каждый новый face
    /// ПОСЛЕДОВАТЕЛЬНО внутри пре-резолва: `fs::read` + WOFF-декод +
    /// `build_face_metrics` (~180 мс на 1000000-final.html). Здесь та же
    /// работа выполняется до резолва пачкой в scoped-потоках: диск и декод
    /// независимых face-ов идут параллельно, вставка в `self.faces` — на
    /// UI-потоке в детерминированном порядке (порядок первого появления в
    /// display list-е, как у последовательного кода).
    ///
    /// Семантика [`Self::resolve_face_id_uncached`] сохранена: грузится
    /// только первый `pick_face`-кандидат каждого списка family; если его
    /// загрузка/парсинг провалились — face просто не вставляется, и
    /// последующий последовательный резолв повторит попытку и упадёт на
    /// следующую family штатным путём (редкий случай битого шрифта).
    /// Тёплый кадр (все ключи в `resolve_cache`) не делает ничего.
    fn prefetch_faces_parallel(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
    ) {
        let Some(provider) = self.font_provider.clone() else {
            return;
        };
        // Кандидаты: путь + байты из провайдера (@font-face virtual path)
        // либо None → fs::read в воркере.
        let mut jobs: Vec<(PathBuf, Option<Arc<[u8]>>)> = Vec::new();
        let mut seen_keys: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut scheduled: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for cmd in content.iter().chain(overlay.iter()) {
            let DisplayCommand::DrawText { font_family, font_weight, font_style, .. } = cmd
            else {
                continue;
            };
            let key = Self::resolve_cache_key(font_family, *font_weight, *font_style);
            if self.resolve_cache.contains_key(&key) || !seen_keys.insert(key) {
                continue;
            }
            for fam in font_family {
                // DS-4: reserved bundled chrome names resolve without the
                // provider in `resolve_face_id` — skip them here too, else
                // every frame re-attempts a `pick_face` lookup for hot chrome
                // text (omnibox, DevTools) that never gets cached (the actual
                // resolve short-circuits before reaching `resolve_cache`).
                if matches!(fam.as_str(), "Golos Text" | "Golos Text Medium" | "JetBrains Mono") {
                    continue;
                }
                let lc = fam.to_lowercase();
                if Self::is_generic_family(&lc) {
                    continue;
                }
                let Some(rec) =
                    provider.pick_face(fam, font_weight.0, Self::css_style_of(*font_style))
                else {
                    continue;
                };
                if !self.face_id_by_path.contains_key(&rec.path)
                    && !scheduled.contains(&rec.path)
                {
                    let mem = provider.read_face_bytes(&rec.path);
                    scheduled.insert(rec.path.clone());
                    jobs.push((rec.path, mem));
                }
                break; // как в резолве: первый pick_face-хит завершает перебор
            }
        }
        if jobs.is_empty() {
            return;
        }

        // Воркеры разбирают job-ы через атомарный курсор; результат кладётся
        // по индексу job-а — порядок вставки детерминирован.
        let n_workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZeroUsize::get)
            .min(jobs.len())
            .min(8);
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        // Слот результата job-а: байты шрифта + построенные метрики.
        type FaceSlot = std::sync::Mutex<Option<(Arc<[u8]>, FaceMetrics)>>;
        let results: Vec<FaceSlot> =
            jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|s| {
            for _ in 0..n_workers {
                s.spawn(|| {
                    loop {
                        let i = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((path, mem)) = jobs.get(i) else {
                            break;
                        };
                        // `mem` (Arc из @font-face-реестра) клонируется как
                        // счётчик ссылок; диск читается в новый Arc (BUG-272).
                        let raw: Arc<[u8]> = match mem {
                            Some(bytes) => Arc::clone(bytes),
                            None => match std::fs::read(path) {
                                Ok(b) => Arc::from(b),
                                Err(_) => continue,
                            },
                        };
                        // Ошибки декода/парсинга здесь НЕ логируются: face
                        // просто не вставляется, последовательный резолв
                        // повторит попытку и залогирует штатно (без дублей).
                        // `Ok(None)` (уже sfnt — как все @font-face-байты) отдаёт
                        // тот же Arc, что и реестр: рендер разделяет буфер.
                        let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
                            Ok(Some(decoded)) => Arc::from(decoded),
                            Ok(None) => raw,
                            Err(_) => continue,
                        };
                        let Some(metrics) = build_face_metrics(&bytes) else {
                            continue;
                        };
                        if let Ok(mut slot) = results[i].lock() {
                            *slot = Some((bytes, metrics));
                        }
                    }
                });
            }
        });

        for ((path, _), slot) in jobs.into_iter().zip(results) {
            let Ok(mut guard) = slot.lock() else { continue };
            let Some((bytes, metrics)) = guard.take() else {
                continue; // битый шрифт: последовательный резолв повторит и залогирует
            };
            let id = self.faces.len();
            self.faces.push(LoadedFace { bytes, metrics: Some(metrics) });
            self.face_id_by_path.insert(path, id);
        }
    }

    /// Полный (немемоизированный) резолв — вынесен из [`Self::resolve_face_id`],
    /// который добавляет кэш поверх.
    fn resolve_face_id_uncached(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        provider: &Arc<dyn FontProvider>,
    ) -> usize {
        for fam in families {
            let lc = fam.to_lowercase();
            if Self::is_generic_family(&lc) {
                continue;
            }
            let Some(rec) = provider.pick_face(fam, weight.0, Self::css_style_of(style)) else {
                continue;
            };
            if let Some(&id) = self.face_id_by_path.get(&rec.path) {
                return id;
            }
            // @font-face in-memory байты (virtual path) или диск для системных шрифтов.
            // Реестр отдаёт Arc (клон = счётчик ссылок), диск — новый Arc (BUG-272).
            let raw: Arc<[u8]> = if let Some(mem_bytes) = provider.read_face_bytes(&rec.path) {
                mem_bytes
            } else {
                let Ok(disk_bytes) = std::fs::read(&rec.path) else {
                    continue;
                };
                Arc::from(disk_bytes)
            };
            // Transparent WOFF/WOFF2 → sfnt conversion before parsing.
            // `Ok(None)` (@font-face-байты уже sfnt) переиспользует Arc реестра.
            let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
                Ok(Some(decoded)) => Arc::from(decoded),
                Ok(None) => raw,
                Err(e) => {
                    eprintln!("[font] WOFF decode failed {}: {e}", rec.path.display());
                    continue;
                }
            };
            let Some(metrics) = build_face_metrics(&bytes) else {
                eprintln!("[font] parse failed {}", rec.path.display());
                continue;
            };
            let id = self.faces.len();
            self.faces.push(LoadedFace { bytes, metrics: Some(metrics) });
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
        self.content_generation = self.content_generation.wrapping_add(1);
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

        // CPU-копия декода нужна только старому пути (on-demand resize при
        // DrawImage). Mip-путь читает исключительно GPU-текстуру — не платим
        // RAM за второй экземпляр каждой картинки.
        if image_mips_disabled() {
            self.raw_images.insert(src.clone(), image.clone());
        }

        // Загружаем оригинал в GPU с mip-цепочкой (blit-каскад): даунскейл
        // под любой placed-размер делает сэмплер по mip-ам, CPU-ресайзы и
        // текстуры "src@WxH" не нужны. Kill-switch LUMEN_NO_IMAGE_MIPS=1
        // возвращает старый путь (1 mip + CPU-ресайзы в ensure/prefetch).
        let mut rgba = convert_to_rgba(image);
        // Apply ICC colour correction before GPU upload so wide-gamut (Display P3,
        // Rec2020) photos render correctly on sRGB displays.
        if let Some(ref profile) = image.icc_profile {
            correct_rgba_pixels(&mut rgba, profile);
        }
        let gi = if image_mips_disabled() {
            self.make_gpu_image_entry(&rgba, image.width, image.height)
        } else {
            self.make_gpu_image_entry_mipped(&rgba, image.width, image.height)
        };
        self.images.insert(src, gi);
        Ok(())
    }

    /// Вычисляет GPU-ключ без мутации — только `&self`. Используется внутри
    /// render-цикла, где `lazy_faces` держит `&self.faces`.
    /// Предполагается, что нужная текстура уже создана через `ensure_image_gpu_key`.
    fn compute_image_gpu_key(&self, src: &str, box_rect: Rect, fit: ObjectFit, pos: ObjectPosition) -> String {
        // Mip-путь: текстура одна (оригинал с mip-цепочкой), ключ всегда src;
        // масштабирование делает трилинейный сэмплер.
        if !image_mips_disabled() {
            return src.to_owned();
        }
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
        // Mip-путь: ресайз-текстуры не создаются, оригинал уже загружен
        // с mip-цепочкой в register_image.
        if !image_mips_disabled() {
            return;
        }
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

    /// Параллельный image pre-pass (p1-exp-wgpu-only, ярус 1 «не рисовать
    /// лишнее»): CPU-ресайзы всех `DrawImage`/`LazyImageSlot` кадра.
    ///
    /// Раньше холодный кадр ресайзил картинки ПОСЛЕДОВАТЕЛЬНО внутри
    /// [`Self::ensure_image_gpu_key`] (~158 мс на 1000000-final.html,
    /// 12 картинок) — это и была почти вся «фаза faces» холодного кадра
    /// (замер faces-sub 2026-07-09). Здесь CPU-часть (resize, RGBA-конверсия,
    /// ICC-коррекция) выполняется в scoped-потоках, заимствуя
    /// `self.raw_images` разделяемо; заливка GPU-текстур — после, на
    /// UI-потоке, в детерминированном порядке job-ов. Тёплый кадр (все
    /// gpu_key уже в `self.images`) не делает ничего.
    fn prefetch_image_resizes_parallel(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
    ) {
        // Mip-путь: CPU-ресайзов нет вовсе — pre-pass не нужен.
        if !image_mips_disabled() {
            return;
        }
        // (gpu_key, src, tw, th) — уникальные недостающие ресайзы кадра.
        let mut jobs: Vec<(String, String, u32, u32)> = Vec::new();
        let mut scheduled: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cmd in content.iter().chain(overlay.iter()) {
            let (DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. }
            | DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. }) = cmd
            else {
                continue;
            };
            let Some(raw) = self.raw_images.get(src) else {
                continue;
            };
            let placed = fit_image_rect(*rect, (raw.width, raw.height), *object_fit, *object_position);
            let tw = placed.width.round().max(1.0) as u32;
            let th = placed.height.round().max(1.0) as u32;
            if tw == raw.width && th == raw.height {
                continue; // интринсик-размер: текстура есть из register_image
            }
            let gpu_key = format!("{src}@{tw}x{th}");
            if self.images.contains_key(&gpu_key) || !scheduled.insert(gpu_key.clone()) {
                continue;
            }
            jobs.push((gpu_key, src.clone(), tw, th));
        }
        if jobs.is_empty() {
            return;
        }

        // CPU-часть параллельно: воркеры разбирают job-ы атомарным курсором,
        // raw_images заимствуется разделяемо (только чтение).
        let raw_images = &self.raw_images;
        let n_workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZeroUsize::get)
            .min(jobs.len())
            .min(8);
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        let results: Vec<std::sync::Mutex<Option<Vec<u8>>>> =
            jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|s| {
            for _ in 0..n_workers {
                s.spawn(|| {
                    loop {
                        let i = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((_, src, tw, th)) = jobs.get(i) else {
                            break;
                        };
                        let Some(raw) = raw_images.get(src) else {
                            continue;
                        };
                        let resized = if *tw <= raw.width && *th <= raw.height {
                            resize_area_avg(raw, *tw, *th)
                        } else {
                            resize_bilinear(raw, *tw, *th)
                        };
                        let mut rgba = convert_to_rgba(&resized);
                        // ICC-профиль лежит на оригинале — resize_* его не переносит.
                        if let Some(ref profile) = raw.icc_profile {
                            correct_rgba_pixels(&mut rgba, profile);
                        }
                        if let Ok(mut slot) = results[i].lock() {
                            *slot = Some(rgba);
                        }
                    }
                });
            }
        });

        // Заливка GPU-текстур — на UI-потоке, порядок детерминирован.
        for ((gpu_key, _, tw, th), slot) in jobs.into_iter().zip(results) {
            let Ok(mut guard) = slot.lock() else { continue };
            let Some(rgba) = guard.take() else { continue };
            let gi = self.make_gpu_image_entry(&rgba, tw, th);
            self.images.insert(gpu_key, gi);
        }
    }

    /// Создаёт `GpuImage` из RGBA8-буфера заданного размера.
    /// `&self` достаточно — мутировать нужно только `images`, это делает caller.
    fn make_gpu_image_entry(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
        count_texture_created_labeled("image", width, height);
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

    /// Создаёт `GpuImage` с полной mip-цепочкой: mip 0 заливается с CPU,
    /// остальные уровни строятся GPU blit-каскадом (`mipgen_pipeline`,
    /// bilinear = 2×2 box на пасс). Замена CPU-ресайзов под каждый
    /// placed-размер: одна текстура на `src`, даунскейл при отрисовке делает
    /// трилинейный сэмплер (как в Chromium). Стоимость каскада — по одному
    /// крошечному пассу на уровень, один раз на `register_image`.
    fn make_gpu_image_entry_mipped(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
        count_texture_created_labeled("image-mipped", width, height);
        // floor(log2(max(w,h))) + 1; width/height ≥ 1 гарантированы caller-ом.
        let mip_level_count = 32 - width.max(height).leading_zeros();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumen-image-texture-mipped"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Не sRGB — как make_gpu_image_entry (surface тоже non-sRGB).
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
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
        if mip_level_count > 1 {
            let mut encoder = self.device.create_command_encoder(
                &wgpu::CommandEncoderDescriptor { label: Some("lumen-image-mipgen") },
            );
            let mip_view = |level: u32| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("lumen-image-mip-level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            };
            let mut src_view = mip_view(0);
            for level in 1..mip_level_count {
                let dst_view = mip_view(level);
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mipgen-bg"),
                    layout: &self.image_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                        },
                    ],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mipgen-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &dst_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            // Fullscreen triangle перекрывает уровень целиком.
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.mipgen_pipeline);
                pass.set_bind_group(0, &bg, &[]);
                pass.draw(0..3, 0..1);
                drop(pass);
                src_view = dst_view;
            }
            self.queue.submit(Some(encoder.finish()));
        }
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
        self.content_generation = self.content_generation.wrapping_add(1);
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
        self.content_generation = self.content_generation.wrapping_add(1);
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

        count_texture_created_labeled("layer-snapshot", width, height);
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
        self.content_generation = self.content_generation.wrapping_add(1);
        self.layer_snapshots.remove(&id);
    }

    /// Удаляет все снимки (например, при переходе на новую страницу).
    pub fn clear_layer_snapshots(&mut self) {
        self.content_generation = self.content_generation.wrapping_add(1);
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
        self.content_generation = self.content_generation.wrapping_add(1);
        for ord in self.backdrop_cache.on_memory_pressure(level) {
            self.backdrop_cache_textures.remove(&ord);
        }
    }

    /// Forwards a memory-pressure signal to the glyph atlas so it can evict
    /// cached entries (ADR-008 §10H).  Medium: evict ~50% LRU glyphs.
    /// High: clear entirely.  Wire into the shell's `MemoryPressureSource` poll loop.
    pub fn atlas_on_memory_pressure(&mut self, level: lumen_core::ext::MemoryPressureLevel) {
        self.content_generation = self.content_generation.wrapping_add(1);
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
        self.content_generation = self.content_generation.wrapping_add(1);
        self.layer_cache.promote_layer(node_id, width, height)
    }

    /// Returns `true` if the given node has a promoted GPU layer.
    pub fn is_layer_promoted(&self, node_id: u32) -> bool {
        self.layer_cache.is_layer_promoted(node_id)
    }

    /// Remove the promoted GPU layer for a node, freeing its cache entry.
    pub fn demote_layer(&mut self, node_id: u32) {
        self.content_generation = self.content_generation.wrapping_add(1);
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
        self.content_generation = self.content_generation.wrapping_add(1);
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
        self.content_generation = self.content_generation.wrapping_add(1);
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

    /// Target color space for this renderer's output surface.
    ///
    /// Informs the compositor and paint steps whether depth → display conversion
    /// must be performed. Srgb ≈ legacy path; DisplayP3/Rec2020 enable wide-gamut
    /// output (ph3-color-management Step 4).
    #[must_use]
    pub fn target_color_space(&self) -> ColorSpace {
        self.target_color_space
    }

    /// Updates the root-element canvas background used as the framebuffer clear colour.
    ///
    /// Receives an sRGB `Color` (8-bit gamma-encoded) from shell. Stored verbatim;
    /// the conversion to the current `target_color_space` happens lazily at the
    /// start of each `render()` call inside `flush_batch` (ph3-color-management Step 5).
    pub fn set_canvas_background(&mut self, color: Option<Color>) {
        if self.canvas_bg != color {
            self.content_generation = self.content_generation.wrapping_add(1);
            self.canvas_bg = color;
        }
    }

    fn wgpu_color_for_canvas_bg(color: &Color, target: ColorSpace) -> [f32; 4] {
        fn srgb_gamma_decode(c: f32) -> f32 {
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        fn srgb_gamma_encode(c: f32) -> f32 {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
        }
        fn rec2020_gamma_encode(c: f32) -> f32 {
            let c = c.clamp(0.0, 1.0);
            if c < 0.018053_968 { 4.5 * c } else { 1.099_296_8 * c.powf(0.45) - 0.099_296_82 }
        }
        fn srgb_linear_to_p3_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
            (0.822_462_14 * r + 0.177_537_87 * g, 0.033_076_44 * r + 0.966_923_53 * g, -0.028_916_533 * r - 0.080_738_96 * g + 1.109_655_5 * b)
        }
        fn srgb_linear_to_rec2020_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
            (0.627_403_9 * r + 0.329_275_13 * g + 0.043_320_952 * b, 0.069_097_29 * r + 0.919_541_4 * g + 0.011_361_319 * b, 0.016_391_587 * r + 0.088_012_21 * g + 0.895_596_2 * b)
        }

        let r = color.r as f32 / 255.0;
        let g = color.g as f32 / 255.0;
        let b = color.b as f32 / 255.0;
        let a = color.a as f32 / 255.0;
        match target {
            ColorSpace::Srgb | ColorSpace::Lab => [r, g, b, a],
            ColorSpace::DisplayP3 => {
                let (pr, pg, pb) = srgb_linear_to_p3_linear(srgb_gamma_decode(r), srgb_gamma_decode(g), srgb_gamma_decode(b));
                [srgb_gamma_encode(pr), srgb_gamma_encode(pg), srgb_gamma_encode(pb), a]
            }
            ColorSpace::Rec2020 => {
                let (rr, rg, rb) = srgb_linear_to_rec2020_linear(srgb_gamma_decode(r), srgb_gamma_decode(g), srgb_gamma_decode(b));
                [rec2020_gamma_encode(rr), rec2020_gamma_encode(rg), rec2020_gamma_encode(rb), a]
            }
        }
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
        use std::sync::atomic::Ordering::Relaxed;

        // Try to acquire a texture from the pool before creating a new one (Phase 2).
        if let Some(pooled) = self.texture_pool.acquire(width, height) {
            TEXTURE_POOL_HITS.fetch_add(1, Relaxed);
            return OffscreenLayer {
                texture: pooled.texture,
                view: pooled.view,
                bind_group: pooled.bind_group,
                width: pooled.width,
                height: pooled.height,
            };
        }

        // Pool miss: allocate a new texture.
        TEXTURE_POOL_MISSES.fetch_add(1, Relaxed);
        count_texture_created_labeled("opacity-layer", width, height);
        let t_alloc0 = std::time::Instant::now();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("opacity-layer"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            // COPY_SRC needed for encoder.copy_texture_to_texture in blend compositing.
            // COPY_DST added for the backdrop bbox path: pooled ping-pong
            // textures receive the parent-region copy (copy_texture_to_texture).
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
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
        TEXTURE_CREATE_NANOS.fetch_add(
            u64::try_from(t_alloc0.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Relaxed,
        );
        self.texture_pool.update_size(1); // Track new allocation.
        OffscreenLayer { texture, view, bind_group, width, height }
    }

    /// Возвращает offscreen-слой в texture_pool для переиспользования.
    /// Безопасно сразу после записи команд: команды исполняются в порядке
    /// encoder-а, повторное использование той же текстуры позже в кадре
    /// упорядочено записью (та же дисциплина, что у слотов layer_textures).
    fn release_layer_to_pool(&mut self, layer: OffscreenLayer) {
        self.texture_pool.release(crate::texture_pool::PooledTexture {
            texture: layer.texture,
            view: layer.view,
            bind_group: layer.bind_group,
            width: layer.width,
            height: layer.height,
        });
    }

    /// Depth-текстура под пасс с bbox-офскрином (регион меньше окна/полосы).
    /// Кэшируется по размеру: blur-пассы backdrop-фильтра гоняются каждый
    /// кадр, а классов размеров мало (выравнивание до 64 px).
    fn small_depth_view(&mut self, width: u32, height: u32) -> wgpu::TextureView {
        if let Some(v) = self.small_depth_cache.get(&(width, height)) {
            return v.clone();
        }
        if self.small_depth_cache.len() > 16 {
            self.small_depth_cache.clear();
        }
        let (_t, v) = create_depth_texture(&self.device, width, height);
        self.small_depth_cache.insert((width, height), v.clone());
        v
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
            count_texture_created_labeled("blend-scratch-layer", width, height);
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
        count_texture_created_labeled("backdrop-cache-layer", width, height);
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
                // Band↔window флап размеров на каждом miss полосы: вытесняемую
                // текстуру вернуть в пул, а не дропать — следующий кадр другого
                // режима возьмёт её обратно (классов размера всего два).
                let t = self.create_layer_texture(width, height);
                let old = std::mem::replace(&mut self.layer_textures[i], t);
                self.release_layer_to_pool(old);
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
    /// Скролл-композитор страницы, срез 1 (EXPERIMENT.md §2): пробует собрать
    /// кадр из персистентной полосы документа вместо перерисовки контента.
    ///
    /// Применим, когда кадр — чистая трансляция контента: оконный рендер,
    /// нет горизонтального скролла, скролл ДВИЖЕТСЯ (кадры «DL изменился,
    /// скролл тот же» — анимация, ввод — идут монолитом) и в контенте нет
    /// `BeginStickyLayer` — единственной команды, чей результат зависит от
    /// scroll_y нелинейно (sticky-кламп); всё остальное транслируется
    /// равномерно, включая fixed (см. BUG-159: fixed не получает спец-
    /// обработки в рендере — полоса воспроизводит его поведение бит-в-бит).
    ///
    /// Ключ полосы scroll-инвариантен — хэш контента при scroll (0,0) +
    /// `content_generation` + геометрия (урок п.15: скролл в ключе = промах
    /// каждый кадр = 30× регрессия). Промах стоит ОДИН рендер контента
    /// (в полосу) + дешёвую композицию (blit + overlay) — урок п.15 №2.
    ///
    /// Static/animated split (EXPERIMENT.md §2): при непустых `anim_ranges`
    /// (диапазоны анимируемых сегментов от
    /// [`build_display_list_ordered_with_anim_split`]) полоса строится и
    /// хэшируется ТОЛЬКО по статичной части списка, а сегменты рисуются
    /// поверх blit-а каждым кадром (реплей их transform/clip-контекста —
    /// `anim_split_compose_plan`). Так медленный скролл анимированной
    /// страницы попадает в полосу, хотя display list меняется каждый кадр.
    /// Painter's-order guard: если статичная команда позже сегмента
    /// пересекает его bbox — split небезопасен, кадр идёт монолитом.
    /// Kill-switch: `LUMEN_NO_ANIM_SPLIT=1`.
    ///
    /// [`build_display_list_ordered_with_anim_split`]: crate::display_list::build_display_list_ordered_with_anim_split
    /// [`anim_split_compose_plan`]: crate::display_list::anim_split_compose_plan
    ///
    /// Возвращает `Ok(true)`, если кадр показан этим путём.
    fn try_page_compose(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
        anim_ranges: &[std::ops::Range<usize>],
    ) -> Result<bool, wgpu::SurfaceError> {
        if self.surface.is_none()
            || scroll_compositor_disabled()
            || scroll_x != 0.0
            || content.is_empty()
            || content
                .iter()
                .any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }))
        {
            return Ok(false);
        }
        let (sw, sh) = self.surface_dims();
        let dpr = self.scale_factor.max(1e-6) as f32;
        let vp_h_css = sh as f32 / dpr;
        // Запас полосы: по 3/4 вьюпорта сверху и снизу, но не больше 768 CSS px.
        let margin_css = (vp_h_css * 0.75).min(768.0).floor();
        let band_h_px = sh + 2 * (margin_css * dpr).round() as u32;
        if band_h_px > self.device.limits().max_texture_dimension_2d {
            return Ok(false);
        }
        let band_h_css = band_h_px as f32 / dpr;

        // Static/animated split: план оверлея сегментов. При конфликте
        // painter's order план сам расширяет диапазоны tail-split-ом —
        // хэш/полосу дальше считаем по ЕГО effective-диапазонам. Полный
        // отказ (нереплеябельный контекст и т.п.) — split выключается на
        // кадр, ключ считается по полному списку (= поведение до среза).
        let mut ranges: &[std::ops::Range<usize>] = if anim_split_disabled() {
            &[]
        } else {
            anim_ranges
        };
        let effective_ranges: Vec<std::ops::Range<usize>>;
        let seg_plan: Option<crate::display_list::DisplayList> = if ranges.is_empty() {
            None
        } else {
            match crate::display_list::anim_split_compose_plan(content, ranges) {
                Some((p, eff)) => {
                    effective_ranges = eff;
                    ranges = &effective_ranges;
                    Some(p)
                }
                None => {
                    ranges = &[];
                    None
                }
            }
        };

        // Scroll-инвариантный ключ содержимого полосы (по статике при split-е).
        let key = {
            use std::hash::Hasher;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            h.write_u64(crate::display_list::hash_display_list_skipping(
                content, ranges, &[], 0.0, 0.0, sw, band_h_px,
            ));
            h.write_u64(self.content_generation);
            h.finish()
        };

        // Контент стабилен, если его ключ совпал с ключом прошлого кадра.
        // Нестабильный контент (анимация, GIF, стриминг парсера) в полосу не
        // рисуем: промах на КАЖДОМ кадре при полосе 1.7× вьюпорта дороже
        // монолита (замер 2026-07-10: медиана 10.7 → 21 мс). После первого
        // же стабильного кадра полоса легализуется, а редкие тики (GIF
        // 10 fps под 60 fps скроллом) дают band-рендер раз в тик + hit-ы
        // между тиками — это всё ещё выигрыш.
        let content_stable = self.last_content_key == Some(key);
        self.last_content_key = Some(key);
        if !content_stable && crate::frame_log_level() >= 2 {
            eprintln!(
                "[frame:wgpu] page-compose unstable-key: gen {} ranges {} dl {}",
                self.content_generation,
                ranges.len(),
                content.len(),
            );
        }

        let fits = self.page_band.as_ref().is_some_and(|b| {
            b.key == key
                && b.w_px == sw
                && b.h_px == band_h_px
                && scroll_y >= b.band_top_css
                && scroll_y + vp_h_css <= b.band_top_css + band_h_css
        });
        if !fits {
            if !content_stable {
                return Ok(false);
            }
            // Промах: перерисовать полосу — один рендер контента. Верх полосы
            // выравнен на целый CSS px, чтобы blit был texel-точным при целых
            // scroll_y (при dpr=1).
            //
            // Направленный сдвиг (срез 2026-07-13): полный запас полосы =
            // `2*margin_css`. Симметрия кладёт вьюпорт по центру → промах после
            // ~margin_css скролла в любую сторону. Скролл почти всегда
            // непрерывен в одну сторону, поэтому кладём бо́льшую долю запаса ПО
            // ходу движения: вьюпорт садится ближе к «хвостовому» краю полосы,
            // а «ведущий» запас (по ходу) ~4× больше → следующий промах дальше.
            // Направление берём из СТАРОЙ полосы (ещё не заменена): вьюпорт вышел
            // за верх (`scroll_y < band_top`) ⇒ скролл вверх, иначе вниз. Первая
            // полоса (полосы ещё нет) — вниз (типичный первый скролл). Это меняет
            // только положение полосы, не её пиксели.
            let band_top_css = if band_bias_disabled() {
                (scroll_y - margin_css).max(0.0).floor()
            } else {
                let reserve_total = 2.0 * margin_css;
                let reserve_trail = (reserve_total * 0.20).floor();
                let reserve_lead = reserve_total - reserve_trail;
                let scrolling_up = self
                    .page_band
                    .as_ref()
                    .is_some_and(|b| scroll_y < b.band_top_css);
                // top-запас = ведущий при скролле вверх, хвостовой при скролле вниз.
                let top_margin = if scrolling_up { reserve_lead } else { reserve_trail };
                (scroll_y - top_margin).max(0.0).floor()
            };
            let recreate = self
                .page_band
                .as_ref()
                .is_none_or(|b| b.w_px != sw || b.h_px != band_h_px);
            if recreate {
                count_texture_created_labeled("page-band", sw, band_h_px);
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("page-band"),
                    size: wgpu::Extent3d {
                        width: sw,
                        height: band_h_px,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.surface_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let (depth_t, depth_v) = create_depth_texture(&self.device, sw, band_h_px);
                self.page_band = Some(PageBandCache {
                    _texture: texture,
                    view,
                    key: 0, // невалиден, пока рендер полосы ниже не пройдёт
                    band_top_css,
                    w_px: sw,
                    h_px: band_h_px,
                    depth_t,
                    depth_v,
                });
            }
            let Some(view) = self.page_band.as_ref().map(|b| b.view.clone()) else {
                return Ok(false);
            };
            // Split: в полосу идёт только статичная часть списка — сегменты
            // выколоты (они рисуются поверх blit-а каждым кадром).
            let static_content: std::borrow::Cow<'_, [DisplayCommand]> = if ranges.is_empty() {
                std::borrow::Cow::Borrowed(content)
            } else {
                let mut v = Vec::with_capacity(content.len());
                let mut prev = 0usize;
                for r in ranges {
                    v.extend_from_slice(&content[prev..r.start]);
                    prev = r.end;
                }
                v.extend_from_slice(&content[prev..]);
                std::borrow::Cow::Owned(v)
            };
            // Depth-attachment обязан совпадать по размеру с целью пасса —
            // на время Band-рендера подменяем оконную depth-текстуру
            // полосной из кэша (и возвращаем обратно, включая случай ошибки).
            let (band_depth_t, band_depth_v) = self
                .page_band
                .as_ref()
                .map(|b| (b.depth_t.clone(), b.depth_v.clone()))
                .unwrap_or_else(|| create_depth_texture(&self.device, sw, band_h_px));
            let saved_depth_t = self.depth_texture.replace(band_depth_t);
            let saved_depth_v = self.depth_view.replace(band_depth_v);
            let band_result = self.render_impl(
                &static_content,
                &[],
                band_top_css,
                0.0,
                RenderPassMode::Band { view, w_px: sw, h_px: band_h_px },
            );
            self.depth_texture = saved_depth_t;
            self.depth_view = saved_depth_v;
            band_result?;
            if let Some(b) = self.page_band.as_mut() {
                b.key = key;
                b.band_top_css = band_top_css;
            }
            if crate::frame_log_level() >= 2 {
                eprintln!(
                    "[frame:wgpu] page-compose MISS: band y={band_top_css:.0}..{:.0} css ({sw}x{band_h_px} px, {} anim segs)",
                    band_top_css + band_h_css,
                    ranges.len(),
                );
            }
        } else if crate::frame_log_level() >= 2 {
            eprintln!("[frame:wgpu] page-compose HIT ({} anim segs)", ranges.len());
        }

        // Композиция: blit полосы со сдвигом + overlay поверх.
        let Some((band_top_css, band_view)) =
            self.page_band.as_ref().map(|b| (b.band_top_css, b.view.clone()))
        else {
            return Ok(false);
        };
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("page-band-bg"),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&band_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        });
        self.pending_base_blit = Some(PendingBaseBlit {
            bind_group,
            dy_css: band_top_css - scroll_y,
            w_css: sw as f32 / dpr,
            h_css: band_h_css,
        });
        // Split: анимируемые сегменты рисуются как content-полоса Compose-кадра
        // (получают штатный сдвиг -scroll_y) — поверх blit-а, под overlay.
        let seg_content: &[DisplayCommand] = seg_plan.as_deref().unwrap_or(&[]);
        self.render_impl(seg_content, overlay, scroll_y, 0.0, RenderPassMode::Compose)?;
        Ok(true)
    }

    /// `scroll_y ≥ 0`, `scroll_x ≥ 0`. Negatives caller обязан клампить до 0.
    pub fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), wgpu::SurfaceError> {
        self.render_with_anim(content, overlay, scroll_y, scroll_x, &[])
    }

    /// Как [`render`](Self::render), но с диапазонами анимируемых сегментов
    /// `content` (static/animated split скролл-композитора, EXPERIMENT.md §2).
    /// Пустые `anim_ranges` — поведение идентично `render`.
    pub fn render_with_anim(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
        anim_ranges: &[std::ops::Range<usize>],
    ) -> Result<(), wgpu::SurfaceError> {
        // Skip-identical-frame (p1-exp-wgpu-only): тотальный хэш кадра —
        // display list + overlay + scroll + размер поверхности (структурный
        // фолд команд, см. hash_display_list) — складывается с поколением
        // контента (register_image / GIF-кадры / снапшоты / шрифты / canvas-bg
        // бампают content_generation). Совпадение с последним успешно
        // отрисованным кадром гарантирует пиксельную идентичность: кадр не
        // рисуется вовсе, на экране остаётся последний present. Только для
        // оконного режима — headless обязан рисовать для readback.
        // LUMEN_NO_FRAME_SKIP=1 отключает пропуск (диагностика).
        // Живёт в оркестраторе, а не в render_impl: скролл-композитор ниже
        // разбивает кадр на band/compose-вызовы, чьи собственные хэши кадр
        // не описывают.
        let (sw0, sh0) = self.surface_dims();
        let base_hash = crate::display_list::hash_display_list(
            content, overlay, scroll_x, scroll_y, sw0, sh0,
        );
        let frame_hash = {
            use std::hash::Hasher;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            h.write_u64(base_hash);
            h.write_u64(self.content_generation);
            h.finish()
        };
        if self.surface.is_some()
            && !frame_skip_disabled()
            && self.last_frame_hash == Some(frame_hash)
        {
            FRAMES_SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if crate::frame_log_level() >= 2 {
                eprintln!("[frame:wgpu] skip (identical frame)");
            }
            return Ok(());
        }

        // Скролл-композитор страницы (EXPERIMENT.md §2): при попадании кадр
        // собирается из персистентной полосы + overlay, минуя перерисовку
        // контента. `false` — путь неприменим, рисуем монолитом как раньше.
        if self.try_page_compose(content, overlay, scroll_y, scroll_x, anim_ranges)? {
            self.last_frame_hash = Some(frame_hash);
            return Ok(());
        }

        self.render_impl(
            content,
            overlay,
            scroll_y,
            scroll_x,
            RenderPassMode::Normal { frame_hash },
        )
    }

    /// Тело рендера одного пасса-цели (см. [`RenderPassMode`]). Общий для
    /// обычного кадра, оффскрин-рендера полосы скролл-композитора и
    /// композиции полоса+overlay; отличия сведены к выбору целевого view,
    /// размеров «поверхности» и финализации (present / счётчики / хэш).
    fn render_impl(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
        mode: RenderPassMode,
    ) -> Result<(), wgpu::SurfaceError> {
        // BUG-274: пофазный тайминг кадра (LUMEN_FRAME_LOG=2) — разбивка
        // wgpu-кадра на faces/collect/prep/acquire/encode/submit, чтобы
        // диагностировать, какая фаза жжёт CPU в простое.
        let phase_log = crate::frame_log_level() >= 2;
        let t_frame0 = std::time::Instant::now();

        // BUG-274: снимки диагностических счётчиков на входе в кадр — печатаем
        // дельту за кадр, а не процессный итог (кумулятивные числа не отвечают
        // на вопрос «сколько текстур родилось именно в этом кадре»).
        let tex_created_at_entry = load_counter(&TEXTURES_CREATED);
        let tex_nanos_at_entry = load_counter(&TEXTURE_CREATE_NANOS);
        let pool_hits_at_entry = load_counter(&TEXTURE_POOL_HITS);
        let pool_misses_at_entry = load_counter(&TEXTURE_POOL_MISSES);

        // Размеры цели: для Band — полоса, иначе — поверхность окна/headless.
        let (sw0, sh0) = match &mode {
            RenderPassMode::Band { w_px, h_px, .. } => (*w_px, *h_px),
            _ => self.surface_dims(),
        };

        // CSS Filter Effects L1 §2 — backdrop-filter result cache.
        // Two consecutive frames hashing identically guarantee every backdrop
        // element's filtered output is identical, so the composite step can
        // reuse the cached texture and skip the expensive blur passes.
        // (Хэш считается только при наличии backdrop-filter — после переезда
        // skip-identical в render() дешёвого готового base_hash здесь нет.)
        let backdrop_frame_hash: Option<u64> = if self.backdrop_cache.is_enabled()
            && crate::display_list::contains_backdrop_filter(content, overlay)
        {
            Some(crate::display_list::hash_display_list(
                content, overlay, scroll_x, scroll_y, sw0, sh0,
            ))
        } else {
            None
        };

        // Параллельная предзагрузка новых face-ов (диск + WOFF-декод +
        // метрики в scoped-потоках) — до пре-резолва, чтобы холодный кадр
        // не грузил шрифты последовательно (~180 мс → max по одному face).
        self.prefetch_faces_parallel(content, overlay);

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
        // lazy_faces займёт &self.faces. Scroll offset не влияет на SIZE
        // (только на position), поэтому используем rect напрямую.
        // Тяжёлая CPU-часть (resize + RGBA + ICC) — параллельно; цикл
        // ensure_image_gpu_key ниже остаётся страховкой (на попадании — no-op).
        self.prefetch_image_resizes_parallel(content, overlay);
        for cmd in content.iter().chain(overlay.iter()) {
            match cmd {
                DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. }
                | DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                    self.ensure_image_gpu_key(src, *rect, *object_fit, *object_position);
                }
                _ => {}
            }
        }

        // Codepoint-cascade и baseline берутся из owned `FaceMetrics`
        // (построены один раз при загрузке face-а). `Font::parse` нужен
        // только на медленных путях — промах глиф-атласа и variation axes —
        // и выполняется лениво через per-frame memo. Тёплый кадр не парсит
        // ни одного face-а (раньше: все face-ы каждый кадр, 1.4–2.7 мс).
        let mut lazy_faces = LazyParsedFaces::new(&self.faces);
        let t_after_faces = t_frame0.elapsed();

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
        enum LoadOpChoice {
            /// Clear colour converted to the target `target_color_space` (default: white).
            Clear(wgpu::Color),
            /// Transparent clear for off-screen opacity layers.
            ClearTransparent,
            /// Load existing contents (accumulate).
            Load,
        }
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
            /// Ограничение закрашиваемой области всех трёх фильтр-пассов
            /// (blur H / blur V / composite): bbox контента уровня, раздутый
            /// на радиус блюра (= min(ceil(3σ),32) текселей — как в шейдере,
            /// и как BLUR_SAMPLE_SCALE=3.0 в WebRender). None = контент
            /// уровня не удалось ограничить → полноэкранные пассы, как раньше.
            /// Корректность чтений за пределами scissor гарантирована полными
            /// LoadOp::Clear этих текстур (clear не подчиняется scissor).
            scissor: Option<DeviceScissor>,
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
            /// bbox-офскрины backdrop-фильтра (EXPERIMENT.md §2): рабочая
            /// область `[x, y, w, h]` в device px родительской текстуры —
            /// element bounds + радиус ядра блюра (формула шейдера), ширина/
            /// высота выровнены вверх до 64 px (стабильность texture_pool).
            /// Ping-pong/кэш-текстуры создаются этого размера, а не размера
            /// родителя; UV bounds-квада запечены относительно региона.
            /// `None` — фолбэк на полноразмерный путь (kill-switch, вырожденные
            /// bounds или регион ≈ весь родитель).
            region: Option<[u32; 4]>,
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

        /// Фактически закрашенная область offscreen-уровня в CSS px
        /// (эксперимент bbox-scissor, EXPERIMENT.md §2). `Empty` — в уровень
        /// ещё ничего не нарисовано; `Rect` — объединение вершин всех draw-ops
        /// уровня плюс области дочерних композитов; `Unbounded` — состав
        /// уровня не удалось ограничить (маска/backdrop) → пассы уровня
        /// остаются полноэкранными. Безопасность по построению: любой
        /// не-учтённый источник пикселей обязан помечать уровень Unbounded.
        #[derive(Clone, Copy)]
        enum LevelBounds {
            Empty,
            Rect { x0: f32, y0: f32, x1: f32, y1: f32 },
            Unbounded,
        }
        impl LevelBounds {
            fn add_point(&mut self, x: f32, y: f32) {
                if !x.is_finite() || !y.is_finite() {
                    *self = LevelBounds::Unbounded;
                    return;
                }
                match self {
                    LevelBounds::Empty => *self = LevelBounds::Rect { x0: x, y0: y, x1: x, y1: y },
                    LevelBounds::Rect { x0, y0, x1, y1 } => {
                        *x0 = x0.min(x);
                        *y0 = y0.min(y);
                        *x1 = x1.max(x);
                        *y1 = y1.max(y);
                    }
                    LevelBounds::Unbounded => {}
                }
            }
            fn add_rect(&mut self, rx0: f32, ry0: f32, rx1: f32, ry1: f32) {
                self.add_point(rx0, ry0);
                self.add_point(rx1, ry1);
            }
        }
        let mut level_bounds: Vec<LevelBounds> = vec![LevelBounds::Unbounded];

        let mut current_level: usize = 0;
        // (alpha, метка render_plan.len() на момент Push) — метка позволяет
        // выбросить из плана ВСЕ пассы слоя (viewport-cull невидимых слоёв):
        // offscreen-текстуры имеют размер окна, контент за его пределами
        // физически не попадает ни в одну текстуру, так что отсечение
        // эквивалентно сегодняшнему клиппингу растеризацией.
        let mut level_alpha_stack: Vec<(f32, usize)> = Vec::new();
        // Tracks blend mode per opened offscreen level (for non-Normal PushBlendMode).
        let mut level_blend_mode_stack: Vec<(BlendMode, usize)> = Vec::new();
        // Tracks filter list per opened offscreen level (for CSS filter compositing).
        let mut filter_stack: Vec<(Vec<FilterFn>, usize)> = Vec::new();
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
        // Размеры цели пасса (для Band — полосы), не поверхности окна.
        let (surface_w, surface_h) = (sw0, sh0);

        let dpr_f32 = self.scale_factor.max(1e-6) as f32;

        // Объединяет позиции вершин диапазона draw-op-а в bbox уровня.
        // Вершины уже в CSS px, после transform/scroll — bbox финальный.
        macro_rules! union_op_verts {
            ($lb:expr, $vec:ident, $start:expr, $count:expr) => {
                for v in &$vec[*$start as usize..(*$start + *$count) as usize] {
                    $lb.add_point(v.pos[0], v.pos[1]);
                }
            };
        }

        macro_rules! flush_batch {
            () => {{
                // bbox-scissor: перед сбросом батча учесть его вершины в
                // границах текущего offscreen-уровня (уровень 0 не считаем).
                if current_level > 0 && batch_start < draw_ops.len() {
                    if let Some(lb) = level_bounds.get_mut(current_level) {
                        for op in &draw_ops[batch_start..] {
                            match op {
                                DrawOp::SetScissor(_) => {}
                                DrawOp::Fill { v_start, v_count } => union_op_verts!(lb, fill_vertices, v_start, v_count),
                                DrawOp::Circle { v_start, v_count } => union_op_verts!(lb, circle_vertices, v_start, v_count),
                                DrawOp::RRect { v_start, v_count } => union_op_verts!(lb, rrect_vertices, v_start, v_count),
                                DrawOp::Text { v_start, v_count } => union_op_verts!(lb, text_vertices, v_start, v_count),
                                DrawOp::Image { v_start, v_count, .. } => union_op_verts!(lb, image_vertices, v_start, v_count),
                                DrawOp::Gradient { v_start, v_count, .. } => union_op_verts!(lb, grad_vertices, v_start, v_count),
                                DrawOp::CrossFade { v_start, v_count, .. } => union_op_verts!(lb, cross_fade_vertices, v_start, v_count),
                            }
                        }
                    }
                }
                let first = level_first.get(current_level).copied().unwrap_or(false);
                let load_op = if first {
                    if current_level == 0 {
                        let rgba = self.canvas_bg
                            .map_or_else(
                                || Self::wgpu_color_for_canvas_bg(&Color::WHITE, self.target_color_space),
                                |bg| Self::wgpu_color_for_canvas_bg(&bg, self.target_color_space),
                            );
                        LoadOpChoice::Clear(wgpu::Color { r: rgba[0] as f64, g: rgba[1] as f64, b: rgba[2] as f64, a: rgba[3] as f64 })
                    } else {
                        LoadOpChoice::ClearTransparent
                    }
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

        // Compose-путь скролл-композитора: полоса страницы рисуется первым
        // op-ом level 0 (после LoadOp::Clear того же пасса) — под overlay,
        // на месте контента, который она заменяет. Обычный image-квад:
        // painter's order и батчинг не нарушаются.
        if let Some(blit) = self.pending_base_blit.take() {
            let v_start = image_vertices.len() as u32;
            push_image_quad(
                &mut image_vertices,
                Rect { x: 0.0, y: blit.dy_css, width: blit.w_css, height: blit.h_css },
                [0.0, 0.0],
                [1.0, 1.0],
                1.0,
            );
            let image_batch_idx = image_bind_groups.len() as u32;
            image_bind_groups.push(blit.bind_group);
            draw_ops.push(DrawOp::Image { v_start, v_count: 6, image_batch_idx });
        }

        let iter_content = content.iter().map(|c| (c, false));
        let iter_overlay = overlay.iter().map(|c| (c, true));
        for (cmd, is_overlay) in iter_content.chain(iter_overlay) {
            let (dy, dx) = if is_overlay {
                (0.0_f32, 0.0_f32)
            } else {
                sticky_stack.last().copied().unwrap_or((-scroll_y, -scroll_x))
            };
            // ADR-016 M0.2 viewport culling: skip self-contained leaf draws
            // whose box — shifted by the scroll/sticky offset and mapped
            // through the current accumulated transform — lands fully outside
            // the viewport (+ slop). `cull_rect` returns `None` for every
            // structural `Push*`/`Pop*`, which must always run to keep the
            // level/clip/transform stacks balanced.
            if let Some(local) = cmd.cull_rect()
                && leaf_is_offscreen(
                    translate_rect(local, dx, dy),
                    transform_stack.last(),
                    viewport_css_w,
                    viewport_css_h,
                )
            {
                continue;
            }
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
                    font_features: _,
                    font_palette: _,
                    tab_size,
                    highlight_name: _,
                    text_orientation,
                } => {
                    let primary_face_id = text_face_iter.next().unwrap_or(0);
                    if lazy_faces
                        .faces
                        .get(primary_face_id)
                        .and_then(|f| f.metrics.as_ref())
                        .is_none()
                    {
                        continue;
                    }
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let v_start = text_vertices.len() as u32;
                    let dest_rect = translate_rect(*rect, dx, dy);
                    // Ph3 writing-mode vertical (wgpu — live default backend,
                    // ADR-017): `Sideways` rotates the whole run 90° CW
                    // (Срез 2), mirroring the CPU rasterizer
                    // (`rasterize_text_rotated`) — glyphs are laid out
                    // horizontally at the local origin, then
                    // `rotate_text_vertices_cw` maps them onto `dest_rect`.
                    // `Mixed` splits per glyph — CJK upright, Latin rotated
                    // (Срез 3, `push_text_glyphs_mixed`, mirrors
                    // `rasterize_text_mixed`). `Upright`/`None` keep the
                    // existing horizontal path.
                    match text_orientation {
                        Some(TextOrientation::Sideways) => {
                            let glyph_rect = Rect::new(0.0, 0.0, dest_rect.height, dest_rect.width);
                            push_text_glyphs(
                                &mut text_vertices,
                                glyph_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                font_variation_axes,
                                *tab_size,
                            );
                            rotate_text_vertices_cw(&mut text_vertices[v_start as usize..], dest_rect);
                        }
                        Some(TextOrientation::Mixed) => {
                            push_text_glyphs_mixed(
                                &mut text_vertices,
                                dest_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                font_variation_axes,
                                *tab_size,
                            );
                        }
                        _ => {
                            push_text_glyphs(
                                &mut text_vertices,
                                dest_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                font_variation_axes,
                                *tab_size,
                            );
                        }
                    }
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
                            && lazy_faces.faces.first().and_then(|f| f.metrics.as_ref()).is_some()
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
                                &mut lazy_faces,
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
                DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                    // A lazy `<img>` stays a LazyImageSlot even after the shell
                    // fetches it (the `loading="lazy"` attribute never clears).
                    // Draw the registered image if present, else the grey
                    // placeholder — same behaviour as DrawImage. (BUG-163)
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let scrolled = translate_rect(*rect, dx, dy);
                    let fit = *object_fit;
                    let pos = *object_position;
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
                                apply_affine_to_verts(&mut image_vertices[v_start as usize..], m);
                            }
                            let v_count = image_vertices.len() as u32 - v_start;
                            let image_batch_idx = image_bind_groups.len() as u32;
                            image_bind_groups.push(gpu.bind_group_linear.clone());
                            draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                        }
                        continue;
                    }
                    // Not yet fetched — grey placeholder.
                    let v_start = fill_vertices.len() as u32;
                    push_fill_quad(
                        &mut fill_vertices,
                        scrolled,
                        apply_alpha_to_color([0.85, 0.85, 0.85, 1.0], 1.0),
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // Clip-stack управление. PushClipRect добавляет пересечение
                // с топом (CSS Masking L1 §3 — clip-rect = intersection всех
                // ancestor clip-region-ов). PopClip снимает топ. Scissor для
                // wgpu выставляется лениво — следующая draw-команда вызовет
                // sync_scissor_to_stack.
                DisplayCommand::PushClipRect { rect } => {
                    let scrolled = translate_rect(*rect, dx, dy);
                    // Apply accumulated transform so clip is in screen space (BUG-276).
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                }
                DisplayCommand::PushClipRoundedRect { rect, radii: _ } => {
                    let scrolled = translate_rect(*rect, dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                }
                // BUG-140: wgpu-fallback клиппит shape-клип bounding box-ом
                // (scissor не умеет произвольные формы; точная форма — в
                // femtovg/cpu_raster путях). Push обязателен для баланса
                // пар с общим PopClip.
                DisplayCommand::PushClipPath { shape } => {
                    let scrolled = translate_rect(shape.bounding_rect(), dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                }
                DisplayCommand::PopClip => {
                    clip_stack.pop();
                }
                DisplayCommand::PushOpacity { alpha, .. } => {
                    flush_batch!();
                    level_alpha_stack.push((*alpha, render_plan.len()));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopOpacity => {
                    if !level_alpha_stack.is_empty() {
                        flush_batch!();
                        let (layer_alpha, plan_mark) = level_alpha_stack.pop().unwrap();
                        // viewport-cull: слой с alpha=0, пустой или целиком вне
                        // поверхности не виден — выбросить из плана и его
                        // контент, и композит.
                        let child_now = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        let invisible = layer_alpha <= 0.0
                            || match child_now {
                                LevelBounds::Empty => true,
                                LevelBounds::Rect { x0, y0, x1, y1 } => {
                                    x1 * dpr_f32 <= 0.0
                                        || y1 * dpr_f32 <= 0.0
                                        || x0 * dpr_f32 >= surface_w as f32
                                        || y0 * dpr_f32 >= surface_h as f32
                                }
                                LevelBounds::Unbounded => false,
                            };
                        if invisible {
                            render_plan.truncate(plan_mark);
                            current_level -= 1;
                            continue;
                        }
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, layer_alpha);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode: BlendMode::Normal,
                        }));
                        let child = level_bounds
                            .get(current_level)
                            .copied()
                            .unwrap_or(LevelBounds::Unbounded);
                        current_level -= 1;
                        // Composite переносит контент дочернего уровня 1:1 —
                        // границы родителя расширяются на bbox ребёнка.
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match child {
                                LevelBounds::Empty => {}
                                LevelBounds::Rect { x0, y0, x1, y1 } => lb.add_rect(x0, y0, x1, y1),
                                LevelBounds::Unbounded => *lb = LevelBounds::Unbounded,
                            }
                        }
                    }
                }
                // CSS Compositing & Blending L1 §5 — mix-blend-mode compositing.
                // Non-Normal mode: push offscreen level + track blend mode.
                // Normal mode: no offscreen layer needed (pass-through).
                DisplayCommand::PushBlendMode { mode, .. } => {
                    blend_mode_stack.push(*mode);
                    if *mode != BlendMode::Normal {
                        flush_batch!();
                        level_blend_mode_stack.push((*mode, render_plan.len()));
                        current_level += 1;
                        while level_first.len() <= current_level {
                            level_first.push(true);
                        }
                        level_first[current_level] = true;
                        while level_bounds.len() <= current_level {
                            level_bounds.push(LevelBounds::Empty);
                        }
                        level_bounds[current_level] = LevelBounds::Empty;
                    }
                }
                DisplayCommand::PopBlendMode => {
                    blend_mode_stack.pop();
                    if let Some((mode, plan_mark)) = level_blend_mode_stack.pop() {
                        flush_batch!();
                        // viewport-cull: для всех CSS-блэндов прозрачный src
                        // оставляет backdrop неизменным (co = cs + cb·(1−as)),
                        // поэтому пустой/за-экранный слой невидим целиком.
                        let child_now = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        let invisible = match child_now {
                            LevelBounds::Empty => true,
                            LevelBounds::Rect { x0, y0, x1, y1 } => {
                                x1 * dpr_f32 <= 0.0
                                    || y1 * dpr_f32 <= 0.0
                                    || x0 * dpr_f32 >= surface_w as f32
                                    || y0 * dpr_f32 >= surface_h as f32
                            }
                            LevelBounds::Unbounded => false,
                        };
                        if invisible {
                            render_plan.truncate(plan_mark);
                            current_level -= 1;
                            continue;
                        }
                        let comp_v_start = composite_vertices.len() as u32;
                        // alpha=1.0: blend shader handles all compositing math.
                        push_composite_quad(&mut composite_vertices, 1.0);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode,
                        }));
                        let child = level_bounds
                            .get(current_level)
                            .copied()
                            .unwrap_or(LevelBounds::Unbounded);
                        current_level -= 1;
                        // Blend-composite тоже красит родителя только в bbox
                        // ребёнка (за его пределами src прозрачен).
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match child {
                                LevelBounds::Empty => {}
                                LevelBounds::Rect { x0, y0, x1, y1 } => lb.add_rect(x0, y0, x1, y1),
                                LevelBounds::Unbounded => *lb = LevelBounds::Unbounded,
                            }
                        }
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
                            // CSS Backgrounds L3 §3.5: percent axes resolve against the
                            // positioning area; `auto` derives from the intrinsic ratio.
                            match (w.resolve(oarea.width), h.resolve(oarea.height)) {
                                (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                                (Some(tw), None) => {
                                    let tw = tw.max(1.0);
                                    (tw, (img_h * (tw / img_w)).max(1.0))
                                }
                                (None, Some(th)) => {
                                    let th = th.max(1.0);
                                    ((img_w * (th / img_h)).max(1.0), th)
                                }
                                (None, None) => (img_w, img_h),
                            }
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

                    let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match repeat {
                        BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
                        BackgroundRepeat::RepeatX => (
                            tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                            tile_y0, tile_h, false,
                        ),
                        BackgroundRepeat::RepeatY => (
                            tile_x0, tile_w, false,
                            tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                        ),
                        BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
                            tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                            tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                        ),
                        BackgroundRepeat::Space => {
                            let (sx, step_x, rx) = space_axis_geometry(oarea.x, oarea.width, tile_w, off_x);
                            let (sy, step_y, ry) = space_axis_geometry(oarea.y, oarea.height, tile_h, off_y);
                            (sx, step_x, rx, sy, step_y, ry)
                        }
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
                            tx += step_x;
                        }
                        if !repeat_y { break; }
                        ty += step_y;
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
                    let (p0, p1, line_len) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, line_len);
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
                DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = radial_gradient_uv_params(*center_x_pct, *center_y_pct);
                    // Px/Calc stops resolve against the larger ending-shape radius,
                    // matching `cpu_raster::rasterize_radial_gradient` (BUG-277).
                    let line_len = radius_x.max(*radius_y).max(1.0);
                    let resolved = resolve_gradient_stops(stops, line_len);
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                // CSS Masking L1 §4 — gradient masks: build GradParamsCpu at plan time;
                // render-time pass renders gradient → surface-size temp texture → use as mask.
                DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1, line_len) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, line_len);
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PushMaskRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = radial_gradient_uv_params(*center_x_pct, *center_y_pct);
                    // Mask radial gradients stay circular (farthest-corner) — the mask
                    // command carries no ending-shape, matching `cpu_raster::render_mask`.
                    let mask_dx = center_x_pct.max(1.0 - center_x_pct) * scrolled.width;
                    let mask_dy = center_y_pct.max(1.0 - center_y_pct) * scrolled.height;
                    let line_len = mask_dx.hypot(mask_dy).max(1.0);
                    let resolved = resolve_gradient_stops(stops, line_len);
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
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
                                        // CSS Masking L1 §4: percent axes resolve against the
                                        // mask painting area; `auto` keeps the intrinsic ratio.
                                        match (w.resolve(area.width), h.resolve(area.height)) {
                                            (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                                            (Some(tw), None) => {
                                                let tw = tw.max(1.0);
                                                (tw, (img_h * (tw / img_w)).max(1.0))
                                            }
                                            (None, Some(th)) => {
                                                let th = th.max(1.0);
                                                ((img_w * (th / img_h)).max(1.0), th)
                                            }
                                            (None, None) => (img_w, img_h),
                                        }
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
                                let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match info.repeat {
                                    BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
                                    BackgroundRepeat::RepeatX => (
                                        tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                                        tile_y0, tile_h, false,
                                    ),
                                    BackgroundRepeat::RepeatY => (
                                        tile_x0, tile_w, false,
                                        tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                                    ),
                                    BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
                                        tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                                        tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                                    ),
                                    BackgroundRepeat::Space => {
                                        let (sx, step_x, rx) = space_axis_geometry(area.x, area.width, tile_w, off_x);
                                        let (sy, step_y, ry) = space_axis_geometry(area.y, area.height, tile_h, off_y);
                                        (sx, step_x, rx, sy, step_y, ry)
                                    }
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
                                        tx += step_x;
                                    }
                                    if !repeat_y { break; }
                                    ty += step_y;
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
                    // bbox-scissor v1: mask-композит не отслеживаем — родитель
                    // помечается неограниченным (безопасный фолбэк).
                    if current_level > 0
                        && let Some(lb) = level_bounds.get_mut(current_level)
                    {
                        *lb = LevelBounds::Unbounded;
                    }
                }
                // CSS Filter Effects L1 — PushFilter opens an offscreen level;
                // PopFilter composites it onto the parent with filter applied.
                DisplayCommand::PushFilter { filters, bounds: _ } => {
                    flush_batch!();
                    filter_stack.push((filters.clone(), render_plan.len()));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopFilter => {
                    if let Some((filters, plan_mark)) = filter_stack.pop() {
                        flush_batch!();
                        let content = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        // Радиус блюра в текселях — та же формула, что в
                        // BLUR_SHADER_SRC: min(ceil(3σ),32); шейпер шагает по
                        // 1 текселю surface-текстуры. + 2 px запас на bilinear.
                        let blur_pad = filters
                            .iter()
                            .find_map(|f| match f {
                                FilterFn::Blur(s) if *s > 0.0 => {
                                    Some((3.0 * *s).ceil().min(32.0))
                                }
                                _ => None,
                            })
                            .unwrap_or(0.0)
                            + 2.0;
                        // (scissor пассов, раздутый bbox в CSS px для родителя)
                        let (scissor, parent_rect) = match content {
                            LevelBounds::Unbounded => (None, None),
                            LevelBounds::Empty => {
                                // Слой пуст: composite прозрачной текстуры —
                                // визуальный no-op; выбросить из плана и
                                // отрисовку контента слоя (viewport-cull).
                                render_plan.truncate(plan_mark);
                                current_level -= 1;
                                continue;
                            }
                            LevelBounds::Rect { x0, y0, x1, y1 } => {
                                let pad_css = blur_pad / dpr_f32;
                                let (ix0, iy0) = (x0 - pad_css, y0 - pad_css);
                                let (ix1, iy1) = (x1 + pad_css, y1 + pad_css);
                                let sx0 = ((ix0 * dpr_f32).floor().max(0.0) as u32).min(surface_w);
                                let sy0 = ((iy0 * dpr_f32).floor().max(0.0) as u32).min(surface_h);
                                let sx1 = ((ix1 * dpr_f32).ceil().max(0.0) as u32).min(surface_w);
                                let sy1 = ((iy1 * dpr_f32).ceil().max(0.0) as u32).min(surface_h);
                                if sx1 <= sx0 || sy1 <= sy0 {
                                    // Контент целиком за пределами surface —
                                    // фильтр не виден, его контент-пассы тоже
                                    // выбрасываются (viewport-cull).
                                    render_plan.truncate(plan_mark);
                                    current_level -= 1;
                                    continue;
                                }
                                let full = sx0 == 0
                                    && sy0 == 0
                                    && sx1 >= surface_w
                                    && sy1 >= surface_h;
                                (
                                    (!full).then_some(DeviceScissor {
                                        x: sx0,
                                        y: sy0,
                                        width: sx1 - sx0,
                                        height: sy1 - sy0,
                                    }),
                                    Some((ix0, iy0, ix1, iy1)),
                                )
                            }
                        };
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        render_plan.push(RenderPlanItem::FilterComposite(FilterCompositePlan {
                            from_level: current_level,
                            filters,
                            comp_v_start,
                            scissor,
                        }));
                        current_level -= 1;
                        // Композит фильтра красит родителя в пределах
                        // раздутого bbox — учесть в границах родителя.
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match parent_rect {
                                Some((rx0, ry0, rx1, ry1)) => lb.add_rect(rx0, ry0, rx1, ry1),
                                None => *lb = LevelBounds::Unbounded,
                            }
                        }
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopBackdropFilter => {
                    if let Some((filters, bounds)) = backdrop_filter_stack.pop() {
                        flush_batch!();
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        // bbox-офскрины backdrop: рабочая область = bounds +
                        // радиус ядра блюра (формула BLUR_SHADER_SRC:
                        // min(ceil(3σ),32) + 2 запас), клип по родителю,
                        // ширина/высота выровнены вверх до 64 px — чтобы
                        // texture_pool стабильно попадал при движении bounds.
                        // Пиксельная эквивалентность: blit читает только
                        // bounds, а все выборки блюра для пикселей bounds
                        // лежат внутри региона (та же математика, что у
                        // bbox-scissor п.16).
                        let region: Option<[u32; 4]> = if bbox_backdrop_disabled() {
                            None
                        } else {
                            let blur_pad = filters
                                .iter()
                                .find_map(|f| match f {
                                    FilterFn::Blur(s) if *s > 0.0 => {
                                        Some((3.0 * *s).ceil().min(32.0))
                                    }
                                    _ => None,
                                })
                                .unwrap_or(0.0)
                                + 2.0;
                            let pad_css = blur_pad / dpr_f32;
                            let rx0 = (((bounds.x - pad_css) * dpr_f32).floor().max(0.0) as u32)
                                .min(surface_w);
                            let ry0 = (((bounds.y - pad_css) * dpr_f32).floor().max(0.0) as u32)
                                .min(surface_h);
                            let rx1 = (((bounds.x + bounds.width + pad_css) * dpr_f32)
                                .ceil()
                                .max(0.0) as u32)
                                .min(surface_w);
                            let ry1 = (((bounds.y + bounds.height + pad_css) * dpr_f32)
                                .ceil()
                                .max(0.0) as u32)
                                .min(surface_h);
                            if rx1 <= rx0 || ry1 <= ry0 {
                                // Элемент целиком вне surface — blit невидим,
                                // но пассы должны отработать как раньше
                                // (полноразмерный фолбэк, нулевой регион
                                // ломал бы копию/кэш).
                                None
                            } else {
                                let rw = (rx1 - rx0).div_ceil(64) * 64;
                                let rh = (ry1 - ry0).div_ceil(64) * 64;
                                // Регион ≈ весь родитель — выигрыша нет,
                                // остаёмся на старом пути (и его кэш-хэшах).
                                if rw >= surface_w && rh >= surface_h {
                                    None
                                } else {
                                    Some([rx0, ry0, rw, rh])
                                }
                            }
                        };
                        let bounds_v_start = composite_vertices.len() as u32;
                        push_bounded_quad(
                            &mut composite_vertices,
                            bounds,
                            surface_w as f32,
                            surface_h as f32,
                            dpr_f32,
                            1.0,
                            region,
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
                                region,
                            },
                        ));
                        current_level -= 1;
                        // bbox-scissor v1: backdrop-композит не отслеживаем —
                        // родитель помечается неограниченным (безопасный фолбэк).
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            *lb = LevelBounds::Unbounded;
                        }
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
                // BUG-247 / BUG-173: the GPU pipeline has no native path fill, so
                // tessellate the nonzero outline contours into a triangle soup —
                // identical to the old `DrawSvgPath` fill the emitter produced.
                DisplayCommand::DrawSvgFill { contours, color } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    let tris = crate::svg_path::tessellate_fill(contours);
                    for [x, y] in &tris {
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
                // BUG-247: the GPU pipeline has no native stroker, so tessellate
                // the stroke contours into a triangle soup — identical to the old
                // `DrawSvgPath` stroke the emitter produced.
                DisplayCommand::DrawSvgStroke { contours, color, params } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    let tris = crate::svg_path::tessellate_stroke_ex(contours, params);
                    for [x, y] in &tris {
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
                // CSS Positioning L3 §6.1 — position:fixed partition markers
                // (ADR-016 M3.2.1c). No draw-time offset: fixed content is already
                // at viewport-fixed coords, so these are pure no-ops here.
                DisplayCommand::BeginFixedLayer | DisplayCommand::EndFixedLayer => {}
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
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
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
                    // bbox-scissor v1: mask-layer-композит не отслеживаем —
                    // родитель помечается неограниченным (безопасный фолбэк).
                    if current_level > 0
                        && let Some(lb) = level_bounds.get_mut(current_level)
                    {
                        *lb = LevelBounds::Unbounded;
                    }
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
        let t_after_collect = t_frame0.elapsed();

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
        let (dims_w, dims_h) = (surface_w, surface_h);
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

        // CSS Masking L1 §4 — gradient mask temp textures ИЗ ПУЛА.
        // Раньше на каждый кадр на каждый MaskComposite с градиентом
        // создавалась свежая текстура размером с target и дропалась после
        // submit — 196 из 237 созданий за флинг-прогон (перепись п.23/24,
        // 1024×1800 ≈ 7.4 МБ каждая). Пред-захват до цикла (внутри цикла
        // живут заимствования &self), возврат в пул после submit.
        let grad_mask_count = render_plan
            .iter()
            .filter(|item| {
                matches!(item, RenderPlanItem::MaskComposite(c)
                    if c.mask_gradient.is_some()
                        && c.mask_src.as_ref().is_none_or(|src| !self.images.contains_key(src)))
            })
            .count();
        let mut temp_grad_layers: Vec<OffscreenLayer> = Vec::with_capacity(grad_mask_count);
        for _ in 0..grad_mask_count {
            let layer = self.create_layer_texture(surface_w, surface_h);
            temp_grad_layers.push(layer);
        }
        let mut temp_grad_next = 0usize;

        // ── Frame ─────────────────────────────────────────────────────────
        // Windowed: get the next swapchain image from the surface.
        // Headless: create a temporary RGBA8 RENDER_ATTACHMENT|COPY_SRC texture so
        //   render_to_image() can read it back after this call.
        let t_after_prep = t_frame0.elapsed();
        let windowed_frame: Option<wgpu::SurfaceTexture>;
        let headless_tex: Option<wgpu::Texture>;
        let frame_view: wgpu::TextureView;
        if let RenderPassMode::Band { view, .. } = &mode {
            // Оффскрин-рендер полосы: цель задана вызывающим, swapchain не
            // трогаем (клон view — дешёвый Arc-хэндл wgpu).
            frame_view = view.clone();
            windowed_frame = None;
            headless_tex = None;
        } else if let Some(ref surface) = self.surface {
            let f = surface.get_current_texture()?;
            frame_view = f.texture.create_view(&wgpu::TextureViewDescriptor::default());
            windowed_frame = Some(f);
            headless_tex = None;
        } else {
            count_texture_created_labeled("headless-frame", surface_w, surface_h);
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
        let t_after_acquire = t_frame0.elapsed();
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
        // BUG-277 slice 2: same hazard as `filter_param_bufs` above, for blend-mode
        // composites. `self.blend_mode_uniform` used to be written via `queue.write_buffer`
        // once per `PushBlendMode`/`PopBlendMode` pair; with 2+ such pairs in one frame
        // (e.g. several `background-blend-mode` boxes, or one box with 2+ blended
        // background layers) every blend render pass ended up reading whichever mode was
        // written LAST, since all writes land before the single shared encoder submits.
        let mut blend_mode_param_bufs: Vec<wgpu::Buffer> = Vec::new();

        // BUG-274: поэлементный CPU-учёт encode-фазы (LUMEN_FRAME_LOG=2) —
        // суммарное время и число элементов по каждому типу RenderPlanItem.
        let mut t_plan: [std::time::Duration; 6] = Default::default();
        let mut n_plan: [u32; 6] = [0; 6];
        // BUG-274: разбивка Draw-пасса — begin_render_pass / запись ops / drop(pass).
        let mut t_draw_sub: [std::time::Duration; 3] = Default::default();

        // BUG-274 (LUMEN_FRAME_LOG=3): пер-элементный профиль encode.
        // Средние по типу пасса скрывают форму распределения: «161 пасс по
        // 0.62 мс» и «146 пассов по 0.02 мс + 15 по 6.5 мс» дают одну и ту же
        // сумму, но требуют противоположных решений (схлопывать пассы против
        // переиспользовать текстуры). Пишем каждый элемент, печатаем топ.
        let item_log = crate::frame_log_level() >= 3;
        // (plan_kind, target_level, длительность, drop(pass) для Draw)
        let mut items_prof: Vec<(usize, usize, std::time::Duration, std::time::Duration)> =
            if item_log { Vec::with_capacity(render_plan.len()) } else { Vec::new() };

        for item in &render_plan {
            let t_item0 = std::time::Instant::now();
            // usize::MAX = «уровень неприменим к этому типу элемента».
            let item_level = match item {
                RenderPlanItem::Draw(batch) => batch.target_level,
                _ => usize::MAX,
            };
            let mut item_pass_end = std::time::Duration::ZERO;
            let plan_kind = match item {
                RenderPlanItem::Draw(_) => 0,
                RenderPlanItem::Composite(_) => 1,
                RenderPlanItem::MaskComposite(_) => 2,
                RenderPlanItem::FilterComposite(_) => 3,
                RenderPlanItem::BackdropFilterComposite(_) => 4,
                RenderPlanItem::MaskLayerComposite(_) => 5,
            };
            match item {
                RenderPlanItem::Draw(batch) => {
                    let target_view = if batch.target_level == 0 {
                        &frame_view
                    } else {
                        &self.layer_textures[batch.target_level - 1].view
                    };
                    let load = match batch.load_op {
                        LoadOpChoice::Clear(c) => wgpu::LoadOp::Clear(c),
                        LoadOpChoice::ClearTransparent => {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        }
                        LoadOpChoice::Load => wgpu::LoadOp::Load,
                    };
                    // All render passes must supply a depth attachment because the
                    // fill/rrect/circle pipelines use depth_write_enabled:true.
                    // wgpu validation requires: pipeline has depth → pass has depth attachment.
                    // Off-screen opacity layers don't need depth sorting, so they always
                    // clear to 1.0 (far plane) — correct result; they are composited by alpha.
                     let depth_attachment = self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                         view: dv,
                         depth_ops: Some(wgpu::Operations {
                             // Level 0: clear to 1.0 (far) at the start of each draw
                             //          batch so depth tests within the pass are
                             //          accumulated across same-frame batches.
                             // Level > 0: clear to 1.0 so depth sorting within the
                             //            offscreen layer is independent of the parent frame.
                             load: if batch.target_level > 0 {
                                 wgpu::LoadOp::Clear(1.0)
                             } else if matches!(batch.load_op, LoadOpChoice::Load) {
                                 wgpu::LoadOp::Load
                             } else {
                                 wgpu::LoadOp::Clear(1.0)
                             },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    });
                    let t_d0 = std::time::Instant::now();
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
                    let t_d1 = t_d0.elapsed();
                    run_draw_ops!(pass, batch.ops_start, batch.ops_end);
                    let t_d2 = t_d0.elapsed();
                    drop(pass);
                    let t_d3 = t_d0.elapsed();
                    t_draw_sub[0] += t_d1;
                    t_draw_sub[1] += t_d2 - t_d1;
                    t_draw_sub[2] += t_d3 - t_d2;
                    item_pass_end = t_d3 - t_d2;
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
                            // Per-composite blend mode uniform (u32 mode + 3× u32 padding =
                            // 16 bytes) — a fresh buffer per composite, not a `write_buffer`
                            // into the shared `self.blend_mode_uniform` (see
                            // `blend_mode_param_bufs` above for why).
                            let mode_u32 = blend_mode_to_u32(comp.mode);
                            let uniform_data: [u32; 4] = [mode_u32, 0, 0, 0];
                            let mode_buf = make_blend_mode_param_buf(&self.device, &uniform_data);
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
                                        resource: mode_buf.as_entire_binding(),
                                    },
                                ],
                            });
                            blend_mode_param_bufs.push(mode_buf);
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
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
                        // Пул-текстура пред-захвачена до цикла (temp_grad_layers);
                        // LoadOp::Clear ниже гарантирует чистый старт при reuse.
                        let Some(grad_layer) = temp_grad_layers.get(temp_grad_next) else {
                            continue;
                        };
                        temp_grad_next += 1;
                        let temp_view = &grad_layer.view;
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
                                    view: temp_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.gradient_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_bind_group(1, &grad_bg, &[]);
                            pass.set_vertex_buffer(0, grad_vbuf_m.slice(..));
                            pass.draw(0..6, 0..1);
                        }
                        Some(temp_view)
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
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.blur_pipeline);
                            if let Some(s) = plan.scissor {
                                pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
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
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(&self.blur_pipeline);
                            if let Some(s) = plan.scissor {
                                pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
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
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.filter_pipeline);
                        if let Some(s) = plan.scissor {
                            pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                        }
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
                // When from_level < 2 (parent = surface texture, lacks TEXTURE_BINDING),
                // steps 1-3 are skipped (can't copy from surface), but step 4 still runs so
                // the element content is visible (no silent drop).
                RenderPlanItem::BackdropFilterComposite(plan) => {
                    let Some(cvb) = &comp_vbuf else { continue };
                    // from_level < 2 means parent is the surface — backdrop blur/blit impossible.
                    let skip_backdrop = plan.from_level < 2;

                    // Ordinals evicted by `store()` whose textures must be freed once the
                    // current element's passes (which borrow the cache map) have ended.
                    let mut evicted_ordinals: Vec<u32> = Vec::new();

                    if !skip_backdrop {
                    let parent_idx = plan.from_level - 2;
                    let parent_w = self.layer_textures[parent_idx].width;
                    let parent_h = self.layer_textures[parent_idx].height;
                    // bbox-офскрины backdrop-фильтра (EXPERIMENT.md §2):
                    // ping-pong и кэш живут в размере региона (bounds + ядро
                    // блюра), а не родителя. region=None — прежний
                    // полноразмерный путь (kill-switch/фолбэк).
                    let (rx, ry, rw, rh) = match plan.region {
                        Some([x, y, w, h]) => (x, y, w, h),
                        None => (0, 0, parent_w, parent_h),
                    };
                    let use_region = plan.region.is_some();
                    // Копия из родителя не может выйти за его края: текстуры
                    // выровнены до 64 px и бывают шире остатка родителя.
                    // Копия всегда покрывает невыровненный (логический)
                    // регион — все выборки блюра для читаемых blit-ом
                    // пикселей лежат в скопированной области.
                    let copy_w = rw.min(parent_w.saturating_sub(rx)).max(1);
                    let copy_h = rh.min(parent_h.saturating_sub(ry)).max(1);
                    let mut pooled_ping: Option<OffscreenLayer> = None;
                    let mut pooled_pong: Option<OffscreenLayer> = None;
                    if use_region {
                        pooled_ping = Some(self.create_layer_texture(rw, rh));
                        pooled_pong = Some(self.create_layer_texture(rw, rh));
                    } else {
                        self.ensure_scratch_layer(parent_w, parent_h);
                        self.ensure_backdrop_layer(parent_w, parent_h);
                    }
                    // Depth-attachment обязан совпадать по размеру с
                    // color-attachment (валидация wgpu).
                    let bd_depth_view: Option<wgpu::TextureView> = if use_region {
                        Some(self.small_depth_view(rw, rh))
                    } else {
                        self.depth_view.clone()
                    };
                    // The per-ordinal cache texture is the blit source (always), and on a
                    // cache hit it already holds the previous frame's filtered backdrop.
                    if self.ensure_backdrop_cache_texture(plan.ordinal, rw, rh) {
                        // A resize discarded the cached pixels — drop the stale hash so it
                        // cannot produce a hit against the fresh (uninitialised) texture.
                        self.backdrop_cache.invalidate(plan.ordinal);
                    }
                    // Ping = вход блюра (копия родителя), pong = выход H-пасса.
                    let ping_tex: wgpu::Texture;
                    let ping_view: wgpu::TextureView;
                    let pong_view: wgpu::TextureView;
                    if let (Some(a), Some(b)) = (pooled_ping.as_ref(), pooled_pong.as_ref()) {
                        ping_tex = a.texture.clone();
                        ping_view = a.view.clone();
                        pong_view = b.view.clone();
                    } else {
                        let s = self.scratch_layer.as_ref().unwrap();
                        ping_tex = s.texture.clone();
                        ping_view = s.view.clone();
                        pong_view = self.backdrop_layer.as_ref().unwrap().view.clone();
                    }
                    if use_region && crate::frame_log_level() >= 2 {
                        eprintln!(
                            "[frame:wgpu]   bdrop region {rw}x{rh} @({rx},{ry}) of {parent_w}x{parent_h}"
                        );
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

                    if !cache_hit {
                        if let Some(sigma) = blur_sigma {
                            // Step 1: copy parent-region → ping (blur H-pass input).
                            // parent has COPY_SRC, ping (pooled/scratch) has COPY_DST.
                            let mut parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            parent_copy.origin = wgpu::Origin3d { x: rx, y: ry, z: 0 };
                            let ping_copy = ping_tex.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                ping_copy,
                                wgpu::Extent3d { width: copy_w, height: copy_h, depth_or_array_layers: 1 },
                            );

                            // Step 2 H pass: ping → pong (REPLACE).
                            let blur_h = BlurParamsCpu { sigma, direction: 0, _p0: 0, _p1: 0 };
                            self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_h]));
                            let blur_bg_h = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-h-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&ping_view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: self.blur_uniform.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("backdrop-blur-h-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &pong_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: bd_depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(&self.blur_pipeline);
                                pass.set_bind_group(0, &blur_bg_h, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                            // Step 2 V pass: pong → CACHE texture (REPLACE).
                            // The blurred result lands in the cache, ready for reuse next frame.
                            let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                            self.queue.write_buffer(&self.blur_uniform, 0, as_bytes(&[blur_v]));
                            let cache_view_v = &self.backdrop_cache_textures[&plan.ordinal].view;
                            let blur_bg_v = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-v-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&pong_view) },
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
                                    depth_stencil_attachment: bd_depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(&self.blur_pipeline);
                                pass.set_bind_group(0, &blur_bg_v, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                        } else {
                            // Filter-only backdrop (no blur): copy parent-region → cache directly.
                            // parent has COPY_SRC, cache has COPY_DST.
                            let mut parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            parent_copy.origin = wgpu::Origin3d { x: rx, y: ry, z: 0 };
                            let cache_copy = self.backdrop_cache_textures[&plan.ordinal].texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                cache_copy,
                                wgpu::Extent3d { width: copy_w, height: copy_h, depth_or_array_layers: 1 },
                            );
                        }

                        // Record the freshly produced backdrop in the cache (skipped when
                        // caching is disabled — `backdrop_frame_hash == None`).
                        if let Some(fh) = backdrop_frame_hash {
                            let bytes = rw as usize * rh as usize * 4;
                            evicted_ordinals = self.backdrop_cache.store(plan.ordinal, fh, bytes);
                        }
                    }

                    // Step 3: blit cache texture → parent at element bounds.
                    // Uses backdrop_blit_pipeline (REPLACE RGB, preserve dst alpha) to
                    // write the filtered backdrop into the parent layer at element bounds.
                    // Applies color filters (count > 0) or passthrough (count = 0).
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
                    // Source is the cache texture — holds the blurred (or copied) backdrop.
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
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.backdrop_blit_pipeline);
                        pass.set_bind_group(0, &bd_blit_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.bounds_v_start..plan.bounds_v_start + 6, 0..1);
                    }

                    // bbox-офскрины: ping-pong вернуть в пул (кэш-текстура
                    // остаётся жить у ordinal-а — её читают blit и след. кадры).
                    // Переиспользование в этом же кадре безопасно: команды
                    // исполняются в порядке encoder-а.
                    if let Some(l) = pooled_ping.take() {
                        self.release_layer_to_pool(l);
                    }
                    if let Some(l) = pooled_pong.take() {
                        self.release_layer_to_pool(l);
                    }
                    } // end if !skip_backdrop

                    // Step 4: composite element layer → parent (ALPHA_BLENDING).
                    // Runs even when skip_backdrop (from_level < 2) so element content
                    // is always visible; only the filtered backdrop blit is skipped.
                    let parent_dst_view4 = if plan.from_level >= 2 {
                        &self.layer_textures[plan.from_level - 2].view as *const _
                    } else {
                        &frame_view as *const _
                    };
                    // SAFETY: we hold &mut self for the encoder lifetime and frame_view
                    // is valid for the duration of this frame. layer_textures is not
                    // mutated after this point within the current plan item.
                    let parent_dst_view4: &wgpu::TextureView = unsafe { &*parent_dst_view4 };
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
                                view: parent_dst_view4,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
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
            let t_item = t_item0.elapsed();
            t_plan[plan_kind] += t_item;
            n_plan[plan_kind] += 1;
            if item_log {
                items_prof.push((plan_kind, item_level, t_item, item_pass_end));
            }
        }

        let t_after_encode = t_frame0.elapsed();
        self.queue.submit([encoder.finish()]);
        // Градиент-маски: временные текстуры обратно в пул (команды уже
        // сабмичены; wgpu удерживает ресурсы до исполнения сам).
        for layer in temp_grad_layers.drain(..) {
            self.release_layer_to_pool(layer);
        }
        if let Some(frame) = windowed_frame {
            frame.present();
        }
        if phase_log {
            let t_total = t_frame0.elapsed();
            eprintln!(
                "[frame:wgpu] total {:7.2}ms | faces {:6.2} collect {:6.2} prep {:6.2} \
                 acquire {:6.2} encode {:6.2} submit {:6.2} | ops {} layers {}",
                t_total.as_secs_f64() * 1e3,
                t_after_faces.as_secs_f64() * 1e3,
                (t_after_collect - t_after_faces).as_secs_f64() * 1e3,
                (t_after_prep - t_after_collect).as_secs_f64() * 1e3,
                (t_after_acquire - t_after_prep).as_secs_f64() * 1e3,
                (t_after_encode - t_after_acquire).as_secs_f64() * 1e3,
                (t_total - t_after_encode).as_secs_f64() * 1e3,
                draw_ops.len(),
                max_level,
            );
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
            eprintln!(
                "[frame:wgpu]   plan: draw {}x{:.1}ms comp {}x{:.1}ms mask {}x{:.1}ms \
                 filt {}x{:.1}ms bdrop {}x{:.1}ms mlayer {}x{:.1}ms",
                n_plan[0], ms(t_plan[0]), n_plan[1], ms(t_plan[1]), n_plan[2], ms(t_plan[2]),
                n_plan[3], ms(t_plan[3]), n_plan[4], ms(t_plan[4]), n_plan[5], ms(t_plan[5]),
            );
            eprintln!(
                "[frame:wgpu]   draw-sub: begin {:.1}ms ops {:.1}ms end {:.1}ms |                  textures_created {} pool {}",
                ms(t_draw_sub[0]), ms(t_draw_sub[1]), ms(t_draw_sub[2]),
                TEXTURES_CREATED.load(std::sync::atomic::Ordering::Relaxed),
                self.texture_pool.len(),
            );

            // LUMEN_FRAME_LOG=3 — распределение, а не среднее.
            if item_log {
                let d_created = load_counter(&TEXTURES_CREATED) - tex_created_at_entry;
                let d_nanos = load_counter(&TEXTURE_CREATE_NANOS) - tex_nanos_at_entry;
                let d_hits = load_counter(&TEXTURE_POOL_HITS) - pool_hits_at_entry;
                let d_misses = load_counter(&TEXTURE_POOL_MISSES) - pool_misses_at_entry;
                eprintln!(
                    "[frame:wgpu]   alloc: this frame created {d_created} tex in {:.2}ms | \
                     pool hit {d_hits} miss {d_misses}",
                    d_nanos as f64 / 1e6,
                );
                // Перепись «кто создаёт текстуры» (суммарно за процесс,
                // вопрос п.23): топ-8 по количеству, с размерами.
                if let Some(census) = TEXTURE_CENSUS.get()
                    && let Ok(m) = census.lock()
                {
                    let mut rows: Vec<_> = m.iter().map(|(k, n)| (*k, *n)).collect();
                    rows.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
                    let s = rows
                        .iter()
                        .take(8)
                        .map(|((l, w, h), n)| format!("{l} {w}x{h} x{n}"))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("[frame:wgpu]   alloc-census (total): {s}");
                }

                const KIND: [&str; 6] =
                    ["draw", "comp", "mask", "filt", "bdrop", "mlayer"];

                // Гистограмма по длительности: «дорог каждый пасс» против
                // «дороги единицы пассов» различаются здесь и только здесь.
                let mut buckets = [0u32; 5]; // <0.05, <0.2, <1, <5, >=5 ms
                for (_, _, dur, _) in &items_prof {
                    let m = ms(*dur);
                    let b = if m < 0.05 {
                        0
                    } else if m < 0.2 {
                        1
                    } else if m < 1.0 {
                        2
                    } else if m < 5.0 {
                        3
                    } else {
                        4
                    };
                    buckets[b] += 1;
                }
                eprintln!(
                    "[frame:wgpu]   items {} | hist <0.05ms {} <0.2ms {} <1ms {} <5ms {} >=5ms {}",
                    items_prof.len(),
                    buckets[0], buckets[1], buckets[2], buckets[3], buckets[4],
                );

                let mut top = items_prof.clone();
                top.sort_unstable_by_key(|i| std::cmp::Reverse(i.2));
                let shown = top.len().min(12);
                let top_sum: f64 = top[..shown].iter().map(|i| ms(i.2)).sum();
                let all_sum: f64 = top.iter().map(|i| ms(i.2)).sum();
                eprintln!(
                    "[frame:wgpu]   top {shown} items = {top_sum:.1}ms of {all_sum:.1}ms encode"
                );
                for (kind, level, dur, pass_end) in &top[..shown] {
                    let lvl = if *level == usize::MAX {
                        "-".to_string()
                    } else {
                        level.to_string()
                    };
                    eprintln!(
                        "[frame:wgpu]     {:<6} lvl {:<3} {:7.2}ms  (drop(pass) {:6.2}ms)",
                        KIND[*kind], lvl, ms(*dur), ms(*pass_end),
                    );
                }
            }
        }
        // Финализация по режиму: Band — служебный оффскрин-проход, не кадр
        // (не считаем и хэш не трогаем); Compose — настоящий кадр, но его
        // хэш фиксирует вызывающий render() (хэш Compose-аргументов кадр не
        // описывает); Normal — кадр и хэш, как раньше.
        match mode {
            RenderPassMode::Band { .. } => {}
            RenderPassMode::Compose => {
                FRAMES_RENDERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            RenderPassMode::Normal { frame_hash } => {
                FRAMES_RENDERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.last_frame_hash = Some(frame_hash);
            }
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
        images: &[(String, std::sync::Arc<lumen_image::Image>)],
        scroll_x: f32,
        scroll_y: f32,
    ) -> Result<lumen_image::Image, Box<dyn std::error::Error>> {
        crate::cpu_raster::rasterize_cpu(width, height, commands, images, scroll_x, scroll_y)
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

        crate::cpu_raster::rasterize_cpu(tile_size, tile_size, &all, &[], offset_x, offset_y)
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
        target_color_space: ColorSpace,
    ) -> Result<Vec<lumen_image::Image>, Box<dyn std::error::Error>> {
        if pages.is_empty() {
            return Ok(vec![]);
        }
        let mut renderer = Renderer::new_headless(font_bytes, page_w, page_h, target_color_space)?;
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

/// Применяет аккумулированный 2D-аффинный трансформ к clip-rect-у и
/// возвращает AABB трансформированных углов в screen-координатах.
///
/// Нужно для `PushClipRect*`: рект из display-list-а — в page-пространстве,
/// а clip_stack должен хранить координаты в screen-пространстве (с учётом
/// PushTransform-ов, в т.ч. shell-овского сдвига страницы под tab bar).
/// При не-аффинном или отсутствующем трансформе — возвращает rect без
/// изменений (conservative, BUG-140 policy).
fn apply_transform_to_clip(rect: Rect, m: Option<&Mat4>) -> Rect {
    let Some(m) = m.filter(|m| m.is_2d_affine()) else {
        return rect;
    };
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    let corners = [
        m.transform_point_2d(x0, y0),
        m.transform_point_2d(x1, y0),
        m.transform_point_2d(x0, y1),
        m.transform_point_2d(x1, y1),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (sx, sy) in corners {
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    Rect::new(min_x, min_y, (max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
}

/// ADR-016 M0.2: extra margin (CSS px) around the viewport before culling in
/// the wgpu renderer. Mirrors the femtovg backend's `CULL_SLOP_CSS_PX` —
/// absorbs anti-alias fringe / rounding and keeps a small off-screen band
/// live so a fast scroll step never exposes an un-drawn edge.
const WGPU_CULL_SLOP_CSS_PX: f32 = 256.0;

/// ADR-016 M0.2 viewport culling. Returns `true` when `screen_rect` (a leaf
/// command's box from [`DisplayCommand::cull_rect`], already shifted by the
/// scroll / sticky offset), after the current accumulated transform `m`,
/// lands fully outside the viewport (`vw`×`vh` CSS px) expanded by
/// [`WGPU_CULL_SLOP_CSS_PX`]. Because it tests the AABB of the four
/// transformed corners, the result is a conservative superset under
/// rotation/scale — a command is only culled when its entire footprint is
/// off-screen. A missing or non-affine (3D/perspective) transform disables
/// culling (`false`), so no visible pixel is ever dropped.
fn leaf_is_offscreen(screen_rect: Rect, m: Option<&Mat4>, vw: f32, vh: f32) -> bool {
    if screen_rect.width <= 0.0 || screen_rect.height <= 0.0 {
        return false;
    }
    let (x0, y0) = (screen_rect.x, screen_rect.y);
    let (x1, y1) = (screen_rect.x + screen_rect.width, screen_rect.y + screen_rect.height);
    let corners = match m {
        None => [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        Some(mat) if mat.is_2d_affine() => [
            mat.transform_point_2d(x0, y0),
            mat.transform_point_2d(x1, y0),
            mat.transform_point_2d(x0, y1),
            mat.transform_point_2d(x1, y1),
        ],
        // 3D / perspective transform in effect — do not cull (conservative).
        Some(_) => return false,
    };
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for (sx, sy) in corners {
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    let slop = WGPU_CULL_SLOP_CSS_PX;
    max_x < -slop || max_y < -slop || min_x > vw + slop || min_y > vh + slop
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
/// Thin wrapper over the shared [`crate::gradient_math::resolve_stop_positions`]
/// (single source of truth for all backends, PA-1) converting colours to the
/// `[f32; 4]` straight-RGBA layout the GPU vertex buffers use.
fn resolve_gradient_stops(stops: &[GradientStop], line_len: f32) -> Vec<(f32, [f32; 4])> {
    crate::gradient_math::resolve_stop_positions(stops, line_len)
        .into_iter()
        .map(|(pos, c)| {
            (
                pos,
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    c.a as f32 / 255.0,
                ],
            )
        })
        .collect()
}

/// CSS Images L3 §3.4 — compute linear gradient line endpoints in UV [0,1] space.
///
/// Returns `(start_uv, end_uv, line_len)` such that
/// `t = dot(uv-start, end-start)/|end-start|²` gives t=0 at the start-color
/// edge and t=1 at the end-color edge. `line_len` is the gradient line length
/// in CSS px (mirrors `cpu_raster::linear_uv_endpoints`) — callers must feed
/// it to [`resolve_gradient_stops`] so `Px`/`Calc` stop positions resolve
/// against the same length as the CPU/femtovg backends (BUG-277).
///
/// CSS angle convention: 0° = "to top", 90° = "to right", 180° = "to bottom".
/// Box dimensions `w`×`h` in CSS pixels.
fn linear_gradient_uv_endpoints(w: f32, h: f32, angle_deg: f32) -> ([f32; 2], [f32; 2], f32) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0, 0.5], [1.0, 0.5], w.max(h).max(1.0));
    }
    let theta = angle_deg.to_radians();
    let dx = theta.sin();
    let dy = -theta.cos(); // negative because CSS y grows down
    let half_len = (w * dx.abs() + h * dy.abs()) / 2.0;
    if half_len < 1e-6 {
        return ([0.5, 0.5], [0.5, 0.5], 1.0);
    }
    let cx = w / 2.0;
    let cy = h / 2.0;
    let sx = (cx - dx * half_len) / w;
    let sy = (cy - dy * half_len) / h;
    let ex = (cx + dx * half_len) / w;
    let ey = (cy + dy * half_len) / h;
    ([sx, sy], [ex, ey], 2.0 * half_len)
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
///
/// `src_region = Some([rx, ry, rw, rh])` (device px) — источник не
/// полноразмерная текстура, а bbox-офскрин региона: UV считается
/// относительно региона (`(css·dpr − r0) / rдлина`), NDC не меняется.
fn push_bounded_quad(
    out: &mut Vec<CompositeVertex>,
    bounds: lumen_core::geom::Rect,
    surf_w: f32,
    surf_h: f32,
    dpr: f32,
    alpha: f32,
    src_region: Option<[u32; 4]>,
) {
    let vw = surf_w / dpr;
    let vh = surf_h / dpr;
    let x0 = bounds.x / vw * 2.0 - 1.0;
    let x1 = (bounds.x + bounds.width) / vw * 2.0 - 1.0;
    let y0 = 1.0 - bounds.y / vh * 2.0;
    let y1 = 1.0 - (bounds.y + bounds.height) / vh * 2.0;
    let (u0, u1, v0, v1) = match src_region {
        Some([rx, ry, rw, rh]) => {
            let (rx, ry, rw, rh) = (rx as f32, ry as f32, (rw as f32).max(1.0), (rh as f32).max(1.0));
            (
                (bounds.x * dpr - rx) / rw,
                ((bounds.x + bounds.width) * dpr - rx) / rw,
                (bounds.y * dpr - ry) / rh,
                ((bounds.y + bounds.height) * dpr - ry) / rh,
            )
        }
        None => (
            bounds.x / vw,
            (bounds.x + bounds.width) / vw,
            bounds.y / vh,
            (bounds.y + bounds.height) / vh,
        ),
    };
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

/// Returns the final pen `x` (== `rect.x` + shaped advance) — used by
/// [`push_text_glyphs_mixed`] to measure a segment's real width without a
/// separate shaping pass.
#[allow(clippy::too_many_arguments)]
fn push_text_glyphs(
    out: &mut Vec<TextVertex>,
    rect: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    primary_face_id: usize,
    lazy: &mut LazyParsedFaces<'_>,
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    font_variation_axes: &[([u8; 4], f32)],
    tab_size: f32,
) -> f32 {
    // Multi-size atlas: подбираем bin под font_size, растеризируем глифы
    // на этом bin. Display масштаб = font_size / size_bin — если font_size
    // совпал с bin-ом (12/16/24/32/...) — масштаба нет, текст резкий.
    let size_bin = size_bin_for(font_size);
    let display_scale = font_size / size_bin as f32;

    // Baseline: ascent / (ascent − descent) primary face-а. Для Inter ≈ 0.80.
    // Используем primary для всех глифов в run-е — иначе при смешивании
    // face-ов символы прыгали бы по вертикали.
    let primary = lazy.faces[primary_face_id]
        .metrics
        .as_ref()
        .expect("primary face metrics must exist (checked by caller)");
    let ascent_ratio = primary.ascent as f32
        / (primary.ascent as f32 - primary.descent as f32);
    let baseline_y = rect.y + font_size * ascent_ratio;

    // Per-char cache на длительность одного DrawText: одни и те же символы
    // в строке («the the the») не нужно пробовать через все face-ы каждый раз.
    let mut char_face_cache: HashMap<char, (usize, u16)> = HashMap::new();
    // Normalized variation coords per face_id — лениво вычисляется при первом
    // обращении к данному face. Нормализация требует fvar+avar из шрифта
    // (единственный потребитель `ParsedFace` на пути без промахов атласа;
    // при пустых axes face не парсится вовсе).
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
            .or_insert_with(|| pick_face_for_codepoint(ch as u32, primary_face_id, lazy.faces));
        let metrics = lazy.faces[face_id]
            .metrics
            .as_ref()
            .expect("pick_face_for_codepoint вернул face_id с валидными metrics");
        let advance_scale = font_size / metrics.units_per_em as f32;
        let coords: &[f32] = match norm_coords_cache.entry(face_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                let computed = if font_variation_axes.is_empty() {
                    Vec::new()
                } else if let Some(face) = lazy.get(face_id) {
                    normalize_variation_axes(face, font_variation_axes)
                } else {
                    Vec::new()
                };
                v.insert(computed)
            }
        };
        let cached_glyph = ensure_glyph(
            cached,
            atlas,
            lazy,
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
            if let Some(&adv) = metrics.advances.get(glyph_id as usize) {
                cursor_x += adv as f32 * advance_scale;
            }
        }
    }
    cursor_x
}

/// Ph3 writing-mode vertical, Срез 2 — rotates a glyph run's vertices 90° CW
/// around the local origin and translates the result onto `dest`. Mirrors the
/// CPU rasterizer's `rasterize_text_rotated` transform
/// (`tiny_skia::Transform::from_row(0, 1, -1, 0, dest.x, dest.y)`): a point
/// laid out horizontally at `(x, y)` maps to `(-y + dest.x, x + dest.y)`.
/// Callers must have generated `verts` with `push_text_glyphs` at the local
/// origin `(0, 0)` — not at `dest`.
fn rotate_text_vertices_cw(verts: &mut [TextVertex], dest: Rect) {
    for v in verts {
        let (x, y) = (v.pos[0], v.pos[1]);
        v.pos = [-y + dest.x, x + dest.y];
    }
}

/// Ph3 writing-mode vertical, Срез 3 — per-glyph split for `text-orientation:
/// mixed`, wgpu path: each CJK ideograph paints upright at an increasing
/// offset along `dest`'s column (no rotation — same as `push_text_glyphs`
/// generating straight into `dest`); each run of non-CJK characters shapes as
/// one block at the local origin, then [`rotate_text_vertices_cw`] maps it
/// onto `dest` starting at the same column offset. Mirrors the CPU
/// rasterizer's `rasterize_text_mixed`. `push_text_glyphs`'s returned pen
/// position gives each segment's real shaped width, so a whitespace-only
/// segment (no visible glyph, but still an advance) still moves the cursor.
#[allow(clippy::too_many_arguments)]
fn push_text_glyphs_mixed(
    out: &mut Vec<TextVertex>,
    dest: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    primary_face_id: usize,
    lazy: &mut LazyParsedFaces<'_>,
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<AtlasKey, Option<CachedGlyph>>,
    font_variation_axes: &[([u8; 4], f32)],
    tab_size: f32,
) {
    let mut y_cursor = 0.0_f32;
    for seg in crate::display_list::split_mixed_runs(text) {
        let (seg_text, upright) = match seg {
            crate::display_list::MixedSegment::Cjk(ch) => {
                let mut s = String::new();
                s.push(ch);
                (s, true)
            }
            crate::display_list::MixedSegment::Other(s) => (s, false),
        };
        if upright {
            let seg_rect = Rect::new(dest.x, dest.y + y_cursor, dest.width, dest.height);
            let end_x = push_text_glyphs(
                out, seg_rect, &seg_text, font_size, color, primary_face_id, lazy, atlas,
                cached, font_variation_axes, tab_size,
            );
            y_cursor += end_x - dest.x;
        } else {
            let v_start = out.len();
            let local_rect = Rect::new(y_cursor, 0.0, dest.width, dest.height);
            let end_x = push_text_glyphs(
                out, local_rect, &seg_text, font_size, color, primary_face_id, lazy, atlas,
                cached, font_variation_axes, tab_size,
            );
            rotate_text_vertices_cw(&mut out[v_start..], dest);
            y_cursor = end_x;
        }
    }
}

/// CSS Fonts L4 §5.3 — for each character cascade. Сначала пробуем primary
/// face; если `cmap.glyph_index` возвращает None или Some(0) (= .notdef) —
/// обходим остальные loaded faces. Если ни у кого нет — возвращаем
/// `(primary, 0)` (отрисовать .notdef из primary).
///
/// Работает на owned `FaceMetrics.cmap` — без парсинга шрифтов.
fn pick_face_for_codepoint(
    cp: u32,
    primary_face_id: usize,
    faces: &[LoadedFace],
) -> (usize, u16) {
    if let Some(m) = faces.get(primary_face_id).and_then(|f| f.metrics.as_ref())
        && let Some(gid) = m.cmap.glyph_index(cp).filter(|&g| g != 0)
    {
        return (primary_face_id, gid);
    }
    for (idx, face) in faces.iter().enumerate() {
        if idx == primary_face_id {
            continue;
        }
        if let Some(m) = face.metrics.as_ref()
            && let Some(gid) = m.cmap.glyph_index(cp).filter(|&g| g != 0)
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
    lazy: &mut LazyParsedFaces<'_>,
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords: &[f32],
) -> Option<CachedGlyph> {
    let key = atlas_key(face_id, glyph_id, size_bin, AtlasKey::hash_coords(coords));
    if let Some(&entry) = cached.get(&key) {
        return entry;
    }

    // Промах atlas-кэша — единственный путь, где нужен распарсенный шрифт
    // (outline + HVAR). Ленивый парс: на тёплом кадре сюда не заходим.
    let face = lazy.get(face_id)?;
    let result = rasterize_and_insert(
        atlas,
        &face.font,
        &face.hmtx,
        face.head.units_per_em,
        key,
        coords,
    );
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

// Dash/dot геометрия для outline — общая для всех бэкендов (PA-1).
pub(crate) use crate::dash_math::dash_segments;

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
            // Сегменты считает общий crate::dash_math (PA-1): dash=max(6,2w),
            // gap=max(4,w), floor-snapping — совпадает с Edge/Skia.
            for (offset, len) in crate::dash_math::dashed_border_offsets(total, width) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                push_fill_quad(out, seg, color);
            }
        }
        BorderStyle::Dotted => {
            // Сегменты считает общий crate::dash_math (PA-1): симметричный
            // Bresenham-паттерн Edge. For dot_len ≤ 2px: use fill_quad
            // (rectangle) instead of SDF circle — Chrome/Edge renders thin
            // dotted borders as squares, not antialiased circles.
            let use_rect = width.max(1.0) <= 2.0;
            for (offset, len) in crate::dash_math::dotted_border_offsets(total, width) {
                let seg = if horizontal {
                    Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
                } else {
                    Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
                };
                if use_rect {
                    push_fill_quad(out, seg, color);
                } else {
                    push_circle_quad(circle_out, seg, color);
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

/// Создаёт отдельный UNIFORM-буфер с режимом blend одного composite pass —
/// тот же приём и по той же причине, что [`make_filter_param_buf`] (BUG-277 срез 2).
fn make_blend_mode_param_buf(device: &wgpu::Device, mode_padded: &[u32; 4]) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("blend-mode-param"),
        size: std::mem::size_of::<[u32; 4]>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buf.slice(..).get_mapped_range_mut().copy_from_slice(as_bytes(mode_padded.as_slice()));
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
    fn rotate_text_vertices_cw_maps_horizontal_run_into_vertical_column() {
        // Ph3 writing-mode vertical, Срез 2: a horizontal run laid out at the
        // local origin (0,0)..(40,10) — a wide, short glyph quad — must land
        // as a tall, narrow quad once rotated 90° CW onto dest (100, 50).
        let dest = Rect::new(100.0, 50.0, 10.0, 40.0);
        let mut verts = [
            TextVertex { pos: [0.0, 0.0], z: 0.0, uv: [0.0, 0.0], color: [0.0; 4] },
            TextVertex { pos: [40.0, 0.0], z: 0.0, uv: [1.0, 0.0], color: [0.0; 4] },
            TextVertex { pos: [40.0, 10.0], z: 0.0, uv: [1.0, 1.0], color: [0.0; 4] },
            TextVertex { pos: [0.0, 10.0], z: 0.0, uv: [0.0, 1.0], color: [0.0; 4] },
        ];
        rotate_text_vertices_cw(&mut verts, dest);
        // (0,0) -> (-0 + 100, 0 + 50) = (100, 50): local origin lands on dest origin.
        assert_eq!(verts[0].pos, [100.0, 50.0]);
        // (40,0) -> (0 + 100, 40 + 50) = (100, 90): local width becomes vertical extent.
        assert_eq!(verts[1].pos, [100.0, 90.0]);
        // (40,10) -> (-10 + 100, 40 + 50) = (90, 90).
        assert_eq!(verts[2].pos, [90.0, 90.0]);
        // (0,10) -> (-10 + 100, 0 + 50) = (90, 50): local height becomes horizontal extent.
        assert_eq!(verts[3].pos, [90.0, 50.0]);
        // UV/color untouched — only screen position rotates.
        assert_eq!(verts[0].uv, [0.0, 0.0]);
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

    // dash_segments unit-тесты переехали в crate::dash_math (PA-1).

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
                DisplayCommand::PushBlendMode { mode, .. } => {
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
            DisplayCommand::PushBlendMode { mode: BlendMode::Normal, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
        ];
        let (level, stack) = sim_blend_level(&cmds);
        assert_eq!(level, 0, "Normal blend mode не должен открывать offscreen level");
        assert_eq!(stack, vec![BlendMode::Normal]);
    }

    #[test]
    fn push_blend_mode_non_normal_creates_new_level() {
        // PushBlendMode { Multiply } — level становится 1.
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: BlendMode::Multiply, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
        ];
        let (level, _) = sim_blend_level(&cmds);
        assert_eq!(level, 1, "не-Normal blend mode должен открывать offscreen level");
    }

    #[test]
    fn pop_blend_mode_restores_level() {
        // Push/Pop пары: level возвращается в 0.
        let cmds = vec![
            DisplayCommand::PushBlendMode { mode: BlendMode::Screen, bounds: Rect::new(0.0, 0.0, 10.0, 10.0) },
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
