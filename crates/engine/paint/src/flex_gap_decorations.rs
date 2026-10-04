//! CSS Gap Decorations L1 §3 для flex-контейнера.
//!
//! У flex нет дорожек: щели «главной» оси лежат между соседними элементами **одной**
//! flex-строки (у каждой строки свои, даже если они случайно совпали по координате с
//! щелями соседней), щели «поперечной» оси — между строками. Поэтому геометрия берётся
//! из прямоугольников детей, сгруппированных в строки, а не из сетки дорожек.
//!
//! * `*-rule-break: none | normal` — главная щель идёт сплошной линией, поперечная тоже
//!   (для flex `normal` = `none`); `intersection` режет линию в стыках с перпендикулярными
//!   щелями (§3.1.2);
//! * `*-rule-inset-*` — `cap` на краях контейнера и у концов без стыка, `junction` у стыков
//!   (§3.3); `%` — от ширины пересекающей щели, `overlap-join` — полщели + полширины её линии;
//! * номера щелей для `<gap-rule-list>` идут по порядку размещения (§4.6): значения сквозь все
//!   flex-строки и не перезапускаются на каждой строке; `row-reverse`/`column-reverse`/
//!   `wrap-reverse`/`direction: rtl` переворачивают порядок.
//!
//! Работает с теми же `GridGapParams`/`GridGapGeometry`, что и grid-ветка.

use lumen_core::geom::{Rect, Size};
use lumen_layout::{
    AlignValue, BorderStyle, Direction, FlexDirection, FlexWrap, LayoutBox, RuleBreak, RuleInset, RuleInsets,
    RuleList,
};

use crate::gap_decorations::{GapSegment, GridGapGeometry, GridGapParams};

/// Допуск сравнения координат (px, float-округление layout).
const EPS: f32 = 0.5;

/// Одна flex-строка: протяжённость поперёк главной оси и щели между её элементами.
struct FlexLine {
    /// Границы строки по поперечной оси.
    lo: f32,
    hi: f32,
    /// Щели главной оси `(начало, конец)` по возрастанию координаты.
    gaps: Vec<(f32, f32)>,
    /// Номера этих щелей в списке значений (§4.6), в том же порядке.
    ids: Vec<usize>,
}

/// Правила одной оси щелей.
struct Axis<'a> {
    visible: bool,
    insets: &'a RuleInsets,
    brk: RuleBreak,
    widths: &'a RuleList<f32>,
    styles: &'a RuleList<BorderStyle>,
}

impl Axis<'_> {
    /// Ширина линии щели `idx` из `total`; 0, если линия невидима.
    fn width(&self, idx: usize, total: usize) -> f32 {
        if total == 0 || !self.styles.value_for_gap(idx, total).is_visible() {
            return 0.0;
        }
        *self.widths.value_for_gap(idx, total)
    }
}

/// Конец отрезка вдоль щели: край/«колпачок» (`None`) или стык с перпендикулярной щелью
/// `Some((ширина щели, ширина её линии))`.
type Endpoint = Option<(f32, f32)>;

/// `((от, до) по главной оси, (от, до) по поперечной)` margin-box ребёнка.
type ItemExtent = ((f32, f32), (f32, f32));

/// Протяжённости margin-box ребёнка: `(по главной оси, по поперечной)`.
fn item_extents(c: &LayoutBox, row_dir: bool, cw: f32, vp: Size) -> ItemExtent {
    let s = &c.style;
    let em = s.font_size;
    let ml = s.margin_left.resolve_or_zero(em, cw, vp);
    let mr = s.margin_right.resolve_or_zero(em, cw, vp);
    let mt = s.margin_top.resolve_or_zero(em, cw, vp);
    let mb = s.margin_bottom.resolve_or_zero(em, cw, vp);
    // Отрицательное поле больше самого элемента переворачивает margin-box (`start > end`):
    // такой элемент занимает в раскладке нулевую протяжённость на своём `end`, и щель
    // начинается там же. Без зажима он попадал в конец сортировки, а щель после него терялась.
    let ordered = |a: f32, b: f32| (a.min(b), b);
    let (x0, x1) = ordered(c.rect.x - ml, c.rect.x + c.rect.width + mr);
    let (y0, y1) = ordered(c.rect.y - mt, c.rect.y + c.rect.height + mb);
    if row_dir {
        ((x0, x1), (y0, y1))
    } else {
        ((y0, y1), (x0, x1))
    }
}

/// Группирует детей во flex-строки (по возрастанию поперечной координаты) и находит щели
/// главной оси каждой строки.
fn collect_lines(children: &[&LayoutBox], row_dir: bool, wrap: bool, main_gap: f32, cw: f32, vp: Size) -> Vec<FlexLine> {
    let mut items: Vec<ItemExtent> = children.iter().map(|c| item_extents(c, row_dir, cw, vp)).collect();
    items.sort_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal));
    let mut groups: Vec<Vec<ItemExtent>> = Vec::new();
    // Верх и низ текущей строки; элемент нулевой высоты (пустой `div`) начинает новую строку
    // лишь тогда, когда его верх строго ниже верха последнего элемента.
    let (mut cur_hi, mut cur_top) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for it in items {
        match groups.last_mut() {
            Some(g) if !wrap || it.1 .0 < cur_hi - EPS || it.1 .0 <= cur_top + EPS => {
                g.push(it);
                cur_hi = cur_hi.max(it.1 .1);
                cur_top = cur_top.max(it.1 .0);
            }
            _ => {
                groups.push(vec![it]);
                cur_hi = it.1 .1;
                cur_top = it.1 .0;
            }
        }
    }
    groups
        .into_iter()
        .map(|mut g| {
            g.sort_by(|a, b| a.0 .0.partial_cmp(&b.0 .0).unwrap_or(std::cmp::Ordering::Equal));
            let lo = g.iter().map(|i| i.1 .0).fold(f32::INFINITY, f32::min);
            let hi = g.iter().map(|i| i.1 .1).fold(f32::NEG_INFINITY, f32::max);
            let mut gaps = Vec::new();
            let mut reach = g[0].0 .1;
            for it in &g[1..] {
                let dist = it.0 .0 - reach;
                // Щель — зазор не меньше `gap` (с допуском): меньший — это просто соседние элементы.
                if dist > EPS && dist >= main_gap - EPS {
                    gaps.push((reach, it.0 .0));
                }
                reach = reach.max(it.0 .1);
            }
            FlexLine { lo, hi, gaps, ids: Vec::new() }
        })
        .collect()
}

/// Flex-строки растягиваются по поперечной оси: единственная строка занимает всю высоту
/// контейнера, а при `align-content: normal | stretch` свободное место делится между строками
/// поровну (CSS Flexbox L1 §9.4). Для прочих `align-content` строки остаются как есть.
fn stretch_lines(lines: &mut [FlexLine], align: AlignValue, wrap: bool, cc: (f32, f32), gap: f32) {
    let n = lines.len();
    if n == 0 || !(!wrap || matches!(align, AlignValue::Normal | AlignValue::Auto | AlignValue::Stretch)) {
        return;
    }
    let total: f32 = lines.iter().map(|l| l.hi - l.lo).sum::<f32>() + gap * (n - 1) as f32;
    let free = (cc.1 - cc.0) - total;
    if free <= EPS {
        return;
    }
    let extra = free / n as f32;
    let mut pos = cc.0;
    for l in lines.iter_mut() {
        let size = l.hi - l.lo + extra;
        l.lo = pos;
        l.hi = pos + size;
        pos += size + gap;
    }
}

/// Сегменты щелей flex-контейнера (см. модуль). `children` — in-flow дети контейнера.
pub fn flex_gap_segments(children: &[&LayoutBox], p: &GridGapParams<'_>) -> GridGapGeometry {
    let s = p.style;
    let (cx, cy, cw, ch) = p.content;
    let em = s.font_size;
    let vp = Size::new(cw, ch);
    let row_dir = matches!(s.flex_direction, FlexDirection::Row | FlexDirection::RowReverse);
    let wrap = s.flex_wrap != FlexWrap::Nowrap;
    let rtl = s.direction == Direction::Rtl;
    let (main_gap, cross_gap) = if row_dir { (p.col_gap, p.row_gap) } else { (p.row_gap, p.col_gap) };
    // Протяжённость контента: по главной оси / по поперечной.
    let (cm, cc) = if row_dir { ((cx, cx + cw), (cy, cy + ch)) } else { ((cy, cy + ch), (cx, cx + cw)) };

    let col = Axis {
        visible: p.column_visible,
        insets: &s.column_rule_inset,
        brk: s.column_rule_break,
        widths: &s.column_rule_width,
        styles: &s.column_rule_style,
    };
    let row = Axis {
        visible: p.row_visible,
        insets: &s.row_rule_inset,
        brk: s.row_rule_break,
        widths: &s.row_rule_width,
        styles: &s.row_rule_style,
    };
    let (main_ax, cross_ax) = if row_dir { (col, row) } else { (row, col) };

    let mut lines = collect_lines(children, row_dir, wrap, main_gap, cw, vp);
    let n = lines.len();
    stretch_lines(&mut lines, s.align_content, wrap, cc, cross_gap);

    // §4.6: значения раздаются в порядке размещения, сквозь все строки.
    let wrap_rev = s.flex_wrap == FlexWrap::WrapReverse;
    let cross_rev = wrap_rev ^ (!row_dir && rtl);
    let main_rev = if row_dir {
        rtl ^ (s.flex_direction == FlexDirection::RowReverse)
    } else {
        s.flex_direction == FlexDirection::ColumnReverse
    };
    let order: Vec<usize> = if cross_rev { (0..n).rev().collect() } else { (0..n).collect() };
    let mut next = 0;
    for &k in &order {
        let cnt = lines[k].gaps.len();
        lines[k].ids = (0..cnt).map(|j| next + if main_rev { cnt - 1 - j } else { j }).collect();
        next += cnt;
    }
    let main_total = next;
    let cross_total = n.saturating_sub(1);
    let cross_idx = |g: usize| if cross_rev { cross_total - 1 - g } else { g };
    let (col_total, row_total) = if row_dir { (main_total, cross_total) } else { (cross_total, main_total) };
    let mut out = GridGapGeometry { segments: Vec::new(), col_total, row_total, column_reversed: false };

    // Прямоугольник по осям (главная, поперечная) → экранный.
    let mk = |m_lo: f32, m_len: f32, c_lo: f32, c_len: f32| {
        if row_dir {
            Rect::new(m_lo, c_lo, m_len, c_len)
        } else {
            Rect::new(c_lo, m_lo, c_len, m_len)
        }
    };
    // Смещение одного конца отрезка (px, положительное — внутрь отрезка).
    let end_inset = |insets: &RuleInsets, cross: Endpoint, is_start: bool| -> f32 {
        let slot = match (cross.is_some(), is_start) {
            (false, true) => &insets.cap_start,
            (false, false) => &insets.cap_end,
            (true, true) => &insets.junction_start,
            (true, false) => &insets.junction_end,
        };
        match (slot, cross) {
            (RuleInset::Length(l), Some((len, _))) => l.resolve_or_zero(em, len, vp),
            (RuleInset::Length(l), None) => l.resolve_or_zero(em, 0.0, vp),
            (RuleInset::OverlapJoin, Some((len, w))) => -(len * 0.5 + w * 0.5),
            (RuleInset::OverlapJoin, None) => 0.0,
        }
    };

    // Щели главной оси (§3.1.2): отрезок занимает ровно свою flex-строку (щели соседних строк
    // не сливаются, даже если совпали по координате); `*-rule-break` на него не влияет — у flex
    // у щели нет стыков внутри строки. Конец у соседней строки — junction (стык с поперечной
    // линией), у края контейнера — cap. Ось главных щелей в экранных координатах идёт вдоль
    // поперечной оси flex; зеркалится она лишь у горизонтальных отрезков под `direction: rtl`.
    let mirror_main = !row_dir && rtl;
    let junction = |gap: f32, idx: usize| {
        let w = cross_ax.width(idx, cross_total);
        (cross_ax.visible && w > 0.0).then_some((gap, w))
    };
    if main_ax.visible {
        for (k, line) in lines.iter().enumerate() {
            let (lo, hi) = (line.lo, line.hi);
            let lo_x: Endpoint = (k > 0).then(|| junction(line.lo - lines[k - 1].hi, cross_idx(k - 1))).flatten();
            let hi_x: Endpoint = lines.get(k + 1).and_then(|nl| junction(nl.lo - line.hi, cross_idx(k)));
            let lo_inset = end_inset(main_ax.insets, lo_x, !mirror_main);
            let hi_inset = end_inset(main_ax.insets, hi_x, mirror_main);
            let len = hi - lo - lo_inset - hi_inset;
            if len <= 0.0 {
                continue;
            }
            for (&(a, b), &id) in line.gaps.iter().zip(&line.ids) {
                out.segments.push(GapSegment {
                    rect: mk(a, b - a, lo + lo_inset, len),
                    horizontal: !row_dir,
                    gap: id,
                });
            }
        }
    }

    // Щели поперечной оси: между строками, на всю длину контейнера; при `intersection`
    // режутся в стыках с главными щелями соседних строк.
    let mirror_cross = row_dir && rtl;
    if cross_ax.visible {
        for g in 0..cross_total {
            let (a, b) = (&lines[g], &lines[g + 1]);
            let size = b.lo - a.hi;
            if size <= EPS {
                continue;
            }
            // Стыки: объединение главных щелей обеих строк, `(от, до, ширина линии)`.
            let mut junctions: Vec<(f32, f32, f32)> = Vec::new();
            if cross_ax.brk == RuleBreak::Intersection {
                for l in [a, b] {
                    for (&(lo, hi), &id) in l.gaps.iter().zip(&l.ids) {
                        let (lo, hi) = (lo.max(cm.0), hi.min(cm.1));
                        if hi > lo {
                            junctions.push((lo, hi, main_ax.width(id, main_total)));
                        }
                    }
                }
                junctions.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                let mut merged: Vec<(f32, f32, f32)> = Vec::new();
                for j in junctions {
                    match merged.last_mut() {
                        Some(m) if j.0 <= m.1 + 0.01 => {
                            m.1 = m.1.max(j.1);
                            m.2 = m.2.max(j.2);
                        }
                        _ => merged.push(j),
                    }
                }
                junctions = merged;
            }
            // Куски линии между стыками.
            let mut pieces: Vec<(f32, Endpoint, f32, Endpoint)> = Vec::new();
            let (mut pos, mut pos_x): (f32, Endpoint) = (cm.0, None);
            for &(jl, jh, w) in &junctions {
                pieces.push((pos, pos_x, jl, Some((jh - jl, w))));
                pos = jh;
                pos_x = Some((jh - jl, w));
            }
            pieces.push((pos, pos_x, cm.1, None));
            let id = cross_idx(g);
            for (lo, lo_x, hi, hi_x) in pieces {
                let lo_inset = end_inset(cross_ax.insets, lo_x, !mirror_cross);
                let hi_inset = end_inset(cross_ax.insets, hi_x, mirror_cross);
                let len = hi - lo - lo_inset - hi_inset;
                if hi - lo <= 0.01 || len <= 0.0 {
                    continue;
                }
                out.segments.push(GapSegment { rect: mk(lo + lo_inset, len, a.hi, size), horizontal: row_dir, gap: id });
            }
        }
    }
    out
}
