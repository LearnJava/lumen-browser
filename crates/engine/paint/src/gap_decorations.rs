//! CSS Gap Decorations L1 — visual rules rendered inside flex/grid/multicol gaps.
//!
//! Phase 0: geometry and emit logic.
//! Phase 1 (P4): wire `column-rule-*` (vertical segments) and `row-rule-*`
//! (horizontal segments) from `ComputedStyle` into one `GapDecorationContext`
//! per axis and call `emit_gap_rules()` for each.

use lumen_core::geom::Rect;
use lumen_layout::style::WritingMode;
use lumen_layout::{BorderStyle, Color, LayoutBox};

use crate::display_list::{CornerRadii, DisplayCommand};

/// Parameters for gap rule rendering.
///
/// P4 constructs one per axis from `ComputedStyle`: `column_rule_*` for column
/// gaps, `row_rule_*` for row gaps, and passes it to [`emit_gap_rules`].
///
/// // CSS: column-rule-width/style/color, row-rule-width/style/color
pub struct GapDecorationContext {
    /// Thickness of the rule line in CSS px.
    pub rule_width: f32,
    /// Visual style of the rule (matches `<line-style>` grammar).
    pub rule_style: BorderStyle,
    /// Resolved rule color (currentColor already resolved).
    pub rule_color: Color,
}

/// One inter-cell gap in a flex, grid, or multicol layout.
///
/// Each `GapSegment` covers the full gap rectangle; `emit_gap_rules` centers
/// the rule within it.
pub struct GapSegment {
    /// The gap rectangle in layout coordinates (px from viewport top-left).
    /// For column gaps this spans the full container height; for row gaps
    /// it spans the full container width.
    pub rect: Rect,
    /// `true` → row gap (horizontal rule drawn between two rows).
    /// `false` → column gap (vertical rule drawn between two columns).
    pub horizontal: bool,
    /// Номер щели в своей оси (по возрастанию координаты, с 0): значения списков
    /// `*-rule-*` раздаются по нему (§4.6). У щели, разрезанной на куски, он общий.
    pub gap: usize,
}

/// CSS Gap Decorations L1 §3.3 — сдвигает концы отрезка щели вдоль её оси.
///
/// `origin`/`len` — протяжённость отрезка по оси щели; `start`/`end` — вычисленные
/// `*-rule-inset-cap-start/-end` в px (положительное укорачивает, отрицательное
/// удлиняет). `reversed` — ось идёт справа налево (`row-rule` при `direction: rtl`):
/// «начало» тогда у правого края. Возвращает `None`, если отрезок схлопнулся.
pub fn inset_span(origin: f32, len: f32, start: f32, end: f32, reversed: bool) -> Option<(f32, f32)> {
    let (lo, hi) = if reversed { (end, start) } else { (start, end) };
    let new_len = len - lo - hi;
    (new_len > 0.0).then_some((origin + lo, new_len))
}

/// Emits [`DisplayCommand::DrawBorder`] entries for gap decorations between
/// flex/grid/multicol cells.
///
/// `_boxes` — positioned child boxes (reserved for Phase 1 gap-position
/// inference; currently ignored, gaps are passed explicitly).
/// `gaps` — gap segments to decorate.
/// `ctx` — decoration context (width, style, color).
///
/// Returns an empty `Vec` when:
/// - `ctx.rule_style` is `BorderStyle::None`, or
/// - `ctx.rule_width` ≤ 0.
///
/// Rules are centered on each gap rectangle; a rule wider than its gap (or a gap of
/// zero width) overflows it on both sides.
///
/// Phase 0: Solid/Dashed/Dotted are fully supported. Double and other styles
/// render as Solid (same behaviour as `emit_column_rules`).
pub fn emit_gap_rules(
    _boxes: &[LayoutBox],
    gaps: &[GapSegment],
    ctx: &GapDecorationContext,
) -> Vec<DisplayCommand> {
    if !ctx.rule_style.is_visible() || ctx.rule_width <= 0.0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(gaps.len());

    for gap in gaps {
        // Нулевая ширина щели (`gap: 0`) — нормальный случай: линия центрируется на шве
        // элементов (`grid-gap-decorations-042`); пустой кусок вдоль оси не рисуется.
        let along = if gap.horizontal { gap.rect.width } else { gap.rect.height };
        if along <= 0.0 {
            continue;
        }

        // Линия шире щели не обрезается по ней: Chromium центрирует её и пускает поверх
        // соседних элементов.
        if gap.horizontal {
            // Row gap: a horizontal rule centered vertically in the gap.
            let rule_h = ctx.rule_width;
            let rule_y = gap.rect.y + (gap.rect.height - rule_h) * 0.5;
            out.extend(rule_line_commands(
                Rect::new(gap.rect.x, rule_y, gap.rect.width, rule_h),
                true,
                ctx.rule_style,
                ctx.rule_color,
            ));
        } else {
            // Column gap: a vertical rule centered horizontally in the gap.
            let rule_w = ctx.rule_width;
            let rule_x = gap.rect.x + (gap.rect.width - rule_w) * 0.5;
            out.extend(rule_line_commands(
                Rect::new(rule_x, gap.rect.y, rule_w, gap.rect.height),
                false,
                ctx.rule_style,
                ctx.rule_color,
            ));
        }
    }

    out
}

/// A solid rule lies on whole device pixels: both edges are rounded (a tie goes up, as
/// Chromium snaps the reftest rules: `top: 212.5px` + 5px fills rows 213..217) and the line keeps at least one pixel. A fractional rect
/// would be anti-aliased over two columns, while Chromium paints one crisp line
/// (`css-gaps/multicol/multicol-gap-decorations-017`, flex 040/042/043/044/056/058).
fn snap_rule_rect(rect: Rect) -> Rect {
    let snap = |v: f32| (v + 0.5).floor();
    let (x0, y0) = (snap(rect.x), snap(rect.y));
    let x1 = snap(rect.x + rect.width).max(x0 + 1.0);
    let y1 = snap(rect.y + rect.height).max(y0 + 1.0);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// One `DrawBorder` that paints only the bottom (`horizontal`) or right side of `rect`.
fn rule_side_border(rect: Rect, horizontal: bool, style: BorderStyle, color: Color) -> DisplayCommand {
    let none = BorderStyle::None;
    let rect = if style == BorderStyle::Solid { snap_rule_rect(rect) } else { rect };
    if horizontal {
        // Renderer draws the bottom side at rect.y + rect.height - widths[2].
        DisplayCommand::DrawBorder {
            rect,
            widths: [0.0, 0.0, rect.height, 0.0],
            colors: [Color::TRANSPARENT, Color::TRANSPARENT, color, Color::TRANSPARENT],
            styles: [none, none, style, none],
            radii: CornerRadii::default(),
        }
    } else {
        // Renderer draws the right side at rect.x + rect.width - widths[1].
        DisplayCommand::DrawBorder {
            rect,
            widths: [0.0, rect.width, 0.0, 0.0],
            colors: [Color::TRANSPARENT, color, Color::TRANSPARENT, Color::TRANSPARENT],
            styles: [none, style, none, none],
            radii: CornerRadii::default(),
        }
    }
}

/// CSS Gap Decorations L1 §4.2 / CSS Backgrounds L3 §4.2: the commands that paint one
/// rule line occupying `rect` (`horizontal` — a row gap's rule, `false` — a column gap's).
///
/// `groove`/`ridge` are two half-width bands (a dark and a light shade of the colour);
/// a gap rule has no inside, so `inset` paints like `ridge` and `outset` like `groove`.
/// Every other visible style is one solid/dashed/dotted/double side.
pub fn rule_line_commands(rect: Rect, horizontal: bool, style: BorderStyle, color: Color) -> Vec<DisplayCommand> {
    let ridge = match style {
        BorderStyle::Groove | BorderStyle::Outset => false,
        BorderStyle::Ridge | BorderStyle::Inset => true,
        other => return vec![rule_side_border(rect, horizontal, other, color)],
    };
    let (dark, light) = groove_shades(color);
    // Like the bottom/right side of a bordered box: the outer half is `floor(w / 2)`,
    // the inner one the rest (Edge: `border-bottom: 5px groove` = 3px dark, then 2px light).
    let (outer_color, inner_color) = if ridge { (dark, light) } else { (light, dark) };
    let extent = if horizontal { rect.height } else { rect.width };
    let outer = (extent * 0.5).floor();
    if outer <= 0.0 || extent - outer <= 0.0 {
        // A 1px line has no room for two bands: Edge paints it in the colour itself.
        return vec![rule_side_border(rect, horizontal, BorderStyle::Solid, color)];
    }
    // The rule is the bottom/right side of a box: the inner band (the larger one) comes first
    // along the axis, the outer one last.
    let (inner_rect, outer_rect) = if horizontal {
        (
            Rect::new(rect.x, rect.y, rect.width, extent - outer),
            Rect::new(rect.x, rect.y + extent - outer, rect.width, outer),
        )
    } else {
        (
            Rect::new(rect.x, rect.y, extent - outer, rect.height),
            Rect::new(rect.x + extent - outer, rect.y, outer, rect.height),
        )
    };
    vec![
        rule_side_border(inner_rect, horizontal, BorderStyle::Solid, inner_color),
        rule_side_border(outer_rect, horizontal, BorderStyle::Solid, outer_color),
    ]
}

/// Luminance (linear sRGB, Rec. 709) below which a `groove`/`ridge` colour counts as
/// «dark» and is lightened instead of darkened — Chromium's `kBaseDarkColorLuminance`
/// (the luminance of `#202020`; measured in Edge: `#202020` is dark, `#212121` is not).
const DARK_COLOR_LUMINANCE: f32 = 0.014_443_844;

/// Luminance from which the «light» shade of a `groove`/`ridge` colour is the colour itself
/// (Edge: `#ececec` keeps its colour, `#ebebeb` is lightened to white).
const LIGHT_COLOR_LUMINANCE: f32 = 0.835;

fn linear_luminance(c: Color) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// Chromium `Color::Light()`: every channel scaled so the largest one gains `0.33`
/// (clamped at 1); black becomes `#545454`. Channels are truncated, not rounded.
fn lightened(c: Color) -> Color {
    let v = c.r.max(c.g).max(c.b) as f32 / 255.0;
    if v == 0.0 {
        return Color { r: 0x54, g: 0x54, b: 0x54, a: c.a };
    }
    let m = (v + 0.33).min(1.0) / v;
    let ch = |x: u8| (m * (x as f32 / 255.0) * 255.999_97) as u8;
    Color { r: ch(c.r), g: ch(c.g), b: ch(c.b), a: c.a }
}

/// Chromium `Color::Dark()`: the largest channel loses `0.33` (clamped at 0); truncated.
fn darkened(c: Color) -> Color {
    let v = c.r.max(c.g).max(c.b) as f32 / 255.0;
    if v == 0.0 {
        return c;
    }
    let m = ((v - 0.33) / v).max(0.0);
    let ch = |x: u8| (m * (x as f32 / 255.0) * 255.999_97) as u8;
    Color { r: ch(c.r), g: ch(c.g), b: ch(c.b), a: c.a }
}

/// `(dark, light)` shades of a `groove`/`ridge`/`inset`/`outset` colour as Chromium/Edge
/// derive them (`BoxBorderPainter::CalculateBorderStyleColor`): a very dark colour gets two
/// lighter shades (`black` → `#545454` / `#A8A8A8`), any other one its `Dark()` and either
/// itself (light colours) or its `Light()`. Alpha is kept.
pub(crate) fn groove_shades(c: Color) -> (Color, Color) {
    let lum = linear_luminance(c);
    if lum <= DARK_COLOR_LUMINANCE {
        return (lightened(c), lightened(lightened(c)));
    }
    (darkened(c), if lum >= LIGHT_COLOR_LUMINANCE { c } else { lightened(c) })
}

/// Допуск сравнения границ дорожек с рёбрами элементов (px, float-округление layout).
const TRACK_TOL: f32 = 0.5;

/// Диапазон дорожек `[first, last]` (включительно), который занимает отрезок `[lo, hi]`
/// по оси. `tops` — координаты начала щелей по возрастанию, `gap` — их ширина: дорожка `t`
/// лежит между `tops[t-1] + gap` и `tops[t]` (первая и последняя открыты наружу).
pub fn track_span(lo: f32, hi: f32, tops: &[f32], gap: f32) -> (usize, usize) {
    let n = tops.len() + 1;
    // При `gap: 0` соседние дорожки смыкаются на `tops[t]`: край элемента на шве принадлежит
    // той дорожке, которую элемент заканчивает/начинает, а не обеим.
    let seam = if gap > TRACK_TOL { TRACK_TOL } else { -TRACK_TOL };
    let first = (0..n).find(|&t| t == n - 1 || lo <= tops[t] + seam).unwrap_or(0);
    let last = (0..n).rev().find(|&t| t == 0 || hi >= tops[t - 1] + gap - seam).unwrap_or(0);
    (first, last.max(first))
}

/// Элемент сетки как диапазоны дорожек: `t` — поперёк щели (колонки для колоночной щели),
/// `a` — вдоль щели (строки для колоночной щели); границы включительно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridItemSpan {
    pub t0: usize,
    pub t1: usize,
    pub a0: usize,
    pub a1: usize,
}

/// Конец куска линии вдоль щели.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceEnd {
    /// Край контейнера.
    Edge,
    /// Стык с перпендикулярной щелью `k` (между дорожками `k` и `k + 1` вдоль оси щели).
    Junction(usize),
}

/// Кусок линии одной щели (CSS Gap Decorations L1 §3.1.2) в координатах вдоль её оси.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapPiece {
    /// Номер щели поперёк оси (между дорожками `gap` и `gap + 1`).
    pub gap: usize,
    pub lo: f32,
    pub hi: f32,
    pub lo_end: PieceEnd,
    pub hi_end: PieceEnd,
}

/// CSS Gap Decorations L1 §3.1–§3.4 для grid-контейнера: режет щели на куски по
/// `*-rule-break` и скрывает куски у пустых областей по `*-rule-visibility-items`.
///
/// `n_t`/`n_a` — число дорожек поперёк/вдоль оси щели; `items` — элементы в этих
/// индексах; `a_tops` — начала перпендикулярных щелей вдоль оси (`n_a - 1` штук),
/// `a_gap` — их ширина; `[a_lo, a_hi]` — протяжённость контейнера вдоль оси.
///
/// * `none` — одна линия от края до края (позади элементов тоже);
/// * `normal` (grid) — разрыв на «Т»-стыках (перпендикулярная щель только с одной
///   стороны), сквозь «крест» линия идёт;
/// * `intersection` — разрыв на любом стыке;
/// * любой режим, кроме `none`, обрывает линию там, где щель пересекает элемент.
///
/// Куски возвращаются по возрастанию номера щели, внутри щели — вдоль оси.
#[allow(clippy::too_many_arguments)]
pub fn grid_gap_pieces(
    n_t: usize,
    n_a: usize,
    items: &[GridItemSpan],
    a_tops: &[f32],
    a_gap: f32,
    a_lo: f32,
    a_hi: f32,
    brk: lumen_layout::RuleBreak,
    vis: lumen_layout::RuleVisibilityItems,
) -> Vec<GapPiece> {
    use lumen_layout::{RuleBreak, RuleVisibilityItems};
    if n_t < 2 || n_a == 0 {
        return Vec::new();
    }
    // occ[a][t] — клетка занята; across[g][a] — элемент пересекает щель g в дорожке a;
    // along[k][t] — элемент пересекает перпендикулярную щель k в дорожке t.
    let mut occ = vec![vec![false; n_t]; n_a];
    let mut across = vec![vec![false; n_a]; n_t - 1];
    let mut along = vec![vec![false; n_t]; n_a.saturating_sub(1)];
    for it in items {
        let (t1, a1) = (it.t1.min(n_t - 1), it.a1.min(n_a - 1));
        for a in it.a0..=a1 {
            for cell in occ[a].iter_mut().take(t1 + 1).skip(it.t0) {
                *cell = true;
            }
            for row in across.iter_mut().take(t1).skip(it.t0) {
                row[a] = true;
            }
        }
        for row in along.iter_mut().take(a1).skip(it.a0) {
            for cell in row.iter_mut().take(t1 + 1).skip(it.t0) {
                *cell = true;
            }
        }
    }
    let end_of = |a: usize| if a + 1 >= n_a { (a_hi, PieceEnd::Edge) } else { (a_tops[a], PieceEnd::Junction(a)) };
    let start_of = |a: usize| if a == 0 { (a_lo, PieceEnd::Edge) } else { (a_tops[a - 1] + a_gap, PieceEnd::Junction(a - 1)) };

    let mut out = Vec::new();
    for g in 0..n_t - 1 {
        let visible = |a: usize| {
            let (l, r) = (occ[a][g], occ[a][g + 1]);
            let shown = match vis {
                RuleVisibilityItems::Around => l || r,
                RuleVisibilityItems::Between => l && r,
                RuleVisibilityItems::All | RuleVisibilityItems::Normal => true,
            };
            shown && (brk == RuleBreak::None || !across[g][a])
        };
        // Разрыв на стыке `k`: сколько сторон щели `g` имеют перпендикулярную щель.
        let breaks_at = |k: usize| {
            let sides = usize::from(!along[k][g]) + usize::from(!along[k][g + 1]);
            match brk {
                RuleBreak::None => false,
                RuleBreak::Intersection => sides > 0,
                // «Т» с щелью-перекладиной (перпендикулярная щель примыкает только с одной
                // стороны) линию не режет: обрывается примыкающая щель-ножка, а она кончается
                // там, где элемент пересекает её саму (`across`); см. `grid-gap-decorations-009`.
                RuleBreak::Normal => false,
            }
        };
        let mut start: Option<usize> = None;
        for a in 0..n_a {
            if visible(a) && start.is_none() {
                start = Some(a);
            }
            let Some(s) = start else { continue };
            let goes_on = a + 1 < n_a && visible(a + 1) && !breaks_at(a);
            if visible(a) && !goes_on {
                let ((lo, lo_end), (hi, hi_end)) = (start_of(s), end_of(a));
                out.push(GapPiece { gap: g, lo, hi, lo_end, hi_end });
                start = None;
            }
        }
    }
    out
}

/// Сегменты щелей grid-контейнера плюс число щелей каждой оси (куски одной щели
/// делят номер; щель, у которой все куски скрыты, всё равно занимает значение списка).
pub struct GridGapGeometry {
    pub segments: Vec<GapSegment>,
    pub col_total: usize,
    pub row_total: usize,
    /// Колоночные щели нумеруются справа налево (grid при `direction: rtl`); у flex номера
    /// уже розданы в порядке размещения.
    pub column_reversed: bool,
}

/// Щель оси `subgrid`, восстановленная по элементам (Grid L2 §9: дорожки и щели subgrid —
/// часть родительских). Раскладка берёт щель родителя и игнорирует собственные
/// `column-gap`/`row-gap` subgrid'а, поэтому в стиле контейнера она нулевая или другая:
/// настоящая щель — наименьший положительный зазор «правое ребро одного элемента → левое
/// ребро другого» (элементы, не заполняющие дорожку, дают зазор не меньше щели).
/// Элементы вплотную (правое ребро одного = левое другого, дорожки смыкаются) — щель родителя
/// нулевая, как бы далеко ни стояли другие элементы:
/// `Some(0.0)`, а не собственный `gap` subgrid'а (`subgrid-gap-decorations-014/018`).
/// `None`, если об щели ничего не известно (один элемент).
pub fn subgrid_axis_gap(edges: &[(f32, f32)]) -> Option<f32> {
    const MIN_GAP: f32 = 0.5;
    let mut best: Option<f32> = None;
    let mut touching = false;
    for &(lo0, hi) in edges {
        for &(lo, _) in edges {
            let d = lo - hi;
            if d > MIN_GAP && best.is_none_or(|b| d < b) {
                best = Some(d);
            }
            touching |= d.abs() <= MIN_GAP && hi > lo0 + MIN_GAP;
        }
    }
    if touching { Some(0.0) } else { best }
}

/// Параметры [`grid_gap_segments`].
pub struct GridGapParams<'a> {
    /// Content box контейнера: `(x, y, width, height)`.
    pub content: (f32, f32, f32, f32),
    pub col_gap: f32,
    pub row_gap: f32,
    pub column_visible: bool,
    pub row_visible: bool,
    /// Ось колонок/строк — `subgrid`: дорожки родительские, у контейнера их нет в шаблоне, и
    /// щели ищутся по рёбрам элементов, включая щели рядом с пустыми дорожками.
    pub subgrid_cols: bool,
    pub subgrid_rows: bool,
    /// Дорожки оси `subgrid`, унаследованные от родителя (`LayoutBox::subgrid_tracks`), в
    /// абсолютных координатах: щели берутся из них, а не из рёбер элементов, поэтому находятся и
    /// у пустого subgrid'а. `None` — раскладка дорожек не отдала.
    pub subgrid_col_tracks: Option<Vec<(f32, f32)>>,
    pub subgrid_row_tracks: Option<Vec<(f32, f32)>>,
    pub style: &'a lumen_layout::ComputedStyle,
}

/// Начала щелей оси: правые рёбра элементов, за которыми на расстоянии `gap` начинается
/// другой элемент (так же, как это делают flex-щели). По возрастанию, без дублей.
fn gap_starts(edges: &[(f32, f32)], gap: f32) -> Vec<f32> {
    const EPS: f32 = 1.5;
    let mut ends: Vec<f32> = edges.iter().map(|&(_, hi)| hi).collect();
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ends.dedup_by(|a, b| (*a - *b).abs() < EPS);
    ends.into_iter().filter(|e| edges.iter().any(|&(lo, _)| (lo - (e + gap)).abs() < EPS)).collect()
}

/// Начала щелей оси `subgrid`, у которой нет своего шаблона дорожек. Помимо пар «ребро → элемент
/// через `gap`» (`gap_starts`) щель выдают и одиночные рёбра: правое ребро элемента левее конца
/// контейнера — конец дорожки, за ним идёт щель; левое ребро правее начала — начало дорожки,
/// перед ним щель. Так находятся щели у пустых клеток, к которым не примыкает ни один элемент,
/// и щели, которые перекрывает элемент-«мост» (`subgrid-gap-decorations-023/024`).
/// `lo`/`hi` — протяжённость оси (content box). По возрастанию, без дублей.
fn subgrid_gap_starts(edges: &[(f32, f32)], gap: f32, lo: f32, hi: f32) -> Vec<f32> {
    const EPS: f32 = 1.5;
    let mut tops = gap_starts(edges, gap);
    if gap > TRACK_TOL {
        for &(a, b) in edges {
            if b + gap <= hi + EPS && b < hi - EPS {
                tops.push(b);
            }
            if a - gap >= lo - EPS && a > lo + EPS {
                tops.push(a - gap);
            }
        }
    }
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    tops.dedup_by(|a, b| (*a - *b).abs() < EPS);
    tops
}

/// Дорожки оси как `(начало, конец)` по возрастанию, если их можно взять из шаблона:
/// `grid-template-*` целиком из фиксированных длин (`px`/`em`/`%`), без `repeat(auto-*)`.
/// Так щели находятся и там, где ни один элемент не примыкает к соседу (пустые дорожки,
/// элементы уже своей клетки). Возвращает `None`, если шаблон другой или дорожки не
/// совпадают с рёбрами элементов (`justify-content`, неявные дорожки, subgrid) — тогда
/// остаётся восстановление по элементам.
///
/// `start` — где начинается ось, `extent` — её длина (content box), `reversed` — ось идёт
/// справа налево (`direction: rtl`: первая дорожка у правого края).
#[allow(clippy::too_many_arguments)]
fn template_tracks(
    template: &[lumen_layout::GridTrackSize],
    has_auto_repeat: bool,
    start: f32,
    extent: f32,
    gap: f32,
    reversed: bool,
    em: f32,
    vp: lumen_core::geom::Size,
    edges: &[(f32, f32)],
) -> Option<Vec<(f32, f32)>> {
    use lumen_layout::GridTrackSize;
    if has_auto_repeat || template.len() < 2 {
        return None;
    }
    let mut sizes = Vec::with_capacity(template.len());
    for t in template {
        match t {
            GridTrackSize::Length(l) => sizes.push(l.resolve(em, Some(extent), vp)?.max(0.0)),
            _ => return None,
        }
    }
    let total: f32 = sizes.iter().sum::<f32>() + gap * (sizes.len() - 1) as f32;
    let mut pos = if reversed { start + extent - total } else { start };
    if reversed {
        sizes.reverse();
    }
    let tracks: Vec<(f32, f32)> = sizes
        .iter()
        .map(|&w| {
            let t = (pos, pos + w);
            pos += w + gap;
            t
        })
        .collect();
    let on_track = |v: f32, pick: fn(&(f32, f32)) -> f32| tracks.iter().any(|t| (pick(t) - v).abs() <= TRACK_TOL);
    edges
        .iter()
        .all(|&(lo, hi)| on_track(lo, |t| t.0) && on_track(hi, |t| t.1))
        .then_some(tracks)
}

/// Начала щелей фиксированных дорожек, написанных *перед* `repeat(auto-fit|auto-fill, …)`
/// (`100px repeat(auto-fit, 100px) 1fr`). Пустые дорожки повтора схлопнуты (CSS Grid L1
/// §7.2.3.2: размер 0, гутеры сливаются), поэтому первый элемент начинается ровно через
/// один `gap` после последней ведущей дорожки: тогда они (в том числе пустые) настоящие и
/// щели между ними видны. Иначе — `None`, и остаётся восстановление по элементам.
fn leading_tops(
    rep: Option<&lumen_layout::GridRepeat>,
    start: f32,
    extent: f32,
    gap: f32,
    em: f32,
    vp: lumen_core::geom::Size,
    first_item_lo: f32,
) -> Option<Vec<f32>> {
    use lumen_layout::GridTrackSize;
    let rep = rep.filter(|r| !r.before.is_empty())?;
    let mut tops = Vec::with_capacity(rep.before.len());
    let mut pos = start;
    for t in &rep.before {
        let GridTrackSize::Length(l) = t else { return None };
        pos += l.resolve(em, Some(extent), vp)?.max(0.0);
        tops.push(pos);
        pos += gap;
    }
    // `pos` — начало первой дорожки после ведущих.
    ((first_item_lo - pos).abs() <= TRACK_TOL).then_some(tops)
}

/// CSS Gap Decorations L1 §3 для grid-контейнера: щели, разрезанные по `*-rule-break`,
/// скрытые по `*-rule-visibility-items` и сдвинутые `*-rule-inset-*` (cap — у края
/// контейнера, junction — у стыка с перпендикулярной щелью).
///
/// Дорожки восстанавливаются из прямоугольников детей: щель — это пара «правое ребро
/// элемента → левое ребро другого через `gap`». Щель, рядом с которой нет ни одной пары
/// таких элементов (целиком пустая дорожка, нестретчнутые элементы), не находится — тот
/// же предел, что у flex-ветки.
pub fn grid_gap_segments(children: &[&LayoutBox], p: &GridGapParams<'_>) -> GridGapGeometry {
    if p.style.writing_mode != WritingMode::HorizontalTb {
        return vertical_grid_gap_segments(children, p);
    }
    grid_gap_segments_logical(children, p, p.content, None)
}

/// Grid в вертикальном `writing-mode` (CSS Gap Decorations L1 §2: `column-*` идёт по инлайновой
/// оси, `row-*` — по блоковой). Колонки лежат вдоль физической `y`, строки — вдоль `x`. Чтобы не
/// плодить вторую копию разбора дорожек и кусков, рамки детей отражаются в логическое
/// пространство «колонки слева направо, строки сверху вниз» (`x'` — инлайн от начала оси,
/// `y'` — блок от начала), щели считаются обычным путём, а готовые отрезки отражаются назад.
/// Поэтому номера щелей идут от начала оси (снизу для `sideways-lr`/`direction: rtl`, справа для
/// `vertical-rl`), как того требуют списки значений §4.6. `horizontal` у отрезков физический:
/// щель колонок рисуется горизонтальной линией. Subgrid в этом режиме не разбирается.
fn vertical_grid_gap_segments(children: &[&LayoutBox], p: &GridGapParams<'_>) -> GridGapGeometry {
    let s = p.style;
    let (cx, cy, cw, ch) = p.content;
    let rtl = s.direction == lumen_layout::Direction::Rtl;
    let inline_rev = (s.writing_mode == WritingMode::SidewaysLr) != rtl;
    let block_rl = matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl);
    let flip = Flip { cx, cy, cw, ch, inline_rev, block_rl };
    let mut out = grid_gap_segments_logical(children, p, (0.0, 0.0, ch, cw), Some(flip));
    for seg in &mut out.segments {
        let r = seg.rect;
        let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
        let (py0, py1) = if inline_rev { (cy + ch - x1, cy + ch - x0) } else { (cy + x0, cy + x1) };
        let (px0, px1) = if block_rl { (cx + cw - y1, cx + cw - y0) } else { (cx + y0, cx + y1) };
        seg.rect = Rect::new(px0, py0, px1 - px0, py1 - py0);
        seg.horizontal = !seg.horizontal;
    }
    out
}

/// Отражение физических рамок в логическое пространство вертикального grid-контейнера.
#[derive(Clone, Copy)]
struct Flip {
    cx: f32,
    cy: f32,
    cw: f32,
    ch: f32,
    inline_rev: bool,
    block_rl: bool,
}

impl Flip {
    /// Инлайновая протяжённость (логический `x'`) физического отрезка по `y`.
    fn inline(&self, lo: f32, hi: f32) -> (f32, f32) {
        if self.inline_rev { (self.cy + self.ch - hi, self.cy + self.ch - lo) } else { (lo - self.cy, hi - self.cy) }
    }

    /// Блоковая протяжённость (логический `y'`) физического отрезка по `x`.
    fn block(&self, lo: f32, hi: f32) -> (f32, f32) {
        if self.block_rl { (self.cx + self.cw - hi, self.cx + self.cw - lo) } else { (lo - self.cx, hi - self.cx) }
    }
}

/// Тело [`grid_gap_segments`] в логических координатах: `content` — content box в них, `flip` —
/// как получить логические рамки детей из физических (`None` — координаты уже логические).
fn grid_gap_segments_logical(
    children: &[&LayoutBox],
    p: &GridGapParams<'_>,
    content: (f32, f32, f32, f32),
    flip: Option<Flip>,
) -> GridGapGeometry {
    let s = p.style;
    let (cx, cy, cw, ch) = content;
    let em = s.font_size;
    let vp = lumen_core::geom::Size::new(cw, ch);
    let span = |c: &&LayoutBox, along_x: bool| {
        let (lo, hi) = if along_x { (c.rect.x, c.rect.x + c.rect.width) } else { (c.rect.y, c.rect.y + c.rect.height) };
        (lo, hi)
    };
    // Колонки (логический `x'`): физическая `x` в горизонтальном режиме, `y` — в вертикальном.
    let xs: Vec<(f32, f32)> = children
        .iter()
        .map(|c| match flip {
            None => span(c, true),
            Some(f) => {
                let (lo, hi) = span(c, false);
                f.inline(lo, hi)
            }
        })
        .collect();
    let ys: Vec<(f32, f32)> = children
        .iter()
        .map(|c| match flip {
            None => span(c, false),
            Some(f) => {
                let (lo, hi) = span(c, true);
                f.block(lo, hi)
            }
        })
        .collect();
    // Отражение уже учло `direction: rtl` вертикального режима.
    let vertical = flip.is_some();
    let rtl = !vertical && s.direction == lumen_layout::Direction::Rtl;
    // Subgrid в вертикальном режиме не разбирается: его дорожки лежат в физических координатах.
    let (subgrid_cols, subgrid_rows) = (p.subgrid_cols && !vertical, p.subgrid_rows && !vertical);
    let sub_col_tracks = p.subgrid_col_tracks.clone().filter(|_| !vertical);
    let sub_row_tracks = p.subgrid_row_tracks.clone().filter(|_| !vertical);
    let col_tracks = sub_col_tracks.or_else(|| {
        template_tracks(
            &s.grid_template_columns,
            s.grid_template_col_auto_repeat.is_some(),
            cx,
            cw,
            p.col_gap,
            rtl,
            em,
            vp,
            &xs,
        )
    });
    let row_tracks = sub_row_tracks.or_else(|| {
        template_tracks(
            &s.grid_template_rows,
            s.grid_template_row_auto_repeat.is_some(),
            cy,
            ch,
            p.row_gap,
            false,
            em,
            vp,
            &ys,
        )
    });
    let tops_of = |tracks: &Option<Vec<(f32, f32)>>, edges: &[(f32, f32)], gap: f32, sub: Option<(f32, f32)>| {
        match (tracks, sub) {
            (Some(t), _) => t[..t.len() - 1].iter().map(|x| x.1).collect(),
            (None, Some((lo, hi))) => subgrid_gap_starts(edges, gap, lo, hi),
            (None, None) => gap_starts(edges, gap),
        }
    };
    let mut col_tops = tops_of(&col_tracks, &xs, p.col_gap, subgrid_cols.then_some((cx, cx + cw)));
    let mut row_tops = tops_of(&row_tracks, &ys, p.row_gap, subgrid_rows.then_some((cy, cy + ch)));
    // `grid-template-*` с `repeat(auto-*)`: ведущие фиксированные дорожки до повтора.
    let min_lo = |edges: &[(f32, f32)]| edges.iter().map(|e| e.0).fold(f32::INFINITY, f32::min);
    let mut lead_lo = (None, None);
    if col_tracks.is_none() && !xs.is_empty() {
        let rep = s.grid_template_col_auto_repeat.as_ref();
        if let Some(mut lead) = leading_tops(rep, cx, cw, p.col_gap, em, vp, min_lo(&xs)).filter(|_| !rtl) {
            lead_lo.0 = Some(cx);
            lead.append(&mut col_tops);
            col_tops = lead;
        }
    }
    if row_tracks.is_none() && !ys.is_empty() {
        let rep = s.grid_template_row_auto_repeat.as_ref();
        if let Some(mut lead) = leading_tops(rep, cy, ch, p.row_gap, em, vp, min_lo(&ys)) {
            lead_lo.1 = Some(cy);
            lead.append(&mut row_tops);
            row_tops = lead;
        }
    }
    let (n_cols, n_rows) = (col_tops.len() + 1, row_tops.len() + 1);
    let spans: Vec<(usize, usize, usize, usize)> = xs
        .iter()
        .zip(&ys)
        .map(|(&(x0, x1), &(y0, y1))| {
            let (c0, c1) = track_span(x0, x1, &col_tops, p.col_gap);
            let (r0, r1) = track_span(y0, y1, &row_tops, p.row_gap);
            (c0, c1, r0, r1)
        })
        .collect();
    let mut out = GridGapGeometry {
        segments: Vec::new(),
        col_total: col_tops.len(),
        row_total: row_tops.len(),
        column_reversed: rtl,
    };

    // Ширина линии пересекающей щели `k` — для `overlap-join`.
    // Chromium берёт здесь *вычисленную* ширину, а не нарисованную: у пересекающей оси без
    // `*-rule-style` (`none`) она остаётся начальной `medium` (3px), и концы `overlap-join`
    // всё равно вытягиваются на её половину (`grid-gap-decorations-081`).
    let cross_width = |widths: &lumen_layout::RuleList<f32>,
                       _styles: &lumen_layout::RuleList<BorderStyle>,
                       k: usize,
                       total: usize| { *widths.value_for_gap(k, total) };

    // Куски обеих осей считаются всегда: `overlap-join` смотрит, есть ли на стыке кусок
    // пересекающей щели, даже когда её линия не рисуется (`*-rule-style: none`).
    let mut axis_pieces: [Vec<GapPiece>; 2] = [Vec::new(), Vec::new()];
    for horizontal in [false, true] {
        let (brk, vis, tops_t, tops_a, gap_a, n_t, n_a, a_lo, a_len) = if horizontal {
            (
                s.row_rule_break,
                s.row_rule_visibility_items,
                &row_tops,
                &col_tops,
                p.col_gap,
                n_rows,
                n_cols,
                cx,
                cw,
            )
        } else {
            (
                s.column_rule_break,
                s.column_rule_visibility_items,
                &col_tops,
                &row_tops,
                p.row_gap,
                n_cols,
                n_rows,
                cy,
                ch,
            )
        };
        if tops_t.is_empty() {
            continue;
        }
        let items: Vec<GridItemSpan> = spans
            .iter()
            .map(|&(c0, c1, r0, r1)| {
                if horizontal {
                    GridItemSpan { t0: r0, t1: r1, a0: c0, a1: c1 }
                } else {
                    GridItemSpan { t0: c0, t1: c1, a0: r0, a1: r1 }
                }
            })
            .collect();
        // Дорожки, вылезшие за content box (`width: 120px` при трёх 100px-колонках), тянут
        // линию до своего края: Chromium рисует щель на всю протяжённость сетки.
        let (a_lo, a_hi) = {
            let along: &[(f32, f32)] = if horizontal { &xs } else { &ys };
            let along_tracks = if horizontal { &col_tracks } else { &row_tracks };
            match along_tracks {
                // Дорожки из шаблона: линия идёт от первой до последней, как рисует Chromium.
                Some(t) => (t[0].0, t[t.len() - 1].1),
                None => {
                    let lead = if horizontal { lead_lo.0 } else { lead_lo.1 };
                    let lo = along.iter().map(|e| e.0).fold(lead.unwrap_or(a_lo), f32::min);
                    let hi = along.iter().map(|e| e.1).fold(a_lo + a_len, f32::max);
                    (lo, hi)
                }
            }
        };
        let pieces = grid_gap_pieces(n_t, n_a, &items, tops_a, gap_a, a_lo, a_hi, brk, vis);
        axis_pieces[usize::from(horizontal)] = pieces;
    }

    for horizontal in [false, true] {
        let (visible, insets, tops_t, gap_t, gap_a, reversed) = if horizontal {
            (p.row_visible, &s.row_rule_inset, &row_tops, p.row_gap, p.col_gap, rtl)
        } else {
            (p.column_visible, &s.column_rule_inset, &col_tops, p.col_gap, p.row_gap, false)
        };
        if !visible || tops_t.is_empty() {
            continue;
        }
        let pieces = &axis_pieces[usize::from(horizontal)];
        let crossing = &axis_pieces[usize::from(!horizontal)];
        let (cross_w, cross_s, cross_total) = if horizontal {
            (&s.column_rule_width, &s.column_rule_style, col_tops.len())
        } else {
            (&s.row_rule_width, &s.row_rule_style, row_tops.len())
        };
        // Смещение одного конца куска в px (положительное — внутрь куска).
        let end_inset = |end: PieceEnd, is_start: bool, g: usize| -> f32 {
            // Стык, на котором куска пересекающей щели `k` нет (щель скрыта `visibility-items`
            // или оборвана), — «висячий» конец: на нём действует `cap`, а не `junction`
            // (`grid-gap-decorations-069/078/080`); `%` при этом считается от ширины щели.
            let meets = |k: usize| {
                let (g_lo, g_hi) = (tops_t[g], tops_t[g] + gap_t);
                crossing.iter().any(|c| c.gap == k && c.lo <= g_hi + TRACK_TOL && c.hi >= g_lo - TRACK_TOL)
            };
            let junction = matches!(end, PieceEnd::Junction(k) if meets(k));
            let slot = match (junction, is_start) {
                (false, true) => &insets.cap_start,
                (false, false) => &insets.cap_end,
                (true, true) => &insets.junction_start,
                (true, false) => &insets.junction_end,
            };
            match (slot, end) {
                (lumen_layout::RuleInset::Length(l), PieceEnd::Junction(_)) => l.resolve_or_zero(em, gap_a, vp),
                (lumen_layout::RuleInset::Length(l), PieceEnd::Edge) => l.resolve_or_zero(em, 0.0, vp),
                (lumen_layout::RuleInset::OverlapJoin, PieceEnd::Junction(k)) if junction => {
                    -(gap_a * 0.5 + cross_width(cross_w, cross_s, k, cross_total) * 0.5)
                }
                (lumen_layout::RuleInset::OverlapJoin, _) => 0.0,
            }
        };
        for piece in pieces {
            // `reversed` — ось идёт справа налево: «начало» куска у его правого конца.
            let lo_inset = end_inset(piece.lo_end, !reversed, piece.gap);
            let hi_inset = end_inset(piece.hi_end, reversed, piece.gap);
            let len = piece.hi - piece.lo - lo_inset - hi_inset;
            if len <= 0.0 {
                continue;
            }
            let along = piece.lo + lo_inset;
            let across = tops_t[piece.gap];
            let rect = if horizontal {
                Rect::new(along, across, len, gap_t)
            } else {
                Rect::new(across, along, gap_t, len)
            };
            out.segments.push(GapSegment { rect, horizontal, gap: piece.gap });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Rect;

    fn red() -> Color {
        Color { r: 255, g: 0, b: 0, a: 255 }
    }

    fn ctx(style: BorderStyle, width: f32) -> GapDecorationContext {
        GapDecorationContext { rule_width: width, rule_style: style, rule_color: red() }
    }

    fn col_gap(x: f32, y: f32, w: f32, h: f32) -> GapSegment {
        GapSegment { rect: Rect::new(x, y, w, h), horizontal: false, gap: 0 }
    }

    fn row_gap(x: f32, y: f32, w: f32, h: f32) -> GapSegment {
        GapSegment { rect: Rect::new(x, y, w, h), horizontal: true, gap: 0 }
    }

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Reference values measured in Edge (`border: 4px inset <colour>`: top = dark, bottom = light).
    #[test]
    fn groove_shades_match_edge() {
        let cases = [
            ((0, 0, 0), (84, 84, 84), (168, 168, 168)),
            ((30, 0, 0), (114, 0, 0), (198, 0, 0)),
            ((32, 32, 32), (116, 116, 116), (200, 200, 200)),
            ((20, 20, 60), (48, 48, 144), (76, 76, 229)),
            ((33, 33, 33), (0, 0, 0), (117, 117, 117)),
            ((0, 40, 0), (0, 0, 0), (0, 124, 0)),
            ((136, 136, 136), (52, 52, 52), (221, 221, 221)),
            ((204, 0, 0), (120, 0, 0), (255, 0, 0)),
            ((10, 200, 30), (5, 116, 17), (12, 255, 38)),
            ((200, 200, 200), (116, 116, 116), (255, 255, 255)),
            ((235, 235, 235), (151, 151, 151), (255, 255, 255)),
            ((236, 236, 236), (152, 152, 152), (236, 236, 236)),
            ((250, 250, 200), (166, 166, 133), (250, 250, 200)),
            ((255, 255, 255), (171, 171, 171), (255, 255, 255)),
            ((255, 255, 0), (171, 171, 0), (255, 255, 0)),
        ];
        for (c, dark, light) in cases {
            assert_eq!(groove_shades(rgb(c.0, c.1, c.2)), (rgb(dark.0, dark.1, dark.2), rgb(light.0, light.1, light.2)), "{c:?}");
        }
    }

    /// `ridge` is the bottom/right side of Edge's `border: 5px ridge #000`: outer 2px dark,
    /// inner 3px light (so top→bottom: 3px light, 2px dark); `groove` (and `outset`) the other way round.
    #[test]
    fn ridge_and_groove_split_the_line_in_two_shades() {
        let rule = Rect::new(10.0, 50.0, 100.0, 5.0);
        let bands = |style| -> Vec<(f32, f32, u8)> {
            rule_line_commands(rule, true, style, rgb(0, 0, 0))
                .iter()
                .map(|c| match c {
                    DisplayCommand::DrawBorder { rect, colors, .. } => (rect.y, rect.height, colors[2].r),
                    _ => panic!("expected DrawBorder"),
                })
                .collect()
        };
        assert_eq!(bands(BorderStyle::Ridge), vec![(50.0, 3.0, 168), (53.0, 2.0, 84)]);
        assert_eq!(bands(BorderStyle::Inset), bands(BorderStyle::Ridge));
        assert_eq!(bands(BorderStyle::Groove), vec![(50.0, 3.0, 84), (53.0, 2.0, 168)]);
        assert_eq!(bands(BorderStyle::Outset), bands(BorderStyle::Groove));
        // A 1px line has no room for two bands: one stripe in the colour itself.
        let thin = rule_line_commands(Rect::new(0.0, 0.0, 10.0, 1.0), true, BorderStyle::Ridge, rgb(7, 8, 9));
        assert_eq!(thin.len(), 1);
        assert!(matches!(&thin[0], DisplayCommand::DrawBorder { colors, .. } if colors[2] == rgb(7, 8, 9)));
        // Vertical rule: the bands split the width, painted as right sides.
        let cmds = rule_line_commands(Rect::new(20.0, 0.0, 10.0, 80.0), false, BorderStyle::Groove, rgb(136, 136, 136));
        assert_eq!(cmds.len(), 2);
        if let DisplayCommand::DrawBorder { rect, widths, colors, .. } = &cmds[1] {
            assert_eq!((rect.x, rect.width, widths[1], colors[1].r), (25.0, 5.0, 5.0, 221));
        } else {
            panic!("expected DrawBorder");
        }
        // Plain styles stay a single command.
        assert_eq!(rule_line_commands(rule, true, BorderStyle::Dashed, rgb(1, 2, 3)).len(), 1);
    }

    #[test]
    fn solid_rule_snaps_to_device_pixels() {
        let snapped = |x: f32, w: f32| {
            let r = snap_rule_rect(Rect::new(x, 2.0, w, 50.0));
            (r.x, r.width, r.y, r.height)
        };
        // A tie goes up (`58.5` → 59), the width follows the rounded far edge.
        assert_eq!(snapped(58.5, 5.0), (59.0, 5.0, 2.0, 50.0));
        assert_eq!(snapped(58.666, 20.0), (59.0, 20.0, 2.0, 50.0));
        // A sub-pixel line keeps one pixel.
        assert_eq!(snapped(10.2, 0.3), (10.0, 1.0, 2.0, 50.0));
        // The command carries the snapped rect, a dashed one keeps the fractional rect.
        let solid = rule_line_commands(Rect::new(58.666, 0.0, 20.0, 50.0), false, BorderStyle::Solid, red());
        assert!(matches!(&solid[0], DisplayCommand::DrawBorder { rect, .. } if rect.x == 59.0));
        let dashed = rule_line_commands(Rect::new(58.666, 0.0, 20.0, 50.0), false, BorderStyle::Dashed, red());
        assert!(matches!(&dashed[0], DisplayCommand::DrawBorder { rect, .. } if rect.x == 58.666));
    }

    #[test]
    fn gap_rule_none_style_emits_nothing() {
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::None, 2.0));
        assert!(cmds.is_empty());
    }

    #[test]
    fn gap_rule_zero_width_emits_nothing() {
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 0.0));
        assert!(cmds.is_empty());
    }

    #[test]
    fn column_gap_emits_vertical_draw_border() {
        // gap rect: x=40, y=0, w=20, h=100; rule_width=2 → rule_x=49, rule_w=2
        let cmds = emit_gap_rules(&[], &[col_gap(40.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 2.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, styles, .. } = &cmds[0] {
            // Centered in gap: x=40 + (20-2)/2 = 49
            assert!((rect.x - 49.0).abs() < 0.01, "rule_x={}", rect.x);
            assert!((rect.width - 2.0).abs() < 0.01);
            assert!((rect.height - 100.0).abs() < 0.01);
            // Right side only
            assert_eq!(widths[1], 2.0);
            assert_eq!(widths[0], 0.0);
            assert_eq!(styles[1], BorderStyle::Solid);
        } else {
            panic!("expected DrawBorder");
        }
    }

    #[test]
    fn row_gap_emits_horizontal_draw_border() {
        // gap rect: x=0, y=50, w=200, h=16; rule_width=2 → rule_y=57, rule_h=2
        let cmds = emit_gap_rules(&[], &[row_gap(0.0, 50.0, 200.0, 16.0)], &ctx(BorderStyle::Dashed, 2.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, styles, .. } = &cmds[0] {
            assert!((rect.y - 57.0).abs() < 0.01, "rule_y={}", rect.y);
            assert!((rect.height - 2.0).abs() < 0.01);
            assert!((rect.width - 200.0).abs() < 0.01);
            // Bottom side only
            assert_eq!(widths[2], 2.0);
            assert_eq!(widths[0], 0.0);
            assert_eq!(styles[2], BorderStyle::Dashed);
        } else {
            panic!("expected DrawBorder");
        }
    }

    #[test]
    fn multiple_gaps_emit_multiple_commands() {
        let gaps = vec![col_gap(20.0, 0.0, 10.0, 100.0), col_gap(60.0, 0.0, 10.0, 100.0)];
        let cmds = emit_gap_rules(&[], &gaps, &ctx(BorderStyle::Solid, 1.0));
        assert_eq!(cmds.len(), 2);
    }

    #[test]
    fn rule_wider_than_gap_is_centered_and_overflows_it() {
        // rule_width=30 > gap.width=20 → the rule stays 30 wide, centred on the gap (x = 10 - 5).
        let cmds = emit_gap_rules(&[], &[col_gap(10.0, 0.0, 20.0, 100.0)], &ctx(BorderStyle::Solid, 30.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, widths, .. } = &cmds[0] {
            assert!((rect.width - 30.0).abs() < 0.01);
            assert!((rect.x - 5.0).abs() < 0.01);
            assert!((widths[1] - 30.0).abs() < 0.01);
        } else {
            panic!("expected DrawBorder");
        }
    }

    #[test]
    fn zero_width_gap_still_paints_a_centered_rule() {
        // `gap: 0`: the rule sits on the seam of the items (grid-gap-decorations-042).
        let cmds = emit_gap_rules(&[], &[col_gap(100.0, 0.0, 0.0, 50.0)], &ctx(BorderStyle::Solid, 5.0));
        assert_eq!(cmds.len(), 1);
        if let DisplayCommand::DrawBorder { rect, .. } = &cmds[0] {
            // 97.5 snaps up to 98 (a tie goes up), the width stays 5.
            assert!((rect.x - 98.0).abs() < 0.01 && (rect.width - 5.0).abs() < 0.01);
        } else {
            panic!("expected DrawBorder");
        }
        // A piece of zero length along the axis paints nothing.
        assert!(emit_gap_rules(&[], &[col_gap(100.0, 0.0, 0.0, 0.0)], &ctx(BorderStyle::Solid, 5.0)).is_empty());
    }

    #[test]
    fn inset_span_shortens_and_extends() {
        assert_eq!(inset_span(10.0, 100.0, 0.0, 0.0, false), Some((10.0, 100.0)));
        assert_eq!(inset_span(10.0, 100.0, 5.0, 15.0, false), Some((15.0, 80.0)));
        // Negative insets extend past the edges.
        assert_eq!(inset_span(10.0, 100.0, -5.0, -5.0, false), Some((5.0, 110.0)));
        // Reversed axis: the start inset applies at the far (right) edge.
        assert_eq!(inset_span(10.0, 100.0, 5.0, 15.0, true), Some((25.0, 80.0)));
        // A collapsed span disappears.
        assert_eq!(inset_span(10.0, 100.0, 50.0, 50.0, false), None);
        assert_eq!(inset_span(10.0, 100.0, 60.0, 60.0, false), None);
    }

    // ── grid_gap_pieces: CSS Gap Decorations L1 §3.1–§3.4 ──────────────────

    use lumen_layout::{RuleBreak, RuleVisibilityItems};

    /// Колоночные щели 3×3-сетки: дорожки 100px, щели 20px, начала строковых щелей 100/220.
    fn pieces(items: &[GridItemSpan], brk: RuleBreak, vis: RuleVisibilityItems) -> Vec<(usize, f32, f32)> {
        grid_gap_pieces(3, 3, items, &[100.0, 220.0], 20.0, 0.0, 340.0, brk, vis)
            .into_iter()
            .map(|p| (p.gap, p.lo, p.hi))
            .collect()
    }

    fn cell(t: usize, a: usize) -> GridItemSpan {
        GridItemSpan { t0: t, t1: t, a0: a, a1: a }
    }

    fn full_grid() -> Vec<GridItemSpan> {
        (0..3).flat_map(|a| (0..3).map(move |t| cell(t, a))).collect()
    }

    #[test]
    fn subgrid_axis_gap_is_the_smallest_positive_seam() {
        // Три дорожки по 30 через щель 10; элемент уже дорожки даёт больший зазор, а не щель.
        assert_eq!(subgrid_axis_gap(&[(0.0, 30.0), (40.0, 70.0), (80.0, 110.0)]), Some(10.0));
        assert_eq!(subgrid_axis_gap(&[(0.0, 20.0), (40.0, 70.0)]), Some(20.0));
        // Один элемент — о щели ничего не известно; вплотную — щель родителя нулевая.
        assert_eq!(subgrid_axis_gap(&[(0.0, 30.0)]), None);
        assert_eq!(subgrid_axis_gap(&[(0.0, 30.0), (30.0, 60.0)]), Some(0.0));
        assert_eq!(subgrid_axis_gap(&[(0.0, 30.0), (30.0, 60.0), (80.0, 110.0)]), Some(0.0));
    }

    #[test]
    fn track_span_at_a_zero_gap_seam_belongs_to_one_track() {
        // gap: 0, seams at 100 and 200: an item 0..100 is track 0 only, 100..300 is tracks 1..2.
        assert_eq!(track_span(0.0, 100.0, &[100.0, 200.0], 0.0), (0, 0));
        assert_eq!(track_span(100.0, 300.0, &[100.0, 200.0], 0.0), (1, 2));
    }

    #[test]
    fn full_grid_normal_runs_through_crosses() {
        // Все стыки — «кресты»: `normal` не режет, `intersection` режет на каждом.
        let n = pieces(&full_grid(), RuleBreak::Normal, RuleVisibilityItems::Normal);
        assert_eq!(n, vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]);
        let i = pieces(&full_grid(), RuleBreak::Intersection, RuleVisibilityItems::Normal);
        assert_eq!(
            i,
            vec![(0, 0.0, 100.0), (0, 120.0, 220.0), (0, 240.0, 340.0), (1, 0.0, 100.0), (1, 120.0, 220.0), (1, 240.0, 340.0)]
        );
    }

    #[test]
    fn spanning_item_interrupts_the_gap_it_covers() {
        // Элемент на колонки 0–1 в строке 1 перекрывает щель 0 в этой строке.
        let mut items: Vec<_> = full_grid().into_iter().filter(|c| !(c.a0 == 1 && c.t0 < 2 && c.t0 != 2)).collect();
        items.push(GridItemSpan { t0: 0, t1: 1, a0: 1, a1: 1 });
        let normal = pieces(&items, RuleBreak::Normal, RuleVisibilityItems::Normal);
        assert_eq!(normal, vec![(0, 0.0, 100.0), (0, 240.0, 340.0), (1, 0.0, 340.0)]);
        // `none` рисует сквозь элемент.
        let none = pieces(&items, RuleBreak::None, RuleVisibilityItems::Normal);
        assert_eq!(none, vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]);
    }

    #[test]
    fn normal_ends_at_t_junctions_by_the_crossing_item_only() {
        // Колонка 0 держит один элемент на строки 0–1: он пересекает строковую щель 0.
        // «Т» на стыке 0: строковая щель (ножка) примыкает к колоночной (перекладине) справа и
        // обрывается об элемент, а колоночная щель 0 идёт сквозь стык целиком.
        let mut items: Vec<_> = full_grid().into_iter().filter(|c| !(c.t0 == 0 && c.a0 < 2)).collect();
        items.push(GridItemSpan { t0: 0, t1: 0, a0: 0, a1: 1 });
        let normal = pieces(&items, RuleBreak::Normal, RuleVisibilityItems::Normal);
        assert_eq!(normal, vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]);
        // `intersection` режет и на «Т», и на кресте.
        let inter = pieces(&items, RuleBreak::Intersection, RuleVisibilityItems::Normal);
        assert_eq!(
            inter,
            vec![(0, 0.0, 100.0), (0, 120.0, 220.0), (0, 240.0, 340.0), (1, 0.0, 100.0), (1, 120.0, 220.0), (1, 240.0, 340.0)]
        );
        // `none` не режет вовсе.
        assert_eq!(
            pieces(&items, RuleBreak::None, RuleVisibilityItems::Normal),
            vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]
        );
    }

    #[test]
    fn visibility_items_hide_pieces_next_to_empty_cells() {
        // Пустая клетка (колонка 2, строка 1): щель 1 в строке 1 касается только колонки 1.
        let items: Vec<_> = full_grid().into_iter().filter(|c| !(c.t0 == 2 && c.a0 == 1)).collect();
        let all = pieces(&items, RuleBreak::None, RuleVisibilityItems::All);
        assert_eq!(all, vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]);
        let around = pieces(&items, RuleBreak::None, RuleVisibilityItems::Around);
        assert_eq!(around, vec![(0, 0.0, 340.0), (1, 0.0, 340.0)]);
        let between = pieces(&items, RuleBreak::None, RuleVisibilityItems::Between);
        assert_eq!(between, vec![(0, 0.0, 340.0), (1, 0.0, 100.0), (1, 240.0, 340.0)]);
    }
}
