//! P1/SPLIT-RN9: vertex/font/cache-типы рендер-пайплайна (17 vertex-структур,
//! `DrawOp`, кэш-структуры `GpuImage`/`PageBandCache`/`OverlayCache`/
//! `OffscreenLayer`/`GpuLayerSnapshot`, font-структуры `CachedGlyph`/
//! `LoadedFace`/`FaceMetrics`/`ParsedFace`/`LazyParsedFaces`) и центральный
//! тип `struct Renderer` — из `renderer.rs` (1653…3018 до вырезки). Хаб
//! группы RN: остальные файлы группы читают `Renderer` и эти типы через
//! `use super::*;`. Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-9).

use super::*;

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct FillVertex {
    pub(crate) pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D elements;
    /// populated from `project_point_z` for 3D-transformed elements (CSS Transforms L2).
    /// Shader maps this to WebGPU NDC depth [0,1] so `CompareFunction::LessEqual` gives
    /// correct occlusion: closer elements (higher z) have lower depth value and win.
    pub(crate) z: f32,
    pub(crate) color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct TextVertex {
    /// Screen position in CSS pixels.
    pub(crate) pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D text;
    /// populated by `apply_affine_to_verts` via `VertexPos::set_depth` when the
    /// glyph quad is under a 3D CSS transform. Shader maps to WebGPU NDC depth
    /// via the same `0.5 - z/20000` formula as `FillVertex`, so depth testing
    /// is consistent across all vertex types in a `preserve-3d` rendering context.
    pub(crate) z: f32,
    pub(crate) uv: [f32; 2],
    pub(crate) color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ImageVertex {
    /// Screen position in CSS pixels.
    pub(crate) pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D images;
    /// populated by `apply_affine_to_verts` for 3D-transformed image quads. Same
    /// NDC mapping as `FillVertex`/`TextVertex` for cross-type depth testing.
    pub(crate) z: f32,
    pub(crate) uv: [f32; 2],
    pub(crate) alpha: f32,
}

/// CSS Images L4 §4 — vertex for the two-texture `cross-fade` blend pipeline.
///
/// Layout (16 bytes): `pos[8] + uv[8]`. The quad covers the destination rect
/// with UVs spanning `[0,0]→[1,1]`; both textures are sampled at the same UV
/// (CSS Images L4 §4.1 — images are stretched to the destination, intrinsic
/// sizes do not participate in the blend). No depth field: the shader writes
/// a fixed mid-plane depth (0.5 NDC) and does not currently take part in
/// preserve-3d cross-type sorting.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CrossFadeVertex {
    /// Screen position in CSS pixels.
    pub(crate) pos: [f32; 2],
    /// UV in `[0,1]×[0,1]` over the destination rect — applied to both
    /// `tex_a` and `tex_b` (CSS Images L4 §4.1: images stretched to fit dest).
    pub(crate) uv: [f32; 2],
}

/// Вершина для SDF-круга. `uv` — нормализованные координаты (-1..1) от центра
/// (quad расширен на 0.5px в каждую сторону). `radius_px` — CSS-радиус точки.
/// Layout: pos(8) + uv(8) + color(16) + radius_px(4) = 36 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CircleVertex {
    /// Screen position in CSS pixels.
    pub(crate) pos: [f32; 2],
    /// UV in [-1,1] over the expanded quad (CSS_radius + 0.5 in each direction).
    pub(crate) uv: [f32; 2],
    /// RGBA color.
    pub(crate) color: [f32; 4],
    /// CSS radius of the dot in pixels (= border_width / 2).
    pub(crate) radius_px: f32,
}

/// Вершина для SDF-скруглённого прямоугольника (`RRECT_SHADER_SRC`).
/// `center`/`half_size`/`radii_x`/`radii_y` одинаковы для всех 6 вершин одного quad-а
/// и передаются как interpolants (константны внутри одного треугольника).
/// Layout: pos(8) + z(4) + color(16) + center(8) + half_size(8) + radii_x(16) + radii_y(16) = 76 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct RRectVertex {
    /// Screen position in CSS pixels.
    pub(crate) pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D rrect;
    /// populated by `apply_affine_to_rrect_verts` for 3D-transformed quads.
    /// Same NDC mapping as `FillVertex` so border-radius backgrounds participate
    /// correctly in cross-type depth testing under CSS Transforms L2 `preserve-3d`.
    pub(crate) z: f32,
    /// RGBA color (linear premultiplied alpha is handled by blend state).
    pub(crate) color: [f32; 4],
    /// Center of the rounded rect in CSS pixels.
    pub(crate) center: [f32; 2],
    /// Half-dimensions of the rect: (width/2, height/2).
    pub(crate) half_size: [f32; 2],
    /// Horizontal corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 5.
    pub(crate) radii_x: [f32; 4],
    /// Vertical corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 6.
    /// Equal to `radii_x` for circular corners; differs for elliptical (`border-radius: H/V`).
    pub(crate) radii_y: [f32; 4],
}

/// Вершина аналитической размытой тени (`SHADOW_SHADER_SRC`, BUG-405 срез 7).
/// Поля `RRectVertex` один в один плюс `sigma` — квад тени шире самой фигуры на
/// радиус ядра, поэтому `pos` и `center`/`half_size` здесь расходятся.
/// Layout: pos(8) + z(4) + color(16) + center(8) + half_size(8) + radii_x(16)
/// + radii_y(16) + sigma(4) = 80 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ShadowVertex {
    /// Позиция вершины квада в CSS px (фигура + запас на ядро блюра).
    pub(crate) pos: [f32; 2],
    /// CSS-глубина, как у [`RRectVertex::z`].
    pub(crate) z: f32,
    /// Цвет тени RGBA (прямая альфа; премультиплицирование — дело blend-state).
    pub(crate) color: [f32; 4],
    /// Центр размываемой фигуры в CSS px.
    pub(crate) center: [f32; 2],
    /// Полуразмеры фигуры (w/2, h/2) в CSS px.
    pub(crate) half_size: [f32; 2],
    /// Горизонтальные радиусы углов в CSS px: [tl, tr, br, bl].
    pub(crate) radii_x: [f32; 4],
    /// Вертикальные радиусы углов в CSS px: [tl, tr, br, bl].
    pub(crate) radii_y: [f32; 4],
    /// σ гауссианы — в тех же единицах, что у пассов блюра (device px).
    pub(crate) sigma: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct CompositeVertex {
    pub(crate) pos: [f32; 2],
    pub(crate) uv: [f32; 2],
    pub(crate) alpha: f32,
}

/// Вершина composite-пасса скруглённого клипа (`RRECT_CLIP_SHADER_SRC`).
/// Как `CompositeVertex` (NDC + UV, без viewport-uniform), плюс параметры
/// контура для `sdf_rrect`: они одинаковы для всех 6 вершин quad-а.
/// Layout: pos(8) + uv(8) + world_pos(8) + center(8) + half_size(8)
/// + radii_x(16) + radii_y(16) = 72 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct RRectClipVertex {
    /// Position in NDC (clip-space), same convention as `CompositeVertex::pos`.
    pub(crate) pos: [f32; 2],
    /// UV into the offscreen level texture.
    pub(crate) uv: [f32; 2],
    /// Screen position in CSS pixels — the space `center`/`half_size` live in.
    pub(crate) world_pos: [f32; 2],
    /// Center of the clip contour in CSS pixels.
    pub(crate) center: [f32; 2],
    /// Half-dimensions of the clip rect: (width/2, height/2).
    pub(crate) half_size: [f32; 2],
    /// Horizontal corner radii in CSS pixels: [tl, tr, br, bl].
    pub(crate) radii_x: [f32; 4],
    /// Vertical corner radii in CSS pixels: [tl, tr, br, bl].
    pub(crate) radii_y: [f32; 4],
}

/// Вершина composite-пасса формы `clip-path` (`PATH_CLIP_SHADER_SRC`).
/// Как `CompositeVertex` (NDC + UV), плюс экранная позиция в CSS px — форма
/// живёт в uniform-буфере, поэтому per-vertex параметров контура тут нет.
/// Layout: pos(8) + uv(8) + world_pos(8) = 24 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct PathClipVertex {
    /// Position in NDC (clip-space), same convention as `CompositeVertex::pos`.
    pub(crate) pos: [f32; 2],
    /// UV into the offscreen level texture.
    pub(crate) uv: [f32; 2],
    /// Screen position in CSS pixels — the space the shape lives in.
    pub(crate) world_pos: [f32; 2],
}

/// CPU-зеркало WGSL `ShapeUniform` из [`PATH_CLIP_SHADER_SRC`].
/// Все координаты — экранные CSS px (накопленный transform уже применён).
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct PathClipParamsCpu {
    /// [вид формы (0 = эллипс, 1 = полигон), число вершин, even-odd, pad].
    pub(crate) header: [u32; 4],
    /// [cx, cy, pad, pad] — центр эллипса.
    pub(crate) center: [f32; 4],
    /// Обратная матрица (row-major) отображения «единичный круг → эллипс».
    pub(crate) inv_m: [f32; 4],
    /// Вершины полигона, по две точки на `vec4`.
    pub(crate) verts: [[f32; 4]; PATH_CLIP_MAX_VERTS / 2],
}

/// CSS Masking L1 §4 — вершина mask-composite пайплайна.
/// `pos` — pixel-space (convert to NDC via viewport uniform).
/// `uv_mask` — UV [0,1]×[0,1] в пределах одной плитки mask-изображения.
/// `uv_layer` вычисляется в вершинном шейдере из `pos / viewport`.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct MaskVertex {
    pub(crate) pos: [f32; 2],
    pub(crate) uv_mask: [f32; 2],
}

/// CPU-side зеркало WGSL `FilterEntry` (kind:u32, amount:f32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct FilterEntryCpu { pub(crate) kind: u32, pub(crate) amount: f32, pub(crate) _p0: u32, pub(crate) _p1: u32 }

/// CPU-side зеркало WGSL `FilterParams` (16 bytes header + 8×FilterEntry = 144 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct FilterParamsCpu {
    pub(crate) count: u32, pub(crate) _pad0: u32, pub(crate) _pad1: u32, pub(crate) _pad2: u32,
    pub(crate) entries: [FilterEntryCpu; 8],
}

/// CPU-side зеркало WGSL `BlurParams` (sigma:f32, direction:u32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct BlurParamsCpu { pub(crate) sigma: f32, pub(crate) direction: u32, pub(crate) _p0: u32, pub(crate) _p1: u32 }

/// CSS Images L3 §3.3 — вершина градиентного пайплайна.
/// `uv` — нормализованные координаты [0,1]×[0,1] внутри прямоугольника градиента,
/// бейкятся в вершины, чтобы фрагментный шейдер не нуждался в размерах rect в uniform.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct GradVertex {
    /// CSS pixel position.
    pub(crate) pos: [f32; 2],
    /// Normalized rect coords: (0,0)=TL, (1,1)=BR.
    pub(crate) uv: [f32; 2],
}

/// CPU-side зеркало WGSL `GradStop` (color: vec4 + pos: f32 + 12 bytes pad = 32 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct GradStopCpu {
    pub(crate) color: [f32; 4],
    pub(crate) pos: f32,
    pub(crate) _p0: f32, pub(crate) _p1: f32, pub(crate) _p2: f32,
}

/// CPU-side зеркало заголовка WGSL `GradParams` — 32 байта, ровно до
/// runtime-sized массива стопов (WGSL требует выравнивания `array<GradStop>`
/// на 16 байт, заголовок уже кратен). Стопы дописываются в тот же storage
/// buffer сразу за заголовком, см. [`GradParamsCpu`] и `write_grad_buffer`.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct GradHeaderCpu {
    /// Linear: (sx, sy) — gradient-line start in UV [0,1].
    /// Radial: (cx, cy) — center in UV [0,1].
    /// Conic:  (cx, cy) — center in UV [0,1].
    pub(crate) p0: [f32; 2],
    /// Linear: (ex, ey) — gradient-line end (used with p0 start).
    /// Radial: (rx, ry) — farthest-corner semi-axes in UV [0,1].
    /// Conic:  (w, h) — box dimensions in CSS pixels (for box-space angle).
    pub(crate) p1: [f32; 2],
    pub(crate) n_stops: u32,
    /// 0 = linear, 1 = radial, 2 = conic.
    pub(crate) kind: u32,
    /// 0 = clamp, 1 = repeating (fold t into the `[first, last]` stop period).
    pub(crate) repeating: u32,
    /// Conic: starting angle in radians (0 = top, CW). Linear: box aspect
    /// `h/w` (see [`box_aspect`]). Unused for radial.
    pub(crate) param0: f32,
}

/// Полное описание одного `DrawLinearGradient`/`DrawRadialGradient`/
/// `DrawConicGradient` для GPU: заголовок + список стопов ПРОИЗВОЛЬНОЙ длины.
///
/// Длина не ограничена: стопы уезжают в storage buffer (`write_grad_buffer`),
/// а не в uniform-массив фиксированного размера. Прежний потолок в 16 стопов
/// обрезался молча, из-за чего хвост любого градиента с
/// `<color-interpolation-method>` (17 стопов после полифилла) заливался
/// плоским цветом 16-го стопа — BUG-277 срез 11.
#[derive(Clone)]
pub(crate) struct GradParamsCpu {
    /// Скалярная часть — байт-в-байт заголовок WGSL-структуры.
    pub(crate) header: GradHeaderCpu,
    /// Стопы в порядке возрастания позиции; пишутся со смещения 32.
    pub(crate) stops: Vec<GradStopCpu>,
}

/// Конвертирует `FilterFn` в `FilterEntryCpu` для GPU uniform.
/// Blur (kind=0) передаётся как is; color-filter pass пропускает его по kind.
pub(crate) fn filter_fn_to_entry(f: &FilterFn) -> FilterEntryCpu {
    let (kind, amount) = match f {
        FilterFn::Blur(v)       => (0u32, *v),
        FilterFn::Brightness(v) => (1,    *v),
        FilterFn::Contrast(v)   => (2,    *v),
        FilterFn::Grayscale(v)  => (3,    *v),
        FilterFn::HueRotate(v)  => (4,    *v),
        FilterFn::Invert(v)     => (5,    *v),
        FilterFn::Opacity(v)    => (6,    *v),
        FilterFn::Saturate(v)   => (7,    *v),
        FilterFn::Sepia(v)      => (8,    *v),
    };
    FilterEntryCpu { kind, amount, _p0: 0, _p1: 0 }
}

/// Атомарная команда render-pass-а после сборки display list-а. Каждый
/// DisplayCommand → один (рисующий) DrawOp; PushClipRect/PopClip → отдельные
/// SetScissor (если scissor реально меняется). Render-pass проходит список
/// линейно: SetScissor вызывает `pass.set_scissor_rect`, Fill/Text/Image
/// — соответствующий pipeline + draw на указанный диапазон вершин.
/// `image_batch_idx` индексирует `image_batches[i].bind_group` (Vec на
/// уровне render(), не клонируется в DrawOp).
pub(crate) enum DrawOp {
    SetScissor(DeviceScissor),
    /// BUG-405 срез 4: сменить активный скруглённый клип — номер слота в
    /// uniform-буфере группы 0 (0 = клипа нет). Пассов не добавляет: смена
    /// bind group живёт внутри уже открытого пасса, в отличие от
    /// offscreen-уровня, который требовал своего.
    SetClip(u32),
    Fill { v_start: u32, v_count: u32 },
    Circle { v_start: u32, v_count: u32 },
    /// SDF rounded-rect draw — uses `rrect_pipeline` + `rrect_vbuf`.
    RRect { v_start: u32, v_count: u32 },
    /// BUG-405 срез 7: аналитическая размытая rrect (`box-shadow`) —
    /// `shadow_pipeline` + `shadow_vbuf`. Пассов не добавляет: рисуется внутри
    /// батча родителя, в отличие от прежнего offscreen-уровня с блюром.
    Shadow { v_start: u32, v_count: u32 },
    Text { v_start: u32, v_count: u32 },
    Image { v_start: u32, v_count: u32, image_batch_idx: u32 },
    /// CSS Images L3 §3.3 — linear or radial gradient quad. `grad_batch_idx`
    /// indexes into the per-frame `grad_bind_groups` Vec.
    Gradient { v_start: u32, v_count: u32, grad_batch_idx: u32 },
    /// CSS Images L4 §4 — `cross-fade(A, B, p)` two-texture blend quad.
    /// `cf_batch_idx` indexes into the per-frame `cross_fade_bind_groups` Vec
    /// (one bind group per command: holds both textures + sampler + progress
    /// uniform). Pipeline: `cross_fade_pipeline`.
    CrossFade { v_start: u32, v_count: u32, cf_batch_idx: u32 },
}

/// GPU-ресурсы для одной зарегистрированной картинки. Texture хранит уже
/// декодированные пиксели в формате `Rgba8Unorm` (Gray / GrayA / Rgb
/// конвертируются в Rgba при upload-е); bind group привязан к
/// `image_bind_group_layout` + общему sampler-у renderer-а. Intrinsic
/// dimensions (`width` / `height` в пикселях) хранятся для расчёта
/// `object-fit` / `object-position` на стадии рендеринга.
#[derive(Clone)]
pub(crate) struct GpuImage {
    /// Linear (bilinear) filtered bind group — default for auto/smooth.
    pub(crate) bind_group_linear: wgpu::BindGroup,
    /// Nearest-neighbor filtered bind group — used for pixelated/crisp-edges.
    pub(crate) bind_group_nearest: wgpu::BindGroup,
    /// Texture view (needed for mask-composite bind group creation in render loop).
    pub(crate) view: wgpu::TextureView,
    // texture держим как поле — wgpu освобождает GPU-память когда дропается
    // последняя ссылка; bind_group её не держит.
    pub(crate) _texture: wgpu::Texture,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Скролл-композитор страницы (EXPERIMENT.md §2, срез 1): персистентная
/// текстура «полосы» документа — вьюпорт плюс запас сверху и снизу,
/// растеризованная в документных координатах (scroll-инвариантно). Пока
/// вьюпорт остаётся внутри полосы и содержимое не меняется, кадр скролла =
/// один blit этой текстуры со сдвигом + overlay, без перерисовки страницы.
pub(crate) struct PageBandCache {
    /// Держит GPU-память полосы (wgpu освобождает её при дропе последней
    /// ссылки; view её не удерживает — как `GpuImage::_texture`).
    pub(crate) _texture: wgpu::Texture,
    /// View полосы — источник blit-а и цель Band-рендера.
    pub(crate) view: wgpu::TextureView,
    /// Scroll-инвариантный ключ содержимого: хэш content-полосы display
    /// list-а при scroll (0,0) + `content_generation` + геометрия полосы.
    /// Урок EXPERIMENT.md п.15: скролл в ключе = промах каждый кадр.
    pub(crate) key: u64,
    /// Y верхнего края полосы в документных CSS px (≥ 0).
    pub(crate) band_top_css: f32,
    /// База кольцевой адресации: документный Y (CSS px), лежащий в строке 0
    /// текстуры. Совпадает с `band_top_css` сразу после ПОЛНОЙ перерисовки и
    /// расходится с ним по мере инкрементальных сдвигов: строка текстуры
    /// `(y − ring_base_css)·dpr mod h_px` держит документную строку `y`
    /// (BUG-405 срез 32, пункт 58 остатка).
    pub(crate) ring_base_css: f32,
    /// Ширина текстуры полосы в device px (= ширине surface).
    pub(crate) w_px: u32,
    /// Высота текстуры полосы в device px (surface + 2×запас).
    pub(crate) h_px: u32,
    /// Bind group блита полосы (`image_bgl`: view полосы + linear sampler).
    /// Оба входа живут ровно столько же, сколько сама полоса, поэтому
    /// группа создаётся вместе с ней, а не на каждый Compose-кадр
    /// (BUG-405 срез 21: 40 дескрипторных наборов за прогон прокрутки).
    pub(crate) blit_bg: wgpu::BindGroup,
    /// Depth-текстура Band-рендера (обязана совпадать размером с полосой).
    /// Кэшируется вместе с полосой: раньше создавалась заново на каждый
    /// miss (7+ МБ Depth32 на band-размере — чистый churn VRAM).
    pub(crate) depth_t: wgpu::Texture,
    /// View depth-текстуры полосы.
    pub(crate) depth_v: wgpu::TextureView,
}

/// Retained-текстура СТАБИЛЬНОГО ХВОСТА overlay-списка (BUG-405 срез 41).
///
/// Архитектура среза 40 (п.76) предполагала обратное — вынести горячую
/// команду и реплеить её ПОВЕРХ всего остального. Перепись самой правки
/// (`overlay-cache` плечо `compose_pass_census.py`) показала, что этот план
/// на реальном хроме Lumen не срабатывает НИ РАЗУ за 400 тиков стенда:
/// `anim_split_compose_plan` честно бракует реплей «поверх всего», потому
/// что скроллбар (обычно `overlay[1]`) геометрически пересекается с фоновой
/// панелью хедера (`overlay[3]`, рисуется ПОЗЖЕ и накрыла бы его) —
/// изменение ОДНОЙ команды не означает, что её можно вынести из порядка.
///
/// Эта версия порядок не меняет вовсе: НЕСТАБИЛЬНЫЙ ПРЕФИКС списка
/// (`overlay[..prefix_len]`, на практике 1-2 команды скроллбара у самого
/// начала) рисуется живьём каждый кадр как раньше; СТАБИЛЬНЫЙ ХВОСТ
/// (`overlay[prefix_len..]`, подавляющее большинство команд) остаётся на
/// своём месте в порядке — блитуется той же текстурой, если совпадает с
/// той, что была на момент постройки. Painter's-order безопасность не
/// требует НИКАКОГО геометрического анализа: раз относительный порядок не
/// меняется, а `prefix_len` выбирается на границе сбалансированного
/// push/pop (`balanced_cut_at_or_after`), результат идентичен полной
/// перерисовке по построению.
pub(crate) struct OverlayCache {
    /// Держит GPU-память текстуры (см. `PageBandCache::_texture`). Отдельный
    /// `view` не хранится — эта текстура никогда не перерисовывается
    /// повторно (устарела → строится СОВСЕМ новая, вместе с новым view),
    /// поэтому view нужен только на момент постройки — он живёт внутри
    /// `blit_bg` (bind group держит свою ссылку на него).
    pub(crate) _texture: wgpu::Texture,
    /// Bind group блита (`image_bgl`: view + linear sampler), переиспользует
    /// [`Renderer::create_band_blit_bind_group`] — оба входа те же, что у
    /// блита полосы.
    pub(crate) blit_bg: wgpu::BindGroup,
    /// Ширина/высота текстуры в device px (= размеру поверхности — overlay
    /// viewport-locked, полосы у него нет).
    pub(crate) w_px: u32,
    pub(crate) h_px: u32,
    /// Digest хвоста (`overlay[prefix_len..]`, `hash_one_command` на
    /// элемент) НА МОМЕНТ постройки — сравнивается с
    /// `current_overlay_digests[prefix_len..]` БЕЗ сдвига индексов.
    pub(crate) tail_digests: Vec<u64>,
    /// Длина живого префикса. Текстура содержит РОВНО `overlay[prefix_len..]`
    /// в исходном относительном порядке.
    pub(crate) prefix_len: usize,
}

/// Одноразовая инъекция blit-квада полосы в начало draw-плана level 0
/// следующего `render_impl`-вызова (Compose-путь скролл-композитора).
pub(crate) struct PendingBaseBlit {
    /// Bind group `image_bgl` поверх текстуры полосы (linear sampler).
    pub(crate) bind_group: wgpu::BindGroup,
    /// Квады блита: прямоугольник в CSS px кадра плюс uv-угол текстуры полосы.
    /// Штатно один (uv `[0,0]`…`[1,1]`, вся полоса со сдвигом
    /// `band_top_css − scroll_y`); при ненулевой фазе кольца (срез 32) — два,
    /// по обе стороны шва, чтобы не заводить sampler с `Repeat`.
    pub(crate) quads: Vec<(Rect, [f32; 2], [f32; 2])>,
}

/// Всё, что кадр обязан знать о полосе ДО хэширования списка (BUG-405 срез 35).
///
/// Результат `Renderer::prepare_page_compose`: путь компоновки применим, вот
/// его геометрия и план static/animated split-а. Ключ полосы считается по
/// `sw`/`band_h_px` и `ranges`, поэтому подготовка обязана быть раньше хэша.
pub(crate) struct ComposePrep {
    /// Ширина полосы (= ширина поверхности), device px.
    pub(crate) sw: u32,
    /// Масштаб поверхности (device px на CSS px).
    pub(crate) dpr: f32,
    /// Запас полосы за пределами вьюпорта, CSS px (половина полного запаса).
    pub(crate) margin_css: f32,
    /// Высота полосы, device px.
    pub(crate) band_h_px: u32,
    /// Высота полосы, CSS px.
    pub(crate) band_h_css: f32,
    /// Высота вьюпорта, CSS px.
    pub(crate) vp_h_css: f32,
    /// Effective-диапазоны анимируемых сегментов (пусто = split не применён).
    pub(crate) ranges: Vec<std::ops::Range<usize>>,
    /// План реплея сегментов поверх блита (`None` = сегментов нет).
    pub(crate) seg_plan: Option<crate::display_list::DisplayList>,
}

/// Секундомер подстатей кадра компоновки (`compose-top`, BUG-405 срез 34).
///
/// Метки берутся только под `LUMEN_FRAME_LOG=2` — как и весь пофазный лог;
/// без него `mark` вырождается в проверку `Option`. Живёт в кадре, а не в
/// одной функции: срез 35 разнёс подготовку, хэш и саму компоновку по трём
/// вызовам, а печатается разбивка по-прежнему одной строкой.
pub(crate) struct ComposeMarks {
    /// Начало отсчёта; `None` — лог выключен, метки не берутся.
    pub(crate) t0: Option<std::time::Instant>,
    /// Накопленные отсечки от `t0`, мс: skip / geom / split / hash / band.
    pub(crate) ms: [f64; 5],
}

impl ComposeMarks {
    /// Заводит секундомер, если пофазный лог включён.
    ///
    /// BUG-405 срез 37: порог опущен со 2 до 1. Метки нужны не только своим
    /// печатным строкам (они остались на уровне 2), но и счётчикам
    /// [`FRAME_PHASE_NANOS`], по которым кадр раскладывается на УРОВНЕ 1 — там,
    /// где надбавки пункта 71 нет.
    pub(crate) fn new() -> Self {
        Self {
            t0: crate::frame_log_enabled().then(std::time::Instant::now),
            ms: [0.0; 5],
        }
    }

    /// Взяты ли метки (уровень ≥ 1). Печать своих строк требует уровня 2 —
    /// см. [`ComposeMarks::printing`].
    pub(crate) fn enabled(&self) -> bool {
        self.t0.is_some()
    }

    /// Печатать ли пофазные строки компоновки (уровень ≥ 2).
    pub(crate) fn printing(&self) -> bool {
        self.t0.is_some() && crate::frame_log_level() >= 2
    }

    /// Отсечка `i`-й подстатьи.
    pub(crate) fn mark(&mut self, i: usize) {
        if let Some(t0) = self.t0
            && let Some(slot) = self.ms.get_mut(i)
        {
            *slot = t0.elapsed().as_secs_f64() * 1e3;
        }
    }
}

/// Строки цели Band-рендера, которые пассу РАЗРЕШЕНО перерисовать
/// (BUG-405 срез 32, кольцевая адресация полосы — пункт 58 остатка).
/// `None` в [`RenderPassMode::Band`] = вся полоса, штатный полный промах.
///
/// Клип строк заводится не scissor-ом пасса, а базовым элементом `clip_stack`
/// (`SetScissor` из списка иначе затирает scissor пасса — ловушка пункта 60):
/// так его наследует и отсев команд, и scissor каждого батча, и композиты
/// offscreen-уровней.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BandStrip {
    /// Первая перерисовываемая строка текстуры полосы, device px.
    pub(crate) row0: u32,
    /// Сколько строк перерисовывается, device px (> 0).
    pub(crate) rows: u32,
}

/// Финальная цель одного `render_impl`-вызова.
pub(crate) enum RenderPassMode {
    /// Обычный кадр: present, `FRAMES_RENDERED`, обновление `last_frame_hash`.
    Normal {
        /// Тотальный хэш кадра, посчитанный в `render()` (для skip-identical).
        frame_hash: u64,
    },
    /// Оффскрин-рендер полосы страницы: без present, без счётчиков кадров,
    /// без `last_frame_hash`; размеры «поверхности» = размеры полосы.
    Band {
        /// Цель рендера (view текстуры полосы).
        view: wgpu::TextureView,
        /// Ширина полосы в device px.
        w_px: u32,
        /// Высота полосы в device px.
        h_px: u32,
        /// Кромка кольца: какие строки полосы пасс перерисовывает.
        /// `None` — вся полоса (клир цвета, как до среза 32).
        strip: Option<BandStrip>,
    },
    /// Композиция кадра из готовой полосы (через `pending_base_blit`) +
    /// overlay: present и `FRAMES_RENDERED`, но `last_frame_hash` обновляет
    /// вызывающий (`render()`) — хэш Compose-аргументов не описывает кадр.
    Compose,
    /// Оффскрин-рендер retained-текстуры стабильного хвоста overlay-списка
    /// (BUG-405 срез 41). Без present, без счётчиков кадров; в отличие от
    /// [`RenderPassMode::Band`] клир ВСЕГДА прозрачный (уровень 0 тоже, а не
    /// только offscreen-уровни) — это UI-хром поверх страницы, а не
    /// непрозрачный фон документа.
    OverlayCache {
        /// Цель рендера (view текстуры кэша).
        view: wgpu::TextureView,
        /// Ширина текстуры в device px (= ширина поверхности).
        w_px: u32,
        /// Высота текстуры в device px (= высота поверхности).
        h_px: u32,
    },
}

/// Чем кончился путь компоновки (скролл-композитор) на последнем кадре —
/// BUG-405 срез 37. Взводится [`Renderer::compose_page`], читается
/// [`last_compose`].
///
/// Процессный счётчик, а не поле рендерера: рендерер живёт в отдельном потоке
/// за прокси (`ThreadedRenderBackend`), и шеллу его состояние иначе не видно.
/// Значения — дискриминанты [`ComposeOutcome`].
static COMPOSE_OUTCOME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Чем кончился путь компоновки (скролл-композитор) на кадре — BUG-405 срез 37.
///
/// Нужен переписи, а не движку: до среза 37 кадр ПОПАДАНИЯ опознавался строкой
/// `page-compose HIT`, а она печатается только под `LUMEN_FRAME_LOG=2`, чей
/// пофазный блок стоит 1.3–3.5 мс на кадр (пункт 71 остатка). Разбивку шелла
/// надо снимать на уровне 1, где такой строки нет — и признак попадания берётся
/// отсюда.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum ComposeOutcome {
    /// Путь компоновки не применялся (монолитный кадр или отказ подготовки).
    #[default]
    Skip = 0,
    /// Попадание: кадр собран блитом готовой полосы плюс overlay.
    Hit = 1,
    /// Промах: полоса перерисована этим кадром, затем композиция.
    Miss = 2,
}

impl ComposeOutcome {
    /// Короткая метка для строки пофазного лога шелла.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Skip => "-",
            Self::Hit => "hit",
            Self::Miss => "miss",
        }
    }

    /// Записать исход текущего кадра в процессный счётчик.
    pub(crate) fn store(self) {
        COMPOSE_OUTCOME.store(self as u8, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Исход пути компоновки на последнем отрисованном кадре (BUG-405 срез 37).
///
/// Читается шеллом ради одной строки пофазного лога: разбивку кадра попадания
/// надо снимать на уровне 1, где строк `page-compose HIT/MISS` нет (пункт 71 —
/// печать уровня 2 крупнее самого кадра попадания).
#[must_use]
pub fn last_compose() -> ComposeOutcome {
    match COMPOSE_OUTCOME.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ComposeOutcome::Hit,
        2 => ComposeOutcome::Miss,
        _ => ComposeOutcome::Skip,
    }
}

/// GPU-ресурсы одного off-screen opacity layer-а. Создаётся лениво через
/// `ensure_layer_textures`; переиспользуется пока размер surface не меняется.
/// `texture` хранится pub чтобы можно было использовать в
/// `encoder.copy_texture_to_texture` для blend-mode compositing.
pub struct OffscreenLayer {
    /// GPU texture resource.
    pub texture: wgpu::Texture,
    /// Texture view for rendering operations.
    pub view: wgpu::TextureView,
    /// Bind group for composite operations.
    pub bind_group: wgpu::BindGroup,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

/// GPU-снимок слоя, загруженный из CPU-пикселей через
/// `Renderer::upload_layer_snapshot`. Хранит `Rgba8Unorm`-текстуру
/// (COPY_DST | TEXTURE_BINDING) и bind group для `image_bgl`,
/// позволяя рендерить снимок через image-pipeline как позиционированный quad.
///
/// Bind group использует `image_bgl` (а не `composite_bgl`), чтобы
/// переиспользовать существующую image-pipeline с поддержкой rect/alpha.
pub(crate) struct GpuLayerSnapshot {
    // texture держим даже без явного обращения — wgpu освобождает GPU-память
    // когда дропается последняя ссылка; bind_group её не держит.
    pub(crate) _texture: wgpu::Texture,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) width: u32,
    pub(crate) height: u32,
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
    /// (живое окно — [`MAX_TEXTURE_DIM_TARGET`] или потолок адаптера, что
    /// меньше; headless-устройство — 2048 из `downlevel_defaults`).
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
pub(crate) struct CachedGlyph {
    pub(crate) entry: GlyphEntry,
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) advance_native: u16,
}

/// Один загруженный face: TTF-байты + owned-кэш метрик, построенный один
/// раз при загрузке (образец — cosmic-text `FontSystem`: fontdb парсит
/// метаданные однажды, дальше живут кэши).
/// face_id 0 — default (bundled, передан в `Renderer::new`); остальные
/// `face_id` назначаются по мере lazy-загрузки из путей `FaceRecord`.
pub(crate) struct LoadedFace {
    /// Байты sfnt-шрифта. `Arc<[u8]>` (BUG-272 срез 6): для @font-face-фейсов
    /// это та же аллокация, что лежит в `FontRegistry::bytes_store` — вместо
    /// двух копий одного шрифта (в реестре и здесь) обе стороны разделяют один
    /// буфер через `read_face_bytes` → клон Arc.
    pub(crate) bytes: Arc<[u8]>,
    /// Метрики для горячего текстового пути (cmap-каскад, advance, baseline).
    /// `None` — face не распарсился при загрузке; такие face пропускаются
    /// в каскаде (эквивалент прежнего `Option<ParsedFace>` = None).
    pub(crate) metrics: Option<FaceMetrics>,
}

/// Owned-метрики face-а, независимые от лайфтайма `bytes`. Живут в
/// `LoadedFace` весь срок жизни рендера — снимают необходимость звать
/// `Font::parse` всех face-ов каждый кадр (тёплый кадр экономит 1.4–2.7 мс,
/// холодный — до 200 мс на 1000000-final.html).
pub(crate) struct FaceMetrics {
    /// `head.units_per_em` — масштаб font units → px.
    pub(crate) units_per_em: u16,
    /// `hhea.ascent` — для baseline (ascent ratio).
    pub(crate) ascent: i16,
    /// `hhea.descent` — для baseline (ascent ratio).
    pub(crate) descent: i16,
    /// Owned-копия cmap subtable: codepoint → glyph id без парсинга шрифта.
    pub(crate) cmap: OwnedCmap,
    /// hmtx advance per glyph id (хвост longHorMetric расширен по спеке).
    /// Индекс = glyph id; длина = num_glyphs.
    pub(crate) advances: Box<[u16]>,
    /// `COLR`+`CPAL` цветного шрифта, разобранные один раз при загрузке.
    /// `None` — монохромный face (нет одной из таблиц, или в `COLR` нет ни
    /// одной v0-записи); тогда текстовый путь не меняется вовсе.
    pub(crate) color: Option<ColorTables>,
}

/// Цветные таблицы face-а: layered-глифы (`COLR` v0) + палитры (`CPAL`).
/// Хранятся вместе, потому что по отдельности бесполезны: `palette_index`
/// слоя адресует запись палитры.
#[derive(Debug)]
pub(crate) struct ColorTables {
    /// Слои цветных глифов — `layers_for(glyph_id)` даёт список
    /// (glyph, palette entry).
    pub(crate) colr: Colr,
    /// Палитры, среди которых выбирает CSS `font-palette`.
    pub(crate) cpal: Cpal,
}

/// Строит [`FaceMetrics`] по байтам шрифта. Возвращает `None`, если любая
/// из обязательных таблиц не парсится (head/hhea/cmap/hmtx/maxp).
pub(crate) fn build_face_metrics(bytes: &[u8]) -> Option<FaceMetrics> {
    let font = Font::parse(bytes).ok()?;
    let head = font.head().ok()?;
    let hhea = font.hhea().ok()?;
    let cmap = font.cmap().ok()?;
    let hmtx = font.hmtx().ok()?;
    let num_glyphs = font.maxp().ok()?.num_glyphs;
    let advances: Box<[u16]> = (0..num_glyphs)
        .map(|gid| hmtx.advance_width(gid).unwrap_or(0))
        .collect();
    // Цветной путь включается только когда есть ОБЕ таблицы и в COLR есть
    // v0-записи: COLR v1-only шрифт (paint graph, отложен) сюда не попадает
    // и рисуется монохромным outline-ом, как раньше.
    let color = match (font.colr(), font.cpal()) {
        (Ok(colr), Ok(cpal)) if !colr.is_empty() => Some(ColorTables { colr, cpal }),
        _ => None,
    };
    Some(FaceMetrics {
        units_per_em: head.units_per_em,
        ascent: hhea.ascent,
        descent: hhea.descent,
        cmap: cmap.to_owned_cmap(),
        advances,
        color,
    })
}

/// CSS Fonts L4 §11.3 — разворачивает выбор `font-palette` в плоский список
/// RGBA-цветов записей палитры для конкретного face-а.
///
/// `selection` = `None` → `normal` → палитра 0. `Light`/`Dark` → первая
/// палитра с соответствующим флагом `paletteType`; если такой нет (CPAL v0
/// или флаг ни у кого не выставлен) — по спеке ведёт себя как `normal`.
/// `Custom` → `base-palette` из `@font-palette-values` плюс
/// `override-colors` поверх; неизвестный `base-palette` тоже падает на 0.
///
/// Возвращает `None`, если у шрифта нет ни одной валидной палитры — тогда
/// вызывающая сторона рисует все слои текстовым цветом.
pub(crate) fn resolve_palette(
    tables: &ColorTables,
    selection: Option<&FontPaletteSelection>,
) -> Option<Vec<[f32; 4]>> {
    let base_index = match selection {
        None => 0,
        Some(FontPaletteSelection::Light) => tables.cpal.first_light_palette().unwrap_or(0),
        Some(FontPaletteSelection::Dark) => tables.cpal.first_dark_palette().unwrap_or(0),
        Some(FontPaletteSelection::Custom { base_palette, .. }) => *base_palette,
    };
    // Битый/несуществующий base-palette → палитра 0 (спека: невалидный
    // `base-palette` игнорируется, а не отключает цветной рендер).
    let entries = tables
        .cpal
        .palette(base_index)
        .or_else(|| tables.cpal.palette(0))?;
    let mut colors: Vec<[f32; 4]> = entries
        .iter()
        .map(|c| {
            [
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                c.a as f32 / 255.0,
            ]
        })
        .collect();
    if let Some(FontPaletteSelection::Custom { overrides, .. }) = selection {
        for ov in overrides {
            if let Some(slot) = colors.get_mut(ov.index as usize) {
                *slot = [
                    ov.color.r as f32 / 255.0,
                    ov.color.g as f32 / 255.0,
                    ov.color.b as f32 / 255.0,
                    ov.color.a as f32 / 255.0,
                ];
            }
        }
    }
    Some(colors)
}

/// Цвет одного COLR-слоя: запись палитры либо текстовый цвет для
/// `paletteIndex == 0xFFFF`.
///
/// Альфа записи палитры домножается на альфу текстового цвета — прозрачность
/// `color: rgba(…)` / унаследованная alpha должна гасить цветной глиф так же,
/// как гасит монохромный, иначе полупрозрачный текст «проявляется» на
/// эмодзи. Индекс за пределами палитры (битый шрифт) → текстовый цвет.
pub(crate) fn layer_color(palette: Option<&[[f32; 4]]>, palette_index: u16, text: [f32; 4]) -> [f32; 4] {
    if palette_index == lumen_font::PALETTE_INDEX_FOREGROUND {
        return text;
    }
    match palette.and_then(|p| p.get(palette_index as usize)) {
        Some(&[r, g, b, a]) => [r, g, b, a * text[3]],
        None => text,
    }
}

/// Распарсенный face: Font + таблицы для растеризации. Borrow от
/// `LoadedFace.bytes`.
///
/// После введения `FaceMetrics` нужен только на «медленных» путях:
/// растеризация глифа при промахе atlas-кэша и нормализация
/// font-variation-осей (fvar/avar). Тёплый кадр (все глифы в атласе,
/// без variation settings) не парсит ни одного face-а.
pub(crate) struct ParsedFace<'a> {
    pub(crate) font: Font<'a>,
    pub(crate) head: Head,
    pub(crate) hmtx: Hmtx<'a>,
}

/// Ленивый per-frame кэш [`ParsedFace`]-ов: face парсится при первом
/// обращении внутри одного `render()`-вызова (промах атласа / variation
/// axes), повторные обращения бесплатны. На тёплом кадре не создаётся
/// ни одного `ParsedFace`.
pub(crate) struct LazyParsedFaces<'a> {
    pub(crate) faces: &'a [LoadedFace],
    /// Внешний `Option` — «ещё не пробовали», внутренний — результат парсинга.
    pub(crate) parsed: Vec<Option<Option<ParsedFace<'a>>>>,
}

impl<'a> LazyParsedFaces<'a> {
    pub(crate) fn new(faces: &'a [LoadedFace]) -> Self {
        Self { faces, parsed: Vec::new() }
    }

    /// Парсит face `id` при первом обращении; дальше отдаёт кэш.
    pub(crate) fn get(&mut self, id: usize) -> Option<&ParsedFace<'a>> {
        if id >= self.faces.len() {
            return None;
        }
        if self.parsed.len() < self.faces.len() {
            self.parsed.resize_with(self.faces.len(), || None);
        }
        if self.parsed[id].is_none() {
            let attempt = (|| {
                let font = Font::parse(&self.faces[id].bytes).ok()?;
                let head = font.head().ok()?;
                let hmtx = font.hmtx().ok()?;
                Some(ParsedFace { font, head, hmtx })
            })();
            self.parsed[id] = Some(attempt);
        }
        self.parsed[id].as_ref().and_then(|p| p.as_ref())
    }
}

pub struct Renderer {
    /// Windowed surface; `None` in headless mode (created with `new_headless()`).
    pub(crate) surface: Option<wgpu::Surface<'static>>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    /// BUG-453: `Some(reason)` после коллбэка `Device::set_device_lost_callback`
    /// (регистрируется один раз в `init_pipelines`) — после этого рендер
    /// перестаёт трогать `device`/`queue`/`surface`: на потерянном
    /// устройстве любой вызов, включая `SurfaceTexture::present()`, падает
    /// панику библиотеки без пути восстановления изнутри `render_impl`.
    /// `OnceLock`, а не `AtomicBool`, чтобы донести настоящую причину до
    /// `WgpuBackend::render` — `wgpu::SurfaceError` (тип, который может
    /// вернуть `render_impl`) не умеет нести произвольную строку.
    pub(crate) device_lost: Arc<std::sync::OnceLock<String>>,
    /// Surface configuration; `None` in headless mode.
    pub(crate) config: Option<wgpu::SurfaceConfiguration>,
    /// Width in physical pixels when headless (`surface = None`); 0 otherwise.
    pub(crate) headless_w: u32,
    /// Height in physical pixels when headless (`surface = None`); 0 otherwise.
    pub(crate) headless_h: u32,
    /// Device-pixel-ratio от winit (`Window::scale_factor`). Surface
    /// сконфигурирован в physical pixels (`config.width/height`), но shader
    /// делит позицию вершины на logical viewport (`config / scale_factor`),
    /// чтобы 1 CSS pixel = `scale_factor` device pixels — корректное
    /// масштабирование на HiDPI без правки display list-а.
    /// Обновляется через [`Renderer::set_scale_factor`] при `ScaleFactorChanged`
    /// событии winit (например, drag окна между мониторами с разной DPI).
    pub(crate) scale_factor: f64,
    /// Target color space for wide-gamut output (ph3-color-management Step 4).
    /// Determines the chosen swap-chain format:
    /// `DisplayP3`/`Rec2020` → `Rgba16Float` (or first non-sRGB fallback);
    /// `Srgb` → non-sRGB preferred (existing behaviour).
    pub(crate) target_color_space: ColorSpace,

    /// PILI-CANVAS-BG: sRGB background color (root element's `background-color`)
    /// at the time the current frame started rendering. `None` means use white
    /// (CSS UA default). Used for the LoadOp clear colour at frame start.
    /// Converted from sRGB to `target_color_space` before being passed to the
    /// GPU clear colour (ph3-color-management Step 5).
    pub(crate) canvas_bg: Option<Color>,

    /// GPU depth buffer for CSS 3D transforms (`transform-style: preserve-3d`).
    /// Size matches the frame surface; recreated on every `resize()`.
    /// `None` only when both dimensions are zero at construction time.
    // CSS: transform-style — when P4 wires preserve-3d, depth_sorted_child_order()
    // in display_list.rs emits commands back-to-front; the GPU depth test here
    // provides correct occlusion for the rare case of intersecting 3D planes.
    pub(crate) depth_texture: Option<wgpu::Texture>,
    pub(crate) depth_view: Option<wgpu::TextureView>,

    /// Снимок хэндлов, из которых собирается любой ленивый пайплайн.
    /// Отдаётся клоном фоновому потоку прогрева (BUG-405).
    pub(crate) pdeps: PipelineDeps,
    /// Сколько командных списков **кадра** этот рендер отправил в очередь за
    /// свою жизнь (BUG-405 срез 2). Служебные подачи (mip-генерация при
    /// загрузке картинки, обратное чтение в headless) не считаются: гейт
    /// подачи порциями — про кадр.
    pub(crate) submissions: u64,
    /// Сколько скруглённых клипов этот рендер обслужил offscreen-уровнем
    /// (три пасса на клип) за свою жизнь — BUG-405 срез 4. Шейдерный контур
    /// счётчик не двигает, поэтому «правка работает» = «счётчик не растёт».
    pub(crate) rrect_clip_levels: u64,
    /// Сколько разрезов пасса родителя этот рендер склеил обратно после
    /// выброса невидимого offscreen-уровня (BUG-405 срез 5). Растёт ровно на
    /// те уровни, которые `viewport-cull` убрал из плана, а прежний код
    /// оставлял за собой лишний пасс родителя.
    pub(crate) cull_merges: u64,
    /// Сколько пассов (элементов плана) этот рендер закодировал за свою жизнь
    /// — BUG-405 срез 5. Эффект склейки виден именно здесь: механизм считает
    /// `cull_merges`, а «пассов стало меньше» — этот счётчик.
    pub(crate) plan_passes: u64,
    /// Сколько render-пассов закодировали filter-элементы плана (BUG-405
    /// срез 6): blur даёт два (H + слитый V-композит) вместо трёх, фильтр
    /// без blur - один. Гейт правки стоит на нём, а не на времени кадра.
    pub(crate) filter_passes: u64,
    /// Склейка вертикального прохода блюра с композитом включена (срез 6).
    /// Инстансный выключатель нужен гейту пикселей: оба плеча снимаются в
    /// одном процессе. Поверх него - рычаг процесса `LUMEN_NO_BLUR_MERGE=1`.
    pub(crate) blur_merge_enabled: bool,
    /// Склейка пасса родителя вокруг выброшенного уровня включена
    /// (BUG-405 срез 5). Инстансное плечо A/B: тест рисует один и тот же
    /// список обоими путями в одном процессе и сверяет пиксели побайтово, не
    /// требуя второго прогона с `LUMEN_NO_CULL_MERGE=1`.
    pub(crate) cull_merge_enabled: bool,
    /// Сколько внешних теней нарисовано аналитически, без offscreen-уровня
    /// (BUG-405 срез 7). Гейт правки — этот счётчик рядом с `filter_passes`.
    pub(crate) shadow_draws: u64,
    /// Сколько команд состояния пасса (пайплайн / bind-группа / вершинный
    /// буфер / scissor) не отправлено, потому что там уже стояло ровно это
    /// значение — BUG-405 срез 10. Команды пасса стоят в `drop(pass)`, где
    /// `wgpu-core` проигрывает их в командный список, поэтому «правка
    /// работает» = «счётчик растёт», а не «кадр стал быстрее».
    pub(crate) state_elisions: u64,
    /// Сколько вызовов `draw` слито с предыдущим, потому что состояние пасса
    /// между ними не менялось, а диапазоны вершин оказались соседними
    /// (BUG-405 срез 10). Второй счётчик того же среза: команд состояния
    /// стало меньше — этот, самих draw'ов меньше — тот.
    pub(crate) draw_merges: u64,
    /// Отсев повторных команд состояния включён (BUG-405 срез 10). Инстансное
    /// плечо A/B: тест рисует один и тот же список обоими путями в одном
    /// процессе и сверяет пиксели, не требуя второго прогона с
    /// `LUMEN_NO_STATE_ELISION=1`.
    pub(crate) state_elision_enabled: bool,
    /// Аналитическая размытая тень включена (BUG-405 срез 7). Инстансное
    /// плечо A/B: тест рисует один и тот же список обоими путями в одном
    /// процессе и сверяет пиксели.
    pub(crate) shadow_analytic_enabled: bool,
    /// Сколько байт пикселей атласа отправлено в GPU за жизнь рендерера
    /// (BUG-405 срез 11). Гейт правки — этот счётчик: заливка целой текстуры
    /// (1 МиБ) против заливки только изменившихся строк.
    pub(crate) atlas_bytes_uploaded: u64,
    /// Сколько раз атлас заливался в GPU (BUG-405 срез 11). Байты без числа
    /// заливок не отличают «стало реже» от «стало меньше за раз».
    pub(crate) atlas_uploads: u64,
    /// Заливка только изменившихся строк атласа включена (BUG-405 срез 11).
    /// Инстансное плечо A/B: тест гоняет один и тот же список обоими путями в
    /// одном процессе и сверяет пиксели. Поверх — рычаг
    /// `LUMEN_NO_ATLAS_PARTIAL=1`.
    pub(crate) atlas_partial_upload_enabled: bool,
    /// Сколько ВЛОЖЕННЫХ скруглённых клипов обслужено вторым шейдерным
    /// контуром, то есть без offscreen-уровня (BUG-405 срез 8). Гейт правки —
    /// этот счётчик рядом с `rrect_clip_levels`.
    pub(crate) nested_shader_clips: u64,
    /// Второй шейдерный контур включён (BUG-405 срез 8). Инстансное плечо A/B:
    /// тест рисует один и тот же список обоими путями в одном процессе и
    /// сверяет пиксели. Поверх — рычаг `LUMEN_NO_NESTED_SHADER_CLIP=1`.
    pub(crate) nested_shader_clip_enabled: bool,
    /// Мемоизация покрытия SVG-супов (BUG-405 срез 9).
    pub(crate) coverage_cache: CoverageCache,
    /// Кэш покрытия включён (BUG-405 срез 9). Инстансное плечо A/B: тест
    /// рисует один и тот же список обоими путями в одном процессе и сверяет
    /// пиксели. Поверх — рычаг `LUMEN_NO_COVERAGE_CACHE=1`.
    pub(crate) coverage_cache_enabled: bool,
    /// Мемоизация целых фигур SVG (BUG-405 срез 12).
    pub(crate) svg_shape_cache: SvgShapeCache,
    /// Кэш фигур SVG включён (BUG-405 срез 12). Инстансное плечо A/B: тест
    /// рисует один и тот же список обоими путями в одном процессе и сверяет
    /// пиксели. Поверх — рычаг `LUMEN_NO_SVG_SHAPE_CACHE=1`.
    pub(crate) svg_shape_cache_enabled: bool,
    /// Мемоизация укладки целого текстового run-а (BUG-405 срез 13).
    pub(crate) text_run_cache: TextRunCache,
    /// Кэш укладки текста включён (BUG-405 срез 13). Инстансное плечо A/B:
    /// тест рисует один и тот же список обоими путями в одном процессе и
    /// сверяет вершины. Поверх — рычаг `LUMEN_NO_TEXT_RUN_CACHE=1`.
    pub(crate) text_run_cache_enabled: bool,
    /// Приёмник готовых пайплайнов с потока прогрева (BUG-405). `None` —
    /// прогрев ещё не запущен, уже завершён, или отключён
    /// `LUMEN_NO_PIPELINE_WARMUP=1`. Сбрасывается в `None`, когда поток
    /// закрыл отправитель, — дальше `try_recv` был бы холостым.
    pub(crate) warm_rx: Option<std::sync::mpsc::Receiver<WarmedPipeline>>,
    /// Прогрев уже запускался (BUG-405). Отдельно от `warm_rx`, который
    /// обнуляется по завершении потока: без флага прогрев перезапускался бы
    /// каждый кадр после его окончания.
    pub(crate) warm_started: bool,

    /// Сплошная заливка. BUG-406 срез 3: ячейка наполняется фоновым потоком
    /// сборки горячих пайплайнов, читается только через [`Self::fill_pipeline`].
    pub(crate) fill_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`circle_pipeline()`), не при старте окна.
    pub(crate) circle_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS border-radius SDF pipeline. Uses `RRectVertex` layout.
    /// BUG-406 срез 3: см. [`Self::fill_pipeline`].
    pub(crate) rrect_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-405 срез 7 — аналитическая размытая rrect (`box-shadow`), формат
    /// `ShadowVertex`. Ленивая компиляция: страница без теней за неё не платит,
    /// прогрев подхватывает её в общем списке (`build_all_lazy`).
    pub(crate) shadow_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Квады глифов из атласа. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    pub(crate) text_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Текстурный квад картинки. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    pub(crate) image_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Blit-каскад mip-цепочки картинок: пасс «mip N−1 → mip N» при
    /// `register_image` (fullscreen triangle, bilinear = 2×2 box).
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`mipgen_pipeline()`), не при старте окна.
    pub(crate) mipgen_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Images L4 §4 — `cross-fade(A, B, p)` two-texture blend pipeline.
    /// Uses `CrossFadeVertex` layout (pos+uv). Bind group 0 = viewport uniform
    /// (shared with `image_pipeline`); bind group 1 = `cross_fade_bgl`
    /// (tex_a, tex_b, sampler, progress uniform). Blend state: `ALPHA_BLENDING`.
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`cross_fade_pipeline()`), не при старте окна.
    pub(crate) cross_fade_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Bind group layout for the `cross_fade_pipeline` per-quad bindings
    /// (group 1): two textures + sampler + progress uniform.
    pub(crate) cross_fade_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`composite_pipeline()`), не при старте окна.
    pub(crate) composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    pub(crate) composite_bgl: wgpu::BindGroupLayout,
    /// CSS Overflow L3 §2 — composite-пайплайн скруглённого клипа
    /// (`PushClipRoundedRect` → offscreen-уровень, `PopClip` → этот пасс).
    /// Разделяет `composite_bgl` с обычным композитом: та же пара
    /// {текстура уровня, sampler}. BUG-406: компиляция ленивая.
    pub(crate) rrect_clip_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Masking L1 §3 — composite-пайплайн формы `clip-path`
    /// (`PushClipPath` → offscreen-уровень, `PopClip` → этот пасс).
    /// Свой BGL: к паре {текстура уровня, sampler} добавлен uniform формы.
    /// BUG-406: компиляция ленивая.
    pub(crate) path_clip_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BGL пайплайна формы клипа: текстура(0) + sampler(1) + uniform формы(2).
    pub(crate) path_clip_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blend_pipeline()`), не при старте окна.
    pub(crate) blend_pipeline: OnceCell<wgpu::RenderPipeline>,
    pub(crate) blend_bgl: wgpu::BindGroupLayout,
    /// CSS Masking L1 §4 — mask composite pipeline + bind group layout.
    /// Used by PopMask to composite the offscreen layer using a mask image.
    pub(crate) mask_composite_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`mask_composite_pipeline()`), не при старте окна.
    pub(crate) mask_composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Masking L1 §5 — mask-layer composite pipelines.
    /// Used by PopMaskLayer to apply an arbitrary rendered mask to the parent layer.
    /// `_alpha` samples mask.a; `_luma` converts RGB to luminance × alpha.
    /// Shared BGL with mask_composite (same binding layout: t_content, t_mask, s).
    /// BUG-406: ленивая компиляция пары (alpha, luminance) — общий шейдер,
    /// поэтому один `OnceCell` на оба пайплайна.
    pub(crate) mask_layer_pipelines: OnceCell<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    /// CSS Filter Effects L1 — color filter pipeline (grayscale/sepia/brightness/etc.).
    pub(crate) filter_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`filter_pipeline()`), не при старте окна.
    pub(crate) filter_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Filter Effects L1 — separable Gaussian blur pipeline (one pass: H or V).
    pub(crate) blur_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blur_pipeline()`), не при старте окна.
    pub(crate) blur_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-405 срез 6 — вертикальный проход блюра вместе с цветовыми фильтрами
    /// и композитом в родителя: один пасс вместо двух. Layout = `blur_bgl`
    /// плюс четвёртый слот с `FilterParams`.
    pub(crate) blur_composite_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blur_composite_pipeline()`), не при старте окна.
    pub(crate) blur_composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Filter Effects L1 §2 — backdrop-filter blit pipeline.
    /// Same shader as `filter_pipeline` but uses REPLACE blend so the filtered
    /// backdrop snapshot overwrites (not composites over) the parent layer at
    /// the bounded element rect.
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`backdrop_blit_pipeline()`), не при старте окна.
    pub(crate) backdrop_blit_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Intermediate texture for backdrop-filter: ping-pong target for blur passes
    /// (H: scratch → backdrop_layer; V: backdrop_layer → scratch), and color-filter
    /// target when compositing filtered backdrop back onto parent.
    pub(crate) backdrop_layer: Option<OffscreenLayer>,
    /// CSS Filter Effects L1 §2 — `backdrop-filter` result cache (metadata).
    /// Tracks, per backdrop element ordinal, the content hash of the inputs that
    /// produced the cached filtered texture. Used to skip the blur passes when a
    /// frame's backdrop inputs are unchanged from the previous frame.
    pub(crate) backdrop_cache: crate::backdrop_cache::BackdropCache,
    /// Cached filtered backdrop textures, keyed by the same ordinal as
    /// [`Self::backdrop_cache`]. Each is a full parent-layer-sized snapshot of
    /// the blurred (or, for filter-only backdrops, copied) backdrop region.
    /// Reused across frames on a cache hit; the color-filter pass still runs at
    /// blit time so only the expensive blur is skipped.
    pub(crate) backdrop_cache_textures: HashMap<u32, OffscreenLayer>,
    /// Кэш depth-текстур под bbox-офскрины (регион ≠ размеру окна/полосы):
    /// пасс с маленьким color-attachment обязан иметь depth того же размера
    /// (валидация wgpu). Ключ — (w, h) в device px; размеры регионов
    /// выровнены до 64 px, так что классов мало. Чистится при переполнении
    /// (> 16 записей) — обычная страница держит 1-3 размера.
    pub(crate) small_depth_cache: HashMap<(u32, u32), wgpu::TextureView>,
    /// CSS Images L3 §3.3 — linear/radial gradient pipeline.
    pub(crate) gradient_bgl: wgpu::BindGroupLayout,
    /// Градиентная заливка. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    pub(crate) gradient_pipeline: OnceCell<wgpu::RenderPipeline>,
    pub(crate) scratch_layer: Option<OffscreenLayer>,
    pub(crate) layer_sampler: wgpu::Sampler,
    pub(crate) layer_textures: Vec<OffscreenLayer>,
    pub(crate) surface_format: wgpu::TextureFormat,

    pub(crate) uniform_buffer: wgpu::Buffer,
    pub(crate) uniform_bind_group: wgpu::BindGroup,
    /// Сколько слотов [`ClipUniformSlot`] помещается в `uniform_buffer`
    /// (BUG-405 срез 4). Слот 0 — «скруглённого клипа нет».
    pub(crate) uniform_slots: usize,

    pub(crate) atlas_texture: wgpu::Texture,
    pub(crate) atlas_bind_group: wgpu::BindGroup,
    /// Сколько раз атлас глифов сбрасывался из-за исчерпания места (BUG-435).
    /// Растёт — атласу 1024×1024 тесно на этом контенте.
    pub(crate) atlas_resets: u64,

    pub(crate) image_bgl: wgpu::BindGroupLayout,
    pub(crate) image_sampler: wgpu::Sampler,
    pub(crate) image_sampler_nearest: wgpu::Sampler,
    /// Sampler блита полосы скролл-композитора: линейный, но `Repeat` по V —
    /// полоса адресуется кольцом (BUG-405 срез 32).
    pub(crate) band_sampler: wgpu::Sampler,
    /// Декодированные изображения в CPU-памяти. Хранятся для on-demand
    /// ресайза под конкретный layout-размер (CPU bilinear resize).
    pub(crate) raw_images: HashMap<String, Image>,
    /// Cache GPU-текстур: ключ `"src"` (оригинал) или `"src@WxH"` (ресайз).
    /// Заполняется через [`Renderer::register_image`] и лениво при DrawImage.
    pub(crate) images: HashMap<String, GpuImage>,
    /// Cache GPU-снимков слоёв per-id. Заполняется compositor-ом через
    /// [`Renderer::upload_layer_snapshot`] для кеширования неизменных слоёв.
    pub(crate) layer_snapshots: HashMap<u64, GpuLayerSnapshot>,
    /// Skip-identical-frame: поколение контента, не входящего в display list
    /// (картинки/GIF-кадры/снапшоты/шрифты/canvas-bg/промо-слои). Бампается
    /// каждой мутирующей операцией; входит в хэш кадра.
    pub(crate) content_generation: u64,
    /// Фиксированное смещение страницы в CSS px (ADR-016 M0.4, BUG-405 срез 38).
    ///
    /// Применяется как самая внешняя трансляция КОНТЕНТА (не overlay-я) —
    /// ровно то, что раньше шелл каждый кадр заворачивал в
    /// `PushTransform(translate(offset))`, копируя ради этого весь display
    /// list. Входит в поколение контента: смена смещения не меняет список, но
    /// меняет пиксели.
    pub(crate) page_offset: (f32, f32),
    /// Хэш последнего успешно отрисованного оконного кадра
    /// (display list + overlay + scroll + размер + `content_generation`).
    /// Совпадение со следующим кадром ⇒ пиксели идентичны ⇒ кадр пропускается.
    pub(crate) last_frame_hash: Option<u64>,
    /// Скролл-композитор страницы (EXPERIMENT.md §2): персистентная полоса
    /// документа. `None` — ещё не рисовалась (или сброшена сменой геометрии).
    pub(crate) page_band: Option<PageBandCache>,
    /// Blit-квад полосы для следующего Compose-рендера. Ставится только
    /// `try_page_compose`, снимается `take()`-ом в начале сбора вершин.
    pub(crate) pending_base_blit: Option<PendingBaseBlit>,
    /// Retained-текстура стабильного хвоста overlay-списка (BUG-405 срез 41).
    /// `None` — ещё не построена, либо прошлый кадр её признал устаревшей.
    pub(crate) overlay_cache: Option<OverlayCache>,
    /// Blit-квад overlay-кэша для следующего Compose-рендера — тот же
    /// одноразовый контракт, что у `pending_base_blit` (ставится
    /// `compose_page`, снимается `take()`-ом сразу после него).
    pub(crate) pending_overlay_blit: Option<PendingBaseBlit>,
    /// Digest-вектор overlay-списка ПРОШЛОГО кадра (`hash_one_command` на
    /// элемент) — не путать с `OverlayCache::tail_digests` (тот держит digest
    /// ТОЛЬКО хвоста на момент постройки КЭША, а не прошлого кадра; нужен
    /// обоим: этот ловит «где кончается изменившийся префикс» ради выбора
    /// точки разреза при пересборке, тот — «кэш всё ещё валиден»).
    pub(crate) last_overlay_digests: Vec<u64>,
    /// Причина последнего отказа скролл-композитора (BUG-405 срез 22) —
    /// печатается при её СМЕНЕ под `LUMEN_FRAME_LOG>=2`. Без неё отказ виден
    /// только как отсутствие строк `page-compose`, и перепись не может
    /// отличить «композитор не применим» от «композитора нет в сборке».
    pub(crate) last_compose_skip: Option<&'static str>,
    /// Scroll-инвариантный ключ контента ПРОШЛОГО кадра. Полоса рисуется
    /// только по стабильному контенту (ключ совпал два кадра подряд):
    /// анимация/GIF/стриминг парсера меняют ключ каждый кадр, и рендер
    /// полосы (1.7× выше вьюпорта) там был бы дороже монолита — замерено
    /// 2026-07-10: 511 промахов из 629 кадров, медиана 10.7 → 21 мс.
    pub(crate) last_content_key: Option<u64>,
    /// Версия content-списка, объявленная shell-ом (BUG-405 срез 39).
    /// `0` — «версия неизвестна», свёртка не мемоизируется. Контракт —
    /// [`RenderBackend::set_content_epoch`](crate::backend::RenderBackend::set_content_epoch).
    pub(crate) content_epoch: u64,
    /// Свёртка content-части обоих кадровых хэшей с прошлого кадра плюс всё,
    /// по чему её законно переиспользовать: версия списка, его адрес, длина и
    /// подпись выколотых диапазонов. Адрес и длина — не замена версии, а
    /// страховка: они ловят подмену списка, о которой shell не сказал, но не
    /// ловят правку на месте (её обязана поймать версия).
    pub(crate) content_fold_memo: Option<ContentFoldMemo>,
    /// GPU layer cache with LRU eviction (ADR-008 Phase 2).
    /// Tracks layer textures by stacking context ID + size for off-viewport eviction.
    pub(crate) layer_cache: crate::layer_cache::LayerCache,

    pub(crate) atlas: GlyphAtlas,
    /// Загруженные face-ы. `faces[0]` — default (bundled), используется когда
    /// `font-family` пуст или ни одно имя не нашлось через `FontProvider`.
    /// Остальные добавляются лениво при первом `DrawText` с известной family.
    pub(crate) faces: Vec<LoadedFace>,
    /// `face_id` bundled Golos Text Regular (DS-4) — default chrome UI font,
    /// used by [`Self::resolve_face_id`] when `font_family` is empty (every
    /// chrome `DrawText` call site) or requests reserved family `"Golos Text"`.
    pub(crate) chrome_face_id: Option<usize>,
    /// `face_id` bundled Golos Text Medium (DS-4) — reserved family `"Golos Text Medium"`.
    pub(crate) chrome_face_medium_id: Option<usize>,
    /// `face_id` bundled JetBrains Mono Regular (DS-4) — reserved family
    /// `"JetBrains Mono"`, used for the omnibox URL field and DevTools panels.
    pub(crate) mono_face_id: Option<usize>,
    /// `face_id` по абсолютному пути TTF — чтобы не грузить файл повторно.
    pub(crate) face_id_by_path: HashMap<PathBuf, usize>,
    /// Мемоизация `resolve_face_id`: хэш `(families, weight, style)` →
    /// `face_id`. Без него каждый `DrawText` каждого кадра гонял
    /// `to_lowercase` + `FontProvider::pick_face` (двe Vec-аллокации +
    /// матчинг). Ключ — u64-хэш (SipHash); коллизия теоретически возможна,
    /// но при десятках ключей пренебрежима (та же логика, что skip-frame
    /// hash). Сбрасывается в `set_font_provider` — новый провайдер
    /// (например, FontRegistry с @font-face) меняет ответы резолва.
    pub(crate) resolve_cache: HashMap<u64, usize>,
    /// Источник лукапа face-ов по `(family, weight, style)`. По умолчанию —
    /// `SystemFontIndex`, который лениво сканирует системные font-директории.
    /// `None` означает «без resolver-а — всегда default face» (для тестов /
    /// headless-режимов).
    pub(crate) font_provider: Option<Arc<dyn FontProvider>>,
    /// Кэш растеризованных глифов: ключ `(face_id, glyph_id, size_bin)`.
    /// `face_id` — глифы у разных face-ов имеют разный glyph_id; `size_bin`
    /// — multi-size atlas (см. `SIZE_BINS`): один и тот же глиф для
    /// font-size 16 и 32 даёт две разные записи (разная растеризация,
    /// разный atlas-rect).
    pub(crate) cached_glyphs: HashMap<AtlasKey, Option<CachedGlyph>>,
    /// In headless mode: the `RENDER_ATTACHMENT | COPY_SRC` texture rendered to
    /// by the most recent `render()` call. Kept alive between `render()` and
    /// `render_to_image()` pixel readback, then dropped.
    pub(crate) pending_readback: Option<wgpu::Texture>,
    /// GPU texture pool for layer recycling (ADR-008 Phase 2).
    /// Maintains free textures keyed by (width, height) for reuse instead of
    /// allocating a new `wgpu::Texture` for each layer. Свободный список
    /// ограничен байтовым бюджетом (BUG-272 срез 21) — вытеснение в `trim()`
    /// после сабмита кадра.
    pub(crate) texture_pool: crate::texture_pool::TexturePool<crate::texture_pool::PooledTexture>,
    /// Normalized GPU fingerprint: prevents WebGL renderer/vendor fingerprinting (ADR-007).
    pub(crate) gpu_fingerprint: GpuFingerprint,
    /// Потоки, собравшие горячие пайплайны этого рендера — см.
    /// [`Renderer::hot_pipeline_threads`]. `RefCell`: при фоновой сборке
    /// (BUG-406 срез 3) множество пополняется по мере приёма пайплайнов, то
    /// есть уже из `&self`-аксессоров кадра.
    pub(crate) hot_pipeline_threads: RefCell<HashSet<std::thread::ThreadId>>,
    /// Приёмник горячих пайплайнов с фоновых потоков сборки (BUG-406 срез 3).
    /// `None` — сборка была синхронной (headless, `LUMEN_SERIAL_PIPELINES=1`,
    /// `LUMEN_WAIT_HOT_PIPELINES=1`) либо канал уже опустел.
    pub(crate) hot_rx: RefCell<Option<std::sync::mpsc::Receiver<HotDelivery>>>,
    /// Входы сборки горячих пайплайнов — нужны, чтобы кадр мог собрать
    /// пайплайн сам, если фоновый поток не стартовал или умер (BUG-406 срез 3).
    pub(crate) hot_deps: HotDeps,
    /// Сколько горячих пайплайнов пришлось скомпилировать САМОМУ UI-потоку —
    /// гейт среза 3 BUG-406, см. [`Renderer::hot_pipelines_built_on_ui_thread`].
    pub(crate) hot_built_on_ui: std::cell::Cell<usize>,
}

