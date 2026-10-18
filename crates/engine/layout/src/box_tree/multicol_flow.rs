//! CSS Fragmentation L3 §4 / CSS Multicol L1 §3.4 — a laid-out segment of a multicol container
//! cut across its columns at the break points of the box tree.
//!
//! `multicol_trampoline` measures every item of a segment once, as one tall column (the
//! "virtual" stack: item `i` starts at `top_i = Σ(mt+h+mb) + mt_i`). This module then
//!
//! 1. collects the places where a column may end ([`Model`]): the gaps between the in-flow
//!    children of any block on any depth, the gaps between line boxes (honouring `orphans` /
//!    `widows`), the forced breaks (`break-before/after: column|always`), and the "free" runs
//!    inside a block without in-flow children or below its last child, where a break may fall
//!    at any height;
//! 2. paginates the virtual stack into column windows ([`Model::paginate`]) — the last break
//!    that fits, preferring one that violates no rule, then one that only breaks `orphans` /
//!    `widows`, then one that also breaks a `break-*: avoid`; `column-fill: balance` bisects the
//!    smallest column height whose strict pagination still fits ([`Model::balanced`]);
//! 3. cuts the measured boxes along the windows ([`cut`]): a box that lies inside a window moves
//!    there as is, a block that straddles a break becomes one shell per window with its
//!    children cut recursively (`box-decoration-break: slice`: the cut edge loses its border,
//!    padding, margin and corner radii), the lines of an `InlineRun` are dealt out by position.
//!
//! Monolithic boxes (replaced elements, scroll containers, `contain: size|layout|paint`,
//! floats, flex/grid/table, nested multicol, transformed boxes — [`is_fragmentable`]) are never
//! cut: they go whole into the window their top edge falls in and overflow it.

use super::multicol_break::{breaks_after, breaks_before};
use super::multicol_fragmentation::{item_lines, ItemLines};
use super::multicol_span::trim_edge;
use super::*;
use crate::style::{BreakValue, ContainFlags};

const EPS: f32 = 0.01;
/// Boxes nested deeper than this are monolithic (the walks are native-recursive).
const MAX_DEPTH: usize = 48;
/// A column height of a pixel or two would otherwise cut a long document into endless columns.
const MAX_WINDOWS: usize = 4000;

/// A place where a column may end: the column holds the stack up to `end`, the next one starts
/// at `resume` (the part between is a dropped margin, Fragmentation L3 §5.2).
#[derive(Clone, Copy, Debug)]
struct Cand {
    end: f32,
    resume: f32,
    forced: bool,
    /// 0 — no rule broken, 1 — `orphans`/`widows`, 2 — a `break-*: avoid` rule.
    tier: u8,
}

/// A run of the stack where a break may fall at any height (a block without in-flow children,
/// the empty space below the last child).
#[derive(Clone, Copy, Debug)]
struct Free {
    lo: f32,
    hi: f32,
    tier: u8,
}

/// Break points of one segment, in virtual coordinates.
#[derive(Default, Debug)]
pub(super) struct Model {
    cands: Vec<Cand>,
    free: Vec<Free>,
    /// Bottom of the last item (the stack without the last margin).
    pub(super) end: f32,
}

/// The measured items of a segment with their position in the virtual stack.
pub(super) struct Flow<'a> {
    pub(super) items: Vec<(&'a LayoutBox, f32)>,
    pub(super) model: Model,
}

/// `(box, margin-top, margin-bottom)` of an item as measured.
pub(super) type Item<'a> = (&'a LayoutBox, f32, f32);

impl<'a> Flow<'a> {
    pub(super) fn new(items: &[Item<'a>], viewport: Size) -> Self {
        let mut v = 0.0f32;
        let mut placed: Vec<(&'a LayoutBox, f32)> = Vec::with_capacity(items.len());
        let mut list: Vec<(&LayoutBox, f32, f32)> = Vec::with_capacity(items.len());
        let mut end = 0.0f32;
        for &(b, mt, mb) in items {
            let top = v + mt;
            let off = top - b.rect.y;
            placed.push((b, off));
            list.push((b, off, mt));
            end = top + b.rect.height;
            v = end + mb;
        }
        let mut model = Model { end, ..Model::default() };
        walk_list(&mut model, &list, false, 0, viewport);
        model.cands.sort_by(|a, b| a.end.total_cmp(&b.end).then(a.resume.total_cmp(&b.resume)));
        model.free.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        Flow { items: placed, model }
    }
}

fn participates(b: &LayoutBox) -> bool {
    !matches!(b.kind, BoxKind::Skip)
        && !matches!(b.style.position, Position::Absolute | Position::Fixed)
        && b.style.float_side == FloatSide::None
}

/// Can a column break fall inside this block (between its children, or through its box)?
/// Everything else is monolithic.
pub(super) fn is_fragmentable(b: &LayoutBox, depth: usize) -> bool {
    let s = &b.style;
    let barred = ContainFlags::SIZE.0 | ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0;
    depth < MAX_DEPTH
        && matches!(b.kind, BoxKind::Block)
        && matches!(s.display, Display::Block | Display::ListItem | Display::FlowRoot)
        && s.float_side == FloatSide::None
        && !matches!(s.position, Position::Absolute | Position::Fixed | Position::Sticky)
        && matches!(s.overflow_x, Overflow::Visible | Overflow::Clip)
        && matches!(s.overflow_y, Overflow::Visible | Overflow::Clip)
        && s.contain.0 & barred == 0
        && s.writing_mode == crate::style::WritingMode::HorizontalTb
        && s.column_count.is_none()
        && s.column_width.is_none()
        && s.transform.is_empty()
        && s.filter.is_empty()
        && s.clip_path.is_none()
        && s.mask_layers.is_empty()
        && b.fieldset_legend.is_none()
        && b.svg_group_transform.is_none()
        && b.subgrid_tracks.is_none()
        // Content that sticks out of the box would be lost in a window the box has no part in.
        && b.children.iter().all(|c| {
            matches!(c.kind, BoxKind::Skip)
                || matches!(c.style.position, Position::Absolute | Position::Fixed)
                || (c.rect.y >= b.rect.y - EPS && c.rect.y + c.rect.height <= b.rect.y + b.rect.height + EPS)
        })
}

/// Anything that the stack cannot reproduce by a plain shift: out-of-flow or sticky boxes
/// resolve against a containing block outside the item.
pub(super) fn has_out_of_flow(items: &[&LayoutBox]) -> bool {
    let mut stack: Vec<&LayoutBox> = items.to_vec();
    while let Some(b) = stack.pop() {
        if !matches!(b.kind, BoxKind::Skip)
            && matches!(b.style.position, Position::Absolute | Position::Fixed | Position::Sticky)
        {
            return true;
        }
        stack.extend(b.children.iter());
    }
    false
}

/// `(lines before the item inside its run, lines of the whole run, run number)` for every
/// participant. A run is a maximal sequence of `InlineRun`s joined by zero-height glue (`<br>`).
fn line_runs(kinds: &[ItemLines]) -> Vec<Option<(u32, u32, usize)>> {
    let mut out = vec![None; kinds.len()];
    let mut i = 0;
    let mut run_id = 0;
    while i < kinds.len() {
        if !matches!(kinds[i], ItemLines::Lines(_)) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        let mut k = i + 1;
        while k < kinds.len() {
            match kinds[k] {
                ItemLines::Lines(_) => {
                    k += 1;
                    end = k;
                }
                ItemLines::Glue => k += 1,
                ItemLines::Other => break,
            }
        }
        let total: u32 = kinds[start..end].iter().map(|k| if let ItemLines::Lines(n) = k { *n } else { 0 }).sum();
        let mut before = 0;
        for (slot, kind) in out[start..end].iter_mut().zip(&kinds[start..end]) {
            *slot = Some((before, total, run_id));
            if let ItemLines::Lines(n) = kind {
                before += n;
            }
        }
        run_id += 1;
        i = end;
    }
    out
}

fn tier(avoid: bool, owid: bool) -> u8 {
    if avoid {
        2
    } else if owid {
        1
    } else {
        0
    }
}

/// `orphans`/`widows` are broken by a break after `before` of `total` lines of a run.
fn breaks_orphans_widows(style: &ComputedStyle, before: u32, total: u32) -> bool {
    before < style.orphans.max(1) || total.saturating_sub(before) < style.widows.max(1)
}

/// Break points between the in-flow `list` entries (`(box, off, margin-top kept after a forced
/// break)`) and inside each of them. `off` maps a box's `rect.y` to its virtual position.
fn walk_list(m: &mut Model, list: &[(&LayoutBox, f32, f32)], avoid: bool, depth: usize, vp: Size) {
    let part: Vec<&(&LayoutBox, f32, f32)> = list.iter().filter(|e| participates(e.0)).collect();
    let kinds: Vec<ItemLines> = part.iter().map(|e| item_lines(e.0)).collect();
    let runs = line_runs(&kinds);
    for (j, &&(c, off, mt)) in part.iter().enumerate() {
        let vt = c.rect.y + off;
        if j > 0 {
            let (p, poff, _) = *part[j - 1];
            let end = p.rect.y + poff + p.rect.height;
            let forced = breaks_after(p, 0) || breaks_before(c, 0);
            let resume = if forced { (vt - mt).max(end) } else { vt.max(end) };
            let owid = match (runs[j - 1], runs[j]) {
                (Some((_, _, a)), Some((before, total, b))) if a == b => breaks_orphans_widows(&c.style, before, total),
                _ => false,
            };
            let avoided = avoid
                || p.style.break_after == BreakValue::Avoid
                || c.style.break_before == BreakValue::Avoid;
            m.cands.push(Cand { end, resume, forced, tier: tier(avoided, owid) });
        }
        walk_item(m, c, off, runs[j], avoid, depth, vp);
    }
}

/// Break points inside one in-flow box.
fn walk_item(m: &mut Model, b: &LayoutBox, off: f32, run: Option<(u32, u32, usize)>, avoid: bool, depth: usize, vp: Size) {
    let vt = b.rect.y + off;
    let vb = vt + b.rect.height;
    match &b.kind {
        BoxKind::InlineRun { lines, .. } if lines.len() > 1 => {
            let l = lines.len() as u32;
            let lh = b.rect.height / l as f32;
            for t in 1..l {
                let pos = vt + t as f32 * lh;
                let owid = run.is_some_and(|(before, total, _)| breaks_orphans_widows(&b.style, before + t, total));
                m.cands.push(Cand { end: pos, resume: pos, forced: false, tier: tier(avoid, owid) });
            }
        }
        BoxKind::Block if is_fragmentable(b, depth) => {
            let avoid = avoid || b.style.break_inside == BreakValue::Avoid;
            let s = &b.style;
            let bottom_edge = |w: f32| -> f32 {
                let pad = s.padding_bottom.resolve_or_zero(s.font_size, w, vp);
                vb - s.border_bottom_width - pad
            };
            let kids: Vec<(&LayoutBox, f32, f32)> = b.children.iter().map(|c| (c, off, 0.0)).collect();
            let last = b.children.iter().filter(|c| participates(c)).map(|c| c.rect.y + off + c.rect.height).reduce(f32::max);
            match last {
                None => {
                    let lo = vt + s.border_top_width;
                    let hi = bottom_edge(b.rect.width);
                    if hi > lo + EPS {
                        m.free.push(Free { lo, hi, tier: tier(avoid, false) });
                    }
                }
                Some(last) => {
                    walk_list(m, &kids, avoid, depth + 1, vp);
                    let hi = bottom_edge(b.rect.width);
                    if hi > last + EPS {
                        m.free.push(Free { lo: last, hi, tier: tier(avoid, false) });
                    }
                }
            }
        }
        _ => {}
    }
}

impl Model {
    fn free_tier_at(&self, x: f32) -> Option<u8> {
        let k = self.free.partition_point(|f| f.lo <= x + EPS);
        self.free[..k].iter().rev().take(4).find(|f| x <= f.hi + EPS && x >= f.lo - EPS).map(|f| f.tier)
    }

    /// Does the stack hold a forced break?
    pub(super) fn has_forced(&self) -> bool {
        self.cands.iter().any(|c| c.forced)
    }

    /// Column windows `[start, end)` of the stack; `h_of(k)` is the height of column `k`
    /// (infinite for `column-fill: auto` without a height). With `strict` any break that
    /// violates a rule — or a column that cannot hold its content — makes the result `None`.
    pub(super) fn paginate(&self, h_of: &dyn Fn(usize) -> f32, strict: bool) -> Option<Vec<(f32, f32)>> {
        let mut out: Vec<(f32, f32)> = Vec::new();
        let mut lo = 0.0f32;
        loop {
            let k = out.len();
            let h = h_of(k);
            if self.end - lo <= h + EPS || k + 1 >= MAX_WINDOWS {
                out.push((lo, self.end.max(lo)));
                return Some(out);
            }
            let reach = lo + h + EPS;
            let a = self.cands.partition_point(|c| c.end <= lo + EPS);
            let b = self.cands.partition_point(|c| c.end <= reach);
            let range = &self.cands[a..b];
            let mut pick: Option<(f32, f32, u8)> = None;
            if let Some(f) = range.iter().find(|c| c.forced) {
                pick = Some((f.end, f.resume, 0));
            } else {
                for c in range {
                    if pick.is_none_or(|(e, _, t)| c.tier < t || (c.tier == t && c.end > e)) {
                        pick = Some((c.end, c.resume, c.tier));
                    }
                }
                if h.is_finite()
                    && let Some(t) = self.free_tier_at(lo + h)
                    && pick.is_none_or(|(e, _, pt)| t < pt || (t == pt && lo + h >= e))
                {
                    pick = Some((lo + h, lo + h, t));
                }
            }
            match pick {
                Some((e, r, t)) => {
                    if strict && t > 0 {
                        return None;
                    }
                    out.push((lo, e));
                    lo = r.max(e);
                }
                None => {
                    if strict {
                        return None;
                    }
                    // Nothing fits: the column overflows up to the next possible break.
                    match self.cands.get(b) {
                        Some(c) => {
                            out.push((lo, c.end));
                            lo = c.resume.max(c.end);
                        }
                        None => {
                            out.push((lo, self.end.max(lo)));
                            return Some(out);
                        }
                    }
                }
            }
        }
    }

    /// `column-fill: balance` (Multicol L1 §7.1): the smallest whole-pixel column height at
    /// which the stack breaks without breaking a rule into no more columns than `n` (or than
    /// the forced breaks need), capped by `limit`. The cap itself when nothing smaller fits.
    pub(super) fn balanced(&self, n: usize, limit: Option<f32>) -> f32 {
        let wanted = n.max(self.paginate(&|_| f32::INFINITY, false).map_or(1, |w| w.len()));
        let cap = limit.unwrap_or(self.end).max(1.0);
        let ok = |h: f32| self.paginate(&|_| h, true).is_some_and(|w| w.len() <= wanted);
        if !ok(cap.ceil()) && limit.is_some() {
            return cap;
        }
        let (mut lo, mut hi) = (1u32, cap.ceil().max(1.0) as u32);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if ok(mid as f32) {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        (hi as f32).min(cap.max(1.0))
    }
}

/// One column window and where it lands.
pub(super) struct Window {
    pub(super) lo: f32,
    pub(super) hi: f32,
    /// The last window also takes what starts exactly at its end.
    pub(super) last: bool,
    /// Page `y` of the window's top edge.
    pub(super) col_top: f32,
    /// Shift of the item's `x` from its measured position into the column.
    pub(super) dx: f32,
}

/// A copy of `b` without its children.
fn shell(b: &LayoutBox) -> LayoutBox {
    LayoutBox {
        node: b.node,
        rect: b.rect,
        style: Arc::clone(&b.style),
        used_line_height: b.used_line_height,
        used_padding: b.used_padding,
        grid_baselines: b.grid_baselines,
        fieldset_legend: b.fieldset_legend,
        table_caption_inset: b.table_caption_inset,
        clamp_clip: b.clamp_clip,
        subgrid_tracks: b.subgrid_tracks.clone(),
        kind: b.kind.clone(),
        children: Vec::new(),
        col_span: b.col_span,
        row_span: b.row_span,
        svg_group_transform: b.svg_group_transform.clone(),
        scroll_x: b.scroll_x,
        scroll_y: b.scroll_y,
        dirty: b.dirty,
        origin: b.origin.clone(),
    }
}

/// `box-decoration-break: slice` on the edges a break cut.
fn slice_edges(frag: &mut LayoutBox, top: bool, bottom: bool) {
    if top {
        trim_edge(frag, true);
    }
    if bottom {
        trim_edge(frag, false);
    }
    if top || bottom {
        let st = Arc::make_mut(&mut frag.style);
        let zero = || Length::Px(0.0);
        if top {
            st.border_top_left_radius = zero();
            st.border_top_right_radius = zero();
            st.border_top_left_radius_y = zero();
            st.border_top_right_radius_y = zero();
        }
        if bottom {
            st.border_bottom_left_radius = zero();
            st.border_bottom_right_radius = zero();
            st.border_bottom_left_radius_y = zero();
            st.border_bottom_right_radius_y = zero();
        }
    }
}

/// The part of `d` that falls into window `w`, placed in its column; `None` when it has none.
/// `off` maps `d.rect.y` to its virtual position.
pub(super) fn cut(d: &LayoutBox, off: f32, w: &Window, depth: usize) -> Option<LayoutBox> {
    let vt = d.rect.y + off;
    let vb = vt + d.rect.height;
    let starts_here = vt >= w.lo - EPS && (vt < w.hi - EPS || (w.last && vt <= w.hi + EPS));
    let is_run = matches!(d.kind, BoxKind::InlineRun { .. });
    let fragmentable = is_run || is_fragmentable(d, depth);
    if starts_here && (vb <= w.hi + EPS || !fragmentable) {
        let mut whole = d.clone();
        super::shift_tree(&mut whole, w.dx, w.col_top - w.lo + off);
        return Some(whole);
    }
    if !fragmentable || vb <= w.lo + EPS || vt >= w.hi - EPS {
        return None;
    }
    let top_cut = vt < w.lo - EPS;
    let bottom_cut = vb > w.hi + EPS;
    let from = vt.max(w.lo);
    let to = vb.min(w.hi);
    let mut frag = shell(d);
    frag.rect.x = d.rect.x + w.dx;
    if let BoxKind::InlineRun { lines, .. } = &mut frag.kind {
        let l = lines.len();
        if l == 0 {
            return None;
        }
        let lh = d.rect.height / l as f32;
        let a = (((w.lo - vt) / lh - EPS).ceil().max(0.0) as usize).min(l);
        let b = if w.last { l } else { (((w.hi - vt) / lh - EPS).ceil().max(0.0) as usize).min(l) };
        if b <= a {
            return None;
        }
        *lines = lines.drain(a..b).collect();
        frag.rect.y = w.col_top + (vt + a as f32 * lh - w.lo);
        frag.rect.height = (b - a) as f32 * lh;
        return Some(frag);
    }
    frag.rect.y = w.col_top + (from - w.lo);
    frag.rect.height = (to - from).max(0.0);
    slice_edges(&mut frag, top_cut, bottom_cut);
    for c in &d.children {
        if matches!(c.kind, BoxKind::Skip) {
            if !top_cut {
                frag.children.push(c.clone());
            }
        } else if let Some(piece) = cut(c, off, w, depth + 1) {
            frag.children.push(piece);
        }
    }
    Some(frag)
}
