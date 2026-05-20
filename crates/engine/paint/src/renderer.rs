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
use lumen_font::{Bitmap, Cmap, Font, Head, Hhea, Hmtx, Outline, Rasterizer, SystemFontIndex};
use lumen_image::{Image, PixelFormat};
use lumen_layout::{BorderStyle, Color, FontStyle, FontWeight, OutlineStyle};
use winit::window::Window;

use crate::atlas::{AtlasKey, GlyphAtlas, GlyphEntry};
use crate::display_list::{fit_image_quad, BlendMode};
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
    @location(1) color: vec4<f32>,
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
    var out: VOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return in.color;
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
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
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
    var out: VOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
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
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
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
    var out: VOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
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
    return vec4<f32>(c.rgb, c.a * in.alpha);
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

#[repr(C)]
#[derive(Copy, Clone)]
struct FillVertex {
    pos: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CompositeVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
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
    Text { v_start: u32, v_count: u32 },
    Image { v_start: u32, v_count: u32, image_batch_idx: u32 },
}

/// GPU-ресурсы для одной зарегистрированной картинки. Texture хранит уже
/// декодированные пиксели в формате `Rgba8Unorm` (Gray / GrayA / Rgb
/// конвертируются в Rgba при upload-е); bind group привязан к
/// `image_bind_group_layout` + общему sampler-у renderer-а. Intrinsic
/// dimensions (`width` / `height` в пикселях) хранятся для расчёта
/// `object-fit` / `object-position` на стадии рендеринга.
#[derive(Clone)]
struct GpuImage {
    bind_group: wgpu::BindGroup,
    // texture держим как поле даже без явного использования — wgpu освобождает
    // GPU-память когда дропается последняя ссылка; bind_group её не держит.
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

/// GPU-ресурсы одного off-screen opacity layer-а. Создаётся лениво через
/// `ensure_layer_textures`; переиспользуется пока размер surface не меняется.
/// `texture` хранится pub чтобы можно было использовать в
/// `encoder.copy_texture_to_texture` для blend-mode compositing.
struct OffscreenLayer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
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
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Device-pixel-ratio от winit (`Window::scale_factor`). Surface
    /// сконфигурирован в physical pixels (`config.width/height`), но shader
    /// делит позицию вершины на logical viewport (`config / scale_factor`),
    /// чтобы 1 CSS pixel = `scale_factor` device pixels — корректное
    /// масштабирование на HiDPI без правки display list-а.
    /// Обновляется через [`Renderer::set_scale_factor`] при `ScaleFactorChanged`
    /// событии winit (например, drag окна между мониторами с разной DPI).
    scale_factor: f64,

    fill_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bgl: wgpu::BindGroupLayout,
    blend_pipeline: wgpu::RenderPipeline,
    blend_bgl: wgpu::BindGroupLayout,
    blend_mode_uniform: wgpu::Buffer,
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
    /// Cache декодированных картинок per-src. Заполняется через
    /// [`Renderer::register_image`] из shell-уровня (после fetch+decode).
    images: HashMap<String, GpuImage>,
    /// Cache GPU-снимков слоёв per-id. Заполняется compositor-ом через
    /// [`Renderer::upload_layer_snapshot`] для кеширования неизменных слоёв.
    layer_snapshots: HashMap<u64, GpuLayerSnapshot>,

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

        let instance = wgpu::Instance::default();
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
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 8,
                            shader_location: 1,
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
            depth_stencil: None,
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
            depth_stencil: None,
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
            label: Some("image-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
            depth_stencil: None,
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

        let atlas = GlyphAtlas::new(ATLAS_DIM);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            scale_factor,
            fill_pipeline,
            text_pipeline,
            image_pipeline,
            uniform_buffer,
            uniform_bind_group,
            atlas_texture,
            atlas_bind_group,
            image_bgl,
            image_sampler,
            images: HashMap::new(),
            layer_snapshots: HashMap::new(),
            composite_pipeline,
            composite_bgl,
            blend_pipeline,
            blend_bgl,
            blend_mode_uniform,
            scratch_layer: None,
            layer_sampler,
            layer_textures: Vec::new(),
            surface_format: format,
            atlas,
            faces: vec![LoadedFace { bytes: font_bytes }],
            face_id_by_path: HashMap::new(),
            font_provider: Some(Arc::new(SystemFontIndex::new())),
            cached_glyphs: HashMap::new(),
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
            let Ok(bytes) = std::fs::read(&rec.path) else {
                continue;
            };
            if Font::parse(&bytes).is_err() {
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
        let rgba = convert_to_rgba(image);

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumen-image-texture"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
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
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image-bg"),
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
        self.images.insert(
            src,
            GpuImage {
                bind_group,
                _texture: texture,
                width: image.width,
                height: image.height,
            },
        );
        Ok(())
    }

    /// Снимает регистрацию изображения. После этого `DrawImage` для `src`
    /// снова рисует placeholder fill-quad.
    pub fn unregister_image(&mut self, src: &str) {
        self.images.remove(src);
    }

    /// Снимает регистрацию всех картинок (например, при переходе на новую
    /// страницу). GPU-память освобождается при drop-е `GpuImage.texture`.
    pub fn clear_images(&mut self) {
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

    /// Возвращает `(width, height)` снимка, или `None` если `id` не зарегистрирован.
    #[must_use]
    pub fn snapshot_dimensions(&self, id: u64) -> Option<(u32, u32)> {
        self.layer_snapshots.get(&id).map(|s| (s.width, s.height))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.layer_textures.clear();
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
        winit::dpi::PhysicalSize::new(self.config.width, self.config.height)
            .to_logical(self.scale_factor)
    }

    fn create_layer_texture(&self, width: u32, height: u32) -> OffscreenLayer {
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
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
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
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut image_vertices: Vec<ImageVertex> = Vec::new();
        // Bind groups для image draw-ов в порядке появления. DrawOp::Image
        // хранит индекс в этот Vec вместо клонирования BindGroup в каждый op.
        let mut image_bind_groups: Vec<wgpu::BindGroup> = Vec::new();

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

        // Render plan: список батчей и composite-переходов.
        #[derive(Clone, Copy)]
        enum LoadOpChoice { ClearWhite, ClearTransparent, Load }
        struct DrawBatchPlan { target_level: usize, load_op: LoadOpChoice, ops_start: usize, ops_end: usize }
        struct CompositePlan { from_level: usize, comp_v_start: u32, mode: BlendMode }
        enum RenderPlanItem { Draw(DrawBatchPlan), Composite(CompositePlan) }

        let mut render_plan: Vec<RenderPlanItem> = Vec::new();
        let mut composite_vertices: Vec<CompositeVertex> = Vec::new();

        let mut current_level: usize = 0;
        let mut level_alpha_stack: Vec<f32> = Vec::new();
        // Tracks blend mode per opened offscreen level (for non-Normal PushBlendMode).
        let mut level_blend_mode_stack: Vec<BlendMode> = Vec::new();
        let mut level_first: Vec<bool> = vec![true];
        let mut batch_start: usize = 0;

        // Текущий выставленный scissor (для дедупликации SetScисsor-команд).
        // None = не выставлен (первый SetScissor нужен в любом случае).
        let mut current_scissor: Option<DeviceScissor> = None;
        let surface_w = self.config.width;
        let surface_h = self.config.height;

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

        let chained = content
            .iter()
            .map(|c| (c, -scroll_y, -scroll_x))
            .chain(overlay.iter().map(|c| (c, 0.0_f32, 0.0_f32)));
        for (cmd, dy, dx) in chained {
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
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                DisplayCommand::DrawBorder {
                    rect,
                    widths: [wt, wr, wb, wl],
                    colors: [ct, cr, cb, cl],
                    styles: [st, sr, sb, sl],
                } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let r = translate_rect(*rect, dx, dy);
                    let v_start = fill_vertices.len() as u32;
                    // CSS Backgrounds L3 §6.3 — рёбра рисуются как
                    // прямоугольники полной width/height; Phase 0 без
                    // mitre-углов (углы overlap-ятся как fillRect-ы,
                    // что нормально пока border-color одинаков).
                    if *wt > 0.0 {
                        emit_border_side(
                            &mut fill_vertices,
                            Rect::new(r.x, r.y, r.width, *wt),
                            true,
                            *wt,
                            apply_alpha_to_color(color_to_array(ct), alpha),
                            *st,
                        );
                    }
                    if *wr > 0.0 {
                        emit_border_side(
                            &mut fill_vertices,
                            Rect::new(r.x + r.width - wr, r.y, *wr, r.height),
                            false,
                            *wr,
                            apply_alpha_to_color(color_to_array(cr), alpha),
                            *sr,
                        );
                    }
                    if *wb > 0.0 {
                        emit_border_side(
                            &mut fill_vertices,
                            Rect::new(r.x, r.y + r.height - wb, r.width, *wb),
                            true,
                            *wb,
                            apply_alpha_to_color(color_to_array(cb), alpha),
                            *sb,
                        );
                    }
                    if *wl > 0.0 {
                        emit_border_side(
                            &mut fill_vertices,
                            Rect::new(r.x, r.y, *wl, r.height),
                            false,
                            *wl,
                            apply_alpha_to_color(color_to_array(cl), alpha),
                            *sl,
                        );
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
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
                    font_variation_coords,
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
                        font_variation_coords,
                    );
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
                    let v_start = fill_vertices.len() as u32;
                    // Top stripe (с "ear" по углам слева/справа).
                    emit_outline_side(
                        &mut fill_vertices,
                        Rect::new(inner.x - w, inner.y - w, inner.width + 2.0 * w, w),
                        true,
                        w,
                        c,
                        *style,
                    );
                    // Bottom stripe (тоже с углами).
                    emit_outline_side(
                        &mut fill_vertices,
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
                        Rect::new(inner.x - w, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    // Right stripe.
                    emit_outline_side(
                        &mut fill_vertices,
                        Rect::new(inner.x + inner.width, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                DisplayCommand::DrawImage {
                    rect,
                    src,
                    alt: _,
                    object_fit,
                    object_position,
                } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let scrolled = translate_rect(*rect, dx, dy);
                    if let Some(gpu) = self.images.get(src) {
                        // CSS Images L3 §5.5: размещаем intrinsic-картинку
                        // согласно object-fit / object-position, обрезаем
                        // по box через UV-crop (без отдельной scissor-стадии).
                        // Пустое пересечение (полностью за пределами box) —
                        // пропускаем quad, placeholder тоже не рисуем.
                        if let Some((visible, uv_min, uv_max)) = fit_image_quad(
                            scrolled,
                            (gpu.width, gpu.height),
                            *object_fit,
                            *object_position,
                        ) {
                            let v_start = image_vertices.len() as u32;
                            push_image_quad(&mut image_vertices, visible, uv_min, uv_max, alpha);
                            let v_count = image_vertices.len() as u32 - v_start;
                            let image_batch_idx = image_bind_groups.len() as u32;
                            image_bind_groups.push(gpu.bind_group.clone());
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
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
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
                        let v_count = image_vertices.len() as u32 - v_start;
                        let image_batch_idx = image_bind_groups.len() as u32;
                        image_bind_groups.push(snap.bind_group.clone());
                        draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                    }
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
        let viewport = [
            self.config.width as f32 / dpr,
            self.config.height as f32 / dpr,
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

        // ── Off-screen textures ───────────────────────────────────────────
        // Blend composites (mode != Normal) also need from_level offscreen layers.
        let max_level = render_plan.iter().fold(0usize, |m, item| match item {
            RenderPlanItem::Draw(b) => m.max(b.target_level),
            RenderPlanItem::Composite(c) => m.max(c.from_level),
        });
        if max_level > 0 {
            self.ensure_layer_textures(max_level, surface_w, surface_h);
        }

        // ── Frame ─────────────────────────────────────────────────────────
        let frame = self.surface.get_current_texture()?;
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
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
                    }
                }
            };
        }

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
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("draw-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: None,
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
            }
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}

/// Сдвиг rect-а по Y (CSS px). Используется в `render` для применения
/// scroll-offset-а к page-полосе display list-а; overlay-полоса получает
/// `dy = 0`. Без mutation — Rect: Copy.
fn translate_rect(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

fn push_fill_quad(out: &mut Vec<FillVertex>, rect: Rect, color: [f32; 4]) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    out.extend_from_slice(&[
        FillVertex { pos: [x0, y0], color },
        FillVertex { pos: [x1, y0], color },
        FillVertex { pos: [x1, y1], color },
        FillVertex { pos: [x0, y0], color },
        FillVertex { pos: [x1, y1], color },
        FillVertex { pos: [x0, y1], color },
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
        ImageVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y0], uv: [u1, v0], alpha },
        ImageVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y0], uv: [u0, v0], alpha },
        ImageVertex { pos: [x1, y1], uv: [u1, v1], alpha },
        ImageVertex { pos: [x0, y1], uv: [u0, v1], alpha },
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
    font_variation_coords: &[f32],
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

    let mut cursor_x = rect.x;
    for ch in text.chars() {
        let (face_id, glyph_id) = *char_face_cache
            .entry(ch)
            .or_insert_with(|| pick_face_for_codepoint(ch as u32, primary_face_id, parsed));
        let face = parsed[face_id]
            .as_ref()
            .expect("pick_face_for_codepoint вернул face_id с valid parsed face");
        let advance_scale = font_size / face.head.units_per_em as f32;
        let cached_glyph = ensure_glyph(
            cached,
            atlas,
            &face.font,
            &face.hmtx,
            face.head.units_per_em,
            face_id,
            glyph_id,
            size_bin,
            font_variation_coords,
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
                TextVertex { pos: [x0, y0], uv: [u0, v0], color },
                TextVertex { pos: [x1, y0], uv: [u1, v0], color },
                TextVertex { pos: [x1, y1], uv: [u1, v1], color },
                TextVertex { pos: [x0, y0], uv: [u0, v0], color },
                TextVertex { pos: [x1, y1], uv: [u1, v1], color },
                TextVertex { pos: [x0, y1], uv: [u0, v1], color },
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
    let advance_native = hmtx.advance_width(key.glyph_id).unwrap_or(0);
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
/// `(offset, length)` по pattern-у `(dash_len, gap_len)`. Используется
/// для outline-style Dashed/Dotted. Сегменты центрируются: если общая
/// длина пользованного pattern-а меньше `total_length`, leftover делится
/// поровну в leading/trailing — визуально аккуратные углы.
///
/// Возвращает empty при degenerate-входе: `total_length <= 0`,
/// `dash_len <= 0`. При `gap_len <= 0` возвращает один full-length сегмент
/// (= Solid fallback, защищает от деления на ноль).
///
/// `n_dashes` — `floor((total_length + gap_len) / (dash_len + gap_len))`
/// округлено вниз до >= 1. Последний даш обрезается до `total_length`,
/// если pattern не помещается ровно (например, total=10, dash=3, gap=2 →
/// 3 даша на 13 пытались бы, helper зажимает до 10 — обрезка финального).
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
    let n_floor = ((total_length + gap_len) / period).floor() as i32;
    let n_dashes = n_floor.max(1) as usize;
    let used = n_dashes as f32 * dash_len + (n_dashes.saturating_sub(1)) as f32 * gap_len;
    let leading = ((total_length - used) * 0.5).max(0.0);
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
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: BorderStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    let pattern = match style {
        BorderStyle::Dashed => {
            let dash_len = (width * 2.0).max(1.0);
            let gap_len = width.max(1.0);
            dash_segments(total, dash_len, gap_len)
        }
        BorderStyle::Dotted => {
            let dot_len = width.max(1.0);
            dash_segments(total, dot_len, dot_len)
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
            return;
        }
        BorderStyle::Solid | BorderStyle::None => {
            push_fill_quad(out, side_rect, color);
            return;
        }
    };
    for (offset, len) in pattern {
        let segment_rect = if horizontal {
            Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
        } else {
            Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
        };
        push_fill_quad(out, segment_rect, color);
    }
}

/// Рисует одну сторону outline (top / right / bottom / left) с учётом
/// `OutlineStyle`. `horizontal=true` для top/bottom (даш-pattern идёт
/// по X), `false` для left/right (по Y). `width` — толщина outline
/// (CSS px), используется как dash/dot длина. Для Solid/Auto/None —
/// один full-rect; для Dashed — pattern `(2w, w)`; для Dotted — `(w, w)`.
fn emit_outline_side(
    out: &mut Vec<FillVertex>,
    side_rect: Rect,
    horizontal: bool,
    width: f32,
    color: [f32; 4],
    style: OutlineStyle,
) {
    let total = if horizontal { side_rect.width } else { side_rect.height };
    let pattern = match style {
        OutlineStyle::Dashed => {
            let dash_len = (width * 2.0).max(1.0);
            let gap_len = width.max(1.0);
            dash_segments(total, dash_len, gap_len)
        }
        OutlineStyle::Dotted => {
            let dot_len = width.max(1.0);
            dash_segments(total, dot_len, dot_len)
        }
        // Solid / Auto / None — full-length rect. None обычно не доходит
        // до emit (фильтр на стороне build_display_list), но мы устойчивы.
        OutlineStyle::Solid | OutlineStyle::Auto | OutlineStyle::None => {
            push_fill_quad(out, side_rect, color);
            return;
        }
    };
    for (offset, len) in pattern {
        let segment_rect = if horizontal {
            Rect::new(side_rect.x + offset, side_rect.y, len, side_rect.height)
        } else {
            Rect::new(side_rect.x, side_rect.y + offset, side_rect.width, len)
        };
        push_fill_quad(out, segment_rect, color);
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
        // dash=4, gap=2 → period=6; total=10 → (10+2)/6=2 dashes;
        // used = 2*4 + 1*2 = 10; leading=(10-10)/2 = 0.
        // Сегменты: (0, 4), (6, 4).
        let segs = dash_segments(10.0, 4.0, 2.0);
        assert_eq!(segs.len(), 2);
        assert!((segs[0].0 - 0.0).abs() < 1e-6);
        assert!((segs[0].1 - 4.0).abs() < 1e-6);
        assert!((segs[1].0 - 6.0).abs() < 1e-6);
        assert!((segs[1].1 - 4.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_centered_leftover() {
        // dash=2, gap=2 → period=4; total=10 → (10+2)/4=3 dashes;
        // used = 3*2 + 2*2 = 10; leading=0; сегменты (0,2),(4,2),(8,2).
        let segs = dash_segments(10.0, 2.0, 2.0);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], (0.0, 2.0));
        assert_eq!(segs[1], (4.0, 2.0));
        assert_eq!(segs[2], (8.0, 2.0));
    }

    #[test]
    fn dash_segments_with_leftover_centers() {
        // dash=2, gap=2 → period=4; total=11 → (11+2)/4=3 dashes;
        // used=10; leading=(11-10)/2=0.5.
        let segs = dash_segments(11.0, 2.0, 2.0);
        assert_eq!(segs.len(), 3);
        assert!((segs[0].0 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_too_short_one_dash() {
        // total=3, dash=4, gap=2 — n_floor=(3+2)/6=0 → max(1)=1; used=4;
        // leading=max((3-4)/2, 0)=0; сегмент (0,3) обрезается до total.
        let segs = dash_segments(3.0, 4.0, 2.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].0, 0.0);
        assert!((segs[0].1 - 3.0).abs() < 1e-6);
    }

    #[test]
    fn dash_segments_dotted_pattern() {
        // dot_len=2, gap=2 (как Dotted с width=2): total=10 → 3 точки на (0,2),(4,2),(8,2).
        let segs = dash_segments(10.0, 2.0, 2.0);
        assert_eq!(segs.len(), 3);
    }

    #[test]
    fn dash_segments_count_for_typical_outline() {
        // Outline width=2, dashed: dash=4, gap=2; полоса 100 px.
        // n=(100+2)/6=17 dashes; used=17*4 + 16*2 = 68+32 = 100; leading=0.
        let segs = dash_segments(100.0, 4.0, 2.0);
        assert_eq!(segs.len(), 17);
    }

    // ── emit_border_side ──────────────────────────────────────────────────

    fn collect_border_quads(
        side_rect: Rect,
        horizontal: bool,
        width: f32,
        style: BorderStyle,
    ) -> Vec<Rect> {
        let color = [1.0f32; 4];
        let mut verts: Vec<FillVertex> = Vec::new();
        emit_border_side(&mut verts, side_rect, horizontal, width, color, style);
        // Each quad = 6 vertices (2 triangles); reconstruct bounding rects.
        verts
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
        let quads = collect_border_quads(r, true, 6.0, BorderStyle::Solid);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0], r);
    }

    #[test]
    fn emit_border_side_dashed_produces_multiple_quads() {
        // width=4 → dash=8, gap=4; side 100 wide → several segments.
        let r = Rect::new(0.0, 0.0, 100.0, 4.0);
        let quads = collect_border_quads(r, true, 4.0, BorderStyle::Dashed);
        assert!(quads.len() > 1, "dashed must produce multiple segments");
        for q in &quads {
            assert_eq!(q.height, 4.0, "all segments must span full border height");
        }
    }

    #[test]
    fn emit_border_side_dotted_square_segments() {
        // width=4 → dot=4; horizontal side 40 wide → 5 dots.
        let r = Rect::new(0.0, 0.0, 40.0, 4.0);
        let quads = collect_border_quads(r, true, 4.0, BorderStyle::Dotted);
        assert!(quads.len() > 1, "dotted must produce multiple segments");
        for q in &quads {
            assert_eq!(q.height, 4.0);
        }
    }

    #[test]
    fn emit_border_side_double_two_quads_horizontal() {
        // width=9 → line≈3; two lines at top and bottom of the side_rect.
        let r = Rect::new(0.0, 0.0, 100.0, 9.0);
        let quads = collect_border_quads(r, true, 9.0, BorderStyle::Double);
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
        let quads = collect_border_quads(r, true, 2.0, BorderStyle::Double);
        assert_eq!(quads.len(), 1, "width<3 must fall back to single solid quad");
    }

    #[test]
    fn emit_border_side_double_vertical() {
        // Vertical double border (left/right side).
        let r = Rect::new(0.0, 0.0, 9.0, 100.0);
        let quads = collect_border_quads(r, false, 9.0, BorderStyle::Double);
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
}
