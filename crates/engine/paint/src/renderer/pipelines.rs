//! P1/SPLIT-RN7: pipeline-геттеры + `impl PipelineDeps` + pipeline-builder'ы/
//! `Hot*`/`PipelineDeps`/`WarmedPipeline` — три диапазона из `renderer.rs`
//! (2594…3317, 6267…7250, 7253…7319 до вырезки). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-7).

use super::*;

// BUG-406: на DX12 суммарная компиляция шейдеров/пайплайнов стоит 3–7 с против
// 0.28 с на Vulkan (то же железо), и до этих двух счётчиков известна была только
// суммарная цифра. Обе обёртки — no-op без `LUMEN_FRAME_LOG`.
/// Создаёт шейдерный модуль, печатая время его трансляции под `LUMEN_FRAME_LOG`
/// (naga: парсинг + валидация WGSL).
fn timed_shader(
    device: &wgpu::Device,
    desc: wgpu::ShaderModuleDescriptor<'_>,
) -> wgpu::ShaderModule {
    if !crate::frame_log_enabled() {
        return device.create_shader_module(desc);
    }
    let label = desc.label.unwrap_or("<unlabeled>").to_string();
    let t0 = std::time::Instant::now();
    let module = device.create_shader_module(desc);
    eprintln!("[wgpu]   shader {label}: {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
    module
}
/// Создаёт render-пайплайн, печатая время его компиляции под `LUMEN_FRAME_LOG`
/// (на DX12 здесь идёт naga → HLSL → FXC/DXC).
fn timed_pipeline(
    device: &wgpu::Device,
    desc: &wgpu::RenderPipelineDescriptor<'_>,
) -> wgpu::RenderPipeline {
    if !crate::frame_log_enabled() {
        return device.create_render_pipeline(desc);
    }
    let label = desc.label.unwrap_or("<unlabeled>");
    let t0 = std::time::Instant::now();
    let pipeline = device.create_render_pipeline(desc);
    eprintln!("[wgpu]   pipeline {label}: {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
    pipeline
}

/// Пайплайн сплошных прямоугольников — самый частый примитив страницы.
/// Горячий: компилируется при старте (BUG-406).
fn build_fill_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let fill_shader = timed_shader(device, wgpu::ShaderModuleDescriptor {
        label: Some("fill-shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{FILL_SHADER_SRC}").into()),
    });
    let fill_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fill-layout"),
        bind_group_layouts: &[uniform_bgl],
        push_constant_ranges: &[],
    });
    timed_pipeline(device, &wgpu::RenderPipelineDescriptor {
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
    })
}

/// Пайплайн скруглённого прямоугольника (SDF) — фоны и рамки с `border-radius`.
/// Горячий: компилируется при старте (BUG-406).
fn build_rrect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let rrect_shader = timed_shader(device, wgpu::ShaderModuleDescriptor {
        label: Some("rrect-shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{RRECT_SHADER_SRC}").into()),
    });
    let rrect_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rrect-layout"),
        bind_group_layouts: &[uniform_bgl],
        push_constant_ranges: &[],
    });
    timed_pipeline(device, &wgpu::RenderPipelineDescriptor {
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
    })
}

/// Пайплайн текста (квады глифов из атласа).
/// Горячий: компилируется при старте (BUG-406).
fn build_text_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
    atlas_bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let text_shader = timed_shader(device, wgpu::ShaderModuleDescriptor {
        label: Some("text-shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{TEXT_SHADER_SRC}").into()),
    });
    let text_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text-layout"),
        bind_group_layouts: &[uniform_bgl, atlas_bgl],
        push_constant_ranges: &[],
    });
    timed_pipeline(device, &wgpu::RenderPipelineDescriptor {
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
    })
}

/// Пайплайн растровой картинки (текстурный квад, bind group на картинку).
/// Горячий: компилируется при старте (BUG-406).
fn build_image_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
    image_bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let image_shader = timed_shader(device, wgpu::ShaderModuleDescriptor {
        label: Some("image-shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{IMAGE_SHADER_SRC}").into()),
    });
    let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("image-layout"),
        bind_group_layouts: &[uniform_bgl, image_bgl],
        push_constant_ranges: &[],
    });
    timed_pipeline(device, &wgpu::RenderPipelineDescriptor {
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
    })
}

/// Пайплайн градиентов (linear + radial).
/// Горячий: компилируется при старте (BUG-406).
fn build_gradient_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
    gradient_bgl: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let gradient_shader = timed_shader(device, wgpu::ShaderModuleDescriptor {
        label: Some("gradient-shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{GRADIENT_SHADER_SRC}").into()),
    });
    let gradient_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gradient-layout"),
        bind_group_layouts: &[uniform_bgl, gradient_bgl],
        push_constant_ranges: &[],
    });
    timed_pipeline(device, &wgpu::RenderPipelineDescriptor {
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
    })
}

/// Пять пайплайнов, без которых не обходится почти ни одна страница
/// (`fill` / `rrect` / `text` / `image` / `gradient`). Остальные одиннадцать
/// компилируются лениво — см. [`PipelineDeps::build_all_lazy`] и BUG-406.
pub(crate) struct HotPipelines {
    /// Сплошная заливка прямоугольника.
    pub(crate) fill: wgpu::RenderPipeline,
    /// Скруглённый прямоугольник (SDF).
    pub(crate) rrect: wgpu::RenderPipeline,
    /// Квады глифов из атласа.
    pub(crate) text: wgpu::RenderPipeline,
    /// Текстурный квад картинки.
    pub(crate) image: wgpu::RenderPipeline,
    /// Градиентная заливка.
    pub(crate) gradient: wgpu::RenderPipeline,
    /// Какие РАЗНЫЕ потоки скомпилировали эти пять пайплайнов — гейт
    /// среза 2 BUG-406. Ставить его на wall-clock нельзя: разброс старта на
    /// этой машине доходит до 2.5× между прогонами (`docs/perf-method.md`),
    /// а «компиляции разъехались по потокам» проверяется точно и не зависит
    /// ни от железа, ни от загрузки машины. 5 — параллельный путь, 1 —
    /// `LUMEN_SERIAL_PIPELINES=1`.
    pub(crate) threads: HashSet<std::thread::ThreadId>,
}

/// `LUMEN_SERIAL_PIPELINES=1` — собирать горячие пайплайны по очереди на
/// вызывающем потоке (поведение до среза 2 BUG-406). Нужен для A/B в одном
/// бинарнике и как откат, если параллельная сборка где-то мешает драйверу.
pub(crate) fn hot_pipelines_serial() -> bool {
    std::env::var("LUMEN_SERIAL_PIPELINES").is_ok_and(|v| v == "1" || v == "true")
}

/// `LUMEN_WAIT_HOT_PIPELINES=1` — дождаться горячих пайплайнов прямо в
/// `init_pipelines` (поведение среза 2 BUG-406: параллельно, но конструктор
/// блокируется). Нужен для A/B среза 3 в одном бинарнике и как откат.
pub(crate) fn hot_pipelines_awaited_in_ctor() -> bool {
    std::env::var("LUMEN_WAIT_HOT_PIPELINES").is_ok_and(|v| v == "1" || v == "true")
}

/// Какой из пяти горячих пайплайнов имеется в виду (BUG-406 срез 3). Нужен
/// потому, что по `wgpu::RenderPipeline` отличить их друг от друга нельзя, а
/// приезжают они с фоновых потоков в произвольном порядке.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HotKind {
    /// → [`Renderer::fill_pipeline`].
    Fill,
    /// → [`Renderer::rrect_pipeline`].
    RRect,
    /// → [`Renderer::text_pipeline`].
    Text,
    /// → [`Renderer::image_pipeline`].
    Image,
    /// → [`Renderer::gradient_pipeline`].
    Gradient,
}

/// Все пять горячих видов в порядке запуска потоков.
pub(crate) const HOT_KINDS: [HotKind; 5] =
    [HotKind::Fill, HotKind::RRect, HotKind::Text, HotKind::Image, HotKind::Gradient];

/// Готовый горячий пайплайн вместе с видом и потоком-сборщиком.
pub(crate) type HotDelivery = (HotKind, std::thread::ThreadId, wgpu::RenderPipeline);

/// Входы сборки горячих пайплайнов — ровно те хэндлы, которых достаточно, и
/// ничего сверх. Все поля `Clone + Send` (в wgpu 26 хэндлы внутри `Arc`),
/// поэтому снимок уезжает на фоновый поток так же, как [`PipelineDeps`] у
/// ленивых.
#[derive(Clone)]
pub(crate) struct HotDeps {
    /// Устройство, на котором компилируются пайплайны.
    pub(crate) device: wgpu::Device,
    /// Формат цветового attachment-а.
    pub(crate) format: wgpu::TextureFormat,
    /// Layout viewport-униформы (bind group 0) — нужен всем пяти.
    pub(crate) uniform_bgl: wgpu::BindGroupLayout,
    /// Layout атласа глифов — нужен `text`.
    pub(crate) atlas_bgl: wgpu::BindGroupLayout,
    /// Layout сэмплируемой картинки — нужен `image`.
    pub(crate) image_bgl: wgpu::BindGroupLayout,
    /// Layout буфера стопов градиента — нужен `gradient`.
    pub(crate) gradient_bgl: wgpu::BindGroupLayout,
}

impl HotDeps {
    /// Компилирует один горячий пайплайн. Дескрипторы те же, что и до среза 3,
    /// — вид выбирает только, какой из пяти билдеров позвать.
    pub(crate) fn build(&self, kind: HotKind) -> wgpu::RenderPipeline {
        match kind {
            HotKind::Fill => build_fill_pipeline(&self.device, self.format, &self.uniform_bgl),
            HotKind::RRect => build_rrect_pipeline(&self.device, self.format, &self.uniform_bgl),
            HotKind::Text => {
                build_text_pipeline(&self.device, self.format, &self.uniform_bgl, &self.atlas_bgl)
            }
            HotKind::Image => {
                build_image_pipeline(&self.device, self.format, &self.uniform_bgl, &self.image_bgl)
            }
            HotKind::Gradient => build_gradient_pipeline(
                &self.device,
                self.format,
                &self.uniform_bgl,
                &self.gradient_bgl,
            ),
        }
    }
}

/// Запускает сборку пяти горячих пайплайнов на пяти отдельных потоках и
/// возвращает канал, в который каждый кладёт свой результат СРАЗУ по
/// готовности (BUG-406 срез 3).
///
/// Отличие от [`build_hot_pipelines`] — не в параллельности (она была и в
/// срезе 2), а в том, что вызывающий поток здесь никого не ждёт. На DX12/Intel
/// цена компиляции привязана к **вызывающему** потоку (`create_render_pipeline`
/// возвращается раньше, чем драйвер дособрал шейдер), поэтому конструктор
/// рендера переставал отвечать ровно на время сборки; теперь этого времени в
/// нём нет, а кадр ждёт только тот пайплайн, который ему действительно нужен,
/// и к тому моменту фон уже успел отработать сетевой/парсерный кусок старта.
///
/// Потоки отвязанные (не `scope`): они переживают выход из `init_pipelines` по
/// построению. Если приёмник умрёт раньше отправителя, `send` вернёт `Err` и
/// поток просто завершится — пайплайн будет собран заново кадром.
pub(crate) fn spawn_hot_pipelines(deps: &HotDeps) -> std::sync::mpsc::Receiver<HotDelivery> {
    let (tx, rx) = std::sync::mpsc::channel();
    for kind in HOT_KINDS {
        let deps = deps.clone();
        let tx = tx.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("lumen-hot-{kind:?}"))
            .spawn(move || {
                let pipeline = deps.build(kind);
                let _ = tx.send((kind, std::thread::current().id(), pipeline));
            });
        if spawned.is_err() {
            // Поток не стартовал — кадр соберёт этот пайплайн сам через
            // `HotDeps::build`; счётчик `hot_built_on_ui` это покажет.
            eprintln!("[wgpu] поток сборки горячего пайплайна {kind:?} не стартовал");
        }
    }
    rx
}

/// Пайплайн вместе с id потока, который его скомпилировал (см.
/// [`HotPipelines::threads`]).
type PipelineOnThread = (std::thread::ThreadId, wgpu::RenderPipeline);

/// Оборачивает сборщик так, чтобы он заодно сообщил свой поток.
fn on_this_thread(pipeline: wgpu::RenderPipeline) -> PipelineOnThread {
    (std::thread::current().id(), pipeline)
}

/// Забирает пайплайн у потока сборки. Паника внутри потока пробрасывается
/// дальше как есть: без пайплайна кадр всё равно не соберётся, а `unwrap`
/// в продакшне запрещён (`clippy::unwrap_used`).
fn join_pipeline(
    handle: std::thread::ScopedJoinHandle<'_, PipelineOnThread>,
) -> PipelineOnThread {
    match handle.join() {
        Ok(pipeline) => pipeline,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Собирает все пять горячих пайплайнов, по умолчанию — **параллельно**
/// (BUG-406, срез 2).
///
/// Причина: на DX12/Intel `create_render_pipeline` возвращается раньше, чем
/// драйвер дособрал шейдер, и остаток (~1.0–1.6 с на пайплайн) догоняет
/// **вызывающий** поток уже за пределами вызова — то же наблюдение, на котором
/// стоит фоновый прогрев ленивых пайплайнов
/// ([`Renderer::spawn_pipeline_warmup`]). Пять последовательных компиляций
/// поэтому складываются, а выданные с разных потоков — перекрываются. На
/// Vulkan разрыва нет, и параллельность там просто нейтральна.
///
/// Пиксельно нейтрально по построению: дескрипторы те же, меняется только
/// поток-создатель. `wgpu::Device` — `Send + Sync`, одновременное создание
/// пайплайнов на нём разрешено.
pub(crate) fn build_hot_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_bgl: &wgpu::BindGroupLayout,
    atlas_bgl: &wgpu::BindGroupLayout,
    image_bgl: &wgpu::BindGroupLayout,
    gradient_bgl: &wgpu::BindGroupLayout,
) -> HotPipelines {
    if hot_pipelines_serial() {
        return HotPipelines {
            fill: build_fill_pipeline(device, format, uniform_bgl),
            rrect: build_rrect_pipeline(device, format, uniform_bgl),
            text: build_text_pipeline(device, format, uniform_bgl, atlas_bgl),
            image: build_image_pipeline(device, format, uniform_bgl, image_bgl),
            gradient: build_gradient_pipeline(device, format, uniform_bgl, gradient_bgl),
            threads: HashSet::from([std::thread::current().id()]),
        };
    }
    // Четыре потока плюс вызывающий: пятый пайплайн строится здесь же — поток
    // под него пришлось бы всё равно дожидаться.
    std::thread::scope(|scope| {
        let rrect =
            scope.spawn(|| on_this_thread(build_rrect_pipeline(device, format, uniform_bgl)));
        let text = scope
            .spawn(|| on_this_thread(build_text_pipeline(device, format, uniform_bgl, atlas_bgl)));
        let image = scope
            .spawn(|| on_this_thread(build_image_pipeline(device, format, uniform_bgl, image_bgl)));
        let gradient = scope.spawn(|| {
            on_this_thread(build_gradient_pipeline(device, format, uniform_bgl, gradient_bgl))
        });
        let fill = on_this_thread(build_fill_pipeline(device, format, uniform_bgl));
        let built = [fill, join_pipeline(rrect), join_pipeline(text), join_pipeline(image),
            join_pipeline(gradient)];
        let threads: HashSet<std::thread::ThreadId> = built.iter().map(|(id, _)| *id).collect();
        let [fill, rrect, text, image, gradient] = built;
        HotPipelines {
            fill: fill.1,
            rrect: rrect.1,
            text: text.1,
            image: image.1,
            gradient: gradient.1,
            threads,
        }
    })
}


/// Неизменяемые wgpu-хэндлы, которых достаточно для сборки любого ленивого
/// пайплайна (BUG-406). Выделены из [`Renderer`] отдельной структурой ради
/// BUG-405: все поля здесь — `Clone + Send + Sync` (в wgpu 26 хэндлы внутри
/// `Arc`), поэтому снимок можно отдать фоновому потоку прогрева, а сам
/// `Renderer` (с `OnceCell`, `HashMap`-кэшами и `Surface`) остаётся
/// не-`Send`-овым и живёт только на UI-потоке.
///
/// Все поля выставляются один раз в конструкторе и больше не переприсваиваются
/// — снимок не может устареть. `surface_format` в том числе: пересоздание
/// swapchain'а в `resize`/`set_scale_factor` формат не меняет.
#[derive(Clone)]
pub(crate) struct PipelineDeps {
    /// Устройство, на котором компилируются пайплайны.
    pub(crate) device: wgpu::Device,
    /// Формат цветового attachment'а всех пайплайнов кадра.
    pub(crate) surface_format: wgpu::TextureFormat,
    /// Layout viewport-униформы (bind group 0).
    pub(crate) uniform_bgl: wgpu::BindGroupLayout,
    /// Layout сэмплируемой картинки (текстура + сэмплер).
    pub(crate) image_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса (склейка offscreen-уровня с родителем).
    pub(crate) composite_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса маски (`mask-image`).
    pub(crate) mask_composite_bgl: wgpu::BindGroupLayout,
    /// Layout пасса CSS-фильтров.
    pub(crate) filter_bgl: wgpu::BindGroupLayout,
    /// Layout пасса блюра (H/V разделяемое ядро).
    pub(crate) blur_bgl: wgpu::BindGroupLayout,
    /// Layout пасса «вертикальный блюр + фильтры + композит» (BUG-405 срез 6).
    pub(crate) blur_composite_bgl: wgpu::BindGroupLayout,
    /// Layout пасса blend-режимов (`mix-blend-mode`).
    pub(crate) blend_bgl: wgpu::BindGroupLayout,
    /// Layout пасса cross-fade (`image-set`/переходы картинок).
    pub(crate) cross_fade_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса клипа произвольной формы (`clip-path`).
    pub(crate) path_clip_bgl: wgpu::BindGroupLayout,
    /// Сколько ленивых пайплайнов этого рендера уже скомпилировано (BUG-405).
    /// Считается **на рендер**, а не на процесс: гейт правки — тест, а тесты
    /// одного бинарника идут параллельно и на общем счётчике мешали бы друг
    /// другу. `Arc` разделяется с клоном снимка, уехавшим на поток прогрева,
    /// поэтому фоновые компиляции тоже видны владельцу.
    pub(crate) built: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Готовый пайплайн, приехавший с потока прогрева (BUG-405). Вариант несёт
/// ровно ту `OnceCell`, в которую его нужно положить, — иначе принимающая
/// сторона не отличила бы один `RenderPipeline` от другого.
pub(crate) enum WarmedPipeline {
    /// → [`Renderer::circle_pipeline`].
    Circle(wgpu::RenderPipeline),
    /// → [`Renderer::mipgen_pipeline`].
    Mipgen(wgpu::RenderPipeline),
    /// → [`Renderer::cross_fade_pipeline`].
    CrossFade(wgpu::RenderPipeline),
    /// → [`Renderer::composite_pipeline`].
    Composite(wgpu::RenderPipeline),
    /// → [`Renderer::rrect_clip_pipeline`].
    RRectClip(wgpu::RenderPipeline),
    /// → [`Renderer::path_clip_pipeline`].
    PathClip(wgpu::RenderPipeline),
    /// → [`Renderer::blend_pipeline`].
    Blend(wgpu::RenderPipeline),
    /// → [`Renderer::mask_composite_pipeline`].
    MaskComposite(wgpu::RenderPipeline),
    /// → [`Renderer::mask_layer_pipelines`] (пара luminance/alpha).
    MaskLayer(Box<(wgpu::RenderPipeline, wgpu::RenderPipeline)>),
    /// → [`Renderer::filter_pipeline`].
    Filter(wgpu::RenderPipeline),
    /// → [`Renderer::blur_composite_pipeline`].
    BlurComposite(wgpu::RenderPipeline),
    /// → [`Renderer::blur_pipeline`].
    Blur(wgpu::RenderPipeline),
    /// → [`Renderer::shadow_pipeline`].
    Shadow(wgpu::RenderPipeline),
    /// → [`Renderer::backdrop_blit_pipeline`].
    BackdropBlit(wgpu::RenderPipeline),
}

impl PipelineDeps {
    /// Компилирует один ленивый пайплайн и учитывает его в счётчике рендера
    /// (BUG-405). Все `build_*_pipeline` ниже ходят только сюда, поэтому
    /// счётчик не может разойтись с реальным числом компиляций.
    fn timed(&self, desc: &wgpu::RenderPipelineDescriptor<'_>) -> wgpu::RenderPipeline {
        self.built.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        timed_pipeline(&self.device, desc)
    }

    /// Собирает **все** ленивые пайплайны (BUG-406) и отдаёт каждый в `emit`
    /// сразу по готовности; `emit` вернул `false` — приёмника больше нет, и
    /// дальше строить нечего.
    ///
    /// Единственное место, где перечислен набор прогрева: и фоновый поток
    /// ([`Renderer::spawn_pipeline_warmup`]), и синхронный вход для тестов
    /// ([`Renderer::warm_lazy_pipelines_blocking`]) ходят сюда, поэтому тест
    /// не может проверять список, отличный от продакшн-набора.
    ///
    /// Порядок — по измеренной цене: на прокрутке `lenta.ru` единственный
    /// кадр, компилировавший ленивые пайплайны, брал ровно `filter`+`blur` и
    /// стоил 823/1048 мс против 320/268 мс с прогревом (тот же кадр #8, два
    /// раунда A/B). Поэтому они первыми — прогрев обязан успеть закрыть именно
    /// их до первой прокрутки.
    pub(crate) fn build_all_lazy(&self, mut emit: impl FnMut(WarmedPipeline) -> bool) {
        macro_rules! emit {
            ($v:expr) => {
                if !emit($v) {
                    return;
                }
            };
        }
        emit!(WarmedPipeline::Filter(self.build_filter_pipeline()));
        emit!(WarmedPipeline::Blur(self.build_blur_pipeline()));
        emit!(WarmedPipeline::Shadow(self.build_shadow_pipeline()));
        emit!(WarmedPipeline::BlurComposite(self.build_blur_composite_pipeline()));
        emit!(WarmedPipeline::RRectClip(self.build_rrect_clip_pipeline()));
        emit!(WarmedPipeline::Composite(self.build_composite_pipeline()));
        emit!(WarmedPipeline::PathClip(self.build_path_clip_pipeline()));
        emit!(WarmedPipeline::Blend(self.build_blend_pipeline()));
        emit!(WarmedPipeline::MaskComposite(self.build_mask_composite_pipeline()));
        emit!(WarmedPipeline::MaskLayer(Box::new(self.build_mask_layer_pipeline())));
        emit!(WarmedPipeline::BackdropBlit(self.build_backdrop_blit_pipeline()));
        emit!(WarmedPipeline::Circle(self.build_circle_pipeline()));
        emit!(WarmedPipeline::Mipgen(self.build_mipgen_pipeline()));
        emit!(WarmedPipeline::CrossFade(self.build_cross_fade_pipeline()));
    }

    /// Пайплайн кружков (SDF): маркеры списков, radio-кнопки.
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_circle_pipeline(&self) -> wgpu::RenderPipeline {
        // ── Circle pipeline ───────────────────────────────────────────────
        let circle_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("circle-shader"),
            source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{CIRCLE_SHADER_SRC}").into()),
        });
        let circle_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("circle-layout"),
            bind_group_layouts: &[&self.uniform_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн генерации mip-цепочки картинок (даунскейл 2×2 box).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_mipgen_pipeline(&self) -> wgpu::RenderPipeline {
        // ── Mipgen pipeline (mip-цепочка картинок) ────────────────────────
        // Пасс «mip N−1 → mip N» без depth и без блендинга: fullscreen
        // triangle пишет bilinear-выборку источника (2×2 box-даунскейл).
        let mipgen_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("mipgen-shader"),
            source: wgpu::ShaderSource::Wgsl(MIPGEN_SHADER_SRC.into()),
        });
        let mipgen_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mipgen-layout"),
            bind_group_layouts: &[&self.image_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
        })
    }

    /// Пайплайн `cross-fade(A, B, p)` (CSS Images L4 §4).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_cross_fade_pipeline(&self) -> wgpu::RenderPipeline {
        let cross_fade_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("cross-fade-shader"),
            source: wgpu::ShaderSource::Wgsl(format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{CROSS_FADE_SHADER_SRC}").into()),
        });
        let cross_fade_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cross-fade-layout"),
            bind_group_layouts: &[&self.uniform_bgl, &self.cross_fade_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн композита offscreen-слоя в родителя (opacity/clip-группы).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_composite_pipeline(&self) -> wgpu::RenderPipeline {
        let composite_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("composite-shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER_SRC.into()),
        });
        let composite_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite-layout"),
            bind_group_layouts: &[&self.composite_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн композита уровня в родителя ЧЕРЕЗ скруглённый контур
    /// (CSS Overflow L3 §2: `overflow: hidden` на боксе с `border-radius`).
    ///
    /// Отличия от `build_composite_pipeline`: свой шейдер с `sdf_rrect` и
    /// вершинный layout `RRectClipVertex` (7 атрибутов вместо 3). Bind group
    /// layout общий — `composite_bgl`, поэтому композитить можно готовым
    /// `OffscreenLayer::bind_group`.
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// страница без скруглённого `overflow` за него не платит.
    fn build_rrect_clip_pipeline(&self) -> wgpu::RenderPipeline {
        let shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("rrect-clip-shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{SDF_RRECT_WGSL}{RRECT_CLIP_SHADER_SRC}").into(),
            ),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rrect-clip-layout"),
            bind_group_layouts: &[&self.composite_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
            label: Some("rrect-clip-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<RRectClipVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0,  shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8,  shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 16, shader_location: 2 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 24, shader_location: 3 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 32, shader_location: 4 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 40, shader_location: 5 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 56, shader_location: 6 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    // Как у composite: содержимое уровня премультиплировано,
                    // шейдер домножает rgb и a на одно покрытие.
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
        })
    }

    /// CSS Masking L1 §3 — пайплайн композита формы `clip-path`.
    /// Тот же контракт, что у `build_rrect_clip_pipeline`, но контур приходит
    /// не per-vertex, а uniform-ом: у полигона переменное число вершин.
    fn build_path_clip_pipeline(&self) -> wgpu::RenderPipeline {
        let shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("path-clip-shader"),
            source: wgpu::ShaderSource::Wgsl(PATH_CLIP_SHADER_SRC.into()),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("path-clip-layout"),
            bind_group_layouts: &[&self.path_clip_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
            label: Some("path-clip-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PathClipVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0,  shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8,  shader_location: 1 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 16, shader_location: 2 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    // Как у composite: содержимое уровня премультиплировано,
                    // шейдер домножает rgb и a на одно покрытие.
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
        })
    }

    /// Пайплайн CSS-блендинга двух текстур (CSS Compositing L1 §8).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_blend_pipeline(&self) -> wgpu::RenderPipeline {
        let blend_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("blend-shader"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER_SRC.into()),
        });
        let blend_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blend-layout"),
            bind_group_layouts: &[&self.blend_bgl],
            push_constant_ranges: &[],
        });
        // REPLACE blend state: shader implements full CSS compositing formula.
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн композита слоя по маске-картинке (CSS Masking L1 §4).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_mask_composite_pipeline(&self) -> wgpu::RenderPipeline {
        let mask_composite_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask-composite-layout"),
            bind_group_layouts: &[&self.uniform_bgl, &self.mask_composite_bgl],
            push_constant_ranges: &[],
        });
        let mask_composite_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("mask-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(MASK_COMPOSITE_SHADER_SRC.into()),
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пара пайплайнов композита по отрисованному mask-слою (CSS Masking L1 §5), alpha и luminance.
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_mask_layer_pipeline(&self) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
        // ── Mask-layer composite pipelines ──────────────────────────────────
        // CSS Masking L1 §5: apply a rendered mask layer to the parent layer.
        // Reuses mask_composite_bgl (same binding layout: t_content, t_mask, s).
        // Two pipelines sharing one shader module: alpha mode and luminance mode.
        // Blend: REPLACE (src_factor=One, dst_factor=Zero) — overwrites parent at element rect.
        // Свой `PipelineLayout` поверх общего `mask_composite_bgl`: билдер
        // `mask_composite`-пайплайна тоже ленив (BUG-406), его локальный layout
        // сюда не дотягивается.
        let mask_composite_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask-layer-layout"),
            bind_group_layouts: &[&self.uniform_bgl, &self.mask_composite_bgl],
            push_constant_ranges: &[],
        });
        let mask_layer_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
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
        let mask_layer_alpha_pipeline = self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        let mask_layer_luma_pipeline = self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        (mask_layer_alpha_pipeline, mask_layer_luma_pipeline)
    }

    /// Пайплайн цветовых CSS-фильтров (CSS Filter Effects L1).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_filter_pipeline(&self) -> wgpu::RenderPipeline {
        let filter_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("filter-shader"),
            source: wgpu::ShaderSource::Wgsl(filter_shader_src().into()),
        });
        let filter_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("filter-layout"),
            bind_group_layouts: &[&self.filter_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
                    // Источник — offscreen-слой, его содержимое премультиплировано
                    // (та же конвенция, что у `composite_pipeline`), и `fs_main`
                    // возвращает премультиплированный результат. Straight-alpha
                    // `ALPHA_BLENDING` домножал бы rgb на alpha второй раз.
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
        })
    }

    /// Пайплайн «вертикальный проход блюра + цветовые фильтры + композит»
    /// (BUG-405 срез 6). Отличия от [`Self::build_filter_pipeline`] — свой
    /// BGL (четвёртый слот под `BlurParams`) и своя фрагментная часть;
    /// blend тот же премультиплированный, цель — родительский уровень.
    fn build_blur_composite_pipeline(&self) -> wgpu::RenderPipeline {
        let shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("blur-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(blur_composite_shader_src().into()),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur-composite-layout"),
            bind_group_layouts: &[&self.blur_composite_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
            label: Some("blur-composite-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн аналитической размытой тени (BUG-405 срез 7).
    ///
    /// Отличия от `rrect_pipeline`, с которого он списан: своя фрагментная
    /// часть ([`SHADOW_SHADER_SRC`]) и лишний вершинный атрибут `sigma`.
    /// Группа 0 та же — viewport + слот скруглённого клипа, поэтому тень
    /// рисуется прямо в батче родителя и своего пасса не открывает.
    ///
    /// BUG-406: компилируется лениво, при первом использовании.
    fn build_shadow_pipeline(&self) -> wgpu::RenderPipeline {
        let shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("shadow-shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{SDF_RRECT_WGSL}{CLIP_UNIFORM_WGSL}{SHADOW_SHADER_SRC}").into(),
            ),
        });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow-layout"),
            bind_group_layouts: &[&self.uniform_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ShadowVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // loc 0: pos (vec2)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        // loc 1: z (f32)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 8, shader_location: 1 },
                        // loc 2: color (vec4)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 12, shader_location: 2 },
                        // loc 3: center (vec2)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 28, shader_location: 3 },
                        // loc 4: half_size (vec2)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 36, shader_location: 4 },
                        // loc 5: radii_x (vec4)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 44, shader_location: 5 },
                        // loc 6: radii_y (vec4)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 60, shader_location: 6 },
                        // loc 7: sigma (f32)
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 76, shader_location: 7 },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
                    // Тот же blend, что у `rrect_pipeline`: прямая альфа —
                    // прежний путь композитил уровень премультиплицированно,
                    // но там альфа уже была вмножена в цвет самой заливкой.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
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
        })
    }

    /// Пайплайн сепарабельного гауссова блюра (один проход, H или V).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_blur_pipeline(&self) -> wgpu::RenderPipeline {
        let blur_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("blur-shader"),
            source: wgpu::ShaderSource::Wgsl(BLUR_SHADER_SRC.into()),
        });
        let blur_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur-layout"),
            bind_group_layouts: &[&self.blur_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }

    /// Пайплайн блита отфильтрованного backdrop-снимка (REPLACE-блендинг).
    ///
    /// BUG-406: компилируется лениво, при первом реальном использовании —
    /// на DX12 компиляция одного пайплайна стоит ~1 с, и держать её в
    /// старте окна ради эффекта, которого на странице может не быть, нельзя.
    fn build_backdrop_blit_pipeline(&self) -> wgpu::RenderPipeline {
        // ── Backdrop-filter blit pipeline ────────────────────────────────────
        // Same shader + bind group layout as filter_pipeline, but REPLACE blend.
        // Used to overwrite the parent layer's element-bounds region with the
        // filtered backdrop snapshot (with optional color-matrix filter applied).
        // Собственные shader/layout: `filter_pipeline` тоже ленив (BUG-406), и его
        // локальные shader/layout не переживают своего билдера.
        let filter_shader = timed_shader(&self.device, wgpu::ShaderModuleDescriptor {
            label: Some("filter-shader"),
            source: wgpu::ShaderSource::Wgsl(filter_shader_src().into()),
        });
        let filter_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("filter-layout"),
            bind_group_layouts: &[&self.filter_bgl],
            push_constant_ranges: &[],
        });
        self.timed(&wgpu::RenderPipelineDescriptor {
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
                    format: self.surface_format,
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
        })
    }
}

impl Renderer {
    /// Ленивый доступ к `circle`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn circle_pipeline(&self) -> &wgpu::RenderPipeline {
        self.circle_pipeline.get_or_init(|| self.pdeps.build_circle_pipeline())
    }
    /// Ленивый доступ к `mipgen`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn mipgen_pipeline(&self) -> &wgpu::RenderPipeline {
        self.mipgen_pipeline.get_or_init(|| self.pdeps.build_mipgen_pipeline())
    }
    /// Ленивый доступ к `cross_fade`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn cross_fade_pipeline(&self) -> &wgpu::RenderPipeline {
        self.cross_fade_pipeline.get_or_init(|| self.pdeps.build_cross_fade_pipeline())
    }
    /// Ленивый доступ к `composite`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.composite_pipeline.get_or_init(|| self.pdeps.build_composite_pipeline())
    }
    /// Ленивый доступ к пайплайну скруглённого клипа (BUG-406, BUG-277 срез 5).
    pub(crate) fn rrect_clip_pipeline(&self) -> &wgpu::RenderPipeline {
        self.rrect_clip_pipeline.get_or_init(|| self.pdeps.build_rrect_clip_pipeline())
    }

    /// Ленивый доступ к пайплайну композита формы `clip-path` (BUG-406).
    pub(crate) fn path_clip_pipeline(&self) -> &wgpu::RenderPipeline {
        self.path_clip_pipeline.get_or_init(|| self.pdeps.build_path_clip_pipeline())
    }
    /// Ленивый доступ к `blend`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn blend_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blend_pipeline.get_or_init(|| self.pdeps.build_blend_pipeline())
    }
    /// Ленивый доступ к `mask_composite`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn mask_composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.mask_composite_pipeline.get_or_init(|| self.pdeps.build_mask_composite_pipeline())
    }
    /// Ленивый доступ к `filter`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn filter_pipeline(&self) -> &wgpu::RenderPipeline {
        self.filter_pipeline.get_or_init(|| self.pdeps.build_filter_pipeline())
    }
    /// Ленивый доступ к `blur`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn blur_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blur_pipeline.get_or_init(|| self.pdeps.build_blur_pipeline())
    }
    /// Ленивый доступ к `blur-composite`-пайплайну (BUG-405 срез 6): вертикальный
    /// проход блюра вместе с цветовыми фильтрами и композитом в родителя.
    pub(crate) fn blur_composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blur_composite_pipeline.get_or_init(|| self.pdeps.build_blur_composite_pipeline())
    }
    /// Ленивый доступ к пайплайну аналитической тени (BUG-405 срез 7).
    pub(crate) fn shadow_pipeline(&self) -> &wgpu::RenderPipeline {
        self.shadow_pipeline.get_or_init(|| self.pdeps.build_shadow_pipeline())
    }
    /// Ленивый доступ к `backdrop_blit`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    pub(crate) fn backdrop_blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.backdrop_blit_pipeline.get_or_init(|| self.pdeps.build_backdrop_blit_pipeline())
    }
    /// Ленивый доступ к паре mask-layer-пайплайнов (alpha, luminance) — BUG-406.
    pub(crate) fn mask_layer_pipelines(&self) -> &(wgpu::RenderPipeline, wgpu::RenderPipeline) {
        self.mask_layer_pipelines.get_or_init(|| self.pdeps.build_mask_layer_pipeline())
    }
}
