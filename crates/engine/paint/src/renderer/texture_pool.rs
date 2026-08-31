//! P1/SPLIT-RN4: регистрация изображений в GPU-cache, layer-snapshot/backdrop/
//! texture-pool API, resize/viewport и GPU buffer/texture-хелперы —
//! непрерывный регион `impl Renderer` #2 из `renderer.rs` (7878…9065 до
//! вырезки). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-4).

use super::*;

impl Renderer {
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
    pub(crate) fn compute_image_gpu_key(&self, src: &str, box_rect: Rect, fit: ObjectFit, pos: ObjectPosition) -> String {
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
    pub(crate) fn ensure_image_gpu_key(
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
    pub(crate) fn prefetch_image_resizes_parallel(
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
    pub(crate) fn make_gpu_image_entry(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
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
    pub(crate) fn make_gpu_image_entry_mipped(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
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
                pass.set_pipeline(self.mipgen_pipeline());
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
        // BUG-435: мемоизации записей атласа живут ВНЕ атласа и переживали
        // эвикцию. При `High` атлас откатывает курсоры упаковки, новые глифы
        // ложатся поверх старых пикселей — уцелевший `cached_glyphs` после
        // этого рисовал бы чужие буквы; при `Medium` он просто отменял бы
        // эвикцию, возвращая уже удалённые записи.
        self.cached_glyphs.clear();
        self.text_run_cache.clear();
    }

    /// Сколько раз атлас глифов сбрасывался из-за исчерпания места (BUG-435).
    pub fn atlas_resets(&self) -> u64 {
        self.atlas_resets
    }

    /// Сбрасывает атлас глифов, если в прошлом кадре ему не хватило места
    /// (BUG-435).
    ///
    /// Атлас 1024×1024 копит глифы всех размеров, начертаний и загруженных
    /// @font-face-сабсетов страницы и никогда сам не освобождается: эвикция
    /// была только по внешнему memory-pressure. Переполнившись, он молча
    /// переставал принимать новые глифы — буква не рисовалась, advance
    /// оставался, и так до конца процесса, включая хром браузера.
    ///
    /// Сброс отложен до старта кадра намеренно: внутри кадра часть квадов уже
    /// уложена по координатам старых записей, и переупаковка атласа под ними
    /// подменила бы пиксели. Цена — один кадр без «новых» глифов; они
    /// появляются на следующем.
    ///
    /// Вместе с атласом чистятся обе внешние мемоизации его записей, иначе они
    /// вернули бы координаты, которые уже переписаны. Поколение контента
    /// бампается, чтобы кадр не был пропущен как идентичный предыдущему
    /// (глифы-то другие).
    pub(crate) fn recover_exhausted_atlas(&mut self) {
        if !self.atlas.take_exhausted() {
            return;
        }
        self.atlas.reset();
        self.cached_glyphs.clear();
        self.text_run_cache.clear();
        self.atlas_resets += 1;
        self.content_generation = self.content_generation.wrapping_add(1);
        let n = self.atlas_resets;
        timed_log(|| {
            eprintln!("[atlas] место исчерпано — сброс #{n}, глифы растеризуются заново");
        });
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

    /// Однострочная сводка по пулу offscreen-слоёв для `LUMEN_MEM_REPORT`
    /// (BUG-272 срез 21): свободные текстуры, классы размеров, объём
    /// свободного списка против бюджета и сколько вытеснено за сессию.
    #[must_use]
    pub fn texture_pool_report(&self) -> String {
        use std::sync::atomic::Ordering::Relaxed;
        let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        let budget = self.texture_pool.budget_bytes();
        format!(
            "texture_pool: free {} tex / {} size-classes / {:.1} MiB (budget {}) \
             | evicted {} | hits {} misses {}",
            self.texture_pool.len(),
            self.texture_pool.size_classes(),
            mib(self.texture_pool.free_bytes()),
            if budget == 0 { "off".to_string() } else { format!("{:.0} MiB", mib(budget)) },
            self.texture_pool.evicted(),
            TEXTURE_POOL_HITS.load(Relaxed),
            TEXTURE_POOL_MISSES.load(Relaxed),
        )
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
        // BUG-453: устройство потеряно — `surface.configure` ниже валиден не
        // больше, чем `frame.present()`, который эту потерю и обнаруживает.
        // Дальше в этом методе трогать нечего: без Device его пересоздавать
        // здесь не пытаемся (отдельная задача восстановления).
        if self.device_lost.get().is_some() {
            return;
        }
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

    /// BUG-453: `Some(reason)`, если GPU-устройство было потеряно (TDR, сон,
    /// отключение монитора, обновление драйвера) и подтверждено драйвером
    /// через `Device::set_device_lost_callback`. `WgpuBackend::render`
    /// использует это, чтобы вернуть настоящий `RenderError::DeviceLost`
    /// вместо generic-маппинга `wgpu::SurfaceError::Lost` → `SurfaceLost`
    /// (который читается как «пересоздать surface и повторить», а не как
    /// «переключиться на fallback-бэкенд»).
    #[must_use]
    pub fn device_lost_reason(&self) -> Option<String> {
        self.device_lost.get().cloned()
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

    /// Фиксированное смещение страницы в CSS px (ADR-016 M0.4, BUG-405 срез 38).
    ///
    /// Смещение опускает страницу под tab bar и сдвигает её вправо от левой
    /// docked-панели. Раньше шелл добивался этого `PushTransform`-ом вокруг
    /// всего display list-а — то есть глубоким клоном списка КАЖДЫЙ кадр
    /// (0.42 мс, 19 % кадра попадания на стенде среза 37). Здесь смещение
    /// становится затравкой стека трансформаций в [`render_impl`], что
    /// эквивалентно той обёртке команда-в-команду: скролл по-прежнему
    /// применяется к rect-у ДО матрицы, а страничная трансляция — после всех
    /// вложенных, как самая внешняя.
    ///
    /// Смещение входит в поколение контента, а не в хэш списка: список от него
    /// не меняется, а пиксели меняются — без бампа кадр после смены смещения
    /// был бы пропущен как идентичный, а полоса скролл-композитора (в чьи
    /// пиксели смещение запечено) переиспользована со старым смещением.
    ///
    /// Нефинитные значения (NaN/inf) сломали бы CTM — падаем на «без смещения»,
    /// как femtovg-бэкенд.
    ///
    /// [`render_impl`]: Renderer::render_impl
    pub fn set_page_offset(&mut self, x: f32, y: f32) {
        let next = if x.is_finite() && y.is_finite() { (x, y) } else { (0.0, 0.0) };
        if self.page_offset != next {
            self.content_generation = self.content_generation.wrapping_add(1);
            self.page_offset = next;
        }
    }

    /// Текущее смещение страницы (см. [`set_page_offset`](Self::set_page_offset)).
    #[must_use]
    pub fn page_offset(&self) -> (f32, f32) {
        self.page_offset
    }

    pub(crate) fn wgpu_color_for_canvas_bg(color: &Color, target: ColorSpace) -> [f32; 4] {
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
    pub(crate) fn surface_dims(&self) -> (u32, u32) {
        if let Some(c) = &self.config {
            (c.width, c.height)
        } else {
            (self.headless_w, self.headless_h)
        }
    }

    /// Создать uniform-буфер группы 0 на `slots` слотов и bind group к нему.
    /// Привязывается ОДИН слот (`size` = размер структуры), выбираемый
    /// динамическим офсетом на `set_bind_group` (BUG-405 срез 4).
    pub(crate) fn create_uniform_buffer(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        slots: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform-buf"),
            size: UNIFORM_SLOT_STRIDE * slots.max(1) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform-bg"),
            layout: bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ClipUniformSlot>() as u64),
                }),
            }],
        });
        (buffer, bind_group)
    }

    /// Записать слоты кадра в uniform-буфер, вырастив его при нехватке места.
    /// Создаёт и заливает вершинный буфер одной категории кадра.
    ///
    /// Тринадцать одинаковых блоков фазы `prep` собраны сюда (BUG-405 срез 10),
    /// чтобы подстатьи «создание ресурса» и «запись вершин» измерялись по
    /// отдельности: `t_create`/`t_write` накапливают их за кадр.
    /// Пустая категория буфера не создаёт — `None`.
    pub(crate) fn upload_vertex_buffer<T: Copy>(
        &self,
        label: &str,
        verts: &[T],
        t_create: &mut std::time::Duration,
        t_write: &mut std::time::Duration,
    ) -> Option<wgpu::Buffer> {
        if verts.is_empty() {
            return None;
        }
        let t0 = std::time::Instant::now();
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of_val(verts) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let t1 = std::time::Instant::now();
        self.queue.write_buffer(&buf, 0, as_bytes(verts));
        *t_create += t1 - t0;
        *t_write += t1.elapsed();
        Some(buf)
    }

    /// BUG-405 срез 11: подстатьи `uniforms` (выращивание буфера / раскладка
    /// слотов по шагу / `write_buffer`) считаются порознь — «дорого просить
    /// новый буфер», «дорого раскладывать» и «дорого отправлять» лечатся
    /// по-разному, а фаза до среза была одним числом.
    pub(crate) fn write_uniform_slots(
        &mut self,
        slots: &[ClipUniformSlot],
        t_grow: &mut std::time::Duration,
        t_build: &mut std::time::Duration,
        t_write: &mut std::time::Duration,
    ) {
        let stride = UNIFORM_SLOT_STRIDE as usize;
        let t0 = std::time::Instant::now();
        if slots.len() > self.uniform_slots {
            let want = slots.len().next_power_of_two();
            let (buf, bg) = Self::create_uniform_buffer(&self.device, &self.pdeps.uniform_bgl, want);
            self.uniform_buffer = buf;
            self.uniform_bind_group = bg;
            self.uniform_slots = want;
        }
        let t1 = std::time::Instant::now();
        // Один write_buffer вместо N: слоты раскладываются по шагу 256 в
        // промежуточный буфер (у кадра прокрутки их до трёх сотен).
        let mut bytes = vec![0u8; stride * slots.len()];
        for (i, slot) in slots.iter().enumerate() {
            let src = as_bytes(std::slice::from_ref(slot));
            bytes[i * stride..i * stride + src.len()].copy_from_slice(src);
        }
        let t2 = std::time::Instant::now();
        self.queue.write_buffer(&self.uniform_buffer, 0, &bytes);
        *t_grow += t1 - t0;
        *t_build += t2 - t1;
        *t_write += t2.elapsed();
    }

    pub(crate) fn create_layer_texture(&mut self, width: u32, height: u32) -> OffscreenLayer {
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
    pub(crate) fn release_layer_to_pool(&mut self, layer: OffscreenLayer) {
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
    pub(crate) fn small_depth_view(&mut self, width: u32, height: u32) -> wgpu::TextureView {
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
    pub(crate) fn ensure_scratch_layer(&mut self, width: u32, height: u32) {
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
    pub(crate) fn ensure_backdrop_layer(&mut self, width: u32, height: u32) {
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
    pub(crate) fn ensure_backdrop_cache_texture(&mut self, ordinal: u32, width: u32, height: u32) -> bool {
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

    pub(crate) fn ensure_layer_textures(&mut self, count: usize, width: u32, height: u32) {
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
}
