//! P1/SPLIT-RN6: точки входа рендера (`render`/`render_with_anim`/
//! `set_content_epoch` + их приватные хелперы content-fold) и
//! `render_to_image_*`/`render_tile`/`render_print_pages` — вынесены из
//! `renderer.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN,
//! батч RN-6). `render_impl` (5375 строк, риск группы) остаётся в
//! `renderer.rs` — расследование показало, что единственное заимствование
//! `self` внутри неё (`lazy_faces = &self.faces`) полностью потребляется
//! фазой «Сбор вершин» и не пересекает границы фаз, так что разбиение на
//! `&mut self`-хелперы конфликтом заимствований не блокируется — но и не
//! достигает потолка §5 (≤2000 строк/файл), поскольку сама фаза «Сбор
//! вершин» — 3080 строк, больше потолка уже сама по себе. Тот же
//! компромисс, что JS-7 принял для `install_dom` в `v8_runtime.rs`.

use super::*;

impl Renderer {
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

    /// Объявляет версию списка `content` ближайшего кадра (BUG-405 срез 39).
    /// Контракт вызывающего — [`RenderBackend::set_content_epoch`].
    pub fn set_content_epoch(&mut self, epoch: u64) {
        self.content_epoch = epoch;
    }

    /// Свёртка content-части с прошлого кадра, если её законно переиспользовать
    /// (BUG-405 срез 39); `None` — считать заново.
    ///
    /// Версия — главный сторож (только она ловит правку списка на месте), адрес
    /// и длина — страховка от подмены списка без смены версии, подпись
    /// выколотых диапазонов — от смены набора анимируемых сегментов (они входят
    /// в ключ полосы).
    fn content_fold_reuse(
        &self,
        content: &[DisplayCommand],
        skip: &[std::ops::Range<usize>],
    ) -> Option<(u64, u64)> {
        if self.content_epoch == 0 || dl_epoch_disabled() {
            return None;
        }
        let memo = self.content_fold_memo.as_ref()?;
        if memo.epoch != self.content_epoch
            || memo.ptr != content.as_ptr().addr()
            || memo.len != content.len()
            || memo.skip_sig != skip_signature(skip)
        {
            return None;
        }
        if dl_epoch_verify() {
            let fresh = crate::display_list::fold_content_dual(content, skip);
            if fresh != memo.folds {
                DL_EPOCH_MISMATCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                eprintln!(
                    "[dl-epoch] РАСХОЖДЕНИЕ: версия {} не сменилась, а список \
                     изменился ({} команд, запомнено {:?}, пересчитано {:?})",
                    self.content_epoch,
                    content.len(),
                    memo.folds,
                    fresh,
                );
                return None;
            }
        }
        Some(memo.folds)
    }

    /// Запоминает свёртку content-части для следующего кадра (BUG-405 срез 39).
    /// При неизвестной версии (`0`) память чистится — переиспользовать нечего.
    fn remember_content_fold(
        &mut self,
        content: &[DisplayCommand],
        skip: &[std::ops::Range<usize>],
        folds: (u64, u64),
    ) {
        if self.content_epoch == 0 || dl_epoch_disabled() {
            self.content_fold_memo = None;
            return;
        }
        self.content_fold_memo = Some(ContentFoldMemo {
            epoch: self.content_epoch,
            ptr: content.as_ptr().addr(),
            len: content.len(),
            skip_sig: skip_signature(skip),
            folds,
        });
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
        // BUG-405 срез 44: точка отсчёта ДО первой отсечки `ComposeMarks` —
        // см. doc-комментарий `PRE_MARKS_NANOS`.
        let t_entry = crate::frame_log_enabled().then(std::time::Instant::now);
        // BUG-435: место в атласе кончилось на прошлом кадре — сбрасываем ДО
        // хэша кадра, чтобы бамп поколения контента попал в хэш и кадр не был
        // пропущен как идентичный.
        self.recover_exhausted_atlas();
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
        // BUG-405 срез 34 (пункт 68 остатка): кадр ПОПАДАНИЯ стоит 4.3 мс при
        // пассе композитора 0.9 мс — остаток платится здесь, в оркестраторе, и
        // до среза 34 не был расписан ни одной статьёй. Хэш кадра — O(n) по
        // всему списку и считается на КАЖДОМ кадре, включая попадания.
        //
        // Срез 35 (пункт 70): вторым таким O(n)-хэшом был ключ полосы, и вместе
        // они стоили дороже композитного пасса. Теперь список обходится ОДИН
        // раз на оба хэша, поэтому подготовка компоновки (её размеры и
        // диапазоны сегментов — входы ключа) идёт до хэша, а не после.
        // BUG-405 срез 37: исход прошлого кадра к этому отношения не имеет —
        // `compose_page` может не дойти до своей развилки вовсе (отказ
        // подготовки, нестабильный ключ), и тогда кадр обязан читаться как
        // «компоновки не было», а не как повтор прошлого попадания.
        ComposeOutcome::Skip.store();
        let mut marks = ComposeMarks::new();
        if let Some(t0) = t_entry {
            PRE_MARKS_NANOS.fetch_add(
                t0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        let prep = self.prepare_page_compose(content, scroll_x, anim_ranges, &mut marks);
        let skip: &[std::ops::Range<usize>] = prep.as_ref().map_or(&[], |p| &p.ranges);
        let band_dims = prep.as_ref().map_or((0, 0), |p| (p.sw, p.band_h_px));
        // BUG-405 срез 39: переиспользована ли свёртка content-части на этом
        // кадре — единственная статья, которой различаются плечи `frame-hash`,
        // поэтому она печатается рядом с его временем.
        let mut fold_reused = false;
        // BUG-405 срез 47: overlay-дайджест ([`crate::display_list::fold_overlay`])
        // считается здесь ОДИН раз и переиспользуется ниже в `compose_page` →
        // `overlay_cache_step`, которая раньше пересчитывала тот же дайджест
        // заново (статья `послекэша`, срез 44). `None` на плече `dual_hash_disabled()`
        // — та ветка сознательно воспроизводит поведение до среза 35 (два
        // раздельных обхода) и не должна получать переиспользуемый дайджест.
        let mut overlay_digests: Option<Vec<u64>> = None;
        let (base_hash, band_key_base) = if dual_hash_disabled() {
            // Плечо A/B: два раздельных обхода, как до среза 35.
            (
                crate::display_list::hash_display_list(
                    content, overlay, scroll_x, scroll_y, sw0, sh0,
                ),
                crate::display_list::hash_display_list_skipping(
                    content, skip, &[], 0.0, 0.0, band_dims.0, band_dims.1,
                ),
            )
        } else {
            // BUG-405 срез 39: свёртка content-части переиспользуется, пока
            // shell не сменил версию списка. Остальные входы обоих хэшей
            // (скролл, размеры поверхности и полосы, длины, overlay) в свёртку
            // не входят и дописываются каждый кадр, поэтому кадр не становится
            // слеп ни к одному из них.
            let reuse = self.content_fold_reuse(content, skip);
            fold_reused = reuse.is_some();
            let digests = crate::display_list::fold_overlay(overlay);
            let (hashes, folds) =
                crate::display_list::hash_display_list_dual_memo_with_overlay_digests(
                    content,
                    &digests,
                    skip,
                    (scroll_x, scroll_y),
                    (sw0, sh0),
                    band_dims,
                    reuse,
                );
            overlay_digests = Some(digests);
            if fold_reused {
                DL_FOLD_REUSED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            self.remember_content_fold(content, skip, folds);
            hashes
        };
        marks.mark(3);
        if marks.printing() {
            let ms = marks.ms[3] - marks.ms[2];
            let (nc, no) = (content.len(), overlay.len());
            let mode = if dual_hash_disabled() { "два прохода" } else { "один проход" };
            // BUG-405 срез 39: «свёртка» — content-часть переиспользована,
            // обойдён только overlay; «обход» — список обойдён целиком.
            let fold = if fold_reused { "свёртка" } else { "обход" };
            timed_log(|| {
                eprintln!(
                    "[frame:wgpu] frame-hash: {ms:.2}ms ({nc} + {no} cmds, {mode}, {fold})"
                );
            });
            // Печать стоит 0.1–0.3 мс (срез 34) — сдвигаем метку, чтобы она не
            // легла в статью `band` соседней строки.
            marks.mark(3);
        }
        // Поколение контента (register_image / GIF-кадры / снапшоты / шрифты /
        // canvas-bg) складывается с обеими свёртками: список тот же, а пиксели
        // уже другие.
        let generation = self.content_generation;
        let fold_gen = |base: u64| {
            use std::hash::Hasher;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            h.write_u64(base);
            h.write_u64(generation);
            h.finish()
        };
        let frame_hash = fold_gen(base_hash);
        let band_key = fold_gen(band_key_base);
        if self.surface.is_some()
            && !frame_skip_disabled()
            && self.last_frame_hash == Some(frame_hash)
        {
            FRAMES_SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if crate::frame_log_level() >= 2 {
                eprintln!("[frame:wgpu] skip (identical frame)");
            }
            flush_compose_marks(&marks);
            return Ok(());
        }

        // Скролл-композитор страницы (EXPERIMENT.md §2): при попадании кадр
        // собирается из персистентной полосы + overlay, минуя перерисовку
        // контента. `None`/`false` — путь неприменим, рисуем монолитом.
        if let Some(prep) = prep
            && self.compose_page(
                content,
                overlay,
                overlay_digests.as_deref(),
                scroll_y,
                &prep,
                band_key,
                &mut marks,
            )?
        {
            self.last_frame_hash = Some(frame_hash);
            flush_compose_marks(&marks);
            return Ok(());
        }

        flush_compose_marks(&marks);
        self.render_impl(
            content,
            overlay,
            scroll_y,
            scroll_x,
            RenderPassMode::Normal { frame_hash },
        )
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
        self.render_to_image_with_overlay(commands, &[], scroll_y, scroll_x)
    }

    /// Как [`render_to_image`](Self::render_to_image), но с overlay-списком.
    ///
    /// Отделено BUG-405 срезом 38: страничное смещение обязано ложиться на
    /// контент и НЕ ложиться на overlay, поэтому гейт эквивалентности должен
    /// уметь читать кадр, в котором есть оба списка.
    pub(crate) fn render_to_image_with_overlay(
        &mut self,
        commands: &[crate::DisplayCommand],
        overlay: &[crate::DisplayCommand],
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
        self.render(commands, overlay, scroll_y, scroll_x)
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
