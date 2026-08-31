//! P1/SPLIT-RN8: census-статики и рычаги переписи BUG-405/BUG-406
//! (флаги `LUMEN_NO_*`/диагностические счётчики), типы uniform-слота
//! скруглённого клипа (`ClipUniformSlot`/`ClipContour`/`clip_slot_from`/
//! `no_clip_slot`) и геттеры метрик `Renderer` — из `renderer.rs`
//! (3018…3998 + 5289…5540 до вырезки). Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-8).

use super::*;


/// Creates a `Depth32Float` texture + view sized `width×height` for GPU depth testing.
/// Called once in `init_pipelines` and on every `resize`.
pub(crate) fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
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
pub(crate) fn select_surface_format(
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
pub(crate) fn frame_skip_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_FRAME_SKIP").is_ok_and(|v| v == "1"))
}

/// BUG-274 diagnostics: total number of `create_texture` calls in this
/// process (all `Renderer`s). Printed by the `LUMEN_FRAME_LOG=2` phase log
/// to correlate pass-end cost with live-resource growth.
pub static TEXTURES_CREATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 21: сколько раз собрана bind group блита полосы
/// скролл-композитора. Гейт правки: группа обязана жить вместе с полосой, а
/// не пересобираться каждый Compose-кадр (прогон прокрутки `lenta.ru`:
/// 40 → 1). Счётчик процессный, как [`TEXTURES_CREATED`].
pub static BAND_BLIT_BGS_CREATED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-274 diagnostics: bump [`TEXTURES_CREATED`].
fn count_texture_created() {
    TEXTURES_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Перепись созданных текстур по `(label, w, h)` — отвечает на вопрос п.23
/// «кто создаёт ~350 текстур за флинг». Заполняется только при
/// `LUMEN_FRAME_LOG=3`; в обычном режиме — один branch поверх счётчика.
pub(crate) type TextureCensusMap = HashMap<(&'static str, u32, u32), u64>;
pub(crate) static TEXTURE_CENSUS: std::sync::OnceLock<std::sync::Mutex<TextureCensusMap>> =
    std::sync::OnceLock::new();

/// Как [`count_texture_created`], но при `LUMEN_FRAME_LOG=3` дополнительно
/// пишет `(label, w, h)` в [`TEXTURE_CENSUS`] (печатается в `alloc:`-блоке).
pub(crate) fn count_texture_created_labeled(label: &'static str, width: u32, height: u32) {
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

/// BUG-405 срез 3: число промахов глиф-атласа (растеризованных глифов) за
/// процесс. Фаза `collect` держит 40–90 мс на кадре перерисовки полосы, и
/// «растеризация впервые показанного текста» была лишь гипотезой — счётчик
/// отделяет её от остального обхода display list.
pub static GLYPHS_RASTERIZED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 3: наносекунды внутри растеризации глифа при промахе атласа
/// (парс outline + `Rasterizer` + вставка в атлас).
pub static GLYPH_RASTER_NANOS: std::sync::atomic::AtomicU64 =
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

/// BUG-405 срез 34: наносекунды, потраченные кадром на ПЕЧАТЬ пофазного лога
/// (`LUMEN_FRAME_LOG=2`, ~12 строк на кадр).
///
/// Печать идёт внутри окна, которое шелл меряет как `[frame] total`, поэтому
/// на дешёвом кадре (попадание скролл-композитора, ~1.2 мс работы) инструмент
/// становится крупнейшей статьёй кадра и завышает его в полтора раза. Без
/// этого счётчика невязка разбивки читается как «неназванная работа движка».
pub static FRAME_LOG_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 44: наносекунды между входом в [`Renderer::render_with_anim`]
/// и стартом секундомера [`ComposeMarks`] — работа, которая идёт ДО первой
/// отсечки `marks.t0` и потому не попадает ни в один слот
/// [`FRAME_PHASE_NANOS`] (те все — дельты ОТ `marks.t0`).
///
/// Кандидат остатка п. 84 (невязка честного кадра попадания не падает до нуля
/// даже при `LUMEN_NO_OVERLAY_CACHE=1`): `recover_exhausted_atlas` и
/// `ComposeOutcome::Skip.store()` выполняются здесь, прежде чем заводится
/// секундомер. Складывается процессно, как [`FRAME_LOG_NANOS`].
pub static PRE_MARKS_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 44: наносекунды между решением `overlay_cache_step` и
/// вызовом `render_impl(..., RenderPassMode::Compose)` внутри `compose_page`
/// (сборка `seg_content`/`compose_overlay`) — второй кандидат остатка п. 84,
/// названный самим текстом пункта. Складывается процессно, как
/// [`FRAME_LOG_NANOS`].
pub static POST_CACHE_NANOS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 45: наносекунды между отсечкой `FRAME_PHASE_NANOS[3]`
/// (конец статьи `пасс`) и фактическим возвратом `render_impl` — диспетчер
/// по `mode` (`FRAMES_RENDERED`/`last_frame_hash`) и запись
/// `pending_readback` идут ПОСЛЕ этой отсечки и не покрыты ни одной другой
/// статьёй. Третий кандидат остатка п. 84 (BUG-405-OPEN.md, срез 44 назвал
/// только «предметки» и «послекэша», этот участок он не проверял). Не
/// включает диагностический блок `if phase_log { .. }` — тот уже посчитан
/// отдельно `FRAME_LOG_NANOS`. Складывается процессно, как
/// [`FRAME_LOG_NANOS`].
pub static TAIL_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// BUG-405 срез 37: подстатьи вызова рендерера в наносекундах, доступные на
/// УРОВНЕ 1 покадрового лога.
///
/// До среза 37 разбивка кадра существовала только под `LUMEN_FRAME_LOG=2`, чей
/// пофазный блок стоит 1.3–3.5 мс на кадр (пункт 71) — то есть больше самого
/// кадра попадания. Счётчик снимает разбивку без единой печати внутри кадра:
/// шелл читает дельту за кадр и печатает её ПОСЛЕ своего таймера.
///
/// Слоты: `0` — подготовка компоновки (применимость, геометрия полосы, план
/// сегментов), `1` — хэш кадра (общий проход среза 35), `2` — решение
/// попадание/промах вместе с рендером полосы на промахе, `3` — сумма
/// wgpu-пассов кадра (`render_impl`, любой режим).
///
/// **Слоты складываются в кадр без пересечений только на ПОПАДАНИИ.** Отсечка
/// `marks[4]` стоит перед композитным `render_impl`, но ПОСЛЕ рендера полосы,
/// поэтому на промахе пасс полосы попадает и в слот 2, и в слот 3, а сумма
/// слотов превышает кадр. Разбирать по слотам можно кадры, у которых
/// [`last_compose`] вернул [`ComposeOutcome::Hit`]; на промахе слот 2 читается
/// как «решение плюс полоса», а слот 3 — как «оба пасса», и складывать их
/// нельзя.
pub static FRAME_PHASE_NANOS: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Разносит отсечки [`ComposeMarks`] по слотам 0–2 [`FRAME_PHASE_NANOS`].
///
/// Вызывается на каждом выходе из [`Renderer::render_with_anim`]: слоты обязаны
/// покрывать кадр целиком, иначе разбивка на уровне 1 молча потеряет путь, по
/// которому кадр ушёл (пропуск тождественного кадра, монолит, попадание).
pub(crate) fn flush_compose_marks(marks: &ComposeMarks) {
    if !marks.enabled() {
        return;
    }
    let add = |i: usize, ms: f64| {
        if let Some(slot) = FRAME_PHASE_NANOS.get(i) {
            slot.fetch_add(
                (ms.max(0.0) * 1e6) as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    };
    add(0, marks.ms[2]);
    add(1, marks.ms[3] - marks.ms[2]);
    add(2, marks.ms[4] - marks.ms[3]);
}

/// Печатает диагностическую строку кадра, относя её цену на [`FRAME_LOG_NANOS`].
///
/// BUG-405 срез 34: строки скролл-композитора печатаются посреди кадра, и без
/// такого учёта их цена оседает в невязке разбивки.
pub(crate) fn timed_log(f: impl FnOnce()) {
    let t = std::time::Instant::now();
    f();
    FRAME_LOG_NANOS.fetch_add(
        t.elapsed().as_nanos() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Reads a diagnostics counter.
pub fn load_counter(c: &std::sync::atomic::AtomicU64) -> u64 {
    c.load(std::sync::atomic::Ordering::Relaxed)
}

/// `true`, если скролл-композитор страницы отключён
/// (`LUMEN_NO_SCROLL_COMPOSITOR=1`). Диагностика: A/B картинки и скорости
/// на одном бинарнике (как `LUMEN_NO_BBOX_SCISSOR`).
/// Сколько элементов плана кадра кодируется в один командный список
/// (BUG-405 срез 2). Кадр перерисовки полосы состоит из десятков пассов, и
/// цена `drop(pass)` растёт по мере накопления списка: одинаковые по
/// дескриптору пассы стоят 0.05 мс в начале кадра и 1.2–2 мс дальше, даже
/// если в них не записано ни одной операции. Подача списка порциями
/// возвращает цену пасса к начальной. Размер подобран замером на живой
/// прокрутке `lenta.ru`: 8 — минимум суммы кадров (4 хуже на 8%, 16 и 32 не
/// отличаются от «без порций» вовсе).
pub(crate) const SUBMIT_CHUNK_ITEMS: usize = 8;

/// `true`, если подача кадра порциями отключена (`LUMEN_NO_SPLIT_SUBMIT=1`) —
/// рычаг отката BUG-405 срез 2 к одному командному списку на кадр.
pub(crate) fn split_submit_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SPLIT_SUBMIT").is_ok_and(|v| v == "1"))
}

/// Слот uniform-буфера группы 0 — CPU-зеркало WGSL-структуры `Uniforms`
/// из [`CLIP_UNIFORM_WGSL`] (BUG-405 срез 4). Все поля в DEVICE px:
/// фрагментный этап берёт позицию из `@builtin(position)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ClipUniformSlot {
    /// Размер вьюпорта в CSS px — им вершинный этап переводит позицию в NDC.
    viewport: [f32; 2],
    /// `scale_factor` кадра: фрагментный этап делит на него
    /// `@builtin(position)`, чтобы получить CSS px.
    dpr: f32,
    _pad0: f32,
    /// Центр прямоугольника клипа (CSS px, экранные координаты).
    clip_center: [f32; 2],
    /// Полуразмер прямоугольника клипа (CSS px).
    clip_half: [f32; 2],
    /// Горизонтальные радиусы углов (tl, tr, br, bl), CSS px.
    clip_radii_x: [f32; 4],
    /// Вертикальные радиусы углов (tl, tr, br, bl), CSS px.
    clip_radii_y: [f32; 4],
    /// Центр ВТОРОГО (вложенного) контура, CSS px (BUG-405 срез 8).
    clip2_center: [f32; 2],
    /// Полуразмер второго контура; `NO_CLIP_HALF` — контура нет.
    clip2_half: [f32; 2],
    /// Горизонтальные радиусы углов второго контура, CSS px.
    clip2_radii_x: [f32; 4],
    /// Вертикальные радиусы углов второго контура, CSS px.
    clip2_radii_y: [f32; 4],
}

/// Полуразмер контура, при котором SDF отрицателен во всей поверхности —
/// «клипа нет». То же число проверяет фрагментный шейдер
/// ([`CLIP_UNIFORM_WGSL`]), решая, считать ли второй `sdf_rrect`.
const NO_CLIP_HALF: f32 = 1.0e7;

/// Сколько скруглённых контуров одновременно держит шейдер (BUG-405 срез 8).
/// Клип глубже этого остаётся на offscreen-уровне: следующий слот стоил бы
/// ещё одного `sdf_rrect` на фрагмент, а вложенность 3+ на живых страницах не
/// встретилась (перепись `lenta.ru`: максимум 2).
pub(crate) const SHADER_CLIP_MAX_CONTOURS: usize = 2;

/// Скруглённый контур активного шейдерного клипа в экранных CSS px
/// (BUG-405 срез 8). Стек этих записей и есть то, что слот uniform'а
/// пересекает произведением покрытий.
#[derive(Clone, Copy)]
pub(crate) struct ClipContour {
    /// Центр прямоугольника клипа.
    pub(crate) center: [f32; 2],
    /// Полуразмер прямоугольника клипа.
    pub(crate) half: [f32; 2],
    /// Горизонтальные радиусы углов (tl, tr, br, bl).
    pub(crate) radii_x: [f32; 4],
    /// Вертикальные радиусы углов (tl, tr, br, bl).
    pub(crate) radii_y: [f32; 4],
}

/// Собирает слот uniform'а из стека активных контуров (BUG-405 срез 8).
/// Пустой стек невозможен — слот 0 строит [`no_clip_slot`].
pub(crate) fn clip_slot_from(
    contours: &[ClipContour],
    viewport: [f32; 2],
    dpr: f32,
) -> ClipUniformSlot {
    let mut slot = no_clip_slot(viewport, dpr);
    if let Some(c) = contours.first() {
        slot.clip_center = c.center;
        slot.clip_half = c.half;
        slot.clip_radii_x = c.radii_x;
        slot.clip_radii_y = c.radii_y;
    }
    if let Some(c) = contours.get(1) {
        slot.clip2_center = c.center;
        slot.clip2_half = c.half;
        slot.clip2_radii_x = c.radii_x;
        slot.clip2_radii_y = c.radii_y;
    }
    slot
}

/// Шаг слота в uniform-буфере. Динамический офсет обязан быть кратен
/// `min_uniform_buffer_offset_alignment` (256 у D3D12/Vulkan/Metal), поэтому
/// 64-байтовый слот кладётся с запасом.
pub(crate) const UNIFORM_SLOT_STRIDE: u64 = 256;

/// «Клипа нет»: полуразмер 1e7 CSS px делает SDF отрицательным во всей
/// поверхности, покрытие — ровно 1.0.
pub(crate) fn no_clip_slot(viewport: [f32; 2], dpr: f32) -> ClipUniformSlot {
    ClipUniformSlot {
        viewport,
        dpr,
        _pad0: 0.0,
        clip_center: [0.0, 0.0],
        clip_half: [NO_CLIP_HALF, NO_CLIP_HALF],
        clip_radii_x: [0.0; 4],
        clip_radii_y: [0.0; 4],
        clip2_center: [0.0, 0.0],
        clip2_half: [NO_CLIP_HALF, NO_CLIP_HALF],
        clip2_radii_x: [0.0; 4],
        clip2_radii_y: [0.0; 4],
    }
}

/// `true`, если шейдерный скруглённый клип отключён
/// (`LUMEN_NO_SHADER_RRECT_CLIP=1`) — рычаг отката BUG-405 срез 4 к
/// offscreen-уровню на каждый `PushClipRoundedRect` и A/B-плечо для проверки
/// пикселей.
pub(crate) fn shader_rrect_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SHADER_RRECT_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если ВТОРОЙ шейдерный контур отключён
/// (`LUMEN_NO_NESTED_SHADER_CLIP=1`) — рычаг отката BUG-405 срез 8 к
/// offscreen-уровню на каждый вложенный `PushClipRoundedRect` и A/B-плечо для
/// проверки пикселей. Отдельный от `LUMEN_NO_SHADER_RRECT_CLIP` затем, что тот
/// снимает шейдерный клип целиком и потому не отделяет срез 8 от среза 4.
pub(crate) fn nested_shader_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_NESTED_SHADER_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если склейка пасса родителя вокруг выброшенного (невидимого)
/// offscreen-уровня отключена (`LUMEN_NO_CULL_MERGE=1`) — рычаг отката
/// BUG-405 срез 5 к прежнему поведению «уровень выброшен из плана, но разрез
/// пасса родителя остался» и A/B-плечо для проверки пикселей.
pub(crate) fn blur_merge_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_BLUR_MERGE").is_ok_and(|v| v == "1"))
}

/// `true`, если аналитическая размытая тень отключена
/// (`LUMEN_NO_SHADOW_ANALYTIC=1`) — рычаг отката BUG-405 срез 7 к
/// offscreen-уровню с блюром на каждую внешнюю тень и A/B-плечо для сверки
/// пикселей.
pub(crate) fn shadow_analytic_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SHADOW_ANALYTIC").is_ok_and(|v| v == "1"))
}

/// Разбирает сигнатуру внешней тени, какой её кладёт `emit_box_shadows`
/// (`display_list.rs`): `PushFilter [Blur(σ)]` → **одна** заливка
/// (`FillRect`/`FillRoundedRect`) → `PopFilter`, между ними ничего.
///
/// Возвращает `(σ, rect, color, radii)`. Всё, что в сигнатуру не укладывается
/// (несколько фильтров, цветовой фильтр рядом с блюром, содержимое из
/// нескольких операций, вложенный уровень), — не тень: такой `PushFilter`
/// уходит прежним путём.
pub(crate) fn box_shadow_body(
    list: &[DisplayCommand],
    push_idx: usize,
    filters: &[FilterFn],
) -> Option<(f32, Rect, Color, CornerRadii)> {
    let [FilterFn::Blur(sigma)] = filters else {
        return None;
    };
    // NaN сюда доезжать не должен, но если доедет — тень уходит прежним путём,
    // а не рисуется квадом с невычислимым радиусом.
    if !sigma.is_finite() || *sigma <= 0.0 {
        return None;
    }
    if !matches!(list.get(push_idx + 2), Some(DisplayCommand::PopFilter)) {
        return None;
    }
    match list.get(push_idx + 1)? {
        DisplayCommand::FillRect { rect, color } => {
            Some((*sigma, *rect, *color, CornerRadii::default()))
        }
        DisplayCommand::FillRoundedRect { rect, color, radii } => {
            Some((*sigma, *rect, *color, *radii))
        }
        _ => None,
    }
}

pub(crate) fn cull_merge_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_CULL_MERGE").is_ok_and(|v| v == "1"))
}

/// Рычаг отката построчной заливки атласа (BUG-405 срез 11).
pub(crate) fn atlas_partial_upload_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_ATLAS_PARTIAL").is_ok_and(|v| v == "1"))
}

/// Слот пре-резолва face-а под команду, которая текстом не является
/// (BUG-771). Вектор пре-резолва адресуется глобальным индексом команды, а не
/// порядковым номером `DrawText`, поэтому нетекстовые слоты обязаны быть
/// заполнены — и заполнены значением, которое нельзя спутать с `face_id`.
pub(crate) const NO_TEXT_FACE: usize = usize::MAX;

/// Пре-резолв primary face_id под каждую команду кадра (BUG-771).
///
/// Длина результата равна `content.len() + overlay.len()`, а слот команды —
/// её собственный глобальный индекс ([`text_face_slot`]); команда, которая
/// текстом не является, получает [`NO_TEXT_FACE`]. Раньше сюда клались
/// только `DrawText`, а читались курсором по мере отрисовки — и первая же
/// команда текста, не дошедшая до своей ветки (viewport-кулинг), сдвигала
/// весь остаток кадра на чужие face-ы.
pub(crate) fn resolve_text_face_ids(
    content: &[DisplayCommand],
    overlay: &[DisplayCommand],
    mut resolve: impl FnMut(&[String], FontWeight, FontStyle, FontStretch) -> usize,
) -> Vec<usize> {
    let mut ids = Vec::with_capacity(content.len() + overlay.len());
    for cmd in content.iter().chain(overlay.iter()) {
        ids.push(match cmd {
            DisplayCommand::DrawText {
                font_family, font_weight, font_style, font_stretch, ..
            } => resolve(font_family, *font_weight, *font_style, *font_stretch),
            _ => NO_TEXT_FACE,
        });
    }
    ids
}

/// Слот команды в векторе [`resolve_text_face_ids`]: полосы склеены в том же
/// порядке, в котором их обходит render-loop (`content`, затем `overlay`).
pub(crate) const fn text_face_slot(is_overlay: bool, cmd_idx: usize, content_len: usize) -> usize {
    if is_overlay { content_len + cmd_idx } else { cmd_idx }
}

/// Диагностика BUG-771: печатать подпись текста overlay-а и атласа глифов на
/// каждом кадре (`LUMEN_TEXT_SIG=1`; `=2` — ещё и по вершине на квад).
/// Отдельно от `LUMEN_FRAME_LOG`, потому что хэш атласа — это проход по
/// мегабайту на кадр.
pub(crate) fn text_sig_level() -> u8 {
    use std::sync::OnceLock;
    static LEVEL: OnceLock<u8> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("LUMEN_TEXT_SIG").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    })
}

/// Рычаг отката отсева повторных команд состояния пасса (BUG-405 срез 10).
pub(crate) fn state_elision_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_STATE_ELISION").is_ok_and(|v| v == "1"))
}

pub(crate) fn scroll_compositor_disabled() -> bool {
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
pub(crate) fn anim_split_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_ANIM_SPLIT").is_ok_and(|v| v == "1")
    })
}

/// `true`, если bbox-scissor фильтр-пассов отключён (`LUMEN_NO_BBOX_SCISSOR=1`).
/// Диагностика: A/B-сравнение картинки и скорости на одном бинарнике.
pub(crate) fn bbox_scissor_disabled() -> bool {
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
pub(crate) fn bbox_backdrop_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BBOX_BACKDROP").is_ok_and(|v| v == "1")
    })
}

/// `true`, если bbox-офскрин блюра element-фильтра отключён
/// (`LUMEN_NO_BBOX_FILTER=1`) — рычаг отката BUG-405 среза 24 к scratch-у
/// размером во всю цель рендера. Плечо A/B: одно и то же плечо и по пикселям
/// (сверка картинки), и по счётчику созданных текстур.
pub(crate) fn bbox_filter_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_BBOX_FILTER").is_ok_and(|v| v == "1"))
}

/// `true`, если сглаживание SVG-супа отключено (`LUMEN_NO_SVG_AA=1`):
/// `DrawSvgFill`/`DrawSvgStroke` рисуются бинарным треугольным супом, как до
/// BUG-277 среза 12. Диагностика: A/B-сравнение картинки и скорости на одном
/// бинарнике (растеризация покрытия идёт на CPU и стоит O(площадь фигуры)).
pub(crate) fn svg_aa_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_SVG_AA").is_ok_and(|v| v == "1"))
}

/// `true`, если сглаживание кромки повёрнутого/скошенного `FillRect` отключено
/// (`LUMEN_NO_ROT_AA=1`): квад рисуется бинарно, как до BUG-277 среза 13.
/// Диагностика: A/B-сравнение картинки и скорости на одном бинарнике
/// (растеризация покрытия идёт на CPU и стоит O(площадь bbox квада)).
pub(crate) fn rot_aa_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_ROT_AA").is_ok_and(|v| v == "1"))
}

/// `true`, если точный клип повёрнутого/скошенного прямоугольника отключён
/// (`LUMEN_NO_ROT_CLIP=1`): `PushClipRect` под поворотом снова режет по AABB
/// трансформированного прямоугольника, как до BUG-277 среза 14. Диагностика:
/// A/B-сравнение картинки и скорости на одном бинарнике (точный клип открывает
/// offscreen-уровень и стоит один composite-пасс на каждый такой клип).
pub(crate) fn rot_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_ROT_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если применение накопленного `PushTransform` к квадам
/// `DrawBackgroundImage`/`DrawCrossFade` отключено (`LUMEN_NO_IMG_XFORM=1`):
/// квады снова кладутся в нетрансформированных координатах, как до BUG-277
/// среза 15. Диагностика: A/B-сравнение картинки на одном бинарнике.
pub(crate) fn img_xform_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_IMG_XFORM").is_ok_and(|v| v == "1"))
}

/// `true`, если mip-цепочка картинок отключена (`LUMEN_NO_IMAGE_MIPS=1`):
/// возврат к CPU-ресайзу под каждый placed-размер (`src@WxH`-зоопарк) и
/// nearest-выбору mip-уровня в сэмплере. Диагностика: A/B-сравнение картинки,
/// скорости и памяти на одном бинарнике.
pub(crate) fn image_mips_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_IMAGE_MIPS").is_ok_and(|v| v == "1")
    })
}

/// `true`, если фоновый прогрев ленивых пайплайнов отключён
/// (`LUMEN_NO_PIPELINE_WARMUP=1`): каждый пайплайн снова компилируется на том
/// кадре, где впервые понадобился (поведение до BUG-405). A/B-рычаг и откат.
pub(crate) fn pipeline_warmup_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_PIPELINE_WARMUP").is_ok_and(|v| v == "1")
    })
}

/// `true`, если направленный сдвиг полосы скролл-композитора отключён
/// (`LUMEN_NO_BAND_BIAS=1`): полоса рецентрируется симметрично (вьюпорт по
/// центру), как до среза. По умолчанию **включён**: при промахе бо́льшая часть
/// запаса полосы кладётся ПО ходу скролла, поэтому непрерывный скролл проходит
/// дальше до следующего промаха (реже полная переросфинкция полосы). Меняет
/// только ПОЛОЖЕНИЕ полосы, не её содержимое — пиксельно идентично симметрии.
/// Диагностика: A/B скорости/p95 на одном бинарнике.
pub(crate) fn band_bias_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BAND_BIAS").is_ok_and(|v| v == "1")
    })
}

/// `true`, если прогрев полосы скролл-композитора отключён
/// (`LUMEN_NO_BAND_WARM=1`): текстуры полосы создаются лениво, на первом же
/// промахе, как до среза 20 BUG-405. По умолчанию **включён**: первая
/// отрисовка в свежую цель стоит на порядок дороже последующих (перепись
/// среза 20 на `lenta.ru`, Vulkan: `drop(pass)` 4.6 мс против 0.15 мс у
/// следующих промахов с той же полосой), и без прогрева эту цену платит
/// первый кадр ПРОКРУТКИ. Прогрев переносит её в кадр загрузки, где уже
/// компилируются пайплайны. Пиксельно нейтрален: прогревающий пасс только
/// чистит текстуру, а ключ полосы остаётся невалидным, поэтому первое
/// реальное использование всё равно перерисовывает её содержимое целиком.
/// Диагностика: A/B на одном бинарнике.
pub(crate) fn band_warm_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_BAND_WARM").is_ok_and(|v| v == "1")
    })
}

/// Запас полосы (CSS px), заданный `LUMEN_BAND_MARGIN_CSS`, либо `None` —
/// штатная формула `min(0.75 · вьюпорт, `[`BAND_MARGIN_CAP_CSS`]`)`.
///
/// Рычаг ПЕРЕПИСИ (BUG-405 срез 27), а не настройка: он меняет высоту полосы
/// при неизменных вьюпорте и содержимом, то есть позволяет измерить, какая
/// часть цены промаха пропорциональна площади полосы, а какая постоянна. Без
/// него площадь полосы нельзя изменить, не меняя заодно окно или страницу, и
/// вопрос «окупится ли инкрементальная дорисовка полосы» (п. 43 остатка)
/// неотличим от разницы стендов. Переопределение ПОЛНОЕ, а не потолок: штатный
/// запас упирается в долю 0.75 вьюпорта раньше, чем в потолок 768 CSS px, —
/// одним лишь потолком свип не поднимает полосу выше штатной. Ограничения,
/// стоящие выше рычага, остаются: ужатие под лимит текстуры и порог
/// [`BAND_MIN_MARGIN_RATIO`]. Нечисловое, неположительное или нефинитное
/// значение игнорируется.
pub(crate) fn band_margin_override_css() -> Option<f32> {
    use std::sync::OnceLock;
    static OVERRIDE: OnceLock<Option<f32>> = OnceLock::new();
    *OVERRIDE.get_or_init(|| {
        std::env::var("LUMEN_BAND_MARGIN_CSS")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
    })
}

/// `true`, если текстура полосы создаётся ПРИГОДНОЙ для копирования
/// (`LUMEN_BAND_COPY_USAGE=1`): к штатным `RENDER_ATTACHMENT | TEXTURE_BINDING`
/// добавляются `COPY_SRC | COPY_DST`.
///
/// Рычаг переписи BUG-405 срез 28. Инкрементальная дорисовка полосы (п. 43
/// остатка) требует от текстуры права быть источником и приёмником копии, а
/// само это право на многих драйверах отключает сжатие цели без потерь — то
/// есть может подорожать сам ПРОМАХ, который правка и удешевляет. Плечи
/// различаются только этими двумя битами, поэтому надбавку промаха до и после
/// меряет один бинарник (`scripts/band_miss_census.py`, `docs/perf-method.md`).
/// Пиксельно нейтрально: биты usage не меняют ни содержимого текстуры, ни
/// одного пути отрисовки.
pub(crate) fn band_copy_usage_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("LUMEN_BAND_COPY_USAGE").is_ok_and(|v| v == "1"))
}

/// Доля высоты полосы, которую разрешено ПЕРЕРИСОВЫВАТЬ на промахе
/// (`LUMEN_BAND_DRAW_FRACTION`, `0 < v < 1`), либо `None` — рисуется вся полоса.
///
/// Рычаг ПЕРЕПИСИ BUG-405 срез 29 (пункт 60 остатка), пиксельно НЕВЕРНЫЙ: ниже
/// доли полоса остаётся пустой, и композит показывает мусор. Вопрос, ради
/// которого он существует: падает ли цена промаха пропорционально числу
/// ПЕРЕРИСОВАННЫХ строк при неизменном размере ЦЕЛИ. Свип среза 27
/// (`LUMEN_BAND_MARGIN_CSS`) менял площадь рисования вместе с размером
/// текстуры, а инкрементальная дорисовка (пункт 43) меняет только первое, и
/// срез 25 уже ловил случай, где цена привязана к объекту текстуры, а не к
/// обработанной площади. Без этого числа выигрыш правки не назван.
///
/// **Срез 33 (2026-08-20) исправил область действия рычага.** До него он
/// понижал только `cull_h`, а `cull_h` кормит ИСКЛЮЧИТЕЛЬНО отсев уровней и
/// scissor: на странице без offscreen-уровней (сплошной текст) рычаг не
/// выбрасывал из кадра полосы ни одной команды и ни одной вершины — свип
/// 0.05…1.0 давал плоскую надбавку при неизменных `vbufs 3293 KiB` и одном и
/// том же числе отсеянных команд. Теперь доля понижает и границу отсева команд
/// (`cull_y1`). Следствие для уже снятых чисел: модель среза 29
/// («надбавка = 2.91 + 8.75 · доля») снята стендом, где переменная часть
/// принадлежала УРОВНЯМ, и переносить её на страницу без уровней нельзя.
///
/// Реализован ПОНИЖЕНИЕМ высоты цели, которую видит отсев Band-рендера
/// ([`Renderer::render_impl`], `cull_h`/`cull_y1`): текстура, пасс и depth
/// остаются полноразмерными, а команды, невидимые уровни и пустые scissor-ы
/// отсекаются по доле —
/// то есть ровно тем механизмом, которым уже отсекается содержимое за
/// пределами полосы. Клип-обёртка вокруг списка (первый заход среза 29)
/// отвергнута замером: она удваивала число элементов плана (`filt` 18 → 38,
/// `draw` 28 → 68, `layers` 1 → 2) и делала промах ВДВОЕ дороже при меньшем
/// числе draw'ов — то есть мерила другую конфигурацию, а не ту же дешевле.
///
/// Ловушка «`SetScissor` из списка затирает scissor пасса» этим путём закрыта
/// сама собой: понижена не команда, а граница, по которой scissor каждой
/// команды считается ([`sync_scissor_to_stack`]).
///
/// Гейт тождества плеч — `frac` в строке `page-compose MISS` и падение `ops`
/// в `[frame:wgpu] total` (`scripts/band_draw_fraction_census.py`).
pub(crate) fn band_draw_fraction() -> Option<f32> {
    use std::sync::OnceLock;
    static FRACTION: OnceLock<Option<f32>> = OnceLock::new();
    *FRACTION.get_or_init(|| {
        std::env::var("LUMEN_BAND_DRAW_FRACTION")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0 && *v < 1.0)
    })
}

/// Высота цели, которую видит отсев Band-рендера, под долей [`band_draw_fraction`].
///
/// Ноль запрещён: `cull_h = 0` схлопнул бы КАЖДЫЙ scissor в пустой и померил бы
/// «полоса не рисуется вовсе», а не «рисуется её доля».
pub(crate) fn band_cull_height(surface_h: u32, frac: f32) -> u32 {
    ((surface_h as f32 * frac).round() as u32).clamp(1, surface_h)
}

/// Какие `LoadOp::Clear` пасса ПОЛОСЫ заменить на `Load` под рычагом переписи
/// (`LUMEN_BAND_PASS_LOAD` = `color` | `depth` | `both`): `(цвет, depth)`.
///
/// Рычаг ПЕРЕПИСИ BUG-405 срез 30 (пункт 62 остатка), пиксельно НЕВЕРНЫЙ:
/// полоса стартует с содержимым прошлого кадра, а depth — с прошлыми
/// значениями. Вопрос, ради которого он существует: из чего состоит
/// ПОСТОЯННАЯ четверть надбавки промаха (2.9 мс из 11.7, срез 29), которую
/// инкрементальная дорисовка (пункт 43) не адресует. Кандидаты пункта 62 —
/// `Clear` цвета всей полосы, `Clear` полноразмерного depth и постоянная цена
/// самого пасса (пункт 5); первые два эта пара битов и снимает по одному,
/// остаток на доле 0.05 с обоими снятыми — третий.
///
/// Меряется вместе с [`band_draw_fraction`] на малой доле: чем меньше
/// рисования, тем меньше искажает измерение единственный побочный эффект
/// `depth`-плеча — старые значения глубины отбраковывают часть фрагментов,
/// то есть удешевляют не только клир, но и рисование. Число draw-команд при
/// этом не меняется, поэтому счётчики работы кадра гейтят тождество плеч, но
/// НЕ ловят этот эффект — отсюда требование малой доли.
pub(crate) fn band_pass_load_ops() -> (bool, bool) {
    use std::sync::OnceLock;
    static CHOICE: OnceLock<(bool, bool)> = OnceLock::new();
    *CHOICE.get_or_init(|| {
        std::env::var("LUMEN_BAND_PASS_LOAD")
            .map_or((false, false), |v| band_pass_load_choice(&v))
    })
}

/// Разбор значения `LUMEN_BAND_PASS_LOAD` в пару «грузить цвет, грузить depth».
///
/// Неизвестное значение — штатный путь (оба клира на месте): рычаг переписи не
/// должен молча менять конфигурацию из-за опечатки в имени плеча.
pub(crate) fn band_pass_load_choice(v: &str) -> (bool, bool) {
    match v.trim() {
        "color" => (true, false),
        "depth" => (false, true),
        "both" => (true, true),
        _ => (false, false),
    }
}

/// Включена ли кольцевая адресация полосы (`LUMEN_BAND_RING=1`).
///
/// **По умолчанию ВЫКЛЮЧЕНА, и это результат замера, а не осторожность**
/// (BUG-405 срез 32, `scripts/band_ring_census.py`). Схема построена и
/// пиксельно верна — промах перерисовывает только вышедшую вперёд кромку
/// (≈60 % строк вместо 100 %), план кадра полосы падает с `draw` 25 / `filt` 16
/// до 16 / 10, а гейт `scripts/band_ring_accept.py` даёт 0.000 % расхождения на
/// всех четырёх стендах, — но цена прокрутки НЕ падает: 6 интерливед-повторов с
/// вращением порядка дали надбавку 13.63 против 13.17 мс/1000 px (+3.5 % при
/// разбросе 5.54), то есть ровно ноль. Экономия на строках (модель среза 29
/// обещала −30 %) съедается вторым пассом там, где кромку режет край текстуры,
/// и уровнями, которые кромка пересекает и потому рисует целиком.
///
/// Рычаг оставлен включаемым: следующему, кто возьмётся за пункт 43, он даёт
/// рабочую реализацию вместо чистого листа — и стенд, на котором она мерилась,
/// заведомо самый неудобный для неё (`bench-static-scroll.html` кладёт по
/// гауссову блюру на строку, то есть цена промаха там принадлежит уровням, а не
/// строкам).
/// `LUMEN_NO_DUAL_HASH=1` — считать два кадровых хэша двумя РАЗДЕЛЬНЫМИ
/// обходами списка, как до среза 35 (BUG-405, пункт 70 остатка).
///
/// Плечо A/B и рычаг отката: работа в обоих плечах одна и та же (хэш кадра для
/// skip-identical и scroll-инвариантный ключ полосы), меряется одной и той же
/// меткой `frame-hash`, поэтому разница плеч — цена самого второго обхода.
/// Значения хэшей у плеч разные, но каждое плечо самосогласовано: обе свёртки
/// сравниваются только с прошлым кадром того же процесса.
pub(crate) fn dual_hash_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_DUAL_HASH").is_ok_and(|v| v != "0"))
}

/// `LUMEN_NO_DL_EPOCH=1` — не переиспользовать свёртку content-части, считать
/// оба кадровых хэша обходом всего списка, как до среза 39 (BUG-405).
///
/// Плечо A/B и рычаг отката: работа плеч различается ровно на обход списка,
/// меряется той же меткой `frame-hash`.
pub(crate) fn dl_epoch_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_DL_EPOCH").is_ok_and(|v| v != "0"))
}

/// `LUMEN_VERIFY_DL_EPOCH=1` — на каждом кадре пересчитывать свёртку заново и
/// сверять с мемоизированной (BUG-405 срез 39).
///
/// Проверка КОНТРАКТА, а не оптимизация: она стоит ровно того обхода, который
/// срез убирает, поэтому включается только для диагностики. Расхождение значит,
/// что кто-то поменял список, не сменив версию, — кадр показал бы устаревшие
/// пиксели. Расхождение печатается, и кадр берёт свежую свёртку.
pub(crate) fn dl_epoch_verify() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("LUMEN_VERIFY_DL_EPOCH").is_ok_and(|v| v != "0"))
}

/// Сколько раз мемоизированная свёртка разошлась с пересчитанной под
/// `LUMEN_VERIFY_DL_EPOCH=1` (BUG-405 срез 39). Ноль за прогон — доказательство
/// того, что контракт версии соблюдён; счётчик читают тесты и диагностика.
pub static DL_EPOCH_MISMATCHES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Сколько кадров переиспользовали свёртку content-части вместо обхода списка
/// (BUG-405 срез 39) — счётчик-гейт среза: перф-выигрыш обязан подтверждаться
/// им, а не настенным временем (`docs/perf-method.md`).
pub static DL_FOLD_REUSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Запомненная свёртка content-части кадровых хэшей и всё, по чему её законно
/// переиспользовать (BUG-405 срез 39).
pub(crate) struct ContentFoldMemo {
    /// Версия списка, объявленная shell-ом на кадре, где свёртка снята.
    pub(crate) epoch: u64,
    /// Адрес начала среза `content` — страховка от подмены списка, о которой
    /// shell не сказал. Не разыменовывается: сравнивается как число.
    pub(crate) ptr: usize,
    /// Длина среза `content` — вторая половина той же страховки.
    pub(crate) len: usize,
    /// Свёртка выколотых диапазонов: у ключа полосы они входят в результат,
    /// поэтому смена набора сегментов обязана инвалидировать свёртку.
    pub(crate) skip_sig: u64,
    /// Сама свёртка: `.0` — для хэша кадра, `.1` — для ключа полосы.
    pub(crate) folds: (u64, u64),
}

/// Подпись набора выколотых диапазонов для [`ContentFoldMemo::skip_sig`].
///
/// O(числа диапазонов), а не O(длины списка): на странице их единицы, поэтому
/// подпись считается каждый кадр и мемоизации не требует.
pub(crate) fn skip_signature(skip: &[std::ops::Range<usize>]) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write_usize(skip.len());
    for r in skip {
        h.write_usize(r.start);
        h.write_usize(r.end);
    }
    h.finish()
}

/// `LUMEN_NO_COMPOSE_OVERLAY=1` — не рисовать overlay в кадре КОМПОЗИЦИИ
/// (BUG-405 срез 36, пункт 76 остатка).
///
/// Рычаг ПЕРЕПИСИ, а не настройка: без overlay кадр пиксельно неверен (хром
/// исчезает), зато разница плеч даёт цену хрома внутри композитного пасса —
/// единственной статьи, которая после среза 35 крупнее хэша. Промах полосы
/// рычаг не трогает вовсе: в полосу overlay не идёт никогда.
///
/// Гейт тождества плеч — счётчик РАБОТЫ, а не эхо самого рычага (пункт 69):
/// `ops` и `cmd-mix draw` в строке кадра обязаны упасть вместе с ним.
pub(crate) fn compose_overlay_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_COMPOSE_OVERLAY").is_ok_and(|v| v != "0"))
}

/// Плечо A/B среза 41: выключает retained overlay-кэш (`Renderer::overlay_cache_step`),
/// оставляя штатную полную перерисовку overlay каждый Compose-кадр — как до
/// среза. Не то же самое, что [`compose_overlay_disabled`]: тот убирает
/// overlay из кадра целиком (диагностика цены), этот меняет ТОЛЬКО механизм
/// отрисовки — пиксели обязаны совпасть побитово (гейт слайса).
pub(crate) fn overlay_cache_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_OVERLAY_CACHE").is_ok_and(|v| v != "0"))
}

/// `LUMEN_NO_OVERLAY_DIGEST_REUSE=1` — не переиспользовать overlay-дайджест
/// кадрового хэша в `overlay_cache_step`, пересчитывать его там заново, как
/// до среза 47 (BUG-405, пункт 83/84 остатка).
///
/// Плечо A/B и рычаг отката: `render_with_anim` считает
/// [`crate::display_list::fold_overlay`] один раз и передаёт результат в оба
/// потребителя (кадровый хэш и `overlay_cache_step`) — этот рычаг заставляет
/// `overlay_cache_step` получить `None` и обойти overlay `hash_one_command`-ом
/// САМ, второй раз за кадр, воспроизводя цену до среза. Работа плеч
/// различается ровно на этот второй обход; счётчик-гейт — статья `послекэша`
/// (`POST_CACHE_NANOS`, срез 44), которая покрывает именно этот вызов.
pub(crate) fn overlay_digest_reuse_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_OVERLAY_DIGEST_REUSE").is_ok_and(|v| v != "0"))
}

/// Наименьший индекс `j ≥ from`, при котором `overlay[..j]` сбалансирован по
/// push/pop (кумулятивная глубина возвращается в ноль) — единственно
/// безопасная точка разреза «живой префикс / кэшируемый хвост»
/// (`Renderer::overlay_cache_step`, BUG-405 срез 41): резать список посреди
/// открытого `Push*` нельзя, ни хвост, ни (тем более) префикс порознь не
/// были бы валидным display list-ом. `from == 0` — пустой префикс всегда
/// безопасен (глубина 0 тривиально ДО первой команды). Возвращает
/// `overlay.len()`, если такой точки за `from` не нашлось (весь остаток
/// списка — один открытый контекст; не должно случаться у валидного
/// списка, но отказ безопаснее паники).
pub(crate) fn balanced_cut_at_or_after(overlay: &[DisplayCommand], from: usize) -> usize {
    if from == 0 {
        return 0;
    }
    let mut depth: i32 = 0;
    for (i, cmd) in overlay.iter().enumerate() {
        depth += crate::overlay_partition::layer_delta(cmd);
        if i + 1 >= from && depth == 0 {
            return i + 1;
        }
    }
    overlay.len()
}

thread_local! {
    /// Дайджесты команд overlay прошлого Compose-кадра — только диагностика
    /// (BUG-405 срез 36): заполняется под `LUMEN_FRAME_LOG=2` и нужна ровно
    /// для одного числа — сколько команд хрома изменилось за кадр прокрутки.
    /// Живёт вне `Renderer`, чтобы штатный путь не носил поля ради переписи.
    pub(crate) static OVERLAY_PREV: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `true`, если подъём `max_texture_dimension_2d` до тира адаптера отключён
/// (`LUMEN_NO_TEXTURE_LIMIT_RAISE=1`): устройство снова запрашивается ровно с
/// `downlevel_defaults()`, как до среза 23 BUG-405. Нужен для интерливед-A/B
/// на одном бинарнике (`docs/perf-method.md`).
pub(crate) fn texture_limit_raise_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_TEXTURE_LIMIT_RAISE").is_ok_and(|v| v == "1")
    })
}

/// Какую сторону текстуры просить у устройства при `adapter_max` от адаптера.
///
/// Отдельной функцией (как [`band_geometry`] в срезе 22), потому что решение
/// целиком арифметическое и его гейтит юнит-тест без GPU. Запрос никогда не
/// превышает того, что обещал адаптер: `request_device` на превышении
/// возвращает ошибку, то есть окно вообще не открылось бы.
///
/// Нижняя граница — `downlevel_defaults()`: адаптер, отдающий меньше, не
/// потянул бы и прежний запрос, поэтому поведение в этом углу не меняется
/// (та же ошибка `request_device`, что и до среза).
pub(crate) fn requested_max_texture_dim(adapter_max: u32, raise: bool) -> u32 {
    let base = wgpu::Limits::downlevel_defaults().max_texture_dimension_2d;
    if !raise {
        return base;
    }
    adapter_max.min(MAX_TEXTURE_DIM_TARGET).max(base)
}

impl Renderer {
    /// Сколько командных списков **кадра** отправил в очередь этот рендер
    /// (BUG-405 срез 2, подача кадра порциями).
    ///
    /// Гейт правки стоит на нём: «кадр из многих пассов подан не одним
    /// списком» — утверждение о механизме, оно не зависит от железа и
    /// загрузки машины, в отличие от времени кадра.
    #[must_use]
    pub fn submissions(&self) -> u64 {
        self.submissions
    }

    /// Сколько скруглённых клипов этот рендер обслужил offscreen-уровнем
    /// (BUG-405 срез 4). Клип, обслуженный шейдерным контуром, счётчик не
    /// двигает — уровня, а значит и трёх его пассов, у него нет.
    ///
    /// Гейт правки стоит на нём, а не на времени кадра: «клип больше не стоит
    /// пассов» — утверждение о механизме.
    #[must_use]
    pub fn rrect_clip_levels(&self) -> u64 {
        self.rrect_clip_levels
    }

    /// Сколько разрезов пасса родителя склеено обратно после выброса
    /// невидимого offscreen-уровня (BUG-405 срез 5).
    ///
    /// Гейт правки стоит на нём вместе с [`Renderer::plan_passes`]: счётчик
    /// называет механизм («разрез отменён»), число пассов — его следствие.
    #[must_use]
    pub fn cull_merges(&self) -> u64 {
        self.cull_merges
    }

    /// Сколько пассов (элементов плана кадра) закодировал этот рендер
    /// (BUG-405 срез 5). Цена одного пасса на глубоком командном списке —
    /// около миллисекунды (срез 2), поэтому «пассов меньше» и есть предмет
    /// правки.
    #[must_use]
    pub fn plan_passes(&self) -> u64 {
        self.plan_passes
    }

    /// Сколько команд состояния пасса не отправлено, потому что нужное
    /// значение там уже стояло (BUG-405 срез 10).
    ///
    /// Гейт правки стоит на нём: команды пасса стоят в `drop(pass)`, где
    /// `wgpu-core` проигрывает их в командный список, — «команд меньше» и есть
    /// предмет правки, а не время кадра.
    #[must_use]
    pub fn state_elisions(&self) -> u64 {
        self.state_elisions
    }

    /// Сколько вызовов `draw` слито с предыдущим (BUG-405 срез 10).
    ///
    /// Второй гейт того же среза: цена `drop(pass)` растёт вместе с числом
    /// операций пасса (перепись: 85 операций — 0.14 мс, 310 — 2.14 мс),
    /// поэтому «draw'ов меньше» — предмет правки наравне с «команд меньше».
    #[must_use]
    pub fn draw_merges(&self) -> u64 {
        self.draw_merges
    }

    /// Включает/выключает отсев повторных команд состояния пасса
    /// (BUG-405 срез 10) на этом рендере.
    ///
    /// Инстансное плечо A/B: тест снимает оба пути в одном процессе и сверяет
    /// пиксели, вместо второго прогона с `LUMEN_NO_STATE_ELISION=1`.
    pub fn set_state_elision_enabled(&mut self, enabled: bool) {
        self.state_elision_enabled = enabled;
    }

    /// Сколько байт пикселей атласа глифов отправлено в GPU этим рендером
    /// (BUG-405 срез 11).
    ///
    /// Гейт правки стоит на нём: заливка атласа — это `queue.write_texture`,
    /// чья цена пропорциональна объёму, поэтому «байт меньше» и есть предмет
    /// правки, а не время кадра.
    #[must_use]
    pub fn atlas_bytes_uploaded(&self) -> u64 {
        self.atlas_bytes_uploaded
    }

    /// Сколько раз атлас глифов заливался в GPU этим рендером
    /// (BUG-405 срез 11).
    ///
    /// Второй счётчик того же среза: байты без числа заливок не отличают
    /// «заливок стало меньше» от «одна заливка стала меньше», а правка среза —
    /// про второе.
    #[must_use]
    pub fn atlas_uploads(&self) -> u64 {
        self.atlas_uploads
    }

    /// Включает/выключает построчную заливку атласа (BUG-405 срез 11) на этом
    /// рендере.
    ///
    /// Инстансное плечо A/B: тест снимает оба пути в одном процессе и сверяет
    /// пиксели, вместо второго прогона с `LUMEN_NO_ATLAS_PARTIAL=1`.
    pub fn set_atlas_partial_upload_enabled(&mut self, enabled: bool) {
        self.atlas_partial_upload_enabled = enabled;
    }

    /// Сколько render-пассов закодировали filter-элементы планов этого
    /// рендера (BUG-405 срез 6).
    ///
    /// Гейт правки стоит на нём: «блюр стоит двух пассов вместо трёх» -
    /// утверждение о механизме, а не о времени кадра.
    #[must_use]
    pub fn filter_passes(&self) -> u64 {
        self.filter_passes
    }

    /// Сколько внешних теней (`box-shadow`) этот рендер нарисовал
    /// аналитически — то есть без offscreen-уровня и его пассов
    /// (BUG-405 срез 7).
    ///
    /// Гейт правки стоит на нём вместе с [`Renderer::filter_passes`]: счётчик
    /// называет механизм («тень рисуется в батче родителя»), а число
    /// filter-пассов — его следствие.
    #[must_use]
    pub fn shadow_draws(&self) -> u64 {
        self.shadow_draws
    }

    /// Сколько вложенных скруглённых клипов этот рендер обслужил ВТОРЫМ
    /// шейдерным контуром, то есть без offscreen-уровня (BUG-405 срез 8).
    ///
    /// Гейт правки стоит на нём вместе с [`Renderer::rrect_clip_levels`]:
    /// счётчик называет механизм («вложенный клип уехал в тот же uniform»), а
    /// ноль уровней — его следствие.
    #[must_use]
    pub fn nested_shader_clips(&self) -> u64 {
        self.nested_shader_clips
    }

    /// Сколько раз покрытие SVG-супа взято готовым из кэша, а не пересчитано
    /// (BUG-405 срез 9), и сколько раз пересчитано.
    ///
    /// Гейт правки стоит на нём: счётчик называет механизм («тот же суп
    /// растеризуется один раз»), а не следствие — время фазы `collect`
    /// зависит ещё и от содержимого страницы.
    #[must_use]
    pub fn coverage_cache_stats(&self) -> (u64, u64) {
        (self.coverage_cache.hits, self.coverage_cache.misses)
    }

    /// Сколько команд SVG получили готовую фигуру из кэша (BUG-405 срез 12),
    /// и сколько её пересчитали.
    ///
    /// Гейт правки стоит на нём: счётчик называет механизм («одна и та же
    /// фигура тесселируется один раз»), а не следствие — время фазы `collect`
    /// зависит ещё и от содержимого страницы.
    #[must_use]
    pub fn svg_shape_cache_stats(&self) -> (u64, u64) {
        (self.svg_shape_cache.hits, self.svg_shape_cache.misses)
    }

    /// Включает/выключает мемоизацию фигур SVG (BUG-405 срез 12).
    ///
    /// Нужен гейту пикселей: сравнить пересчёт с попаданием можно только двумя
    /// плечами в одном процессе. Рычаг процесса `LUMEN_NO_SVG_SHAPE_CACHE=1`
    /// выключает кэш поверх него.
    pub fn set_svg_shape_cache_enabled(&mut self, enabled: bool) {
        self.svg_shape_cache_enabled = enabled;
    }

    /// Попаданий и промахов кэша укладки текста (BUG-405 срез 13) за жизнь
    /// рендерера.
    pub fn text_run_cache_stats(&self) -> (u64, u64) {
        (self.text_run_cache.hits, self.text_run_cache.misses)
    }

    /// Включает/выключает мемоизацию укладки текстового run-а (BUG-405 срез 13).
    ///
    /// Нужен гейту вершин: сравнить укладку с попаданием можно только двумя
    /// плечами в одном процессе. Рычаг процесса `LUMEN_NO_TEXT_RUN_CACHE=1`
    /// выключает кэш поверх него.
    pub fn set_text_run_cache_enabled(&mut self, enabled: bool) {
        self.text_run_cache_enabled = enabled;
    }

    /// Включает/выключает кэш покрытия SVG-супов (BUG-405 срез 9).
    ///
    /// Нужен гейту пикселей: сравнить пересчёт с попаданием в кэш можно только
    /// двумя плечами в одном процессе. Рычаг процесса
    /// `LUMEN_NO_COVERAGE_CACHE=1` выключает кэш поверх него.
    pub fn set_coverage_cache_enabled(&mut self, enabled: bool) {
        self.coverage_cache_enabled = enabled;
    }

    /// Включает/выключает второй шейдерный контур (BUG-405 срез 8).
    ///
    /// Нужен гейту пикселей: сравнить прежний путь через offscreen-уровень с
    /// новым можно только двумя плечами в одном процессе. Рычаг процесса
    /// `LUMEN_NO_NESTED_SHADER_CLIP=1` выключает второй контур поверх него.
    pub fn set_nested_shader_clip_enabled(&mut self, enabled: bool) {
        self.nested_shader_clip_enabled = enabled;
    }

    /// Включает/выключает аналитическую размытую тень (BUG-405 срез 7).
    ///
    /// Нужен гейту пикселей: сравнить прежний трёхпассовый путь с новым можно
    /// только двумя плечами в одном процессе. Рычаг процесса
    /// `LUMEN_NO_SHADOW_ANALYTIC=1` выключает аналитику поверх переключателя.
    pub fn set_shadow_analytic_enabled(&mut self, enabled: bool) {
        self.shadow_analytic_enabled = enabled;
    }

    /// Включает/выключает склейку вертикального прохода блюра с композитом
    /// (BUG-405 срез 6).
    ///
    /// Нужен гейту пикселей: правка обязана давать ту же картинку, а сравнить
    /// это можно только двумя плечами в одном процессе. Рычаг процесса
    /// `LUMEN_NO_BLUR_MERGE=1` выключает склейку поверх этого переключателя.
    pub fn set_blur_merge_enabled(&mut self, enabled: bool) {
        self.blur_merge_enabled = enabled;
    }

    /// Включает/выключает склейку пасса родителя вокруг выброшенного
    /// невидимого уровня (BUG-405 срез 5).
    ///
    /// Нужен гейту идентичности: правка обязана не менять ни одного пикселя,
    /// а проверить это можно только сравнив два плеча. Инстансный
    /// переключатель позволяет снять оба плеча в одном процессе — рычаг
    /// процесса `LUMEN_NO_CULL_MERGE=1` выключает склейку поверх него.
    pub fn set_cull_merge_enabled(&mut self, enabled: bool) {
        self.cull_merge_enabled = enabled;
    }

    /// Сколько ленивых ячеек пайплайнов уже заполнено (BUG-405/406).
    /// Счётчик-интроспектор для тестов: гейт стоит на «прогрев довёл ячейки до
    /// заполненного состояния», а не на времени кадра.
    #[must_use]
    pub fn warmed_pipeline_count(&self) -> usize {
        usize::from(self.circle_pipeline.get().is_some())
            + usize::from(self.mipgen_pipeline.get().is_some())
            + usize::from(self.cross_fade_pipeline.get().is_some())
            + usize::from(self.composite_pipeline.get().is_some())
            + usize::from(self.rrect_clip_pipeline.get().is_some())
            + usize::from(self.path_clip_pipeline.get().is_some())
            + usize::from(self.blend_pipeline.get().is_some())
            + usize::from(self.mask_composite_pipeline.get().is_some())
            + usize::from(self.mask_layer_pipelines.get().is_some())
            + usize::from(self.filter_pipeline.get().is_some())
            + usize::from(self.blur_pipeline.get().is_some())
            // Срезы 6 и 7 добавили ленивые ячейки, но не добавили их сюда —
            // прогрев заполнял 14 ячеек, а счётчик-интроспектор сообщал 12.
            + usize::from(self.blur_composite_pipeline.get().is_some())
            + usize::from(self.shadow_pipeline.get().is_some())
            + usize::from(self.backdrop_blit_pipeline.get().is_some())
    }

}
