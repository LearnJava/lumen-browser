//! wgpu-растеризатор для display list.
//!
//! Два конвейера:
//! 1. **Fill** — заливка прямоугольников цветом. Вершина = (pos, color),
//!    альфа-блендинг. Используется для backgrounds блоков.
//! 2. **Text** — текстурированные квады по глифам из atlas-а.
//!    Вершина = (pos, uv, color), фрагмент сэмплит R8-альфу из atlas-а
//!    и умножает на цвет текста.
//!
//! Глифы растеризуются по требованию через `lumen_font::Rasterizer` при
//! фиксированном `ATLAS_PIXEL_SIZE` (24 px); итоговый display-размер
//! получается масштабированием квада (font_size / ATLAS_PIXEL_SIZE).
//! Это компромисс ради простоты — multi-size atlas будет потом.

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use lumen_core::geom::Rect;
use lumen_font::{Bitmap, Cmap, Font, Hhea, Hmtx, Outline, Rasterizer};
use lumen_layout::Color;
use winit::window::Window;

use crate::atlas::{GlyphAtlas, GlyphEntry};
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

/// Закешированная информация о глифе: позиция в атласе + метрики
/// (left/top — на ATLAS_PIXEL_SIZE; advance — в font units, без масштабирования).
#[derive(Clone, Copy)]
struct CachedGlyph {
    entry: GlyphEntry,
    left: f32,
    top: f32,
    advance_native: u16,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    fill_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,

    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    atlas_texture: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup,

    atlas: GlyphAtlas,
    font_bytes: Vec<u8>,
    cached_glyphs: HashMap<u16, Option<CachedGlyph>>,
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

        let atlas = GlyphAtlas::new(ATLAS_DIM);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            fill_pipeline,
            text_pipeline,
            uniform_buffer,
            uniform_bind_group,
            atlas_texture,
            atlas_bind_group,
            atlas,
            font_bytes,
            cached_glyphs: HashMap::new(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&mut self, list: &DisplayList) -> Result<(), wgpu::SurfaceError> {
        // Парсим шрифт раз в кадр (дёшево: только walk table directory).
        let font = match Font::parse(&self.font_bytes) {
            Ok(f) => f,
            Err(_) => return Ok(()), // невозможно при правильном Renderer::new, но защитимся
        };
        let head = font.head().ok();
        let hhea = font.hhea().ok();
        let cmap = font.cmap().ok();
        let hmtx = font.hmtx().ok();

        // ── Сбор вершин ────────────────────────────────────────────────────
        let mut fill_vertices: Vec<FillVertex> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();

        let raster_native = head.map(|h| Rasterizer::new(ATLAS_PIXEL_SIZE, h.units_per_em));

        for cmd in list {
            match cmd {
                DisplayCommand::FillRect { rect, color } => {
                    push_fill_quad(&mut fill_vertices, *rect, color_to_array(color));
                }
                DisplayCommand::DrawText {
                    rect,
                    text,
                    font_size,
                    color,
                } => {
                    let (Some(head), Some(hhea), Some(cmap), Some(hmtx), Some(raster)) =
                        (&head, &hhea, &cmap, &hmtx, &raster_native)
                    else {
                        continue;
                    };
                    push_text_glyphs(
                        &mut text_vertices,
                        *rect,
                        text,
                        *font_size,
                        color_to_array(color),
                        head.units_per_em,
                        hhea,
                        cmap,
                        hmtx,
                        raster,
                        &font,
                        &mut self.atlas,
                        &mut self.cached_glyphs,
                    );
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

#[allow(clippy::too_many_arguments)]
fn push_text_glyphs(
    out: &mut Vec<TextVertex>,
    rect: Rect,
    text: &str,
    font_size: f32,
    color: [f32; 4],
    units_per_em: u16,
    hhea: &Hhea,
    cmap: &Cmap,
    hmtx: &Hmtx,
    raster: &Rasterizer,
    font: &Font,
    atlas: &mut GlyphAtlas,
    cached: &mut HashMap<u16, Option<CachedGlyph>>,
) {
    // Display scale: глифы в атласе растеризованы на ATLAS_PIXEL_SIZE,
    // а нужно показать на font_size — умножаем.
    let display_scale = font_size / ATLAS_PIXEL_SIZE;
    let advance_scale = font_size / units_per_em as f32;

    // Baseline: ascent / (ascent − descent). Для Inter ≈ 0.80.
    let ascent_ratio =
        hhea.ascent as f32 / (hhea.ascent as f32 - hhea.descent as f32);
    let baseline_y = rect.y + font_size * ascent_ratio;

    let mut cursor_x = rect.x;
    for ch in text.chars() {
        let glyph_id = cmap.glyph_index(ch as u32).unwrap_or(0);
        let cached_glyph = ensure_glyph(cached, atlas, font, raster, hmtx, glyph_id);

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
            // Глиф не отрисовался (composite, empty или нет места в атласе).
            // Двигаем cursor на advance, если он известен.
            if let Some(adv) = hmtx.advance_width(glyph_id) {
                cursor_x += adv as f32 * advance_scale;
            }
        }
    }
}

fn ensure_glyph(
    cached: &mut HashMap<u16, Option<CachedGlyph>>,
    atlas: &mut GlyphAtlas,
    font: &Font,
    raster: &Rasterizer,
    hmtx: &Hmtx,
    glyph_id: u16,
) -> Option<CachedGlyph> {
    if let Some(&entry) = cached.get(&glyph_id) {
        return entry;
    }

    let result = rasterize_and_insert(atlas, font, raster, hmtx, glyph_id);
    cached.insert(glyph_id, result);
    result
}

fn rasterize_and_insert(
    atlas: &mut GlyphAtlas,
    font: &Font,
    raster: &Rasterizer,
    hmtx: &Hmtx,
    glyph_id: u16,
) -> Option<CachedGlyph> {
    let glyph = font.glyph(glyph_id).ok().flatten()?;
    // Composite пока не растеризуем.
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
