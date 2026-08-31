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

impl Renderer {

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
}

mod types;
// Реэкспортированы `pub`, а не приватным `use`, потому что `lib.rs` делает
// `pub use renderer::{Renderer, ComposeOutcome, ...}` (часть публичного API
// крейта) и `backends/wgpu_backend.rs` делает `use crate::renderer::Renderer;`
// напрямую — были `pub struct`/`pub enum`/`pub fn` на верхнем уровне
// renderer.rs до вырезки, RN-9.
pub use types::{ComposeOutcome, ImageRegisterError, Renderer, SnapshotUploadError, last_compose};
use types::{
    build_face_metrics, filter_fn_to_entry, layer_color, resolve_palette, BandStrip,
    BlurParamsCpu, CachedGlyph, CircleVertex, ComposeMarks, ComposePrep,
    CompositeVertex, CrossFadeVertex, DrawOp, FaceMetrics, FillVertex, FilterEntryCpu,
    FilterParamsCpu, GpuImage, GpuLayerSnapshot, GradHeaderCpu, GradParamsCpu, GradStopCpu,
    GradVertex, ImageVertex, LazyParsedFaces, LoadedFace, MaskVertex, OffscreenLayer,
    OverlayCache, PageBandCache, ParsedFace, PathClipParamsCpu, PathClipVertex,
    PendingBaseBlit, RRectClipVertex, RRectVertex, RenderPassMode, ShadowVertex, TextVertex,
};
// `ColorTables` зовётся только тестом `renderer/tests/sticky_colr_font.rs`
// (`use super::super::*;`) — не самим renderer.rs, поэтому реэкспорт
// cfg-гейтнут, приём SH-4b/BT-5/.../RN-2/RN-3/RN-5/RN-8.
#[cfg(test)]
use types::ColorTables;

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

mod texture_pool;

mod pipelines;
use pipelines::{
    build_hot_pipelines, hot_pipelines_awaited_in_ctor, hot_pipelines_serial, spawn_hot_pipelines,
    HotDeps, HotDelivery, HotKind, HotPipelines, PipelineDeps, WarmedPipeline, HOT_KINDS,
};

mod band_compose;
#[cfg(test)]
use band_compose::{band_blit_quads, band_geometry, ring_advance_plan, RingStrip};

mod frame_entry;

mod construct;

mod diagnostics;
// Реэкспортированы `pub`, а не приватным `use`, потому что `lib.rs` делает
// `pub use renderer::{load_counter, DL_EPOCH_MISMATCHES, ...}` — часть
// публичного API крейта (были `pub static`/`pub fn` на верхнем уровне
// renderer.rs до вырезки, RN-8).
pub use diagnostics::{
    load_counter, DL_EPOCH_MISMATCHES, DL_FOLD_REUSED, FRAMES_RENDERED, FRAMES_SKIPPED,
    FRAME_LOG_NANOS, FRAME_PHASE_NANOS, POST_CACHE_NANOS, PRE_MARKS_NANOS,
};
use diagnostics::{
    anim_split_disabled, atlas_partial_upload_disabled, balanced_cut_at_or_after,
    band_bias_disabled, band_copy_usage_enabled, band_cull_height, band_draw_fraction,
    band_margin_override_css, band_pass_load_ops, band_warm_disabled, bbox_backdrop_disabled,
    bbox_filter_disabled, bbox_scissor_disabled, blur_merge_disabled, box_shadow_body,
    clip_slot_from, compose_overlay_disabled, count_texture_created_labeled,
    create_depth_texture, cull_merge_disabled, dl_epoch_disabled, dl_epoch_verify,
    dual_hash_disabled, flush_compose_marks, frame_skip_disabled, image_mips_disabled,
    img_xform_disabled, nested_shader_clip_disabled, no_clip_slot,
    overlay_cache_disabled, pipeline_warmup_disabled, requested_max_texture_dim,
    resolve_text_face_ids, rot_aa_disabled, rot_clip_disabled, scroll_compositor_disabled,
    select_surface_format, shader_rrect_clip_disabled, shadow_analytic_disabled,
    skip_signature, split_submit_disabled, state_elision_disabled, svg_aa_disabled,
    text_face_slot, text_sig_level, texture_limit_raise_disabled, timed_log,
    BAND_BLIT_BGS_CREATED, ClipContour, ClipUniformSlot, ContentFoldMemo,
    GLYPHS_RASTERIZED, GLYPH_RASTER_NANOS, NO_TEXT_FACE, OVERLAY_PREV,
    SHADER_CLIP_MAX_CONTOURS, SUBMIT_CHUNK_ITEMS,
    TEXTURES_CREATED, TEXTURE_CENSUS, TEXTURE_CREATE_NANOS, TEXTURE_POOL_HITS,
    TEXTURE_POOL_MISSES, UNIFORM_SLOT_STRIDE,
};
#[cfg(test)]
use diagnostics::band_pass_load_choice;

#[cfg(test)]
mod tests;
