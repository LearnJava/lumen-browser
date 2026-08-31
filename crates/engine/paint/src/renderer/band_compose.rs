//! P1/SPLIT-RN5: band-компоновка скролл-композитора страницы (BUG-405) —
//! геометрия и кольцевая адресация полосы (`RingStrip`/`ring_advance_plan`/
//! `band_blit_quads`/`band_geometry`) + непрерывный регион `impl Renderer` #2
//! из `renderer.rs` (7879…8721 до вырезки): подготовка/попадание-промах/
//! композиция кадра из полосы, overlay-кэш стабильного хвоста, прогрев.
//! Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-5).

use super::*;

fn band_ring_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("LUMEN_BAND_RING").is_ok_and(|v| v != "0"))
}

/// Одна кромка кольцевой полосы: непрерывный диапазон строк ТЕКСТУРЫ и
/// документная строка, попадающая в первую из них. Всё в device px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RingStrip {
    /// Первая перерисовываемая строка текстуры полосы.
    pub(crate) row0: u32,
    /// Сколько строк перерисовывается (> 0).
    pub(crate) rows: u32,
    /// Документный Y строки `row0`.
    pub(crate) doc_y0: i64,
}

/// План инкрементальной дорисовки полосы при сдвиге её верха
/// `old_top` → `new_top` (BUG-405 срез 32, пункт 43/58 остатка).
///
/// Текстура полосы трактуется как ТОР по Y: документная строка `y` живёт в
/// строке текстуры `(y − ring_base) mod band_h`, поэтому сдвиг полосы не
/// требует ни копии (пункт 61: ping-pong стоил бы второй полной текстуры), ни
/// перерисовки перекрытия — обновить надо только вышедшую вперёд кромку.
/// Кромка, разрезанная краем текстуры, отдаётся ДВУМЯ строчными диапазонами:
/// один пасс через край невозможен, потому что scissor непрерывен.
///
/// `None` — кольцом не обойтись, нужна полная перерисовка: сдвиг нулевой либо
/// не меньше высоты полосы (перекрытия нет вовсе).
pub(crate) fn ring_advance_plan(
    band_h: u32,
    ring_base: i64,
    old_top: i64,
    new_top: i64,
) -> Option<Vec<RingStrip>> {
    if band_h == 0 {
        return None;
    }
    let h = i64::from(band_h);
    let delta = new_top - old_top;
    if delta == 0 || delta.abs() >= h {
        return None;
    }
    // Вниз освобождается хвост полосы (документные строки за прежним низом),
    // вверх — её голова. В обоих случаях длина кромки = |сдвиг|.
    let doc_y0 = if delta > 0 { old_top + h } else { new_top };
    let count = delta.unsigned_abs();
    let row0 = (doc_y0 - ring_base).rem_euclid(h) as u32;
    let first = count.min(u64::from(band_h - row0));
    // `count < h`, поэтому кромка режется краем текстуры не больше одного раза.
    let mut strips = vec![RingStrip { row0, rows: first as u32, doc_y0 }];
    if count > first {
        strips.push(RingStrip {
            row0: 0,
            rows: (count - first) as u32,
            doc_y0: doc_y0 + first as i64,
        });
    }
    Some(strips)
}

/// Квад блита полосы на Compose-кадре: `(прямоугольник в CSS px кадра, uv0,
/// uv1)`.
///
/// `dy_css` — сдвиг верха полосы относительно вьюпорта (`band_top − scroll_y`,
/// ≤ 0), `phase_px` — фаза кольца (строка текстуры, в которой лежит верх
/// полосы). Фаза сдвигает uv по V на ту же долю: квад по-прежнему один и
/// по-прежнему покрывает ровно одну высоту текстуры, но его `v` уходит за
/// единицу, а `Repeat` у [`Renderer::band_sampler`] заворачивает хвост в
/// голову. При нулевой фазе это ровно `0…1`, то есть путь до среза 32.
///
/// Разрезать квад по шву вместо `Repeat` НЕЛЬЗЯ: на шве текстуры документно
/// соседствуют строки `H−1` и `0`, и при дробном сдвиге блита (нецелый
/// `scroll_y`) линейная фильтрация обязана взять обе, а два квада с
/// `ClampToEdge` подсунули бы каждому свой край. Шов попадает во вьюпорт почти
/// всегда, внешние края полосы — почти никогда, поэтому цена ошибки у этих
/// двух вариантов разная на порядок.
pub(crate) fn band_blit_quads(
    dy_css: f32,
    w_css: f32,
    band_h_px: u32,
    phase_px: u32,
    dpr: f32,
) -> Vec<(Rect, [f32; 2], [f32; 2])> {
    let h_css = band_h_px as f32 / dpr;
    let v = if band_h_px == 0 { 0.0 } else { phase_px as f32 / band_h_px as f32 };
    vec![(
        Rect { x: 0.0, y: dy_css, width: w_css, height: h_css },
        [0.0, v],
        [1.0, 1.0 + v],
    )]
}

/// Геометрия полосы скролл-композитора под текущую поверхность:
/// `(запас с каждой стороны, полная высота полосы)` в device px, либо причина
/// отказа для `page-compose skip`.
///
/// Вынесено из [`Renderer::try_page_compose`] отдельной функцией (BUG-405 срез
/// 22): решение целиком арифметическое — полосе нужен GPU, а выбору её высоты
/// нет, — поэтому его гейтит юнит-тест без устройства.
///
/// Полный запас — по 3/4 вьюпорта сверху и снизу, но не больше 768 CSS px.
/// Если такая полоса не влезает в `max_dim`, при `clamp` она **ужимается** до
/// лимита вместо отказа (срез 22): до среза 23 живое устройство запрашивалось
/// с `wgpu::Limits::downlevel_defaults()` (`max_texture_dimension_2d` = 2048)
/// при полосе в 2.5 вьюпорта, поэтому прежний безусловный отказ выключал
/// скролл-композитор на ЛЮБОМ окне выше ~819 device px, то есть почти на любом
/// развёрнутом (перепись среза 22: `lenta.ru`, окно 1200×991 — ни одного
/// Compose-кадра, `p50` кадра 0.90–1.06 мс против 0.49–0.56 с ужатием).
///
/// С поднятым лимитом (срез 23, [`requested_max_texture_dim`]) ужатие на
/// живом устройстве не срабатывает ни на одном реальном окне: запас упирается
/// в потолок 768 CSS px раньше, чем в лимит, то есть полоса — это вьюпорт плюс
/// 1536 CSS px. Путь ужатия остаётся рабочим для headless-устройства (там
/// по-прежнему `downlevel_defaults`) и для адаптеров беднее цели.
pub(crate) fn band_geometry(
    sw: u32,
    sh: u32,
    dpr: f32,
    max_dim: u32,
    clamp: bool,
    margin_override_css: Option<f32>,
) -> Result<(u32, u32), &'static str> {
    if sw == 0 || sh == 0 {
        return Err("нулевой размер поверхности");
    }
    // Ниже считается `max_dim - sh`, поэтому вьюпорт крупнее лимита отсеиваем
    // здесь: в такой поверхности полоса невозможна ни с ужатием, ни без.
    if sw > max_dim || sh > max_dim {
        return Err("вьюпорт выше лимита текстуры");
    }
    let vp_h_css = sh as f32 / dpr;
    let margin_want_css =
        margin_override_css.unwrap_or_else(|| (vp_h_css * 0.75).min(BAND_MARGIN_CAP_CSS));
    let margin_want_px = (margin_want_css.floor() * dpr).round() as u32;
    let margin_px = if clamp {
        margin_want_px.min((max_dim - sh) / 2)
    } else {
        margin_want_px
    };
    let band_h_px = sh + 2 * margin_px;
    if band_h_px > max_dim {
        return Err("полоса выше лимита текстуры (ужатие отключено)");
    }
    // Ужатый запас имеет смысл, только пока промахи редки: при запасе меньше
    // [`BAND_MIN_MARGIN_RATIO`] вьюпорта промах случается почти каждым кадром
    // прокрутки, а промах — это рендер всей полосы, то есть дороже монолита во
    // столько раз, во сколько полоса выше вьюпорта. Тогда честнее отказаться.
    if (margin_px as f32) < BAND_MIN_MARGIN_RATIO * sh as f32 {
        return Err("вьюпорт не оставляет запаса в лимите текстуры");
    }
    Ok((margin_px, band_h_px))
}

/// `true`, если ужатие полосы под лимит текстуры отключено
/// (`LUMEN_NO_BAND_CLAMP=1`): полоса выше `max_texture_dimension_2d` снова
/// отключает скролл-композитор целиком, как до среза 22 BUG-405. Нужен для
/// интерливед-A/B на одном бинарнике — плечи различаются только этим
/// решением (`docs/perf-method.md`).
fn band_clamp_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BAND_CLAMP").is_ok_and(|v| v == "1")
    })
}

impl Renderer {
    /// Печатает причину отказа скролл-композитора под `LUMEN_FRAME_LOG>=2` —
    /// только при её смене (BUG-405 срез 22).
    ///
    /// Каждым кадром строка была бы дублем: причина держится десятками кадров
    /// подряд. Интерес представляет ПЕРЕХОД — кадр, на котором композитор
    /// перестал применяться (например, рост окна открыл на странице
    /// sticky-колонку), поэтому повтор той же причины молчит.
    fn note_compose_skip(&mut self, reason: &'static str) {
        if self.last_compose_skip == Some(reason) {
            return;
        }
        self.last_compose_skip = Some(reason);
        if crate::frame_log_level() >= 2 {
            eprintln!("[frame:wgpu] page-compose skip: {reason}");
        }
    }

    /// Создаёт кэш полосы скролл-композитора (цветная текстура + depth) под
    /// размер `sw × band_h_px` в device px, заменяя прежний.
    ///
    /// Ключ полосы ставится в 0 — «содержимое невалидно»: заполняет его только
    /// прошедший Band-рендер. Вынесено из [`Renderer::try_page_compose`]
    /// (BUG-405 срез 20), потому что ту же полосу создаёт прогрев.
    pub(crate) fn create_page_band(&mut self, sw: u32, band_h_px: u32, band_top_css: f32) {
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
            usage: if band_copy_usage_enabled() {
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
            },
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Bind group блита — вход у неё только этот view и постоянный sampler,
        // поэтому она создаётся здесь и живёт до пересоздания полосы.
        let blit_bg = self.create_band_blit_bind_group(&view);
        let (depth_t, depth_v) = create_depth_texture(&self.device, sw, band_h_px);
        self.page_band = Some(PageBandCache {
            _texture: texture,
            view,
            blit_bg,
            key: 0, // невалиден, пока Band-рендер не пройдёт
            band_top_css,
            // Свежая полоса перерисовывается целиком, то есть фаза кольца
            // нулевая: строка 0 текстуры держит документную строку `band_top`.
            ring_base_css: band_top_css,
            w_px: sw,
            h_px: band_h_px,
            depth_t,
            depth_v,
        });
    }

    /// Собирает bind group блита полосы: view полосы + постоянный linear
    /// sampler по layout-у `image_bgl`.
    ///
    /// BUG-405 срез 21: раньше эта группа собиралась на каждом Compose-кадре,
    /// хотя оба её входа меняются только вместе с самой полосой. Счётчик
    /// [`BAND_BLIT_BGS_CREATED`] гейтит именно это — прогон прокрутки
    /// `lenta.ru` давал 40 наборов дескрипторов вместо 1.
    fn create_band_blit_bind_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        BAND_BLIT_BGS_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("page-band-bg"),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    // `Repeat` по V нужен только кольцу (срез 32): без него
                    // фаза всегда нулевая, uv не выходят из `0…1`, и штатный
                    // путь остаётся на том же sampler-е, что до среза, — то
                    // есть выключенный рычаг не меняет ни одного пикселя даже
                    // на краю полосы.
                    resource: wgpu::BindingResource::Sampler(if band_ring_enabled() {
                        &self.band_sampler
                    } else {
                        &self.image_sampler
                    }),
                },
            ],
        })
    }

    /// Строит/пересобирает retained-текстуру стабильного хвоста overlay-списка
    /// (BUG-405 срез 41): `tail` — `overlay[prefix_len..]`, рисуется в новую
    /// текстуру с прозрачным клиром ([`RenderPassMode::OverlayCache`]) в
    /// СВОЁМ исходном относительном порядке. Размер текстуры — вся
    /// поверхность (overlay viewport-locked, полосы у него, в отличие от
    /// контента, нет).
    ///
    /// `tail_digests` — digest ХВОСТА (не всего списка) на момент постройки;
    /// `compose_page` сравнивает его с `current[prefix_len..]` на каждом
    /// последующем вызове, чтобы решить, валиден ли ещё кэш (см.
    /// doc-комментарий [`OverlayCache`]).
    fn build_overlay_cache(
        &mut self,
        w_px: u32,
        h_px: u32,
        tail: &[DisplayCommand],
        tail_digests: Vec<u64>,
        prefix_len: usize,
    ) -> Result<(), wgpu::SurfaceError> {
        count_texture_created_labeled("overlay-cache", w_px, h_px);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay-cache"),
            size: wgpu::Extent3d { width: w_px, height: h_px, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Bind group блита переиспользует `image_bgl` полосы — оба входа
        // (view + linear sampler) устроены одинаково, ring-repeat сюда не
        // нужен (один квад, uv всегда `0…1`), но и не мешает.
        let blit_bg = self.create_band_blit_bind_group(&view);
        self.render_impl(
            &[],
            tail,
            0.0,
            0.0,
            RenderPassMode::OverlayCache { view, w_px, h_px },
        )?;
        self.overlay_cache = Some(OverlayCache {
            _texture: texture,
            blit_bg,
            w_px,
            h_px,
            tail_digests,
            prefix_len,
        });
        Ok(())
    }

    /// Прогрев полосы скролл-композитора (BUG-405 срез 20): создаёт её текстуры
    /// заранее и один раз отрисовывает в них пустой пасс.
    ///
    /// Смысл — не в самой очистке, а в том, что цену ПЕРВОЙ отрисовки в свежую
    /// цель (перепись `lenta.ru`/Vulkan: `drop(pass)` 4.6 мс против 0.15 мс у
    /// следующих отрисовок в ту же текстуру) платит кадр загрузки, а не первый
    /// кадр прокрутки. Пиксельно нейтрально: ключ полосы остаётся невалидным,
    /// поэтому первое реальное обращение перерисовывает её содержимое целиком,
    /// а до того полоса ни разу не читается.
    pub(crate) fn warm_page_band(&mut self, sw: u32, band_h_px: u32) {
        let t0 = std::time::Instant::now();
        self.create_page_band(sw, band_h_px, 0.0);
        let t_create = t0.elapsed();
        let Some((view, depth_v)) = self
            .page_band
            .as_ref()
            .map(|b| (b.view.clone(), b.depth_v.clone()))
        else {
            return;
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("page-band-warm"),
            });
        // Пасс без единого draw: прогревает саму цель, а не конвейер.
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("page-band-warm-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_v,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let t_pass0 = std::time::Instant::now();
        drop(pass);
        let t_pass = t_pass0.elapsed();
        self.queue.submit(Some(encoder.finish()));
        if crate::frame_log_level() >= 2 {
            // Цена, перенесённая с первого кадра прокрутки на кадр загрузки:
            // печатается вместе с разбивкой, чтобы перенос был виден целиком,
            // а не только его результат.
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
            eprintln!(
                "[frame:wgpu] page-band warm: {sw}x{band_h_px} px за {:.2}мс \
                 (текстуры {:.2} / пасс {:.2} / submit {:.2})",
                ms(t0.elapsed()),
                ms(t_create),
                ms(t_pass),
                ms(t_pass0.elapsed()) - ms(t_pass),
            );
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
    /// Эта половина — только подготовка: проверки применимости, геометрия
    /// полосы и план split-а. `None` — путь неприменим, кадр идёт монолитом.
    /// Отделена от [`compose_page`](Self::compose_page) срезом 35 (BUG-405,
    /// пункт 70), потому что ключ полосы считается теперь тем же проходом по
    /// списку, что и хэш кадра, — а для этого его входы (размеры полосы и
    /// effective-диапазоны сегментов) должны быть известны ДО хэша.
    pub(crate) fn prepare_page_compose(
        &mut self,
        content: &[DisplayCommand],
        scroll_x: f32,
        anim_ranges: &[std::ops::Range<usize>],
        marks: &mut ComposeMarks,
    ) -> Option<ComposePrep> {
        let skip = if self.surface.is_none() {
            Some("headless (нет surface)")
        } else if scroll_compositor_disabled() {
            Some("выключен LUMEN_NO_SCROLL_COMPOSITOR")
        } else if scroll_x != 0.0 {
            Some("горизонтальный скролл")
        } else if content.is_empty() {
            Some("пустой display list")
        } else if content
            .iter()
            .any(|c| matches!(c, DisplayCommand::BeginStickyLayer { .. }))
        {
            Some("sticky-слой в контенте")
        } else {
            None
        };
        if let Some(reason) = skip {
            self.note_compose_skip(reason);
            return None;
        }
        marks.mark(0);
        let (sw, sh) = self.surface_dims();
        let dpr = self.scale_factor.max(1e-6) as f32;
        let vp_h_css = sh as f32 / dpr;
        let max_dim = self.device.limits().max_texture_dimension_2d;
        let (margin_px, band_h_px) =
            match band_geometry(
                sw,
                sh,
                dpr,
                max_dim,
                !band_clamp_disabled(),
                band_margin_override_css(),
            ) {
                Ok(g) => g,
                Err(reason) => {
                    self.note_compose_skip(reason);
                    return None;
                }
            };
        self.last_compose_skip = None;
        // Запас в CSS px берём из ФАКТИЧЕСКОЙ высоты полосы: при ужатии он
        // меньше желаемого, а при dpr ≠ 1 это заодно снимает расхождение
        // ≤0.5 px между запасом, по которому полоса построена, и запасом, по
        // которому ниже считается её верх.
        let margin_css = margin_px as f32 / dpr;
        let band_h_css = band_h_px as f32 / dpr;
        marks.mark(1);

        // Static/animated split: план оверлея сегментов. При конфликте
        // painter's order план сам расширяет диапазоны tail-split-ом —
        // хэш/полосу дальше считаем по ЕГО effective-диапазонам. Полный
        // отказ (нереплеябельный контекст и т.п.) — split выключается на
        // кадр, ключ считается по полному списку (= поведение до среза).
        let ranges: &[std::ops::Range<usize>] = if anim_split_disabled() {
            &[]
        } else {
            anim_ranges
        };
        let mut effective_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        let seg_plan: Option<crate::display_list::DisplayList> = if ranges.is_empty() {
            None
        } else {
            match crate::display_list::anim_split_compose_plan(content, ranges) {
                Some((p, eff)) => {
                    effective_ranges = eff;
                    Some(p)
                }
                None => None,
            }
        };

        marks.mark(2);
        Some(ComposePrep {
            sw,
            dpr,
            margin_css,
            band_h_px,
            band_h_css,
            vp_h_css,
            ranges: effective_ranges,
            seg_plan,
        })
    }

    /// Собирает кадр из полосы: попадание — blit + overlay, промах — рендер
    /// полосы и та же композиция. Подготовка (применимость, геометрия, план
    /// сегментов) уже сделана [`prepare_page_compose`](Self::prepare_page_compose),
    /// `key` посчитан вместе с хэшом кадра одним проходом по списку.
    ///
    /// BUG-405 срез 41: решает, чем нарисовать overlay кадра компоновки —
    /// целиком (`None`) или живым префиксом плюс блитом retained-текстуры
    /// стабильного хвоста (`Some(prefix_len)`, и тогда
    /// `self.pending_overlay_blit` уже выставлен) — см. doc-комментарий
    /// [`OverlayCache`] про то, почему хвост, а не «горячая команда поверх
    /// всего» (первая версия этого среза, забракованная переписью на
    /// реальном хроме: painter's-order конфликт был не редким случаем, а
    /// постоянным — скроллбар геометрически пересекается с хедером).
    ///
    /// Кэш валиден, пока digest ХВОСТА (`overlay[prefix_len..]`) совпадает
    /// с тем, что был при постройке — префикс участвует только в выборе
    /// НОВОЙ точки разреза, но не в проверке валидности старого кэша: он
    /// рисуется живьём в любом случае, так что его изменение неважно.
    ///
    /// Новая точка разреза при пересборке — на одну ПОЗЖЕ самой поздней
    /// позиции, отличающейся от ПРОШЛОГО кадра (`self.last_overlay_digests`
    /// — не от кэша, тот мог протухнуть много кадров назад), сдвинутая
    /// вперёд до ближайшей сбалансированной по push/pop границы
    /// (`balanced_cut_at_or_after`) — резать список пополам открытого
    /// `Push*` нельзя.
    ///
    /// `overlay_digests` — тот же [`crate::display_list::fold_overlay`],
    /// который `render_with_anim` уже посчитал для кадрового хэша (BUG-405
    /// срез 47): раньше этот метод обходил `overlay` `hash_one_command`-ом
    /// ЗАНОВО, хотя тот же дайджест уже был посчитан секундами раньше в этом
    /// же кадре (срез 43/44 измерили эту вторую свёртку как статью
    /// `послекэша`, ~0.12 мс на кадре попадания). `None` (только под
    /// `LUMEN_NO_OVERLAY_DIGEST_REUSE=1`) — старое поведение, для A/B.
    pub(crate) fn overlay_cache_step(
        &mut self,
        overlay: &[DisplayCommand],
        overlay_digests: Option<&[u64]>,
    ) -> Result<Option<usize>, wgpu::SurfaceError> {
        let current: Vec<u64> = match overlay_digests {
            Some(d) => d.to_vec(),
            None => overlay.iter().map(crate::display_list::hash_one_command).collect(),
        };
        let (sw, sh) = self.surface_dims();
        let dpr = self.scale_factor.max(1e-6) as f32;
        let full_quad = |bind_group: wgpu::BindGroup| PendingBaseBlit {
            bind_group,
            quads: vec![(
                Rect { x: 0.0, y: 0.0, width: sw as f32 / dpr, height: sh as f32 / dpr },
                [0.0, 0.0],
                [1.0, 1.0],
            )],
        };
        let log = crate::frame_log_level() >= 2;

        // 1. Кэш уже есть — проверить, что его хвост всё ещё совпадает.
        if let Some(cache) = self.overlay_cache.as_ref() {
            let still_matches = cache.w_px == sw
                && cache.h_px == sh
                && cache.prefix_len <= current.len()
                && current.len() - cache.prefix_len == cache.tail_digests.len()
                && current[cache.prefix_len..]
                    .iter()
                    .zip(cache.tail_digests.iter())
                    .all(|(a, b)| a == b);
            if still_matches {
                self.pending_overlay_blit = Some(full_quad(cache.blit_bg.clone()));
                let prefix_len = cache.prefix_len;
                self.last_overlay_digests = current;
                if log {
                    // BUG-405 срез 42: эта строка — тоже инструмент (п. 71),
                    // её печать обязана попасть в FRAME_LOG_NANOS, а не в
                    // невязку разбивки кадра попадания.
                    timed_log(|| {
                        eprintln!("[frame:wgpu]   overlay-cache HIT prefix={prefix_len}");
                    });
                }
                return Ok(Some(prefix_len));
            }
            if log {
                let stale_prefix = cache.prefix_len;
                timed_log(|| {
                    eprintln!("[frame:wgpu]   overlay-cache STALE prefix={stale_prefix}");
                });
            }
        }

        // Хвост не совпал (кэша не было / устарел / поверхность сменила
        // размер) — сбросить и попробовать построить новый.
        self.overlay_cache = None;

        // 2. Точка разреза — сразу после самой поздней позиции, отличающейся
        // от ПРОШЛОГО кадра.
        let same_len = self.last_overlay_digests.len() == current.len();
        let last_change = same_len.then(|| {
            current
                .iter()
                .zip(self.last_overlay_digests.iter())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| i)
                .max()
        }).flatten();
        self.last_overlay_digests = current.clone();

        let Some(last_change) = last_change else {
            if log {
                timed_log(|| {
                    eprintln!("[frame:wgpu]   overlay-cache no-change-info same_len={same_len}");
                });
            }
            return Ok(None);
        };
        let prefix_len = balanced_cut_at_or_after(overlay, last_change + 1);
        if prefix_len >= overlay.len() {
            if log {
                let overlay_len = overlay.len();
                timed_log(|| {
                    eprintln!(
                        "[frame:wgpu]   overlay-cache tail-empty prefix={prefix_len} len={overlay_len}",
                    );
                });
            }
            return Ok(None);
        }
        let tail_digests = current[prefix_len..].to_vec();
        self.build_overlay_cache(sw, sh, &overlay[prefix_len..], tail_digests, prefix_len)?;
        let Some(bind_group) = self.overlay_cache.as_ref().map(|c| c.blit_bg.clone()) else {
            return Ok(None);
        };
        self.pending_overlay_blit = Some(full_quad(bind_group));
        if log {
            timed_log(|| {
                eprintln!("[frame:wgpu]   overlay-cache MISS built prefix={prefix_len}");
            });
        }
        Ok(Some(prefix_len))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compose_page(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        overlay_digests: Option<&[u64]>,
        scroll_y: f32,
        prep: &ComposePrep,
        key: u64,
        marks: &mut ComposeMarks,
    ) -> Result<bool, wgpu::SurfaceError> {
        let ComposePrep { sw, dpr, margin_css, band_h_px, band_h_css, vp_h_css, .. } = *prep;
        let ranges: &[std::ops::Range<usize>] = &prep.ranges;
        let seg_plan = prep.seg_plan.as_deref();

        // BUG-405 срез 20: прогрев полосы. Ниже по функции полоса создаётся
        // лениво — на первом промахе, то есть на первом кадре ПРОКРУТКИ, — и
        // первая отрисовка в свежую цель стоит на порядок дороже последующих
        // (перепись: `drop(pass)` 4.6 мс против 0.15 мс на следующих промахах
        // с той же текстурой). Создаём и прогреваем её здесь, на кадре
        // загрузки: сюда доходят только страницы, для которых композитор в
        // принципе применим (подготовка уже отсеяла непригодные), а размер
        // полосы зависит только от поверхности и dpr.
        if self.page_band.is_none() && !band_warm_disabled() {
            self.warm_page_band(sw, band_h_px);
        }

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
            // BUG-405 срез 32 (пункты 43/58 остатка): перекрытие старой и новой
            // полосы уже нарисовано и лежит в текстуре — перерисовать надо
            // только вышедшую вперёд кромку. Кольцевая адресация (текстура как
            // тор по Y) обходится без копии перекрытия и без второй текстуры.
            //
            // Условия применимости: полоса не пересоздаётся, её содержимое
            // ВАЛИДНО и того же ключа (иначе перекрывать нечего), а верх полосы
            // ложится на целую device-строку — при дробном dpr номер строки
            // кольца перестал бы быть целым, и кромка поехала бы на полпикселя.
            // Полупрозрачный фон холста кольцу противопоказан: клир полной
            // перерисовки ЗАМЕНЯЕТ пиксель, а квад фона кромки смешивается с
            // тем, что лежало в её строках, — то есть с чужой документной
            // строкой. Случай экзотический (фон холста непрозрачен на всех
            // реальных страницах), и дешевле его отсечь, чем заводить
            // «замещающий» pipeline ради него.
            let opaque_bg = self.canvas_bg.is_none_or(|c| c.a == 255);
            let ring = if !band_ring_enabled() || recreate || !opaque_bg {
                None
            } else {
                self.page_band.as_ref().and_then(|b| {
                    if b.key == 0 || b.key != key {
                        return None;
                    }
                    let row_of = |css: f32| {
                        let px = css * dpr;
                        ((px.round() - px).abs() < 1e-3).then_some(px.round() as i64)
                    };
                    ring_advance_plan(
                        band_h_px,
                        row_of(b.ring_base_css)?,
                        row_of(b.band_top_css)?,
                        row_of(band_top_css)?,
                    )
                })
            };
            if recreate {
                self.create_page_band(sw, band_h_px, band_top_css);
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
            // Кольцо: пасс на кромку (два, если её разрезал край текстуры).
            // Полный промах — один пасс со `strip: None`, ровно как до среза 32.
            let passes: Vec<(f32, Option<BandStrip>)> = match &ring {
                Some(strips) => strips
                    .iter()
                    .map(|s| {
                        // Документный Y строки 0 текстуры для этого пасса:
                        // содержимое кладётся в свои строки обычным сдвигом
                        // рендера, а лишнее отсекает клип кромки.
                        let origin_px = s.doc_y0 - i64::from(s.row0);
                        (origin_px as f32 / dpr, Some(BandStrip { row0: s.row0, rows: s.rows }))
                    })
                    .collect(),
                None => vec![(band_top_css, None)],
            };
            let rows_drawn: u32 = match &ring {
                Some(strips) => strips.iter().map(|s| s.rows).sum(),
                None => band_h_px,
            };
            let mut band_result = Ok(());
            for (origin_css, strip) in passes {
                band_result = self.render_impl(
                    &static_content,
                    &[],
                    origin_css,
                    0.0,
                    RenderPassMode::Band { view: view.clone(), w_px: sw, h_px: band_h_px, strip },
                );
                if band_result.is_err() {
                    break;
                }
            }
            self.depth_texture = saved_depth_t;
            self.depth_view = saved_depth_v;
            band_result?;
            if let Some(b) = self.page_band.as_mut() {
                b.key = key;
                b.band_top_css = band_top_css;
                if ring.is_none() {
                    // Полная перерисовка обнуляет фазу кольца: строка 0
                    // текстуры снова держит документную строку `band_top`.
                    b.ring_base_css = band_top_css;
                }
            }
            ComposeOutcome::Miss.store();
            if crate::frame_log_level() >= 2 {
                eprintln!(
                    "[frame:wgpu] page-compose MISS: band y={band_top_css:.0}..{:.0} css ({sw}x{band_h_px} px, rows {rows_drawn}/{band_h_px}, {} anim segs, frac {}, load {})",
                    band_top_css + band_h_css,
                    ranges.len(),
                    band_draw_fraction().map_or(1.0, f64::from),
                    // Гейт тождества плеч среза 30: какое из плеч рычага
                    // `LUMEN_BAND_PASS_LOAD` реально доехало до пасса полосы.
                    match band_pass_load_ops() {
                        (true, true) => "both",
                        (true, false) => "color",
                        (false, true) => "depth",
                        (false, false) => "none",
                    },
                );
            }
        } else {
            ComposeOutcome::Hit.store();
            if crate::frame_log_level() >= 2 {
                timed_log(|| {
                    eprintln!("[frame:wgpu] page-compose HIT ({} anim segs)", ranges.len());
                });
            }
        }

        // Композиция: blit полосы со сдвигом + overlay поверх. Bind group
        // блита взята готовой из кэша полосы (срез 21) — её входы не зависят
        // ни от скролла, ни от содержимого кадра.
        let Some((band_top_css, ring_base_css, bind_group)) = self
            .page_band
            .as_ref()
            .map(|b| (b.band_top_css, b.ring_base_css, b.blit_bg.clone()))
        else {
            return Ok(false);
        };
        // Фаза кольца: на сколько строк текстуры съехал верх полосы против
        // базы. Ноль (полоса только что перерисована целиком) даёт ровно один
        // квад с uv 0…1 — путь до среза 32.
        let phase_px = (((band_top_css - ring_base_css) * dpr).round() as i64)
            .rem_euclid(i64::from(band_h_px)) as u32;
        self.pending_base_blit = Some(PendingBaseBlit {
            bind_group,
            quads: band_blit_quads(
                band_top_css - scroll_y,
                sw as f32 / dpr,
                band_h_px,
                phase_px,
                dpr,
            ),
        });
        marks.mark(4);
        if marks.printing() {
            // Подстатьи композитора ДО композитного пасса. `skip` — проверки
            // применимости (включая O(n) поиск sticky-слоя), `geom` — размеры
            // полосы, `split` — план анимируемых сегментов, `band` — прогрев,
            // решение попадание/промах и рендер полосы на промахе. Ключа
            // полосы среди статей больше нет: срез 35 свёл его в общий проход
            // по списку, и его цена печатается строкой `frame-hash`.
            let ms = marks.ms;
            timed_log(|| {
                eprintln!(
                    "[frame:wgpu]   compose-top: skip {:.2} geom {:.2} split {:.2} \
                     band {:.2} | {} cmds",
                    ms[0],
                    ms[1] - ms[0],
                    ms[2] - ms[1],
                    ms[4] - ms[3],
                    content.len(),
                );
            });
        }

        // BUG-405 срез 36: overlay — единственное содержимое композитного кадра
        // помимо блита полосы, поэтому вопрос «сколько стоит хром на кадре
        // прокрутки» решается его дайджестом (меняется ли он от кадра к кадру)
        // и плечом рычага (сколько стоит его рисовать). Дайджест считается
        // только под пофазным логом — штатный путь за диагностику не платит.
        if crate::frame_log_level() >= 2 {
            // Целиком внутри `timed_log`: сама эта диагностика — тоже
            // инструмент, и её цена обязана попасть в счётчик инструмента, а
            // не в неназванную работу движка (ровно ловушка среза 34, п. 71).
            timed_log(|| {
                // Дайджесты команд хрома + сколько их изменилось против
                // прошлого кадра: «дайджест кадра другой» и «хром надо
                // перерисовать целиком» — разные утверждения, и кэш хрома
                // имеет смысл ровно настолько, насколько мал `changed`.
                let digests: Vec<u64> = overlay
                    .iter()
                    .map(crate::display_list::hash_one_command)
                    .collect();
                let frame_d = digests.iter().fold(0u64, |acc, d| acc.rotate_left(7) ^ *d);
                let (changed, prev_len, at) = OVERLAY_PREV.with(|p| {
                    let mut prev = p.borrow_mut();
                    // Адрес первой изменившейся команды: следующий срез должен
                    // знать не только «сколько», но и «какая» — от этого
                    // зависит, выкалывается ли она из кэша одним диапазоном.
                    let at = digests.iter().zip(prev.iter()).position(|(a, b)| a != b);
                    let changed = digests
                        .iter()
                        .zip(prev.iter())
                        .filter(|(a, b)| a != b)
                        .count()
                        + digests.len().abs_diff(prev.len());
                    let prev_len = prev.len();
                    *prev = digests;
                    (changed, prev_len, at)
                });
                // Вид команды — по началу её `Debug`: отдельного `kind()` у
                // `DisplayCommand` нет. Первые поля (у прямоугольника это его
                // геометрия) и отвечают, что именно в хроме едет за прокруткой.
                let names: String = at
                    .map(|i| {
                        let dbg: String =
                            format!("{:?}", overlay[i]).chars().take(90).collect();
                        format!(" at {i} {dbg}")
                    })
                    .unwrap_or_default();
                eprintln!(
                    "[frame:wgpu]   overlay: {} cmds digest {frame_d:016x} \
                     changed {changed}/{prev_len}{names}",
                    overlay.len(),
                );
            });
        }

        // BUG-405 срез 44: третий кандидат остатка п. 84 — сборка
        // `seg_content`/`compose_overlay` между решением по `overlay_cache_step`
        // и вызовом `render_impl`, ни статьёй FRAME_PHASE_NANOS (кончаются на
        // mark(4)), ни статьёй `пасс` (начинается своим t_frame0 внутри
        // render_impl) не покрытая.
        let t_post_cache = crate::frame_log_enabled().then(std::time::Instant::now);
        // Split: анимируемые сегменты рисуются как content-полоса Compose-кадра
        // (получают штатный сдвиг -scroll_y) — поверх blit-а, под overlay.
        let seg_content: &[DisplayCommand] = seg_plan.unwrap_or(&[]);
        // BUG-405 срез 41: overlay-кэш — retained текстура СТАБИЛЬНОГО ХВОСТА
        // overlay-списка вместо перерисовки его целиком каждый кадр (порядок
        // не меняется — см. doc-комментарий `OverlayCache`). `overlay_cache_step`
        // сама решает, применим ли фаст-пас, и в этом случае ставит
        // `self.pending_overlay_blit`; `LUMEN_NO_OVERLAY_CACHE` — плечо A/B,
        // не трогает `compose_overlay_disabled()` (та убирает overlay из
        // кадра целиком — другая диагностика).
        let overlay_prefix_len = if compose_overlay_disabled() || overlay_cache_disabled() {
            None
        } else {
            self.overlay_cache_step(
                overlay,
                (!overlay_digest_reuse_disabled()).then_some(overlay_digests).flatten(),
            )?
        };
        let compose_overlay: &[DisplayCommand] = if compose_overlay_disabled() {
            &[]
        } else if let Some(prefix_len) = overlay_prefix_len {
            &overlay[..prefix_len]
        } else {
            overlay
        };
        if let Some(t0) = t_post_cache {
            POST_CACHE_NANOS.fetch_add(
                t0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        self.render_impl(seg_content, compose_overlay, scroll_y, 0.0, RenderPassMode::Compose)?;
        Ok(true)
    }
}
