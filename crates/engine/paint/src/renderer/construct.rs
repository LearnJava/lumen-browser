//! P1/SPLIT-RN8: конструкторы `Renderer` (`new`/`new_headless` + их
//! async-варианты), `init_pipelines`, фоновый прогрев ленивых пайплайнов
//! (`Hot*`) и ленивые аксессоры пайплайнов (fill/rrect/text/image/
//! gradient) — `impl Renderer` #1 из `renderer.rs` (4000…5287 до
//! вырезки). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-8).

use super::*;

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
        // Windows order is Vulkan-first (2026-07-28, user decision): pipeline
        // compilation on this Intel Iris Plus costs ~0.28 s on Vulkan against
        // 3–7 s on DX12 for the exact same 16 pipelines (measured under
        // `LUMEN_FRAME_LOG=1`, see `bugs/BUG-274-OPEN.md` and BUG-406) — that
        // gap is the bulk of the "window says Not Responding on launch"
        // report. It matches `backend_probe::pick_backend`'s own candidate
        // order (Vulkan → GL → DX12), so the two no longer disagree.
        //
        // This chain is only consulted when the probe does *not* decide: the
        // probe's accepted candidate is prepended below, so on a normal
        // Windows launch the probe still wins. It governs when the probe is
        // switched off (`LUMEN_NO_BACKEND_PROBE=1`) or reports `None`. In
        // that first case the BUG-275 white-window risk is no longer screened
        // by a real presentation check — the probe exists precisely because
        // some Intel iGPUs present a blank Vulkan swapchain — so a machine
        // hitting BUG-275 *and* disabling the probe now needs an explicit
        // `WGPU_BACKEND=dx12`.
        let static_prefs: &[wgpu::Backends] = if cfg!(target_os = "windows") {
            &[wgpu::Backends::VULKAN, wgpu::Backends::DX12, wgpu::Backends::GL]
        } else {
            &[wgpu::Backends::PRIMARY, wgpu::Backends::GL]
        };
        let backend_prefs: Vec<wgpu::Backends> = probed
            .into_iter()
            .chain(static_prefs.iter().copied().filter(|b| Some(*b) != probed))
            .collect();
        // BUG-274 cold-start census: bracket adapter/device acquisition and
        // pipeline compilation separately from the probe (already logged by
        // `backend_probe::pick_backend`) to find where the ~9s launch->first-frame
        // gap actually goes.
        let t_adapter0 = std::time::Instant::now();
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
        // BUG-405 срез 23: всё, кроме стороны текстуры, остаётся на
        // `downlevel_defaults()` (переносимость), а сторона поднимается до
        // тира адаптера — от неё зависит, работает ли скролл-композитор:
        // полоса высотой 2.5 вьюпорта не влезала в 2048 уже на окне
        // клиентской высотой ~819 device px.
        let mut limits = wgpu::Limits::downlevel_defaults();
        let adapter_max_dim = adapter.limits().max_texture_dimension_2d;
        limits.max_texture_dimension_2d =
            requested_max_texture_dim(adapter_max_dim, !texture_limit_raise_disabled());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lumen-device"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
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
        // BUG-277 (срез 3): `mix-blend-mode` на боксе без offscreen-предка
        // композитится прямо в swapchain-поверхность (`from_level == 1`), а
        // blend-шейдеру нужен ЧИТАЕМЫЙ backdrop. Сэмплировать поверхность
        // нельзя (`TEXTURE_BINDING` у неё не запросить), но её можно
        // скопировать в scratch-текстуру — для этого нужен `COPY_SRC`.
        // Драйверы, не отдающие `COPY_SRC` на поверхность, остаются на
        // старом alpha-over fallback (см. `RenderPlanItem::Composite`).
        let surface_usage = if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
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
            // BUG-405 срез 23: от стороны текстуры зависит, работает ли
            // скролл-композитор на этом окне, поэтому запрошенное значение
            // и потолок адаптера видны в том же логе, что и сам адаптер.
            eprintln!(
                "[wgpu] max_texture_dimension_2d: {} (адаптер {}, downlevel {})",
                device.limits().max_texture_dimension_2d,
                adapter_max_dim,
                wgpu::Limits::downlevel_defaults().max_texture_dimension_2d,
            );
            eprintln!(
                "[wgpu] surface: format {:?} (of {:?}) alpha {:?} (of {:?}) present {:?}",
                config.format, caps.formats, config.alpha_mode, caps.alpha_modes,
                config.present_mode,
            );
        }
        let gpu_fingerprint = GpuFingerprint::from_adapter_info(&adapter_info);
        if crate::frame_log_enabled() {
            eprintln!(
                "[wgpu] adapter+device acquired: {:.0}ms",
                t_adapter0.elapsed().as_secs_f64() * 1000.0
            );
        }

        let t_pipelines0 = std::time::Instant::now();
        let result = Self::init_pipelines(
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
        );
        if crate::frame_log_enabled() {
            eprintln!(
                "[wgpu] init_pipelines: {:.0}ms",
                t_pipelines0.elapsed().as_secs_f64() * 1000.0
            );
        }
        result
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
        // Headless keeps DX12 first — deliberately **not** the windowed chain's
        // Vulkan-first order (2026-07-28). Callers here (tests, `--screenshot`,
        // driver snapshots) are pixel-comparison paths that need the same
        // adapter run to run, and there is no window to verify presentation
        // against, so the probe cannot screen BUG-275. Startup pipeline-compile
        // latency — the reason the windowed chain flipped (BUG-406) — does not
        // matter for a one-shot headless render, whereas silently changing which
        // GPU API rasterizes the reference images would. `WGPU_BACKEND` still
        // overrides.
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
        // Лимит стороны здесь НЕ поднимается (в отличие от живого устройства,
        // BUG-405 срез 23): скролл-композитора в headless нет вовсе
        // (`try_page_compose` выходит по «нет surface»), а эталонные снимки
        // тем самым не начинают зависеть от тира адаптера машины — какие
        // картинки примет `register_image`, остаётся одинаковым везде.
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

    /// Общий инициализатор GPU-ресурсов: bind group layouts, atlas, samplers,
    /// буферы и **горячие** пайплайны. Вызывается как из windowed (`new_async`),
    /// так и из headless (`new_headless_async`) путей.
    ///
    /// BUG-406: сразу компилируются только пять пайплайнов, нужные почти любой
    /// странице (fill / rrect / text / image / gradient). Остальные одиннадцать
    /// (circle, mipgen, cross-fade, composite, blend, mask-composite, две
    /// mask-layer, filter, blur, backdrop-blit) компилируются лениво, при первом
    /// использовании — на DX12 компиляция одного пайплайна стоит ~1 с wall-clock,
    /// и страница без соответствующего эффекта не должна за неё платить.
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
        let t_init = std::time::Instant::now();
        // BUG-453: единственная точка создания `Device` для обоих
        // конструкторов (windowed `new_async` и headless `new_headless_async`
        // сходятся сюда) — регистрируем коллбэк потери устройства здесь,
        // один раз. `wgpu::SurfaceTexture::present()` возвращает `()` и
        // паникует изнутри библиотеки при потерянном устройстве без единого
        // способа поймать это исключение из `render_impl`; единственный
        // корректный вариант — не доводить до вызова, реагируя на потерю
        // заранее (флаг проверяется на входе в `render_impl`/`resize`).
        let device_lost: Arc<std::sync::OnceLock<String>> = Arc::new(std::sync::OnceLock::new());
        {
            let cell = device_lost.clone();
            device.set_device_lost_callback(move |reason, message| {
                eprintln!("[wgpu] device lost ({reason:?}): {message}");
                // `Device::set_device_lost_callback` в wgpu 26 фиксирует
                // callback единожды на весь срок жизни `Device`, поэтому
                // повторного вызова после первой потери не бывает — `set`
                // на второй попытке (если он всё же случится) молча
                // отбрасывается, а не паникует.
                let _ = cell.set(format!("{reason:?}: {message}"));
            });
        }
        /// Печатает время от входа в `init_pipelines` до контрольной точки
        /// (только под `LUMEN_FRAME_LOG`).
        fn mark(t0: &std::time::Instant, label: &str) {
            if crate::frame_log_enabled() {
                eprintln!("[wgpu]   @{label}: {:.0}ms", t0.elapsed().as_secs_f64() * 1000.0);
            }
        }

        // ── Uniform bind group (viewport + скруглённый клип) ───────────────
        // BUG-405 срез 4: буфер стал МАССИВОМ слотов с динамическим офсетом —
        // слот 0 хранит «клипа нет», остальные заводит кадр под каждый
        // активный `PushClipRoundedRect`. Видимость расширена до фрагментного
        // этапа: покрытие контура считает именно он.
        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ClipUniformSlot>() as u64,
                    ),
                },
                count: None,
            }],
        });
        let uniform_slots = 64usize;
        let (uniform_buffer, uniform_bind_group) =
            Self::create_uniform_buffer(&device, &uniform_bgl, uniform_slots);

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

        mark(&t_init, "pre-pipelines");
        // ── BGL горячих пайплайнов ────────────────────────────────────────
        // Оба подняты сюда из своих бывших блоков (image / gradient): сборка
        // пайплайнов идёт одним параллельным вызовом ниже, и все её входы
        // должны существовать до него (BUG-406, срез 2).
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
        let gradient_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gradient-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    // Read-only storage, не uniform: список стопов имеет
                    // произвольную длину (`array<GradStop>`), а uniform-массив
                    // требует фиксированного размера и молча терял хвост —
                    // BUG-277 срез 11.
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // ── Горячие пайплайны (BUG-406) ───────────────────────────────────
        let hot_deps = HotDeps {
            device: device.clone(),
            format,
            uniform_bgl: uniform_bgl.clone(),
            atlas_bgl: atlas_bgl.clone(),
            image_bgl: image_bgl.clone(),
            gradient_bgl: gradient_bgl.clone(),
        };
        // Срез 3: по умолчанию конструктор НЕ ждёт компиляции — пять потоков
        // стартуют и кладут результат в канал, а `init_pipelines` идёт дальше.
        // Ждать остаётся только под двумя рычагами отката. Headless идёт тем же
        // путём намеренно: кадр всё равно упирается в `await_all_hot_pipelines`
        // на входе в `render`, зато путей остаётся один, а гейт среза
        // проверяется тестом без окна.
        let wait_in_ctor = hot_pipelines_serial() || hot_pipelines_awaited_in_ctor();
        let fill_pipeline = OnceCell::new();
        let rrect_pipeline = OnceCell::new();
        let text_pipeline = OnceCell::new();
        let image_pipeline = OnceCell::new();
        let gradient_pipeline = OnceCell::new();
        let hot_pipeline_threads: HashSet<std::thread::ThreadId>;
        let hot_rx;
        if wait_in_ctor {
            let HotPipelines { fill, rrect, text, image, gradient, threads } =
                build_hot_pipelines(
                    &device,
                    format,
                    &uniform_bgl,
                    &atlas_bgl,
                    &image_bgl,
                    &gradient_bgl,
                );
            drop(fill_pipeline.set(fill));
            drop(rrect_pipeline.set(rrect));
            drop(text_pipeline.set(text));
            drop(image_pipeline.set(image));
            drop(gradient_pipeline.set(gradient));
            hot_pipeline_threads = threads;
            hot_rx = None;
        } else {
            hot_pipeline_threads = HashSet::new();
            hot_rx = Some(spawn_hot_pipelines(&hot_deps));
        }
        mark(&t_init, "hot-pipelines");

        // ── Сэмплеры картинок ─────────────────────────────────────────────
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
        // Sampler блита полосы: как линейный выше, но по V — `Repeat`
        // (BUG-405 срез 32). Полоса адресуется кольцом, поэтому её строка
        // `H−1` документно соседствует со строкой `0`: при дробном сдвиге
        // блита фильтрации на шве нужны обе, а `ClampToEdge` подсунула бы
        // край. При нулевой фазе кольца (полоса только что перерисована
        // целиком) uv остаются в `0…1` и режим ни на что не влияет.
        let band_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("page-band-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::Repeat,
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

        // ── Cross-fade BGL (CSS Images L4 §4; пайплайн ленив, BUG-406) ────
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

        // ── Composite BGL + layer sampler (пайплайн ленив, BUG-406) ───────
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
        // ── Path-clip BGL (CSS Masking L1 §3; пайплайн ленив, BUG-406) ────
        // Как composite_bgl, плюс uniform с формой клипа (binding 2).
        let path_clip_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path-clip-bgl"),
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

        // ── Blend BGL (CSS Compositing L1 §8; пайплайн ленив, BUG-406) ─────
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

        // ── Mask composite BGL (пайплайн ленив, BUG-406) ─────────────────────
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


        // ── CSS Filter BGL (пайплайн ленив, BUG-406) ─────────────────────────
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

        // ── Blur-composite BGL (BUG-405 срез 6, пайплайн ленив) ──────────────
        // Group 0: { t_src, s_src, BlurParams uniform, FilterParams uniform }
        let blur_composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur-composite-bgl"),
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

        // ── CSS Blur BGL + uniform (пайплайн ленив, BUG-406) ─────────────────
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

        mark(&t_init, "pipelines-done");
        let atlas = GlyphAtlas::new(ATLAS_DIM);
        mark(&t_init, "glyph-atlas");

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

        mark(&t_init, "faces");
        let (depth_texture, depth_view) = {
            let (t, v) = create_depth_texture(&device, headless_w, headless_h);
            (Some(t), Some(v))
        };

        mark(&t_init, "depth-texture");
        // BUG-405: снимок хэндлов для сборки ленивых пайплайнов. Клонируется
        // ДО переезда полей в структуру — wgpu-хэндлы клонируются по `Arc`,
        // так что это не копия ресурсов, а вторая ссылка на те же объекты.
        let pdeps = PipelineDeps {
            device: device.clone(),
            surface_format: format,
            uniform_bgl,
            image_bgl: image_bgl.clone(),
            composite_bgl: composite_bgl.clone(),
            mask_composite_bgl: mask_composite_bgl.clone(),
            filter_bgl: filter_bgl.clone(),
            blur_bgl: blur_bgl.clone(),
            blur_composite_bgl: blur_composite_bgl.clone(),
            blend_bgl: blend_bgl.clone(),
            cross_fade_bgl: cross_fade_bgl.clone(),
            path_clip_bgl: path_clip_bgl.clone(),
            built: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let renderer = Self {
            pdeps,
            submissions: 0,
            rrect_clip_levels: 0,
            cull_merges: 0,
            plan_passes: 0,
            filter_passes: 0,
            blur_merge_enabled: true,
            cull_merge_enabled: true,
            shadow_draws: 0,
            state_elisions: 0,
            draw_merges: 0,
            state_elision_enabled: true,
            shadow_analytic_enabled: true,
            atlas_bytes_uploaded: 0,
            atlas_uploads: 0,
            atlas_partial_upload_enabled: true,
            nested_shader_clips: 0,
            nested_shader_clip_enabled: true,
            coverage_cache: CoverageCache::default(),
            coverage_cache_enabled: true,
            svg_shape_cache: SvgShapeCache::default(),
            svg_shape_cache_enabled: true,
            text_run_cache: TextRunCache::default(),
            text_run_cache_enabled: true,
            warm_rx: None,
            warm_started: false,
            surface,
            device,
            queue,
            device_lost,
            config,
            headless_w,
            headless_h,
            scale_factor,
            depth_texture,
            depth_view,
            fill_pipeline,
            circle_pipeline: OnceCell::new(),
            rrect_pipeline,
            text_pipeline,
            image_pipeline,
            mipgen_pipeline: OnceCell::new(),
            cross_fade_pipeline: OnceCell::new(),
            cross_fade_bgl,
            uniform_buffer,
            uniform_bind_group,
            uniform_slots,
            atlas_texture,
            atlas_bind_group,
            atlas_resets: 0,
            image_bgl,
            image_sampler,
            image_sampler_nearest,
            band_sampler,
            raw_images: HashMap::new(),
            images: HashMap::new(),
            layer_snapshots: HashMap::new(),
            content_generation: 0,
            page_offset: (0.0, 0.0),
            last_frame_hash: None,
            page_band: None,
            last_compose_skip: None,
            pending_base_blit: None,
            overlay_cache: None,
            pending_overlay_blit: None,
            last_overlay_digests: Vec::new(),
            last_content_key: None,
            content_epoch: 0,
            content_fold_memo: None,
            layer_cache: crate::layer_cache::LayerCache::new(),
            composite_pipeline: OnceCell::new(),
            rrect_clip_pipeline: OnceCell::new(),
            path_clip_pipeline: OnceCell::new(),
            path_clip_bgl,
            composite_bgl,
            blend_pipeline: OnceCell::new(),
            blend_bgl,
            mask_composite_bgl,
            mask_composite_pipeline: OnceCell::new(),
            mask_layer_pipelines: OnceCell::new(),
            filter_bgl,
            filter_pipeline: OnceCell::new(),
            blur_bgl,
            blur_pipeline: OnceCell::new(),
            blur_composite_bgl,
            blur_composite_pipeline: OnceCell::new(),
            shadow_pipeline: OnceCell::new(),
            backdrop_blit_pipeline: OnceCell::new(),
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
            hot_pipeline_threads: RefCell::new(hot_pipeline_threads),
            hot_rx: RefCell::new(hot_rx),
            hot_deps,
            hot_built_on_ui: std::cell::Cell::new(0),
        };
        // BUG-406: `LUMEN_EAGER_PIPELINES=1` возвращает доленивое поведение —
        // все 16 пайплайнов компилируются в `init_pipelines`. Нужен для A/B в
        // одном бинарнике и как откат, если ленивая компиляция где-то мешает.
        if std::env::var("LUMEN_EAGER_PIPELINES").is_ok_and(|v| v == "1" || v == "true") {
            renderer.await_all_hot_pipelines();
            renderer.warm_lazy_pipelines_blocking();
            mark(&t_init, "eager-warm");
        }
        Ok(renderer)
    }

    /// BUG-405: запустить фоновую компиляцию ленивых пайплайнов (BUG-406).
    ///
    /// Вызывается один раз, **после** показа первого кадра окна: сдвигать
    /// компиляцию в старт нельзя (ровно это BUG-406 и убрал — `first non-empty
    /// frame` 6357 → 2980 мс на DX12), а оставлять её на первом использовании
    /// значит платить ~0.8 с посреди прокрутки, когда в кадр въезжает первый
    /// элемент с фильтром.
    ///
    /// Стоимость уходит с UI-потока целиком, а не сдвигается по времени:
    /// замеренный на DX12/Intel штраф привязан к **вызывающему** потоку
    /// (`create_render_pipeline` возвращается рано, драйвер доедает компиляцию
    /// после возврата — BUG-406), поэтому вызов из отдельного потока и есть
    /// правка. Headless-путь (без `surface`) прогрев не запускает: там нет
    /// интерактивности, ради которой стоило бы жечь второе ядро.
    pub(crate) fn spawn_pipeline_warmup(&mut self) {
        if self.warm_started || self.surface.is_none() || pipeline_warmup_disabled() {
            return;
        }
        self.warm_started = true;
        let d = self.pdeps.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("lumen-pipeline-warm".to_string())
            .spawn(move || d.build_all_lazy(|p| tx.send(p).is_ok()))
            .is_ok();
        if spawned {
            self.warm_rx = Some(rx);
        }
    }

    /// BUG-405: разложить приехавшие с потока прогрева пайплайны по их
    /// `OnceCell`-ам. `try_recv` не блокирует — кадр никогда не ждёт
    /// компиляции, он лишь перестаёт платить за неё, когда та готова.
    ///
    /// `set` может вернуть `Err`: кадр успел скомпилировать пайплайн сам, пока
    /// поток его строил. Дубликат тогда просто выбрасывается — оба объекта
    /// валидны, а занят уже один.
    pub(crate) fn drain_warmed_pipelines(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = self.warm_rx.take() else { return };
        let mut alive = true;
        loop {
            match rx.try_recv() {
                Ok(p) => self.install_warmed_pipeline(p),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    alive = false;
                    break;
                }
            }
        }
        if alive {
            self.warm_rx = Some(rx);
        }
    }

    /// Кладёт один прогретый пайплайн в его ячейку (см.
    /// [`Self::drain_warmed_pipelines`]).
    fn install_warmed_pipeline(&self, p: WarmedPipeline) {
        match p {
            WarmedPipeline::Circle(x) => drop(self.circle_pipeline.set(x)),
            WarmedPipeline::Mipgen(x) => drop(self.mipgen_pipeline.set(x)),
            WarmedPipeline::CrossFade(x) => drop(self.cross_fade_pipeline.set(x)),
            WarmedPipeline::Composite(x) => drop(self.composite_pipeline.set(x)),
            WarmedPipeline::RRectClip(x) => drop(self.rrect_clip_pipeline.set(x)),
            WarmedPipeline::PathClip(x) => drop(self.path_clip_pipeline.set(x)),
            WarmedPipeline::Blend(x) => drop(self.blend_pipeline.set(x)),
            WarmedPipeline::MaskComposite(x) => drop(self.mask_composite_pipeline.set(x)),
            WarmedPipeline::MaskLayer(x) => drop(self.mask_layer_pipelines.set(*x)),
            WarmedPipeline::Filter(x) => drop(self.filter_pipeline.set(x)),
            WarmedPipeline::Blur(x) => drop(self.blur_pipeline.set(x)),
            WarmedPipeline::Shadow(x) => drop(self.shadow_pipeline.set(x)),
            WarmedPipeline::BlurComposite(x) => drop(self.blur_composite_pipeline.set(x)),
            WarmedPipeline::BackdropBlit(x) => drop(self.backdrop_blit_pipeline.set(x)),
        }
    }

    /// Прогревает ленивые пайплайны синхронно, на вызывающем потоке
    /// (BUG-405) — тем же списком, что и фоновый прогрев.
    ///
    /// Нужен там, где фонового потока нет по построению: форс-режим
    /// `LUMEN_EAGER_PIPELINES=1` (откат к доленивому поведению BUG-406) и
    /// тесты, которым нужен детерминированный момент готовности. Прежний
    /// список форс-режима был отдельным и успел разойтись с настоящим —
    /// не хватало `rrect_clip`/`path_clip`, поэтому «доленивое поведение»
    /// уже не было доленивым; здесь список ровно один.
    pub fn warm_lazy_pipelines_blocking(&self) {
        let deps = self.pdeps.clone();
        deps.build_all_lazy(|p| {
            self.install_warmed_pipeline(p);
            true
        });
    }

    /// Сколько ленивых пайплайнов **этот** рендер скомпилировал за свою жизнь
    /// (BUG-405), считая прогретые фоновым потоком.
    ///
    /// Гейт перф-правки стоит на нём, а не на времени кадра: «компиляция ушла
    /// с кадра» — это «за время кадра счётчик не вырос», и такое утверждение
    /// не зависит ни от железа, ни от нагрузки машины.
    #[must_use]
    pub fn pipelines_compiled(&self) -> u64 {
        self.pdeps.built.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Сколько РАЗНЫХ потоков скомпилировало пять горячих пайплайнов этого
    /// рендера (BUG-406, срез 2): 5 на параллельном пути (по умолчанию), 1
    /// под `LUMEN_SERIAL_PIPELINES=1`.
    ///
    /// Гейт правки стоит на нём, а не на времени старта: разброс wall-clock
    /// компиляции на DX12 доходит до 2.5× между прогонами одного и того же
    /// бинарника (`docs/perf-method.md`), а «компиляции разъехались по
    /// потокам» — точное утверждение.
    #[must_use]
    pub fn hot_pipeline_threads(&self) -> usize {
        self.hot_pipeline_threads.borrow().len()
    }

    /// Сколько горячих пайплайнов пришлось скомпилировать самому UI-потоку —
    /// гейт среза 3 BUG-406, ожидаемое значение **0**.
    ///
    /// Ненулевое означает, что кадр не дождался фонового потока, а собрал
    /// пайплайн сам (поток не стартовал, канал оборвался, или сборка вообще
    /// была синхронной), — то есть цена компиляции вернулась на UI-поток,
    /// ровно то, что срез убирает. Как и соседние счётчики, утверждение об
    /// идентичности, а не о времени (`docs/perf-method.md`).
    #[must_use]
    pub fn hot_pipelines_built_on_ui_thread(&self) -> usize {
        self.hot_built_on_ui.get()
    }

    /// Ячейка соответствующего вида — единственное место, где вид
    /// [`HotKind`] превращается в конкретное поле.
    fn hot_cell(&self, kind: HotKind) -> &OnceCell<wgpu::RenderPipeline> {
        match kind {
            HotKind::Fill => &self.fill_pipeline,
            HotKind::RRect => &self.rrect_pipeline,
            HotKind::Text => &self.text_pipeline,
            HotKind::Image => &self.image_pipeline,
            HotKind::Gradient => &self.gradient_pipeline,
        }
    }

    /// Ждёт с фоновых потоков (BUG-406 срез 3) именно пайплайн вида `want`,
    /// попутно раскладывая по ячейкам всё, что приехало раньше него.
    ///
    /// Ждать здесь правильнее, чем собирать самому: на DX12/Intel штраф
    /// компиляции привязан к потоку-вызывающему, поэтому «собрать самому»
    /// стоило бы UI-потоку тех же ~0.8 с, ради переноса которых сделан срез.
    /// Собственная сборка остаётся только аварийной веткой — когда фонового
    /// потока нет вовсе (не стартовал, уже отдал всё, либо сборка была
    /// синхронной и ячейка почему-то пуста).
    fn await_hot(&self, want: HotKind) -> wgpu::RenderPipeline {
        let t0 = std::time::Instant::now();
        loop {
            // `borrow_mut` на время одного `recv` — приём кладёт чужие
            // пайплайны в их ячейки, а те трогают только `OnceCell`.
            let received = {
                let guard = self.hot_rx.borrow();
                let Some(rx) = guard.as_ref() else { break };
                rx.recv()
            };
            match received {
                Ok((kind, thread, pipeline)) => {
                    self.hot_pipeline_threads.borrow_mut().insert(thread);
                    if kind == want {
                        if crate::frame_log_enabled() {
                            eprintln!(
                                "[wgpu] hot-wait {want:?}: {:.0}ms",
                                t0.elapsed().as_secs_f64() * 1000.0
                            );
                        }
                        return pipeline;
                    }
                    drop(self.hot_cell(kind).set(pipeline));
                }
                // Отправители кончились, а нужного вида среди них не было —
                // дальше ждать нечего.
                Err(_) => {
                    *self.hot_rx.borrow_mut() = None;
                    break;
                }
            }
        }
        self.hot_built_on_ui.set(self.hot_built_on_ui.get() + 1);
        self.hot_pipeline_threads.borrow_mut().insert(std::thread::current().id());
        self.hot_deps.build(want)
    }

    /// Материализует все пять горячих пайплайнов (BUG-406 срез 3). Нужен
    /// `LUMEN_EAGER_PIPELINES=1` и тестам-гейтам: без него ячейки на
    /// фоновом пути пусты до первого кадра.
    pub(crate) fn await_all_hot_pipelines(&self) {
        for kind in HOT_KINDS {
            // Именно `get_or_init`, а не прямой `await_hot`: ожидание одного
            // вида попутно раскладывает по ячейкам все приехавшие раньше него,
            // и второй раз ждать их из канала уже нечего — отправитель своё
            // отдал. Прямой вызов упирался бы в обрыв канала и достраивал
            // пайплайн на UI-потоке, то есть ровно то, что срез убирает.
            self.hot_cell(kind).get_or_init(|| self.await_hot(kind));
        }
    }

    /// Сплошная заливка. BUG-406 срез 3: ждёт фоновый поток сборки, если тот
    /// ещё не отдал пайплайн (см. [`Self::await_hot`]).
    pub(crate) fn fill_pipeline(&self) -> &wgpu::RenderPipeline {
        self.fill_pipeline.get_or_init(|| self.await_hot(HotKind::Fill))
    }

    /// Скруглённый прямоугольник (SDF). BUG-406 срез 3, см.
    /// [`Self::fill_pipeline`].
    pub(crate) fn rrect_pipeline(&self) -> &wgpu::RenderPipeline {
        self.rrect_pipeline.get_or_init(|| self.await_hot(HotKind::RRect))
    }

    /// Квады глифов из атласа. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    pub(crate) fn text_pipeline(&self) -> &wgpu::RenderPipeline {
        self.text_pipeline.get_or_init(|| self.await_hot(HotKind::Text))
    }

    /// Текстурный квад картинки. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    pub(crate) fn image_pipeline(&self) -> &wgpu::RenderPipeline {
        self.image_pipeline.get_or_init(|| self.await_hot(HotKind::Image))
    }

    /// Градиентная заливка. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    pub(crate) fn gradient_pipeline(&self) -> &wgpu::RenderPipeline {
        self.gradient_pipeline.get_or_init(|| self.await_hot(HotKind::Gradient))
    }
}
