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
//! Глифы растеризуются по требованию через `lumen_font::Rasterizer` при
//! фиксированном `ATLAS_PIXEL_SIZE` (24 px); итоговый display-размер
//! получается масштабированием квада (font_size / ATLAS_PIXEL_SIZE).
//! Это компромисс ради простоты — multi-size atlas будет потом.

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::ext::{FontProvider, FontStyle as CssFontStyle};
use lumen_core::geom::Rect;
use lumen_font::{Bitmap, Cmap, Font, Head, Hhea, Hmtx, Outline, Rasterizer, SystemFontIndex};
use lumen_image::{Image, PixelFormat};
use lumen_layout::{Color, FontStyle, FontWeight};
use winit::window::Window;

use crate::atlas::{GlyphAtlas, GlyphEntry};
use crate::display_list::fit_image_quad;
use crate::{DisplayCommand, DisplayList};

/// Размер атласа в пикселях (квадратный).
const ATLAS_DIM: u32 = 512;
/// Размер растеризации глифа в атласе. Display-сторона потом масштабирует.
const ATLAS_PIXEL_SIZE: f32 = 24.0;

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
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(image_tex, image_smp, in.uv);
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

/// Закешированная информация о глифе: позиция в атласе + метрики
/// (left/top — на ATLAS_PIXEL_SIZE; advance — в font units, без масштабирования).
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

/// Распарсенный face для одного `render()`-вызова: Font + ключевые таблицы
/// + per-face Rasterizer. Borrow от `LoadedFace.bytes`. Используется в
/// codepoint-cascade: per-char проверяем `cmap.glyph_index` у каждого
/// face-а и выбираем тот, где глиф найден.
struct ParsedFace<'a> {
    font: Font<'a>,
    head: Head,
    hhea: Hhea,
    cmap: Cmap<'a>,
    hmtx: Hmtx<'a>,
    raster: Rasterizer,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

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
    /// Кэш растеризованных глифов: ключ `(face_id, glyph_id)`, потому что
    /// glyph_id у разных face-ов имеют разное значение.
    cached_glyphs: HashMap<(usize, u16), Option<CachedGlyph>>,
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

    pub fn render(&mut self, list: &DisplayList) -> Result<(), wgpu::SurfaceError> {
        // Pre-resolve primary face_id для каждой DrawText-команды +
        // lazy-загрузка новых face-ов до сбора вершин. Делается до парсинга
        // (resolve мутирует self.faces).
        let mut text_face_ids: Vec<usize> = Vec::with_capacity(list.len());
        for cmd in list {
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
                let raster = Rasterizer::new(ATLAS_PIXEL_SIZE, head.units_per_em);
                Some(ParsedFace { font, head, hhea, cmap, hmtx, raster })
            })
            .collect();

        // ── Сбор вершин ────────────────────────────────────────────────────
        let mut fill_vertices: Vec<FillVertex> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut image_vertices: Vec<ImageVertex> = Vec::new();
        // Per-batch info: bind_group (clone — Arc inside wgpu) + диапазон
        // вершин в общем image_vbuf. Кладём в порядке появления DrawImage —
        // картины с painter's order не сливаются между Block/InlineRun
        // соседями, batching по src в Phase 0 не делаем (5-10 изображений
        // на страницу = pareto draw call-ов).
        let mut image_batches: Vec<(wgpu::BindGroup, u32, u32)> = Vec::new();

        for cmd in list {
            match cmd {
                DisplayCommand::FillRect { rect, color } => {
                    push_fill_quad(&mut fill_vertices, *rect, color_to_array(color));
                }
                DisplayCommand::DrawBorder { rect, widths: [wt, wr, wb, wl], colors: [ct, cr, cb, cl] } => {
                    let r = *rect;
                    if *wt > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y, r.width, *wt), color_to_array(ct));
                    }
                    if *wr > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x + r.width - wr, r.y, *wr, r.height), color_to_array(cr));
                    }
                    if *wb > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y + r.height - wb, r.width, *wb), color_to_array(cb));
                    }
                    if *wl > 0.0 {
                        push_fill_quad(&mut fill_vertices, Rect::new(r.x, r.y, *wl, r.height), color_to_array(cl));
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
                } => {
                    let primary_face_id = text_face_iter.next().unwrap_or(0);
                    if parsed_faces
                        .get(primary_face_id)
                        .and_then(|p| p.as_ref())
                        .is_none()
                    {
                        continue;
                    }
                    push_text_glyphs(
                        &mut text_vertices,
                        *rect,
                        text,
                        *font_size,
                        color_to_array(color),
                        primary_face_id,
                        &parsed_faces,
                        &mut self.atlas,
                        &mut self.cached_glyphs,
                    );
                }
                DisplayCommand::DrawImage {
                    rect,
                    src,
                    alt: _,
                    object_fit,
                    object_position,
                } => {
                    if let Some(gpu) = self.images.get(src) {
                        // CSS Images L3 §5.5: размещаем intrinsic-картинку
                        // согласно object-fit / object-position, обрезаем
                        // по box через UV-crop (без отдельной scissor-стадии).
                        // Пустое пересечение (полностью за пределами box) —
                        // пропускаем quad, placeholder тоже не рисуем.
                        if let Some((visible, uv_min, uv_max)) = fit_image_quad(
                            *rect,
                            (gpu.width, gpu.height),
                            *object_fit,
                            *object_position,
                        ) {
                            let offset = image_vertices.len() as u32;
                            push_image_quad(&mut image_vertices, visible, uv_min, uv_max);
                            let count = image_vertices.len() as u32 - offset;
                            image_batches.push((gpu.bind_group.clone(), offset, count));
                        }
                    } else {
                        // Картинку никто не зарегистрировал (fetch не сделан /
                        // декодер упал / неизвестный формат) — fallback на
                        // серый placeholder, чтобы место в layout-е было видно.
                        push_fill_quad(&mut fill_vertices, *rect, [0.85, 0.85, 0.85, 1.0]);
                    }
                }
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
        let viewport = [
            self.config.width as f32,
            self.config.height as f32,
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
            if let Some(vb) = &fill_vbuf {
                pass.set_pipeline(&self.fill_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..fill_vertices.len() as u32, 0..1);
            }
            if let Some(vb) = &image_vbuf {
                pass.set_pipeline(&self.image_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                for (bind_group, offset, count) in &image_batches {
                    pass.set_bind_group(1, bind_group, &[]);
                    pass.draw(*offset..*offset + *count, 0..1);
                }
            }
            if let Some(vb) = &text_vbuf {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..text_vertices.len() as u32, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
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

fn push_image_quad(out: &mut Vec<ImageVertex>, rect: Rect, uv_min: [f32; 2], uv_max: [f32; 2]) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width;
    let y1 = rect.y + rect.height;
    let [u0, v0] = uv_min;
    let [u1, v1] = uv_max;
    out.extend_from_slice(&[
        ImageVertex { pos: [x0, y0], uv: [u0, v0] },
        ImageVertex { pos: [x1, y0], uv: [u1, v0] },
        ImageVertex { pos: [x1, y1], uv: [u1, v1] },
        ImageVertex { pos: [x0, y0], uv: [u0, v0] },
        ImageVertex { pos: [x1, y1], uv: [u1, v1] },
        ImageVertex { pos: [x0, y1], uv: [u0, v1] },
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
    cached: &mut HashMap<(usize, u16), Option<CachedGlyph>>,
) {
    // Display scale: глифы в атласе растеризованы на ATLAS_PIXEL_SIZE,
    // а нужно показать на font_size — умножаем.
    let display_scale = font_size / ATLAS_PIXEL_SIZE;

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
            &face.raster,
            &face.hmtx,
            face_id,
            glyph_id,
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

fn ensure_glyph(
    cached: &mut HashMap<(usize, u16), Option<CachedGlyph>>,
    atlas: &mut GlyphAtlas,
    font: &Font,
    raster: &Rasterizer,
    hmtx: &Hmtx,
    face_id: usize,
    glyph_id: u16,
) -> Option<CachedGlyph> {
    let key = (face_id, glyph_id);
    if let Some(&entry) = cached.get(&key) {
        return entry;
    }

    let result = rasterize_and_insert(atlas, font, raster, hmtx, glyph_id);
    cached.insert(key, result);
    result
}

fn rasterize_and_insert(
    atlas: &mut GlyphAtlas,
    font: &Font,
    raster: &Rasterizer,
    hmtx: &Hmtx,
    glyph_id: u16,
) -> Option<CachedGlyph> {
    // glyph_resolved разворачивает composite в Simple рекурсивно, подставляя
    // компоненты с их transform/offset. Для уже simple-глифа возвращает как есть.
    let glyph = font.glyph_resolved(glyph_id).ok().flatten()?;
    if !matches!(glyph.outline, Outline::Simple(_)) {
        return None;
    }
    let bitmap: Bitmap = raster.rasterize(&glyph)?;
    let entry = atlas.insert(glyph_id, &bitmap)?;
    let advance_native = hmtx.advance_width(glyph_id).unwrap_or(0);
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
