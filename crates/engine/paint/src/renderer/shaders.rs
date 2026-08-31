//! P1/SPLIT-RN10: WGSL-исходники всех пайплайнов (fill/rrect/text/image/
//! gradient/circle/shadow/mipgen/cross-fade/composite/path-clip/blend/
//! mask/filter/blur) + glyph-атлас (`ATLAS_DIM`/`SIZE_BINS`/`size_bin_for`/
//! `atlas_key`) — из `renderer.rs` (54…1652 до вырезки). Последний батч
//! группы RN. Вынесено из `renderer.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа RN, батч RN-10).

use crate::atlas::AtlasKey;

/// Размер атласа в пикселях (квадратный). Поднят с 512 до 1024 под
/// multi-size atlas: типичная страница использует 2-3 размера шрифта,
/// что даёт ~3× больше уникальных глифов в кеше.
pub(crate) const ATLAS_DIM: u32 = 1024;

/// Минимальный запас полосы скролл-композитора с каждой стороны вьюпорта,
/// в долях его высоты (BUG-405 срез 22).
///
/// Полная полоса — 0.75 вьюпорта сверху и снизу; когда столько не влезает в
/// `max_texture_dimension_2d`, запас режется, но ниже этой доли полоса теряет
/// смысл: промах случается почти каждым кадром прокрутки, а промах стоит
/// рендера всей полосы, то есть дороже монолита. При 0.25 и типичном шаге
/// колеса (~120 CSS px) промах приходится примерно на каждый третий кадр —
/// остальные обслуживает дешёвая композиция.
pub(crate) const BAND_MIN_MARGIN_RATIO: f32 = 0.25;

/// Потолок запаса полосы скролл-композитора с каждой стороны вьюпорта, CSS px.
///
/// Полный запас — 0.75 вьюпорта, но не больше этого потолка: на развёрнутом
/// окне доля дала бы полосу в 2.5 вьюпорта, то есть промах стоил бы рендера
/// 2.5 экранов. Значение переопределяется `LUMEN_BAND_MARGIN_CSS` — рычаг
/// переписи (BUG-405 срез 27), см. [`band_margin_override_css`].
///
/// **Подкручивать это число нечем — замерено, а не предположено** (BUG-405
/// срез 31, `scripts/band_margin_census.py`). Свип запасов 300…900 CSS px по
/// 3–6 интерливед-повторов на точку: число промахов на пути меняется втрое
/// (24 против 10 на 14 400 px), а приведённая к пути цена прокрутки стоит на
/// месте — 47.8–49.4 мс/1000 px по среднему плеча при разбросе повторов
/// ВНУТРИ плеча до 3.8 мс. Полоса больше — промах ровно во столько же дороже,
/// во сколько реже (срез 27), и постоянная четверть надбавки (срез 30) этой
/// картины не переворачивает. Точка 500, единственный намёк на выигрыш в
/// одиночном свипе среза 27, тремя свипами не подтверждена: знак разницы
/// переворачивается между свипами и между метриками при модуле ≤3 %.
pub(crate) const BAND_MARGIN_CAP_CSS: f32 = 768.0;

/// CSS-`z` квада, которым пасс кромки кольцевой полосы заливает её фон
/// (BUG-405 срез 32).
///
/// Шейдер переводит CSS-`z` в глубину как `0.5 − z/20000`, обрезая в `[0,1]`,
/// поэтому −10000 — это ровно дальняя плоскость: квад пишет ту же глубину 1.0,
/// которую оставил бы `LoadOp::Clear(1.0)`, и не отбраковывает содержимое с
/// отрицательным `z-index`, как отбраковал бы фон на `z = 0`.
pub(crate) const BAND_STRIP_BG_Z: f32 = -10_000.0;

/// Сторона текстуры, которую движок запрашивает у устройства, когда адаптер
/// её отдаёт (BUG-405 срез 23).
///
/// Совпадает с `max_texture_dimension_2d` дефолтного тира WebGPU
/// (`wgpu::Limits::default()`); всё остальное остаётся на
/// `downlevel_defaults()`, где эта сторона — 2048. Полоса скролл-композитора
/// (2.5 вьюпорта) в 2048 не влезала уже на окне клиентской высотой ~819 device
/// px, поэтому на развёрнутом окне композитор либо ужимался до почти нулевого
/// запаса, либо (выше ~1365 px) отключался целиком. 8192 покрывает 4K-вьюпорт
/// с полным запасом и не даёт запросить у драйвера тир, которого он не
/// обещает: значение всегда режется по [`wgpu::Adapter::limits`].
pub(crate) const MAX_TEXTURE_DIM_TARGET: u32 = 8192;

/// Bin размеров растеризации (CSS px). `font_size` округляется до
/// ближайшего bin вверх через `size_bin_for`. Если ≤ 8 — используется
/// bin 8 (нечитаемо иначе всё равно); если > 64 — bin 64 с up-scaling-ом
/// (большие заголовки редки, потеря качества на единичных headline-ах
/// приемлема в Phase 0). При совпадении font_size с bin-ом квад не
/// масштабируется (нет blur).
pub(crate) const SIZE_BINS: [u16; 8] = [8, 12, 16, 20, 24, 32, 48, 64];

/// CSS px → размер растеризации в `SIZE_BINS`. Round-up до ближайшего bin;
/// > последнего bin — клампим к последнему.
pub(crate) fn size_bin_for(font_size: f32) -> u16 {
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
pub(crate) fn atlas_key(
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords_hash: u64,
) -> AtlasKey {
    AtlasKey::new((face_id & 0xFFFF) as u16, glyph_id, size_bin, coords_hash)
}

pub(crate) const FILL_SHADER_SRC: &str = r#"
struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000).
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) color: vec4<f32>,
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
    // CSS z: positive=closer. WebGPU: smaller depth=front.
    // ±10000 CSS px → [0,1]: z=0→0.5 (2D, painter's order), z>0→<0.5 (front), z<0→>0.5 (back).
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb, in.color.a * rrect_clip_coverage(in.clip.xy));
}
"#;

/// SDF-круг: UV (-1..1) из центра; фрагменты за радиусом 1.0 discarded.
/// Anti-aliasing через smoothstep(0.9, 1.0, dist).
/// SDF-круг: Skia-compatible 1px linear AA: coverage = clamp(0.5 + r - dist_px, 0, 1).
/// Quad расширен на 0.5px с каждой стороны, UV=±1 соответствует r+0.5 px от центра.
/// `radius_px` (loc 3) — CSS-радиус точки. Формула совпадает с Skia, что минимизирует
/// разницу с Chrome/Edge (пиксельный pixel-diff для dotted border ≈ sub-pixel noise).
pub(crate) const CIRCLE_SHADER_SRC: &str = r#"
struct VIn {
    @location(0) pos:       vec2<f32>,
    @location(1) uv:        vec2<f32>,
    @location(2) color:     vec4<f32>,
    @location(3) radius_px: f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>,
    @location(1) color:     vec4<f32>,
    @location(2) radius_px: f32,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    var out: VOut;
    out.clip      = vec4<f32>(ndc, 0.0, 1.0);
    out.uv        = in.uv;
    out.color     = in.color;
    out.radius_px = in.radius_px;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Quad spans (r+0.5) px in each direction from center, so dist_px = |uv| * (r+0.5).
    let dist_px = length(in.uv) * (in.radius_px + 0.5);
    let alpha = clamp(0.5 + in.radius_px - dist_px, 0.0, 1.0) * rrect_clip_coverage(in.clip.xy);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

/// WGSL `sdf_rrect` — shared by every shader that needs a rounded-rect contour
/// (`RRECT_SHADER_SRC` fills one, `RRECT_CLIP_SHADER_SRC` clips a layer to one).
/// Prepended to those sources at pipeline-build time: WGSL has no `#include`,
/// and a second hand-copied SDF is exactly how BUG-277 slice 4's edge-band bug
/// would come back on one path only.
pub(crate) const SDF_RRECT_WGSL: &str = r#"
/// SDF for an axis-aligned rounded rectangle with per-corner elliptical radii.
/// `p`         = position relative to rect center.
/// `half_size` = half-dimensions of the rect.
/// `radii_x`   = horizontal corner radii (tl, tr, br, bl).
/// `radii_y`   = vertical  corner radii (tl, tr, br, bl).
///
/// Screen y-axis is DOWN: p.y < 0 = top half, p.y > 0 = bottom half.
/// Radii arrive already reduced by the CSS Backgrounds L3 §5.5 overlap factor
/// (`CornerRadii::clamped_to_box` on the CPU side) — the shader must NOT clamp
/// them again per axis: `border-radius: 100px 0 0 0` on a 100px-wide box is a
/// legal corner ellipse wider than the half-box.
///
/// Outside the corner bands the exact axis-aligned box SDF applies; only inside
/// a corner band on BOTH axes does the ellipse term kick in. Elliptical corners
/// use a first-order approximation: (|q/r| - 1) * min(rx,ry), which is exact on
/// the ellipse surface and has unit gradient near the boundary; for circular
/// corners (rx == ry) it is identical to the standard Quilez SDF and joins the
/// box branch continuously at the tangent lines.
fn sdf_rrect(p: vec2<f32>, half_size: vec2<f32>, radii_x: vec4<f32>, radii_y: vec4<f32>) -> f32 {
    // Select corner radii based on quadrant (y-down screen space).
    var rx: f32 = radii_x.x; // top-left (default)
    var ry: f32 = radii_y.x;
    if p.x >= 0.0 && p.y <= 0.0 { rx = radii_x.y; ry = radii_y.y; } // top-right
    if p.x >= 0.0 && p.y >  0.0 { rx = radii_x.z; ry = radii_y.z; } // bottom-right
    if p.x <  0.0 && p.y >  0.0 { rx = radii_x.w; ry = radii_y.w; } // bottom-left
    // Offsets from the box edges (standard box-SDF form, negative = inside).
    let d = abs(p) - half_size;
    // Position relative to the corner ellipse centre: q > 0 on an axis means the
    // point lies inside that corner band.
    let q = d + vec2<f32>(rx, ry);
    // Straight region on at least one axis (or a degenerate radius) — the nearest
    // boundary is a flat edge, so the plain box SDF is exact.
    if q.x <= 0.0 || q.y <= 0.0 || rx < 0.001 || ry < 0.001 {
        return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
    }
    // Both axes inside the corner band: first-order ellipse SDF approximation.
    let k = length(q / vec2<f32>(rx, ry));
    return (k - 1.0) * min(rx, ry);
}
"#;

/// BUG-405 срез 4 — общий пролог всех рисующих шейдеров: viewport-uniform
/// группы 0 и покрытие активного скруглённого клипа.
///
/// Раньше `overflow: hidden` со скруглением всегда открывал offscreen-уровень,
/// а `PopClip` композитил его через контур: три пасса на клип (сброс батча
/// родителя + пасс уровня + композит). На `lenta.ru` это 751 пасс из 915 за
/// прокрутку — при том, что 235 из 251 таких уровней содержат РОВНО ОДНУ
/// операцию рисования. Теперь контур приезжает uniform-ом (динамический офсет
/// на слот в общем буфере) и каждый шейдер сам домножает альфу на покрытие —
/// клип не стоит ни одного пасса. Уровень остаётся запасным путём (вложенный
/// клип, фильтр/маска внутри поддерева — см. `shader_rrect_clip_allowed`).
///
/// Слот 0 — «клипа нет»: полуразмер 1e7 даёт покрытие 1 во всей поверхности.
/// Позиция берётся из `@builtin(position)` — это device px, поэтому шейдер
/// делит её на `dpr` и считает контур в CSS px: ровно в тех же единицах, в
/// каких контур считает композит-пасс `RRECT_CLIP_SHADER_SRC` и сам
/// скруглённый бокс (`RRECT_SHADER_SRC`). Иначе на HiDPI полоса сглаживания
/// клипа была бы вдвое уже полосы собственного края бокса. Новых varying'ов
/// ни одному шейдеру при этом не нужно.
/// BUG-405 срез 8 — ВТОРОЙ контур в том же слоте (вложенный клип).
///
/// Срез 4 оставил вложенному скруглённому клипу offscreen-уровень: контур в
/// шейдере был один, и второй `PushClipRoundedRect` внутри первого падал на
/// прежний путь. Перепись на `lenta.ru` (53 кадра) назвала цену: **все 43**
/// клипа, ушедших на уровень, ушли туда именно из-за занятого контура (ни
/// одного — из-за содержимого поддерева), а максимальная вложенность
/// скруглённых клипов на странице — ровно **2**. Один уровень — это три пасса
/// (разрез батча родителя, пасс уровня, композит), то есть 126 пассов из 178
/// за прогон.
///
/// Пересечение двух контуров считается произведением покрытий — ровно то, что
/// делал путь уровня: содержимое уровня рисовалось с активным ВНЕШНИМ контуром
/// (uniform), а композит `RRECT_CLIP_SHADER_SRC` домножал его на покрытие
/// ВНУТРЕННЕГО. Разница только в том, что произведение теперь берётся во
/// фрагменте, а не через восьмибитную текстуру уровня.
///
/// Второй контур неактивен → `clip2_half` = `NO_CLIP_HALF`: ветка uniform'а
/// одинакова для всего варпа, поэтому дивергенции у неё нет, а фрагменты
/// невложенных клипов (их подавляющее большинство) второй `sdf_rrect` не
/// считают вовсе.
pub(crate) const CLIP_UNIFORM_WGSL: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
    dpr: f32,
    _pad0: f32,
    clip_center: vec2<f32>,
    clip_half: vec2<f32>,
    clip_radii_x: vec4<f32>,
    clip_radii_y: vec4<f32>,
    clip2_center: vec2<f32>,
    clip2_half: vec2<f32>,
    clip2_radii_x: vec4<f32>,
    clip2_radii_y: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

fn rrect_clip_coverage(p_device: vec2<f32>) -> f32 {
    let p = p_device / u.dpr;
    let d = sdf_rrect(p - u.clip_center, u.clip_half, u.clip_radii_x, u.clip_radii_y);
    var cov = 1.0 - smoothstep(-0.5, 0.5, d);
    if u.clip2_half.x < 1.0e6 {
        let d2 = sdf_rrect(p - u.clip2_center, u.clip2_half, u.clip2_radii_x, u.clip2_radii_y);
        cov = cov * (1.0 - smoothstep(-0.5, 0.5, d2));
    }
    return cov;
}
"#;

/// SDF rounded-rect shader with elliptical per-corner radii.
/// Per-vertex data carries the rect's center, half-size, and two vec4s for
/// horizontal (x) and vertical (y) corner radii, enabling `border-radius: H/V`.
///
/// Vertex layout (matches `RRectVertex`):
///   loc 0  pos       vec2  – screen CSS-px position
///   loc 1  z         f32   – CSS depth px (transform-style: preserve-3d)
///   loc 2  color     vec4  – premultiplied RGBA
///   loc 3  center    vec2  – CSS-px center of the rounded rect
///   loc 4  half_size vec2  – CSS-px half-dimensions (w/2, h/2)
///   loc 5  radii_x   vec4  – horizontal corner radii px: tl, tr, br, bl
///   loc 6  radii_y   vec4  – vertical corner radii px:   tl, tr, br, bl
pub(crate) const RRECT_SHADER_SRC: &str = r#"
struct VIn {
    @location(0) pos:       vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_rrect_verts.
    @location(1) z:         f32,
    @location(2) color:     vec4<f32>,
    @location(3) center:    vec2<f32>,
    @location(4) half_size: vec2<f32>,
    @location(5) radii_x:   vec4<f32>,
    @location(6) radii_y:   vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color:     vec4<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) center:    vec2<f32>,
    @location(3) half_size: vec2<f32>,
    @location(4) radii_x:   vec4<f32>,
    @location(5) radii_y:   vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip      = vec4<f32>(ndc, depth, 1.0);
    out.color     = in.color;
    out.world_pos = in.pos;
    out.center    = in.center;
    out.half_size = in.half_size;
    out.radii_x   = in.radii_x;
    out.radii_y   = in.radii_y;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let p = in.world_pos - in.center;
    let d = sdf_rrect(p, in.half_size, in.radii_x, in.radii_y);
    // Sub-pixel anti-aliasing: smoothstep over [-0.5, 0.5] px.
    let alpha = (1.0 - smoothstep(-0.5, 0.5, d)) * rrect_clip_coverage(in.clip.xy);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

/// BUG-405 срез 7 — аналитическая размытая скруглённая rrect (`box-shadow`).
///
/// Раньше тень = `PushFilter [blur(σ)]` + одна заливка + `PopFilter`, то есть
/// offscreen-уровень и три пасса на тень: контент уровня, горизонтальный
/// проход блюра и слитый вертикальный+композит (срез 6). Здесь тень — обычная
/// операция внутри уже открытого батча родителя: пассов не добавляет вовсе.
///
/// Вместо свёртки растра фрагмент считает размытие фигуры **точно**, пользуясь
/// тем, что каждая строка скруглённого прямоугольника — один отрезок
/// `[left(y), right(y)]`:
///
/// ```text
/// blur(shape)(x,y) = Σ_j w_j · ∫ g_σ(x−t) dt   по t ∈ [left(y+j), right(y+j)]
/// ```
///
/// Внутренний интеграл гауссианы по отрезку — разность двух `erf`, внешняя
/// сумма по строкам берёт **те же** веса и тот же обрез ядра `min(ceil(3σ),32)`,
/// что и [`BLUR_SHADER_SRC`]: не «похожий блюр», а тот же самый по вертикали.
///
/// Единицы — device px, как у пассов блюра: там шаг ядра равен одному текселю
/// surface-текстуры, а σ приезжает из CSS без домножения на dpr. Поэтому
/// геометрия домножается на `u.dpr`, а σ — нет: на HiDPI аналитическая тень
/// повторяет прежнюю картинку, а не «чинит» её (это отдельный вопрос).
///
/// Вершинный формат (`ShadowVertex`) — `RRectVertex` плюс `sigma` в loc 7.
pub(crate) const SHADOW_SHADER_SRC: &str = r#"
struct VIn {
    @location(0) pos:       vec2<f32>,
    @location(1) z:         f32,
    @location(2) color:     vec4<f32>,
    @location(3) center:    vec2<f32>,
    @location(4) half_size: vec2<f32>,
    @location(5) radii_x:   vec4<f32>,
    @location(6) radii_y:   vec4<f32>,
    @location(7) sigma:     f32,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color:     vec4<f32>,
    @location(1) center:    vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) radii_x:   vec4<f32>,
    @location(4) radii_y:   vec4<f32>,
    @location(5) @interpolate(flat) sigma: f32,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip      = vec4<f32>(ndc, depth, 1.0);
    out.color     = in.color;
    out.center    = in.center;
    out.half_size = in.half_size;
    out.radii_x   = in.radii_x;
    out.radii_y   = in.radii_y;
    out.sigma     = in.sigma;
    return out;
}

// Abramowitz–Stegun 7.1.26: |ошибка| ≤ 1.5e-7 — на три порядка тоньше
// восьмибитного кванта цели, то есть на пиксель не влияет.
fn erf_approx(x: f32) -> f32 {
    let s = sign(x);
    let a = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * a);
    let poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t
        - 0.284496736) * t + 0.254829592) * t;
    return s * (1.0 - poly * exp(-a * a));
}

// Доля гауссианы с центром в `x`, попавшая в отрезок [a, b].
//
// Ядро обрезано на ±`radius` и перенормировано на собственную массу — ровно
// как дискретное ядро в [`BLUR_SHADER_SRC`], которое суммирует
// `min(ceil(3σ),32)` отсчётов и делит на их сумму. Без обреза у аналитики
// оставались бы хвосты за 3σ, которых у образца нет, и тень выходила бы
// систематически темнее (замер: +2/255 по всей полутени `1000000-final`).
fn gauss_span(a: f32, b: f32, x: f32, inv_sigma_sqrt2: f32, radius: f32) -> f32 {
    let lo = max(a, x - radius);
    let hi = min(b, x + radius);
    if hi <= lo {
        return 0.0;
    }
    let mass = erf_approx(radius * inv_sigma_sqrt2);
    return 0.5 * (erf_approx((hi - x) * inv_sigma_sqrt2) - erf_approx((lo - x) * inv_sigma_sqrt2))
        / mass;
}

// Полуширина строки `dy` (от центра) со стороны угла с радиусами (crx, cry).
// Вне углового пояса — прямой край, внутри — эллипс: x = crx·(1 − √(1 − t²)).
fn shadow_row_half(dy: f32, half: vec2<f32>, crx: f32, cry: f32) -> f32 {
    if crx < 0.001 || cry < 0.001 {
        return half.x;
    }
    let flat = half.y - cry;
    let ay = abs(dy);
    if ay <= flat {
        return half.x;
    }
    let t = clamp((ay - flat) / cry, 0.0, 1.0);
    return half.x - crx * (1.0 - sqrt(max(0.0, 1.0 - t * t)));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Всё в device px: p — от центра фигуры, ядро шагает по одному device px.
    let p = in.clip.xy - in.center * u.dpr;
    let half = in.half_size * u.dpr;
    let rx = in.radii_x * u.dpr;
    let ry = in.radii_y * u.dpr;
    let sigma = max(in.sigma, 0.001);
    let radius = min(i32(ceil(3.0 * sigma)), 32);
    let inv_sigma_sqrt2 = 1.0 / (sigma * 1.4142135623730951);
    var sum = 0.0;
    var weight_total = 0.0;
    for (var j = -radius; j <= radius; j = j + 1) {
        let fj = f32(j);
        let w = exp(-fj * fj / (2.0 * sigma * sigma));
        weight_total = weight_total + w;
        let dy = p.y + fj;
        // Доля строки, накрытая фигурой по вертикали. Прежний путь растеризовал
        // фигуру со сглаживанием (`1 − smoothstep(−0.5, 0.5, sdf)`) и лишь затем
        // размывал; без этого множителя верхний и нижний края тени
        // округлялись бы до целой строки — сдвиг края на полпикселя.
        let cover_y = clamp(half.y - abs(dy) + 0.5, 0.0, 1.0);
        if cover_y <= 0.0 {
            continue;
        }
        // y вниз: строка выше центра режется верхними углами (tl, tr),
        // ниже — нижними (bl, br).
        let top = dy < 0.0;
        let left_rx  = select(rx.w, rx.x, top);
        let left_ry  = select(ry.w, ry.x, top);
        let right_rx = select(rx.z, rx.y, top);
        let right_ry = select(ry.z, ry.y, top);
        let hl = shadow_row_half(dy, half, left_rx, left_ry);
        let hr = shadow_row_half(dy, half, right_rx, right_ry);
        sum = sum + w * cover_y * gauss_span(-hl, hr, p.x, inv_sigma_sqrt2, f32(radius));
    }
    let alpha = sum / weight_total * rrect_clip_coverage(in.clip.xy);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

pub(crate) const TEXT_SHADER_SRC: &str = r#"
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
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
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_tex, atlas_smp, in.uv).r * rrect_clip_coverage(in.clip.xy);
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

pub(crate) const IMAGE_SHADER_SRC: &str = r#"
@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_smp: sampler;

struct VIn {
    @location(0) pos: vec2<f32>,
    // CSS depth in pixels: positive = closer to viewer.
    // Mapped to WebGPU NDC [0=front, 1=back] via (0.5 - z/20000), identical to FillVertex.
    // CSS: transform-style — populated for preserve-3d by apply_affine_to_verts.
    @location(1) z: f32,
    @location(2) uv: vec2<f32>,
    @location(3) alpha: f32,
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
    let depth = clamp(0.5 - in.z / 20000.0, 0.0, 1.0);
    var out: VOut;
    out.clip = vec4<f32>(ndc, depth, 1.0);
    out.uv = in.uv;
    out.alpha = in.alpha;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sample = textureSample(image_tex, image_smp, in.uv);
    return vec4<f32>(sample.rgb, sample.a * in.alpha * rrect_clip_coverage(in.clip.xy));
}
"#;

/// Mip-каскад картинок (p1-exp-wgpu-only): fullscreen-triangle blit
/// «mip N−1 → mip N». Bilinear-выборка ровно между четырьмя текселями
/// источника = 2×2 box-фильтр — стандартный GPU-даунскейл (так же строит
/// mip-ы Chromium). Bind group — `image_bgl` (texture + sampler).
pub(crate) const MIPGEN_SHADER_SRC: &str = r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_smp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    // Fullscreen triangle: (-1,-1), (3,-1), (-1,3); uv (0,1), (2,1), (0,-1).
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32((vi >> 1u) & 1u) * 4.0 - 1.0;
    var out: VOut;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x, -y) * 0.5 + 0.5;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(src_tex, src_smp, in.uv);
}
"#;

/// CSS Images L4 §4 — `cross-fade(A, B, p)` shader.
///
/// Bindings:
/// * group 0 binding 0 — viewport uniform (shared with `image_pipeline`).
/// * group 1 binding 0 — `tex_a` (Rgba8Unorm).
/// * group 1 binding 1 — `tex_b` (Rgba8Unorm).
/// * group 1 binding 2 — shared `sampler` (filtering).
/// * group 1 binding 3 — `CrossFadeParams { progress: f32 }` uniform
///   (padded to 16 bytes for std140 alignment).
///
/// Fragment formula: `mix(sample_a, sample_b, progress)` — straight RGBA
/// interpolation (CSS Images L4 §4.2). Shader emits straight-alpha; pipeline
/// uses `ALPHA_BLENDING` so the GPU performs `SrcAlpha · src + (1-SrcAlpha) · dst`
/// — same convention as `image_pipeline`.
pub(crate) const CROSS_FADE_SHADER_SRC: &str = r#"
struct CrossFadeParams {
    // x = progress, yzw = padding (uniform buffer requires 16-byte alignment).
    progress: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(1) @binding(0) var tex_a: texture_2d<f32>;
@group(1) @binding(1) var tex_b: texture_2d<f32>;
@group(1) @binding(2) var smp: sampler;
@group(1) @binding(3) var<uniform> p: CrossFadeParams;

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
    // CrossFade is a flat 2D primitive: depth = 0.5 (mid plane), matching how
    // FillVertex maps z = 0.0 → 0.5. preserve-3d transforms are deferred.
    out.clip = vec4<f32>(ndc, 0.5, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(tex_a, smp, in.uv);
    let b = textureSample(tex_b, smp, in.uv);
    let t = clamp(p.progress, 0.0, 1.0);
    // CSS Images L4 §4.2 — the mix is defined on PREMULTIPLIED colours:
    //   Cout = (1−t)·αa·Ca + t·αb·Cb,  αout = (1−t)·αa + t·αb
    // Mixing straight-alpha RGB instead (the old `mix(a, b, t)`) multiplied a
    // fully transparent source's colour into the result and then let the blend
    // stage scale it by αout a second time — a 50 % cross-fade over a
    // transparent region came out roughly half as bright as Edge (BUG-277
    // срез 15). The textures hold straight RGBA and the pipeline blend state is
    // ALPHA_BLENDING (SrcAlpha·src + (1−SrcAlpha)·dst), so the shader has to
    // return the un-premultiplied colour Cout/αout together with αout.
    let premul = mix(a.rgb * a.a, b.rgb * b.a, t);
    let out_a = mix(a.a, b.a, t);
    if (out_a <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(premul / out_a, out_a * rrect_clip_coverage(in.clip.xy));
}
"#;

pub(crate) const COMPOSITE_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_layer: texture_2d<f32>;
@group(0) @binding(1) var s_layer: sampler;

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

@vertex fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    out.clip = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    out.alpha = in.alpha;
    return out;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_layer, s_layer, in.uv);
    // Off-screen layers accumulate premultiplied-alpha content (ALPHA_BLENDING onto clear).
    // Apply opacity to both rgb and alpha so premultiplied invariant is preserved.
    return vec4<f32>(c.rgb * in.alpha, c.a * in.alpha);
}
"#;

/// CSS Overflow L3 §2 / Backgrounds L3 §5.3 — rounded-clip composite shader.
///
/// Composites an offscreen level onto its parent through a rounded-rect
/// contour: the scissor rect can only cut a box, so the corners of an
/// `overflow: hidden` box with `border-radius` are carved here instead
/// (`PushClipRoundedRect` → level, `PopClip` → this pass).
///
/// Bindings mirror `COMPOSITE_SHADER_SRC` (group 0 = layer texture + sampler),
/// so the per-level `OffscreenLayer::bind_group` is reused as-is. Vertices
/// carry NDC position + UV (no viewport uniform needed) plus the CSS-px screen
/// position and the rounded-rect parameters for `sdf_rrect`.
/// Blend: PREMULTIPLIED_ALPHA_BLENDING — the level content is premultiplied and
/// the shader scales rgb and a by the same coverage, preserving the invariant.
pub(crate) const RRECT_CLIP_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_layer: texture_2d<f32>;
@group(0) @binding(1) var s_layer: sampler;

struct VIn {
    @location(0) pos:       vec2<f32>,
    @location(1) uv:        vec2<f32>,
    @location(2) world_pos: vec2<f32>,
    @location(3) center:    vec2<f32>,
    @location(4) half_size: vec2<f32>,
    @location(5) radii_x:   vec4<f32>,
    @location(6) radii_y:   vec4<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) center:    vec2<f32>,
    @location(3) half_size: vec2<f32>,
    @location(4) radii_x:   vec4<f32>,
    @location(5) radii_y:   vec4<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip      = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv        = in.uv;
    o.world_pos = in.world_pos;
    o.center    = in.center;
    o.half_size = in.half_size;
    o.radii_x   = in.radii_x;
    o.radii_y   = in.radii_y;
    return o;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let d = sdf_rrect(in.world_pos - in.center, in.half_size, in.radii_x, in.radii_y);
    // Same sub-pixel AA ramp as the rounded-rect fill, so a clipped child edge
    // lands on the same coverage as the container's own painted contour.
    let cov = 1.0 - smoothstep(-0.5, 0.5, d);
    if cov <= 0.0 { discard; }
    let c = textureSample(t_layer, s_layer, in.uv);
    return vec4<f32>(c.rgb * cov, c.a * cov);
}
"#;

/// Максимум вершин полигональной формы `clip-path`, которую wgpu клиппит
/// точно. Форма едет в uniform-буфер как `array<vec4<f32>, 32>` (две точки на
/// vec4), фрагментный шейдер обходит рёбра циклом. Всё, что длиннее (сильно
/// флэттеный `path()`), падает на исторический bbox-клип — лучше грубая
/// коробка, чем 200-итерационный цикл на каждый пиксель.
pub(crate) const PATH_CLIP_MAX_VERTS: usize = 64;

/// CSS Masking L1 §3 — composite-пасс формы `clip-path`.
/// Композитит offscreen-уровень на родителя через покрытие произвольной формы:
/// scissor умеет только коробку, поэтому круг/эллипс/полигон вырезаются здесь
/// (`PushClipPath` → уровень, `PopClip` → этот пасс) — тем же механизмом, что
/// `RRECT_CLIP_SHADER_SRC` вырезает скруглённые углы (BUG-277 срез 5).
///
/// Форма приходит уже в экранных CSS px (накопленный `PushTransform` применён
/// на CPU): у полигона — трансформированные вершины, у круга/эллипса — центр
/// и обратная матрица `inv_m` отображения «единичный круг → фигура», поэтому
/// поворот/скос/неравномерный масштаб поддержаны точно, а не через AABB.
///
/// Bindings: 0 = текстура уровня, 1 = sampler, 2 = uniform формы.
/// Blend: PREMULTIPLIED_ALPHA_BLENDING — содержимое уровня премультиплировано,
/// шейдер домножает rgb и a на одно покрытие (инвариант сохраняется).
pub(crate) const PATH_CLIP_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_layer: texture_2d<f32>;
@group(0) @binding(1) var s_layer: sampler;

struct ShapeUniform {
    /// x = вид формы (0 = эллипс/круг, 1 = полигон),
    /// y = число вершин полигона, z = 1 при even-odd fill rule.
    header:  vec4<u32>,
    /// xy = центр эллипса в экранных CSS px.
    center:  vec4<f32>,
    /// Обратная матрица (row-major) отображения единичного круга в фигуру.
    inv_m:   vec4<f32>,
    /// Вершины полигона в экранных CSS px, по две точки на vec4.
    verts:   array<vec4<f32>, 32>,
};
@group(0) @binding(2) var<uniform> u: ShapeUniform;

struct VIn {
    @location(0) pos:       vec2<f32>,
    @location(1) uv:        vec2<f32>,
    @location(2) world_pos: vec2<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>,
    @location(1) world_pos: vec2<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip      = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv        = in.uv;
    o.world_pos = in.world_pos;
    return o;
}

/// i-я вершина полигона (две точки упакованы в один vec4).
fn poly_vert(i: u32) -> vec2<f32> {
    let v = u.verts[i >> 1u];
    if (i & 1u) == 0u { return v.xy; }
    return v.zw;
}

/// Знаковое расстояние (CSS px, отрицательное внутри) до контура полигона.
/// Модуль — минимум по расстояниям до отрезков; знак — правило заливки
/// (CSS Shapes L1 §3/§4: nonzero по умолчанию, even-odd по `header.z`).
fn sdf_polygon(p: vec2<f32>, n: u32, even_odd: bool) -> f32 {
    var d2 = 1e20;
    var wind: i32 = 0;
    var crossings: u32 = 0u;
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        var j = i + 1u;
        if j == n { j = 0u; }
        let a = poly_vert(i);
        let b = poly_vert(j);
        let e = b - a;
        let w = p - a;
        let t = clamp(dot(w, e) / max(dot(e, e), 1e-12), 0.0, 1.0);
        let dv = w - e * t;
        d2 = min(d2, dot(dv, dv));
        // Winding number (Sunday): знак `cr` сам говорит, с какой стороны от
        // ребра лежит точка, поэтому сравнение по x тут не нужно.
        let cr = e.x * w.y - e.y * w.x;
        if a.y <= p.y {
            if b.y > p.y && cr > 0.0 { wind = wind + 1; }
        } else {
            if b.y <= p.y && cr < 0.0 { wind = wind - 1; }
        }
        // Even-odd — честный луч вправо: считаются ТОЛЬКО пересечения правее
        // точки, иначе чётность собирает обе стороны и всё оказывается снаружи.
        if (a.y > p.y) != (b.y > p.y) {
            let xi = a.x + (p.y - a.y) / e.y * e.x;
            if xi > p.x { crossings = crossings + 1u; }
        }
    }
    var inside = wind != 0;
    if even_odd { inside = (crossings & 1u) == 1u; }
    let d = sqrt(d2);
    if inside { return -d; }
    return d;
}

/// Знаковое расстояние (CSS px) до контура эллипса, заданного центром и
/// обратной матрицей `inv_m`. `length(inv_m·p)` = 1 на контуре; деление на
/// длину градиента переводит эту безразмерную величину в пиксели (первый
/// порядок — точен на самом контуре, где и лежит AA-кромка).
fn sdf_mapped_circle(p: vec2<f32>, inv_m: vec4<f32>) -> f32 {
    let q = vec2<f32>(inv_m.x * p.x + inv_m.y * p.y, inv_m.z * p.x + inv_m.w * p.y);
    let lq = length(q);
    let qn = q / max(lq, 1e-6);
    let g = vec2<f32>(inv_m.x * qn.x + inv_m.z * qn.y, inv_m.y * qn.x + inv_m.w * qn.y);
    return (lq - 1.0) / max(length(g), 1e-6);
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var d: f32;
    if u.header.x == 1u {
        d = sdf_polygon(in.world_pos, u.header.y, u.header.z == 1u);
    } else {
        d = sdf_mapped_circle(in.world_pos - u.center.xy, u.inv_m);
    }
    // Та же суб-пиксельная AA-рампа, что у скруглённого клипа и заливок —
    // кромка обрезанного ребёнка садится на кромку собственного контура.
    let cov = 1.0 - smoothstep(-0.5, 0.5, d);
    if cov <= 0.0 { discard; }
    let c = textureSample(t_layer, s_layer, in.uv);
    return vec4<f32>(c.rgb * cov, c.a * cov);
}
"#;

/// CSS Compositing & Blending L1 §8 blend shader.
/// Bindings: 0=t_src (offscreen element), 1=t_dst (copy of parent layer),
/// 2=sampler (shared), 3=blend_mode uniform (u32, padded to 16 bytes).
/// Blend mode u32 mapping: 0=Normal, 1=Multiply, 2=Screen, 3=Overlay,
/// 4=Darken, 5=Lighten, 6=ColorDodge, 7=ColorBurn, 8=HardLight, 9=SoftLight,
/// 10=Difference, 11=Exclusion, 12=Hue, 13=Saturation, 14=Color,
/// 15=Luminosity, 16=PlusLighter.
/// Output is written as pre-composited RGBA (REPLACE blend state).
pub(crate) const BLEND_SHADER_SRC: &str = r#"
@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var t_dst: texture_2d<f32>;
@group(0) @binding(2) var s_layer: sampler;

struct BlendUniform {
    mode: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};
@group(0) @binding(3) var<uniform> u: BlendUniform;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    out.clip = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

// ── Luminance / Saturation helpers (non-separable modes) ──────────────
fn lum(c: vec3<f32>) -> f32 {
    return 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var result = c;
    if n < 0.0 {
        result = l + (c - l) * l / (l - n);
    }
    let l2 = lum(result);
    let x2 = max(result.r, max(result.g, result.b));
    if x2 > 1.0 {
        result = l2 + (result - l2) * (1.0 - l2) / (x2 - l2);
    }
    return result;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    let d = l - lum(c);
    return clip_color(c + d);
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    // Sort components to find min/mid/max indices.
    var result = c;
    // Use if-chains to set min/mid/max channels.
    var cmin: f32; var cmid: f32; var cmax: f32;
    var imin: i32; var imid: i32; var imax: i32;
    let cv = array<f32, 3>(c.r, c.g, c.b);
    // Find indices of min, mid, max by sorting.
    if cv[0] <= cv[1] && cv[0] <= cv[2] {
        imin = 0;
        if cv[1] <= cv[2] { imid = 1; imax = 2; } else { imid = 2; imax = 1; }
    } else if cv[1] <= cv[0] && cv[1] <= cv[2] {
        imin = 1;
        if cv[0] <= cv[2] { imid = 0; imax = 2; } else { imid = 2; imax = 0; }
    } else {
        imin = 2;
        if cv[0] <= cv[1] { imid = 0; imax = 1; } else { imid = 1; imax = 0; }
    }
    cmin = cv[imin]; cmid = cv[imid]; cmax = cv[imax];
    var rmin: f32; var rmid: f32; var rmax: f32;
    if cmax > cmin {
        rmid = (cmid - cmin) * s / (cmax - cmin);
        rmax = s;
    } else {
        rmid = 0.0;
        rmax = 0.0;
    }
    rmin = 0.0;
    // Reconstruct result in original channel order.
    var arr = array<f32, 3>(0.0, 0.0, 0.0);
    arr[imin] = rmin;
    arr[imid] = rmid;
    arr[imax] = rmax;
    return vec3<f32>(arr[0], arr[1], arr[2]);
}

// ── Separable blend functions B(Cs, Cd) ───────────────────────────────
fn blend_channel(mode: u32, cs: f32, cd: f32) -> f32 {
    if mode == 1u { // Multiply
        return cs * cd;
    } else if mode == 2u { // Screen
        return cs + cd - cs * cd;
    } else if mode == 3u { // Overlay
        if cd <= 0.5 { return 2.0 * cs * cd; }
        else { return 1.0 - 2.0 * (1.0 - cs) * (1.0 - cd); }
    } else if mode == 4u { // Darken
        return min(cs, cd);
    } else if mode == 5u { // Lighten
        return max(cs, cd);
    } else if mode == 6u { // ColorDodge
        if cd == 0.0 { return 0.0; }
        else if cs == 1.0 { return 1.0; }
        else { return min(1.0, cd / (1.0 - cs)); }
    } else if mode == 7u { // ColorBurn
        if cd == 1.0 { return 1.0; }
        else if cs == 0.0 { return 0.0; }
        else { return 1.0 - min(1.0, (1.0 - cd) / cs); }
    } else if mode == 8u { // HardLight — Overlay with Cs/Cd swapped
        if cs <= 0.5 { return 2.0 * cs * cd; }
        else { return 1.0 - 2.0 * (1.0 - cs) * (1.0 - cd); }
    } else if mode == 9u { // SoftLight
        if cs <= 0.5 {
            return cd - (1.0 - 2.0 * cs) * cd * (1.0 - cd);
        } else {
            var d: f32;
            if cd <= 0.25 {
                d = ((16.0 * cd - 12.0) * cd + 4.0) * cd;
            } else {
                d = sqrt(cd);
            }
            return cd + (2.0 * cs - 1.0) * (d - cd);
        }
    } else if mode == 10u { // Difference
        return abs(cd - cs);
    } else if mode == 11u { // Exclusion
        return cs + cd - 2.0 * cs * cd;
    } else if mode == 16u { // PlusLighter
        return min(1.0, cs + cd);
    }
    // Normal (0) or unknown — alpha-over handled by compositor formula
    return cs;
}

// ── CSS Compositing L1 §8 general compositing formula ─────────────────
// Cs' = (1 - αd) × Cs + αd × B(Cd, Cs)        // §8 blending with the backdrop
// Co  = αs × Cs' + (1 - αs) × αd × Cd         // §7 source-over, premultiplied
// αo  = αs + αd × (1 - αs)
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let src = textureSample(t_src, s_layer, in.uv);
    let dst = textureSample(t_dst, s_layer, in.uv);
    let mode = u.mode;

    // Un-premultiply for blending: offscreen layers accumulate PREMULTIPLIED content
    // (each draw is composited onto them with straight-alpha ALPHA_BLENDING, which —
    // starting from a transparent-black clear — leaves rgb scaled by alpha). The CSS
    // Compositing L1 §8 formulas below (and blend_channel) expect straight Cs/Cd, so
    // divide it back out; a fully-transparent source/dest has no meaningful straight
    // color and `as_`/`ad` zero it out later in the compositing formula regardless.
    let as_ = src.a;
    let ad = dst.a;
    var cs = select(src.rgb / as_, vec3<f32>(0.0), as_ <= 0.0);
    var cd = select(dst.rgb / ad, vec3<f32>(0.0), ad <= 0.0);

    var blended: vec3<f32>;

    // Non-separable modes operate on full RGB vector.
    if mode == 12u { // Hue: hue of src, sat+lum of dst
        blended = set_lum(set_sat(cs, sat(cd)), lum(cd));
    } else if mode == 13u { // Saturation: sat of src, hue+lum of dst
        blended = set_lum(set_sat(cd, sat(cs)), lum(cd));
    } else if mode == 14u { // Color: hue+sat of src, lum of dst
        blended = set_lum(cs, lum(cd));
    } else if mode == 15u { // Luminosity: lum of src, hue+sat of dst
        blended = set_lum(cd, lum(cs));
    } else {
        // Separable modes — apply per channel.
        blended = vec3<f32>(
            blend_channel(mode, cs.r, cd.r),
            blend_channel(mode, cs.g, cd.g),
            blend_channel(mode, cs.b, cd.b),
        );
    }

    // Full CSS Compositing L1 §8 formula.
    //
    // Where the backdrop is transparent (ad = 0) the spec takes the SOURCE
    // colour, not B(Cd, Cs): Cs' = (1 - ad)*Cs + ad*B(Cd, Cs). The previous
    // form (`as*B + as*Cd*(1 - ad) + Cd*(1 - as)`) dropped the `ad` factor on
    // the blended term and substituted Cd where the spec takes Cs, so
    // multiply/difference over a TRANSPARENT backdrop (an element inside
    // `isolation: isolate`) came out black instead of the element's own
    // colour — BUG-277 slice 3. At ad = 1 (compositing into the opaque frame)
    // both forms agree, so non-isolated blends are unchanged.
    //
    // The result is PREMULTIPLIED (as/ad folded into co) — the same convention
    // offscreen layers accumulate and the composite pipeline expects
    // (`One`/`OneMinusSrcAlpha`). The previous form returned a straight colour,
    // which disagreed with the layer convention whenever ad < 1.
    let cs_blended = mix(cs, blended, ad);
    let ao = as_ + ad * (1.0 - as_);
    let co = as_ * cs_blended + (1.0 - as_) * ad * cd;
    if ao <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(co, ao);
}
"#;

/// CSS Masking L1 §4 — mask composite shader.
/// Group 0: viewport uniform (shared with fill/image pipelines).
/// Group 1: t_layer (offscreen element content), t_mask (mask image), s_layer.
///
/// Fragment output: content_sample.rgba * mask_sample.alpha — mask-mode: alpha.
/// `pos` (pixel space) is converted to NDC the same way as fill/image shaders.
/// `uv_layer` = pos / viewport (auto-derived in vertex shader; not a separate attribute).
/// `uv_mask` = UV within the mask image tile (0..1 per tile instance).
pub(crate) const MASK_COMPOSITE_SHADER_SRC: &str = r#"
struct Uniforms {
    viewport: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@group(1) @binding(0) var t_layer: texture_2d<f32>;
@group(1) @binding(1) var t_mask:  texture_2d<f32>;
@group(1) @binding(2) var s_layer: sampler;

struct VIn {
    @location(0) pos:     vec2<f32>,
    @location(1) uv_mask: vec2<f32>,
};
struct VOut {
    @builtin(position) clip:     vec4<f32>,
    @location(0)       uv_layer: vec2<f32>,
    @location(1)       uv_mask:  vec2<f32>,
};

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip     = vec4<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
        0.0, 1.0,
    );
    // uv_layer: sample the offscreen content layer at the same pixel position.
    o.uv_layer = in.pos / u.viewport;
    o.uv_mask  = in.uv_mask;
    return o;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(t_layer, s_layer, in.uv_layer);
    let m = textureSample(t_mask,  s_layer, in.uv_mask);
    // mask-mode: alpha — CSS Masking L1 §6.2 default for raster images.
    return vec4<f32>(c.rgb, c.a * m.a);
}
"#;

/// CSS Masking L1 §5 — mask-layer composite shader.
///
/// Two fragment entry points sharing one vertex shader:
/// - `fs_alpha`:  mask value = mask.a  (CSS mask-mode: alpha, default)
/// - `fs_luma`:   mask value = luma(mask.rgb) × mask.a  (mask-mode: luminance, ITU-R BT.709)
///
/// Group 0: viewport uniform. Group 1: { t_content, t_mask, s }.
/// `t_content` = scratch copy of the parent layer (element content saved before this pass).
/// `t_mask`    = the mask offscreen layer rendered between PushMaskLayer / PopMaskLayer.
///
/// Vertex: pos (CSS px, location 0) + uv (location 1, = pos/surface_size set at plan time).
/// Blend: REPLACE — overwrites parent layer at element rect without compositing.
/// This is correct because `t_content` already carries the full element alpha.
pub(crate) const MASK_LAYER_SHADER_SRC: &str = r#"
struct Uniforms { viewport: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var t_content: texture_2d<f32>;
@group(1) @binding(1) var t_mask:    texture_2d<f32>;
@group(1) @binding(2) var s:         sampler;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
        0.0, 1.0,
    );
    o.uv = in.uv;
    return o;
}

// mask-mode: alpha — use mask alpha channel directly (CSS Masking L1 §6.2).
@fragment fn fs_alpha(in: VOut) -> @location(0) vec4<f32> {
    let c  = textureSample(t_content, s, in.uv);
    let m  = textureSample(t_mask,    s, in.uv);
    let ma = m.a;
    return vec4<f32>(c.rgb * ma, c.a * ma);
}

// mask-mode: luminance — relative luminance × alpha (CSS Masking L1 §6.1, ITU-R BT.709).
@fragment fn fs_luma(in: VOut) -> @location(0) vec4<f32> {
    let c    = textureSample(t_content, s, in.uv);
    let m    = textureSample(t_mask,    s, in.uv);
    let luma = dot(m.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let ma   = luma * m.a;
    return vec4<f32>(c.rgb * ma, c.a * ma);
}
"#;

/// CSS Filter Effects Module L1 — color filter pipeline.
/// Bindings: 0=t_src (offscreen layer), 1=s_src (sampler), 2=FilterParams uniform.
/// Uses CompositeVertex layout. Blend: PREMULTIPLIED_ALPHA_BLENDING (the source is
/// an offscreen layer, whose content is premultiplied; `fs_main` returns premultiplied).
/// Kind values: 1=Brightness, 2=Contrast, 3=Grayscale, 4=HueRotate(rad), 5=Invert,
/// 6=Opacity, 7=Saturate, 8=Sepia. Kind=0 (Blur) is handled by the blur shader, not here.
/// Типы CSS-фильтров и общая вершинная часть — одинаковы у `filter`-шейдера и
/// у `blur-composite` (BUG-405 срез 6), поэтому вынесены в отдельный кусок:
/// две копии `apply_filter_fn` разъезжаются при первой же правке спеки.
const FILTER_TYPES_WGSL: &str = r#"
struct FilterEntry {
    kind: u32,
    amount: f32,
    _p0: u32,
    _p1: u32,
}
struct FilterParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    entries: array<FilterEntry, 8>,
}

struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) alpha: f32 }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv = in.uv;
    return o;
}
"#;

/// Биндинги `filter`-шейдера: слой-источник, сэмплер, список фильтров.
const FILTER_BINDINGS_WGSL: &str = r#"
@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> u: FilterParams;
"#;

/// Реализация функций CSS Filter Effects L1 §7 — общая для обоих шейдеров.
const FILTER_FN_WGSL: &str = r#"
fn apply_filter_fn(c: vec4<f32>, kind: u32, amount: f32) -> vec4<f32> {
    if kind == 1u { // Brightness
        return vec4<f32>(clamp(c.rgb * amount, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }
    if kind == 2u { // Contrast
        return vec4<f32>(clamp((c.rgb - 0.5) * amount + 0.5, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
    }
    if kind == 3u { // Grayscale
        let lum3 = vec3<f32>(dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
        return vec4<f32>(mix(c.rgb, lum3, amount), c.a);
    }
    if kind == 4u { // HueRotate (amount in radians)
        let cos_a = cos(amount);
        let sin_a = sin(amount);
        let r = dot(c.rgb, vec3<f32>(0.213+0.787*cos_a-0.213*sin_a, 0.715-0.715*cos_a-0.715*sin_a, 0.072-0.072*cos_a+0.928*sin_a));
        let g = dot(c.rgb, vec3<f32>(0.213-0.213*cos_a+0.143*sin_a, 0.715+0.285*cos_a+0.140*sin_a, 0.072-0.072*cos_a-0.283*sin_a));
        let b = dot(c.rgb, vec3<f32>(0.213-0.213*cos_a-0.787*sin_a, 0.715-0.715*cos_a+0.715*sin_a, 0.072+0.928*cos_a+0.072*sin_a));
        return vec4<f32>(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0), c.a);
    }
    if kind == 5u { // Invert
        return vec4<f32>(mix(c.rgb, 1.0 - c.rgb, amount), c.a);
    }
    if kind == 6u { // Opacity
        return vec4<f32>(c.rgb, c.a * amount);
    }
    if kind == 7u { // Saturate
        let r = dot(c.rgb, vec3<f32>(0.213+0.787*amount, 0.715-0.715*amount, 0.072-0.072*amount));
        let g = dot(c.rgb, vec3<f32>(0.213-0.213*amount, 0.715+0.285*amount, 0.072-0.072*amount));
        let b = dot(c.rgb, vec3<f32>(0.213-0.213*amount, 0.715-0.715*amount, 0.072+0.928*amount));
        return vec4<f32>(clamp(r, 0.0, 1.0), clamp(g, 0.0, 1.0), clamp(b, 0.0, 1.0), c.a);
    }
    if kind == 8u { // Sepia
        let sr = clamp(dot(c.rgb, vec3<f32>(0.393, 0.769, 0.189)), 0.0, 1.0);
        let sg = clamp(dot(c.rgb, vec3<f32>(0.349, 0.686, 0.168)), 0.0, 1.0);
        let sb = clamp(dot(c.rgb, vec3<f32>(0.272, 0.534, 0.131)), 0.0, 1.0);
        return vec4<f32>(mix(c.rgb, vec3<f32>(sr, sg, sb), amount), c.a);
    }
    return c;
}
"#;

/// Фрагментная часть `filter`-шейдера: цветовые фильтры над готовым слоем.
const FILTER_FS_WGSL: &str = r#"
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let src = textureSample(t_src, s_src, in.uv);
    // Offscreen-слои копят ПРЕМУЛЬТИПЛИРОВАННЫЙ цвет, а CSS Filter Effects L1
    // §7 определяет фильтры над НЕпремультиплированным: invert/contrast/
    // brightness от `rgb·a` — не то же, что `(invert rgb)·a`. Поэтому
    // распремультиплить → фильтры → премультиплить обратно. Результат снова
    // премультиплирован — этого ждут оба потребителя шейдера:
    // `filter_pipeline` (PREMULTIPLIED_ALPHA_BLENDING) и
    // `backdrop_blit_pipeline` (REPLACE в премультиплированный слой).
    let straight = select(src.rgb / max(src.a, 1e-6), vec3<f32>(0.0), src.a <= 0.0);
    var c = vec4<f32>(straight, src.a);
    for (var i = 0u; i < u.count; i = i + 1u) {
        c = apply_filter_fn(c, u.entries[i].kind, u.entries[i].amount);
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}
"#;

/// Биндинги `blur-composite` (BUG-405 срез 6): к тем же слою/сэмплеру
/// добавлен uniform блюра — вертикальный проход и цветовые фильтры считаются
/// одним пассом прямо в родителя.
const BLUR_COMPOSITE_BINDINGS_WGSL: &str = r#"
struct BlurParams {
    sigma: f32,
    direction: u32,   // всегда 1 (вертикальный проход)
    _p0: u32,
    _p1: u32,
}

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> b: BlurParams;
@group(0) @binding(3) var<uniform> u: FilterParams;
"#;

/// Фрагментная часть `blur-composite`: вертикальный проход сепарабельного
/// гаусса по горизонтально размытому источнику + цветовые фильтры.
/// Ядро повторяет [`BLUR_SHADER_SRC`] буква в букву — иначе два прохода
/// одного и того же блюра давали бы разный результат.
const BLUR_COMPOSITE_FS_WGSL: &str = r#"
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sigma = max(b.sigma, 0.001);
    let radius = min(i32(ceil(3.0 * sigma)), 32);
    let dim = vec2<f32>(textureDimensions(t_src));
    let step = select(vec2<f32>(1.0 / dim.x, 0.0), vec2<f32>(0.0, 1.0 / dim.y), b.direction == 1u);
    var sum = vec4<f32>(0.0);
    var weight_total = 0.0;
    for (var i = -radius; i <= radius; i = i + 1) {
        let fi = f32(i);
        let w = exp(-fi * fi / (2.0 * sigma * sigma));
        sum = sum + textureSample(t_src, s_src, in.uv + fi * step) * w;
        weight_total = weight_total + w;
    }
    let src = sum / weight_total;
    // Дальше — тот же путь, что в `FILTER_FS_WGSL`: слой премультиплирован,
    // фильтры CSS Filter Effects L1 §7 определены над непремультиплированным.
    let straight = select(src.rgb / max(src.a, 1e-6), vec3<f32>(0.0), src.a <= 0.0);
    var c = vec4<f32>(straight, src.a);
    for (var i = 0u; i < u.count; i = i + 1u) {
        c = apply_filter_fn(c, u.entries[i].kind, u.entries[i].amount);
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}
"#;

/// Исходник шейдера цветовых CSS-фильтров (склейка общих кусков).
pub(crate) fn filter_shader_src() -> String {
    [FILTER_TYPES_WGSL, FILTER_BINDINGS_WGSL, FILTER_FN_WGSL, FILTER_FS_WGSL].concat()
}

/// Исходник шейдера «вертикальный блюр + фильтры + композит» (BUG-405 срез 6).
pub(crate) fn blur_composite_shader_src() -> String {
    [
        FILTER_TYPES_WGSL,
        BLUR_COMPOSITE_BINDINGS_WGSL,
        FILTER_FN_WGSL,
        BLUR_COMPOSITE_FS_WGSL,
    ]
    .concat()
}

/// CSS Filter Effects — separable Gaussian blur shader (one pass: H or V).
/// Bindings: 0=t_src, 1=s_src (linear sampler), 2=BlurParams uniform.
/// Uses CompositeVertex layout. Blend: REPLACE (intermediate buffer pass).
pub(crate) const BLUR_SHADER_SRC: &str = r#"
struct BlurParams {
    sigma: f32,
    direction: u32,   // 0 = horizontal, 1 = vertical
    _p0: u32,
    _p1: u32,
}

@group(0) @binding(0) var t_src: texture_2d<f32>;
@group(0) @binding(1) var s_src: sampler;
@group(0) @binding(2) var<uniform> u: BlurParams;

struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) alpha: f32 }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv = in.uv;
    return o;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let sigma = max(u.sigma, 0.001);
    let radius = min(i32(ceil(3.0 * sigma)), 32);
    let dim = vec2<f32>(textureDimensions(t_src));
    let step = select(vec2<f32>(1.0 / dim.x, 0.0), vec2<f32>(0.0, 1.0 / dim.y), u.direction == 1u);
    var sum = vec4<f32>(0.0);
    var weight_total = 0.0;
    for (var i = -radius; i <= radius; i = i + 1) {
        let fi = f32(i);
        let w = exp(-fi * fi / (2.0 * sigma * sigma));
        sum = sum + textureSample(t_src, s_src, in.uv + fi * step) * w;
        weight_total = weight_total + w;
    }
    return sum / weight_total;
}
"#;

/// CSS Images L3 §3.3 — GPU gradient pipeline shader (linear + radial).
///
/// Single shader module handles both kinds via `gp.kind` uniform (0=linear, 1=radial).
///
/// Group 0, binding 0: viewport uniform (shared with fill pipeline).
/// Group 1, binding 0: GradParams uniform — gradient line/center/stops.
///
/// Vertex layout (GradVertex): loc 0 = pos (CSS px), loc 1 = uv [0,1]×[0,1].
/// UV is baked into vertices as normalized rect coordinates; the fragment
/// shader uses UV directly without needing rect bounds in the uniform.
///
/// Linear gradient: p0=(sx,sy), p1=(ex,ey) are gradient-line endpoints
/// in UV space, `param0` is the box aspect h/w. The projection is weighted by
/// that aspect so the iso-colour bands keep their pixel-space angle
/// (0 at start, 1 at end).
///
/// Radial gradient: p0=(cx,cy) is center in UV space; p1=(rx,ry) are
/// semi-axes (farthest-corner size) in UV space.
/// t = length((uv-p0)/p1)  (0 at center, 1 at ellipse edge).
///
/// Conic gradient (CSS Images L4 §3.7): p0=(cx,cy) is center in UV space;
/// p1=(w,h) is box size in CSS px (for box-space angle calculation);
/// `param0` is starting angle in radians (0 = top, clockwise).
/// t = (atan2(dx_box, -dy_box) - param0) / (2π), wrapped to [0,1].
///
/// `repeating` folds `t` into the **stop period** `[first, last]`, not into
/// `[0,1]` (see `wrap_repeat` below), and stop-to-stop interpolation runs in
/// premultiplied sRGBA (`mix_premul`, CSS Images L4 §3.1).
pub(crate) const GRADIENT_SHADER_SRC: &str = r#"
struct GradStop {
    color: vec4<f32>,
    pos:   f32,
    _p0:   f32, _p1: f32, _p2: f32,
}
// CSS Images L4 §3.1 — the stop list is a RUNTIME-SIZED array in a read-only
// storage buffer, not a fixed uniform array. A gradient carrying an
// interpolation space (`in oklab`, `in lab`, …) is polyfilled by densifying
// every segment into 16 sub-stops (`densify_gradient_stops_for_space`), so even
// the two-stop `linear-gradient(to right in oklab, red, blue)` arrives with 17
// stops — one more than the old `array<GradStop, 16>` could hold, and the tail
// was dropped silently (BUG-277 срез 11).
struct GradParams {
    p0:        vec2<f32>,
    p1:        vec2<f32>,
    n_stops:   u32,
    kind:      u32,
    repeating: u32,
    param0:    f32,
    stops: array<GradStop>,
}
@group(1) @binding(0) var<storage, read> gp: GradParams;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32> }
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex fn vs_main(in: VIn) -> VOut {
    let ndc = vec2<f32>(
        in.pos.x / u.viewport.x * 2.0 - 1.0,
        1.0 - in.pos.y / u.viewport.y * 2.0,
    );
    return VOut(vec4<f32>(ndc, 0.0, 1.0), in.uv);
}

// CSS Images L3 §3.6 — a repeating gradient tiles the span between its FIRST
// and LAST stop, which is not [0,1]: `repeating-linear-gradient(45deg, #333 0
// 10px, #666 10px 20px)` on a 200px line resolves to positions 0…0.1, so
// folding t by 1.0 leaves every pixel past 0.1 clamped to the last stop — a
// flat colour instead of stripes. Fold by the stop period instead; mirrors
// tiny-skia `SpreadMode::Repeat` over the rescaled `[first,last]` tile in
// `cpu_raster::skia_gradient_stops`.
fn wrap_repeat(t: f32) -> f32 {
    if gp.repeating == 0u || gp.n_stops < 2u { return t; }
    let lo = gp.stops[0].pos;
    let span = gp.stops[gp.n_stops - 1u].pos - lo;
    if span <= 0.0001 { return t; }
    let rel = t - lo;
    return lo + rel - floor(rel / span) * span;
}

// CSS Images L4 §3.1 — gradient colour interpolation is defined in
// PREMULTIPLIED sRGBA. Straight `mix()` drags a fade to `transparent`
// (rgba(0,0,0,0)) toward black, so a red→transparent layer darkens whatever it
// covers. The raster backends approximate this by subdividing such segments
// (`gradient_math::premultiplied_subdivide_stops`, BUG-190); on the GPU the
// exact form costs nothing.
fn mix_premul(a: vec4<f32>, b: vec4<f32>, f: f32) -> vec4<f32> {
    let pa = mix(a.a, b.a, f);
    if pa <= 0.0001 { return vec4<f32>(0.0); }
    let rgb = mix(a.rgb * a.a, b.rgb * b.a, f) / pa;
    return vec4<f32>(rgb, pa);
}

fn sample_grad(t_in: f32) -> vec4<f32> {
    if gp.n_stops == 0u { return vec4<f32>(0.0); }
    // Repeating `t` arrives already folded into the stop period by
    // `wrap_repeat`; only the non-repeating case clamps to the gradient line.
    var t = t_in;
    if gp.repeating == 0u {
        t = clamp(t, 0.0, 1.0);
    }
    if gp.n_stops == 1u { return gp.stops[0].color; }
    if t <= gp.stops[0].pos { return gp.stops[0].color; }
    let last = gp.n_stops - 1u;
    if t >= gp.stops[last].pos { return gp.stops[last].color; }
    for (var i = 0u; i + 1u < gp.n_stops; i = i + 1u) {
        let a = gp.stops[i];
        let b = gp.stops[i + 1u];
        if t >= a.pos && t <= b.pos {
            let span = b.pos - a.pos;
            let f = select(0.0, (t - a.pos) / span, span > 0.0001);
            return mix_premul(a.color, b.color, f);
        }
    }
    return gp.stops[last].color;
}

@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var t: f32;
    if gp.kind == 0u {
        // The gradient line is projected in PIXEL space, not in the squashed
        // UV box. `p0`/`p1` are UV endpoints, so a naive dot() in UV tilts the
        // iso-colour bands by the box aspect ratio: `linear-gradient(45deg,…)`
        // on a 180×80 box came out at ~11° instead of 45°. `param0` carries
        // h/w for kind 0, which is all the pixel metric needs (dividing both
        // sums by w² leaves a single k² weight on the y term).
        let k2 = gp.param0 * gp.param0;
        let d = gp.p1 - gp.p0;
        let rel = in.uv - gp.p0;
        let den = d.x * d.x + d.y * d.y * k2;
        t = select(0.0, (rel.x * d.x + rel.y * d.y * k2) / den, den > 0.0000001);
    } else if gp.kind == 1u {
        let rel = (in.uv - gp.p0) / gp.p1;
        t = length(rel);
    } else {
        // Conic: convert UV offset back to box-space pixels so the polar
        // angle is computed in the box coordinate system (CSS spec).
        let dx = (in.uv.x - gp.p0.x) * gp.p1.x;
        let dy = (in.uv.y - gp.p0.y) * gp.p1.y;
        // CSS convention: 0° = top (-y), angles grow clockwise.
        // atan2(dx, -dy) gives the angle measured CW from -y axis.
        let two_pi = 6.2831853;
        let raw = atan2(dx, -dy) - gp.param0;
        let frac = raw / two_pi;
        t = frac - floor(frac);  // [0, 1) — one full revolution
    }
    // Repeating conic (CSS Images L4 §3.7) tiles the stop span within one
    // revolution — the same fold linear/radial need, so it lives in one place.
    let c = sample_grad(wrap_repeat(t));
    return vec4<f32>(c.rgb, c.a * rrect_clip_coverage(in.clip.xy));
}
"#;

