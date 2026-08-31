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
//! Глифы растеризуются по требованию через `lumen_font::Rasterizer` на
//! **подобранный bin размера** (`size_bin_for(font_size)`). Bin-набор —
//! `SIZE_BINS = [8, 12, 16, 20, 24, 32, 48, 64]`; font_size округляется
//! вверх до ближайшего bin (или до 64 если больше). Display-сторона
//! масштабирует квад в долю `font_size / size_bin` — если font_size совпал
//! с bin-ом (16/24 px), масштаба нет вовсе. Это устраняет blur от линейной
//! интерполяции fixed-size атласа (раньше всё рисовалось на 24 px и потом
//! масштабировалось).

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::ColorSpace;
use lumen_core::ext::{FaceRecord, FontProvider, FontStyle as CssFontStyle};
use lumen_core::geom::Rect;
use lumen_font::{
    Bitmap, Colr, Cpal, Font, Head, Hmtx, Outline, OwnedCmap, Rasterizer,
    SystemFontIndex, maybe_decode_font,
};
use lumen_image::{correct_rgba_pixels, Image, PixelFormat};
use lumen_layout::{BackgroundRepeat, BackgroundSize, BorderStyle, Color, FilterFn, FontStretch, FontStyle, FontWeight, GradientStop, ImageRendering, Mat4, ObjectFit, ObjectPosition, OutlineStyle, PositionComponent, font_palette::FontPaletteSelection, style::TextOrientation};
use winit::window::Window;

use crate::atlas::{AtlasKey, GlyphAtlas, GlyphEntry, InsertOutcome};
use crate::display_list::{
    fit_image_quad, fit_image_rect, space_axis_geometry, BlendMode, CornerRadii, MaskMode,
    ResolvedClipShape,
};
use crate::fingerprint::GpuFingerprint;
use lumen_image::{resize_area_avg, resize_bilinear};
use crate::DisplayCommand;

/// Размер атласа в пикселях (квадратный). Поднят с 512 до 1024 под
/// multi-size atlas: типичная страница использует 2-3 размера шрифта,
/// что даёт ~3× больше уникальных глифов в кеше.
const ATLAS_DIM: u32 = 1024;

/// Минимальный запас полосы скролл-композитора с каждой стороны вьюпорта,
/// в долях его высоты (BUG-405 срез 22).
///
/// Полная полоса — 0.75 вьюпорта сверху и снизу; когда столько не влезает в
/// `max_texture_dimension_2d`, запас режется, но ниже этой доли полоса теряет
/// смысл: промах случается почти каждым кадром прокрутки, а промах стоит
/// рендера всей полосы, то есть дороже монолита. При 0.25 и типичном шаге
/// колеса (~120 CSS px) промах приходится примерно на каждый третий кадр —
/// остальные обслуживает дешёвая композиция.
const BAND_MIN_MARGIN_RATIO: f32 = 0.25;

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
const BAND_MARGIN_CAP_CSS: f32 = 768.0;

/// CSS-`z` квада, которым пасс кромки кольцевой полосы заливает её фон
/// (BUG-405 срез 32).
///
/// Шейдер переводит CSS-`z` в глубину как `0.5 − z/20000`, обрезая в `[0,1]`,
/// поэтому −10000 — это ровно дальняя плоскость: квад пишет ту же глубину 1.0,
/// которую оставил бы `LoadOp::Clear(1.0)`, и не отбраковывает содержимое с
/// отрицательным `z-index`, как отбраковал бы фон на `z = 0`.
const BAND_STRIP_BG_Z: f32 = -10_000.0;

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
const MAX_TEXTURE_DIM_TARGET: u32 = 8192;

/// Bin размеров растеризации (CSS px). `font_size` округляется до
/// ближайшего bin вверх через `size_bin_for`. Если ≤ 8 — используется
/// bin 8 (нечитаемо иначе всё равно); если > 64 — bin 64 с up-scaling-ом
/// (большие заголовки редки, потеря качества на единичных headline-ах
/// приемлема в Phase 0). При совпадении font_size с bin-ом квад не
/// масштабируется (нет blur).
const SIZE_BINS: [u16; 8] = [8, 12, 16, 20, 24, 32, 48, 64];

/// CSS px → размер растеризации в `SIZE_BINS`. Round-up до ближайшего bin;
/// > последнего bin — клампим к последнему.
fn size_bin_for(font_size: f32) -> u16 {
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
fn atlas_key(
    face_id: usize,
    glyph_id: u16,
    size_bin: u16,
    coords_hash: u64,
) -> AtlasKey {
    AtlasKey::new((face_id & 0xFFFF) as u16, glyph_id, size_bin, coords_hash)
}

const FILL_SHADER_SRC: &str = r#"
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
const CIRCLE_SHADER_SRC: &str = r#"
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
const SDF_RRECT_WGSL: &str = r#"
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
const CLIP_UNIFORM_WGSL: &str = r#"
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
const RRECT_SHADER_SRC: &str = r#"
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
const SHADOW_SHADER_SRC: &str = r#"
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

const TEXT_SHADER_SRC: &str = r#"
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

const IMAGE_SHADER_SRC: &str = r#"
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
const MIPGEN_SHADER_SRC: &str = r#"
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
const CROSS_FADE_SHADER_SRC: &str = r#"
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

const COMPOSITE_SHADER_SRC: &str = r#"
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
const RRECT_CLIP_SHADER_SRC: &str = r#"
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
const PATH_CLIP_MAX_VERTS: usize = 64;

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
const PATH_CLIP_SHADER_SRC: &str = r#"
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
const BLEND_SHADER_SRC: &str = r#"
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
const MASK_COMPOSITE_SHADER_SRC: &str = r#"
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
const MASK_LAYER_SHADER_SRC: &str = r#"
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
fn filter_shader_src() -> String {
    [FILTER_TYPES_WGSL, FILTER_BINDINGS_WGSL, FILTER_FN_WGSL, FILTER_FS_WGSL].concat()
}

/// Исходник шейдера «вертикальный блюр + фильтры + композит» (BUG-405 срез 6).
fn blur_composite_shader_src() -> String {
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
const BLUR_SHADER_SRC: &str = r#"
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
const GRADIENT_SHADER_SRC: &str = r#"
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

#[repr(C)]
#[derive(Copy, Clone)]
struct FillVertex {
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D elements;
    /// populated from `project_point_z` for 3D-transformed elements (CSS Transforms L2).
    /// Shader maps this to WebGPU NDC depth [0,1] so `CompareFunction::LessEqual` gives
    /// correct occlusion: closer elements (higher z) have lower depth value and win.
    z: f32,
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct TextVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D text;
    /// populated by `apply_affine_to_verts` via `VertexPos::set_depth` when the
    /// glyph quad is under a 3D CSS transform. Shader maps to WebGPU NDC depth
    /// via the same `0.5 - z/20000` formula as `FillVertex`, so depth testing
    /// is consistent across all vertex types in a `preserve-3d` rendering context.
    z: f32,
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D images;
    /// populated by `apply_affine_to_verts` for 3D-transformed image quads. Same
    /// NDC mapping as `FillVertex`/`TextVertex` for cross-type depth testing.
    z: f32,
    uv: [f32; 2],
    alpha: f32,
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
struct CrossFadeVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// UV in `[0,1]×[0,1]` over the destination rect — applied to both
    /// `tex_a` and `tex_b` (CSS Images L4 §4.1: images stretched to fit dest).
    uv: [f32; 2],
}

/// Вершина для SDF-круга. `uv` — нормализованные координаты (-1..1) от центра
/// (quad расширен на 0.5px в каждую сторону). `radius_px` — CSS-радиус точки.
/// Layout: pos(8) + uv(8) + color(16) + radius_px(4) = 36 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct CircleVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// UV in [-1,1] over the expanded quad (CSS_radius + 0.5 in each direction).
    uv: [f32; 2],
    /// RGBA color.
    color: [f32; 4],
    /// CSS radius of the dot in pixels (= border_width / 2).
    radius_px: f32,
}

/// Вершина для SDF-скруглённого прямоугольника (`RRECT_SHADER_SRC`).
/// `center`/`half_size`/`radii_x`/`radii_y` одинаковы для всех 6 вершин одного quad-а
/// и передаются как interpolants (константны внутри одного треугольника).
/// Layout: pos(8) + z(4) + color(16) + center(8) + half_size(8) + radii_x(16) + radii_y(16) = 76 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct RRectVertex {
    /// Screen position in CSS pixels.
    pos: [f32; 2],
    /// CSS depth in pixels (positive = closer to viewer). Set to 0.0 for 2D rrect;
    /// populated by `apply_affine_to_rrect_verts` for 3D-transformed quads.
    /// Same NDC mapping as `FillVertex` so border-radius backgrounds participate
    /// correctly in cross-type depth testing under CSS Transforms L2 `preserve-3d`.
    z: f32,
    /// RGBA color (linear premultiplied alpha is handled by blend state).
    color: [f32; 4],
    /// Center of the rounded rect in CSS pixels.
    center: [f32; 2],
    /// Half-dimensions of the rect: (width/2, height/2).
    half_size: [f32; 2],
    /// Horizontal corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 5.
    radii_x: [f32; 4],
    /// Vertical corner radii in CSS pixels: [tl, tr, br, bl]. Matches WGSL loc 6.
    /// Equal to `radii_x` for circular corners; differs for elliptical (`border-radius: H/V`).
    radii_y: [f32; 4],
}

/// Вершина аналитической размытой тени (`SHADOW_SHADER_SRC`, BUG-405 срез 7).
/// Поля `RRectVertex` один в один плюс `sigma` — квад тени шире самой фигуры на
/// радиус ядра, поэтому `pos` и `center`/`half_size` здесь расходятся.
/// Layout: pos(8) + z(4) + color(16) + center(8) + half_size(8) + radii_x(16)
/// + radii_y(16) + sigma(4) = 80 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct ShadowVertex {
    /// Позиция вершины квада в CSS px (фигура + запас на ядро блюра).
    pos: [f32; 2],
    /// CSS-глубина, как у [`RRectVertex::z`].
    z: f32,
    /// Цвет тени RGBA (прямая альфа; премультиплицирование — дело blend-state).
    color: [f32; 4],
    /// Центр размываемой фигуры в CSS px.
    center: [f32; 2],
    /// Полуразмеры фигуры (w/2, h/2) в CSS px.
    half_size: [f32; 2],
    /// Горизонтальные радиусы углов в CSS px: [tl, tr, br, bl].
    radii_x: [f32; 4],
    /// Вертикальные радиусы углов в CSS px: [tl, tr, br, bl].
    radii_y: [f32; 4],
    /// σ гауссианы — в тех же единицах, что у пассов блюра (device px).
    sigma: f32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CompositeVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

/// Вершина composite-пасса скруглённого клипа (`RRECT_CLIP_SHADER_SRC`).
/// Как `CompositeVertex` (NDC + UV, без viewport-uniform), плюс параметры
/// контура для `sdf_rrect`: они одинаковы для всех 6 вершин quad-а.
/// Layout: pos(8) + uv(8) + world_pos(8) + center(8) + half_size(8)
/// + radii_x(16) + radii_y(16) = 72 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct RRectClipVertex {
    /// Position in NDC (clip-space), same convention as `CompositeVertex::pos`.
    pos: [f32; 2],
    /// UV into the offscreen level texture.
    uv: [f32; 2],
    /// Screen position in CSS pixels — the space `center`/`half_size` live in.
    world_pos: [f32; 2],
    /// Center of the clip contour in CSS pixels.
    center: [f32; 2],
    /// Half-dimensions of the clip rect: (width/2, height/2).
    half_size: [f32; 2],
    /// Horizontal corner radii in CSS pixels: [tl, tr, br, bl].
    radii_x: [f32; 4],
    /// Vertical corner radii in CSS pixels: [tl, tr, br, bl].
    radii_y: [f32; 4],
}

/// Вершина composite-пасса формы `clip-path` (`PATH_CLIP_SHADER_SRC`).
/// Как `CompositeVertex` (NDC + UV), плюс экранная позиция в CSS px — форма
/// живёт в uniform-буфере, поэтому per-vertex параметров контура тут нет.
/// Layout: pos(8) + uv(8) + world_pos(8) = 24 bytes.
#[repr(C)]
#[derive(Copy, Clone)]
struct PathClipVertex {
    /// Position in NDC (clip-space), same convention as `CompositeVertex::pos`.
    pos: [f32; 2],
    /// UV into the offscreen level texture.
    uv: [f32; 2],
    /// Screen position in CSS pixels — the space the shape lives in.
    world_pos: [f32; 2],
}

/// CPU-зеркало WGSL `ShapeUniform` из [`PATH_CLIP_SHADER_SRC`].
/// Все координаты — экранные CSS px (накопленный transform уже применён).
#[repr(C)]
#[derive(Copy, Clone)]
struct PathClipParamsCpu {
    /// [вид формы (0 = эллипс, 1 = полигон), число вершин, even-odd, pad].
    header: [u32; 4],
    /// [cx, cy, pad, pad] — центр эллипса.
    center: [f32; 4],
    /// Обратная матрица (row-major) отображения «единичный круг → эллипс».
    inv_m: [f32; 4],
    /// Вершины полигона, по две точки на `vec4`.
    verts: [[f32; 4]; PATH_CLIP_MAX_VERTS / 2],
}

/// CSS Masking L1 §4 — вершина mask-composite пайплайна.
/// `pos` — pixel-space (convert to NDC via viewport uniform).
/// `uv_mask` — UV [0,1]×[0,1] в пределах одной плитки mask-изображения.
/// `uv_layer` вычисляется в вершинном шейдере из `pos / viewport`.
#[repr(C)]
#[derive(Copy, Clone)]
struct MaskVertex {
    pos: [f32; 2],
    uv_mask: [f32; 2],
}

/// CPU-side зеркало WGSL `FilterEntry` (kind:u32, amount:f32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct FilterEntryCpu { kind: u32, amount: f32, _p0: u32, _p1: u32 }

/// CPU-side зеркало WGSL `FilterParams` (16 bytes header + 8×FilterEntry = 144 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct FilterParamsCpu {
    count: u32, _pad0: u32, _pad1: u32, _pad2: u32,
    entries: [FilterEntryCpu; 8],
}

/// CPU-side зеркало WGSL `BlurParams` (sigma:f32, direction:u32, 2×u32 pad = 16 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct BlurParamsCpu { sigma: f32, direction: u32, _p0: u32, _p1: u32 }

/// CSS Images L3 §3.3 — вершина градиентного пайплайна.
/// `uv` — нормализованные координаты [0,1]×[0,1] внутри прямоугольника градиента,
/// бейкятся в вершины, чтобы фрагментный шейдер не нуждался в размерах rect в uniform.
#[repr(C)]
#[derive(Copy, Clone)]
struct GradVertex {
    /// CSS pixel position.
    pos: [f32; 2],
    /// Normalized rect coords: (0,0)=TL, (1,1)=BR.
    uv: [f32; 2],
}

/// CPU-side зеркало WGSL `GradStop` (color: vec4 + pos: f32 + 12 bytes pad = 32 bytes).
#[repr(C)]
#[derive(Copy, Clone)]
struct GradStopCpu {
    color: [f32; 4],
    pos: f32,
    _p0: f32, _p1: f32, _p2: f32,
}

/// CPU-side зеркало заголовка WGSL `GradParams` — 32 байта, ровно до
/// runtime-sized массива стопов (WGSL требует выравнивания `array<GradStop>`
/// на 16 байт, заголовок уже кратен). Стопы дописываются в тот же storage
/// buffer сразу за заголовком, см. [`GradParamsCpu`] и `write_grad_buffer`.
#[repr(C)]
#[derive(Copy, Clone)]
struct GradHeaderCpu {
    /// Linear: (sx, sy) — gradient-line start in UV [0,1].
    /// Radial: (cx, cy) — center in UV [0,1].
    /// Conic:  (cx, cy) — center in UV [0,1].
    p0: [f32; 2],
    /// Linear: (ex, ey) — gradient-line end (used with p0 start).
    /// Radial: (rx, ry) — farthest-corner semi-axes in UV [0,1].
    /// Conic:  (w, h) — box dimensions in CSS pixels (for box-space angle).
    p1: [f32; 2],
    n_stops: u32,
    /// 0 = linear, 1 = radial, 2 = conic.
    kind: u32,
    /// 0 = clamp, 1 = repeating (fold t into the `[first, last]` stop period).
    repeating: u32,
    /// Conic: starting angle in radians (0 = top, CW). Linear: box aspect
    /// `h/w` (see [`box_aspect`]). Unused for radial.
    param0: f32,
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
struct GradParamsCpu {
    /// Скалярная часть — байт-в-байт заголовок WGSL-структуры.
    header: GradHeaderCpu,
    /// Стопы в порядке возрастания позиции; пишутся со смещения 32.
    stops: Vec<GradStopCpu>,
}

/// Конвертирует `FilterFn` в `FilterEntryCpu` для GPU uniform.
/// Blur (kind=0) передаётся как is; color-filter pass пропускает его по kind.
fn filter_fn_to_entry(f: &FilterFn) -> FilterEntryCpu {
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
enum DrawOp {
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
struct GpuImage {
    /// Linear (bilinear) filtered bind group — default for auto/smooth.
    bind_group_linear: wgpu::BindGroup,
    /// Nearest-neighbor filtered bind group — used for pixelated/crisp-edges.
    bind_group_nearest: wgpu::BindGroup,
    /// Texture view (needed for mask-composite bind group creation in render loop).
    view: wgpu::TextureView,
    // texture держим как поле — wgpu освобождает GPU-память когда дропается
    // последняя ссылка; bind_group её не держит.
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

/// Скролл-композитор страницы (EXPERIMENT.md §2, срез 1): персистентная
/// текстура «полосы» документа — вьюпорт плюс запас сверху и снизу,
/// растеризованная в документных координатах (scroll-инвариантно). Пока
/// вьюпорт остаётся внутри полосы и содержимое не меняется, кадр скролла =
/// один blit этой текстуры со сдвигом + overlay, без перерисовки страницы.
struct PageBandCache {
    /// Держит GPU-память полосы (wgpu освобождает её при дропе последней
    /// ссылки; view её не удерживает — как `GpuImage::_texture`).
    _texture: wgpu::Texture,
    /// View полосы — источник blit-а и цель Band-рендера.
    view: wgpu::TextureView,
    /// Scroll-инвариантный ключ содержимого: хэш content-полосы display
    /// list-а при scroll (0,0) + `content_generation` + геометрия полосы.
    /// Урок EXPERIMENT.md п.15: скролл в ключе = промах каждый кадр.
    key: u64,
    /// Y верхнего края полосы в документных CSS px (≥ 0).
    band_top_css: f32,
    /// База кольцевой адресации: документный Y (CSS px), лежащий в строке 0
    /// текстуры. Совпадает с `band_top_css` сразу после ПОЛНОЙ перерисовки и
    /// расходится с ним по мере инкрементальных сдвигов: строка текстуры
    /// `(y − ring_base_css)·dpr mod h_px` держит документную строку `y`
    /// (BUG-405 срез 32, пункт 58 остатка).
    ring_base_css: f32,
    /// Ширина текстуры полосы в device px (= ширине surface).
    w_px: u32,
    /// Высота текстуры полосы в device px (surface + 2×запас).
    h_px: u32,
    /// Bind group блита полосы (`image_bgl`: view полосы + linear sampler).
    /// Оба входа живут ровно столько же, сколько сама полоса, поэтому
    /// группа создаётся вместе с ней, а не на каждый Compose-кадр
    /// (BUG-405 срез 21: 40 дескрипторных наборов за прогон прокрутки).
    blit_bg: wgpu::BindGroup,
    /// Depth-текстура Band-рендера (обязана совпадать размером с полосой).
    /// Кэшируется вместе с полосой: раньше создавалась заново на каждый
    /// miss (7+ МБ Depth32 на band-размере — чистый churn VRAM).
    depth_t: wgpu::Texture,
    /// View depth-текстуры полосы.
    depth_v: wgpu::TextureView,
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
struct OverlayCache {
    /// Держит GPU-память текстуры (см. `PageBandCache::_texture`). Отдельный
    /// `view` не хранится — эта текстура никогда не перерисовывается
    /// повторно (устарела → строится СОВСЕМ новая, вместе с новым view),
    /// поэтому view нужен только на момент постройки — он живёт внутри
    /// `blit_bg` (bind group держит свою ссылку на него).
    _texture: wgpu::Texture,
    /// Bind group блита (`image_bgl`: view + linear sampler), переиспользует
    /// [`Renderer::create_band_blit_bind_group`] — оба входа те же, что у
    /// блита полосы.
    blit_bg: wgpu::BindGroup,
    /// Ширина/высота текстуры в device px (= размеру поверхности — overlay
    /// viewport-locked, полосы у него нет).
    w_px: u32,
    h_px: u32,
    /// Digest хвоста (`overlay[prefix_len..]`, `hash_one_command` на
    /// элемент) НА МОМЕНТ постройки — сравнивается с
    /// `current_overlay_digests[prefix_len..]` БЕЗ сдвига индексов.
    tail_digests: Vec<u64>,
    /// Длина живого префикса. Текстура содержит РОВНО `overlay[prefix_len..]`
    /// в исходном относительном порядке.
    prefix_len: usize,
}

/// Одноразовая инъекция blit-квада полосы в начало draw-плана level 0
/// следующего `render_impl`-вызова (Compose-путь скролл-композитора).
struct PendingBaseBlit {
    /// Bind group `image_bgl` поверх текстуры полосы (linear sampler).
    bind_group: wgpu::BindGroup,
    /// Квады блита: прямоугольник в CSS px кадра плюс uv-угол текстуры полосы.
    /// Штатно один (uv `[0,0]`…`[1,1]`, вся полоса со сдвигом
    /// `band_top_css − scroll_y`); при ненулевой фазе кольца (срез 32) — два,
    /// по обе стороны шва, чтобы не заводить sampler с `Repeat`.
    quads: Vec<(Rect, [f32; 2], [f32; 2])>,
}

/// Всё, что кадр обязан знать о полосе ДО хэширования списка (BUG-405 срез 35).
///
/// Результат `Renderer::prepare_page_compose`: путь компоновки применим, вот
/// его геометрия и план static/animated split-а. Ключ полосы считается по
/// `sw`/`band_h_px` и `ranges`, поэтому подготовка обязана быть раньше хэша.
struct ComposePrep {
    /// Ширина полосы (= ширина поверхности), device px.
    sw: u32,
    /// Масштаб поверхности (device px на CSS px).
    dpr: f32,
    /// Запас полосы за пределами вьюпорта, CSS px (половина полного запаса).
    margin_css: f32,
    /// Высота полосы, device px.
    band_h_px: u32,
    /// Высота полосы, CSS px.
    band_h_css: f32,
    /// Высота вьюпорта, CSS px.
    vp_h_css: f32,
    /// Effective-диапазоны анимируемых сегментов (пусто = split не применён).
    ranges: Vec<std::ops::Range<usize>>,
    /// План реплея сегментов поверх блита (`None` = сегментов нет).
    seg_plan: Option<crate::display_list::DisplayList>,
}

/// Секундомер подстатей кадра компоновки (`compose-top`, BUG-405 срез 34).
///
/// Метки берутся только под `LUMEN_FRAME_LOG=2` — как и весь пофазный лог;
/// без него `mark` вырождается в проверку `Option`. Живёт в кадре, а не в
/// одной функции: срез 35 разнёс подготовку, хэш и саму компоновку по трём
/// вызовам, а печатается разбивка по-прежнему одной строкой.
struct ComposeMarks {
    /// Начало отсчёта; `None` — лог выключен, метки не берутся.
    t0: Option<std::time::Instant>,
    /// Накопленные отсечки от `t0`, мс: skip / geom / split / hash / band.
    ms: [f64; 5],
}

impl ComposeMarks {
    /// Заводит секундомер, если пофазный лог включён.
    ///
    /// BUG-405 срез 37: порог опущен со 2 до 1. Метки нужны не только своим
    /// печатным строкам (они остались на уровне 2), но и счётчикам
    /// [`FRAME_PHASE_NANOS`], по которым кадр раскладывается на УРОВНЕ 1 — там,
    /// где надбавки пункта 71 нет.
    fn new() -> Self {
        Self {
            t0: crate::frame_log_enabled().then(std::time::Instant::now),
            ms: [0.0; 5],
        }
    }

    /// Взяты ли метки (уровень ≥ 1). Печать своих строк требует уровня 2 —
    /// см. [`ComposeMarks::printing`].
    fn enabled(&self) -> bool {
        self.t0.is_some()
    }

    /// Печатать ли пофазные строки компоновки (уровень ≥ 2).
    fn printing(&self) -> bool {
        self.t0.is_some() && crate::frame_log_level() >= 2
    }

    /// Отсечка `i`-й подстатьи.
    fn mark(&mut self, i: usize) {
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
struct BandStrip {
    /// Первая перерисовываемая строка текстуры полосы, device px.
    row0: u32,
    /// Сколько строк перерисовывается, device px (> 0).
    rows: u32,
}

/// Финальная цель одного `render_impl`-вызова.
enum RenderPassMode {
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
    fn store(self) {
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
struct GpuLayerSnapshot {
    // texture держим даже без явного обращения — wgpu освобождает GPU-память
    // когда дропается последняя ссылка; bind_group её не держит.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
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
struct CachedGlyph {
    entry: GlyphEntry,
    left: f32,
    top: f32,
    advance_native: u16,
}

/// Один загруженный face: TTF-байты + owned-кэш метрик, построенный один
/// раз при загрузке (образец — cosmic-text `FontSystem`: fontdb парсит
/// метаданные однажды, дальше живут кэши).
/// face_id 0 — default (bundled, передан в `Renderer::new`); остальные
/// `face_id` назначаются по мере lazy-загрузки из путей `FaceRecord`.
struct LoadedFace {
    /// Байты sfnt-шрифта. `Arc<[u8]>` (BUG-272 срез 6): для @font-face-фейсов
    /// это та же аллокация, что лежит в `FontRegistry::bytes_store` — вместо
    /// двух копий одного шрифта (в реестре и здесь) обе стороны разделяют один
    /// буфер через `read_face_bytes` → клон Arc.
    bytes: Arc<[u8]>,
    /// Метрики для горячего текстового пути (cmap-каскад, advance, baseline).
    /// `None` — face не распарсился при загрузке; такие face пропускаются
    /// в каскаде (эквивалент прежнего `Option<ParsedFace>` = None).
    metrics: Option<FaceMetrics>,
}

/// Owned-метрики face-а, независимые от лайфтайма `bytes`. Живут в
/// `LoadedFace` весь срок жизни рендера — снимают необходимость звать
/// `Font::parse` всех face-ов каждый кадр (тёплый кадр экономит 1.4–2.7 мс,
/// холодный — до 200 мс на 1000000-final.html).
struct FaceMetrics {
    /// `head.units_per_em` — масштаб font units → px.
    units_per_em: u16,
    /// `hhea.ascent` — для baseline (ascent ratio).
    ascent: i16,
    /// `hhea.descent` — для baseline (ascent ratio).
    descent: i16,
    /// Owned-копия cmap subtable: codepoint → glyph id без парсинга шрифта.
    cmap: OwnedCmap,
    /// hmtx advance per glyph id (хвост longHorMetric расширен по спеке).
    /// Индекс = glyph id; длина = num_glyphs.
    advances: Box<[u16]>,
    /// `COLR`+`CPAL` цветного шрифта, разобранные один раз при загрузке.
    /// `None` — монохромный face (нет одной из таблиц, или в `COLR` нет ни
    /// одной v0-записи); тогда текстовый путь не меняется вовсе.
    color: Option<ColorTables>,
}

/// Цветные таблицы face-а: layered-глифы (`COLR` v0) + палитры (`CPAL`).
/// Хранятся вместе, потому что по отдельности бесполезны: `palette_index`
/// слоя адресует запись палитры.
#[derive(Debug)]
struct ColorTables {
    /// Слои цветных глифов — `layers_for(glyph_id)` даёт список
    /// (glyph, palette entry).
    colr: Colr,
    /// Палитры, среди которых выбирает CSS `font-palette`.
    cpal: Cpal,
}

/// Строит [`FaceMetrics`] по байтам шрифта. Возвращает `None`, если любая
/// из обязательных таблиц не парсится (head/hhea/cmap/hmtx/maxp).
fn build_face_metrics(bytes: &[u8]) -> Option<FaceMetrics> {
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
fn resolve_palette(
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
fn layer_color(palette: Option<&[[f32; 4]]>, palette_index: u16, text: [f32; 4]) -> [f32; 4] {
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
struct ParsedFace<'a> {
    font: Font<'a>,
    head: Head,
    hmtx: Hmtx<'a>,
}

/// Ленивый per-frame кэш [`ParsedFace`]-ов: face парсится при первом
/// обращении внутри одного `render()`-вызова (промах атласа / variation
/// axes), повторные обращения бесплатны. На тёплом кадре не создаётся
/// ни одного `ParsedFace`.
struct LazyParsedFaces<'a> {
    faces: &'a [LoadedFace],
    /// Внешний `Option` — «ещё не пробовали», внутренний — результат парсинга.
    parsed: Vec<Option<Option<ParsedFace<'a>>>>,
}

impl<'a> LazyParsedFaces<'a> {
    fn new(faces: &'a [LoadedFace]) -> Self {
        Self { faces, parsed: Vec::new() }
    }

    /// Парсит face `id` при первом обращении; дальше отдаёт кэш.
    fn get(&mut self, id: usize) -> Option<&ParsedFace<'a>> {
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
struct HotPipelines {
    /// Сплошная заливка прямоугольника.
    fill: wgpu::RenderPipeline,
    /// Скруглённый прямоугольник (SDF).
    rrect: wgpu::RenderPipeline,
    /// Квады глифов из атласа.
    text: wgpu::RenderPipeline,
    /// Текстурный квад картинки.
    image: wgpu::RenderPipeline,
    /// Градиентная заливка.
    gradient: wgpu::RenderPipeline,
    /// Какие РАЗНЫЕ потоки скомпилировали эти пять пайплайнов — гейт
    /// среза 2 BUG-406. Ставить его на wall-clock нельзя: разброс старта на
    /// этой машине доходит до 2.5× между прогонами (`docs/perf-method.md`),
    /// а «компиляции разъехались по потокам» проверяется точно и не зависит
    /// ни от железа, ни от загрузки машины. 5 — параллельный путь, 1 —
    /// `LUMEN_SERIAL_PIPELINES=1`.
    threads: HashSet<std::thread::ThreadId>,
}

/// `LUMEN_SERIAL_PIPELINES=1` — собирать горячие пайплайны по очереди на
/// вызывающем потоке (поведение до среза 2 BUG-406). Нужен для A/B в одном
/// бинарнике и как откат, если параллельная сборка где-то мешает драйверу.
fn hot_pipelines_serial() -> bool {
    std::env::var("LUMEN_SERIAL_PIPELINES").is_ok_and(|v| v == "1" || v == "true")
}

/// `LUMEN_WAIT_HOT_PIPELINES=1` — дождаться горячих пайплайнов прямо в
/// `init_pipelines` (поведение среза 2 BUG-406: параллельно, но конструктор
/// блокируется). Нужен для A/B среза 3 в одном бинарнике и как откат.
fn hot_pipelines_awaited_in_ctor() -> bool {
    std::env::var("LUMEN_WAIT_HOT_PIPELINES").is_ok_and(|v| v == "1" || v == "true")
}

/// Какой из пяти горячих пайплайнов имеется в виду (BUG-406 срез 3). Нужен
/// потому, что по `wgpu::RenderPipeline` отличить их друг от друга нельзя, а
/// приезжают они с фоновых потоков в произвольном порядке.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HotKind {
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
const HOT_KINDS: [HotKind; 5] =
    [HotKind::Fill, HotKind::RRect, HotKind::Text, HotKind::Image, HotKind::Gradient];

/// Готовый горячий пайплайн вместе с видом и потоком-сборщиком.
type HotDelivery = (HotKind, std::thread::ThreadId, wgpu::RenderPipeline);

/// Входы сборки горячих пайплайнов — ровно те хэндлы, которых достаточно, и
/// ничего сверх. Все поля `Clone + Send` (в wgpu 26 хэндлы внутри `Arc`),
/// поэтому снимок уезжает на фоновый поток так же, как [`PipelineDeps`] у
/// ленивых.
#[derive(Clone)]
struct HotDeps {
    /// Устройство, на котором компилируются пайплайны.
    device: wgpu::Device,
    /// Формат цветового attachment-а.
    format: wgpu::TextureFormat,
    /// Layout viewport-униформы (bind group 0) — нужен всем пяти.
    uniform_bgl: wgpu::BindGroupLayout,
    /// Layout атласа глифов — нужен `text`.
    atlas_bgl: wgpu::BindGroupLayout,
    /// Layout сэмплируемой картинки — нужен `image`.
    image_bgl: wgpu::BindGroupLayout,
    /// Layout буфера стопов градиента — нужен `gradient`.
    gradient_bgl: wgpu::BindGroupLayout,
}

impl HotDeps {
    /// Компилирует один горячий пайплайн. Дескрипторы те же, что и до среза 3,
    /// — вид выбирает только, какой из пяти билдеров позвать.
    fn build(&self, kind: HotKind) -> wgpu::RenderPipeline {
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
fn spawn_hot_pipelines(deps: &HotDeps) -> std::sync::mpsc::Receiver<HotDelivery> {
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
fn build_hot_pipelines(
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
struct PipelineDeps {
    /// Устройство, на котором компилируются пайплайны.
    device: wgpu::Device,
    /// Формат цветового attachment'а всех пайплайнов кадра.
    surface_format: wgpu::TextureFormat,
    /// Layout viewport-униформы (bind group 0).
    uniform_bgl: wgpu::BindGroupLayout,
    /// Layout сэмплируемой картинки (текстура + сэмплер).
    image_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса (склейка offscreen-уровня с родителем).
    composite_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса маски (`mask-image`).
    mask_composite_bgl: wgpu::BindGroupLayout,
    /// Layout пасса CSS-фильтров.
    filter_bgl: wgpu::BindGroupLayout,
    /// Layout пасса блюра (H/V разделяемое ядро).
    blur_bgl: wgpu::BindGroupLayout,
    /// Layout пасса «вертикальный блюр + фильтры + композит» (BUG-405 срез 6).
    blur_composite_bgl: wgpu::BindGroupLayout,
    /// Layout пасса blend-режимов (`mix-blend-mode`).
    blend_bgl: wgpu::BindGroupLayout,
    /// Layout пасса cross-fade (`image-set`/переходы картинок).
    cross_fade_bgl: wgpu::BindGroupLayout,
    /// Layout composite-пасса клипа произвольной формы (`clip-path`).
    path_clip_bgl: wgpu::BindGroupLayout,
    /// Сколько ленивых пайплайнов этого рендера уже скомпилировано (BUG-405).
    /// Считается **на рендер**, а не на процесс: гейт правки — тест, а тесты
    /// одного бинарника идут параллельно и на общем счётчике мешали бы друг
    /// другу. `Arc` разделяется с клоном снимка, уехавшим на поток прогрева,
    /// поэтому фоновые компиляции тоже видны владельцу.
    built: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Готовый пайплайн, приехавший с потока прогрева (BUG-405). Вариант несёт
/// ровно ту `OnceCell`, в которую его нужно положить, — иначе принимающая
/// сторона не отличила бы один `RenderPipeline` от другого.
enum WarmedPipeline {
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

pub struct Renderer {
    /// Windowed surface; `None` in headless mode (created with `new_headless()`).
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// BUG-453: `Some(reason)` после коллбэка `Device::set_device_lost_callback`
    /// (регистрируется один раз в `init_pipelines`) — после этого рендер
    /// перестаёт трогать `device`/`queue`/`surface`: на потерянном
    /// устройстве любой вызов, включая `SurfaceTexture::present()`, падает
    /// панику библиотеки без пути восстановления изнутри `render_impl`.
    /// `OnceLock`, а не `AtomicBool`, чтобы донести настоящую причину до
    /// `WgpuBackend::render` — `wgpu::SurfaceError` (тип, который может
    /// вернуть `render_impl`) не умеет нести произвольную строку.
    device_lost: Arc<std::sync::OnceLock<String>>,
    /// Surface configuration; `None` in headless mode.
    config: Option<wgpu::SurfaceConfiguration>,
    /// Width in physical pixels when headless (`surface = None`); 0 otherwise.
    headless_w: u32,
    /// Height in physical pixels when headless (`surface = None`); 0 otherwise.
    headless_h: u32,
    /// Device-pixel-ratio от winit (`Window::scale_factor`). Surface
    /// сконфигурирован в physical pixels (`config.width/height`), но shader
    /// делит позицию вершины на logical viewport (`config / scale_factor`),
    /// чтобы 1 CSS pixel = `scale_factor` device pixels — корректное
    /// масштабирование на HiDPI без правки display list-а.
    /// Обновляется через [`Renderer::set_scale_factor`] при `ScaleFactorChanged`
    /// событии winit (например, drag окна между мониторами с разной DPI).
    scale_factor: f64,
    /// Target color space for wide-gamut output (ph3-color-management Step 4).
    /// Determines the chosen swap-chain format:
    /// `DisplayP3`/`Rec2020` → `Rgba16Float` (or first non-sRGB fallback);
    /// `Srgb` → non-sRGB preferred (existing behaviour).
    target_color_space: ColorSpace,

    /// PILI-CANVAS-BG: sRGB background color (root element's `background-color`)
    /// at the time the current frame started rendering. `None` means use white
    /// (CSS UA default). Used for the LoadOp clear colour at frame start.
    /// Converted from sRGB to `target_color_space` before being passed to the
    /// GPU clear colour (ph3-color-management Step 5).
    canvas_bg: Option<Color>,

    /// GPU depth buffer for CSS 3D transforms (`transform-style: preserve-3d`).
    /// Size matches the frame surface; recreated on every `resize()`.
    /// `None` only when both dimensions are zero at construction time.
    // CSS: transform-style — when P4 wires preserve-3d, depth_sorted_child_order()
    // in display_list.rs emits commands back-to-front; the GPU depth test here
    // provides correct occlusion for the rare case of intersecting 3D planes.
    depth_texture: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,

    /// Снимок хэндлов, из которых собирается любой ленивый пайплайн.
    /// Отдаётся клоном фоновому потоку прогрева (BUG-405).
    pdeps: PipelineDeps,
    /// Сколько командных списков **кадра** этот рендер отправил в очередь за
    /// свою жизнь (BUG-405 срез 2). Служебные подачи (mip-генерация при
    /// загрузке картинки, обратное чтение в headless) не считаются: гейт
    /// подачи порциями — про кадр.
    submissions: u64,
    /// Сколько скруглённых клипов этот рендер обслужил offscreen-уровнем
    /// (три пасса на клип) за свою жизнь — BUG-405 срез 4. Шейдерный контур
    /// счётчик не двигает, поэтому «правка работает» = «счётчик не растёт».
    rrect_clip_levels: u64,
    /// Сколько разрезов пасса родителя этот рендер склеил обратно после
    /// выброса невидимого offscreen-уровня (BUG-405 срез 5). Растёт ровно на
    /// те уровни, которые `viewport-cull` убрал из плана, а прежний код
    /// оставлял за собой лишний пасс родителя.
    cull_merges: u64,
    /// Сколько пассов (элементов плана) этот рендер закодировал за свою жизнь
    /// — BUG-405 срез 5. Эффект склейки виден именно здесь: механизм считает
    /// `cull_merges`, а «пассов стало меньше» — этот счётчик.
    plan_passes: u64,
    /// Сколько render-пассов закодировали filter-элементы плана (BUG-405
    /// срез 6): blur даёт два (H + слитый V-композит) вместо трёх, фильтр
    /// без blur - один. Гейт правки стоит на нём, а не на времени кадра.
    filter_passes: u64,
    /// Склейка вертикального прохода блюра с композитом включена (срез 6).
    /// Инстансный выключатель нужен гейту пикселей: оба плеча снимаются в
    /// одном процессе. Поверх него - рычаг процесса `LUMEN_NO_BLUR_MERGE=1`.
    blur_merge_enabled: bool,
    /// Склейка пасса родителя вокруг выброшенного уровня включена
    /// (BUG-405 срез 5). Инстансное плечо A/B: тест рисует один и тот же
    /// список обоими путями в одном процессе и сверяет пиксели побайтово, не
    /// требуя второго прогона с `LUMEN_NO_CULL_MERGE=1`.
    cull_merge_enabled: bool,
    /// Сколько внешних теней нарисовано аналитически, без offscreen-уровня
    /// (BUG-405 срез 7). Гейт правки — этот счётчик рядом с `filter_passes`.
    shadow_draws: u64,
    /// Сколько команд состояния пасса (пайплайн / bind-группа / вершинный
    /// буфер / scissor) не отправлено, потому что там уже стояло ровно это
    /// значение — BUG-405 срез 10. Команды пасса стоят в `drop(pass)`, где
    /// `wgpu-core` проигрывает их в командный список, поэтому «правка
    /// работает» = «счётчик растёт», а не «кадр стал быстрее».
    state_elisions: u64,
    /// Сколько вызовов `draw` слито с предыдущим, потому что состояние пасса
    /// между ними не менялось, а диапазоны вершин оказались соседними
    /// (BUG-405 срез 10). Второй счётчик того же среза: команд состояния
    /// стало меньше — этот, самих draw'ов меньше — тот.
    draw_merges: u64,
    /// Отсев повторных команд состояния включён (BUG-405 срез 10). Инстансное
    /// плечо A/B: тест рисует один и тот же список обоими путями в одном
    /// процессе и сверяет пиксели, не требуя второго прогона с
    /// `LUMEN_NO_STATE_ELISION=1`.
    state_elision_enabled: bool,
    /// Аналитическая размытая тень включена (BUG-405 срез 7). Инстансное
    /// плечо A/B: тест рисует один и тот же список обоими путями в одном
    /// процессе и сверяет пиксели.
    shadow_analytic_enabled: bool,
    /// Сколько байт пикселей атласа отправлено в GPU за жизнь рендерера
    /// (BUG-405 срез 11). Гейт правки — этот счётчик: заливка целой текстуры
    /// (1 МиБ) против заливки только изменившихся строк.
    atlas_bytes_uploaded: u64,
    /// Сколько раз атлас заливался в GPU (BUG-405 срез 11). Байты без числа
    /// заливок не отличают «стало реже» от «стало меньше за раз».
    atlas_uploads: u64,
    /// Заливка только изменившихся строк атласа включена (BUG-405 срез 11).
    /// Инстансное плечо A/B: тест гоняет один и тот же список обоими путями в
    /// одном процессе и сверяет пиксели. Поверх — рычаг
    /// `LUMEN_NO_ATLAS_PARTIAL=1`.
    atlas_partial_upload_enabled: bool,
    /// Сколько ВЛОЖЕННЫХ скруглённых клипов обслужено вторым шейдерным
    /// контуром, то есть без offscreen-уровня (BUG-405 срез 8). Гейт правки —
    /// этот счётчик рядом с `rrect_clip_levels`.
    nested_shader_clips: u64,
    /// Второй шейдерный контур включён (BUG-405 срез 8). Инстансное плечо A/B:
    /// тест рисует один и тот же список обоими путями в одном процессе и
    /// сверяет пиксели. Поверх — рычаг `LUMEN_NO_NESTED_SHADER_CLIP=1`.
    nested_shader_clip_enabled: bool,
    /// Мемоизация покрытия SVG-супов (BUG-405 срез 9).
    coverage_cache: CoverageCache,
    /// Кэш покрытия включён (BUG-405 срез 9). Инстансное плечо A/B: тест
    /// рисует один и тот же список обоими путями в одном процессе и сверяет
    /// пиксели. Поверх — рычаг `LUMEN_NO_COVERAGE_CACHE=1`.
    coverage_cache_enabled: bool,
    /// Мемоизация целых фигур SVG (BUG-405 срез 12).
    svg_shape_cache: SvgShapeCache,
    /// Кэш фигур SVG включён (BUG-405 срез 12). Инстансное плечо A/B: тест
    /// рисует один и тот же список обоими путями в одном процессе и сверяет
    /// пиксели. Поверх — рычаг `LUMEN_NO_SVG_SHAPE_CACHE=1`.
    svg_shape_cache_enabled: bool,
    /// Мемоизация укладки целого текстового run-а (BUG-405 срез 13).
    text_run_cache: TextRunCache,
    /// Кэш укладки текста включён (BUG-405 срез 13). Инстансное плечо A/B:
    /// тест рисует один и тот же список обоими путями в одном процессе и
    /// сверяет вершины. Поверх — рычаг `LUMEN_NO_TEXT_RUN_CACHE=1`.
    text_run_cache_enabled: bool,
    /// Приёмник готовых пайплайнов с потока прогрева (BUG-405). `None` —
    /// прогрев ещё не запущен, уже завершён, или отключён
    /// `LUMEN_NO_PIPELINE_WARMUP=1`. Сбрасывается в `None`, когда поток
    /// закрыл отправитель, — дальше `try_recv` был бы холостым.
    warm_rx: Option<std::sync::mpsc::Receiver<WarmedPipeline>>,
    /// Прогрев уже запускался (BUG-405). Отдельно от `warm_rx`, который
    /// обнуляется по завершении потока: без флага прогрев перезапускался бы
    /// каждый кадр после его окончания.
    warm_started: bool,

    /// Сплошная заливка. BUG-406 срез 3: ячейка наполняется фоновым потоком
    /// сборки горячих пайплайнов, читается только через [`Self::fill_pipeline`].
    fill_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`circle_pipeline()`), не при старте окна.
    circle_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS border-radius SDF pipeline. Uses `RRectVertex` layout.
    /// BUG-406 срез 3: см. [`Self::fill_pipeline`].
    rrect_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-405 срез 7 — аналитическая размытая rrect (`box-shadow`), формат
    /// `ShadowVertex`. Ленивая компиляция: страница без теней за неё не платит,
    /// прогрев подхватывает её в общем списке (`build_all_lazy`).
    shadow_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Квады глифов из атласа. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    text_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Текстурный квад картинки. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    image_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Blit-каскад mip-цепочки картинок: пасс «mip N−1 → mip N» при
    /// `register_image` (fullscreen triangle, bilinear = 2×2 box).
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`mipgen_pipeline()`), не при старте окна.
    mipgen_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Images L4 §4 — `cross-fade(A, B, p)` two-texture blend pipeline.
    /// Uses `CrossFadeVertex` layout (pos+uv). Bind group 0 = viewport uniform
    /// (shared with `image_pipeline`); bind group 1 = `cross_fade_bgl`
    /// (tex_a, tex_b, sampler, progress uniform). Blend state: `ALPHA_BLENDING`.
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`cross_fade_pipeline()`), не при старте окна.
    cross_fade_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Bind group layout for the `cross_fade_pipeline` per-quad bindings
    /// (group 1): two textures + sampler + progress uniform.
    cross_fade_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`composite_pipeline()`), не при старте окна.
    composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    composite_bgl: wgpu::BindGroupLayout,
    /// CSS Overflow L3 §2 — composite-пайплайн скруглённого клипа
    /// (`PushClipRoundedRect` → offscreen-уровень, `PopClip` → этот пасс).
    /// Разделяет `composite_bgl` с обычным композитом: та же пара
    /// {текстура уровня, sampler}. BUG-406: компиляция ленивая.
    rrect_clip_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Masking L1 §3 — composite-пайплайн формы `clip-path`
    /// (`PushClipPath` → offscreen-уровень, `PopClip` → этот пасс).
    /// Свой BGL: к паре {текстура уровня, sampler} добавлен uniform формы.
    /// BUG-406: компиляция ленивая.
    path_clip_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BGL пайплайна формы клипа: текстура(0) + sampler(1) + uniform формы(2).
    path_clip_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blend_pipeline()`), не при старте окна.
    blend_pipeline: OnceCell<wgpu::RenderPipeline>,
    blend_bgl: wgpu::BindGroupLayout,
    /// CSS Masking L1 §4 — mask composite pipeline + bind group layout.
    /// Used by PopMask to composite the offscreen layer using a mask image.
    mask_composite_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`mask_composite_pipeline()`), не при старте окна.
    mask_composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Masking L1 §5 — mask-layer composite pipelines.
    /// Used by PopMaskLayer to apply an arbitrary rendered mask to the parent layer.
    /// `_alpha` samples mask.a; `_luma` converts RGB to luminance × alpha.
    /// Shared BGL with mask_composite (same binding layout: t_content, t_mask, s).
    /// BUG-406: ленивая компиляция пары (alpha, luminance) — общий шейдер,
    /// поэтому один `OnceCell` на оба пайплайна.
    mask_layer_pipelines: OnceCell<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    /// CSS Filter Effects L1 — color filter pipeline (grayscale/sepia/brightness/etc.).
    filter_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`filter_pipeline()`), не при старте окна.
    filter_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Filter Effects L1 — separable Gaussian blur pipeline (one pass: H or V).
    blur_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blur_pipeline()`), не при старте окна.
    blur_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// BUG-405 срез 6 — вертикальный проход блюра вместе с цветовыми фильтрами
    /// и композитом в родителя: один пасс вместо двух. Layout = `blur_bgl`
    /// плюс четвёртый слот с `FilterParams`.
    blur_composite_bgl: wgpu::BindGroupLayout,
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`blur_composite_pipeline()`), не при старте окна.
    blur_composite_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// CSS Filter Effects L1 §2 — backdrop-filter blit pipeline.
    /// Same shader as `filter_pipeline` but uses REPLACE blend so the filtered
    /// backdrop snapshot overwrites (not composites over) the parent layer at
    /// the bounded element rect.
    /// BUG-406: ленивая компиляция — `OnceCell` заполняется при первом
    /// использовании (`backdrop_blit_pipeline()`), не при старте окна.
    backdrop_blit_pipeline: OnceCell<wgpu::RenderPipeline>,
    /// Intermediate texture for backdrop-filter: ping-pong target for blur passes
    /// (H: scratch → backdrop_layer; V: backdrop_layer → scratch), and color-filter
    /// target when compositing filtered backdrop back onto parent.
    backdrop_layer: Option<OffscreenLayer>,
    /// CSS Filter Effects L1 §2 — `backdrop-filter` result cache (metadata).
    /// Tracks, per backdrop element ordinal, the content hash of the inputs that
    /// produced the cached filtered texture. Used to skip the blur passes when a
    /// frame's backdrop inputs are unchanged from the previous frame.
    backdrop_cache: crate::backdrop_cache::BackdropCache,
    /// Cached filtered backdrop textures, keyed by the same ordinal as
    /// [`Self::backdrop_cache`]. Each is a full parent-layer-sized snapshot of
    /// the blurred (or, for filter-only backdrops, copied) backdrop region.
    /// Reused across frames on a cache hit; the color-filter pass still runs at
    /// blit time so only the expensive blur is skipped.
    backdrop_cache_textures: HashMap<u32, OffscreenLayer>,
    /// Кэш depth-текстур под bbox-офскрины (регион ≠ размеру окна/полосы):
    /// пасс с маленьким color-attachment обязан иметь depth того же размера
    /// (валидация wgpu). Ключ — (w, h) в device px; размеры регионов
    /// выровнены до 64 px, так что классов мало. Чистится при переполнении
    /// (> 16 записей) — обычная страница держит 1-3 размера.
    small_depth_cache: HashMap<(u32, u32), wgpu::TextureView>,
    /// CSS Images L3 §3.3 — linear/radial gradient pipeline.
    gradient_bgl: wgpu::BindGroupLayout,
    /// Градиентная заливка. BUG-406 срез 3: см. [`Self::fill_pipeline`].
    gradient_pipeline: OnceCell<wgpu::RenderPipeline>,
    scratch_layer: Option<OffscreenLayer>,
    layer_sampler: wgpu::Sampler,
    layer_textures: Vec<OffscreenLayer>,
    surface_format: wgpu::TextureFormat,

    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    /// Сколько слотов [`ClipUniformSlot`] помещается в `uniform_buffer`
    /// (BUG-405 срез 4). Слот 0 — «скруглённого клипа нет».
    uniform_slots: usize,

    atlas_texture: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup,
    /// Сколько раз атлас глифов сбрасывался из-за исчерпания места (BUG-435).
    /// Растёт — атласу 1024×1024 тесно на этом контенте.
    atlas_resets: u64,

    image_bgl: wgpu::BindGroupLayout,
    image_sampler: wgpu::Sampler,
    image_sampler_nearest: wgpu::Sampler,
    /// Sampler блита полосы скролл-композитора: линейный, но `Repeat` по V —
    /// полоса адресуется кольцом (BUG-405 срез 32).
    band_sampler: wgpu::Sampler,
    /// Декодированные изображения в CPU-памяти. Хранятся для on-demand
    /// ресайза под конкретный layout-размер (CPU bilinear resize).
    raw_images: HashMap<String, Image>,
    /// Cache GPU-текстур: ключ `"src"` (оригинал) или `"src@WxH"` (ресайз).
    /// Заполняется через [`Renderer::register_image`] и лениво при DrawImage.
    images: HashMap<String, GpuImage>,
    /// Cache GPU-снимков слоёв per-id. Заполняется compositor-ом через
    /// [`Renderer::upload_layer_snapshot`] для кеширования неизменных слоёв.
    layer_snapshots: HashMap<u64, GpuLayerSnapshot>,
    /// Skip-identical-frame: поколение контента, не входящего в display list
    /// (картинки/GIF-кадры/снапшоты/шрифты/canvas-bg/промо-слои). Бампается
    /// каждой мутирующей операцией; входит в хэш кадра.
    content_generation: u64,
    /// Фиксированное смещение страницы в CSS px (ADR-016 M0.4, BUG-405 срез 38).
    ///
    /// Применяется как самая внешняя трансляция КОНТЕНТА (не overlay-я) —
    /// ровно то, что раньше шелл каждый кадр заворачивал в
    /// `PushTransform(translate(offset))`, копируя ради этого весь display
    /// list. Входит в поколение контента: смена смещения не меняет список, но
    /// меняет пиксели.
    page_offset: (f32, f32),
    /// Хэш последнего успешно отрисованного оконного кадра
    /// (display list + overlay + scroll + размер + `content_generation`).
    /// Совпадение со следующим кадром ⇒ пиксели идентичны ⇒ кадр пропускается.
    last_frame_hash: Option<u64>,
    /// Скролл-композитор страницы (EXPERIMENT.md §2): персистентная полоса
    /// документа. `None` — ещё не рисовалась (или сброшена сменой геометрии).
    page_band: Option<PageBandCache>,
    /// Blit-квад полосы для следующего Compose-рендера. Ставится только
    /// `try_page_compose`, снимается `take()`-ом в начале сбора вершин.
    pending_base_blit: Option<PendingBaseBlit>,
    /// Retained-текстура стабильного хвоста overlay-списка (BUG-405 срез 41).
    /// `None` — ещё не построена, либо прошлый кадр её признал устаревшей.
    overlay_cache: Option<OverlayCache>,
    /// Blit-квад overlay-кэша для следующего Compose-рендера — тот же
    /// одноразовый контракт, что у `pending_base_blit` (ставится
    /// `compose_page`, снимается `take()`-ом сразу после него).
    pending_overlay_blit: Option<PendingBaseBlit>,
    /// Digest-вектор overlay-списка ПРОШЛОГО кадра (`hash_one_command` на
    /// элемент) — не путать с `OverlayCache::tail_digests` (тот держит digest
    /// ТОЛЬКО хвоста на момент постройки КЭША, а не прошлого кадра; нужен
    /// обоим: этот ловит «где кончается изменившийся префикс» ради выбора
    /// точки разреза при пересборке, тот — «кэш всё ещё валиден»).
    last_overlay_digests: Vec<u64>,
    /// Причина последнего отказа скролл-композитора (BUG-405 срез 22) —
    /// печатается при её СМЕНЕ под `LUMEN_FRAME_LOG>=2`. Без неё отказ виден
    /// только как отсутствие строк `page-compose`, и перепись не может
    /// отличить «композитор не применим» от «композитора нет в сборке».
    last_compose_skip: Option<&'static str>,
    /// Scroll-инвариантный ключ контента ПРОШЛОГО кадра. Полоса рисуется
    /// только по стабильному контенту (ключ совпал два кадра подряд):
    /// анимация/GIF/стриминг парсера меняют ключ каждый кадр, и рендер
    /// полосы (1.7× выше вьюпорта) там был бы дороже монолита — замерено
    /// 2026-07-10: 511 промахов из 629 кадров, медиана 10.7 → 21 мс.
    last_content_key: Option<u64>,
    /// Версия content-списка, объявленная shell-ом (BUG-405 срез 39).
    /// `0` — «версия неизвестна», свёртка не мемоизируется. Контракт —
    /// [`RenderBackend::set_content_epoch`](crate::backend::RenderBackend::set_content_epoch).
    content_epoch: u64,
    /// Свёртка content-части обоих кадровых хэшей с прошлого кадра плюс всё,
    /// по чему её законно переиспользовать: версия списка, его адрес, длина и
    /// подпись выколотых диапазонов. Адрес и длина — не замена версии, а
    /// страховка: они ловят подмену списка, о которой shell не сказал, но не
    /// ловят правку на месте (её обязана поймать версия).
    content_fold_memo: Option<ContentFoldMemo>,
    /// GPU layer cache with LRU eviction (ADR-008 Phase 2).
    /// Tracks layer textures by stacking context ID + size for off-viewport eviction.
    layer_cache: crate::layer_cache::LayerCache,

    atlas: GlyphAtlas,
    /// Загруженные face-ы. `faces[0]` — default (bundled), используется когда
    /// `font-family` пуст или ни одно имя не нашлось через `FontProvider`.
    /// Остальные добавляются лениво при первом `DrawText` с известной family.
    faces: Vec<LoadedFace>,
    /// `face_id` bundled Golos Text Regular (DS-4) — default chrome UI font,
    /// used by [`Self::resolve_face_id`] when `font_family` is empty (every
    /// chrome `DrawText` call site) or requests reserved family `"Golos Text"`.
    chrome_face_id: Option<usize>,
    /// `face_id` bundled Golos Text Medium (DS-4) — reserved family `"Golos Text Medium"`.
    chrome_face_medium_id: Option<usize>,
    /// `face_id` bundled JetBrains Mono Regular (DS-4) — reserved family
    /// `"JetBrains Mono"`, used for the omnibox URL field and DevTools panels.
    mono_face_id: Option<usize>,
    /// `face_id` по абсолютному пути TTF — чтобы не грузить файл повторно.
    face_id_by_path: HashMap<PathBuf, usize>,
    /// Мемоизация `resolve_face_id`: хэш `(families, weight, style)` →
    /// `face_id`. Без него каждый `DrawText` каждого кадра гонял
    /// `to_lowercase` + `FontProvider::pick_face` (двe Vec-аллокации +
    /// матчинг). Ключ — u64-хэш (SipHash); коллизия теоретически возможна,
    /// но при десятках ключей пренебрежима (та же логика, что skip-frame
    /// hash). Сбрасывается в `set_font_provider` — новый провайдер
    /// (например, FontRegistry с @font-face) меняет ответы резолва.
    resolve_cache: HashMap<u64, usize>,
    /// Источник лукапа face-ов по `(family, weight, style)`. По умолчанию —
    /// `SystemFontIndex`, который лениво сканирует системные font-директории.
    /// `None` означает «без resolver-а — всегда default face» (для тестов /
    /// headless-режимов).
    font_provider: Option<Arc<dyn FontProvider>>,
    /// Кэш растеризованных глифов: ключ `(face_id, glyph_id, size_bin)`.
    /// `face_id` — глифы у разных face-ов имеют разный glyph_id; `size_bin`
    /// — multi-size atlas (см. `SIZE_BINS`): один и тот же глиф для
    /// font-size 16 и 32 даёт две разные записи (разная растеризация,
    /// разный atlas-rect).
    cached_glyphs: HashMap<AtlasKey, Option<CachedGlyph>>,
    /// In headless mode: the `RENDER_ATTACHMENT | COPY_SRC` texture rendered to
    /// by the most recent `render()` call. Kept alive between `render()` and
    /// `render_to_image()` pixel readback, then dropped.
    pending_readback: Option<wgpu::Texture>,
    /// GPU texture pool for layer recycling (ADR-008 Phase 2).
    /// Maintains free textures keyed by (width, height) for reuse instead of
    /// allocating a new `wgpu::Texture` for each layer. Свободный список
    /// ограничен байтовым бюджетом (BUG-272 срез 21) — вытеснение в `trim()`
    /// после сабмита кадра.
    texture_pool: crate::texture_pool::TexturePool<crate::texture_pool::PooledTexture>,
    /// Normalized GPU fingerprint: prevents WebGL renderer/vendor fingerprinting (ADR-007).
    gpu_fingerprint: GpuFingerprint,
    /// Потоки, собравшие горячие пайплайны этого рендера — см.
    /// [`Renderer::hot_pipeline_threads`]. `RefCell`: при фоновой сборке
    /// (BUG-406 срез 3) множество пополняется по мере приёма пайплайнов, то
    /// есть уже из `&self`-аксессоров кадра.
    hot_pipeline_threads: RefCell<HashSet<std::thread::ThreadId>>,
    /// Приёмник горячих пайплайнов с фоновых потоков сборки (BUG-406 срез 3).
    /// `None` — сборка была синхронной (headless, `LUMEN_SERIAL_PIPELINES=1`,
    /// `LUMEN_WAIT_HOT_PIPELINES=1`) либо канал уже опустел.
    hot_rx: RefCell<Option<std::sync::mpsc::Receiver<HotDelivery>>>,
    /// Входы сборки горячих пайплайнов — нужны, чтобы кадр мог собрать
    /// пайплайн сам, если фоновый поток не стартовал или умер (BUG-406 срез 3).
    hot_deps: HotDeps,
    /// Сколько горячих пайплайнов пришлось скомпилировать САМОМУ UI-потоку —
    /// гейт среза 3 BUG-406, см. [`Renderer::hot_pipelines_built_on_ui_thread`].
    hot_built_on_ui: std::cell::Cell<usize>,
}

/// Creates a `Depth32Float` texture + view sized `width×height` for GPU depth testing.
/// Called once in `init_pipelines` and on every `resize`.
fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
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
fn select_surface_format(
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
fn frame_skip_disabled() -> bool {
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
type TextureCensusMap = HashMap<(&'static str, u32, u32), u64>;
static TEXTURE_CENSUS: std::sync::OnceLock<std::sync::Mutex<TextureCensusMap>> =
    std::sync::OnceLock::new();

/// Как [`count_texture_created`], но при `LUMEN_FRAME_LOG=3` дополнительно
/// пишет `(label, w, h)` в [`TEXTURE_CENSUS`] (печатается в `alloc:`-блоке).
fn count_texture_created_labeled(label: &'static str, width: u32, height: u32) {
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
fn flush_compose_marks(marks: &ComposeMarks) {
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
fn timed_log(f: impl FnOnce()) {
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
const SUBMIT_CHUNK_ITEMS: usize = 8;

/// `true`, если подача кадра порциями отключена (`LUMEN_NO_SPLIT_SUBMIT=1`) —
/// рычаг отката BUG-405 срез 2 к одному командному списку на кадр.
fn split_submit_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SPLIT_SUBMIT").is_ok_and(|v| v == "1"))
}

/// Слот uniform-буфера группы 0 — CPU-зеркало WGSL-структуры `Uniforms`
/// из [`CLIP_UNIFORM_WGSL`] (BUG-405 срез 4). Все поля в DEVICE px:
/// фрагментный этап берёт позицию из `@builtin(position)`.
#[repr(C)]
#[derive(Clone, Copy)]
struct ClipUniformSlot {
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
const SHADER_CLIP_MAX_CONTOURS: usize = 2;

/// Скруглённый контур активного шейдерного клипа в экранных CSS px
/// (BUG-405 срез 8). Стек этих записей и есть то, что слот uniform'а
/// пересекает произведением покрытий.
#[derive(Clone, Copy)]
struct ClipContour {
    /// Центр прямоугольника клипа.
    center: [f32; 2],
    /// Полуразмер прямоугольника клипа.
    half: [f32; 2],
    /// Горизонтальные радиусы углов (tl, tr, br, bl).
    radii_x: [f32; 4],
    /// Вертикальные радиусы углов (tl, tr, br, bl).
    radii_y: [f32; 4],
}

/// Собирает слот uniform'а из стека активных контуров (BUG-405 срез 8).
/// Пустой стек невозможен — слот 0 строит [`no_clip_slot`].
fn clip_slot_from(
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
const UNIFORM_SLOT_STRIDE: u64 = 256;

/// «Клипа нет»: полуразмер 1e7 CSS px делает SDF отрицательным во всей
/// поверхности, покрытие — ровно 1.0.
fn no_clip_slot(viewport: [f32; 2], dpr: f32) -> ClipUniformSlot {
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
fn shader_rrect_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_SHADER_RRECT_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если ВТОРОЙ шейдерный контур отключён
/// (`LUMEN_NO_NESTED_SHADER_CLIP=1`) — рычаг отката BUG-405 срез 8 к
/// offscreen-уровню на каждый вложенный `PushClipRoundedRect` и A/B-плечо для
/// проверки пикселей. Отдельный от `LUMEN_NO_SHADER_RRECT_CLIP` затем, что тот
/// снимает шейдерный клип целиком и потому не отделяет срез 8 от среза 4.
fn nested_shader_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_NESTED_SHADER_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если склейка пасса родителя вокруг выброшенного (невидимого)
/// offscreen-уровня отключена (`LUMEN_NO_CULL_MERGE=1`) — рычаг отката
/// BUG-405 срез 5 к прежнему поведению «уровень выброшен из плана, но разрез
/// пасса родителя остался» и A/B-плечо для проверки пикселей.
fn blur_merge_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_BLUR_MERGE").is_ok_and(|v| v == "1"))
}

/// `true`, если аналитическая размытая тень отключена
/// (`LUMEN_NO_SHADOW_ANALYTIC=1`) — рычаг отката BUG-405 срез 7 к
/// offscreen-уровню с блюром на каждую внешнюю тень и A/B-плечо для сверки
/// пикселей.
fn shadow_analytic_disabled() -> bool {
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
fn box_shadow_body(
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

fn cull_merge_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_CULL_MERGE").is_ok_and(|v| v == "1"))
}

/// Рычаг отката построчной заливки атласа (BUG-405 срез 11).
fn atlas_partial_upload_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_ATLAS_PARTIAL").is_ok_and(|v| v == "1"))
}

/// Слот пре-резолва face-а под команду, которая текстом не является
/// (BUG-771). Вектор пре-резолва адресуется глобальным индексом команды, а не
/// порядковым номером `DrawText`, поэтому нетекстовые слоты обязаны быть
/// заполнены — и заполнены значением, которое нельзя спутать с `face_id`.
const NO_TEXT_FACE: usize = usize::MAX;

/// Пре-резолв primary face_id под каждую команду кадра (BUG-771).
///
/// Длина результата равна `content.len() + overlay.len()`, а слот команды —
/// её собственный глобальный индекс ([`text_face_slot`]); команда, которая
/// текстом не является, получает [`NO_TEXT_FACE`]. Раньше сюда клались
/// только `DrawText`, а читались курсором по мере отрисовки — и первая же
/// команда текста, не дошедшая до своей ветки (viewport-кулинг), сдвигала
/// весь остаток кадра на чужие face-ы.
fn resolve_text_face_ids(
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
const fn text_face_slot(is_overlay: bool, cmd_idx: usize, content_len: usize) -> usize {
    if is_overlay { content_len + cmd_idx } else { cmd_idx }
}

/// Диагностика BUG-771: печатать подпись текста overlay-а и атласа глифов на
/// каждом кадре (`LUMEN_TEXT_SIG=1`; `=2` — ещё и по вершине на квад).
/// Отдельно от `LUMEN_FRAME_LOG`, потому что хэш атласа — это проход по
/// мегабайту на кадр.
fn text_sig_level() -> u8 {
    use std::sync::OnceLock;
    static LEVEL: OnceLock<u8> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        std::env::var("LUMEN_TEXT_SIG").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    })
}

/// Рычаг отката отсева повторных команд состояния пасса (BUG-405 срез 10).
fn state_elision_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_STATE_ELISION").is_ok_and(|v| v == "1"))
}

fn scroll_compositor_disabled() -> bool {
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
fn anim_split_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_ANIM_SPLIT").is_ok_and(|v| v == "1")
    })
}

/// `true`, если bbox-scissor фильтр-пассов отключён (`LUMEN_NO_BBOX_SCISSOR=1`).
/// Диагностика: A/B-сравнение картинки и скорости на одном бинарнике.
fn bbox_scissor_disabled() -> bool {
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
fn bbox_backdrop_disabled() -> bool {
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
fn bbox_filter_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_BBOX_FILTER").is_ok_and(|v| v == "1"))
}

/// `true`, если сглаживание SVG-супа отключено (`LUMEN_NO_SVG_AA=1`):
/// `DrawSvgFill`/`DrawSvgStroke` рисуются бинарным треугольным супом, как до
/// BUG-277 среза 12. Диагностика: A/B-сравнение картинки и скорости на одном
/// бинарнике (растеризация покрытия идёт на CPU и стоит O(площадь фигуры)).
fn svg_aa_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_SVG_AA").is_ok_and(|v| v == "1"))
}

/// `true`, если сглаживание кромки повёрнутого/скошенного `FillRect` отключено
/// (`LUMEN_NO_ROT_AA=1`): квад рисуется бинарно, как до BUG-277 среза 13.
/// Диагностика: A/B-сравнение картинки и скорости на одном бинарнике
/// (растеризация покрытия идёт на CPU и стоит O(площадь bbox квада)).
fn rot_aa_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_ROT_AA").is_ok_and(|v| v == "1"))
}

/// `true`, если точный клип повёрнутого/скошенного прямоугольника отключён
/// (`LUMEN_NO_ROT_CLIP=1`): `PushClipRect` под поворотом снова режет по AABB
/// трансформированного прямоугольника, как до BUG-277 среза 14. Диагностика:
/// A/B-сравнение картинки и скорости на одном бинарнике (точный клип открывает
/// offscreen-уровень и стоит один composite-пасс на каждый такой клип).
fn rot_clip_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_ROT_CLIP").is_ok_and(|v| v == "1"))
}

/// `true`, если применение накопленного `PushTransform` к квадам
/// `DrawBackgroundImage`/`DrawCrossFade` отключено (`LUMEN_NO_IMG_XFORM=1`):
/// квады снова кладутся в нетрансформированных координатах, как до BUG-277
/// среза 15. Диагностика: A/B-сравнение картинки на одном бинарнике.
fn img_xform_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_IMG_XFORM").is_ok_and(|v| v == "1"))
}

/// `true`, если mip-цепочка картинок отключена (`LUMEN_NO_IMAGE_MIPS=1`):
/// возврат к CPU-ресайзу под каждый placed-размер (`src@WxH`-зоопарк) и
/// nearest-выбору mip-уровня в сэмплере. Диагностика: A/B-сравнение картинки,
/// скорости и памяти на одном бинарнике.
fn image_mips_disabled() -> bool {
    use std::sync::OnceLock;
    static DISABLED: OnceLock<bool> = OnceLock::new();
    *DISABLED.get_or_init(|| {
        std::env::var("LUMEN_NO_IMAGE_MIPS").is_ok_and(|v| v == "1")
    })
}

/// `true`, если фоновый прогрев ленивых пайплайнов отключён
/// (`LUMEN_NO_PIPELINE_WARMUP=1`): каждый пайплайн снова компилируется на том
/// кадре, где впервые понадобился (поведение до BUG-405). A/B-рычаг и откат.
fn pipeline_warmup_disabled() -> bool {
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
fn band_bias_disabled() -> bool {
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
fn band_warm_disabled() -> bool {
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
fn band_margin_override_css() -> Option<f32> {
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
fn band_copy_usage_enabled() -> bool {
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
fn band_draw_fraction() -> Option<f32> {
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
fn band_cull_height(surface_h: u32, frac: f32) -> u32 {
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
fn band_pass_load_ops() -> (bool, bool) {
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
fn band_pass_load_choice(v: &str) -> (bool, bool) {
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
fn dual_hash_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_DUAL_HASH").is_ok_and(|v| v != "0"))
}

/// `LUMEN_NO_DL_EPOCH=1` — не переиспользовать свёртку content-части, считать
/// оба кадровых хэша обходом всего списка, как до среза 39 (BUG-405).
///
/// Плечо A/B и рычаг отката: работа плеч различается ровно на обход списка,
/// меряется той же меткой `frame-hash`.
fn dl_epoch_disabled() -> bool {
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
fn dl_epoch_verify() -> bool {
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
struct ContentFoldMemo {
    /// Версия списка, объявленная shell-ом на кадре, где свёртка снята.
    epoch: u64,
    /// Адрес начала среза `content` — страховка от подмены списка, о которой
    /// shell не сказал. Не разыменовывается: сравнивается как число.
    ptr: usize,
    /// Длина среза `content` — вторая половина той же страховки.
    len: usize,
    /// Свёртка выколотых диапазонов: у ключа полосы они входят в результат,
    /// поэтому смена набора сегментов обязана инвалидировать свёртку.
    skip_sig: u64,
    /// Сама свёртка: `.0` — для хэша кадра, `.1` — для ключа полосы.
    folds: (u64, u64),
}

/// Подпись набора выколотых диапазонов для [`ContentFoldMemo::skip_sig`].
///
/// O(числа диапазонов), а не O(длины списка): на странице их единицы, поэтому
/// подпись считается каждый кадр и мемоизации не требует.
fn skip_signature(skip: &[std::ops::Range<usize>]) -> u64 {
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
fn compose_overlay_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_COMPOSE_OVERLAY").is_ok_and(|v| v != "0"))
}

/// Плечо A/B среза 41: выключает retained overlay-кэш (`Renderer::overlay_cache_step`),
/// оставляя штатную полную перерисовку overlay каждый Compose-кадр — как до
/// среза. Не то же самое, что [`compose_overlay_disabled`]: тот убирает
/// overlay из кадра целиком (диагностика цены), этот меняет ТОЛЬКО механизм
/// отрисовки — пиксели обязаны совпасть побитово (гейт слайса).
fn overlay_cache_disabled() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var("LUMEN_NO_OVERLAY_CACHE").is_ok_and(|v| v != "0"))
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
fn balanced_cut_at_or_after(overlay: &[DisplayCommand], from: usize) -> usize {
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
    static OVERLAY_PREV: std::cell::RefCell<Vec<u64>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn band_ring_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("LUMEN_BAND_RING").is_ok_and(|v| v != "0"))
}

/// Одна кромка кольцевой полосы: непрерывный диапазон строк ТЕКСТУРЫ и
/// документная строка, попадающая в первую из них. Всё в device px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RingStrip {
    /// Первая перерисовываемая строка текстуры полосы.
    row0: u32,
    /// Сколько строк перерисовывается (> 0).
    rows: u32,
    /// Документный Y строки `row0`.
    doc_y0: i64,
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
fn ring_advance_plan(
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
fn band_blit_quads(
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
fn band_geometry(
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

/// `true`, если подъём `max_texture_dimension_2d` до тира адаптера отключён
/// (`LUMEN_NO_TEXTURE_LIMIT_RAISE=1`): устройство снова запрашивается ровно с
/// `downlevel_defaults()`, как до среза 23 BUG-405. Нужен для интерливед-A/B
/// на одном бинарнике (`docs/perf-method.md`).
fn texture_limit_raise_disabled() -> bool {
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
fn requested_max_texture_dim(adapter_max: u32, raise: bool) -> u32 {
    let base = wgpu::Limits::downlevel_defaults().max_texture_dimension_2d;
    if !raise {
        return base;
    }
    adapter_max.min(MAX_TEXTURE_DIM_TARGET).max(base)
}

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
    fn spawn_pipeline_warmup(&mut self) {
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
    fn drain_warmed_pipelines(&mut self) {
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
    fn await_all_hot_pipelines(&self) {
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
    fn fill_pipeline(&self) -> &wgpu::RenderPipeline {
        self.fill_pipeline.get_or_init(|| self.await_hot(HotKind::Fill))
    }

    /// Скруглённый прямоугольник (SDF). BUG-406 срез 3, см.
    /// [`Self::fill_pipeline`].
    fn rrect_pipeline(&self) -> &wgpu::RenderPipeline {
        self.rrect_pipeline.get_or_init(|| self.await_hot(HotKind::RRect))
    }

    /// Квады глифов из атласа. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    fn text_pipeline(&self) -> &wgpu::RenderPipeline {
        self.text_pipeline.get_or_init(|| self.await_hot(HotKind::Text))
    }

    /// Текстурный квад картинки. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    fn image_pipeline(&self) -> &wgpu::RenderPipeline {
        self.image_pipeline.get_or_init(|| self.await_hot(HotKind::Image))
    }

    /// Градиентная заливка. BUG-406 срез 3, см. [`Self::fill_pipeline`].
    fn gradient_pipeline(&self) -> &wgpu::RenderPipeline {
        self.gradient_pipeline.get_or_init(|| self.await_hot(HotKind::Gradient))
    }

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
    fn build_all_lazy(&self, mut emit: impl FnMut(WarmedPipeline) -> bool) {
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
    fn circle_pipeline(&self) -> &wgpu::RenderPipeline {
        self.circle_pipeline.get_or_init(|| self.pdeps.build_circle_pipeline())
    }
    /// Ленивый доступ к `mipgen`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn mipgen_pipeline(&self) -> &wgpu::RenderPipeline {
        self.mipgen_pipeline.get_or_init(|| self.pdeps.build_mipgen_pipeline())
    }
    /// Ленивый доступ к `cross_fade`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn cross_fade_pipeline(&self) -> &wgpu::RenderPipeline {
        self.cross_fade_pipeline.get_or_init(|| self.pdeps.build_cross_fade_pipeline())
    }
    /// Ленивый доступ к `composite`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.composite_pipeline.get_or_init(|| self.pdeps.build_composite_pipeline())
    }
    /// Ленивый доступ к пайплайну скруглённого клипа (BUG-406, BUG-277 срез 5).
    fn rrect_clip_pipeline(&self) -> &wgpu::RenderPipeline {
        self.rrect_clip_pipeline.get_or_init(|| self.pdeps.build_rrect_clip_pipeline())
    }

    /// Ленивый доступ к пайплайну композита формы `clip-path` (BUG-406).
    fn path_clip_pipeline(&self) -> &wgpu::RenderPipeline {
        self.path_clip_pipeline.get_or_init(|| self.pdeps.build_path_clip_pipeline())
    }
    /// Ленивый доступ к `blend`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn blend_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blend_pipeline.get_or_init(|| self.pdeps.build_blend_pipeline())
    }
    /// Ленивый доступ к `mask_composite`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn mask_composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.mask_composite_pipeline.get_or_init(|| self.pdeps.build_mask_composite_pipeline())
    }
    /// Ленивый доступ к `filter`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn filter_pipeline(&self) -> &wgpu::RenderPipeline {
        self.filter_pipeline.get_or_init(|| self.pdeps.build_filter_pipeline())
    }
    /// Ленивый доступ к `blur`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn blur_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blur_pipeline.get_or_init(|| self.pdeps.build_blur_pipeline())
    }
    /// Ленивый доступ к `blur-composite`-пайплайну (BUG-405 срез 6): вертикальный
    /// проход блюра вместе с цветовыми фильтрами и композитом в родителя.
    fn blur_composite_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blur_composite_pipeline.get_or_init(|| self.pdeps.build_blur_composite_pipeline())
    }
    /// Ленивый доступ к пайплайну аналитической тени (BUG-405 срез 7).
    fn shadow_pipeline(&self) -> &wgpu::RenderPipeline {
        self.shadow_pipeline.get_or_init(|| self.pdeps.build_shadow_pipeline())
    }
    /// Ленивый доступ к `backdrop_blit`-пайплайну (BUG-406): компилирует его при
    /// первом обращении и кэширует на весь срок жизни рендера.
    fn backdrop_blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.backdrop_blit_pipeline.get_or_init(|| self.pdeps.build_backdrop_blit_pipeline())
    }
    /// Ленивый доступ к паре mask-layer-пайплайнов (alpha, luminance) — BUG-406.
    fn mask_layer_pipelines(&self) -> &(wgpu::RenderPipeline, wgpu::RenderPipeline) {
        self.mask_layer_pipelines.get_or_init(|| self.pdeps.build_mask_layer_pipeline())
    }

    /// Заменяет источник лукапа face-ов. Полезно для тестов (mock-provider) и
    /// headless-режимов (отключить системный скан). `None` отключает поиск —
    /// рендер всегда использует default face.
    #[must_use]
    pub fn with_font_provider(mut self, provider: Option<Arc<dyn FontProvider>>) -> Self {
        self.font_provider = provider;
        self
    }

    /// Заменяет `FontProvider` на работающем рендере. Используется shell-ом,
    /// чтобы передать `FontRegistry` с @font-face шрифтами после загрузки
    /// страницы (Renderer уже создан, builder-паттерн недоступен).
    pub fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.content_generation = self.content_generation.wrapping_add(1);
        self.font_provider = provider;
        // Новый провайдер может отвечать иначе на те же (families, weight,
        // style) — например, FontRegistry с загруженным @font-face.
        self.resolve_cache.clear();
    }

    /// Эагерно загружает указанные family-имена через текущий `FontProvider`,
    /// чтобы они были доступны для codepoint cascade ещё до первого `DrawText`
    /// с этой family-ой в CSS. Используется shell-ом для прогрева
    /// fallback-цепочки (Noto Color Emoji / Noto Sans CJK / etc.), без
    /// которой эмодзи и CJK на странице без явного `font-family` падают
    /// в `.notdef`. Имена, не найденные в провайдере или с битым TTF, тихо
    /// пропускаются. Берётся weight=400 + style=normal — для fallback-целей
    /// этого достаточно. Идемпотентно: повторный вызов на уже загруженной
    /// family не делает работы благодаря `face_id_by_path` cache-у.
    pub fn preload_fallback_chain(&mut self, families: &[&str]) {
        for name in families {
            let _ = self.resolve_face_id(
                &[(*name).to_string()],
                FontWeight::NORMAL,
                FontStyle::Normal,
                FontStretch::NORMAL,
            );
        }
    }

    /// Returns the normalized GPU fingerprint (vendor/renderer strings).
    ///
    /// Returns ("WebKit", "Generic GPU") regardless of actual adapter to prevent
    /// WebGL fingerprinting attacks (ADR-007 Layer 4).
    pub fn gpu_fingerprint(&self) -> &GpuFingerprint {
        &self.gpu_fingerprint
    }

    /// Shortcut: эагерно загружает `CURATED_FALLBACK_FAMILIES` (Noto Color
    /// Emoji / Noto Sans CJK / Apple Color Emoji / Segoe UI Emoji /
    /// PingFang / Hiragino / Microsoft YaHei / Yu Gothic / Malgun Gothic /
    /// Noto Sans Arabic / Hebrew / Devanagari / Thai). На каждой ОС
    /// найдётся лишь часть имён — остальные тихо пропустятся. Это
    /// разблокирует codepoint-cascade для эмодзи / CJK / RTL / Indic /
    /// Thai на страницах **без явного CSS `font-family`** для этих
    /// скриптов. Вызывается shell-ом один раз после `Renderer::new_async`.
    /// Идемпотентен (preload_fallback_chain → resolve_face_id cache).
    pub fn preload_curated_fallbacks(&mut self) {
        self.content_generation = self.content_generation.wrapping_add(1);
        self.preload_fallback_chain(crate::fallback::CURATED_FALLBACK_FAMILIES);
    }

    /// Резолвит `face_id` для `DrawText` с указанным `font-family` списком.
    /// Если `font_provider` есть — перебирает имена в порядке приоритета
    /// (CSS Fonts L4 §3.1), для первого найденного через [`Self::pick_family_face`]
    /// — лениво загружает TTF и возвращает `face_id`. Generic CSS-family-ы
    /// (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/`system-ui`) резолвятся
    /// через `FontProvider::pick_generic_face` (платформенная таблица кандидатов,
    /// BUG-128) — не пропускаются. Если ни одно имя не найдено — возвращает 0
    /// (default face).
    fn resolve_face_id(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
    ) -> usize {
        // DS-4: chrome never queries the CSS FontProvider — every chrome
        // `DrawText` passes an empty `font_family` (page content always has a
        // non-empty one, from the UA/author stylesheet's font-family cascade),
        // so an empty list defaults to the bundled chrome UI face (Golos
        // Text). Reserved bundled family names resolve directly here,
        // independent of whether a `FontProvider` is installed at all.
        if families.is_empty() {
            return self.chrome_face_id.unwrap_or(0);
        }
        for fam in families {
            match fam.as_str() {
                "Golos Text" => return self.chrome_face_id.unwrap_or(0),
                "Golos Text Medium" => {
                    return self.chrome_face_medium_id.or(self.chrome_face_id).unwrap_or(0);
                }
                "JetBrains Mono" => return self.mono_face_id.unwrap_or(0),
                _ => {}
            }
        }
        let Some(provider) = self.font_provider.clone() else {
            return 0;
        };
        // Мемоизация: горячий путь (каждый DrawText каждого кадра) — один
        // hash-lookup без аллокаций вместо to_lowercase + pick_face.
        let cache_key = Self::resolve_cache_key(families, weight, style, stretch);
        if let Some(&id) = self.resolve_cache.get(&cache_key) {
            return id;
        }
        let resolved =
            self.resolve_face_id_uncached(families, weight, style, stretch, &provider);
        self.resolve_cache.insert(cache_key, resolved);
        resolved
    }

    /// Ключ мемо-кэша [`Self::resolve_face_id`]: хэш `(families, weight,
    /// style, stretch)` без аллокаций. Вынесен, чтобы префетч и резолв
    /// считали ключ одинаково.
    fn resolve_cache_key(
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
    ) -> u64 {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for fam in families {
            h.write(fam.as_bytes());
            h.write_u8(0xFF); // разделитель — ["ab","c"] ≠ ["a","bc"]
        }
        h.write_u16(weight.0);
        h.write_u8(match style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
        });
        // Часть ключа: `pick_face` выбирает по stretch-у отдельный
        // condensed/expanded face, значит два stretch-а одного
        // (family, weight, style) — это два разных face_id.
        h.write_u16(stretch.0);
        h.finish()
    }

    /// Резолвит одну CSS-family в системный face.
    ///
    /// Generic-имя (`serif`/`sans-serif`/`monospace`/`cursive`/`fantasy`/
    /// `system-ui`) идёт через [`FontProvider::pick_generic_face`] — таблицу
    /// платформенных кандидатов (BUG-128); конкретное имя — напрямую через
    /// `pick_face`. Раньше generic-имена молча пропускались, и любой
    /// `font-family: serif` рисовался bundled-Inter-ом (sans).
    fn pick_family_face(
        provider: &Arc<dyn FontProvider>,
        family: &str,
        weight: u16,
        style: CssFontStyle,
        stretch: u16,
    ) -> Option<FaceRecord> {
        if lumen_core::ext::is_generic_family(family) {
            provider.pick_generic_face(family, weight, style, stretch)
        } else {
            provider.pick_face(family, weight, style, stretch)
        }
    }

    /// Конверсия paint-стиля в стиль `FontProvider`-а.
    fn css_style_of(style: FontStyle) -> CssFontStyle {
        match style {
            FontStyle::Normal => CssFontStyle::Normal,
            FontStyle::Italic => CssFontStyle::Italic,
            FontStyle::Oblique => CssFontStyle::Oblique,
        }
    }

    /// Параллельная предзагрузка face-ов для всех `DrawText` кадра
    /// (p1-exp-wgpu-only, ярус 1 «вынос загрузки face-ов с render-пути»).
    ///
    /// Раньше первый кадр страницы грузил каждый новый face
    /// ПОСЛЕДОВАТЕЛЬНО внутри пре-резолва: `fs::read` + WOFF-декод +
    /// `build_face_metrics` (~180 мс на 1000000-final.html). Здесь та же
    /// работа выполняется до резолва пачкой в scoped-потоках: диск и декод
    /// независимых face-ов идут параллельно, вставка в `self.faces` — на
    /// UI-потоке в детерминированном порядке (порядок первого появления в
    /// display list-е, как у последовательного кода).
    ///
    /// Семантика [`Self::resolve_face_id_uncached`] сохранена: грузится
    /// только первый `pick_face`-кандидат каждого списка family; если его
    /// загрузка/парсинг провалились — face просто не вставляется, и
    /// последующий последовательный резолв повторит попытку и упадёт на
    /// следующую family штатным путём (редкий случай битого шрифта).
    /// Тёплый кадр (все ключи в `resolve_cache`) не делает ничего.
    fn prefetch_faces_parallel(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
    ) {
        let Some(provider) = self.font_provider.clone() else {
            return;
        };
        // Кандидаты: путь + байты из провайдера (@font-face virtual path)
        // либо None → fs::read в воркере.
        let mut jobs: Vec<(PathBuf, Option<Arc<[u8]>>)> = Vec::new();
        let mut seen_keys: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut scheduled: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for cmd in content.iter().chain(overlay.iter()) {
            let DisplayCommand::DrawText {
                font_family, font_weight, font_style, font_stretch, ..
            } = cmd
            else {
                continue;
            };
            let key =
                Self::resolve_cache_key(font_family, *font_weight, *font_style, *font_stretch);
            if self.resolve_cache.contains_key(&key) || !seen_keys.insert(key) {
                continue;
            }
            for fam in font_family {
                // DS-4: reserved bundled chrome names resolve without the
                // provider in `resolve_face_id` — skip them here too, else
                // every frame re-attempts a `pick_face` lookup for hot chrome
                // text (omnibox, DevTools) that never gets cached (the actual
                // resolve short-circuits before reaching `resolve_cache`).
                if matches!(fam.as_str(), "Golos Text" | "Golos Text Medium" | "JetBrains Mono") {
                    continue;
                }
                let Some(rec) = Self::pick_family_face(
                    &provider,
                    fam,
                    font_weight.0,
                    Self::css_style_of(*font_style),
                    font_stretch.as_percent(),
                ) else {
                    continue;
                };
                if !self.face_id_by_path.contains_key(&rec.path)
                    && !scheduled.contains(&rec.path)
                {
                    let mem = provider.read_face_bytes(&rec.path);
                    scheduled.insert(rec.path.clone());
                    jobs.push((rec.path, mem));
                }
                break; // как в резолве: первый pick_face-хит завершает перебор
            }
        }
        if jobs.is_empty() {
            return;
        }

        // Воркеры разбирают job-ы через атомарный курсор; результат кладётся
        // по индексу job-а — порядок вставки детерминирован.
        let n_workers = std::thread::available_parallelism()
            .map_or(4, std::num::NonZeroUsize::get)
            .min(jobs.len())
            .min(8);
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        // Слот результата job-а: байты шрифта + построенные метрики.
        type FaceSlot = std::sync::Mutex<Option<(Arc<[u8]>, FaceMetrics)>>;
        let results: Vec<FaceSlot> =
            jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|s| {
            for _ in 0..n_workers {
                s.spawn(|| {
                    loop {
                        let i = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((path, mem)) = jobs.get(i) else {
                            break;
                        };
                        // `mem` (Arc из @font-face-реестра) клонируется как
                        // счётчик ссылок; диск читается в новый Arc (BUG-272).
                        let raw: Arc<[u8]> = match mem {
                            Some(bytes) => Arc::clone(bytes),
                            None => match std::fs::read(path) {
                                Ok(b) => Arc::from(b),
                                Err(_) => continue,
                            },
                        };
                        // Ошибки декода/парсинга здесь НЕ логируются: face
                        // просто не вставляется, последовательный резолв
                        // повторит попытку и залогирует штатно (без дублей).
                        // `Ok(None)` (уже sfnt — как все @font-face-байты) отдаёт
                        // тот же Arc, что и реестр: рендер разделяет буфер.
                        let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
                            Ok(Some(decoded)) => Arc::from(decoded),
                            Ok(None) => raw,
                            Err(_) => continue,
                        };
                        let Some(metrics) = build_face_metrics(&bytes) else {
                            continue;
                        };
                        if let Ok(mut slot) = results[i].lock() {
                            *slot = Some((bytes, metrics));
                        }
                    }
                });
            }
        });

        for ((path, _), slot) in jobs.into_iter().zip(results) {
            let Ok(mut guard) = slot.lock() else { continue };
            let Some((bytes, metrics)) = guard.take() else {
                continue; // битый шрифт: последовательный резолв повторит и залогирует
            };
            let id = self.faces.len();
            self.faces.push(LoadedFace { bytes, metrics: Some(metrics) });
            self.face_id_by_path.insert(path, id);
        }
    }

    /// Loads one `FaceRecord` into `self.faces` (WOFF/WOFF2 → sfnt decode +
    /// `build_face_metrics`) and returns its `face_id`. `None` on read/decode/
    /// parse failure. Idempotent: an already-loaded path is looked up instead
    /// of reparsed. Factored out of [`Self::resolve_face_id_uncached`] so both
    /// the primary face and its same-bucket siblings (BUG-434) share the
    /// loading logic.
    fn load_face_by_record(
        &mut self,
        rec: &FaceRecord,
        provider: &Arc<dyn FontProvider>,
    ) -> Option<usize> {
        if let Some(&id) = self.face_id_by_path.get(&rec.path) {
            return Some(id);
        }
        // @font-face in-memory байты (virtual path) или диск для системных шрифтов.
        // Реестр отдаёт Arc (клон = счётчик ссылок), диск — новый Arc (BUG-272).
        let raw: Arc<[u8]> = if let Some(mem_bytes) = provider.read_face_bytes(&rec.path) {
            mem_bytes
        } else {
            let disk_bytes = std::fs::read(&rec.path).ok()?;
            Arc::from(disk_bytes)
        };
        // Transparent WOFF/WOFF2 → sfnt conversion before parsing.
        // `Ok(None)` (@font-face-байты уже sfnt) переиспользует Arc реестра.
        let bytes: Arc<[u8]> = match maybe_decode_font(&raw) {
            Ok(Some(decoded)) => Arc::from(decoded),
            Ok(None) => raw,
            Err(e) => {
                eprintln!("[font] WOFF decode failed {}: {e}", rec.path.display());
                return None;
            }
        };
        let Some(metrics) = build_face_metrics(&bytes) else {
            eprintln!("[font] parse failed {}", rec.path.display());
            return None;
        };
        let id = self.faces.len();
        self.faces.push(LoadedFace { bytes, metrics: Some(metrics) });
        self.face_id_by_path.insert(rec.path.clone(), id);
        Some(id)
    }

    /// Полный (немемоизированный) резолв — вынесен из [`Self::resolve_face_id`],
    /// который добавляет кэш поверх.
    fn resolve_face_id_uncached(
        &mut self,
        families: &[String],
        weight: FontWeight,
        style: FontStyle,
        stretch: FontStretch,
        provider: &Arc<dyn FontProvider>,
    ) -> usize {
        for fam in families {
            let Some(rec) = Self::pick_family_face(
                provider,
                fam,
                weight.0,
                Self::css_style_of(style),
                stretch.as_percent(),
            ) else {
                continue;
            };
            let Some(primary_id) = self.load_face_by_record(&rec, provider) else {
                continue;
            };
            // BUG-434: @font-face subsets of the same (family, weight, style,
            // stretch) partition the codepoint space via non-overlapping
            // `unicode-range` (CSS Fonts L4 §5.1) instead of competing —
            // `pick_family_face` only ever returns one of them. Load every
            // sibling too, so `pick_face_for_codepoint`'s cmap scan over
            // `self.faces` can actually find the one that has the glyph,
            // instead of silently missing subsets that were never parsed.
            for sibling in provider.lookup_faces(fam) {
                if sibling.path == rec.path
                    || sibling.weight != rec.weight
                    || sibling.style != rec.style
                    || sibling.stretch != rec.stretch
                {
                    continue;
                }
                self.load_face_by_record(&sibling, provider);
            }
            return primary_id;
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
    fn compute_image_gpu_key(&self, src: &str, box_rect: Rect, fit: ObjectFit, pos: ObjectPosition) -> String {
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
    fn ensure_image_gpu_key(
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
    fn prefetch_image_resizes_parallel(
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
    fn make_gpu_image_entry(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
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
    fn make_gpu_image_entry_mipped(&self, rgba: &[u8], width: u32, height: u32) -> GpuImage {
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
    fn recover_exhausted_atlas(&mut self) {
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

    fn wgpu_color_for_canvas_bg(color: &Color, target: ColorSpace) -> [f32; 4] {
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
    fn surface_dims(&self) -> (u32, u32) {
        if let Some(c) = &self.config {
            (c.width, c.height)
        } else {
            (self.headless_w, self.headless_h)
        }
    }

    /// Создать uniform-буфер группы 0 на `slots` слотов и bind group к нему.
    /// Привязывается ОДИН слот (`size` = размер структуры), выбираемый
    /// динамическим офсетом на `set_bind_group` (BUG-405 срез 4).
    fn create_uniform_buffer(
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
    fn upload_vertex_buffer<T: Copy>(
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
    fn write_uniform_slots(
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

    fn create_layer_texture(&mut self, width: u32, height: u32) -> OffscreenLayer {
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
    fn release_layer_to_pool(&mut self, layer: OffscreenLayer) {
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
    fn small_depth_view(&mut self, width: u32, height: u32) -> wgpu::TextureView {
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
    fn ensure_scratch_layer(&mut self, width: u32, height: u32) {
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
    fn ensure_backdrop_layer(&mut self, width: u32, height: u32) {
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
    fn ensure_backdrop_cache_texture(&mut self, ordinal: u32, width: u32, height: u32) -> bool {
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

    fn ensure_layer_textures(&mut self, count: usize, width: u32, height: u32) {
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
    fn create_page_band(&mut self, sw: u32, band_h_px: u32, band_top_css: f32) {
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
    fn warm_page_band(&mut self, sw: u32, band_h_px: u32) {
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
    fn prepare_page_compose(
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
    fn overlay_cache_step(
        &mut self,
        overlay: &[DisplayCommand],
    ) -> Result<Option<usize>, wgpu::SurfaceError> {
        let current: Vec<u64> =
            overlay.iter().map(crate::display_list::hash_one_command).collect();
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

    fn compose_page(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
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
            self.overlay_cache_step(overlay)?
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
            let (hashes, folds) = crate::display_list::hash_display_list_dual_memo(
                content,
                overlay,
                skip,
                (scroll_x, scroll_y),
                (sw0, sh0),
                band_dims,
                reuse,
            );
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
            && self.compose_page(content, overlay, scroll_y, &prep, band_key, &mut marks)?
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

    /// Тело рендера одного пасса-цели (см. [`RenderPassMode`]). Общий для
    /// обычного кадра, оффскрин-рендера полосы скролл-композитора и
    /// композиции полоса+overlay; отличия сведены к выбору целевого view,
    /// размеров «поверхности» и финализации (present / счётчики / хэш).
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn render_impl(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
        mode: RenderPassMode,
    ) -> Result<(), wgpu::SurfaceError> {
        // BUG-453: устройство потеряно (TDR, сон, отключение монитора,
        // обновление драйвера — коллбэк из `init_pipelines`) — дальше по
        // функции идут вызовы `self.device`/`self.queue`, а в конце —
        // `frame.present()`, который на потерянном устройстве паникует
        // изнутри wgpu без пути перехвата (`SurfaceTexture::present`
        // возвращает `()`). Выходим раньше, чем тронули хоть один из них.
        if self.device_lost.get().is_some() {
            return Err(wgpu::SurfaceError::Lost);
        }
        // BUG-274: пофазный тайминг кадра (LUMEN_FRAME_LOG=2) — разбивка
        // wgpu-кадра на faces/collect/prep/acquire/encode/submit, чтобы
        // диагностировать, какая фаза жжёт CPU в простое.
        let phase_log = crate::frame_log_level() >= 2;
        let t_frame0 = std::time::Instant::now();

        // BUG-405: забрать всё, что успел скомпилировать поток прогрева, до
        // того как кадр полезет за пайплайнами. Не блокирует: не приехало —
        // ячейка останется пустой и кадр скомпилирует пайплайн сам, как раньше.
        self.drain_warmed_pipelines();

        // BUG-406 срез 3: горячие пайплайны, наоборот, дожидаются — но ЗДЕСЬ, до
        // захвата surface-текстуры, а не посреди уже открытого пасса: ждать
        // секунду, держа кадр swapchain-а, значит рисковать таймаутом презента.
        // Со второго кадра все пять ячеек полны и вызов вырождается в пять
        // проверок `OnceCell`. Ждать конкретно нужный вид (аварийная ветка
        // `await_hot` в аксессорах) толку не даёт: пять компиляций идут
        // параллельно, так что ожидание всех пяти ≈ ожиданию самой долгой.
        self.await_all_hot_pipelines();

        // BUG-274: снимки диагностических счётчиков на входе в кадр — печатаем
        // дельту за кадр, а не процессный итог (кумулятивные числа не отвечают
        // на вопрос «сколько текстур родилось именно в этом кадре»).
        let tex_created_at_entry = load_counter(&TEXTURES_CREATED);
        let tex_nanos_at_entry = load_counter(&TEXTURE_CREATE_NANOS);
        let pool_hits_at_entry = load_counter(&TEXTURE_POOL_HITS);
        let pool_misses_at_entry = load_counter(&TEXTURE_POOL_MISSES);
        // BUG-405 срез 3: та же дельта-схема для промахов глиф-атласа.
        let glyphs_at_entry = load_counter(&GLYPHS_RASTERIZED);
        let glyph_nanos_at_entry = load_counter(&GLYPH_RASTER_NANOS);

        // Размеры цели: для Band — полоса, для OverlayCache — сама поверхность
        // (см. её doc-комментарий), иначе — поверхность окна/headless.
        let (sw0, sh0) = match &mode {
            RenderPassMode::Band { w_px, h_px, .. }
            | RenderPassMode::OverlayCache { w_px, h_px, .. } => (*w_px, *h_px),
            _ => self.surface_dims(),
        };

        // CSS Filter Effects L1 §2 — backdrop-filter result cache.
        // Two consecutive frames hashing identically guarantee every backdrop
        // element's filtered output is identical, so the composite step can
        // reuse the cached texture and skip the expensive blur passes.
        // (Хэш считается только при наличии backdrop-filter — после переезда
        // skip-identical в render() дешёвого готового base_hash здесь нет.)
        let backdrop_frame_hash: Option<u64> = if self.backdrop_cache.is_enabled()
            && crate::display_list::contains_backdrop_filter(content, overlay)
        {
            Some(crate::display_list::hash_display_list(
                content, overlay, scroll_x, scroll_y, sw0, sh0,
            ))
        } else {
            None
        };

        // Параллельная предзагрузка новых face-ов (диск + WOFF-декод +
        // метрики в scoped-потоках) — до пре-резолва, чтобы холодный кадр
        // не грузил шрифты последовательно (~180 мс → max по одному face).
        self.prefetch_faces_parallel(content, overlay);

        // Pre-resolve primary face_id для каждой DrawText-команды +
        // lazy-загрузка новых face-ов до сбора вершин. Делается до парсинга
        // (resolve мутирует self.faces).
        //
        // BUG-771: результат адресуется ГЛОБАЛЬНЫМ индексом команды
        // (`content`, затем `overlay`), а не курсором по «встреченным
        // DrawText». Курсор был сдвигаемым: команда текста может не дойти до
        // своей ветки (viewport-кулинг, пропуск тени) и не забрать свой id —
        // после первой такой команды весь остаток кадра брал чужой face.
        // На странице это незаметно (у тела один face), а хром рисуется
        // последним и другой family-ой — и получал face страницы: те же
        // строки, другие метрики и другие записи атласа, то есть «тот же
        // текст, но плотнее». Индекс не сдвигается ни от какого `continue`.
        let text_face_ids = resolve_text_face_ids(content, overlay, |fam, w, s, st| {
            self.resolve_face_id(fam, w, s, st)
        });

        // PRE-PASS: создаём CPU-ресайз текстуры для DrawImage до того, как
        // lazy_faces займёт &self.faces. Scroll offset не влияет на SIZE
        // (только на position), поэтому используем rect напрямую.
        // Тяжёлая CPU-часть (resize + RGBA + ICC) — параллельно; цикл
        // ensure_image_gpu_key ниже остаётся страховкой (на попадании — no-op).
        self.prefetch_image_resizes_parallel(content, overlay);
        for cmd in content.iter().chain(overlay.iter()) {
            match cmd {
                DisplayCommand::DrawImage { rect, src, object_fit, object_position, .. }
                | DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                    self.ensure_image_gpu_key(src, *rect, *object_fit, *object_position);
                }
                _ => {}
            }
        }

        // Codepoint-cascade и baseline берутся из owned `FaceMetrics`
        // (построены один раз при загрузке face-а). `Font::parse` нужен
        // только на медленных путях — промах глиф-атласа и variation axes —
        // и выполняется лениво через per-frame memo. Тёплый кадр не парсит
        // ни одного face-а (раньше: все face-ы каждый кадр, 1.4–2.7 мс).
        let mut lazy_faces = LazyParsedFaces::new(&self.faces);
        let t_after_faces = t_frame0.elapsed();

        // ── Сбор вершин ────────────────────────────────────────────────────
        let mut fill_vertices: Vec<FillVertex> = Vec::new();
        let mut circle_vertices: Vec<CircleVertex> = Vec::new();
        let mut rrect_vertices: Vec<RRectVertex> = Vec::new();
        let mut shadow_vertices: Vec<ShadowVertex> = Vec::new();
        let mut text_vertices: Vec<TextVertex> = Vec::new();
        let mut image_vertices: Vec<ImageVertex> = Vec::new();
        // Bind groups для image draw-ов в порядке появления. DrawOp::Image
        // хранит индекс в этот Vec вместо клонирования BindGroup в каждый op.
        let mut image_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
        let mut grad_vertices: Vec<GradVertex> = Vec::new();
        // Per-gradient CPU uniform data; index = grad_batch_idx in DrawOp::Gradient.
        let mut grad_params: Vec<GradParamsCpu> = Vec::new();
        // CSS Images L4 §4 — `cross-fade` GPU resources. Vertices form one quad
        // per command; bind groups hold both image textures, sampler and the
        // progress uniform. Index in `cross_fade_bind_groups` = `cf_batch_idx`
        // on `DrawOp::CrossFade`.
        let mut cross_fade_vertices: Vec<CrossFadeVertex> = Vec::new();
        let mut cross_fade_bind_groups: Vec<wgpu::BindGroup> = Vec::new();

        // Ordered draw operations. Каждая рисующая DisplayCommand → один
        // DrawOp в этом списке. SetScissor добавляется при изменении clip-стека.
        // В render-pass обходим список линейно — это сохраняет painter's order
        // между типами команд (fill/image/text больше не идут тремя раздельными
        // блоками — теперь смешаны в исходном порядке появления).
        let mut draw_ops: Vec<DrawOp> = Vec::new();

        // Стек активных clip-rect-ов в CSS-px (после intersection с предыдущими).
        // Пустой стек = full-frame scissor. PushClipRect добавляет пересечение
        // с топом; PopClip снимает.
        let mut clip_stack: Vec<Rect> = Vec::new();

        // Стек активных blend-mode-ов (CSS Compositing & Blending L1 §5).
        // Phase 0: stack отслеживается для корректного баланса Push/Pop;
        // рендеринг всегда использует Normal pipeline (ALPHA_BLENDING).
        // Реальное переключение pipeline по mode — задача 1B.5+.
        let mut blend_mode_stack: Vec<BlendMode> = Vec::new();

        // CSS Transforms L1 §13 — стек активных forward-матриц. Каждый элемент
        // хранит АККУМУЛИРОВАННОЕ произведение (родитель · self), т.е. на топе
        // лежит матрица, готовая к прямому применению к viewport-координатам
        // вершин. На PushTransform — `top.multiply(&new)` (multiplication
        // справа моделирует «применить self до родителя» в column-major
        // конвенции, что соответствует CSS «inner transform applied first»).
        // На PopTransform — сбрасываем топ.
        //
        // BUG-405 срез 38: стек ЗАТРАВЛЕН страничным смещением (ADR-016 M0.4).
        // До среза его клал сюда сам шелл — командой `PushTransform` в начале
        // копии display list-а; копия и была самой дорогой снимаемой статьёй
        // кадра попадания. Затравка эквивалентна той обёртке: топ стека — та же
        // матрица, накопление вложенных `PushTransform` идёт через неё, и
        // страничная трансляция остаётся самой внешней. Нулевое смещение
        // (headless, `render_to_image`, тесты) стек не трогает — там обёртки
        // не было и подавно, и путь «матрицы нет» обязан остаться прежним.
        let mut transform_stack: Vec<Mat4> = Vec::new();
        let page_offset_seeded = self.page_offset != (0.0, 0.0);
        if page_offset_seeded {
            transform_stack.push(Mat4::translation_2d(self.page_offset.0, self.page_offset.1));
        }

        // Render plan: список батчей и composite-переходов.
        #[derive(Clone, Copy)]
        enum LoadOpChoice {
            /// Clear colour converted to the target `target_color_space` (default: white).
            Clear(wgpu::Color),
            /// Transparent clear for off-screen opacity layers.
            ClearTransparent,
            /// Load existing contents (accumulate).
            Load,
            /// Первый батч уровня 0 в пассе кромки кольцевой полосы (BUG-405
            /// срез 32): цвет ГРУЗИТСЯ — клир снёс бы строки полосы за
            /// пределами кромки, их фон рисует явный квад, — а глубина всё
            /// равно ЧИСТИТСЯ: она живёт один пасс, и `Load` притащил бы сюда
            /// значения прошлой кромки.
            LoadColorClearDepth,
        }
        struct DrawBatchPlan { target_level: usize, load_op: LoadOpChoice, ops_start: usize, ops_end: usize }
        struct CompositePlan { from_level: usize, comp_v_start: u32, mode: BlendMode }
        // CSS Masking L1 §4: gradient mask spec — stored in plan for render-time GPU pass.
        // For Linear: p0/p1 are UV endpoints (from linear_gradient_uv_endpoints).
        // For Radial: p0=[cx_pct,cy_pct], p1=ending-shape radii in UV units.
        // For Conic:  p0=[cx_pct,cy_pct], p1=[width,height] (box-space atan2).
        // `quad` — вершины квада маски в ЭКРАННЫХ CSS px: rect уже прогнан
        // через накопленный `PushTransform`, потому что рендер-стадия матрицы
        // не видит, а контент-слой рисуется трансформированным (BUG-277).
        #[derive(Clone)]
        enum MaskGradientSpec {
            Linear { params: GradParamsCpu, quad: [GradVertex; 6] },
            Radial { params: GradParamsCpu, quad: [GradVertex; 6] },
            Conic  { params: GradParamsCpu, quad: [GradVertex; 6] },
        }
        // CSS Masking L1 §4: mask composite plan. `from_level` = offscreen level
        // with element content; `mask_src` = key in self.images (image mask).
        // `mask_gradient` = gradient mask rendered to temp surface-size texture.
        // `mask_v_start..mask_v_end` indexes into `mask_vertices`.
        // Box<MaskGradientSpec>: варианты несут заголовок + квад из 6 вершин;
        // boxing avoids large-variant warning.
        struct MaskCompositePlan {
            from_level: usize,
            mask_v_start: u32,
            mask_v_end: u32,
            mask_src: Option<String>,
            mask_gradient: Option<Box<MaskGradientSpec>>,
        }
        // CSS Filter Effects L1 — filter composite plan.
        // `from_level` = offscreen layer with element content.
        // `filters` = filter list (may include Blur + color filters).
        // `comp_v_start` = start of 6-vertex fullscreen quad in composite_vertices.
        /// bbox-офскрин блюра element-фильтра (BUG-405 срез 24): scratch
        /// H-прохода живёт в размере региона, а не всей цели рендера.
        ///
        /// `rect` = `[x, y, w, h]` в device px цели (начало = начало scissor-а,
        /// ширина/высота выровнены вверх до 64 px ради попаданий
        /// `texture_pool`). `src_v_start` — квад H-прохода (позиция во весь
        /// офскрин, UV — область полноразмерного источника), `dst_v_start` —
        /// квад слитого пасса (позиция — прямоугольник региона в цели, UV —
        /// весь офскрин).
        #[derive(Clone, Copy)]
        struct FilterRegion {
            rect: [u32; 4],
            src_v_start: u32,
            dst_v_start: u32,
        }
        struct FilterCompositePlan {
            from_level: usize,
            filters: Vec<FilterFn>,
            comp_v_start: u32,
            /// bbox-офскрин блюра (см. [`FilterRegion`]). `None` — scratch
            /// размером в цель, путь до среза 24: kill-switch
            /// `LUMEN_NO_BBOX_FILTER=1`, отсутствие scissor-а (контент слоя
            /// не ограничен) или регион ≈ вся цель.
            region: Option<FilterRegion>,
            /// Ограничение закрашиваемой области всех трёх фильтр-пассов
            /// (blur H / blur V / composite): bbox контента уровня, раздутый
            /// на радиус блюра (= min(ceil(3σ),32) текселей — как в шейдере,
            /// и как BLUR_SAMPLE_SCALE=3.0 в WebRender). None = контент
            /// уровня не удалось ограничить → полноэкранные пассы, как раньше.
            /// Корректность чтений за пределами scissor гарантирована полными
            /// LoadOp::Clear этих текстур (clear не подчиняется scissor).
            scissor: Option<DeviceScissor>,
        }
        // CSS Filter Effects L1 §2 / Compositing §13 — backdrop-filter plan.
        // `from_level` = element's offscreen layer (content rendered here).
        // `filters` = backdrop filter list.
        // `comp_v_start` = fullscreen quad (blur passes + element composite).
        // `bounds_v_start` = bounded quad (color-filter blit to parent at element bounds).
        struct BackdropFilterCompositePlan {
            from_level: usize,
            filters: Vec<FilterFn>,
            comp_v_start: u32,
            bounds_v_start: u32,
            /// Stable index among backdrop elements in this frame (paint order).
            /// Cache key for [`Renderer::backdrop_cache`] and the matching texture.
            ordinal: u32,
            /// bbox-офскрины backdrop-фильтра (EXPERIMENT.md §2): рабочая
            /// область `[x, y, w, h]` в device px родительской текстуры —
            /// element bounds + радиус ядра блюра (формула шейдера), ширина/
            /// высота выровнены вверх до 64 px (стабильность texture_pool).
            /// Ping-pong/кэш-текстуры создаются этого размера, а не размера
            /// родителя; UV bounds-квада запечены относительно региона.
            /// `None` — фолбэк на полноразмерный путь (kill-switch, вырожденные
            /// bounds или регион ≈ весь родитель).
            region: Option<[u32; 4]>,
        }
        // CSS Masking L1 §5 — mask-layer composite plan.
        // `from_level`   = offscreen level where mask content was rendered.
        // `parent_level` = from_level − 1: where element content lives.
        // `ml_v_start/end` indexes into `mask_layer_vertices`.
        // `mode` selects alpha vs. luminance mask compositing.
        struct MaskLayerCompositePlan {
            from_level: usize,
            mode: MaskMode,
            ml_v_start: u32,
            ml_v_end: u32,
        }
        // CSS Overflow L3 §2 — rounded-clip composite plan.
        // `from_level` = offscreen level holding the clipped subtree;
        // `v_start` = first of 6 vertices in `rrect_clip_vertices`.
        struct RRectClipCompositePlan {
            from_level: usize,
            v_start: u32,
        }
        // CSS Masking L1 §3 — composite-план формы `clip-path`.
        // `from_level` = offscreen-уровень с обрезаемым поддеревом;
        // `v_start` = первая из 6 вершин в `path_clip_vertices`;
        // `params` = форма в экранных CSS px (свой uniform на каждый пасс —
        // общий буфер дал бы тот же write_buffer-hazard, что в срезе 7).
        struct PathClipCompositePlan {
            from_level: usize,
            v_start: u32,
            params: Box<PathClipParamsCpu>,
        }
        enum RenderPlanItem {
            Draw(DrawBatchPlan),
            Composite(CompositePlan),
            MaskComposite(MaskCompositePlan),
            FilterComposite(FilterCompositePlan),
            BackdropFilterComposite(BackdropFilterCompositePlan),
            MaskLayerComposite(MaskLayerCompositePlan),
            RRectClipComposite(RRectClipCompositePlan),
            PathClipComposite(PathClipCompositePlan),
        }

        let mut render_plan: Vec<RenderPlanItem> = Vec::new();
        // BUG-405 срез 4: сколько скруглённых клипов кадра всё же открыли
        // offscreen-уровень (по три пасса на клип) вместо шейдерного контура.
        // Это и есть гейт правки — см. `rrect_clip_levels()`.
        let mut level_rrect_clips: u64 = 0;
        let mut composite_vertices: Vec<CompositeVertex> = Vec::new();
        // Accumulated vertex data for mask composite passes.
        let mut mask_vertices: Vec<MaskVertex> = Vec::new();
        // Accumulated vertex data for mask-layer composite passes (PushMaskLayer/PopMaskLayer).
        let mut mask_layer_vertices: Vec<MaskVertex> = Vec::new();
        // Stack of PushMask params. Pushed by PushMask*, popped by PopMask.
        // Either `src` (image key) or `gradient` is set; never both.
        struct MaskPushInfo {
            src: Option<String>,
            gradient: Option<MaskGradientSpec>,
            size: BackgroundSize,
            position: ObjectPosition,
            repeat: BackgroundRepeat,
            rect: Rect,
            /// Накопленный `PushTransform` на момент открытия маски. `PopMask`
            /// гонит через него вершины квада: сам контент рисуется
            /// трансформированным, а квад композита без этого лежал бы в
            /// нетрансформированных координатах (BUG-277).
            transform: Option<Mat4>,
        }
        let mut mask_params_stack: Vec<MaskPushInfo> = Vec::new();
        // Accumulated vertex data for rounded-clip composite passes.
        let mut rrect_clip_vertices: Vec<RRectClipVertex> = Vec::new();
        // CSS Overflow L3 §2 — параметры скруглённого клипа, открывшего
        // offscreen-уровень: контур в экранных CSS px + метка в `render_plan`.
        struct RRectClipLevel {
            rect: Rect,
            radii: CornerRadii,
            plan_mark: usize,
        }
        // CSS Masking L1 §3 — параметры формы `clip-path`, открывшей
        // offscreen-уровень: форма в экранных CSS px, её bbox (геометрия
        // квада) + метка в `render_plan`.
        // `params` в боксе: без индирекции вариант формы (584 байта) раздул бы
        // весь `ClipLevel` в десять раз против скруглённого (56 байт).
        struct PathClipLevel {
            rect: Rect,
            params: Box<PathClipParamsCpu>,
            plan_mark: usize,
        }
        // Какой контур открыл уровень — общий `PopClip` разбирает по варианту.
        enum ClipLevel {
            RRect(RRectClipLevel),
            Path(PathClipLevel),
            /// BUG-405 срез 4: скруглённый клип без уровня — контур уехал в
            /// uniform, парный `PopClip` только возвращает прежний слот.
            ///
            /// Срез 8 сделал слот вложенным, поэтому «прежний» — это не всегда
            /// 0: внутренний клип возвращает управление слоту ВНЕШНЕГО, а не
            /// «клипа нет». Хранить его в самой записи надёжнее пересборки на
            /// `PopClip`: стек контуров к этому моменту уже укорочен.
            Shader { prev_slot: u32 },
        }
        // По записи на КАЖДЫЙ push клипа (`PushClipRect`/`PushClipRoundedRect`/
        // `PushClipPath`), чтобы `PopClip` знал, открывал ли его парный push
        // уровень. `None` — обычный scissor-клип, как раньше.
        // Инвариант: стек двигается ровно теми же командами, что и `clip_stack`,
        // за вычетом `PushScrollLayer`/`PopScrollLayer` (у них своя пара).
        let mut clip_level_stack: Vec<Option<ClipLevel>> = Vec::new();
        // Накопленные вершины composite-пассов формы клипа.
        let mut path_clip_vertices: Vec<PathClipVertex> = Vec::new();
        // Stack for PushMaskLayer: (rect, mode). Popped by PopMaskLayer.
        let mut mask_layer_stack: Vec<(Rect, MaskMode)> = Vec::new();

        /// Фактически закрашенная область offscreen-уровня в CSS px
        /// (эксперимент bbox-scissor, EXPERIMENT.md §2). `Empty` — в уровень
        /// ещё ничего не нарисовано; `Rect` — объединение вершин всех draw-ops
        /// уровня плюс области дочерних композитов; `Unbounded` — состав
        /// уровня не удалось ограничить (маска/backdrop) → пассы уровня
        /// остаются полноэкранными. Безопасность по построению: любой
        /// не-учтённый источник пикселей обязан помечать уровень Unbounded.
        #[derive(Clone, Copy)]
        enum LevelBounds {
            Empty,
            Rect { x0: f32, y0: f32, x1: f32, y1: f32 },
            Unbounded,
        }
        impl LevelBounds {
            fn add_point(&mut self, x: f32, y: f32) {
                if !x.is_finite() || !y.is_finite() {
                    *self = LevelBounds::Unbounded;
                    return;
                }
                match self {
                    LevelBounds::Empty => *self = LevelBounds::Rect { x0: x, y0: y, x1: x, y1: y },
                    LevelBounds::Rect { x0, y0, x1, y1 } => {
                        *x0 = x0.min(x);
                        *y0 = y0.min(y);
                        *x1 = x1.max(x);
                        *y1 = y1.max(y);
                    }
                    LevelBounds::Unbounded => {}
                }
            }
            fn add_rect(&mut self, rx0: f32, ry0: f32, rx1: f32, ry1: f32) {
                self.add_point(rx0, ry0);
                self.add_point(rx1, ry1);
            }
        }
        let mut level_bounds: Vec<LevelBounds> = vec![LevelBounds::Unbounded];

        let mut current_level: usize = 0;
        // (alpha, метка render_plan.len() на момент Push) — метка позволяет
        // выбросить из плана ВСЕ пассы слоя (viewport-cull невидимых слоёв):
        // offscreen-текстуры имеют размер окна, контент за его пределами
        // физически не попадает ни в одну текстуру, так что отсечение
        // эквивалентно сегодняшнему клиппингу растеризацией.
        let mut level_alpha_stack: Vec<(f32, usize)> = Vec::new();
        // Tracks blend mode per opened offscreen level (for non-Normal PushBlendMode).
        let mut level_blend_mode_stack: Vec<(BlendMode, usize)> = Vec::new();
        // Tracks filter list per opened offscreen level (for CSS filter compositing).
        let mut filter_stack: Vec<(Vec<FilterFn>, usize)> = Vec::new();
        // Stack for backdrop-filter: (filter_list, element_bounds_css_px).
        // Bounds are stored already in **screen** space (scroll offset applied,
        // accumulated `PushTransform` applied) — both the parent-layer region the
        // backdrop is copied from and the quad it is blitted back through are
        // read out of the parent layer, whose contents live in screen space.
        let mut backdrop_filter_stack: Vec<(Vec<FilterFn>, lumen_core::geom::Rect)> = Vec::new();
        // Monotonic counter assigning a stable ordinal to each backdrop element
        // (in paint/pop order) — the key into the backdrop-filter result cache.
        let mut backdrop_ordinal: u32 = 0;
        let mut level_first: Vec<bool> = vec![true];
        let mut batch_start: usize = 0;
        // BUG-405 срез 5: сколько offscreen-уровней кадра оказались невидимы
        // (viewport-cull) и сколько разрезов пасса родителя от них удалось
        // склеить обратно. Гейт правки — второй счётчик, а не время кадра.
        let mut culled_levels: u32 = 0;
        let mut merged_cull_splits: u32 = 0;
        // Плечо A/B снимается один раз на кадр: рычаг процесса
        // (`LUMEN_NO_CULL_MERGE=1`) ИЛИ инстансный переключатель, которым тест
        // рисует один и тот же список обоими путями в одном процессе.
        let cull_merge_off = cull_merge_disabled() || !self.cull_merge_enabled;
        // BUG-405 срез 7: то же двойное плечо для аналитической тени.
        let shadow_analytic_off = shadow_analytic_disabled() || !self.shadow_analytic_enabled;
        // Сколько внешних теней кадра нарисованы аналитически, то есть без
        // своего offscreen-уровня. Гейт правки — этот счётчик вместе с
        // `filter_passes`, а не время кадра.
        let mut shadow_draws: u32 = 0;
        // Хвост тени, который уже учтён её квадом: заливка и парный
        // `PopFilter` пропускаются как команды. Пара «список (content/overlay),
        // индекс последней пропускаемой команды».
        let mut shadow_skip_until: Option<(bool, usize)> = None;

        // Текущий выставленный scissor (для дедупликации SetScисsor-команд).
        // None = не выставлен (первый SetScissor нужен в любом случае).
        let mut current_scissor: Option<DeviceScissor> = None;
        // Размеры цели пасса (для Band — полосы), не поверхности окна.
        let (surface_w, surface_h) = (sw0, sh0);
        // BUG-405 срез 29: высота цели ДЛЯ ОТСЕВА — обычно она же, но под
        // рычагом переписи [`band_draw_fraction`] это доля высоты полосы.
        // Отсев (scissor команд и видимость уровней) считает цель короче,
        // тогда как сама текстура, пасс и depth остаются полноразмерными:
        // так меряется цена промаха по числу ПЕРЕРИСОВАННЫХ строк при
        // неизменном размере цели (пункт 60 остатка).
        let cull_h = match (&mode, band_draw_fraction()) {
            (RenderPassMode::Band { .. }, Some(frac)) => band_cull_height(surface_h, frac),
            _ => surface_h,
        };
        // BUG-405 срез 30: рычаг переписи [`band_pass_load_ops`] — снять
        // `Clear` цвета и/или depth У ПАССА ПОЛОСЫ, чтобы разложить постоянную
        // статью надбавки промаха (пункт 62). Вне Band-рендера рычаг не
        // действует: окно чистится каждым кадром независимо от полосы.
        let (band_load_color, band_load_depth) = match &mode {
            RenderPassMode::Band { .. } => band_pass_load_ops(),
            _ => (false, false),
        };
        // BUG-405 срез 32: кромка кольца — какие строки цели пасс имеет право
        // перерисовать. `None` (и любой не-Band режим) — вся цель.
        let band_strip = match &mode {
            RenderPassMode::Band { strip, .. } => *strip,
            _ => None,
        };
        // BUG-405 срез 41: клир уровня 0 у [`RenderPassMode::OverlayCache`]
        // обязан быть прозрачным, а не цветом фона страницы — эта цель
        // держит UI-хром, а не документ (см. её doc-комментарий).
        let level0_transparent = matches!(&mode, RenderPassMode::OverlayCache { .. });

        let dpr_f32 = self.scale_factor.max(1e-6) as f32;
        // CSS-px размер цели пасса — ровно то, что уходит в `Uniforms.viewport`
        // (см. запись uniform-буфера ниже). Маски сэмплируют полноразмерные
        // текстуры по `pos / viewport`, а НЕ `pos / surface`: позиции вершин в
        // CSS px, а surface — в device px, и при dpr ≠ 1 это разные числа.
        let viewport_w = (surface_w as f32 / dpr_f32).max(1e-6);
        let viewport_h = (surface_h as f32 / dpr_f32).max(1e-6);

        // Объединяет позиции вершин диапазона draw-op-а в bbox уровня.
        // Вершины уже в CSS px, после transform/scroll — bbox финальный.
        macro_rules! union_op_verts {
            ($lb:expr, $vec:ident, $start:expr, $count:expr) => {
                for v in &$vec[*$start as usize..(*$start + *$count) as usize] {
                    $lb.add_point(v.pos[0], v.pos[1]);
                }
            };
        }

        macro_rules! flush_batch {
            () => {{
                // bbox-scissor: перед сбросом батча учесть его вершины в
                // границах текущего offscreen-уровня (уровень 0 не считаем).
                if current_level > 0 && batch_start < draw_ops.len() {
                    if let Some(lb) = level_bounds.get_mut(current_level) {
                        for op in &draw_ops[batch_start..] {
                            match op {
                                DrawOp::SetScissor(_) | DrawOp::SetClip(_) => {}
                                DrawOp::Fill { v_start, v_count } => union_op_verts!(lb, fill_vertices, v_start, v_count),
                                DrawOp::Circle { v_start, v_count } => union_op_verts!(lb, circle_vertices, v_start, v_count),
                                DrawOp::RRect { v_start, v_count } => union_op_verts!(lb, rrect_vertices, v_start, v_count),
                                DrawOp::Shadow { v_start, v_count } => union_op_verts!(lb, shadow_vertices, v_start, v_count),
                                DrawOp::Text { v_start, v_count } => union_op_verts!(lb, text_vertices, v_start, v_count),
                                DrawOp::Image { v_start, v_count, .. } => union_op_verts!(lb, image_vertices, v_start, v_count),
                                DrawOp::Gradient { v_start, v_count, .. } => union_op_verts!(lb, grad_vertices, v_start, v_count),
                                DrawOp::CrossFade { v_start, v_count, .. } => union_op_verts!(lb, cross_fade_vertices, v_start, v_count),
                            }
                        }
                    }
                }
                let first = level_first.get(current_level).copied().unwrap_or(false);
                let load_op = if first {
                    if current_level == 0 && band_strip.is_some() {
                        LoadOpChoice::LoadColorClearDepth
                    } else if current_level == 0 && level0_transparent {
                        LoadOpChoice::ClearTransparent
                    } else if current_level == 0 {
                        let rgba = self.canvas_bg
                            .map_or_else(
                                || Self::wgpu_color_for_canvas_bg(&Color::WHITE, self.target_color_space),
                                |bg| Self::wgpu_color_for_canvas_bg(&bg, self.target_color_space),
                            );
                        LoadOpChoice::Clear(wgpu::Color { r: rgba[0] as f64, g: rgba[1] as f64, b: rgba[2] as f64, a: rgba[3] as f64 })
                    } else {
                        LoadOpChoice::ClearTransparent
                    }
                } else {
                    LoadOpChoice::Load
                };
                let has_ops = batch_start < draw_ops.len();
                if has_ops || first {
                    render_plan.push(RenderPlanItem::Draw(DrawBatchPlan {
                        target_level: current_level,
                        load_op,
                        ops_start: batch_start,
                        ops_end: draw_ops.len(),
                    }));
                    if current_level < level_first.len() {
                        level_first[current_level] = false;
                    }
                }
                batch_start = draw_ops.len();
                current_scissor = None;
            }}
        }

        // BUG-405 срез 5 — выбросить невидимый уровень из плана И склеить пасс
        // родителя, который разрезал парный `push`.
        //
        // Любой `push*`, открывающий уровень, начинает со `flush_batch!()`:
        // накопленный батч родителя обязан уехать в свой пасс, потому что
        // дальше меняется цель. Когда на `pop` уровень оказывается невидимым
        // (пустой bbox или целиком за поверхностью), `render_plan.truncate`
        // убирает его контент и композит — но пасс родителя уже разрезан, и
        // ровно этот разрез стоит ~1 мс на пасс на глубоком командном списке
        // (срез 2). Здесь разрез отменяется: `Draw`-элемент, положенный тем
        // флешем, снимается, а его операции возвращаются в открытый батч —
        // следующий флеш выпустит их одним пассом вместе с продолжением.
        //
        // Операции самого выброшенного поддерева (хвост `draw_ops` за
        // `ops_end`) удаляются: на них больше не ссылается ни один элемент
        // плана, а внутри склеенного батча они рисовались бы прямо в родителя.
        //
        // Условие применимости — последний оставшийся элемент плана есть
        // `Draw` в уровень родителя. Ничего не добавляется в `draw_ops` между
        // тем флешем и первой операцией поддерева, поэтому его `ops_end` и есть
        // длина `draw_ops` на момент `push`.
        macro_rules! cull_invisible_level {
            ($plan_mark:expr) => {{
                render_plan.truncate($plan_mark);
                culled_levels += 1;
                let parent = current_level.wrapping_sub(1);
                let reopen = match render_plan.last() {
                    Some(RenderPlanItem::Draw(b)) if b.target_level == parent => {
                        Some((b.ops_start, b.ops_end, !matches!(b.load_op, LoadOpChoice::Load)))
                    }
                    _ => None,
                };
                if let Some((ops_start, ops_end, was_first)) = reopen
                    && !cull_merge_off
                {
                    render_plan.pop();
                    draw_ops.truncate(ops_end);
                    batch_start = ops_start;
                    // Батч снова открыт: если он был первым для уровня, его
                    // `Clear` ещё не выполнен — вернуть флаг, иначе цель
                    // останется с прошлого кадра.
                    if was_first
                        && let Some(f) = level_first.get_mut(parent)
                    {
                        *f = true;
                    }
                    current_scissor = None;
                    merged_cull_splits += 1;
                }
            }};
        }

        // BUG-277 срезы 8/14 — открыть offscreen-уровень под клип точной формы:
        // поддерево рисуется в собственный уровень, а парный `PopClip`
        // композитит его через покрытие контура (`PathClipComposite`). `$rect` —
        // экранный AABB клипа (квад композита), `$params` — сам контур.
        macro_rules! open_path_clip_level {
            ($rect:expr, $params:expr) => {{
                flush_batch!();
                let plan_mark = render_plan.len();
                current_level += 1;
                while level_first.len() <= current_level {
                    level_first.push(true);
                }
                level_first[current_level] = true;
                while level_bounds.len() <= current_level {
                    level_bounds.push(LevelBounds::Empty);
                }
                level_bounds[current_level] = LevelBounds::Empty;
                clip_level_stack.push(Some(ClipLevel::Path(PathClipLevel {
                    rect: $rect,
                    params: Box::new($params),
                    plan_mark,
                })));
            }};
        }

        // CSS Positioning L3 §6.3 — position:sticky offset stack.
        // Each BeginStickyLayer pushes a (dy, dx) that clamps scroll for its subtree.
        let viewport_css_h = surface_h as f32 / dpr_f32;
        let viewport_css_w = surface_w as f32 / dpr_f32;
        let mut sticky_stack: Vec<(f32, f32)> = Vec::new();

        // Compose-путь скролл-композитора: полоса страницы рисуется первым
        // op-ом level 0 (после LoadOp::Clear того же пасса) — под overlay,
        // на месте контента, который она заменяет. Обычный image-квад:
        // painter's order и батчинг не нарушаются.
        if let Some(blit) = self.pending_base_blit.take() {
            let image_batch_idx = image_bind_groups.len() as u32;
            image_bind_groups.push(blit.bind_group);
            // Квадов два, когда полоса адресуется кольцом и шов попал внутрь
            // вьюпорта (срез 32); иначе один — как раньше.
            for (rect, uv0, uv1) in blit.quads {
                let v_start = image_vertices.len() as u32;
                push_image_quad(&mut image_vertices, rect, uv0, uv1, 1.0);
                draw_ops.push(DrawOp::Image { v_start, v_count: 6, image_batch_idx });
            }
        }

        // BUG-405 срез 32: кромка кольцевой полосы. Пасс перерисовывает только
        // свои строки, и ограничение это живёт ТРЕМЯ отдельными механизмами,
        // потому что у них разная область действия:
        //
        // * `strip_rect` — клип, который [`sync_scissor_to_stack`] подмешивает
        //   в scissor КАЖДОЙ команды уровня 0. Не `clip_stack`: тот
        //   пересекается с CSS-клипами и уехал бы внутрь offscreen-уровней,
        //   а туда ему нельзя (см. ниже). Не scissor пасса: его затирает
        //   первый же `SetScissor` из списка (ловушка пункта 60 остатка).
        // * `cull_y0/cull_y1` — границы отсева команд, тоже только на уровне 0.
        // * `cull_top_px/cull_bot_px` — те же границы для отсева НЕВИДИМЫХ
        //   УРОВНЕЙ, здесь глубина уже не важна: что бы уровень ни рисовал, в
        //   кадр он попадает через композит в уровень 0, обрезанный кромкой.
        //
        // Внутрь уровня кромка НЕ проникает: блюр читает свою текстуру целиком,
        // и обрезанный по кромке источник дал бы у её края другой результат,
        // чем полная перерисовка. Уровень рисуется целиком, а обрезается его
        // композит — то есть кромка экономит на уровнях только выброшенными
        // целиком, а не срезанными наполовину.
        //
        // Клир цвета такому пассу запрещён (он не скрайзится и снёс бы
        // соседние строки кольца), поэтому фон кромки заливается явным квадом
        // на ДАЛЬНЕЙ плоскости: записанная им глубина 1.0 совпадает с той,
        // которую оставил бы `Clear(1.0)`, то есть квад не отбраковывает
        // содержимое с отрицательным `z`, как отбраковал бы на `z = 0`.
        let strip_rect: Option<Rect> = band_strip.map(|s| Rect {
            x: 0.0,
            y: s.row0 as f32 / dpr_f32,
            width: viewport_w,
            height: s.rows as f32 / dpr_f32,
        });
        let (cull_y0, cull_y1) = match strip_rect {
            Some(r) => (r.y, r.y + r.height),
            // BUG-405 срез 33: рычаг переписи [`band_draw_fraction`] обязан
            // резать и отсев КОМАНД, а не только видимость уровней. До среза 33
            // он понижал одну лишь `cull_h`, то есть на странице БЕЗ уровней
            // (сплошной текст) не убирал из кадра полосы ни одной команды и ни
            // одной вершины — печатал `frac 0.05` в лог и мерил ту же
            // конфигурацию, что и `frac 1.0`. Вне рычага выражение тождественно
            // прежнему: `cull_h == surface_h`, а `viewport_css_h` — это тот же
            // `surface_h / dpr`.
            None => (0.0, cull_h as f32 / dpr_f32),
        };
        let cull_top_px = cull_y0 * dpr_f32;
        let cull_bot_px = (cull_y1 * dpr_f32).min(cull_h as f32);
        if let Some(strip) = strip_rect {
            let scissor = css_rect_to_device_scissor(strip, dpr_f32, surface_w, cull_h);
            if !scissor.is_empty() {
                draw_ops.push(DrawOp::SetScissor(scissor));
                current_scissor = Some(scissor);
                let bg = self.canvas_bg.map_or_else(
                    || Self::wgpu_color_for_canvas_bg(&Color::WHITE, self.target_color_space),
                    |bg| Self::wgpu_color_for_canvas_bg(&bg, self.target_color_space),
                );
                let v_start = fill_vertices.len() as u32;
                push_fill_quad(&mut fill_vertices, strip, bg);
                for v in &mut fill_vertices[v_start as usize..] {
                    v.z = BAND_STRIP_BG_Z;
                }
                draw_ops.push(DrawOp::Fill { v_start, v_count: 6 });
            }
        }
        // Клип кромки действует только на уровне 0 — см. блок выше.
        macro_rules! strip_clip_now {
            () => {
                if current_level == 0 { strip_rect } else { None }
            };
        }
        macro_rules! sync_scissor {
            () => {
                sync_scissor_to_stack(
                    &clip_stack,
                    strip_clip_now!(),
                    &mut current_scissor,
                    &mut draw_ops,
                    dpr_f32,
                    surface_w,
                    cull_h,
                )
            };
        }

        // BUG-405 срез 4: пред-проход по спискам — какие скруглённые клипы
        // обойдутся шейдерным контуром вместо своего offscreen-уровня.
        let shader_clip_ok_content = shader_rrect_clip_allowed(content);
        let shader_clip_ok_overlay = shader_rrect_clip_allowed(overlay);
        // Слоты uniform-буфера группы 0: 0 — «клипа нет», дальше по слоту на
        // каждый шейдерный клип кадра.
        let mut clip_slots: Vec<ClipUniformSlot> =
            vec![no_clip_slot([viewport_css_w, viewport_css_h], dpr_f32)];
        let mut active_clip_slot: u32 = 0;
        // BUG-405 срез 8: контуры активных шейдерных клипов. Слот собирается из
        // всего стека, поэтому вложенный клип пересекается с внешним прямо во
        // фрагменте.
        let mut clip_contours: Vec<ClipContour> = Vec::new();
        // Сколько контуров шейдер держит В ЭТОМ кадре: рычаг отката среза 8
        // оставляет один, и вложенный клип снова уходит на offscreen-уровень.
        let max_clip_contours =
            if nested_shader_clip_disabled() || !self.nested_shader_clip_enabled {
                1
            } else {
                SHADER_CLIP_MAX_CONTOURS
            };
        // `LUMEN_FRAME_LOG=3`: разбивка фазы `collect` по вариантам команд —
        // сколько времени, сколько команд, сколько из них отсёк кулинг. Сестра
        // femtovg-строки `[frame] top:`; на wgpu-пути её не было, и статья
        // «SVG-обводка» (BUG-405 срез 9) поэтому два среза не была видна.
        //
        // Время команды считается меткой в начале СЛЕДУЮЩЕЙ итерации: любой
        // `continue` внутри arm'а иначе терял бы свой вклад, а именно такие
        // ветки (кулинг, отказ scissor'а) в этой фазе и интересны. Уровень 3
        // отдельно от 2 затем, что `Instant::now()` на команду — заметная доля
        // самой фазы, и замеры уровня 2 обязаны остаться чистыми.
        let cmd_log = crate::frame_log_level() >= 3;
        if cmd_log {
            SVG_SUB.reset();
            TEXT_SUB.reset();
        }
        // Слоты записи: время команды, сколько их было, сколько отсеял кулинг и
        // (срез 16) сколько времени ушло на САМ кулинг — `cull_rect` считает
        // bbox по геометрии команды, и у SVG это не константа, а проход по
        // контурам. Без отдельного слота эта работа сидит в разности между
        // `collect-top` и arm'ом команды и выглядит «остатком».
        let mut probe: std::collections::HashMap<
            &'static str,
            (std::time::Duration, u32, u32, std::time::Duration),
        > = std::collections::HashMap::new();
        let mut probe_prev: Option<(&'static str, std::time::Instant)> = None;
        let iter_content = content.iter().enumerate().map(|(i, c)| (c, false, i));
        let iter_overlay = overlay.iter().enumerate().map(|(i, c)| (c, true, i));
        // BUG-771 (диагностика): граница «контент | overlay» в text_vertices —
        // чтобы подпись текста overlay-а можно было сравнить между монолитным
        // кадром и кадром компоновки, у которых контент разный по построению.
        let mut overlay_text_v0: Option<usize> = None;
        // BUG-405 срез 38: граница «контент | overlay» снимает затравку
        // страничного смещения — overlay viewport-locked и смещения не берёт.
        // В обёртке шелла эту роль играл замыкающий `PopTransform`, стоявший
        // ровно перед overlay-списком.
        let mut page_offset_dropped = !page_offset_seeded;
        for (cmd, is_overlay, cmd_idx) in iter_content.chain(iter_overlay) {
            if is_overlay && overlay_text_v0.is_none() {
                overlay_text_v0 = Some(text_vertices.len());
            }
            if is_overlay && !page_offset_dropped {
                transform_stack.clear();
                page_offset_dropped = true;
            }
            if cmd_log {
                let now = std::time::Instant::now();
                if let Some((name, t0)) = probe_prev.take() {
                    let e = probe.entry(name).or_default();
                    e.0 += now - t0;
                    e.1 += 1;
                }
                probe_prev = Some((cmd.variant_name(), now));
            }
            // Срез 16: вся итерация SVG-команды целиком. Снимается на выходе из
            // тела цикла (в том числе через `continue`), поэтому разность
            // `iter − arm − кулинг` называет работу над командой, которую не
            // видит ни одна подстатья arm'а.
            let _t_iter = sub_timer(
                cmd_log
                    && matches!(
                        cmd,
                        DisplayCommand::DrawSvgStroke { .. } | DisplayCommand::DrawSvgFill { .. }
                    ),
                &SVG_SUB.iter,
            );
            // BUG-405 срез 7: заливка и `PopFilter` тени, нарисованной
            // аналитически, уже учтены её квадом — пропускаем их как команды.
            match shadow_skip_until {
                Some((list_overlay, until)) if list_overlay == is_overlay && cmd_idx <= until => {
                    continue;
                }
                Some(_) => shadow_skip_until = None,
                None => {}
            }
            let (dy, dx) = if is_overlay {
                (0.0_f32, 0.0_f32)
            } else {
                sticky_stack.last().copied().unwrap_or((-scroll_y, -scroll_x))
            };
            // BUG-771 (диагностика, `LUMEN_TEXT_SIG=2`): сама команда текста
            // overlay-а — чтобы отличить «шелл прислал другой список» от
            // «одну и ту же команду два пути нарисовали по-разному».
            if is_overlay
                && text_sig_level() >= 2
                && let DisplayCommand::DrawText {
                    rect, text, font_size, font_family, font_weight, font_stretch, ..
                } = cmd
            {
                eprintln!(
                    "[frame:wgpu] text-cmd rect={:.3},{:.3} size={font_size:.4} \
                     fam={font_family:?} w={font_weight:?} st={font_stretch:?} txt={:?}",
                    rect.x,
                    rect.y,
                    text.chars().take(24).collect::<String>(),
                );
            }
            // ADR-016 M0.2 viewport culling: skip self-contained leaf draws
            // whose box — shifted by the scroll/sticky offset and mapped
            // through the current accumulated transform — lands fully outside
            // the viewport (+ slop). `cull_rect` returns `None` for every
            // structural `Push*`/`Pop*`, which must always run to keep the
            // level/clip/transform stacks balanced.
            let t_cull = cmd_log.then(std::time::Instant::now);
            let culled = cmd.cull_rect().is_some_and(|local| {
                // Кромка кольца сужает отсев только на уровне 0: содержимое
                // offscreen-уровня обязано попасть в его текстуру целиком.
                let (cy0, cy1) = if current_level == 0 {
                    (cull_y0, cull_y1)
                } else {
                    (0.0, viewport_css_h)
                };
                leaf_is_offscreen(
                    translate_rect(local, dx, dy),
                    transform_stack.last(),
                    viewport_css_w,
                    cy0,
                    cy1,
                )
            });
            if let Some(t0) = t_cull {
                probe.entry(cmd.variant_name()).or_default().3 += t0.elapsed();
            }
            if culled {
                if cmd_log {
                    probe.entry(cmd.variant_name()).or_default().2 += 1;
                }
                continue;
            }
            match cmd {
                DisplayCommand::FillRect { rect, color } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), alpha);
                    push_fill_quad(&mut fill_vertices, translate_rect(*rect, dx, dy), c);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        // BUG-277 срез 13: только повёрнутый/скошенный квад уходит
                        // кромкой с растровой сетки — осевой остаётся на прежнем
                        // (побитово идентичном) пути.
                        if rotates_axes_2d(m) && !rot_aa_disabled() {
                            antialias_fill_soup(
                            &mut fill_vertices,
                            v_start as usize,
                            c,
                            dpr_f32,
                            coverage_cache_arm(
                                &mut self.coverage_cache,
                                self.coverage_cache_enabled,
                            ),
                        );
                        }
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                DisplayCommand::FillRoundedRect { rect, color, radii } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let r = translate_rect(*rect, dx, dy);
                    let v_start = rrect_vertices.len() as u32;
                    push_rrect_quad(&mut rrect_vertices, r, color_to_array(color), *radii);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_rrect_verts(&mut rrect_vertices[v_start as usize..], m);
                    }
                    let v_count = rrect_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::RRect { v_start, v_count });
                    }
                }
                DisplayCommand::DrawBorder {
                    rect,
                    widths: [wt, wr, wb, wl],
                    colors: [ct, cr, cb, cl],
                    styles: [st, sr, sb, sl],
                    radii,
                } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let r = translate_rect(*rect, dx, dy);
                    let fill_v_start = fill_vertices.len() as u32;
                    let circle_v_start = circle_vertices.len() as u32;

                    if radii.all_zero() {
                        // CSS Backgrounds L3 §6.3 — прямоугольные рёбра без угловых дуг.
                        // Каждая сторона укорочена на corner-квадраты, чтобы dash/dot
                        // паттерн шёл только вдоль прямого участка (как в Chrome/Edge).
                        // Угловые квадраты всегда solid.
                        let ct_arr = apply_alpha_to_color(color_to_array(ct), alpha);
                        let cr_arr = apply_alpha_to_color(color_to_array(cr), alpha);
                        let cb_arr = apply_alpha_to_color(color_to_array(cb), alpha);
                        let cl_arr = apply_alpha_to_color(color_to_array(cl), alpha);

                        // All styles span full box width/height including corners.
                        // Chrome/Edge draws each side at full extent; adjacent sides overlap
                        // at corners, with later-drawn sides overwriting earlier ones.
                        // Rendering order: top → right → bottom → left (left wins at corners).
                        if *wt > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y, r.width, *wt),
                                true, *wt, ct_arr, *st,
                            );
                        }
                        if *wr > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x + r.width - *wr, r.y, *wr, r.height),
                                false, *wr, cr_arr, *sr,
                            );
                        }
                        if *wb > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y + r.height - *wb, r.width, *wb),
                                true, *wb, cb_arr, *sb,
                            );
                        }
                        if *wl > 0.0 {
                            emit_border_side(
                                &mut fill_vertices, &mut circle_vertices,
                                Rect::new(r.x, r.y, *wl, r.height),
                                false, *wl, cl_arr, *sl,
                            );
                        }
                    } else {
                        // CSS Backgrounds L3 §5 + §6.3 — стороны укорочены у углов;
                        // каждый угол рисуется как дуга-сектор (tessellated arc).
                        // Каждый радиус также ограничен половиной соответствующей стороны.
                        let r_tl = radii.tl.min(r.width / 2.0).min(r.height / 2.0);
                        let r_tr = radii.tr.min(r.width / 2.0).min(r.height / 2.0);
                        let r_br = radii.br.min(r.width / 2.0).min(r.height / 2.0);
                        let r_bl = radii.bl.min(r.width / 2.0).min(r.height / 2.0);
                        let ct_arr = apply_alpha_to_color(color_to_array(ct), alpha);
                        let cr_arr = apply_alpha_to_color(color_to_array(cr), alpha);
                        let cb_arr = apply_alpha_to_color(color_to_array(cb), alpha);
                        let cl_arr = apply_alpha_to_color(color_to_array(cl), alpha);
                        // Top side (shortened by r_tl on left, r_tr on right).
                        if *wt > 0.0 {
                            let x0 = r.x + r_tl;
                            let x1 = r.x + r.width - r_tr;
                            if x1 > x0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(x0, r.y, x1 - x0, *wt),
                                    true, *wt, ct_arr, *st,
                                );
                            }
                        }
                        // Right side (shortened by r_tr on top, r_br on bottom).
                        if *wr > 0.0 {
                            let y0 = r.y + r_tr;
                            let y1 = r.y + r.height - r_br;
                            if y1 > y0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(r.x + r.width - wr, y0, *wr, y1 - y0),
                                    false, *wr, cr_arr, *sr,
                                );
                            }
                        }
                        // Bottom side (shortened by r_br on right, r_bl on left).
                        if *wb > 0.0 {
                            let x0 = r.x + r_bl;
                            let x1 = r.x + r.width - r_br;
                            if x1 > x0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(x0, r.y + r.height - wb, x1 - x0, *wb),
                                    true, *wb, cb_arr, *sb,
                                );
                            }
                        }
                        // Left side (shortened by r_tl on top, r_bl on bottom).
                        if *wl > 0.0 {
                            let y0 = r.y + r_tl;
                            let y1 = r.y + r.height - r_bl;
                            if y1 > y0 {
                                emit_border_side(
                                    &mut fill_vertices, &mut circle_vertices,
                                    Rect::new(r.x, y0, *wl, y1 - y0),
                                    false, *wl, cl_arr, *sl,
                                );
                            }
                        }
                        // Corner arcs: quarter-annulus for each corner with radius > 0.
                        // TL corner (180°→270° in screen-Y-down coords = left→up).
                        if r_tl > 0.0 {
                            let inner = (r_tl - wt.max(*wl)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r_tl, r.y + r_tl], r_tl, inner, 180.0, 270.0, ct_arr);
                        }
                        // TR corner (270°→360° = up→right).
                        if r_tr > 0.0 {
                            let inner = (r_tr - wt.max(*wr)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r.width - r_tr, r.y + r_tr], r_tr, inner, 270.0, 360.0, ct_arr);
                        }
                        // BR corner (0°→90° = right→down).
                        if r_br > 0.0 {
                            let inner = (r_br - wb.max(*wr)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r.width - r_br, r.y + r.height - r_br], r_br, inner, 0.0, 90.0, cb_arr);
                        }
                        // BL corner (90°→180° = down→left).
                        if r_bl > 0.0 {
                            let inner = (r_bl - wb.max(*wl)).max(0.0);
                            emit_border_arc(&mut fill_vertices, [r.x + r_bl, r.y + r.height - r_bl], r_bl, inner, 90.0, 180.0, cb_arr);
                        }
                    }

                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[fill_v_start as usize..], m);
                        apply_affine_to_circle_verts(&mut circle_vertices[circle_v_start as usize..], m);
                    }
                    let fill_v_count = fill_vertices.len() as u32 - fill_v_start;
                    if fill_v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start: fill_v_start, v_count: fill_v_count });
                    }
                    let circle_v_count = circle_vertices.len() as u32 - circle_v_start;
                    if circle_v_count > 0 {
                        draw_ops.push(DrawOp::Circle { v_start: circle_v_start, v_count: circle_v_count });
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
                    // Уже учтён при резолве face_id в пре-проходе выше —
                    // здесь берём готовый id из `text_face_ids`.
                    font_stretch: _,
                    font_variation_axes,
                    font_features: _,
                    font_palette,
                    tab_size,
                    highlight_name: _,
                    text_orientation,
                } => {
                    // BUG-405 срез 13: охватывающая статья arm-а — снимается и
                    // на `continue` (нет метрик / отказ scissor'а).
                    let _t_arm = sub_timer(cmd_log, &TEXT_SUB.arm);
                    // Слот пре-резолва этой самой команды (BUG-771): своя
                    // полоса + свой индекс в ней, поэтому пропуск соседа
                    // ничего не сдвигает.
                    let primary_face_id = text_face_ids
                        .get(text_face_slot(is_overlay, cmd_idx, content.len()))
                        .copied()
                        .filter(|&id| id != NO_TEXT_FACE)
                        .unwrap_or(0);
                    // BUG-771 (диагностика, `LUMEN_TEXT_SIG=2`): какой face
                    // и какие оси вариаций достались команде overlay-а.
                    if is_overlay && text_sig_level() >= 2 {
                        eprintln!(
                            "[frame:wgpu] text-face id={primary_face_id} nfaces={} upem={:?} bytes={:?} axes={font_variation_axes:?}",
                            lazy_faces.faces.len(),
                            lazy_faces
                                .faces
                                .get(primary_face_id)
                                .and_then(|f| f.metrics.as_ref())
                                .map(|m| m.units_per_em),
                            lazy_faces.faces.get(primary_face_id).map(|f| f.bytes.len()),
                        );
                    }
                    if lazy_faces
                        .faces
                        .get(primary_face_id)
                        .and_then(|f| f.metrics.as_ref())
                        .is_none()
                    {
                        continue;
                    }
                    let t_sciss = cmd_log.then(std::time::Instant::now);
                    let scissor_ok = sync_scissor!();
                    if let Some(t0) = t_sciss {
                        sub_add(&TEXT_SUB.sciss, t0);
                    }
                    if !scissor_ok {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let v_start = text_vertices.len() as u32;
                    let dest_rect = translate_rect(*rect, dx, dy);
                    // Ph3 writing-mode vertical (wgpu — live default backend,
                    // ADR-017): `Sideways` rotates the whole run 90° CW
                    // (Срез 2), mirroring the CPU rasterizer
                    // (`rasterize_text_rotated`) — glyphs are laid out
                    // horizontally at the local origin, then
                    // `rotate_text_vertices_cw` maps them onto `dest_rect`.
                    // `Mixed` splits per glyph — CJK upright, Latin rotated
                    // (Срез 3, `push_text_glyphs_mixed`, mirrors
                    // `rasterize_text_mixed`). `Upright`/`None` keep the
                    // existing horizontal path.
                    match text_orientation {
                        Some(TextOrientation::Sideways) => {
                            let glyph_rect = Rect::new(0.0, 0.0, dest_rect.height, dest_rect.width);
                            push_text_glyphs(
                                &mut text_vertices,
                                glyph_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                &mut self.text_run_cache,
                                self.text_run_cache_enabled,
                                font_variation_axes,
                                *tab_size,
                                font_palette.as_ref(),
                            );
                            rotate_text_vertices_cw(&mut text_vertices[v_start as usize..], dest_rect);
                        }
                        Some(TextOrientation::Mixed) => {
                            push_text_glyphs_mixed(
                                &mut text_vertices,
                                dest_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                &mut self.text_run_cache,
                                self.text_run_cache_enabled,
                                font_variation_axes,
                                *tab_size,
                                font_palette.as_ref(),
                            );
                        }
                        _ => {
                            push_text_glyphs(
                                &mut text_vertices,
                                dest_rect,
                                text,
                                *font_size,
                                apply_alpha_to_color(color_to_array(color), alpha),
                                primary_face_id,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                &mut self.text_run_cache,
                                self.text_run_cache_enabled,
                                font_variation_axes,
                                *tab_size,
                                font_palette.as_ref(),
                            );
                        }
                    }
                    if let Some(m) = transform_stack.last() {
                        let t_xform = cmd_log.then(std::time::Instant::now);
                        apply_affine_to_verts(&mut text_vertices[v_start as usize..], m);
                        if let Some(t0) = t_xform {
                            sub_add(&TEXT_SUB.xform, t0);
                        }
                    }
                    let v_count = text_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Text { v_start, v_count });
                    }
                }
                DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
                    // CSS Basic UI L4 §5: outline рисуется СНАРУЖИ box-а.
                    // Outer rect = box + outline-offset (по всем сторонам) +
                    // outline-width (тоже по всем сторонам). Inner граница =
                    // box + outline-offset. `OutlineStyle::Auto` рендерится
                    // как Solid (UA focus ring без дополнительного хвоста);
                    // Dashed/Dotted разворачиваются в pattern из квадратов
                    // через `emit_outline_side`.
                    if *width <= 0.0 {
                        continue;
                    }
                    if !sync_scissor!() {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let r = translate_rect(*rect, dx, dy);
                    let inner = Rect::new(
                        r.x - offset,
                        r.y - offset,
                        r.width + 2.0 * offset,
                        r.height + 2.0 * offset,
                    );
                    let w = *width;
                    let c = apply_alpha_to_color(color_to_array(color), alpha);
                    let fill_v_start = fill_vertices.len() as u32;
                    let circle_v_start = circle_vertices.len() as u32;
                    // Top stripe (с "ear" по углам слева/справа).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y - w, inner.width + 2.0 * w, w),
                        true,
                        w,
                        c,
                        *style,
                    );
                    // Bottom stripe (тоже с углами).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y + inner.height, inner.width + 2.0 * w, w),
                        true,
                        w,
                        c,
                        *style,
                    );
                    // Left stripe (между inner.y и inner.y+inner.height,
                    // без углов — они уже в top/bottom).
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x - w, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    // Right stripe.
                    emit_outline_side(
                        &mut fill_vertices,
                        &mut circle_vertices,
                        Rect::new(inner.x + inner.width, inner.y, w, inner.height),
                        false,
                        w,
                        c,
                        *style,
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[fill_v_start as usize..], m);
                        apply_affine_to_circle_verts(&mut circle_vertices[circle_v_start as usize..], m);
                    }
                    let fill_v_count = fill_vertices.len() as u32 - fill_v_start;
                    if fill_v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start: fill_v_start, v_count: fill_v_count });
                    }
                    let circle_v_count = circle_vertices.len() as u32 - circle_v_start;
                    if circle_v_count > 0 {
                        draw_ops.push(DrawOp::Circle { v_start: circle_v_start, v_count: circle_v_count });
                    }
                }
                DisplayCommand::DrawImage {
                    rect,
                    src,
                    alt,
                    object_fit,
                    object_position,
                    image_rendering,
                } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let scrolled = translate_rect(*rect, dx, dy);
                    let fit = *object_fit;
                    let pos = *object_position;

                    // Вычисляем GPU-ключ (текстура уже создана в pre-pass).
                    // GPU делает 1:1 сэмплинг по CPU-bilinear scaled текстуре →
                    // pixel-perfect совпадение с браузерами на одном железе.
                    let gpu_key = self.compute_image_gpu_key(src, scrolled, fit, pos);
                    if let Some(gpu) = self.images.get(&gpu_key) {
                        if let Some((visible, uv_min, uv_max)) = fit_image_quad(
                            scrolled,
                            (gpu.width, gpu.height),
                            fit,
                            pos,
                        ) {
                            let v_start = image_vertices.len() as u32;
                            push_image_quad(&mut image_vertices, visible, uv_min, uv_max, alpha);
                            if let Some(m) = transform_stack.last() {
                                apply_affine_to_verts(
                                    &mut image_vertices[v_start as usize..],
                                    m,
                                );
                            }
                            let v_count = image_vertices.len() as u32 - v_start;
                            let image_batch_idx = image_bind_groups.len() as u32;
                            let bg = if matches!(image_rendering, ImageRendering::Pixelated | ImageRendering::CrispEdges) {
                                gpu.bind_group_nearest.clone()
                            } else {
                                gpu.bind_group_linear.clone()
                            };
                            image_bind_groups.push(bg);
                            draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                        }
                    } else {
                        // Картинку никто не зарегистрировал (fetch не сделан /
                        // декодер упал / неизвестный формат) — fallback на
                        // серый placeholder, чтобы место в layout-е было видно.
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            scrolled,
                            apply_alpha_to_color([0.85, 0.85, 0.85, 1.0], alpha),
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                        // BUG-015: render alt text over the placeholder when the
                        // image fails to load. Uses face 0 (bundled Inter) at 12px.
                        // Only rendered when the box is tall enough for one text line.
                        const BROKEN_FONT_SIZE: f32 = 12.0;
                        const BROKEN_PAD: f32 = 4.0;
                        if !alt.is_empty()
                            && scrolled.height >= BROKEN_FONT_SIZE + 2.0 * BROKEN_PAD
                            && lazy_faces.faces.first().and_then(|f| f.metrics.as_ref()).is_some()
                        {
                            let text_rect = Rect::new(
                                scrolled.x + BROKEN_PAD,
                                scrolled.y + BROKEN_PAD,
                                (scrolled.width - 2.0 * BROKEN_PAD).max(0.0),
                                (scrolled.height - 2.0 * BROKEN_PAD).max(0.0),
                            );
                            let t_start = text_vertices.len() as u32;
                            push_text_glyphs(
                                &mut text_vertices,
                                text_rect,
                                alt,
                                BROKEN_FONT_SIZE,
                                apply_alpha_to_color([0.35, 0.35, 0.35, 1.0], alpha),
                                0,
                                &mut lazy_faces,
                                &mut self.atlas,
                                &mut self.cached_glyphs,
                                &mut self.text_run_cache,
                                self.text_run_cache_enabled,
                                &[],
                                0.0,
                                None,
                            );
                            if let Some(m) = transform_stack.last() {
                                apply_affine_to_verts(
                                    &mut text_vertices[t_start as usize..],
                                    m,
                                );
                            }
                            let t_count = text_vertices.len() as u32 - t_start;
                            if t_count > 0 {
                                draw_ops.push(DrawOp::Text { v_start: t_start, v_count: t_count });
                            }
                        }
                    }
                }
                DisplayCommand::LazyImageSlot { rect, src, object_fit, object_position, .. } => {
                    // A lazy `<img>` stays a LazyImageSlot even after the shell
                    // fetches it (the `loading="lazy"` attribute never clears).
                    // Draw the registered image if present, else the grey
                    // placeholder — same behaviour as DrawImage. (BUG-163)
                    if !sync_scissor!() {
                        continue;
                    }
                    let alpha = 1.0_f32;
                    let scrolled = translate_rect(*rect, dx, dy);
                    let fit = *object_fit;
                    let pos = *object_position;
                    let gpu_key = self.compute_image_gpu_key(src, scrolled, fit, pos);
                    if let Some(gpu) = self.images.get(&gpu_key) {
                        if let Some((visible, uv_min, uv_max)) = fit_image_quad(
                            scrolled,
                            (gpu.width, gpu.height),
                            fit,
                            pos,
                        ) {
                            let v_start = image_vertices.len() as u32;
                            push_image_quad(&mut image_vertices, visible, uv_min, uv_max, alpha);
                            if let Some(m) = transform_stack.last() {
                                apply_affine_to_verts(&mut image_vertices[v_start as usize..], m);
                            }
                            let v_count = image_vertices.len() as u32 - v_start;
                            let image_batch_idx = image_bind_groups.len() as u32;
                            image_bind_groups.push(gpu.bind_group_linear.clone());
                            draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                        }
                        continue;
                    }
                    // Not yet fetched — grey placeholder.
                    let v_start = fill_vertices.len() as u32;
                    push_fill_quad(
                        &mut fill_vertices,
                        scrolled,
                        apply_alpha_to_color([0.85, 0.85, 0.85, 1.0], 1.0),
                    );
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // Clip-stack управление. PushClipRect добавляет пересечение
                // с топом (CSS Masking L1 §3 — clip-rect = intersection всех
                // ancestor clip-region-ов). PopClip снимает топ. Scissor для
                // wgpu выставляется лениво — следующая draw-команда вызовет
                // sync_scissor_to_stack.
                DisplayCommand::PushClipRect { rect } => {
                    let scrolled = translate_rect(*rect, dx, dy);
                    // Apply accumulated transform so clip is in screen space (BUG-276).
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                    // BUG-277 срез 14: под `rotate`/`skew` scissor-AABB — уже не
                    // сам клип, а описанная рамка, и ребёнок протекает в её
                    // угловые треугольники (TEST-100 c5). Точная форма идёт
                    // через ту же машинерию, что `clip-path` (срез 8). Осевые
                    // трансформы (подавляющее большинство) остаются на дешёвом
                    // scissor-пути побитово нетронутыми.
                    match rotated_rect_clip_params(scrolled, transform_stack.last()) {
                        Some(params) => open_path_clip_level!(in_screen, params),
                        None => clip_level_stack.push(None),
                    }
                }
                // CSS Overflow L3 §2 — `overflow: hidden` на боксе с
                // `border-radius`: bbox по-прежнему идёт в scissor (дешёвое
                // отсечение), но углы им не вырезать, поэтому поддерево
                // рисуется в offscreen-уровень, а `PopClip` композитит его
                // через тот же SDF-контур, каким рисуется сам бокс.
                // Фолбэк на прежний bbox-клип (уровень НЕ открывается):
                // радиусов нет; ИЛИ трансформ не axis-aligned — контур в
                // экранных координатах тогда уже не rounded-rect.
                DisplayCommand::PushClipRoundedRect { rect, radii } => {
                    let scrolled = translate_rect(*rect, dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                    let scale = axis_aligned_scale(transform_stack.last());
                    match scale {
                        Some((sx, sy)) if radii.iter().any(|r| *r > 0.0) => {
                            // Радиусы приходят круговыми ([tl, tr, br, bl]);
                            // axis-aligned scale делает их эллиптическими.
                            let r = |i: usize| radii[i].max(0.0);
                            let radii = CornerRadii {
                                tl: r(0) * sx, tl_y: r(0) * sy,
                                tr: r(1) * sx, tr_y: r(1) * sy,
                                br: r(2) * sx, br_y: r(2) * sy,
                                bl: r(3) * sx, bl_y: r(3) * sy,
                            };
                            // BUG-405 срез 4: контур помещается в uniform —
                            // ни уровня, ни композита, ни разрыва батча. Уровень
                            // остаётся, если контуры уже заняты (срез 8 держит
                            // два, вложенность 3+ идёт на уровень) или поддерево
                            // содержит фильтр/маску/blend
                            // (`shader_rrect_clip_allowed`).
                            let ok = if is_overlay {
                                shader_clip_ok_overlay.get(cmd_idx)
                            } else {
                                shader_clip_ok_content.get(cmd_idx)
                            };
                            if clip_contours.len() < max_clip_contours
                                && *ok.unwrap_or(&false)
                                && !shader_rrect_clip_disabled()
                            {
                                // Радиусы обязаны быть ужаты в бокс тем же
                                // правилом, что и на путь уровня
                                // (`push_rrect_clip_quad`): CSS Backgrounds L3
                                // §5.5 масштабирует пересекающиеся кривые, а
                                // без этого `border-radius: 50%` (pill, круг)
                                // даёт контур с радиусами больше половины
                                // стороны — TEST-101 5.39% против 0.25%.
                                let radii = radii
                                    .clamped_to_box(in_screen.width, in_screen.height);
                                if !clip_contours.is_empty() {
                                    self.nested_shader_clips += 1;
                                }
                                clip_contours.push(ClipContour {
                                    center: [
                                        in_screen.x + in_screen.width * 0.5,
                                        in_screen.y + in_screen.height * 0.5,
                                    ],
                                    half: [in_screen.width * 0.5, in_screen.height * 0.5],
                                    radii_x: [radii.tl, radii.tr, radii.br, radii.bl],
                                    radii_y: [radii.tl_y, radii.tr_y, radii.br_y, radii.bl_y],
                                });
                                let slot = clip_slots.len() as u32;
                                clip_slots.push(clip_slot_from(
                                    &clip_contours,
                                    [viewport_css_w, viewport_css_h],
                                    dpr_f32,
                                ));
                                draw_ops.push(DrawOp::SetClip(slot));
                                clip_level_stack
                                    .push(Some(ClipLevel::Shader { prev_slot: active_clip_slot }));
                                active_clip_slot = slot;
                                continue;
                            }
                            flush_batch!();
                            let plan_mark = render_plan.len();
                            current_level += 1;
                            while level_first.len() <= current_level {
                                level_first.push(true);
                            }
                            level_first[current_level] = true;
                            while level_bounds.len() <= current_level {
                                level_bounds.push(LevelBounds::Empty);
                            }
                            level_bounds[current_level] = LevelBounds::Empty;
                            clip_level_stack.push(Some(ClipLevel::RRect(RRectClipLevel {
                                rect: in_screen,
                                radii,
                                plan_mark,
                            })));
                        }
                        // BUG-277 срез 14: повёрнутый клип без радиусов — это
                        // обычный прямоугольник, и его точный контур доступен
                        // (`rotated_rect_clip_params`). Повёрнутый СО скруглением
                        // остаётся на историческом bbox-фолбэке: контур там уже
                        // не полигон, а стадион под матрицей — отдельный долг.
                        _ => {
                            let exact = if radii.iter().all(|r| *r <= 0.0) {
                                rotated_rect_clip_params(scrolled, transform_stack.last())
                            } else {
                                None
                            };
                            match exact {
                                Some(params) => open_path_clip_level!(in_screen, params),
                                None => clip_level_stack.push(None),
                            }
                        }
                    }
                }
                // CSS Masking L1 §3 — `clip-path` произвольной формой:
                // bbox по-прежнему идёт в scissor (дешёвое отсечение), но
                // круг/эллипс/полигон им не вырезать, поэтому поддерево
                // рисуется в offscreen-уровень, а `PopClip` композитит его
                // через покрытие точной формы (тот же механизм, что у
                // скруглённого клипа, BUG-277 срез 5).
                //
                // Фолбэк на прежний bbox-клип (BUG-140, уровень НЕ
                // открывается) — когда `path_clip_params` возвращает `None`:
                // вырожденная форма, вершин больше `PATH_CLIP_MAX_VERTS` или
                // не-2D-аффинный трансформ.
                DisplayCommand::PushClipPath { shape } => {
                    let scrolled = translate_rect(shape.bounding_rect(), dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    let new = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new);
                    // Форма приходит в page px (до transform элемента), как и
                    // её bbox — сдвигаем её тем же scroll-смещением.
                    let shape_scrolled = translate_clip_shape(shape, dx, dy);
                    match path_clip_params(&shape_scrolled, transform_stack.last()) {
                        Some(params) => open_path_clip_level!(in_screen, params),
                        None => clip_level_stack.push(None),
                    }
                }
                DisplayCommand::PopClip => {
                    clip_stack.pop();
                    // Парный push открыл уровень под скруглённый клип или
                    // форму `clip-path` — закрыть его composite-пассом через
                    // покрытие контура.
                    if let Some(Some(clip)) = clip_level_stack.pop() {
                        // BUG-405 срез 4: у шейдерного клипа уровня нет —
                        // закрыть его значит вернуть ПРЕЖНИЙ слот прямо в
                        // потоке операций, без flush_batch и без композита.
                        // Срез 8: прежний слот — это слот внешнего клипа, если
                        // тот был, и 0 иначе.
                        if let ClipLevel::Shader { prev_slot } = clip {
                            clip_contours.pop();
                            draw_ops.push(DrawOp::SetClip(prev_slot));
                            active_clip_slot = prev_slot;
                            continue;
                        }
                        let (clip_rect, plan_mark) = match &clip {
                            ClipLevel::RRect(c) => (c.rect, c.plan_mark),
                            ClipLevel::Path(c) => (c.rect, c.plan_mark),
                            // Снято выше отдельной веткой: уровня нет.
                            ClipLevel::Shader { .. } => continue,
                        };
                        flush_batch!();
                        // viewport-cull: пустой/за-экранный уровень невидим —
                        // выбросить из плана и контент, и композит (как в
                        // PopOpacity: клип только УБИРАЕТ пиксели).
                        let child_now = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        let invisible = match child_now {
                            LevelBounds::Empty => true,
                            LevelBounds::Rect { x0, y0, x1, y1 } => {
                                x1 * dpr_f32 <= 0.0
                                    || y1 * dpr_f32 <= cull_top_px
                                    || x0 * dpr_f32 >= surface_w as f32
                                    || y0 * dpr_f32 >= cull_bot_px
                            }
                            LevelBounds::Unbounded => false,
                        };
                        if invisible {
                            cull_invisible_level!(plan_mark);
                            current_level -= 1;
                            continue;
                        }
                        match clip {
                            // Шейдерный клип снят веткой выше (уровня нет).
                            ClipLevel::Shader { .. } => {}
                            ClipLevel::RRect(c) => {
                                // BUG-405 срез 4: клип, которому шейдерный путь
                                // не подошёл (вложенный или с фильтром/маской
                                // внутри) — считаем, это гейт правки.
                                level_rrect_clips += 1;
                                let v_start = rrect_clip_vertices.len() as u32;
                                push_rrect_clip_quad(
                                    &mut rrect_clip_vertices,
                                    c.rect,
                                    c.radii,
                                    viewport_css_w,
                                    viewport_css_h,
                                );
                                render_plan.push(RenderPlanItem::RRectClipComposite(
                                    RRectClipCompositePlan { from_level: current_level, v_start },
                                ));
                            }
                            ClipLevel::Path(c) => {
                                let v_start = path_clip_vertices.len() as u32;
                                push_path_clip_quad(
                                    &mut path_clip_vertices,
                                    c.rect,
                                    viewport_css_w,
                                    viewport_css_h,
                                );
                                render_plan.push(RenderPlanItem::PathClipComposite(
                                    PathClipCompositePlan {
                                        from_level: current_level,
                                        v_start,
                                        params: c.params,
                                    },
                                ));
                            }
                        }
                        let child = level_bounds
                            .get(current_level)
                            .copied()
                            .unwrap_or(LevelBounds::Unbounded);
                        current_level -= 1;
                        // Композит красит родителя не шире bbox ребёнка,
                        // пересечённого с самим клипом.
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match child {
                                LevelBounds::Empty => {}
                                LevelBounds::Rect { x0, y0, x1, y1 } => lb.add_rect(
                                    x0.max(clip_rect.x),
                                    y0.max(clip_rect.y),
                                    x1.min(clip_rect.x + clip_rect.width),
                                    y1.min(clip_rect.y + clip_rect.height),
                                ),
                                LevelBounds::Unbounded => *lb = LevelBounds::Unbounded,
                            }
                        }
                    }
                }
                DisplayCommand::PushOpacity { alpha, .. } => {
                    flush_batch!();
                    level_alpha_stack.push((*alpha, render_plan.len()));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopOpacity => {
                    if !level_alpha_stack.is_empty() {
                        flush_batch!();
                        let (layer_alpha, plan_mark) = level_alpha_stack.pop().unwrap();
                        // viewport-cull: слой с alpha=0, пустой или целиком вне
                        // поверхности не виден — выбросить из плана и его
                        // контент, и композит.
                        let child_now = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        let invisible = layer_alpha <= 0.0
                            || match child_now {
                                LevelBounds::Empty => true,
                                LevelBounds::Rect { x0, y0, x1, y1 } => {
                                    x1 * dpr_f32 <= 0.0
                                        || y1 * dpr_f32 <= cull_top_px
                                        || x0 * dpr_f32 >= surface_w as f32
                                        || y0 * dpr_f32 >= cull_bot_px
                                }
                                LevelBounds::Unbounded => false,
                            };
                        if invisible {
                            cull_invisible_level!(plan_mark);
                            current_level -= 1;
                            continue;
                        }
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, layer_alpha);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode: BlendMode::Normal,
                        }));
                        let child = level_bounds
                            .get(current_level)
                            .copied()
                            .unwrap_or(LevelBounds::Unbounded);
                        current_level -= 1;
                        // Composite переносит контент дочернего уровня 1:1 —
                        // границы родителя расширяются на bbox ребёнка.
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match child {
                                LevelBounds::Empty => {}
                                LevelBounds::Rect { x0, y0, x1, y1 } => lb.add_rect(x0, y0, x1, y1),
                                LevelBounds::Unbounded => *lb = LevelBounds::Unbounded,
                            }
                        }
                    }
                }
                // CSS Compositing & Blending L1 §5 — mix-blend-mode compositing.
                // Non-Normal mode: push offscreen level + track blend mode.
                // Normal mode: no offscreen layer needed (pass-through).
                DisplayCommand::PushBlendMode { mode, .. } => {
                    blend_mode_stack.push(*mode);
                    if *mode != BlendMode::Normal {
                        flush_batch!();
                        level_blend_mode_stack.push((*mode, render_plan.len()));
                        current_level += 1;
                        while level_first.len() <= current_level {
                            level_first.push(true);
                        }
                        level_first[current_level] = true;
                        while level_bounds.len() <= current_level {
                            level_bounds.push(LevelBounds::Empty);
                        }
                        level_bounds[current_level] = LevelBounds::Empty;
                    }
                }
                DisplayCommand::PopBlendMode => {
                    blend_mode_stack.pop();
                    if let Some((mode, plan_mark)) = level_blend_mode_stack.pop() {
                        flush_batch!();
                        // viewport-cull: для всех CSS-блэндов прозрачный src
                        // оставляет backdrop неизменным (co = cs + cb·(1−as)),
                        // поэтому пустой/за-экранный слой невидим целиком.
                        let child_now = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        let invisible = match child_now {
                            LevelBounds::Empty => true,
                            LevelBounds::Rect { x0, y0, x1, y1 } => {
                                x1 * dpr_f32 <= 0.0
                                    || y1 * dpr_f32 <= cull_top_px
                                    || x0 * dpr_f32 >= surface_w as f32
                                    || y0 * dpr_f32 >= cull_bot_px
                            }
                            LevelBounds::Unbounded => false,
                        };
                        if invisible {
                            cull_invisible_level!(plan_mark);
                            current_level -= 1;
                            continue;
                        }
                        let comp_v_start = composite_vertices.len() as u32;
                        // alpha=1.0: blend shader handles all compositing math.
                        push_composite_quad(&mut composite_vertices, 1.0);
                        render_plan.push(RenderPlanItem::Composite(CompositePlan {
                            from_level: current_level,
                            comp_v_start,
                            mode,
                        }));
                        let child = level_bounds
                            .get(current_level)
                            .copied()
                            .unwrap_or(LevelBounds::Unbounded);
                        current_level -= 1;
                        // Blend-composite тоже красит родителя только в bbox
                        // ребёнка (за его пределами src прозрачен).
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match child {
                                LevelBounds::Empty => {}
                                LevelBounds::Rect { x0, y0, x1, y1 } => lb.add_rect(x0, y0, x1, y1),
                                LevelBounds::Unbounded => *lb = LevelBounds::Unbounded,
                            }
                        }
                    }
                }
                // CSS Backgrounds L3 §3.3/3.4/3.5 — background-size/position/repeat.
                DisplayCommand::DrawBackgroundImage { rect, origin_rect, src, size, position, repeat, image_rendering } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    // `area`  — paint/clip bounds (background-clip). Tiles are drawn only inside.
                    // `oarea` — positioning area (background-origin). Used for size/position math
                    //           per CSS Backgrounds L3 §3.5/3.5.2.
                    let area  = translate_rect(*rect, dx, dy);
                    let oarea = translate_rect(*origin_rect, dx, dy);
                    let Some(gpu) = self.images.get(src) else { continue };
                    let img_w = gpu.width as f32;
                    let img_h = gpu.height as f32;
                    if img_w <= 0.0 || img_h <= 0.0 { continue; }

                    // Compute tile dimensions from background-size relative to positioning area.
                    let (tile_w, tile_h) = match size {
                        BackgroundSize::Auto => (img_w, img_h),
                        BackgroundSize::Cover => {
                            let s = (oarea.width / img_w).max(oarea.height / img_h);
                            (img_w * s, img_h * s)
                        }
                        BackgroundSize::Contain => {
                            let s = (oarea.width / img_w).min(oarea.height / img_h);
                            (img_w * s, img_h * s)
                        }
                        BackgroundSize::Length(w, h) => {
                            // CSS Backgrounds L3 §3.5: percent axes resolve against the
                            // positioning area; `auto` derives from the intrinsic ratio.
                            match (w.resolve(oarea.width), h.resolve(oarea.height)) {
                                (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                                (Some(tw), None) => {
                                    let tw = tw.max(1.0);
                                    (tw, (img_h * (tw / img_w)).max(1.0))
                                }
                                (None, Some(th)) => {
                                    let th = th.max(1.0);
                                    ((img_w * (th / img_h)).max(1.0), th)
                                }
                                (None, None) => (img_w, img_h),
                            }
                        }
                    };

                    // Compute first tile origin from background-position relative to positioning area.
                    let off_x = match position.x {
                        PositionComponent::Px(px) => px,
                        PositionComponent::Percent(p) => (oarea.width - tile_w) * p,
                    };
                    let off_y = match position.y {
                        PositionComponent::Px(py) => py,
                        PositionComponent::Percent(p) => (oarea.height - tile_h) * p,
                    };
                    let tile_x0 = oarea.x + off_x;
                    let tile_y0 = oarea.y + off_y;

                    let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match repeat {
                        BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
                        BackgroundRepeat::RepeatX => (
                            tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                            tile_y0, tile_h, false,
                        ),
                        BackgroundRepeat::RepeatY => (
                            tile_x0, tile_w, false,
                            tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                        ),
                        BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
                            tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                            tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                        ),
                        BackgroundRepeat::Space => {
                            let (sx, step_x, rx) = space_axis_geometry(oarea.x, oarea.width, tile_w, off_x);
                            let (sy, step_y, ry) = space_axis_geometry(oarea.y, oarea.height, tile_h, off_y);
                            (sx, step_x, rx, sy, step_y, ry)
                        }
                    };

                    let v_start = image_vertices.len() as u32;
                    let image_batch_idx = image_bind_groups.len() as u32;
                    let bg = if matches!(image_rendering, ImageRendering::Pixelated | ImageRendering::CrispEdges) {
                        gpu.bind_group_nearest.clone()
                    } else {
                        gpu.bind_group_linear.clone()
                    };
                    image_bind_groups.push(bg);

                    // Paint bounds: tiles are clipped to the background-clip area.
                    let x_end = area.x + area.width;
                    let y_end = area.y + area.height;
                    let mut ty = tile_y_start;
                    loop {
                        if ty >= y_end { break; }
                        let mut tx = tile_x_start;
                        loop {
                            if tx >= x_end { break; }
                            // Clip tile to background area; compute partial UVs.
                            let cx = tx.max(area.x);
                            let cy = ty.max(area.y);
                            let cx1 = (tx + tile_w).min(x_end);
                            let cy1 = (ty + tile_h).min(y_end);
                            if cx < cx1 && cy < cy1 {
                                let u0 = (cx - tx) / tile_w;
                                let v0 = (cy - ty) / tile_h;
                                let u1 = (cx1 - tx) / tile_w;
                                let v1 = (cy1 - ty) / tile_h;
                                push_image_quad(&mut image_vertices,
                                    Rect::new(cx, cy, cx1 - cx, cy1 - cy),
                                    [u0, v0], [u1, v1], 1.0);
                            }
                            if !repeat_x { break; }
                            tx += step_x;
                        }
                        if !repeat_y { break; }
                        ty += step_y;
                    }
                    // CSS Transforms L1 §13 — тайлы посчитаны в локальных
                    // координатах бокса (там же и обрезаны по background-clip),
                    // поэтому накопленная матрица применяется к готовым квадам,
                    // как у `DrawImage`/`DrawLayerSnapshot`. Без этого шага
                    // фон-картинка ложилась в нетрансформированных координатах
                    // (BUG-277 срез 15).
                    if let Some(m) = transform_stack.last()
                        && !img_xform_disabled()
                    {
                        apply_affine_to_verts(&mut image_vertices[v_start as usize..], m);
                    }
                    let v_count = image_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                    }
                }
                // CSS Images L3 §3.3 — GPU linear gradient pipeline.
                DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1, line_len) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, line_len);
                    let params =
                        build_grad_params(&resolved, p0, p1, 0, *repeating, box_aspect(scrolled));
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                // CSS Images L3 §3.5 — GPU radial gradient pipeline.
                DisplayCommand::DrawRadialGradient { rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1) = radial_gradient_uv_params(
                        *center_x_pct, *center_y_pct, *radius_x, *radius_y, scrolled,
                    );
                    // Px/Calc stops resolve against the larger ending-shape radius,
                    // matching `cpu_raster::rasterize_radial_gradient` (BUG-277).
                    let line_len = radius_x.max(*radius_y).max(1.0);
                    let resolved = resolve_gradient_stops(stops, line_len);
                    let params = build_grad_params(&resolved, p0, p1, 1, *repeating, 0.0);
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                // CSS Images L4 §3.7 — GPU conic gradient pipeline.
                DisplayCommand::DrawConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    if stops.is_empty() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    // p0 = center (UV); p1 = box size in CSS px (for box-space angle).
                    let p0 = [*center_x_pct, *center_y_pct];
                    let p1 = [scrolled.width.max(1e-6), scrolled.height.max(1e-6)];
                    let from_angle_rad = from_angle_deg.to_radians();
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 2, *repeating, from_angle_rad);
                    let v_start = grad_vertices.len() as u32;
                    push_grad_quad(&mut grad_vertices, scrolled);
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_grad_verts(&mut grad_vertices[v_start as usize..], m);
                    }
                    let v_count = grad_vertices.len() as u32 - v_start;
                    let grad_batch_idx = grad_params.len() as u32;
                    grad_params.push(params);
                    draw_ops.push(DrawOp::Gradient { v_start, v_count, grad_batch_idx });
                }
                DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let scrolled = translate_rect(*rect, dx, dy);
                    // Снимок рендерится через image-pipeline: UV всегда [0,0]→[1,1]
                    // (весь снимок без object-fit). Если id не зарегистрирован —
                    // команда молча игнорируется (compositor мог вызвать evict).
                    if let Some(snap) = self.layer_snapshots.get(id) {
                        let v_start = image_vertices.len() as u32;
                        push_image_quad(
                            &mut image_vertices,
                            scrolled,
                            [0.0, 0.0],
                            [1.0, 1.0],
                            *alpha,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut image_vertices[v_start as usize..], m);
                        }
                        let v_count = image_vertices.len() as u32 - v_start;
                        let image_batch_idx = image_bind_groups.len() as u32;
                        image_bind_groups.push(snap.bind_group.clone());
                        draw_ops.push(DrawOp::Image { v_start, v_count, image_batch_idx });
                    }
                }
                // CSS Transforms L1 §13 — пушим matrix умноженную на текущий
                // топ (накопление транcформов вложенных боксов). Топ-матрица
                // применяется ко всем последующим вершинам до парного
                // PopTransform. Сам Push/Pop не флашит batch — transform
                // CPU-side применяется к вершинам, не меняет GPU-pipeline.
                DisplayCommand::PushTransform { matrix } => {
                    let accumulated = match transform_stack.last() {
                        Some(prev) => prev.multiply(matrix),
                        None => *matrix,
                    };
                    transform_stack.push(accumulated);
                }
                DisplayCommand::PopTransform => {
                    transform_stack.pop();
                }
                // CSS Overflow L3 §3.2 — PushScrollLayer: clip to padding-box + translate
                // content by (-scroll_x, -scroll_y). Combines a PushClipRect and a 2D
                // translation on the transform stack; PopScrollLayer unwinds both.
                DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } => {
                    // Clip (same as PushClipRect, accounting for sticky dx/dy).
                    // Apply the accumulated transform so the clip lands in screen
                    // space (BUG-276 fix, missed here originally — BUG-335): the
                    // clip_rect is in the PARENT's page-space, same as
                    // PushClipRect's, and must go through the SAME transform
                    // (captured before this push's own scroll translate is
                    // added to the stack below) before intersecting with
                    // clip_stack, which already holds screen-space rects.
                    let scrolled_clip = translate_rect(*clip_rect, dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled_clip, transform_stack.last());
                    let new_clip = match clip_stack.last() {
                        Some(prev) => intersect_rects(*prev, in_screen),
                        None => in_screen,
                    };
                    clip_stack.push(new_clip);
                    // Scroll translate: shift content by -scroll_x, -scroll_y.
                    let scroll_m = Mat4::translation_2d(-scroll_x, -scroll_y);
                    let accumulated = match transform_stack.last() {
                        Some(prev) => prev.multiply(&scroll_m),
                        None => scroll_m,
                    };
                    transform_stack.push(accumulated);
                }
                DisplayCommand::PopScrollLayer => {
                    transform_stack.pop();
                    clip_stack.pop();
                }
                // CSS Masking L1 §4 — PushMask*: open an offscreen layer for the element,
                // and record mask params so PopMask can composite with the mask.
                DisplayCommand::PushMaskImage { rect, src, size, position, repeat, .. } => {
                    flush_batch!();
                    mask_params_stack.push(MaskPushInfo {
                        src: Some(src.clone()),
                        gradient: None,
                        size: *size,
                        position: *position,
                        repeat: *repeat,
                        rect: translate_rect(*rect, dx, dy),
                        transform: transform_stack.last().copied(),
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                // CSS Masking L1 §4 — gradient masks: build GradParamsCpu at plan time;
                // render-time pass renders gradient → surface-size temp texture → use as mask.
                DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let (p0, p1, line_len) = linear_gradient_uv_endpoints(scrolled.width, scrolled.height, *angle_deg);
                    let resolved = resolve_gradient_stops(stops, line_len);
                    let params =
                        build_grad_params(&resolved, p0, p1, 0, *repeating, box_aspect(scrolled));
                    let transform = transform_stack.last().copied();
                    let quad = transformed_grad_quad(scrolled, transform.as_ref());
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Linear { params, quad }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                        transform,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PushMaskRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    // Mask radial gradients stay circular (farthest-corner) — the mask
                    // command carries no ending-shape, matching `cpu_raster::render_mask`.
                    let mask_dx = center_x_pct.max(1.0 - center_x_pct) * scrolled.width;
                    let mask_dy = center_y_pct.max(1.0 - center_y_pct) * scrolled.height;
                    let line_len = mask_dx.hypot(mask_dy).max(1.0);
                    let (p0, p1) = radial_gradient_uv_params(
                        *center_x_pct, *center_y_pct, line_len, line_len, scrolled,
                    );
                    let resolved = resolve_gradient_stops(stops, line_len);
                    let params = build_grad_params(&resolved, p0, p1, 1, *repeating, 0.0);
                    let transform = transform_stack.last().copied();
                    let quad = transformed_grad_quad(scrolled, transform.as_ref());
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Radial { params, quad }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                        transform,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PushMaskConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                    flush_batch!();
                    let scrolled = translate_rect(*rect, dx, dy);
                    let p0 = [*center_x_pct, *center_y_pct];
                    let p1 = [scrolled.width.max(1e-6), scrolled.height.max(1e-6)];
                    let from_angle_rad = from_angle_deg.to_radians();
                    let resolved = resolve_gradient_stops(stops, 1.0);
                    let params = build_grad_params(&resolved, p0, p1, 2, *repeating, from_angle_rad);
                    let transform = transform_stack.last().copied();
                    let quad = transformed_grad_quad(scrolled, transform.as_ref());
                    mask_params_stack.push(MaskPushInfo {
                        src: None,
                        gradient: Some(MaskGradientSpec::Conic { params, quad }),
                        size: BackgroundSize::Auto,
                        position: ObjectPosition::background_initial(),
                        repeat: BackgroundRepeat::NoRepeat,
                        rect: scrolled,
                        transform,
                    });
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopMask => {
                    flush_batch!();
                    let Some(info) = mask_params_stack.pop() else { continue };
                    let mv_start = mask_vertices.len() as u32;
                    let (mask_src, mask_gradient) = if let Some(src) = &info.src {
                        // Image mask: build tile quads — same tiling logic as DrawBackgroundImage.
                        if let Some(gpu) = self.images.get(src) {
                            let img_w = gpu.width as f32;
                            let img_h = gpu.height as f32;
                            if img_w > 0.0 && img_h > 0.0 {
                                let area = info.rect;
                                let (tile_w, tile_h) = match info.size {
                                    BackgroundSize::Auto => (img_w, img_h),
                                    BackgroundSize::Cover => {
                                        let s = (area.width / img_w).max(area.height / img_h);
                                        (img_w * s, img_h * s)
                                    }
                                    BackgroundSize::Contain => {
                                        let s = (area.width / img_w).min(area.height / img_h);
                                        (img_w * s, img_h * s)
                                    }
                                    BackgroundSize::Length(w, h) => {
                                        // CSS Masking L1 §4: percent axes resolve against the
                                        // mask painting area; `auto` keeps the intrinsic ratio.
                                        match (w.resolve(area.width), h.resolve(area.height)) {
                                            (Some(tw), Some(th)) => (tw.max(1.0), th.max(1.0)),
                                            (Some(tw), None) => {
                                                let tw = tw.max(1.0);
                                                (tw, (img_h * (tw / img_w)).max(1.0))
                                            }
                                            (None, Some(th)) => {
                                                let th = th.max(1.0);
                                                ((img_w * (th / img_h)).max(1.0), th)
                                            }
                                            (None, None) => (img_w, img_h),
                                        }
                                    }
                                };
                                let off_x = match info.position.x {
                                    PositionComponent::Px(px) => px,
                                    PositionComponent::Percent(p) => (area.width - tile_w) * p,
                                };
                                let off_y = match info.position.y {
                                    PositionComponent::Px(py) => py,
                                    PositionComponent::Percent(p) => (area.height - tile_h) * p,
                                };
                                let tile_x0 = area.x + off_x;
                                let tile_y0 = area.y + off_y;
                                let (tile_x_start, step_x, repeat_x, tile_y_start, step_y, repeat_y) = match info.repeat {
                                    BackgroundRepeat::NoRepeat => (tile_x0, tile_w, false, tile_y0, tile_h, false),
                                    BackgroundRepeat::RepeatX => (
                                        tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                                        tile_y0, tile_h, false,
                                    ),
                                    BackgroundRepeat::RepeatY => (
                                        tile_x0, tile_w, false,
                                        tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                                    ),
                                    BackgroundRepeat::Repeat | BackgroundRepeat::Round => (
                                        tile_x0 - (off_x / tile_w).ceil() * tile_w, tile_w, true,
                                        tile_y0 - (off_y / tile_h).ceil() * tile_h, tile_h, true,
                                    ),
                                    BackgroundRepeat::Space => {
                                        let (sx, step_x, rx) = space_axis_geometry(area.x, area.width, tile_w, off_x);
                                        let (sy, step_y, ry) = space_axis_geometry(area.y, area.height, tile_h, off_y);
                                        (sx, step_x, rx, sy, step_y, ry)
                                    }
                                };
                                let x_end = area.x + area.width;
                                let y_end = area.y + area.height;
                                let mut ty = tile_y_start;
                                loop {
                                    if ty >= y_end { break; }
                                    let mut tx = tile_x_start;
                                    loop {
                                        if tx >= x_end { break; }
                                        let cx = tx.max(area.x);
                                        let cy = ty.max(area.y);
                                        let cx1 = (tx + tile_w).min(x_end);
                                        let cy1 = (ty + tile_h).min(y_end);
                                        if cx < cx1 && cy < cy1 {
                                            let u0 = (cx - tx) / tile_w;
                                            let v0 = (cy - ty) / tile_h;
                                            let u1 = (cx1 - tx) / tile_w;
                                            let v1 = (cy1 - ty) / tile_h;
                                            mask_vertices.extend_from_slice(&[
                                                MaskVertex { pos: [cx,  cy ], uv_mask: [u0, v0] },
                                                MaskVertex { pos: [cx1, cy ], uv_mask: [u1, v0] },
                                                MaskVertex { pos: [cx1, cy1], uv_mask: [u1, v1] },
                                                MaskVertex { pos: [cx,  cy ], uv_mask: [u0, v0] },
                                                MaskVertex { pos: [cx1, cy1], uv_mask: [u1, v1] },
                                                MaskVertex { pos: [cx,  cy1], uv_mask: [u0, v1] },
                                            ]);
                                        }
                                        if !repeat_x { break; }
                                        tx += step_x;
                                    }
                                    if !repeat_y { break; }
                                    ty += step_y;
                                }
                            }
                        }
                        // Плитки построены в нетрансформированных координатах;
                        // uv_mask у них внутри-плиточные, поэтому трансформа
                        // касается только позиций (BUG-277 срез 6).
                        if let Some(m) = info.transform.as_ref() {
                            apply_affine_to_verts(&mut mask_vertices[mv_start as usize..], m);
                        }
                        (Some(src.clone()), None)
                    } else if let Some(grad) = info.gradient.clone() {
                        // Gradient mask: the quad is the very same (already transformed)
                        // one the gradient is rendered with, so mask texel and content
                        // pixel line up by construction. uv_mask = pos / viewport_css:
                        // the temp texture is surface-sized but written through the same
                        // `u.viewport` NDC mapping, so a CSS-px position p lands on texel
                        // p/viewport·surface — exactly what uv = p/viewport samples.
                        let quad = match &grad {
                            MaskGradientSpec::Linear { quad, .. }
                            | MaskGradientSpec::Radial { quad, .. }
                            | MaskGradientSpec::Conic { quad, .. } => *quad,
                        };
                        mask_vertices.extend(quad.iter().map(|v| MaskVertex {
                            pos: v.pos,
                            uv_mask: [v.pos[0] / viewport_w, v.pos[1] / viewport_h],
                        }));
                        (None, Some(Box::new(grad)))
                    } else {
                        (None, None)
                    };
                    let mv_end = mask_vertices.len() as u32;
                    render_plan.push(RenderPlanItem::MaskComposite(MaskCompositePlan {
                        from_level: current_level,
                        mask_v_start: mv_start,
                        mask_v_end: mv_end,
                        mask_src,
                        mask_gradient,
                    }));
                    current_level -= 1;
                    // bbox-scissor v1: mask-композит не отслеживаем — родитель
                    // помечается неограниченным (безопасный фолбэк).
                    if current_level > 0
                        && let Some(lb) = level_bounds.get_mut(current_level)
                    {
                        *lb = LevelBounds::Unbounded;
                    }
                }
                // CSS Filter Effects L1 — PushFilter opens an offscreen level;
                // PopFilter composites it onto the parent with filter applied.
                DisplayCommand::PushFilter { filters, bounds: _ } => {
                    // BUG-405 срез 7 — внешняя тень рисуется аналитически, без
                    // своего offscreen-уровня и его трёх пассов.
                    //
                    // Условия — не косметика, каждое держит равенство картинки:
                    // * `clip=0` — шейдерный скруглённый клип умножает покрытие
                    //   ПОСЛЕ размытия, а прежний путь клипал саму фигуру и
                    //   размывал уже обрезанную;
                    // * только перенос — SDF строки выведен по экранным осям;
                    // * scissor не режет тень внутри поверхности — прежний путь
                    //   размывал обрезанную фигуру, здесь обрезается результат.
                    //   Совпадение среза с краем поверхности срезом не считается:
                    //   за краем нет ни пикселей, ни текстуры уровня.
                    if !shadow_analytic_off
                        && active_clip_slot == 0
                        && let Some((sigma, rect, color, radii)) =
                            box_shadow_body(if is_overlay { overlay } else { content }, cmd_idx, filters)
                        && let Some((tx, ty)) = match transform_stack.last() {
                            None => Some((0.0, 0.0)),
                            Some(m) => transform_is_translation(m),
                        }
                    {
                        let r = translate_rect(rect, dx + tx, dy + ty);
                        // Запас квада — обрез ядра блюра, тот же
                        // `min(ceil(3σ),32)` в device px, что в BLUR_SHADER_SRC,
                        // плюс пиксель на сглаживание края.
                        let pad_css = (3.0 * sigma).ceil().min(32.0) / dpr_f32 + 1.0;
                        let padded = Rect::new(
                            r.x - pad_css,
                            r.y - pad_css,
                            r.width + 2.0 * pad_css,
                            r.height + 2.0 * pad_css,
                        );
                        let want =
                            css_rect_to_device_scissor(padded, dpr_f32, surface_w, surface_h);
                        // Кромка кольца (срез 32) в `clip_stack` не лежит и
                        // сюда не попадает — и это существенно: условие `uncut`
                        // держит равенство картинки со старым путём, который
                        // размывал уже обрезанную ФИГУРУ, а кромка режет не
                        // фигуру, а окно перерисовки, и обрезанный ею результат
                        // совпадает с тем, что нарисовал бы полный пасс. Первая
                        // редакция среза клала кромку в стек — и каждая тень у
                        // её границы уезжала на offscreen-уровень: `layers`
                        // 1 → 2, `filt` 16 → 26, промах вдвое дороже полной
                        // перерисовки (тот же отказ, что в пункте 60).
                        let desired = match clip_stack.last() {
                            Some(c) => {
                                css_rect_to_device_scissor(*c, dpr_f32, surface_w, surface_h)
                            }
                            None => DeviceScissor::full(surface_w, surface_h),
                        };
                        let uncut = want.x >= desired.x
                            && want.y >= desired.y
                            && want.x + want.width <= desired.x + desired.width
                            && want.y + want.height <= desired.y + desired.height;
                        if want.is_empty() {
                            // Тень целиком за поверхностью: прежний путь
                            // выбрасывал такой уровень (viewport-cull) — здесь
                            // просто ничего не рисуем.
                            shadow_skip_until = Some((is_overlay, cmd_idx + 2));
                            continue;
                        }
                        if uncut {
                            if sync_scissor!() {
                                let v_start = shadow_vertices.len() as u32;
                                push_shadow_quad(
                                    &mut shadow_vertices,
                                    r,
                                    color_to_array(&color),
                                    radii,
                                    sigma,
                                    pad_css,
                                );
                                let v_count = shadow_vertices.len() as u32 - v_start;
                                draw_ops.push(DrawOp::Shadow { v_start, v_count });
                                shadow_draws += 1;
                            }
                            shadow_skip_until = Some((is_overlay, cmd_idx + 2));
                            continue;
                        }
                    }
                    flush_batch!();
                    filter_stack.push((filters.clone(), render_plan.len()));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopFilter => {
                    if let Some((filters, plan_mark)) = filter_stack.pop() {
                        flush_batch!();
                        let content = if bbox_scissor_disabled() {
                            LevelBounds::Unbounded
                        } else {
                            level_bounds
                                .get(current_level)
                                .copied()
                                .unwrap_or(LevelBounds::Unbounded)
                        };
                        // Радиус блюра в текселях — та же формула, что в
                        // BLUR_SHADER_SRC: min(ceil(3σ),32); шейпер шагает по
                        // 1 текселю surface-текстуры. + 2 px запас на bilinear.
                        let blur_pad = filters
                            .iter()
                            .find_map(|f| match f {
                                FilterFn::Blur(s) if *s > 0.0 => {
                                    Some((3.0 * *s).ceil().min(32.0))
                                }
                                _ => None,
                            })
                            .unwrap_or(0.0)
                            + 2.0;
                        // (scissor пассов, раздутый bbox в CSS px для родителя)
                        let (scissor, parent_rect) = match content {
                            LevelBounds::Unbounded => (None, None),
                            LevelBounds::Empty => {
                                // Слой пуст: composite прозрачной текстуры —
                                // визуальный no-op; выбросить из плана и
                                // отрисовку контента слоя (viewport-cull).
                                cull_invisible_level!(plan_mark);
                                current_level -= 1;
                                continue;
                            }
                            LevelBounds::Rect { x0, y0, x1, y1 } => {
                                let pad_css = blur_pad / dpr_f32;
                                let (ix0, iy0) = (x0 - pad_css, y0 - pad_css);
                                let (ix1, iy1) = (x1 + pad_css, y1 + pad_css);
                                // Кромка кольца (срез 32) режет пассы фильтра
                                // по Y, когда композит идёт прямо в уровень 0:
                                // результат за кромкой всё равно не нужен, а
                                // ИСТОЧНИК блюра остаётся целым (содержимое
                                // уровня кромкой не обрезано). Вложенный
                                // фильтр (`current_level > 1`) так резать
                                // нельзя: его результат читает родительский
                                // уровень целиком.
                                let (ty0, ty1) = if current_level == 1 {
                                    (cull_top_px, cull_bot_px)
                                } else {
                                    (0.0, cull_h as f32)
                                };
                                let sx0 = ((ix0 * dpr_f32).floor().max(0.0) as u32).min(surface_w);
                                let sy0 = ((iy0 * dpr_f32).floor().max(ty0) as u32).min(ty1 as u32);
                                let sx1 = ((ix1 * dpr_f32).ceil().max(0.0) as u32).min(surface_w);
                                let sy1 = ((iy1 * dpr_f32).ceil().max(ty0) as u32).min(ty1 as u32);
                                if sx1 <= sx0 || sy1 <= sy0 {
                                    // Контент целиком за пределами surface —
                                    // фильтр не виден, его контент-пассы тоже
                                    // выбрасываются (viewport-cull).
                                    cull_invisible_level!(plan_mark);
                                    current_level -= 1;
                                    continue;
                                }
                                let full = sx0 == 0
                                    && sy0 == 0
                                    && sx1 >= surface_w
                                    && sy1 >= surface_h;
                                (
                                    (!full).then_some(DeviceScissor {
                                        x: sx0,
                                        y: sy0,
                                        width: sx1 - sx0,
                                        height: sy1 - sy0,
                                    }),
                                    Some((ix0, iy0, ix1, iy1)),
                                )
                            }
                        };
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        // bbox-офскрин блюра (BUG-405 срез 24): scratch
                        // H-прохода — размером со scissor, выровненный вверх
                        // до 64 px, а не во всю цель рендера. Пиксельная
                        // эквивалентность: слитый пасс читает офскрин только
                        // внутри scissor-а, а все его выборки по вертикали для
                        // этих пикселей лежат внутри региона — scissor уже
                        // раздут на радиус ядра, а строки выше/ниже bbox-а
                        // контента прозрачны (горизонтальный проход по
                        // вертикали не размазывает), то есть край региона
                        // отдаёт тот же ноль, что прежде давал полный Clear.
                        let region = match (&scissor, bbox_filter_disabled()) {
                            (Some(s), false) => {
                                let rw = s.width.div_ceil(64) * 64;
                                let rh = s.height.div_ceil(64) * 64;
                                // Регион ≈ вся цель — выигрыша нет, остаёмся
                                // на прежнем пути.
                                if rw >= surface_w && rh >= surface_h {
                                    None
                                } else {
                                    let rect = [s.x, s.y, rw, rh];
                                    let src_v_start = composite_vertices.len() as u32;
                                    push_region_src_quad(
                                        &mut composite_vertices,
                                        rect,
                                        surface_w as f32,
                                        surface_h as f32,
                                        1.0,
                                    );
                                    let dst_v_start = composite_vertices.len() as u32;
                                    push_region_dst_quad(
                                        &mut composite_vertices,
                                        rect,
                                        surface_w as f32,
                                        surface_h as f32,
                                        1.0,
                                    );
                                    Some(FilterRegion { rect, src_v_start, dst_v_start })
                                }
                            }
                            _ => None,
                        };
                        render_plan.push(RenderPlanItem::FilterComposite(FilterCompositePlan {
                            from_level: current_level,
                            filters,
                            comp_v_start,
                            region,
                            scissor,
                        }));
                        current_level -= 1;
                        // Композит фильтра красит родителя в пределах
                        // раздутого bbox — учесть в границах родителя.
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            match parent_rect {
                                Some((rx0, ry0, rx1, ry1)) => lb.add_rect(rx0, ry0, rx1, ry1),
                                None => *lb = LevelBounds::Unbounded,
                            }
                        }
                    }
                }
                // CSS Filter Effects L1 §2 — backdrop-filter.
                // Opens a new offscreen level for the element's own content.
                DisplayCommand::PushBackdropFilter { filters, bounds } => {
                    flush_batch!();
                    // `bounds` приходят в page-пространстве, как и всякий rect
                    // дисплей-листа: без scroll-offset-а и без накопленного
                    // `PushTransform` (в живом окне это сдвиг страницы под
                    // хром, `toolbar::CHROME_H`). Родительский слой, из
                    // которого копируется backdrop и в который он вблитывается,
                    // хранит уже трансформированный контент — значит и регион,
                    // и квад должны быть в screen-пространстве, ровно как
                    // `clip_stack` (BUG-276) и квады масок (BUG-277 срез 6).
                    let scrolled = translate_rect(*bounds, dx, dy);
                    let in_screen = apply_transform_to_clip(scrolled, transform_stack.last());
                    backdrop_filter_stack.push((filters.clone(), in_screen));
                    current_level += 1;
                    while level_first.len() <= current_level {
                        level_first.push(true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                DisplayCommand::PopBackdropFilter => {
                    if let Some((filters, bounds)) = backdrop_filter_stack.pop() {
                        flush_batch!();
                        let comp_v_start = composite_vertices.len() as u32;
                        push_composite_quad(&mut composite_vertices, 1.0);
                        // bbox-офскрины backdrop: рабочая область = bounds +
                        // радиус ядра блюра (формула BLUR_SHADER_SRC:
                        // min(ceil(3σ),32) + 2 запас), клип по родителю,
                        // ширина/высота выровнены вверх до 64 px — чтобы
                        // texture_pool стабильно попадал при движении bounds.
                        // Пиксельная эквивалентность: blit читает только
                        // bounds, а все выборки блюра для пикселей bounds
                        // лежат внутри региона (та же математика, что у
                        // bbox-scissor п.16).
                        let region: Option<[u32; 4]> = if bbox_backdrop_disabled() {
                            None
                        } else {
                            let blur_pad = filters
                                .iter()
                                .find_map(|f| match f {
                                    FilterFn::Blur(s) if *s > 0.0 => {
                                        Some((3.0 * *s).ceil().min(32.0))
                                    }
                                    _ => None,
                                })
                                .unwrap_or(0.0)
                                + 2.0;
                            let pad_css = blur_pad / dpr_f32;
                            let rx0 = (((bounds.x - pad_css) * dpr_f32).floor().max(0.0) as u32)
                                .min(surface_w);
                            let ry0 = (((bounds.y - pad_css) * dpr_f32).floor().max(0.0) as u32)
                                .min(surface_h);
                            let rx1 = (((bounds.x + bounds.width + pad_css) * dpr_f32)
                                .ceil()
                                .max(0.0) as u32)
                                .min(surface_w);
                            let ry1 = (((bounds.y + bounds.height + pad_css) * dpr_f32)
                                .ceil()
                                .max(0.0) as u32)
                                .min(surface_h);
                            if rx1 <= rx0 || ry1 <= ry0 {
                                // Элемент целиком вне surface — blit невидим,
                                // но пассы должны отработать как раньше
                                // (полноразмерный фолбэк, нулевой регион
                                // ломал бы копию/кэш).
                                None
                            } else {
                                let rw = (rx1 - rx0).div_ceil(64) * 64;
                                let rh = (ry1 - ry0).div_ceil(64) * 64;
                                // Регион ≈ весь родитель — выигрыша нет,
                                // остаёмся на старом пути (и его кэш-хэшах).
                                if rw >= surface_w && rh >= surface_h {
                                    None
                                } else {
                                    Some([rx0, ry0, rw, rh])
                                }
                            }
                        };
                        let bounds_v_start = composite_vertices.len() as u32;
                        push_bounded_quad(
                            &mut composite_vertices,
                            bounds,
                            surface_w as f32,
                            surface_h as f32,
                            dpr_f32,
                            1.0,
                            region,
                        );
                        let ordinal = backdrop_ordinal;
                        backdrop_ordinal += 1;
                        render_plan.push(RenderPlanItem::BackdropFilterComposite(
                            BackdropFilterCompositePlan {
                                from_level: current_level,
                                filters,
                                comp_v_start,
                                bounds_v_start,
                                ordinal,
                                region,
                            },
                        ));
                        current_level -= 1;
                        // bbox-scissor v1: backdrop-композит не отслеживаем —
                        // родитель помечается неограниченным (безопасный фолбэк).
                        if current_level > 0
                            && let Some(lb) = level_bounds.get_mut(current_level)
                        {
                            *lb = LevelBounds::Unbounded;
                        }
                    }
                }
                DisplayCommand::DrawSvgPath { vertices, color } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    for [x, y] in vertices {
                        fill_vertices.push(FillVertex {
                            pos: [x + dx, y + dy],
                            z: 0.0,
                            color: c,
                        });
                    }
                    if let Some(m) = transform_stack.last() {
                        apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                    }
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // BUG-247 / BUG-173: the GPU pipeline has no native path fill, so
                // tessellate the nonzero outline contours into a triangle soup —
                // identical to the old `DrawSvgPath` fill the emitter produced.
                DisplayCommand::DrawSvgFill { contours, color } => {
                    let _t_arm = sub_timer(cmd_log, &SVG_SUB.arm);
                    let t_sciss = cmd_log.then(std::time::Instant::now);
                    let scissor_ok = sync_scissor!();
                    if let Some(t0) = t_sciss {
                        sub_add(&SVG_SUB.sciss, t0);
                    }
                    if !scissor_ok {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    let shape = svg_shape_verts(
                        &mut self.svg_shape_cache,
                        &mut self.coverage_cache,
                        self.svg_shape_cache_enabled,
                        self.coverage_cache_enabled,
                        contours,
                        None,
                        dx,
                        dy,
                        transform_stack.last(),
                        dpr_f32,
                        cmd_log,
                    );
                    emit_svg_shape(&mut fill_vertices, &shape, c, cmd_log);
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // BUG-247: the GPU pipeline has no native stroker, so tessellate
                // the stroke contours into a triangle soup — identical to the old
                // `DrawSvgPath` stroke the emitter produced.
                DisplayCommand::DrawSvgStroke { contours, color, params } => {
                    let _t_arm = sub_timer(cmd_log, &SVG_SUB.arm);
                    let t_sciss = cmd_log.then(std::time::Instant::now);
                    let scissor_ok = sync_scissor!();
                    if let Some(t0) = t_sciss {
                        sub_add(&SVG_SUB.sciss, t0);
                    }
                    if !scissor_ok {
                        continue;
                    }
                    let v_start = fill_vertices.len() as u32;
                    let c = apply_alpha_to_color(color_to_array(color), 1.0_f32);
                    let shape = svg_shape_verts(
                        &mut self.svg_shape_cache,
                        &mut self.coverage_cache,
                        self.svg_shape_cache_enabled,
                        self.coverage_cache_enabled,
                        contours,
                        Some(params),
                        dx,
                        dy,
                        transform_stack.last(),
                        dpr_f32,
                        cmd_log,
                    );
                    emit_svg_shape(&mut fill_vertices, &shape, c, cmd_log);
                    let v_count = fill_vertices.len() as u32 - v_start;
                    if v_count > 0 {
                        draw_ops.push(DrawOp::Fill { v_start, v_count });
                    }
                }
                // CSS Positioning L3 §6.3 — position:sticky. Bound is the nearest
                // scrolling ancestor's scrollport (BUG-336: previously always the
                // full viewport, so a sticky element nested in an overflow:auto/
                // scroll container just scrolled away with it instead of pinning).
                DisplayCommand::BeginStickyLayer { flow_rect, top, bottom, left, right } => {
                    if !is_overlay {
                        let bound = sticky_bound(&clip_stack, &transform_stack, viewport_css_w, viewport_css_h);
                        let sdy = sticky_offset_dy(flow_rect, *top, *bottom, scroll_y, bound);
                        let sdx = sticky_offset_dx(flow_rect, *left, *right, scroll_x, bound);
                        sticky_stack.push((sdy, sdx));
                    }
                }
                DisplayCommand::EndStickyLayer => {
                    if !is_overlay {
                        sticky_stack.pop();
                    }
                }
                // CSS Positioning L3 §6.1 — position:fixed partition markers
                // (ADR-016 M3.2.1c). No draw-time offset: fixed content is already
                // at viewport-fixed coords, so these are pure no-ops here.
                DisplayCommand::BeginFixedLayer | DisplayCommand::EndFixedLayer => {}
                // CSS Masking L1 §5 — PushMaskLayer: open an offscreen layer for mask content.
                // The caller (emit_box) is responsible for ensuring the element content is
                // isolated in the parent layer (e.g. via PushOpacity) before calling this.
                // Mask content renders to the new level; PopMaskLayer applies it to the parent.
                DisplayCommand::PushMaskLayer { rect, mode } => {
                    flush_batch!();
                    mask_layer_stack.push((*rect, *mode));
                    current_level += 1;
                    if level_first.len() <= current_level {
                        level_first.resize(current_level + 1, true);
                    }
                    level_first[current_level] = true;
                    while level_bounds.len() <= current_level {
                        level_bounds.push(LevelBounds::Empty);
                    }
                    level_bounds[current_level] = LevelBounds::Empty;
                }
                // CSS Masking L1 §5 — PopMaskLayer: composite mask layer onto parent.
                // Algorithm:
                //   1. Copy parent layer → scratch (scratch preserves element content).
                //   2. Render pass (REPLACE blend): scratch × mask_value → parent at element rect.
                //      This replaces parent content in the element rect with the masked version.
                DisplayCommand::PopMaskLayer => {
                    flush_batch!();
                    let Some((rect, mode)) = mask_layer_stack.pop() else { continue };
                    let ml_v_start = mask_layer_vertices.len() as u32;
                    // Квад по площади элемента, уже прогнанный через накопленный
                    // `PushTransform` (BUG-277 срез 6 — контент рисуется
                    // трансформированным). UV = pos / viewport_css: и t_content
                    // (scratch), и t_mask — полноразмерные слои, записанные через
                    // тот же `u.viewport`-маппинг, поэтому CSS-px позиция p
                    // читается по uv = p/viewport (а не p/surface — это device px).
                    let scrolled = translate_rect(rect, dx, dy);
                    let quad = transformed_grad_quad(scrolled, transform_stack.last());
                    mask_layer_vertices.extend(quad.iter().map(|v| MaskVertex {
                        pos: v.pos,
                        uv_mask: [v.pos[0] / viewport_w, v.pos[1] / viewport_h],
                    }));
                    let ml_v_end = mask_layer_vertices.len() as u32;
                    render_plan.push(RenderPlanItem::MaskLayerComposite(MaskLayerCompositePlan {
                        from_level: current_level,
                        mode,
                        ml_v_start,
                        ml_v_end,
                    }));
                    current_level -= 1;
                    // bbox-scissor v1: mask-layer-композит не отслеживаем —
                    // родитель помечается неограниченным (безопасный фолбэк).
                    if current_level > 0
                        && let Some(lb) = level_bounds.get_mut(current_level)
                    {
                        *lb = LevelBounds::Unbounded;
                    }
                }
                // Scrollbar track + thumb: two fill quads drawn with the current
                // clip/transform stack (parent's, NOT scroll layer's).
                // Colors from `scrollbar-color` (CSS Scrollbars L1 §3).
                DisplayCommand::DrawScrollbar { track_rect, thumb_rect, track_color, thumb_color, .. } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    for (rect, color) in &[(*track_rect, *track_color), (*thumb_rect, *thumb_color)] {
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            translate_rect(*rect, dx, dy),
                            *color,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                    }
                }
                // DevTools box model overlay (7E.3): four semi-transparent layers
                // drawn outside-in. Uses the same fill pipeline as FillRect.
                DisplayCommand::BoxModelOverlay { margin, border, padding, content } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    // Standard DevTools palette (Chrome-matching), ~50% alpha.
                    const MARGIN_COLOR:  [f32; 4] = [0.965, 0.699, 0.420, 0.5]; // #f6b26b
                    const BORDER_COLOR:  [f32; 4] = [1.000, 0.898, 0.600, 0.5]; // #ffe599
                    const PADDING_COLOR: [f32; 4] = [0.576, 0.769, 0.490, 0.5]; // #93c47d
                    const CONTENT_COLOR: [f32; 4] = [0.435, 0.659, 0.863, 0.5]; // #6fa8dc

                    let boxes: &[(Rect, [f32; 4])] = &[
                        (*margin,  MARGIN_COLOR),
                        (*border,  BORDER_COLOR),
                        (*padding, PADDING_COLOR),
                        (*content, CONTENT_COLOR),
                    ];
                    for (rect, color) in boxes {
                        if rect.width <= 0.0 || rect.height <= 0.0 {
                            continue;
                        }
                        let v_start = fill_vertices.len() as u32;
                        push_fill_quad(
                            &mut fill_vertices,
                            translate_rect(*rect, dx, dy),
                            *color,
                        );
                        if let Some(m) = transform_stack.last() {
                            apply_affine_to_verts(&mut fill_vertices[v_start as usize..], m);
                        }
                        let v_count = fill_vertices.len() as u32 - v_start;
                        if v_count > 0 {
                            draw_ops.push(DrawOp::Fill { v_start, v_count });
                        }
                    }
                }
                DisplayCommand::PageBreak => {
                    // No-op in on-screen rendering; only meaningful in render_print_pages().
                }
                // CSS Images L4 §4 — cross-fade(A, B, p) two-texture blend.
                // Both `src_a` and `src_b` must already be registered via
                // `register_image`; if either is missing the command is a no-op
                // (matches DrawBackgroundImage convention for unregistered URLs).
                // The quad covers `dest` after scroll translation; both textures
                // sample at the full UV range [0,1]×[0,1] (CSS Images L4 §4.1).
                DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                    if !sync_scissor!() {
                        continue;
                    }
                    // Look up both GpuImage entries. Use intrinsic-size key
                    // directly — cross-fade stretches each image to `dest`
                    // through UV sampling, so no CPU resize is needed (object-fit
                    // does not apply to cross-fade per CSS Images L4 §4.1).
                    let Some(gpu_a) = self.images.get(src_a) else { continue };
                    let Some(gpu_b) = self.images.get(src_b) else { continue };
                    let scrolled = translate_rect(*dest, dx, dy);
                    if scrolled.width <= 0.0 || scrolled.height <= 0.0 {
                        continue;
                    }
                    let clamped = progress.clamp(0.0, 1.0);

                    // Per-quad progress uniform (std140-padded to 16 bytes).
                    let params: [f32; 4] = [clamped, 0.0, 0.0, 0.0];
                    let cf_idx = cross_fade_bind_groups.len();
                    let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("cross-fade-ubuf-{cf_idx}")),
                        size: std::mem::size_of_val(&params) as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&ubuf, 0, as_bytes(&params));
                    let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("cross-fade-bg-{cf_idx}")),
                        layout: &self.cross_fade_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&gpu_a.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&gpu_b.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: ubuf.as_entire_binding(),
                            },
                        ],
                    });
                    cross_fade_bind_groups.push(bg);

                    let v_start = cross_fade_vertices.len() as u32;
                    push_cross_fade_quad(&mut cross_fade_vertices, scrolled);
                    // Тот же накопленный `PushTransform`, что у остальных
                    // image-путей (BUG-277 срез 15).
                    if let Some(m) = transform_stack.last()
                        && !img_xform_disabled()
                    {
                        apply_affine_to_verts(&mut cross_fade_vertices[v_start as usize..], m);
                    }
                    let v_count = cross_fade_vertices.len() as u32 - v_start;
                    draw_ops.push(DrawOp::CrossFade {
                        v_start,
                        v_count,
                        cf_batch_idx: cf_idx as u32,
                    });
                }
            }
        }
        // BUG-405 срез 41: блит retained-текстуры стабильного хвоста overlay
        // (`Renderer::overlay_cache_step`) — ПОСЛЕ живого overlay-префикса
        // (только что дорисован циклом выше), на месте, где рисовался бы
        // остаток полного overlay-списка: относительный порядок overlay
        // блитом не меняется, только его исполнение — часть команд рисуется
        // живьём, часть берётся готовыми пикселями из текстуры.
        //
        // `sync_scissor!()` обязателен: живой префикс мог закончиться
        // изнутри клипа (`PopClip` — последняя его команда), и та сама по
        // себе не эмитит корректирующий `SetScissor` — она лишь возвращает
        // `clip_stack` в прежнее состояние, а ФАКТИЧЕСКИЙ scissor GPU
        // остаётся тем, что выставила ПОСЛЕДНЯЯ реальная команда ВНУТРИ
        // клипа, пока его не пересчитает следующая команда. Инъекция блита
        // в обход обычного командного цикла эту синхронизацию не получает
        // даром — без неё блит хвоста наследует чужой (более узкий) scissor
        // и обрезается по нему.
        if let Some(blit) = self.pending_overlay_blit.take() {
            sync_scissor!();
            let image_batch_idx = image_bind_groups.len() as u32;
            image_bind_groups.push(blit.bind_group);
            for (rect, uv0, uv1) in blit.quads {
                let v_start = image_vertices.len() as u32;
                push_image_quad(&mut image_vertices, rect, uv0, uv1, 1.0);
                draw_ops.push(DrawOp::Image { v_start, v_count: 6, image_batch_idx });
            }
        }
        flush_batch!();
        let _ = (batch_start, current_scissor); // terminal flush — values not needed after
        // BUG-771 (диагностика, `LUMEN_TEXT_SIG=1`): подпись текста overlay-а и
        // атласа глифов на кадре. Симптом бага — «те же глифы на тех же местах
        // рисуются плотнее» — распадается ровно на два случая, и эти две
        // подписи их разделяют: расходится `verts` → дело в квадах (геометрия
        // или UV), расходится только `atlas` → дело в содержимом атласа.
        if text_sig_level() > 0 {
            use std::hash::Hasher;
            let v0 = overlay_text_v0.unwrap_or(text_vertices.len()).min(text_vertices.len());
            let mut h_pos = std::collections::hash_map::DefaultHasher::new();
            let mut h_uv = std::collections::hash_map::DefaultHasher::new();
            let mut h_col = std::collections::hash_map::DefaultHasher::new();
            for v in &text_vertices[v0..] {
                for f in [v.pos[0], v.pos[1], v.z] {
                    h_pos.write_u32(f.to_bits());
                }
                for f in v.uv {
                    h_uv.write_u32(f.to_bits());
                }
                for f in v.color {
                    h_col.write_u32(f.to_bits());
                }
            }
            let mut ha = std::collections::hash_map::DefaultHasher::new();
            ha.write(self.atlas.pixels());
            let mode_name = match mode {
                RenderPassMode::Band { .. } => "band",
                RenderPassMode::Compose => "compose",
                RenderPassMode::Normal { .. } => "normal",
                RenderPassMode::OverlayCache { .. } => "overlay-cache",
            };
            eprintln!(
                "[frame:wgpu] text-sig {mode_name} n={} pos={:016x} uv={:016x} col={:016x} atlas={:016x}",
                text_vertices.len() - v0,
                h_pos.finish(),
                h_uv.finish(),
                h_col.finish(),
                ha.finish(),
            );
            if text_sig_level() >= 2 {
                for (i, v) in text_vertices[v0..].iter().step_by(6).enumerate() {
                    eprintln!(
                        "[frame:wgpu] text-vert {mode_name} {i} pos={:.4},{:.4} uv={:.6},{:.6} col={:.3},{:.3},{:.3},{:.3}",
                        v.pos[0], v.pos[1], v.uv[0], v.uv[1],
                        v.color[0], v.color[1], v.color[2], v.color[3],
                    );
                }
            }
        }
        if let Some((name, t0)) = probe_prev.take() {
            let e = probe.entry(name).or_default();
            e.0 += t0.elapsed();
            e.1 += 1;
        }
        if cmd_log {
            let mut rows: Vec<_> = probe.iter().collect();
            rows.sort_by_key(|(_, (d, _, _, _))| std::cmp::Reverse(*d));
            let top: Vec<String> = rows
                .iter()
                .take(8)
                .map(|(name, (d, n, c, cd))| {
                    format!(
                        "{name} {:.2}ms/{n}(cull {c}, bbox {:.2}ms)",
                        d.as_secs_f64() * 1e3,
                        cd.as_secs_f64() * 1e3
                    )
                })
                .collect();
            eprintln!("[frame:wgpu]   collect-top: {}", top.join(", "));
            if SVG_SUB.calls.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                eprintln!("[frame:wgpu]   {}", SVG_SUB.line());
            }
            if TEXT_SUB.cmds.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                eprintln!("[frame:wgpu]   {}", TEXT_SUB.line());
            }
        }
        let t_after_collect = t_frame0.elapsed();

        // ── Atlas upload (если изменился) ─────────────────────────────────
        // BUG-405 срез 11: заливаются только строки, изменившиеся с прошлой
        // заливки. Новый глиф трогает десятки строк из 1024, а отправлялась
        // вся текстура целиком (1 МиБ на каждом кадре с новым глифом).
        let mut atlas_frame_bytes = 0usize;
        let mut atlas_frame_rows = 0u32;
        if self.atlas.dirty() {
            let partial = self.atlas_partial_upload_enabled && !atlas_partial_upload_disabled();
            let (y0, y1) = match self.atlas.dirty_rows() {
                Some(rows) if partial => rows,
                // `dirty()` без диапазона строк невозможен, но полагаться на
                // это нельзя: без диапазона заливаем всё, как до среза.
                _ => (0, self.atlas.height()),
            };
            let row = self.atlas.width() as usize;
            let bytes = &self.atlas.pixels()[y0 as usize * row..y1 as usize * row];
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.atlas.width()),
                    rows_per_image: Some(y1 - y0),
                },
                wgpu::Extent3d {
                    width: self.atlas.width(),
                    height: y1 - y0,
                    depth_or_array_layers: 1,
                },
            );
            self.atlas_bytes_uploaded += bytes.len() as u64;
            self.atlas_uploads += 1;
            atlas_frame_bytes = bytes.len();
            atlas_frame_rows = y1 - y0;
            self.atlas.mark_clean();
        }
        // BUG-405 срез 10: разбивка фазы `prep` (атлас / uniform-слоты /
        // вершинные буферы / bind-группы градиентов / offscreen-текстуры).
        // До неё `prep` была одним числом — вторая по величине статья прогона
        // без единой подстатьи, ровно как `collect` до среза 9.
        let t_after_atlas = t_frame0.elapsed();

        // ── Uniforms ──────────────────────────────────────────────────────
        // Shader делит pos на viewport, чтобы получить clip-space. Surface
        // сконфигурирован в physical pixels, но shader считает в CSS px:
        // viewport = config / scale_factor → 1 CSS px = scale_factor device px.
        // scale_factor=1 — поведение pre-DPR (1:1, обычный 1080p); =2 — 4K с
        // 200% scaling, 16-px CSS текст рендерится на 32 device px.
        // f32 cast терпит небольшую потерю точности — DPR редко > 4.0.
        //
        // BUG-405 срез 4: в слоте 0 лежит тот же viewport и «клипа нет»;
        // слоты 1.. заполнены шейдерными скруглёнными клипами этого кадра
        // (`viewport` у всех одинаков — меняется только контур).
        let mut t_uni_grow = std::time::Duration::ZERO;
        let mut t_uni_build = std::time::Duration::ZERO;
        let mut t_uni_write = std::time::Duration::ZERO;
        self.write_uniform_slots(
            &clip_slots,
            &mut t_uni_grow,
            &mut t_uni_build,
            &mut t_uni_write,
        );
        self.rrect_clip_levels += level_rrect_clips;
        // BUG-405 срез 5: гейт склейки — число склеенных разрезов и итоговое
        // число пассов кадра (элемент плана = пасс).
        self.cull_merges += merged_cull_splits as u64;
        self.shadow_draws += shadow_draws as u64;
        self.plan_passes += render_plan.len() as u64;
        let t_after_uniforms = t_frame0.elapsed();

        // ── Vertex buffers ────────────────────────────────────────────────
        // BUG-405 срез 10: цена создания буфера и цена записи вершин считаются
        // порознь — «дорого писать 250 KiB» и «дорого просить у драйвера новый
        // ресурс» лечатся противоположно (первое ничем, второе — переиспользо-
        // ванием буфера между кадрами).
        let mut t_vbuf_create = std::time::Duration::ZERO;
        let mut t_vbuf_write = std::time::Duration::ZERO;
        let fill_vbuf = self.upload_vertex_buffer(
            "fill-vbuf",
            &fill_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let circle_vbuf = self.upload_vertex_buffer(
            "circle-vbuf",
            &circle_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let rrect_vbuf = self.upload_vertex_buffer(
            "rrect-vbuf",
            &rrect_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let shadow_vbuf = self.upload_vertex_buffer(
            "shadow-vbuf",
            &shadow_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let text_vbuf = self.upload_vertex_buffer(
            "text-vbuf",
            &text_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let image_vbuf = self.upload_vertex_buffer(
            "image-vbuf",
            &image_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let comp_vbuf = self.upload_vertex_buffer(
            "comp-vbuf",
            &composite_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let mask_vbuf = self.upload_vertex_buffer(
            "mask-vbuf",
            &mask_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let mask_layer_vbuf = self.upload_vertex_buffer(
            "mask-layer-vbuf",
            &mask_layer_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let rrect_clip_vbuf = self.upload_vertex_buffer(
            "rrect-clip-vbuf",
            &rrect_clip_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let path_clip_vbuf = self.upload_vertex_buffer(
            "path-clip-vbuf",
            &path_clip_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let grad_vbuf = self.upload_vertex_buffer(
            "grad-vbuf",
            &grad_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let cross_fade_vbuf = self.upload_vertex_buffer(
            "cross-fade-vbuf",
            &cross_fade_vertices,
            &mut t_vbuf_create,
            &mut t_vbuf_write,
        );
        let t_after_vbufs = t_frame0.elapsed();
        // BUG-405 срез 10: сколько буферов родилось за кадр и сколько в них
        // байт. Пустые категории буфера не создают, поэтому считаем ровно те,
        // что выше ушли в `Some`.
        let vbuf_sizes: [usize; 13] = [
            std::mem::size_of_val(fill_vertices.as_slice()),
            std::mem::size_of_val(circle_vertices.as_slice()),
            std::mem::size_of_val(rrect_vertices.as_slice()),
            std::mem::size_of_val(shadow_vertices.as_slice()),
            std::mem::size_of_val(text_vertices.as_slice()),
            std::mem::size_of_val(image_vertices.as_slice()),
            std::mem::size_of_val(composite_vertices.as_slice()),
            std::mem::size_of_val(mask_vertices.as_slice()),
            std::mem::size_of_val(mask_layer_vertices.as_slice()),
            std::mem::size_of_val(rrect_clip_vertices.as_slice()),
            std::mem::size_of_val(path_clip_vertices.as_slice()),
            std::mem::size_of_val(grad_vertices.as_slice()),
            std::mem::size_of_val(cross_fade_vertices.as_slice()),
        ];
        let vbuf_count = vbuf_sizes.iter().filter(|s| **s > 0).count();
        let vbuf_bytes: usize = vbuf_sizes.iter().sum();
        // One storage buffer + bind group per gradient draw call (same pattern as image batches).
        let grad_bind_groups: Vec<wgpu::BindGroup> = grad_params
            .iter()
            .enumerate()
            .map(|(i, params)| {
                let sbuf = write_grad_buffer(&self.device, &self.queue, params, &format!("grad-sbuf-{i}"));
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("grad-bg-{i}")),
                    layout: &self.gradient_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: sbuf.as_entire_binding(),
                    }],
                })
            })
            .collect();
        let t_after_grad_bg = t_frame0.elapsed();

        // ── Off-screen textures ───────────────────────────────────────────
        // Blend composites (mode != Normal) also need from_level offscreen layers.
        let max_level = render_plan.iter().fold(0usize, |m, item| match item {
            RenderPlanItem::Draw(b) => m.max(b.target_level),
            RenderPlanItem::Composite(c) => m.max(c.from_level),
            RenderPlanItem::MaskComposite(c) => m.max(c.from_level),
            RenderPlanItem::FilterComposite(c) => m.max(c.from_level),
            RenderPlanItem::BackdropFilterComposite(c) => m.max(c.from_level),
            RenderPlanItem::MaskLayerComposite(c) => m.max(c.from_level),
            RenderPlanItem::RRectClipComposite(c) => m.max(c.from_level),
            RenderPlanItem::PathClipComposite(c) => m.max(c.from_level),
        });
        if max_level > 0 {
            self.ensure_layer_textures(max_level, surface_w, surface_h);
        }

        // CSS Masking L1 §4 — gradient mask temp textures ИЗ ПУЛА.
        // Раньше на каждый кадр на каждый MaskComposite с градиентом
        // создавалась свежая текстура размером с target и дропалась после
        // submit — 196 из 237 созданий за флинг-прогон (перепись п.23/24,
        // 1024×1800 ≈ 7.4 МБ каждая). Пред-захват до цикла (внутри цикла
        // живут заимствования &self), возврат в пул после submit.
        let grad_mask_count = render_plan
            .iter()
            .filter(|item| {
                matches!(item, RenderPlanItem::MaskComposite(c)
                    if c.mask_gradient.is_some()
                        && c.mask_src.as_ref().is_none_or(|src| !self.images.contains_key(src)))
            })
            .count();
        let mut temp_grad_layers: Vec<OffscreenLayer> = Vec::with_capacity(grad_mask_count);
        for _ in 0..grad_mask_count {
            let layer = self.create_layer_texture(surface_w, surface_h);
            temp_grad_layers.push(layer);
        }
        let mut temp_grad_next = 0usize;

        // ── Frame ─────────────────────────────────────────────────────────
        // Windowed: get the next swapchain image from the surface.
        // Headless: create a temporary RGBA8 RENDER_ATTACHMENT|COPY_SRC texture so
        //   render_to_image() can read it back after this call.
        let t_after_prep = t_frame0.elapsed();
        let windowed_frame: Option<wgpu::SurfaceTexture>;
        let headless_tex: Option<wgpu::Texture>;
        let frame_view: wgpu::TextureView;
        if let RenderPassMode::Band { view, .. } | RenderPassMode::OverlayCache { view, .. } =
            &mode
        {
            // Оффскрин-рендер полосы/overlay-кэша: цель задана вызывающим,
            // swapchain не трогаем (клон view — дешёвый Arc-хэндл wgpu).
            frame_view = view.clone();
            windowed_frame = None;
            headless_tex = None;
        } else if let Some(ref surface) = self.surface {
            let f = surface.get_current_texture()?;
            frame_view = f.texture.create_view(&wgpu::TextureViewDescriptor::default());
            windowed_frame = Some(f);
            headless_tex = None;
        } else {
            count_texture_created_labeled("headless-frame", surface_w, surface_h);
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("headless-frame"),
                size: wgpu::Extent3d {
                    width: surface_w,
                    height: surface_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            frame_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            windowed_frame = None;
            headless_tex = Some(tex);
        }
        // BUG-277 (срез 3): текстура кадра как ЧИТАЕМЫЙ backdrop для
        // `mix-blend-mode` на верхнем уровне (`Composite { from_level: 1 }`).
        // Сэмплировать её нельзя, но можно скопировать в scratch — нужен
        // `COPY_SRC`: у headless-текстуры он есть всегда, у swapchain — если
        // его отдал драйвер (см. `surface_usage` в конструкторе). В
        // `RenderPassMode::Band` доступен только view, поэтому `None` —
        // полосный рендер остаётся на старом alpha-over fallback.
        let surface_copy_src = self
            .config
            .as_ref()
            .is_some_and(|c| c.usage.contains(wgpu::TextureUsages::COPY_SRC));
        let frame_blend_dst: Option<&wgpu::Texture> = match (&windowed_frame, &headless_tex) {
            (Some(f), _) => surface_copy_src.then_some(&f.texture),
            (None, Some(t)) => Some(t),
            (None, None) => None,
        };
        let t_after_acquire = t_frame0.elapsed();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });

        // BUG-405 срез 4: активный слот скруглённого клипа. Состояние
        // энкодера, а не пасса: `DrawOp::SetClip` переставляет его в потоке
        // операций, и каждый следующий пасс продолжает с тем же значением.
        let mut clip_slot: u32 = 0;

        // BUG-405 срез 10: последнее выставленное состояние ТЕКУЩЕГО пасса.
        // Каждая операция рисования безусловно ставила пайплайн, обе
        // bind-группы и вершинный буфер — пять команд на операцию при ~83
        // операциях в пассе. Команды пасса стоят не там, где записаны, а в
        // `drop(pass)` (перепись среза 10: `end` — 33.4 мс из 35.7 мс фазы
        // `encode`, и цена растёт вместе с числом операций), поэтому повтор
        // уже выставленного значения — чистая цена.
        //
        // Сравнение по адресу: пайплайны лежат в полях `self`, bind-группы —
        // в векторах кадра, вершинные буферы — в локальных `Option`; ни один
        // из них внутри пасса не пересоздаётся, поэтому «тот же адрес» = «тот
        // же объект» (ABA внутри пасса невозможен).
        //
        // 0 / `u32::MAX` / `None` — «состояние неизвестно»; с них начинается
        // КАЖДЫЙ пасс: у свежего `RenderPass` ничего не выставлено, а scissor
        // равен всей цели.
        // Начальные значения не пишутся здесь намеренно: их выставляет
        // `run_draw_ops!` в начале КАЖДОГО пасса — начальное значение отсюда
        // всё равно не дожило бы до первого чтения.
        let elide_state = self.state_elision_enabled && !state_elision_disabled();
        let mut st_pipeline: usize;
        let mut st_bg0_off: u32;
        let mut st_bg1: usize;
        let mut st_vbuf: usize;
        let mut st_scissor: Option<DeviceScissor>;
        // Отложенный (ещё не отданный) диапазон вершин текущего состояния —
        // см. `bind_and_draw!`.
        let mut st_pending: Option<(u32, u32)>;
        let mut st_elided: u64 = 0;
        let mut st_merged: u64 = 0;
        // BUG-405 срез 14: сколько команд пасса РЕАЛЬНО отправлено, по видам
        // (пайплайн / bg0 / bg1 / вершинный буфер / scissor / draw). Счётчик
        // отсева отвечает на «сколько сэкономлено», а цену `drop(pass)` можно
        // разложить только по фактически отправленным командам.
        let mut n_cmd: [u64; 6] = [0; 6];

        /// Отдаёт отложенный draw, если он есть (BUG-405 срез 10).
        ///
        /// Обязателен перед ЛЮБОЙ командой, меняющей состояние пасса, и в
        /// конце пасса: отложенные примитивы записаны под прежним состоянием.
        macro_rules! flush_pending_draw {
            ($pass:ident) => {{
                if let Some((s, e)) = st_pending.take() {
                    n_cmd[5] += 1;
                    $pass.draw(s..e, 0..1);
                }
            }};
        }

        /// Выставляет состояние операции рисования и рисует, пропуская те
        /// команды, где уже стоит нужное значение, и склеивая соседние
        /// диапазоны одного состояния в один `draw` (BUG-405 срез 10).
        ///
        /// Склейка точна, а не приблизительна: `draw(a..c)` подаёт те же
        /// примитивы в том же порядке, что `draw(a..b)` + `draw(b..c)`, если
        /// между ними не менялось состояние, — значит и пиксели те же.
        macro_rules! bind_and_draw {
            ($pass:ident, $pipeline:expr, $bg1:expr, $vb:expr, $v_start:expr, $v_count:expr) => {{
                let pipeline: &wgpu::RenderPipeline = $pipeline;
                let pid = std::ptr::from_ref(pipeline) as usize;
                let bg1: Option<&wgpu::BindGroup> = $bg1;
                let bid = bg1.map_or(0, |b| std::ptr::from_ref(b) as usize);
                let vb: &wgpu::Buffer = $vb;
                let vid = std::ptr::from_ref(vb) as usize;
                let off = clip_slot * UNIFORM_SLOT_STRIDE as u32;

                let need_pipeline = !elide_state || st_pipeline != pid;
                if need_pipeline {
                    // Смена пайплайна обесценивает bind-группы, чей layout у
                    // нового пайплайна другой (правило совместимости WebGPU).
                    // Группа 0 у всех пайплайнов одна и та же (`uniform_bgl`)
                    // и смену переживает, а группа 1 у text/image/gradient
                    // разная — её знание сбрасывается.
                    st_bg1 = 0;
                }
                let need_bg0 = !elide_state || st_bg0_off != off;
                let need_bg1 = bg1.is_some() && (!elide_state || st_bg1 != bid);
                let need_vbuf = !elide_state || st_vbuf != vid;

                let v_start: u32 = $v_start;
                let v_end: u32 = v_start + $v_count;
                if need_pipeline || need_bg0 || need_bg1 || need_vbuf {
                    flush_pending_draw!($pass);
                    if need_pipeline {
                        st_pipeline = pid;
                        n_cmd[0] += 1;
                        $pass.set_pipeline(pipeline);
                    } else {
                        st_elided += 1;
                    }
                    if need_bg0 {
                        st_bg0_off = off;
                        n_cmd[1] += 1;
                        $pass.set_bind_group(0, &self.uniform_bind_group, &[off]);
                    } else {
                        st_elided += 1;
                    }
                    if let Some(bg) = bg1 {
                        if need_bg1 {
                            st_bg1 = bid;
                            n_cmd[2] += 1;
                            $pass.set_bind_group(1, bg, &[]);
                        } else {
                            st_elided += 1;
                        }
                    }
                    if need_vbuf {
                        st_vbuf = vid;
                        n_cmd[3] += 1;
                        $pass.set_vertex_buffer(0, vb.slice(..));
                    } else {
                        st_elided += 1;
                    }
                    st_pending = Some((v_start, v_end));
                } else {
                    st_elided += 3 + u64::from(bg1.is_some());
                    match st_pending {
                        Some((s, e)) if e == v_start => {
                            st_pending = Some((s, v_end));
                            st_merged += 1;
                        }
                        _ => {
                            flush_pending_draw!($pass);
                            st_pending = Some((v_start, v_end));
                        }
                    }
                }
            }};
        }

        macro_rules! run_draw_ops {
            ($pass:ident, $start:expr, $end:expr) => {
                // BUG-405 срез 10: у свежего пасса ничего не выставлено, а
                // scissor равен всей цели — знание предыдущего пасса здесь
                // недействительно.
                st_pipeline = 0;
                st_bg0_off = u32::MAX;
                st_bg1 = 0;
                st_vbuf = 0;
                st_scissor = None;
                st_pending = None;
                for op in &draw_ops[$start..$end] {
                    match op {
                        // BUG-405 срез 4: слот клипа — состояние пасса, как и
                        // scissor; сама операция ничего не рисует.
                        DrawOp::SetClip(slot) => {
                            clip_slot = *slot;
                        }
                        DrawOp::SetScissor(s) => {
                            if elide_state && st_scissor == Some(*s) {
                                st_elided += 1;
                            } else {
                                // Отложенные примитивы записаны под прежним
                                // scissor — отдать их до смены.
                                flush_pending_draw!($pass);
                                st_scissor = Some(*s);
                                n_cmd[4] += 1;
                                if s.is_empty() {
                                    $pass.set_scissor_rect(0, 0, 1.min(surface_w), 1.min(surface_h));
                                } else {
                                    $pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                                }
                            }
                        }
                        DrawOp::Fill { v_start, v_count } => {
                            if let Some(vb) = &fill_vbuf {
                                bind_and_draw!($pass, self.fill_pipeline(), None, vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::Circle { v_start, v_count } => {
                            if let Some(vb) = &circle_vbuf {
                                bind_and_draw!($pass, self.circle_pipeline(), None, vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::RRect { v_start, v_count } => {
                            if let Some(vb) = &rrect_vbuf {
                                bind_and_draw!($pass, self.rrect_pipeline(), None, vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::Shadow { v_start, v_count } => {
                            if let Some(vb) = &shadow_vbuf {
                                bind_and_draw!($pass, self.shadow_pipeline(), None, vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::Text { v_start, v_count } => {
                            if let Some(vb) = &text_vbuf {
                                bind_and_draw!($pass, self.text_pipeline(), Some(&self.atlas_bind_group), vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::Image { v_start, v_count, image_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &image_vbuf,
                                image_bind_groups.get(*image_batch_idx as usize),
                            ) {
                                bind_and_draw!($pass, self.image_pipeline(), Some(bind_group), vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::Gradient { v_start, v_count, grad_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &grad_vbuf,
                                grad_bind_groups.get(*grad_batch_idx as usize),
                            ) {
                                bind_and_draw!($pass, self.gradient_pipeline(), Some(bind_group), vb, *v_start, *v_count);
                            }
                        }
                        DrawOp::CrossFade { v_start, v_count, cf_batch_idx } => {
                            if let (Some(vb), Some(bind_group)) = (
                                &cross_fade_vbuf,
                                cross_fade_bind_groups.get(*cf_batch_idx as usize),
                            ) {
                                bind_and_draw!($pass, self.cross_fade_pipeline(), Some(bind_group), vb, *v_start, *v_count);
                            }
                        }
                    }
                }
                // Хвост пасса: последний отложенный диапазон.
                flush_pending_draw!($pass);
            };
        }

        // Per-pass filter param buffers — one per filter/backdrop-filter render pass.
        // Using a single shared buffer caused all passes to see the last write_buffer
        // value (wgpu batches all write_buffer calls before any encoder commands run).
        let mut filter_param_bufs: Vec<wgpu::Buffer> = Vec::new();
        // BUG-277 slice 2: same hazard as `filter_param_bufs` above, for blend-mode
        // composites. `self.blend_mode_uniform` used to be written via `queue.write_buffer`
        // once per `PushBlendMode`/`PopBlendMode` pair; with 2+ such pairs in one frame
        // (e.g. several `background-blend-mode` boxes, or one box with 2+ blended
        // background layers) every blend render pass ended up reading whichever mode was
        // written LAST, since all writes land before the single shared encoder submits.
        let mut blend_mode_param_bufs: Vec<wgpu::Buffer> = Vec::new();
        // BUG-277 срез 7: та же ловушка для параметров blur-проходов. Сепарабельный
        // гауссиан кодирует ДВА прохода одним `self.blur_uniform` — H (direction=0)
        // и V (direction=1) — и обе записи ложатся до единственного `submit`, так что
        // оба прохода читали `direction=1`: горизонтальная половина свёртки не
        // выполнялась вовсе (`filter: blur()` и `backdrop-filter: blur()` размывались
        // только по вертикали). Сюда же попадает и `sigma` при 2+ размытиях в кадре.
        let mut blur_param_bufs: Vec<wgpu::Buffer> = Vec::new();

        // BUG-274: поэлементный CPU-учёт encode-фазы (LUMEN_FRAME_LOG=2) —
        // суммарное время и число элементов по каждому типу RenderPlanItem.
        let mut t_plan: [std::time::Duration; 7] = Default::default();
        let mut n_plan: [u32; 7] = [0; 7];
        // BUG-274: разбивка Draw-пасса — begin_render_pass / запись ops / drop(pass).
        let mut t_draw_sub: [std::time::Duration; 3] = Default::default();

        // BUG-274 (LUMEN_FRAME_LOG=3): пер-элементный профиль encode.
        // Средние по типу пасса скрывают форму распределения: «161 пасс по
        // 0.62 мс» и «146 пассов по 0.02 мс + 15 по 6.5 мс» дают одну и ту же
        // сумму, но требуют противоположных решений (схлопывать пассы против
        // переиспользовать текстуры). Пишем каждый элемент, печатаем топ.
        let item_log = crate::frame_log_level() >= 3;
        // (plan_kind, target_level, длительность, drop(pass) для Draw, ops в пассе).
        // BUG-405 срез 2: без числа ops «дорог каждый пасс» неотличимо от
        // «дорога каждая draw-операция» — а лечится это противоположно.
        let mut items_prof: Vec<(
            usize,
            usize,
            std::time::Duration,
            std::time::Duration,
            usize,
            String,
            String,
        )> =
            if item_log { Vec::with_capacity(render_plan.len()) } else { Vec::new() };

        // BUG-405 срез 2: кадр подаётся порциями по `SUBMIT_CHUNK_ITEMS`
        // элементов плана, а не одним командным списком. Подачи исполняются
        // в порядке отправки, поэтому пасс, читающий результат предыдущего,
        // видит его как и раньше — меняется только момент отправки.
        let split_submit = !split_submit_disabled();
        let mut since_submit = 0usize;
        // BUG-405 срез 6: склейка V-прохода блюра с композитом.
        let blur_merge_on = self.blur_merge_enabled && !blur_merge_disabled();
        let mut filter_passes = 0u64;

        for item in &render_plan {
            if split_submit && since_submit >= SUBMIT_CHUNK_ITEMS {
                let next = self.device.create_command_encoder(
                    &wgpu::CommandEncoderDescriptor { label: Some("encoder-chunk") },
                );
                self.queue.submit([std::mem::replace(&mut encoder, next).finish()]);
                self.submissions += 1;
                since_submit = 0;
            }
            since_submit += 1;
            let t_item0 = std::time::Instant::now();
            // usize::MAX = «уровень неприменим к этому типу элемента».
            let item_level = match item {
                RenderPlanItem::Draw(batch) => batch.target_level,
                _ => usize::MAX,
            };
            let mut item_pass_end = std::time::Duration::ZERO;
            let item_ops = match item {
                RenderPlanItem::Draw(batch) => batch.ops_end - batch.ops_start,
                _ => 0,
            };
            // BUG-405 срез 2: маска типов операций пасса (s/f/c/r/t/i/g/x).
            // Цена пасса от их ЧИСЛА не зависит — значит вопрос в том, какой
            // именно тип впервые появляется там, где кадр «переключается» с
            // дешёвых пассов на дорогие.
            let mut desc = String::new();
            let item_kinds: String = if item_log {
                // BUG-405 срез 10: слотов девять, а не восемь — `Shadow`
                // (аналитическая тень среза 7) писалась в индекс 9 при длине
                // 8 и роняла окно на первом же кадре с тенью под
                // `LUMEN_FRAME_LOG=3`.
                let mut m = [false; 9];
                if let RenderPlanItem::Draw(batch) = item {
                    // Дескриптор пасса: цель и load-op цвета. Пустые пассы
                    // (проба) стоят столько же, сколько полные, — значит
                    // различать их можно только этим.
                    desc = format!(
                        "L{}{}",
                        batch.target_level,
                        match batch.load_op {
                            LoadOpChoice::Clear(_) => "c",
                            LoadOpChoice::ClearTransparent => "t",
                            LoadOpChoice::Load => "l",
                            LoadOpChoice::LoadColorClearDepth => "r",
                        }
                    );
                    for op in &draw_ops[batch.ops_start..batch.ops_end] {
                        m[match op {
                            DrawOp::SetScissor(_) | DrawOp::SetClip(_) => 0,
                            DrawOp::Fill { .. } => 1,
                            DrawOp::Circle { .. } => 2,
                            DrawOp::RRect { .. } => 3,
                            DrawOp::Shadow { .. } => 8,
                            DrawOp::Text { .. } => 4,
                            DrawOp::Image { .. } => 5,
                            DrawOp::Gradient { .. } => 6,
                            DrawOp::CrossFade { .. } => 7,
                        }] = true;
                    }
                }
                "sfcrtigxh"
                    .chars()
                    .zip(m)
                    .filter(|(_, on)| *on)
                    .map(|(ch, _)| ch)
                    .collect()
            } else {
                String::new()
            };
            let plan_kind = match item {
                RenderPlanItem::Draw(_) => 0,
                RenderPlanItem::Composite(_) => 1,
                RenderPlanItem::MaskComposite(_) => 2,
                RenderPlanItem::FilterComposite(_) => 3,
                RenderPlanItem::BackdropFilterComposite(_) => 4,
                RenderPlanItem::MaskLayerComposite(_) => 5,
                // Оба клип-композита делят слот статистики: пасс один и тот
                // же по смыслу (уровень → родитель через покрытие контура).
                RenderPlanItem::RRectClipComposite(_) => 6,
                RenderPlanItem::PathClipComposite(_) => 6,
            };
            match item {
                RenderPlanItem::Draw(batch) => {
                    let target_view = if batch.target_level == 0 {
                        &frame_view
                    } else {
                        &self.layer_textures[batch.target_level - 1].view
                    };
                    let load = match batch.load_op {
                        // Рычаг переписи среза 30 действует только на клир
                        // ЦЕЛИ пасса полосы (уровень 0): уровни-offscreen
                        // чистятся своей текстурой, и их клир — не та статья.
                        _ if band_load_color && batch.target_level == 0 => wgpu::LoadOp::Load,
                        LoadOpChoice::Clear(c) => wgpu::LoadOp::Clear(c),
                        LoadOpChoice::ClearTransparent => {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        }
                        LoadOpChoice::Load | LoadOpChoice::LoadColorClearDepth => {
                            wgpu::LoadOp::Load
                        }
                    };
                    // All render passes must supply a depth attachment because the
                    // fill/rrect/circle pipelines use depth_write_enabled:true.
                    // wgpu validation requires: pipeline has depth → pass has depth attachment.
                    // Off-screen opacity layers don't need depth sorting, so they always
                    // clear to 1.0 (far plane) — correct result; they are composited by alpha.
                     let depth_attachment = self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                         view: dv,
                         depth_ops: Some(wgpu::Operations {
                             // Level 0: clear to 1.0 (far) at the start of each draw
                             //          batch so depth tests within the pass are
                             //          accumulated across same-frame batches.
                             // Level > 0: clear to 1.0 so depth sorting within the
                             //            offscreen layer is independent of the parent frame.
                             load: if batch.target_level > 0 {
                                 wgpu::LoadOp::Clear(1.0)
                             } else if band_load_depth
                                 || matches!(batch.load_op, LoadOpChoice::Load)
                             {
                                 // `band_load_depth` — рычаг переписи среза 30
                                 // (пункт 62): снять клир глубины полосы, чтобы
                                 // назвать его долю в постоянной статье.
                                 wgpu::LoadOp::Load
                             } else {
                                 wgpu::LoadOp::Clear(1.0)
                             },
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    });
                    let t_d0 = std::time::Instant::now();
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("draw-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: depth_attachment,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    let t_d1 = t_d0.elapsed();
                    run_draw_ops!(pass, batch.ops_start, batch.ops_end);
                    let t_d2 = t_d0.elapsed();
                    drop(pass);
                    let t_d3 = t_d0.elapsed();
                    t_draw_sub[0] += t_d1;
                    t_draw_sub[1] += t_d2 - t_d1;
                    t_draw_sub[2] += t_d3 - t_d2;
                    item_pass_end = t_d3 - t_d2;
                }
                RenderPlanItem::Composite(comp) => {
                    if let Some(cvb) = &comp_vbuf {
                        // Blend path: non-Normal mode AND a READABLE backdrop exists.
                        // `from_level > 1` — родитель есть offscreen-слой (сэмплируется).
                        // `from_level == 1` — родитель есть сама поверхность кадра: её
                        // нельзя сэмплировать, но можно скопировать в scratch, если
                        // текстура кадра доступна с `COPY_SRC` (BUG-277 срез 3). Без неё
                        // (`RenderPassMode::Band`, драйвер без `COPY_SRC` на swapchain)
                        // остаётся прежний alpha-over fallback.
                        let dst_is_frame = comp.from_level == 1;
                        if comp.mode != BlendMode::Normal
                            && (comp.from_level > 1 || (dst_is_frame && frame_blend_dst.is_some()))
                        {
                            // Ensure scratch layer before borrowing layer_textures immutably.
                            let (dst_w, dst_h) = if dst_is_frame {
                                (surface_w, surface_h)
                            } else {
                                let l = &self.layer_textures[comp.from_level - 2];
                                (l.width, l.height)
                            };
                            self.ensure_scratch_layer(dst_w, dst_h);
                            // Copy dst (parent layer / frame) into scratch before overwriting it.
                            let dst_tex_copy = match frame_blend_dst {
                                Some(t) if dst_is_frame => t.as_image_copy(),
                                _ => self.layer_textures[comp.from_level - 2].texture.as_image_copy(),
                            };
                            let scratch_copy = self.scratch_layer.as_ref().unwrap().texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                dst_tex_copy,
                                scratch_copy,
                                wgpu::Extent3d { width: dst_w, height: dst_h, depth_or_array_layers: 1 },
                            );
                            // Per-composite blend mode uniform (u32 mode + 3× u32 padding =
                            // 16 bytes) — a fresh buffer per composite, not a `write_buffer`
                            // into the shared `self.blend_mode_uniform` (see
                            // `blend_mode_param_bufs` above for why).
                            let mode_u32 = blend_mode_to_u32(comp.mode);
                            let uniform_data: [u32; 4] = [mode_u32, 0, 0, 0];
                            let mode_buf = make_blend_mode_param_buf(&self.device, &uniform_data);
                            // Create per-frame blend bind group (src + scratch + sampler + uniform).
                            let src_view = &self.layer_textures[comp.from_level - 1].view;
                            let scratch_view = &self.scratch_layer.as_ref().unwrap().view;
                            let target_view = if dst_is_frame {
                                &frame_view
                            } else {
                                &self.layer_textures[comp.from_level - 2].view
                            };
                            let blend_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("blend-bg"),
                                layout: &self.blend_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(src_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(scratch_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 3,
                                        resource: mode_buf.as_entire_binding(),
                                    },
                                ],
                            });
                            blend_mode_param_bufs.push(mode_buf);
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blend-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.blend_pipeline());
                            pass.set_bind_group(0, &blend_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(comp.comp_v_start..comp.comp_v_start + 6, 0..1);
                        } else {
                            // Normal alpha-blend path (opacity compositing or Normal blend mode).
                            let target_view = if comp.from_level == 1 {
                                &frame_view
                            } else {
                                &self.layer_textures[comp.from_level - 2].view
                            };
                            let src_bg = &self.layer_textures[comp.from_level - 1].bind_group;
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("composite-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.composite_pipeline());
                            pass.set_bind_group(0, src_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(comp.comp_v_start..comp.comp_v_start + 6, 0..1);
                        }
                    }
                }
                // CSS Overflow L3 §2 — rounded-clip composite (BUG-277 срез 5).
                // Composites the level onto its parent through the container's
                // rounded contour: scissor cut the bbox, the SDF cuts the corners.
                RenderPlanItem::RRectClipComposite(plan) => {
                    let Some(vb) = &rrect_clip_vbuf else { continue };
                    if plan.from_level == 0 { continue; }
                    let target_view = if plan.from_level == 1 {
                        &frame_view
                    } else {
                        &self.layer_textures[plan.from_level - 2].view
                    };
                    let src_bg = &self.layer_textures[plan.from_level - 1].bind_group;
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("rrect-clip-composite-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(self.rrect_clip_pipeline());
                    pass.set_bind_group(0, src_bg, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(plan.v_start..plan.v_start + 6, 0..1);
                }
                // CSS Masking L1 §3 — композит формы `clip-path`: тот же
                // пасс, что у скруглённого клипа, но контур приходит своим
                // uniform-буфером на КАЖДЫЙ пасс (общий буфер повторил бы
                // write_buffer-hazard среза 7: все записи ложатся до submit).
                RenderPlanItem::PathClipComposite(plan) => {
                    let Some(vb) = &path_clip_vbuf else { continue };
                    if plan.from_level == 0 { continue; }
                    let target_view = if plan.from_level == 1 {
                        &frame_view
                    } else {
                        &self.layer_textures[plan.from_level - 2].view
                    };
                    let shape_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("path-clip-shape-ubuf"),
                        size: std::mem::size_of::<PathClipParamsCpu>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    // SAFETY: PathClipParamsCpu is #[repr(C)] POD.
                    self.queue.write_buffer(
                        &shape_buf,
                        0,
                        as_bytes(std::slice::from_ref(plan.params.as_ref())),
                    );
                    let src_view = &self.layer_textures[plan.from_level - 1].view;
                    let shape_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("path-clip-bg"),
                        layout: &self.path_clip_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(src_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: shape_buf.as_entire_binding(),
                            },
                        ],
                    });
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("path-clip-composite-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(self.path_clip_pipeline());
                    pass.set_bind_group(0, &shape_bg, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(plan.v_start..plan.v_start + 6, 0..1);
                }
                // CSS Masking L1 §4 — mask composite.
                // Composites the offscreen element layer onto the parent using the
                // mask as an alpha multiplier (mask-mode: alpha, CSS Masking L1 §6.2).
                RenderPlanItem::MaskComposite(comp) => {
                    let target_view = if comp.from_level == 1 {
                        &frame_view
                    } else if comp.from_level >= 2 {
                        &self.layer_textures[comp.from_level - 2].view
                    } else {
                        continue;
                    };
                    let content_layer_view = &self.layer_textures[comp.from_level - 1].view;

                    // Determine mask texture view: image from cache or rendered gradient.
                    let mask_gpu_image = comp.mask_src.as_ref().and_then(|src| self.images.get(src));
                    let mask_view: Option<&wgpu::TextureView> = if let Some(img) = mask_gpu_image {
                        Some(&img.view)
                    } else if let Some(grad_spec) = &comp.mask_gradient {
                        // Render gradient into a surface-size temp texture and use it as mask.
                        // Gradient rendered in same pixel-coord system as content layer,
                        // so uv_mask = pos/surface (set during plan building) samples correctly.
                        let (grad_params, grad_verts) = match grad_spec.as_ref() {
                            MaskGradientSpec::Linear { params, quad } => (params, quad),
                            MaskGradientSpec::Radial { params, quad } => (params, quad),
                            MaskGradientSpec::Conic  { params, quad } => (params, quad),
                        };
                        // Пул-текстура пред-захвачена до цикла (temp_grad_layers);
                        // LoadOp::Clear ниже гарантирует чистый старт при reuse.
                        let Some(grad_layer) = temp_grad_layers.get(temp_grad_next) else {
                            continue;
                        };
                        temp_grad_next += 1;
                        let temp_view = &grad_layer.view;
                        // Write gradient params + stop list and build bind group.
                        let grad_ubuf =
                            write_grad_buffer(&self.device, &self.queue, grad_params, "mask-grad-sbuf");
                        let grad_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("mask-grad-bg"),
                            layout: &self.gradient_bgl,
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: grad_ubuf.as_entire_binding(),
                            }],
                        });
                        // Квад градиента — в экранных CSS px: `PushTransform`
                        // применён ещё при планировании (BUG-277 срез 6),
                        // иначе маска легла бы мимо трансформированного контента.
                        let grad_vbuf_m = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("mask-grad-vbuf"),
                            size: std::mem::size_of_val(grad_verts) as u64,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        self.queue.write_buffer(&grad_vbuf_m, 0, as_bytes(grad_verts.as_slice()));
                        // Render gradient into temp_tex (cleared to transparent first).
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("mask-grad-render"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: temp_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.gradient_pipeline());
                            pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                            pass.set_bind_group(1, &grad_bg, &[]);
                            pass.set_vertex_buffer(0, grad_vbuf_m.slice(..));
                            pass.draw(0..6, 0..1);
                        }
                        Some(temp_view)
                    } else {
                        None
                    };

                    if let (Some(mvb), Some(mask_view)) = (&mask_vbuf, mask_view) {
                        // Build per-frame bind group: content layer + mask texture + sampler.
                        let mask_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("mask-composite-bg"),
                            layout: &self.mask_composite_bgl,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(content_layer_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(mask_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                                },
                            ],
                        });
                        let v_count = comp.mask_v_end - comp.mask_v_start;
                        if v_count > 0 {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("mask-composite-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: target_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.mask_composite_pipeline());
                            pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                            pass.set_bind_group(1, &mask_bg, &[]);
                            pass.set_vertex_buffer(0, mvb.slice(..));
                            pass.draw(comp.mask_v_start..comp.mask_v_end, 0..1);
                        }
                    } else {
                        // Mask image not registered: fallback — composite content at full opacity.
                        let src_bg = &self.layer_textures[comp.from_level - 1].bind_group;
                        let fallback_verts: [CompositeVertex; 6] = [
                            CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0,  1.0], uv: [1.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha: 1.0 },
                            CompositeVertex { pos: [-1.0,  1.0], uv: [0.0, 0.0], alpha: 1.0 },
                            CompositeVertex { pos: [ 1.0, -1.0], uv: [1.0, 1.0], alpha: 1.0 },
                            CompositeVertex { pos: [-1.0, -1.0], uv: [0.0, 1.0], alpha: 1.0 },
                        ];
                        let fallback_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("mask-fallback-vbuf"),
                            size: std::mem::size_of_val(&fallback_verts) as u64,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        self.queue.write_buffer(&fallback_buf, 0, as_bytes(fallback_verts.as_slice()));
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("mask-fallback-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: target_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(self.composite_pipeline());
                        pass.set_bind_group(0, src_bg, &[]);
                        pass.set_vertex_buffer(0, fallback_buf.slice(..));
                        pass.draw(0..6, 0..1);
                    }
                }
                // CSS Filter Effects L1 — композит filter-группы.
                // Есть блюр: сепарабельный гаусс, H-проход пишет в scratch, а
                // вертикальный идёт вместе с цветовыми фильтрами сразу в
                // родителя (BUG-405 срез 6: один пасс вместо двух). Нет блюра —
                // один пасс цветовых фильтров, как и раньше.
                RenderPlanItem::FilterComposite(plan) => {
                    if plan.from_level == 0 { continue; }
                    let src_layer_idx = plan.from_level - 1;
                    let Some(cvb) = &comp_vbuf else { continue };

                    let blur_sigma = plan.filters.iter().find_map(|f| match f {
                        FilterFn::Blur(s) if *s > 0.0 => Some(*s),
                        _ => None,
                    });
                    // BUG-405 срез 6: склейка вертикального прохода с композитом.
                    // Выключено — прежний трёхпассовый путь (рычаг отката и
                    // плечо A/B).
                    let merge = blur_sigma.is_some() && blur_merge_on;
                    // bbox-офскрин блюра (BUG-405 срез 24) — только на слитом
                    // пути: несклеенный V-проход пишет обратно в сам слой
                    // уровня, там регион ничего не экономит.
                    let region = if merge { plan.region } else { None };
                    // Офскрин региона держится до конца пасса композита (его
                    // читает слитый пасс), затем возвращается в пул.
                    let mut pooled_region: Option<OffscreenLayer> = None;
                    // Цель H-прохода — она же источник слитого пасса.
                    let mut blur_out_view: Option<wgpu::TextureView> = None;
                    // Разбивка цены композита по пассам (только когда план
                    // кадра печатается, `LUMEN_FRAME_LOG=3`): H-проход читает
                    // слой уровня, слитый — офскрин. Без неё «дорогой filt0»
                    // не адресуем. Вне журнала таймеры не берутся вовсе.
                    let mut t_pass_h = std::time::Duration::ZERO;
                    let mut t_pass_v = std::time::Duration::ZERO;

                    if let Some(sigma) = blur_sigma {
                        // Ensure scratch before any immutable borrows of self.
                        let (blur_dst_view, blur_depth_view, h_v_start, h_scissor) = match region {
                            Some(r) => {
                                let [_, _, rw, rh] = r.rect;
                                let layer = self.create_layer_texture(rw, rh);
                                let view = layer.view.clone();
                                pooled_region = Some(layer);
                                // Depth обязан совпадать по размеру с color —
                                // валидация wgpu (как у bbox-backdrop).
                                let depth = Some(self.small_depth_view(rw, rh));
                                // Scissor не нужен: пасс и так кроет только
                                // регион, а его края обязаны быть заполнены —
                                // слитый пасс читает офскрин целиком.
                                (view, depth, r.src_v_start, None)
                            }
                            None => {
                                let src_w = self.layer_textures[src_layer_idx].width;
                                let src_h = self.layer_textures[src_layer_idx].height;
                                self.ensure_scratch_layer(src_w, src_h);
                                let view = self.scratch_layer.as_ref().unwrap().view.clone();
                                (view, self.depth_view.clone(), plan.comp_v_start, plan.scissor)
                            }
                        };
                        blur_out_view = Some(blur_dst_view.clone());

                        // H-проход: src_level → scratch (или bbox-офскрин).
                        let blur_h = BlurParamsCpu { sigma, direction: 0, _p0: 0, _p1: 0 };
                        let blur_h_buf = make_blur_param_buf(&self.device, &blur_h);
                        let src_view_h = &self.layer_textures[src_layer_idx].view;
                        let scratch_view_h = &blur_dst_view;
                        let blur_bg_h = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("blur-h-bg"),
                            layout: &self.blur_bgl,
                            entries: &[
                                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view_h) },
                                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                wgpu::BindGroupEntry { binding: 2, resource: blur_h_buf.as_entire_binding() },
                            ],
                        });
                        let t_h0 = item_log.then(std::time::Instant::now);
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blur-h-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: scratch_view_h,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: blur_depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.blur_pipeline());
                            if let Some(s) = h_scissor {
                                pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
                            pass.set_bind_group(0, &blur_bg_h, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(h_v_start..h_v_start + 6, 0..1);
                        }
                        t_pass_h = t_h0.map_or(t_pass_h, |t| t.elapsed());
                        filter_passes += 1;

                        if !merge {
                            // V-проход: scratch → src_level (полностью размытый результат).
                            let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                            let blur_v_buf = make_blur_param_buf(&self.device, &blur_v);
                            let scratch_view_v = &self.scratch_layer.as_ref().unwrap().view;
                            let src_level_view_v = &self.layer_textures[src_layer_idx].view;
                            let blur_bg_v = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("blur-v-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view_v) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: blur_v_buf.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("blur-v-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: src_level_view_v,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                                view: dv,
                                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                stencil_ops: None,
                            }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(self.blur_pipeline());
                                if let Some(s) = plan.scissor {
                                    pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                                }
                                pass.set_bind_group(0, &blur_bg_v, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                            filter_passes += 1;
                            blur_param_bufs.push(blur_v_buf);
                        }
                        // Буферы должны пережить `encoder` — проходы читают их
                        // на `submit`, а не в момент кодирования.
                        blur_param_bufs.push(blur_h_buf);
                    }

                    // Цветовые фильтры списка (блюр обработан выше).
                    let mut entries = [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8];
                    let mut color_count = 0u32;
                    for f in &plan.filters {
                        if !matches!(f, FilterFn::Blur(_)) && (color_count as usize) < 8 {
                            entries[color_count as usize] = filter_fn_to_entry(f);
                            color_count += 1;
                        }
                    }
                    let filter_params = FilterParamsCpu {
                        count: color_count, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries,
                    };
                    let fp_buf = make_filter_param_buf(&self.device, &filter_params);

                    let dst_view = if plan.from_level == 1 {
                        &frame_view
                    } else {
                        &self.layer_textures[plan.from_level - 2].view
                    };
                    if merge {
                        // Слитый пасс: вертикальный блюр scratch-а + цветовые
                        // фильтры + композит в родителя.
                        let sigma = blur_sigma.unwrap_or(0.0);
                        let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                        let blur_v_buf = make_blur_param_buf(&self.device, &blur_v);
                        // Источник — цель H-прохода: bbox-офскрин (срез 24)
                        // либо прежний полноразмерный scratch.
                        let fallback_scratch_view;
                        let scratch_view_v = match blur_out_view.as_ref() {
                            Some(v) => v,
                            None => {
                                fallback_scratch_view =
                                    self.scratch_layer.as_ref().map(|s| s.view.clone());
                                match fallback_scratch_view.as_ref() {
                                    Some(v) => v,
                                    None => continue,
                                }
                            }
                        };
                        let bc_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("blur-composite-bg"),
                            layout: &self.blur_composite_bgl,
                            entries: &[
                                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scratch_view_v) },
                                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                wgpu::BindGroupEntry { binding: 2, resource: blur_v_buf.as_entire_binding() },
                                wgpu::BindGroupEntry { binding: 3, resource: fp_buf.as_entire_binding() },
                            ],
                        });
                        let t_v0 = item_log.then(std::time::Instant::now);
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("blur-composite-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: dst_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.blur_composite_pipeline());
                            if let Some(s) = plan.scissor {
                                pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
                            pass.set_bind_group(0, &bc_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            // При bbox-офскрине квад кроет прямоугольник
                            // региона в цели, а UV пробегает офскрин целиком.
                            let v_start = region.map_or(plan.comp_v_start, |r| r.dst_v_start);
                            pass.draw(v_start..v_start + 6, 0..1);
                        }
                        t_pass_v = t_v0.map_or(t_pass_v, |t| t.elapsed());
                        filter_passes += 1;
                        blur_param_bufs.push(blur_v_buf);
                    } else {
                        // Пасс цветовых фильтров: src_level → родитель
                        // (PREMULTIPLIED_ALPHA_BLENDING). Если блюр был, в
                        // src_level уже лежит размытое содержимое.
                        let src_view_f = &self.layer_textures[src_layer_idx].view;
                        let filter_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("filter-bg"),
                            layout: &self.filter_bgl,
                            entries: &[
                                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view_f) },
                                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                wgpu::BindGroupEntry { binding: 2, resource: fp_buf.as_entire_binding() },
                            ],
                        });
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("filter-pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: dst_view,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                                })],
                                depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });
                            pass.set_pipeline(self.filter_pipeline());
                            if let Some(s) = plan.scissor {
                                pass.set_scissor_rect(s.x, s.y, s.width, s.height);
                            }
                            pass.set_bind_group(0, &filter_bg, &[]);
                            pass.set_vertex_buffer(0, cvb.slice(..));
                            pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                        }
                        filter_passes += 1;
                    }
                    filter_param_bufs.push(fp_buf);
                    // bbox-офскрин отработал — вернуть в пул. Безопасно сразу
                    // после записи команд: они исполняются в порядке
                    // encoder-а (та же дисциплина, что у ping-pong backdrop-а).
                    if let Some(l) = pooled_region.take() {
                        self.release_layer_to_pool(l);
                    }
                    if item_log {
                        let sc = plan
                            .scissor
                            .as_ref()
                            .map(|s| format!("{}x{}", s.width, s.height))
                            .unwrap_or_else(|| "full".into());
                        let rg = region
                            .map(|r| format!(" r{}x{}", r.rect[2], r.rect[3]))
                            .unwrap_or_default();
                        desc = format!(
                            "L{}b{:.1}c{color_count} sc{sc}{}{rg} h{:.2} v{:.2}",
                            plan.from_level,
                            blur_sigma.unwrap_or(0.0),
                            if merge { " merged" } else { "" },
                            t_pass_h.as_secs_f32() * 1000.0,
                            t_pass_v.as_secs_f32() * 1000.0,
                        );
                    }
                }
                // CSS Filter Effects L1 §2 / Compositing §13 — backdrop-filter composite.
                //
                // Execution order:
                //   1. copy parent layer → scratch (GPU texture copy)
                //   2. blur scratch if needed (H: scratch → backdrop_layer, V: backdrop_layer → scratch)
                //   3. blit scratch → parent at bounds with optional color filter (REPLACE blend)
                //   4. composite element layer → parent (ALPHA_BLENDING, same as FilterComposite)
                //
                // When from_level < 2 (parent = surface texture, lacks TEXTURE_BINDING),
                // steps 1-3 are skipped (can't copy from surface), but step 4 still runs so
                // the element content is visible (no silent drop).
                RenderPlanItem::BackdropFilterComposite(plan) => {
                    let Some(cvb) = &comp_vbuf else { continue };
                    // from_level < 2 means parent is the surface — backdrop blur/blit impossible.
                    let skip_backdrop = plan.from_level < 2;

                    // Ordinals evicted by `store()` whose textures must be freed once the
                    // current element's passes (which borrow the cache map) have ended.
                    let mut evicted_ordinals: Vec<u32> = Vec::new();

                    if !skip_backdrop {
                    let parent_idx = plan.from_level - 2;
                    let parent_w = self.layer_textures[parent_idx].width;
                    let parent_h = self.layer_textures[parent_idx].height;
                    // bbox-офскрины backdrop-фильтра (EXPERIMENT.md §2):
                    // ping-pong и кэш живут в размере региона (bounds + ядро
                    // блюра), а не родителя. region=None — прежний
                    // полноразмерный путь (kill-switch/фолбэк).
                    let (rx, ry, rw, rh) = match plan.region {
                        Some([x, y, w, h]) => (x, y, w, h),
                        None => (0, 0, parent_w, parent_h),
                    };
                    let use_region = plan.region.is_some();
                    // Копия из родителя не может выйти за его края: текстуры
                    // выровнены до 64 px и бывают шире остатка родителя.
                    // Копия всегда покрывает невыровненный (логический)
                    // регион — все выборки блюра для читаемых blit-ом
                    // пикселей лежат в скопированной области.
                    let copy_w = rw.min(parent_w.saturating_sub(rx)).max(1);
                    let copy_h = rh.min(parent_h.saturating_sub(ry)).max(1);
                    let mut pooled_ping: Option<OffscreenLayer> = None;
                    let mut pooled_pong: Option<OffscreenLayer> = None;
                    if use_region {
                        pooled_ping = Some(self.create_layer_texture(rw, rh));
                        pooled_pong = Some(self.create_layer_texture(rw, rh));
                    } else {
                        self.ensure_scratch_layer(parent_w, parent_h);
                        self.ensure_backdrop_layer(parent_w, parent_h);
                    }
                    // Depth-attachment обязан совпадать по размеру с
                    // color-attachment (валидация wgpu).
                    let bd_depth_view: Option<wgpu::TextureView> = if use_region {
                        Some(self.small_depth_view(rw, rh))
                    } else {
                        self.depth_view.clone()
                    };
                    // The per-ordinal cache texture is the blit source (always), and on a
                    // cache hit it already holds the previous frame's filtered backdrop.
                    if self.ensure_backdrop_cache_texture(plan.ordinal, rw, rh) {
                        // A resize discarded the cached pixels — drop the stale hash so it
                        // cannot produce a hit against the fresh (uninitialised) texture.
                        self.backdrop_cache.invalidate(plan.ordinal);
                    }
                    // Ping = вход блюра (копия родителя), pong = выход H-пасса.
                    let ping_tex: wgpu::Texture;
                    let ping_view: wgpu::TextureView;
                    let pong_view: wgpu::TextureView;
                    if let (Some(a), Some(b)) = (pooled_ping.as_ref(), pooled_pong.as_ref()) {
                        ping_tex = a.texture.clone();
                        ping_view = a.view.clone();
                        pong_view = b.view.clone();
                    } else {
                        let s = self.scratch_layer.as_ref().unwrap();
                        ping_tex = s.texture.clone();
                        ping_view = s.view.clone();
                        pong_view = self.backdrop_layer.as_ref().unwrap().view.clone();
                    }
                    if use_region && crate::frame_log_level() >= 2 {
                        eprintln!(
                            "[frame:wgpu]   bdrop region {rw}x{rh} @({rx},{ry}) of {parent_w}x{parent_h}"
                        );
                    }
                    // Cache HIT: the cached texture is unchanged → skip the copy + blur
                    // passes entirely. Disabled cache (`backdrop_frame_hash == None`)
                    // always misses, reproducing the original behaviour.
                    let cache_hit = match backdrop_frame_hash {
                        Some(fh) => self.backdrop_cache.lookup(plan.ordinal, fh),
                        None => false,
                    };

                    let blur_sigma = plan.filters.iter().find_map(|f| match f {
                        FilterFn::Blur(s) if *s > 0.0 => Some(*s),
                        _ => None,
                    });

                    if !cache_hit {
                        if let Some(sigma) = blur_sigma {
                            // Step 1: copy parent-region → ping (blur H-pass input).
                            // parent has COPY_SRC, ping (pooled/scratch) has COPY_DST.
                            let mut parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            parent_copy.origin = wgpu::Origin3d { x: rx, y: ry, z: 0 };
                            let ping_copy = ping_tex.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                ping_copy,
                                wgpu::Extent3d { width: copy_w, height: copy_h, depth_or_array_layers: 1 },
                            );

                            // Step 2 H pass: ping → pong (REPLACE).
                            let blur_h = BlurParamsCpu { sigma, direction: 0, _p0: 0, _p1: 0 };
                            let blur_h_buf = make_blur_param_buf(&self.device, &blur_h);
                            let blur_bg_h = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-h-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&ping_view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: blur_h_buf.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("backdrop-blur-h-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &pong_view,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: bd_depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(self.blur_pipeline());
                                pass.set_bind_group(0, &blur_bg_h, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                            // Step 2 V pass: pong → CACHE texture (REPLACE).
                            // The blurred result lands in the cache, ready for reuse next frame.
                            let blur_v = BlurParamsCpu { sigma, direction: 1, _p0: 0, _p1: 0 };
                            let blur_v_buf = make_blur_param_buf(&self.device, &blur_v);
                            let cache_view_v = &self.backdrop_cache_textures[&plan.ordinal].view;
                            let blur_bg_v = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("backdrop-blur-v-bg"),
                                layout: &self.blur_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&pong_view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                                    wgpu::BindGroupEntry { binding: 2, resource: blur_v_buf.as_entire_binding() },
                                ],
                            });
                            {
                                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: Some("backdrop-blur-v-pass"),
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: cache_view_v,
                                        resolve_target: None,
                                        depth_slice: None,
                                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                                    })],
                                    depth_stencil_attachment: bd_depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                });
                                pass.set_pipeline(self.blur_pipeline());
                                pass.set_bind_group(0, &blur_bg_v, &[]);
                                pass.set_vertex_buffer(0, cvb.slice(..));
                                pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                            }
                            // Буферы должны пережить `encoder` — см. тот же
                            // push в `FilterComposite`.
                            blur_param_bufs.push(blur_h_buf);
                            blur_param_bufs.push(blur_v_buf);
                        } else {
                            // Filter-only backdrop (no blur): copy parent-region → cache directly.
                            // parent has COPY_SRC, cache has COPY_DST.
                            let mut parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                            parent_copy.origin = wgpu::Origin3d { x: rx, y: ry, z: 0 };
                            let cache_copy = self.backdrop_cache_textures[&plan.ordinal].texture.as_image_copy();
                            encoder.copy_texture_to_texture(
                                parent_copy,
                                cache_copy,
                                wgpu::Extent3d { width: copy_w, height: copy_h, depth_or_array_layers: 1 },
                            );
                        }

                        // Record the freshly produced backdrop in the cache (skipped when
                        // caching is disabled — `backdrop_frame_hash == None`).
                        if let Some(fh) = backdrop_frame_hash {
                            let bytes = rw as usize * rh as usize * 4;
                            evicted_ordinals = self.backdrop_cache.store(plan.ordinal, fh, bytes);
                        }
                    }

                    // Step 3: blit cache texture → parent at element bounds.
                    // Uses backdrop_blit_pipeline (REPLACE RGB, preserve dst alpha) to
                    // write the filtered backdrop into the parent layer at element bounds.
                    // Applies color filters (count > 0) or passthrough (count = 0).
                    let mut bd_entries = [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8];
                    let mut bd_color_count = 0u32;
                    for f in &plan.filters {
                        if !matches!(f, FilterFn::Blur(_)) && (bd_color_count as usize) < 8 {
                            bd_entries[bd_color_count as usize] = filter_fn_to_entry(f);
                            bd_color_count += 1;
                        }
                    }
                    let bd_filter_params = FilterParamsCpu {
                        count: bd_color_count, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries: bd_entries,
                    };
                    let bd_fp_buf = make_filter_param_buf(&self.device, &bd_filter_params);
                    let parent_dst_view = &self.layer_textures[parent_idx].view;
                    // Source is the cache texture — holds the blurred (or copied) backdrop.
                    let bd_src_view = &self.backdrop_cache_textures[&plan.ordinal].view;
                    let bd_blit_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("backdrop-blit-bg"),
                        layout: &self.filter_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(bd_src_view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                            wgpu::BindGroupEntry { binding: 2, resource: bd_fp_buf.as_entire_binding() },
                        ],
                    });
                    filter_param_bufs.push(bd_fp_buf);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("backdrop-blit-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_dst_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(self.backdrop_blit_pipeline());
                        pass.set_bind_group(0, &bd_blit_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.bounds_v_start..plan.bounds_v_start + 6, 0..1);
                    }

                    // bbox-офскрины: ping-pong вернуть в пул (кэш-текстура
                    // остаётся жить у ordinal-а — её читают blit и след. кадры).
                    // Переиспользование в этом же кадре безопасно: команды
                    // исполняются в порядке encoder-а.
                    if let Some(l) = pooled_ping.take() {
                        self.release_layer_to_pool(l);
                    }
                    if let Some(l) = pooled_pong.take() {
                        self.release_layer_to_pool(l);
                    }
                    } // end if !skip_backdrop

                    // Step 4: composite element layer → parent (ALPHA_BLENDING).
                    // Runs even when skip_backdrop (from_level < 2) so element content
                    // is always visible; only the filtered backdrop blit is skipped.
                    let parent_dst_view4 = if plan.from_level >= 2 {
                        &self.layer_textures[plan.from_level - 2].view as *const _
                    } else {
                        &frame_view as *const _
                    };
                    // SAFETY: we hold &mut self for the encoder lifetime and frame_view
                    // is valid for the duration of this frame. layer_textures is not
                    // mutated after this point within the current plan item.
                    let parent_dst_view4: &wgpu::TextureView = unsafe { &*parent_dst_view4 };
                    let elem_filter_params = FilterParamsCpu {
                        count: 0, _pad0: 0, _pad1: 0, _pad2: 0,
                        entries: [FilterEntryCpu { kind: 0, amount: 0.0, _p0: 0, _p1: 0 }; 8],
                    };
                    let elem_fp_buf = make_filter_param_buf(&self.device, &elem_filter_params);
                    let elem_src_view = &self.layer_textures[plan.from_level - 1].view;
                    let elem_filter_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("backdrop-elem-composite-bg"),
                        layout: &self.filter_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(elem_src_view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.layer_sampler) },
                            wgpu::BindGroupEntry { binding: 2, resource: elem_fp_buf.as_entire_binding() },
                        ],
                    });
                    filter_param_bufs.push(elem_fp_buf);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("backdrop-elem-composite-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_dst_view4,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(self.filter_pipeline());
                        pass.set_bind_group(0, &elem_filter_bg, &[]);
                        pass.set_vertex_buffer(0, cvb.slice(..));
                        pass.draw(plan.comp_v_start..plan.comp_v_start + 6, 0..1);
                    }

                    // Free textures evicted by the cache's budget enforcement now that
                    // the element's passes (which borrowed the cache map) have ended.
                    for ord in evicted_ordinals {
                        self.backdrop_cache_textures.remove(&ord);
                    }
                }

                // CSS Masking L1 §5 — mask-layer composite.
                // Applies the rendered mask layer (from PushMaskLayer/PopMaskLayer)
                // to the parent layer's content.
                //
                // Algorithm:
                //   1. Copy parent layer → scratch (saves element content).
                //   2. REPLACE-blend pass: fragment = scratch × mask_value → parent at rect.
                //
                // Phase 0 limitation: skipped when from_level <= 1 (parent = surface,
                // lacks TEXTURE_BINDING and COPY_SRC).
                RenderPlanItem::MaskLayerComposite(plan) => {
                    if plan.from_level < 2 { continue; }
                    let Some(mlvb) = &mask_layer_vbuf else { continue };
                    let v_count = plan.ml_v_end - plan.ml_v_start;
                    if v_count == 0 { continue; }

                    let parent_idx = plan.from_level - 2;
                    let mask_idx   = plan.from_level - 1;
                    let parent_w = self.layer_textures[parent_idx].width;
                    let parent_h = self.layer_textures[parent_idx].height;
                    self.ensure_scratch_layer(parent_w, parent_h);

                    // Step 1: copy parent → scratch.
                    let parent_copy = self.layer_textures[parent_idx].texture.as_image_copy();
                    let scratch_copy = self.scratch_layer.as_ref().unwrap().texture.as_image_copy();
                    encoder.copy_texture_to_texture(
                        parent_copy,
                        scratch_copy,
                        wgpu::Extent3d { width: parent_w, height: parent_h, depth_or_array_layers: 1 },
                    );

                    // Step 2: mask-layer composite pass.
                    // Bind group: scratch (content), mask layer (mask), sampler.
                    let scratch_view = &self.scratch_layer.as_ref().unwrap().view;
                    let mask_view    = &self.layer_textures[mask_idx].view;
                    let ml_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("mask-layer-composite-bg"),
                        layout: &self.mask_composite_bgl,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(scratch_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(mask_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&self.layer_sampler),
                            },
                        ],
                    });
                    let parent_view = &self.layer_textures[parent_idx].view;
                    let pipeline = match plan.mode {
                        MaskMode::Alpha     => &self.mask_layer_pipelines().0,
                        MaskMode::Luminance => &self.mask_layer_pipelines().1,
                    };
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("mask-layer-composite-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: parent_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: self.depth_view.as_ref().map(|dv| wgpu::RenderPassDepthStencilAttachment {
                            view: dv,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                        pass.set_bind_group(1, &ml_bg, &[]);
                        pass.set_vertex_buffer(0, mlvb.slice(..));
                        pass.draw(plan.ml_v_start..plan.ml_v_end, 0..1);
                    }
                }
            }
            let t_item = t_item0.elapsed();
            t_plan[plan_kind] += t_item;
            n_plan[plan_kind] += 1;
            if item_log {
                items_prof.push((plan_kind, item_level, t_item, item_pass_end, item_ops, item_kinds, desc));
            }
        }

        self.filter_passes += filter_passes;
        self.state_elisions += st_elided;
        self.draw_merges += st_merged;

        let t_after_encode = t_frame0.elapsed();
        self.queue.submit([encoder.finish()]);
        self.submissions += 1;
        // Градиент-маски: временные текстуры обратно в пул (команды уже
        // сабмичены; wgpu удерживает ресурсы до исполнения сам).
        for layer in temp_grad_layers.drain(..) {
            self.release_layer_to_pool(layer);
        }
        // BUG-272 срез 21: привести свободный список пула к байтовому бюджету.
        // Точка выбрана после `submit` намеренно — освобождаемые здесь текстуры
        // могли быть использованы командами этого кадра, а удерживает их до
        // исполнения сам wgpu (тот же довод, что у `temp_grad_layers` выше).
        self.texture_pool.trim();
        if let Some(frame) = windowed_frame {
            frame.present();
            // BUG-405: первый показанный кадр — момент запуска фонового
            // прогрева ленивых пайплайнов. Раньше нельзя (вернули бы медленный
            // старт BUG-406), позже незачем: до первой прокрутки надо успеть
            // скомпилировать хотя бы filter/blur.
            self.spawn_pipeline_warmup();
        }
        if crate::frame_log_enabled()
            && let Some(slot) = FRAME_PHASE_NANOS.get(3)
        {
            // Слот 3 — сумма ВСЕХ wgpu-пассов кадра: на промахе их два (полоса
            // и композиция), и разделять их здесь нечем — счётчик процессный.
            slot.fetch_add(
                t_frame0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        if phase_log {
            let t_total = t_frame0.elapsed();
            // BUG-405 срез 34: печать этого блока идёт ВНУТРИ окна, которое
            // шелл меряет как `[frame] total`, поэтому её цена копится
            // отдельным счётчиком — иначе она читается как неназванная работа
            // движка (на кадре попадания это 1.6 мс при 1.2 мс самой работы).
            let t_log0 = std::time::Instant::now();
            eprintln!(
                "[frame:wgpu] total {:7.2}ms | faces {:6.2} collect {:6.2} prep {:6.2} \
                 acquire {:6.2} encode {:6.2} submit {:6.2} | ops {} layers {}",
                t_total.as_secs_f64() * 1e3,
                t_after_faces.as_secs_f64() * 1e3,
                (t_after_collect - t_after_faces).as_secs_f64() * 1e3,
                (t_after_prep - t_after_collect).as_secs_f64() * 1e3,
                (t_after_acquire - t_after_prep).as_secs_f64() * 1e3,
                (t_after_encode - t_after_acquire).as_secs_f64() * 1e3,
                (t_total - t_after_encode).as_secs_f64() * 1e3,
                draw_ops.len(),
                max_level,
            );
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
            eprintln!(
                "[frame:wgpu]   plan: draw {}x{:.1}ms comp {}x{:.1}ms mask {}x{:.1}ms \
                 filt {}x{:.1}ms bdrop {}x{:.1}ms mlayer {}x{:.1}ms rclip {}x{:.1}ms",
                n_plan[0], ms(t_plan[0]), n_plan[1], ms(t_plan[1]), n_plan[2], ms(t_plan[2]),
                n_plan[3], ms(t_plan[3]), n_plan[4], ms(t_plan[4]), n_plan[5], ms(t_plan[5]),
                n_plan[6], ms(t_plan[6]),
            );
            // BUG-405 срез 10: сколько команд состояния пасса не отправлено.
            eprintln!("[frame:wgpu]   state: отсеяно команд {st_elided}, склеено draw {st_merged}");
            eprintln!(
                "[frame:wgpu]   draw-sub: begin {:.1}ms ops {:.1}ms end {:.1}ms | textures_created {} pool {} band-bg {}",
                ms(t_draw_sub[0]), ms(t_draw_sub[1]), ms(t_draw_sub[2]),
                TEXTURES_CREATED.load(std::sync::atomic::Ordering::Relaxed),
                self.texture_pool.len(),
                BAND_BLIT_BGS_CREATED.load(std::sync::atomic::Ordering::Relaxed),
            );
            // BUG-405 срез 14: фактически отправленные команды пасса по видам.
            // Счётчик отсева говорит, сколько команд сэкономлено, а этот —
            // сколько их осталось: без него цену `drop(pass)` не на что делить
            // (в срезе 14 это дало 0.15 мкс на команду против 540 мкс на пасс,
            // то есть команды к цене пасса отношения не имеют).
            eprintln!(
                "[frame:wgpu]   cmd-mix: pipeline {} bg0 {} bg1 {} vbuf {} scissor {} draw {}",
                n_cmd[0], n_cmd[1], n_cmd[2], n_cmd[3], n_cmd[4], n_cmd[5],
            );
            // BUG-405 срез 3: чем занята фаза `collect` — растеризацией
            // впервые показанных глифов или самим обходом display list.
            eprintln!(
                "[frame:wgpu]   glyphs: rasterized {} in {:.2}ms | \
                 за процесс {} в {:.1}ms",
                load_counter(&GLYPHS_RASTERIZED) - glyphs_at_entry,
                (load_counter(&GLYPH_RASTER_NANOS) - glyph_nanos_at_entry) as f64 / 1e6,
                load_counter(&GLYPHS_RASTERIZED),
                load_counter(&GLYPH_RASTER_NANOS) as f64 / 1e6,
            );

            // BUG-405 срез 4: чем обслужены скруглённые клипы кадра —
            // шейдерным контуром (0 пассов) или offscreen-уровнем (3 пасса).
            eprintln!(
                "[frame:wgpu]   rclip: шейдером {} | уровнем {}",
                clip_slots.len() - 1,
                level_rrect_clips,
            );

            // BUG-405 срез 5: сколько уровней кадра выброшено как невидимые и
            // сколько разрезов пасса родителя от них склеено обратно.
            eprintln!(
                "[frame:wgpu]   culled: уровней {culled_levels} | склеено разрезов {merged_cull_splits}",
            );

            // BUG-405 срез 10: подстатьи фазы `prep`. Число созданных за кадр
            // вершинных буферов и их суммарный объём отделяют «дорого писать
            // вершины» от «дорого СОЗДАВАТЬ буфер» — лечится это по-разному.
            eprintln!(
                "[frame:wgpu]   prep-top: atlas {:.2} uniforms {:.2} vbuf {:.2} \
                 (create {:.2} write {:.2}) grad-bg {:.2} offscreen {:.2} | \
                 vbufs {} / {} KiB, grad-bg {}",
                ms(t_after_atlas - t_after_collect),
                ms(t_after_uniforms - t_after_atlas),
                ms(t_after_vbufs - t_after_uniforms),
                ms(t_vbuf_create),
                ms(t_vbuf_write),
                ms(t_after_grad_bg - t_after_vbufs),
                ms(t_after_prep - t_after_grad_bg),
                vbuf_count,
                vbuf_bytes / 1024,
                grad_bind_groups.len(),
            );

            // BUG-405 срез 11: подстатьи `atlas` и `uniforms`. У атласа
            // предмет правки — объём (заливка целой текстуры против строк),
            // у uniform-слотов сначала надо понять, чем занята фаза: ростом
            // буфера, раскладкой по шагу 256 или самой отправкой.
            eprintln!(
                "[frame:wgpu]   prep-sub: atlas {} строк / {} KiB | \
                 uni grow {:.2} build {:.2} write {:.2} | слотов {}",
                atlas_frame_rows,
                atlas_frame_bytes / 1024,
                ms(t_uni_grow),
                ms(t_uni_build),
                ms(t_uni_write),
                clip_slots.len(),
            );

            // BUG-405 срез 13: попадания кэша укладки текста нарастающим
            // итогом. На уровне 2, без таймеров: разбивка `text-sub` стоит
            // около трети измеряемой ею статьи, а решение «кэш работает или
            // нет» принимается по счётчику, а не по секундомеру.
            eprintln!(
                "[frame:wgpu]   text-runs: попаданий {} промахов {} | планов {} слов",
                self.text_run_cache.hits,
                self.text_run_cache.misses,
                self.text_run_cache.stored,
            );

            // LUMEN_FRAME_LOG=3 — распределение, а не среднее.
            if item_log {
                let d_created = load_counter(&TEXTURES_CREATED) - tex_created_at_entry;
                let d_nanos = load_counter(&TEXTURE_CREATE_NANOS) - tex_nanos_at_entry;
                let d_hits = load_counter(&TEXTURE_POOL_HITS) - pool_hits_at_entry;
                let d_misses = load_counter(&TEXTURE_POOL_MISSES) - pool_misses_at_entry;
                eprintln!(
                    "[frame:wgpu]   alloc: this frame created {d_created} tex in {:.2}ms | \
                     pool hit {d_hits} miss {d_misses}",
                    d_nanos as f64 / 1e6,
                );
                // Перепись «кто создаёт текстуры» (суммарно за процесс,
                // вопрос п.23): топ-8 по количеству, с размерами.
                if let Some(census) = TEXTURE_CENSUS.get()
                    && let Ok(m) = census.lock()
                {
                    let mut rows: Vec<_> = m.iter().map(|(k, n)| (*k, *n)).collect();
                    rows.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
                    let s = rows
                        .iter()
                        .take(8)
                        .map(|((l, w, h), n)| format!("{l} {w}x{h} x{n}"))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("[frame:wgpu]   alloc-census (total): {s}");

                    // BUG-405 срез 26: та же перепись, свёрнутая ПО МЕТКЕ.
                    // Строка выше — топ-8 по ключу `(label, w, h)`, и на
                    // реальной странице её занимают картинки: каждая своего
                    // размера, то есть свой ключ со счётчиком 1. Метка,
                    // создавшая три текстуры трёх разных размеров, из топ-8
                    // при этом вытесняется целиком — а вопрос п.54 («сколько
                    // слоёв уровня создаётся за сессию») ставится именно к
                    // метке, а не к размеру.
                    // `rows` уже отсортирован по убыванию счётчика, поэтому
                    // размерные классы внутри метки приходят в том же порядке.
                    let mut by_label: Vec<(&'static str, u64, Vec<String>)> = Vec::new();
                    for ((l, w, h), n) in &rows {
                        let size = format!("{w}x{h} x{n}");
                        match by_label.iter_mut().find(|(name, _, _)| name == l) {
                            Some(e) => {
                                e.1 += n;
                                e.2.push(size);
                            }
                            None => by_label.push((l, *n, vec![size])),
                        }
                    }
                    by_label.sort_by_key(|&(_, n, _)| std::cmp::Reverse(n));
                    let s = by_label
                        .iter()
                        .map(|(l, n, sizes)| {
                            format!(
                                "{l} x{n} ({} разм.: {}{})",
                                sizes.len(),
                                sizes.iter().take(4).cloned().collect::<Vec<_>>().join(", "),
                                if sizes.len() > 4 { ", …" } else { "" },
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("[frame:wgpu]   alloc-census (по метке): {s}");
                }

                // Порядок повторяет `plan_kind` выше; `rclip` — общий слот 6
                // обоих клип-композитов (RRect/Path). Раньше его в таблице не
                // было, и `LUMEN_FRAME_LOG=3` ронял окно на первом же клипе.
                const KIND: [&str; 7] =
                    ["draw", "comp", "mask", "filt", "bdrop", "mlayer", "rclip"];

                // Гистограмма по длительности: «дорог каждый пасс» против
                // «дороги единицы пассов» различаются здесь и только здесь.
                let mut buckets = [0u32; 5]; // <0.05, <0.2, <1, <5, >=5 ms
                for (_, _, dur, _, _, _, _) in &items_prof {
                    let m = ms(*dur);
                    let b = if m < 0.05 {
                        0
                    } else if m < 0.2 {
                        1
                    } else if m < 1.0 {
                        2
                    } else if m < 5.0 {
                        3
                    } else {
                        4
                    };
                    buckets[b] += 1;
                }
                eprintln!(
                    "[frame:wgpu]   items {} | hist <0.05ms {} <0.2ms {} <1ms {} <5ms {} >=5ms {}",
                    items_prof.len(),
                    buckets[0], buckets[1], buckets[2], buckets[3], buckets[4],
                );

                // BUG-405 срез 2: последовательность пассов В ПОРЯДКЕ ПЛАНА.
                // Топ-12 отсортирован по времени и потому не отвечает на
                // вопрос «дорогие пассы стоят подряд или размазаны» — а
                // «стоят подряд в начале» означает ожидание, а не работу.
                eprintln!(
                    "[frame:wgpu]   seq: {}",
                    items_prof
                        .iter()
                        .map(|(k, _, d, _, o, kinds, desc)| format!(
                            "{}{}{}[{}]:{:.2}",
                            KIND.get(*k).unwrap_or(&"?"),
                            o,
                            kinds,
                            desc,
                            ms(*d)
                        ))
                        .collect::<Vec<_>>()
                        .join(" "),
                );

                let mut top = items_prof.clone();
                top.sort_unstable_by_key(|i| std::cmp::Reverse(i.2));
                let shown = top.len().min(12);
                let top_sum: f64 = top[..shown].iter().map(|i| ms(i.2)).sum();
                let all_sum: f64 = top.iter().map(|i| ms(i.2)).sum();
                eprintln!(
                    "[frame:wgpu]   top {shown} items = {top_sum:.1}ms of {all_sum:.1}ms encode"
                );
                for (kind, level, dur, pass_end, ops, kinds, _) in &top[..shown] {
                    let lvl = if *level == usize::MAX {
                        "-".to_string()
                    } else {
                        level.to_string()
                    };
                    eprintln!(
                        "[frame:wgpu]     {:<6} lvl {:<3} {:7.2}ms  (drop(pass) {:6.2}ms, ops {} [{}])",
                        KIND.get(*kind).unwrap_or(&"?"), lvl, ms(*dur), ms(*pass_end), ops, kinds,
                    );
                }
            }
            FRAME_LOG_NANOS.fetch_add(
                t_log0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        // Финализация по режиму: Band — служебный оффскрин-проход, не кадр
        // (не считаем и хэш не трогаем); Compose — настоящий кадр, но его
        // хэш фиксирует вызывающий render() (хэш Compose-аргументов кадр не
        // описывает); Normal — кадр и хэш, как раньше.
        match mode {
            RenderPassMode::Band { .. } | RenderPassMode::OverlayCache { .. } => {}
            RenderPassMode::Compose => {
                FRAMES_RENDERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            RenderPassMode::Normal { frame_hash } => {
                FRAMES_RENDERED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.last_frame_hash = Some(frame_hash);
            }
        }
        // In headless mode, keep the rendered texture alive for render_to_image().
        self.pending_readback = headless_tex;
        Ok(())
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
    fn render_to_image_with_overlay(
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

mod sticky_geom;
use sticky_geom::{
    apply_affine_to_grad_verts, apply_transform_to_clip, leaf_is_offscreen, sticky_bound,
    sticky_offset_dx, sticky_offset_dy, translate_rect, VertexPos, COVERAGE_CACHE_MAX_VERTS,
};

mod glyph_raster;
use glyph_raster::{
    count_svg_soup, push_text_glyphs, push_text_glyphs_mixed, rotate_text_vertices_cw, sub_add,
    sub_timer, TextRunCache, SVG_SUB, TEXT_SUB,
};
#[cfg(test)]
use glyph_raster::ensure_glyph;

mod paint_primitives;
use paint_primitives::{
    antialias_fill_soup, apply_affine_to_circle_verts, apply_affine_to_rrect_verts,
    apply_affine_to_verts, apply_alpha_to_color, as_bytes, axis_aligned_scale, bits_hash,
    blend_mode_to_u32, block_on, box_aspect, build_grad_params, color_to_array, convert_to_rgba,
    coverage_cache_arm, css_rect_to_device_scissor, emit_border_arc, emit_border_side,
    emit_outline_side, emit_svg_shape, intersect_rects, linear_gradient_uv_endpoints,
    make_blend_mode_param_buf, make_blur_param_buf, make_filter_param_buf,
    normalize_variation_axes, path_clip_params, push_bounded_quad, push_composite_quad,
    push_cross_fade_quad, push_fill_quad, push_grad_quad, push_image_quad, push_path_clip_quad,
    push_region_dst_quad, push_region_src_quad, push_rrect_clip_quad, push_rrect_quad,
    push_shadow_quad, radial_gradient_uv_params, resolve_gradient_stops,
    rotated_rect_clip_params, rotates_axes_2d, shader_rrect_clip_allowed, svg_shape_verts,
    sync_scissor_to_stack, transform_is_translation, transformed_grad_quad,
    translate_clip_shape, write_grad_buffer, CoverageCache, DeviceScissor, SvgShapeCache,
};
#[cfg(test)]
use paint_primitives::current_blend_mode;

#[cfg(test)]
mod tests;
