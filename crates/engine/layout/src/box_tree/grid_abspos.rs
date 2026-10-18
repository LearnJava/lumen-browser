//! GRID-ABSPOS: абсолютно позиционированные дети grid-контейнера (CSS Grid L1 §9, CSS Box Alignment
//! L3 §5.2 / §6). Они не items сетки (`grid::grid_item_indices`) и раскладываются после того, как
//! размер контейнера окончателен.
//!
//! * Контейнер — containing block ребёнка (`position` ≠ `static`, для `fixed` — transform и т.п.):
//!   CB — область по `grid-row/column-start/end`, где `auto` — край padding box контейнера
//!   (`grid::resolve_abs_axis`). Иначе CB — унаследованный `own_pcb`.
//! * Статическая позиция — прямоугольник той же области (у контейнера, не являющегося CB, — content
//!   box); внутри него бокс выравнивают `justify-self` / `align-self` (`auto` — `justify-items` /
//!   `align-items` контейнера) по каждой оси, где обе вставки `auto`.
//!
//! Физические оси: в горизонтальной сетке `x` ← столбцы (против хода при `direction: rtl`), `y` ←
//! строки; в вертикальной `x` ← строки (справа налево при `rl`), `y` ← столбцы.

use super::grid::{resolve_abs_axis, GridAxis};
use super::grid_trampoline::GridInit;
use super::grid_vertical::{axis_offset, resolve_own, resolve_side};
use super::multicol_abspos::{
    fix_out_of_flow_descendants, fixed_cb_rect, lay_out_abs_children_with, padding_box_rect, uses_anchor_positioning,
};
use super::*;
use crate::style::{Direction, WritingMode};

/// Одна физическая ось контейнера: границы content и padding box, направление и дорожки логической
/// оси, которая на неё ложится.
struct PhysAxis<'a> {
    /// content box: `[lo, hi]`.
    lo: f32,
    hi: f32,
    /// padding box: `[p_lo, p_hi]` — сюда ведёт `auto`-линия.
    p_lo: f32,
    p_hi: f32,
    /// Начало логической оси у `hi` (rtl, `vertical-rl`, …).
    rev: bool,
    /// inline-ось (`justify-*`) либо block-ось (`align-*`).
    inline: bool,
    /// Физическая `x`.
    is_x: bool,
    offsets: &'a [f32],
    sizes: &'a [f32],
}

impl PhysAxis<'_> {
    fn tracks(&self) -> usize {
        self.offsets.len().min(self.sizes.len())
    }

    /// Логическое смещение начала области, начинающейся на линии `l` (1-based): зазор до неё
    /// в область не входит.
    fn line_start(&self, l: u32) -> f32 {
        let n = self.tracks();
        match (l as usize - 1, n) {
            (_, 0) => 0.0,
            (i, n) if i < n => self.offsets[i],
            _ => self.offsets[n - 1] + self.sizes[n - 1],
        }
    }

    /// Логическое смещение конца области, заканчивающейся на линии `l` (конец дорожки `l - 1`).
    fn line_end(&self, l: u32) -> f32 {
        let n = self.tracks();
        if n == 0 {
            return 0.0;
        }
        match l as usize {
            0 | 1 => self.offsets[0],
            l => self.offsets[l - 2] + self.sizes[l - 2],
        }
    }

    /// Физические границы области по линиям `start`/`end` (`None` — край padding box).
    fn range(&self, start: Option<u32>, end: Option<u32>) -> (f32, f32) {
        let s = start.map(|l| self.line_start(l));
        let e = end.map(|l| self.line_end(l));
        if self.rev {
            (e.map_or(self.p_lo, |o| self.hi - o), s.map_or(self.p_hi, |o| self.hi - o))
        } else {
            (s.map_or(self.p_lo, |o| self.lo + o), e.map_or(self.p_hi, |o| self.lo + o))
        }
    }

    /// Выравнивание ребёнка по этой оси и флаг `safe`: `justify-self` для inline-оси, `align-self`
    /// для block-оси; `auto` берётся из `justify-items` / `align-items` контейнера `s`.
    fn alignment(&self, child: &ComputedStyle, s: &ComputedStyle) -> (AlignValue, bool) {
        let (cx, sx) = (&child.content_align_extra, &s.content_align_extra);
        if self.inline {
            let auto = matches!(child.justify_self, AlignValue::Auto);
            let v = if auto { s.justify_items } else { child.justify_self };
            let v = resolve_own(
                v,
                if auto { sx.justify_items_own } else { cx.justify_self_own },
                child,
                self.is_x,
                !self.rev,
            );
            let v = resolve_side(
                v,
                if auto { sx.justify_items_side } else { cx.justify_self_side },
                s.writing_mode,
                self.rev,
            );
            (v, if auto { sx.justify_items_safe } else { cx.justify_self_safe })
        } else {
            let auto = matches!(child.align_self, AlignValue::Auto);
            let v = if auto { s.align_items } else { child.align_self };
            let v = resolve_own(v, if auto { sx.items_own } else { cx.self_own }, child, self.is_x, !self.rev);
            (v, if auto { sx.items_safe } else { cx.self_safe })
        }
    }

    /// Итоговая физическая позиция border box размера `size` с полями `m_lo`/`m_hi` в области
    /// `[cell_lo, cell_lo + cell]`.
    #[allow(clippy::too_many_arguments)]
    fn aligned_pos(
        &self,
        child: &ComputedStyle,
        s: &ComputedStyle,
        cell_lo: f32,
        cell: f32,
        size: f32,
        m_lo: f32,
        m_hi: f32,
    ) -> f32 {
        let (mut align, safe) = self.alignment(child, s);
        let (m_s, m_e) = if self.rev { (m_hi, m_lo) } else { (m_lo, m_hi) };
        if safe && size + m_s + m_e > cell && matches!(align, AlignValue::End | AlignValue::Center) {
            align = AlignValue::Start;
        }
        // Выравнивание по базовой линии у абсолютного бокса вырождается в запасное: first → start,
        // last → end (Box Alignment L3 §9.1).
        if align == AlignValue::LastBaseline {
            align = AlignValue::End;
        }
        let off = axis_offset(align, cell, size, m_s, m_e, false, false);
        if self.rev { cell_lo + cell - off - size } else { cell_lo + off }
    }
}

/// Раскладывает абсолютно позиционированных детей grid-контейнера `b` после того, как его размер
/// окончателен. `init` — `None`, если у сетки нет items (дорожек нет: единственная линия `1` —
/// начало content box). `content` — физический content box контейнера.
#[allow(clippy::too_many_arguments)]
pub(super) fn lay_out_abs(
    b: &mut LayoutBox,
    s: &ComputedStyle,
    init: Option<&GridInit>,
    is_positioned: bool,
    own_pcb: Rect,
    content: Rect,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    hp: &dyn HyphenationProvider,
) {
    let abs: Vec<usize> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| i)
        .collect();
    if !abs.is_empty() {
        let pb = padding_box_rect(b);
        let my_pcb = if is_positioned { pb } else { own_pcb };
        let fixed_is_cb = fixed_cb_rect(b).is_some();

        let (col_offsets, col_sizes, row_offsets, row_sizes): (&[f32], &[f32], &[f32], &[f32]) = match init {
            Some(i) => (&i.col_offsets, &i.col_widths, &i.row_offsets, &i.row_heights),
            None => (&[], &[], &[], &[]),
        };
        let horizontal = s.writing_mode == WritingMode::HorizontalTb;
        let rtl = s.direction == Direction::Rtl;
        let (x_logical_is_col, x_rev, y_rev) = if horizontal {
            (true, rtl, false)
        } else {
            (
                false,
                matches!(s.writing_mode, WritingMode::VerticalRl | WritingMode::SidewaysRl),
                rtl != (s.writing_mode == WritingMode::SidewaysLr),
            )
        };
        let (x_tracks, y_tracks) = if x_logical_is_col {
            ((col_offsets, col_sizes), (row_offsets, row_sizes))
        } else {
            ((row_offsets, row_sizes), (col_offsets, col_sizes))
        };
        let xa = PhysAxis {
            lo: content.x,
            hi: content.x + content.width,
            p_lo: pb.x,
            p_hi: pb.x + pb.width,
            rev: x_rev,
            inline: x_logical_is_col,
            is_x: true,
            offsets: x_tracks.0,
            sizes: x_tracks.1,
        };
        let ya = PhysAxis {
            lo: content.y,
            hi: content.y + content.height,
            p_lo: pb.y,
            p_hi: pb.y + pb.height,
            rev: y_rev,
            inline: !x_logical_is_col,
            is_x: false,
            offsets: y_tracks.0,
            sizes: y_tracks.1,
        };

        let no_names: Vec<Vec<String>> = Vec::new();
        let no_areas: Vec<Vec<String>> = Vec::new();
        let (col_axis, row_axis, n_cols, n_rows) = match init {
            Some(i) => (
                GridAxis {
                    n_tracks: i.col_explicit,
                    names: &i.col_names,
                    areas: &i.s.grid_template_areas,
                    is_col: true,
                    clamp: false,
                },
                GridAxis {
                    n_tracks: i.row_explicit,
                    names: &i.row_names,
                    areas: &i.s.grid_template_areas,
                    is_col: false,
                    clamp: false,
                },
                i.n_cols,
                i.n_rows,
            ),
            None => (
                GridAxis { n_tracks: 0, names: &no_names, areas: &no_areas, is_col: true, clamp: false },
                GridAxis { n_tracks: 0, names: &no_names, areas: &no_areas, is_col: false, clamp: false },
                0,
                0,
            ),
        };

        // (индекс, статическая позиция, переопределённый CB, область для выравнивания, базис полей)
        let mut deferred: Vec<(usize, f32, f32, Option<Rect>)> = Vec::new();
        let mut cells: Vec<(usize, Rect, Rect)> = Vec::new();
        for &i in &abs {
            let cs = &b.children[i].style;
            let grid_is_cb = match cs.position {
                Position::Fixed => fixed_is_cb,
                _ => is_positioned,
            };
            let cell = if grid_is_cb {
                let (c0, c1) = resolve_abs_axis(&cs.grid_column_start, &cs.grid_column_end, &col_axis, n_cols);
                let (r0, r1) = resolve_abs_axis(&cs.grid_row_start, &cs.grid_row_end, &row_axis, n_rows);
                let ((xs, xe), (ys, ye)) = if x_logical_is_col { ((c0, c1), (r0, r1)) } else { ((r0, r1), (c0, c1)) };
                let (x0, x1) = xa.range(xs, xe);
                let (y0, y1) = ya.range(ys, ye);
                Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
            } else {
                content
            };
            deferred.push((i, cell.x, cell.y, grid_is_cb.then_some(cell)));
            cells.push((i, cell, if grid_is_cb { cell } else { my_pcb }));
        }
        lay_out_abs_children_with(b, &deferred, measurer, viewport, my_pcb, hp);

        // Статическая позиция: выровнять бокс внутри области по осям с обеими вставками `auto`.
        for (i, cell, basis) in cells {
            let c = &mut b.children[i];
            let st = &c.style;
            if uses_anchor_positioning(st) {
                continue;
            }
            let em = st.font_size;
            let static_x = st.left.is_auto() && st.right.is_auto();
            let static_y = st.top.is_auto() && st.bottom.is_auto();
            let mut dx = 0.0;
            let mut dy = 0.0;
            if static_x {
                let m_l = st.margin_left.resolve_or_zero(em, basis.width, viewport);
                let m_r = st.margin_right.resolve_or_zero(em, basis.width, viewport);
                dx = xa.aligned_pos(st, s, cell.x, cell.width, c.rect.width, m_l, m_r) - c.rect.x;
            }
            if static_y {
                let m_t = st.margin_top.resolve_or_zero(em, basis.height, viewport);
                let m_b = st.margin_bottom.resolve_or_zero(em, basis.height, viewport);
                dy = ya.aligned_pos(st, s, cell.y, cell.height, c.rect.height, m_t, m_b) - c.rect.y;
            }
            if dx != 0.0 || dy != 0.0 {
                crate::incremental::translate_subtree(c, dx, dy);
            }
        }
    }
    if is_positioned {
        fix_out_of_flow_descendants(b, measurer, viewport, hp);
    }
}
