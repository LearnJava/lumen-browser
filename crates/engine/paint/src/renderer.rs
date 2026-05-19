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
use lumen_layout::{Color, FontStyle, FontWeight, OutlineStyle};
use winit::window::Window;

use crate::atlas::{AtlasKey, GlyphAtlas, GlyphEntry};
use crate::display_list::fit_image_quad;
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
    /// Per-vertex alpha-multiplier для PushOpacity (Phase 0 approximation).
    /// 1.0 — opacity:1 (визуально без изменений), <1.0 — затенение sample.a в шейдере.
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

    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    atlas_texture: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup,

    image_bgl: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    /// Cache декодированных картинок per-src. Заполняется через
    /// [`Renderer::register_image`] из shell-уровня (после fetch+decode).
    images: HashMap<String, GpuImage>,

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

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
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

    /// Рендерит две полосы display list-а одним кадром:
    /// - `content` — основная страница; ко всем `rect`-ам применяется
    ///   вертикальное смещение `-scroll_y` (CSS px). Так пользователь
    ///   «прокручивает» документ под фиксированным viewport-ом.
    /// - `overlay` — UI поверх (find-bar и т.п.); рисуется как есть, без
    ///   scroll-смещения. Делает overlay viewport-locked даже когда страница
    ///   прокручена.
    ///
    /// `scroll_y ≥ 0`. Negatives caller обязан клампить до 0 (top-bounce
    ///  нам пока не нужен).
    pub fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
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

        // Стек активных opacity-multiplier-ов (CSS Color §3.2). Effective opacity
        // = product. Phase 0 approximation: применяется к vertex color.a (fill/text)
        // и к vertex tint (image). Это **не** точная реализация — overlapping
        // дочерние элементы под одной opacity-группой композитятся попарно вместо
        // single-pass alpha (для строгой семантики требуется off-screen layer
        // composite, шаг (c-cont) задачи P2 1B). На одиночных элементах визуально
        // корректно — это покрывает 80%+ web-страниц Phase 0.
        let mut opacity_stack: Vec<f32> = Vec::new();

        // Текущий выставленный scissor (для дедупликации SetScissor-команд).
        // None = не выставлен (первый SetScissor нужен в любом случае).
        let mut current_scissor: Option<DeviceScissor> = None;
        let surface_w = self.config.width;
        let surface_h = self.config.height;

        let dpr_f32 = self.scale_factor.max(1e-6) as f32;

        let chained = content
            .iter()
            .map(|c| (c, -scroll_y))
            .chain(overlay.iter().map(|c| (c, 0.0_f32)));
        for (cmd, dy) in chained {
            match cmd {
                DisplayCommand::FillRect { rect, color } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = effective_alpha(&opacity_stack);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    push_fill_quad(
                        &mut fill_vertices,
                        translate_rect_y(*rect, dy),
                        apply_alpha_to_color(color_to_array(color), alpha),
                    );
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                DisplayCommand::DrawBorder { rect, widths: [wt, wr, wb, wl], colors: [ct, cr, cb, cl] } => {
                    if !sync_scissor_to_stack(&clip_stack, &mut current_scissor, &mut draw_ops, dpr_f32, surface_w, surface_h) {
                        continue;
                    }
                    let alpha = effective_alpha(&opacity_stack);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let r = translate_rect_y(*rect, dy);
                    let v_start = fill_vertices.len() as u32;
                    if *wt > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y, r.width, *wt), apply_alpha_to_color(color_to_array(ct), alpha));
                    }
                    if *wr > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x + r.width - wr, r.y, *wr, r.height), apply_alpha_to_color(color_to_array(cr), alpha));
                    }
                    if *wb > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y + r.height - wb, r.width, *wb), apply_alpha_to_color(color_to_array(cb), alpha));
                    }
                    if *wl > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y, *wl, r.height), apply_alpha_to_color(color_to_array(cl), alpha));
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
                    let alpha = effective_alpha(&opacity_stack);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let v_start = text_vertices.len() as u32;
                    push_text_glyphs(
                        &mut text_vertices,
                        translate_rect_y(*rect, dy),
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
                    let alpha = effective_alpha(&opacity_stack);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let r = translate_rect_y(*rect, dy);
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
                    let alpha = effective_alpha(&opacity_stack);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let scrolled = translate_rect_y(*rect, dy);
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
                    let scrolled = translate_rect_y(*rect, dy);
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, scrolled),
                        None => scrolled,
                    };
                    clip_stack.push(new);
                }
                DisplayCommand::PopClip => {
                    clip_stack.pop();
                }
                // Opacity stack: Phase 0 approximation — applied как
                // alpha-multiplier на vertex color вместо настоящего
                // off-screen-layer compositing. Pop с пустого стека —
                // защитный no-op (display-list builder обязан балансировать
                // Push/Pop, но мы устойчивы к unbalanced sequences).
                DisplayCommand::PushOpacity { alpha } => {
                    opacity_stack.push(*alpha);
                }
                DisplayCommand::PopOpacity => {
                    opacity_stack.pop();
                }
                // Phase 0 stub: blend mode. Off-screen-layer composite pass —
                // задача P2 1B шаг (c-cont) / P2 4 (mix-blend-mode /
                // backdrop-filter).
                DisplayCommand::PushBlendMode { .. } | DisplayCommand::PopBlendMode => {}
            }
        }

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

        // ── Frame ─────────────────────────────────────────────────────────
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // Iterate ordered DrawOp-list. Каждая Set/Draw в нужный момент
            // в исходном painter's order. set_pipeline/set_bind_group/
            // set_vertex_buffer на каждый draw — wgpu кэширует state-changes
            // внутри одного pass-а; для Phase 0 это безопасно (50-500 draw
            // calls на кадр).
            for op in &draw_ops {
                match op {
                    DrawOp::SetScissor(s) => {
                        if s.is_empty() {
                            // Пустой scissor wgpu не примет (panic). Caller
                            // (sync_scissor_to_stack) уже пропустил все draw
                            // под этим scissor-ом — SetScissor можно проставить
                            // на минимальную область, но проще схлопнуть к 1×1
                            // в углу: всё равно ничего не нарисуется.
                            pass.set_scissor_rect(0, 0, 1.min(surface_w), 1.min(surface_h));
                        } else {
                            pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                        }
                    }
                    DrawOp::Fill { v_start, v_count } => {
                        if let Some(vb) = &fill_vbuf {
                            pass.set_pipeline(&self.fill_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_vertex_buffer(0, vb.slice(..));
                            pass.draw(*v_start..*v_start + *v_count, 0..1);
                        }
                    }
                    DrawOp::Text { v_start, v_count } => {
                        if let Some(vb) = &text_vbuf {
                            pass.set_pipeline(&self.text_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                            pass.set_vertex_buffer(0, vb.slice(..));
                            pass.draw(*v_start..*v_start + *v_count, 0..1);
                        }
                    }
                    DrawOp::Image { v_start, v_count, image_batch_idx } => {
                        if let (Some(vb), Some(bind_group)) = (
                            &image_vbuf,
                            image_bind_groups.get(*image_batch_idx as usize),
                        ) {
                            pass.set_pipeline(&self.image_pipeline);
                            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                            pass.set_bind_group(1, bind_group, &[]);
                            pass.set_vertex_buffer(0, vb.slice(..));
                            pass.draw(*v_start..*v_start + *v_count, 0..1);
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
fn translate_rect_y(rect: Rect, dy: f32) -> Rect {
    Rect::new(rect.x, rect.y + dy, rect.width, rect.height)
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

/// Накопительная opacity из стека (CSS Color §3.2): произведение всех
/// текущих `PushOpacity { alpha }`-значений. Пустой стек = 1.0. Каждое
/// значение clamp-ится в `[0, 1]` для устойчивости (CSS spec уже clamp-ит
/// на cascade-уровне, но defensive). NaN на любом уровне → 0.0 (полное
/// затухание — лучше пропустить рисовку, чем нарисовать мусор).
pub(crate) fn effective_alpha(opacity_stack: &[f32]) -> f32 {
    let mut product = 1.0_f32;
    for &a in opacity_stack {
        if a.is_nan() {
            return 0.0;
        }
        product *= a.clamp(0.0, 1.0);
    }
    product
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

    // ── Opacity stack ─────────────────────────────────────────────────────

    #[test]
    fn effective_alpha_empty_stack_is_one() {
        assert_eq!(effective_alpha(&[]), 1.0);
    }

    #[test]
    fn effective_alpha_single_value() {
        assert_eq!(effective_alpha(&[0.5]), 0.5);
        assert_eq!(effective_alpha(&[1.0]), 1.0);
        assert_eq!(effective_alpha(&[0.0]), 0.0);
    }

    #[test]
    fn effective_alpha_nested_multiply() {
        // 0.5 × 0.8 = 0.4 (nested opacity).
        let a = effective_alpha(&[0.5, 0.8]);
        assert!((a - 0.4).abs() < 1e-6, "got {a}");
    }

    #[test]
    fn effective_alpha_three_levels() {
        // 0.5 × 0.5 × 0.5 = 0.125.
        let a = effective_alpha(&[0.5, 0.5, 0.5]);
        assert!((a - 0.125).abs() < 1e-6, "got {a}");
    }

    #[test]
    fn effective_alpha_clamps_out_of_range() {
        // > 1.0 clamped к 1.0; < 0 clamped к 0.
        assert_eq!(effective_alpha(&[1.5]), 1.0);
        assert_eq!(effective_alpha(&[-0.5]), 0.0);
        assert_eq!(effective_alpha(&[2.0, 0.5]), 0.5);
    }

    #[test]
    fn effective_alpha_nan_returns_zero() {
        // NaN — лучше не рисовать (default-safe).
        assert_eq!(effective_alpha(&[f32::NAN]), 0.0);
        assert_eq!(effective_alpha(&[0.5, f32::NAN, 0.8]), 0.0);
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
}
