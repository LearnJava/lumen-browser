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
//! floats, grid/table, nested multicol, transformed boxes — [`is_fragmentable`]) are never
//! cut: they go whole into the window their top edge falls in and overflow it.
//!
//! A flex container is cut like a block whose children are the flex items (Flexbox L1 §12): the
//! items are told into lines by their position ([`walk_flex_lines`]) — a column container has one
//! item per line, a row container breaks between its lines and, inside a line, through every
//! item at once (each item is its own flow, the window edge cuts all of them).

use super::multicol_break::{avoids_after, avoids_before, breaks_after, breaks_before};
use super::multicol_fragmentation::{item_lines, ItemLines};
use super::multicol_span::trim_edge;
use super::multicol_table;
use super::*;
use crate::style::{BreakValue, ContainFlags};

pub(super) const EPS: f32 = 0.01;
/// Boxes nested deeper than this are monolithic (the walks are native-recursive).
const MAX_DEPTH: usize = 48;
/// A column height of a pixel or two would otherwise cut a long document into endless columns.
const MAX_WINDOWS: usize = 4000;

/// A place where a column may end: the column holds the stack up to `end`, the next one starts
/// at `resume` (the part between is a dropped margin, Fragmentation L3 §5.2).
#[derive(Clone, Copy, Debug)]
pub(super) struct Cand {
    pub(super) end: f32,
    pub(super) resume: f32,
    pub(super) forced: bool,
    /// 0 — no rule broken, 1 — `orphans`/`widows`, 2 — a `break-*: avoid` rule.
    pub(super) tier: u8,
    /// Block size the repeated table header takes at the top of the next column, and the footer
    /// at the bottom of this one ([`super::multicol_table`]); 0 outside a table with such sections.
    pub(super) lead: f32,
    pub(super) tail: f32,
}

impl Cand {
    fn new(end: f32, resume: f32, forced: bool, tier: u8) -> Self {
        Cand { end, resume, forced, tier, lead: 0.0, tail: 0.0 }
    }
}

/// A run of the stack where a break may fall at any height (a block without in-flow children,
/// the empty space below the last child).
#[derive(Clone, Copy, Debug)]
pub(super) struct Free {
    pub(super) lo: f32,
    pub(super) hi: f32,
    pub(super) tier: u8,
    /// Where the next column starts after a break in the run (`None` — at the break itself); a
    /// margin run resumes after the margin, which a break truncates (Fragmentation L3 §5.2).
    pub(super) resume: Option<f32>,
    /// As [`Cand::lead`] / [`Cand::tail`].
    pub(super) lead: f32,
    pub(super) tail: f32,
}

impl Free {
    fn new(lo: f32, hi: f32, tier: u8, resume: Option<f32>) -> Self {
        Free { lo, hi, tier, resume, lead: 0.0, tail: 0.0 }
    }
}

/// Break points of one segment, in virtual coordinates.
#[derive(Default, Debug)]
pub(super) struct Model {
    pub(super) cands: Vec<Cand>,
    pub(super) free: Vec<Free>,
    /// Bottom of the last item (the stack without the last margin).
    pub(super) end: f32,
    /// `end` of the last forced break (0 when there is none).
    last_forced: f32,
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
        // A margin above the first item that is taller than a column is cut by the first break.
        if let Some(&(first, mt, _)) = items.first()
            && mt > EPS
            && participates(first)
        {
            model.free.push(Free::new(0.0, mt, 0, Some(mt)));
        }
        walk_list(&mut model, &list, false, 0, viewport);
        model.cands.sort_by(|a, b| a.end.total_cmp(&b.end).then(a.resume.total_cmp(&b.resume)));
        model.free.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        model.last_forced = model.cands.iter().filter(|c| c.forced).map(|c| c.end).fold(0.0, f32::max);
        Flow { items: placed, model }
    }
}

pub(super) fn participates(b: &LayoutBox) -> bool {
    !matches!(b.kind, BoxKind::Skip)
        && !matches!(b.style.position, Position::Absolute | Position::Fixed)
        && b.style.float_side == FloatSide::None
}

/// Can a column break fall inside this block (between its children, or through its box)?
/// Everything else is monolithic.
pub(super) fn is_fragmentable(b: &LayoutBox, depth: usize) -> bool {
    fragmentable(b, depth, false)
}

/// [`is_fragmentable`]; with `oof` an absolutely positioned `b` qualifies as well (an extracted
/// out-of-flow box is cut across the columns by [`cut_oof`]).
fn fragmentable(b: &LayoutBox, depth: usize, oof: bool) -> bool {
    let s = &b.style;
    let barred = ContainFlags::SIZE.0 | ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0;
    depth < MAX_DEPTH
        && matches!(b.kind, BoxKind::Block | BoxKind::Table | BoxKind::TableRowGroup | BoxKind::TableRow)
        && matches!(
            s.display,
            Display::Block
                | Display::ListItem
                | Display::FlowRoot
                | Display::Flex
                | Display::Grid
                | Display::Table
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableRow
                | Display::TableCell
                | Display::TableCaption
        )
        && s.float_side == FloatSide::None
        && !matches!(s.position, Position::Fixed | Position::Sticky)
        && (oof || s.position != Position::Absolute)
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
        // A grid keeps the tracks it recorded for the gap painter; they are clipped per fragment ([`grid_fragment_tracks`]).
        && (b.subgrid_tracks.is_none() || is_own_track_grid(b))
        // Content that sticks out of the box would be lost in a window the box has no part in.
        && b.children.iter().all(|c| {
            matches!(c.kind, BoxKind::Skip)
                || multicol_table::is_col(c)
                || matches!(c.style.position, Position::Absolute | Position::Fixed)
                || (c.rect.y >= b.rect.y - EPS && c.rect.y + c.rect.height <= b.rect.y + b.rect.height + EPS)
        })
}

/// A `display: grid` box whose axes are laid out by itself (no `subgrid`): its row tracks are cut
/// by the column windows.
fn is_own_track_grid(b: &LayoutBox) -> bool {
    use crate::style::GridTrackSize::Subgrid;
    let s = &b.style;
    matches!(s.display, Display::Grid)
        && s.grid_template_columns.first() != Some(&Subgrid)
        && s.grid_template_rows.first() != Some(&Subgrid)
}

/// What the items hold of out-of-flow boxes, as [`scan_oof`] found it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum OofScan {
    /// Nothing out of flow: the stack reproduces the items by a plain shift.
    None,
    /// Absolutely positioned boxes whose containing block lies inside their item — [`take_oof`]
    /// lifts the ones in the cut part out, the rest travels with its monolithic box.
    Inside,
    /// A `fixed`/`sticky`/anchor-positioned box, or an absolute one resolving against a block
    /// outside its item (its position was measured against a different origin): the stack
    /// cannot move it.
    Unsupported,
}

/// CSS Position L3 §3 / Fragmentation L3 §4.4 — classifies the out-of-flow boxes of `items`.
pub(super) fn scan_oof(items: &[&LayoutBox]) -> OofScan {
    use super::multicol_abspos::{establishes_abs_cb, uses_anchor_positioning};
    // (box, a box at or above it inside the item is the containing block of absolute children,
    // the extents `overflow: clip` ancestors cut the content to, per axis — [`take_oof`] would
    // lift an absolute box out of the clip, so one that sticks out of it stays atomic)
    type Span = Option<(f32, f32)>;
    type ClipBox = (Span, Span);
    let narrow = |outer: Span, lo: f32, hi: f32| -> Span {
        Some(outer.map_or((lo, hi), |(a, b)| (lo.max(a), hi.min(b))))
    };
    let clip_of = |b: &LayoutBox, outer: ClipBox| -> ClipBox {
        let r = &b.rect;
        let x = if b.style.overflow_x == Overflow::Clip { narrow(outer.0, r.x, r.x + r.width) } else { outer.0 };
        let y = if b.style.overflow_y == Overflow::Clip { narrow(outer.1, r.y, r.y + r.height) } else { outer.1 };
        (x, y)
    };
    let mut stack: Vec<(&LayoutBox, bool, ClipBox)> =
        items.iter().map(|b| (*b, establishes_abs_cb(&b.style), clip_of(b, (None, None)))).collect();
    let mut found = OofScan::None;
    while let Some((x, cb_in, clip)) = stack.pop() {
        for c in &x.children {
            if matches!(c.kind, BoxKind::Skip) {
                continue;
            }
            match c.style.position {
                Position::Fixed | Position::Sticky => return OofScan::Unsupported,
                Position::Absolute => {
                    let r = &c.rect;
                    let out = |span: Span, lo: f32, hi: f32| span.is_some_and(|(a, b)| lo < a - EPS || hi > b + EPS);
                    let escapes = out(clip.0, r.x, r.x + r.width) || out(clip.1, r.y, r.y + r.height);
                    if !cb_in || escapes || uses_anchor_positioning(&c.style) || holds_avoid(c) {
                        return OofScan::Unsupported;
                    }
                    found = OofScan::Inside;
                    stack.push((c, true, clip_of(c, clip)));
                }
                _ => stack.push((c, cb_in || establishes_abs_cb(&c.style), clip_of(c, clip))),
            }
        }
    }
    found
}

/// `break-inside: avoid` inside an absolute box: [`cut_oof`] slices it by the column windows
/// alone and cannot push the unbreakable part into the next column (nor stretch the fragment
/// that is left behind), so such a box keeps the atomic placement.
fn holds_avoid(b: &LayoutBox) -> bool {
    b.style.break_inside == BreakValue::Avoid || b.children.iter().any(holds_avoid)
}

/// Lifts the absolutely positioned descendants of `b` out of the part of the tree that [`cut`]
/// slices (fragmentable boxes only; a monolithic box keeps its own). They are cut across the
/// columns on their own ([`cut_oof`]): their containing block is a fragmented box, so they have
/// a block extent of their own that can fall into a window the containing block has no part in.
pub(super) fn take_oof(b: &mut LayoutBox, depth: usize, out: &mut Vec<LayoutBox>) {
    if !is_fragmentable(b, depth) {
        return;
    }
    let mut i = 0;
    while i < b.children.len() {
        let c = &b.children[i];
        if !matches!(c.kind, BoxKind::Skip) && c.style.position == Position::Absolute {
            out.push(b.children.remove(i));
            continue;
        }
        take_oof(&mut b.children[i], depth + 1, out);
        i += 1;
    }
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
            let avoided = avoid || avoids_after(p) || avoids_before(c);
            m.cands.push(Cand::new(end, resume, forced, tier(avoided, owid)));
        }
        walk_item(m, c, off, runs[j], avoid, depth, vp);
    }
}

/// Break points of the items of a flex container, `list` in visual (top-to-bottom) order. The
/// items whose block extents overlap form a line (a column container: every item is a line of
/// its own). A break falls between two lines (`break-*` of any item of the lines counts) and
/// inside the items — those of one line side by side, so the break inside one of them cuts them all.
fn walk_flex_lines(m: &mut Model, list: &[(&LayoutBox, f32, f32)], avoid: bool, depth: usize, vp: Size) {
    type Entry<'a, 'b> = &'b (&'a LayoutBox, f32, f32);
    let top_of = |e: Entry| e.0.rect.y + e.1;
    let bottom_of = |e: Entry| e.0.rect.y + e.1 + e.0.rect.height;
    let mut lines: Vec<Vec<Entry>> = Vec::new();
    let mut bottom = f32::NEG_INFINITY;
    for e in list.iter().filter(|e| participates(e.0)) {
        match lines.last_mut() {
            Some(line) if top_of(e) < bottom - EPS => {
                line.push(e);
                bottom = bottom.max(bottom_of(e));
            }
            _ => {
                lines.push(vec![e]);
                bottom = bottom_of(e);
            }
        }
    }
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            let prev = &lines[i - 1];
            let end = prev.iter().map(|e| bottom_of(e)).fold(f32::NEG_INFINITY, f32::max);
            let start = line.iter().map(|e| top_of(e)).fold(f32::INFINITY, f32::min);
            let forced = prev.iter().any(|e| breaks_after(e.0, 0)) || line.iter().any(|e| breaks_before(e.0, 0));
            let avoided = avoid
                || prev.iter().any(|e| avoids_after(e.0))
                || line.iter().any(|e| avoids_before(e.0));
            m.cands.push(Cand::new(end, start.max(end), forced, tier(avoided, false)));
        }
        for &&(c, off, _) in line {
            let run = match item_lines(c) {
                ItemLines::Lines(n) => Some((0, n, 0)),
                _ => None,
            };
            walk_item(m, c, off, run, avoid, depth, vp);
        }
    }
}

/// Break points of the items of a grid container, `list` in visual order, `off` and `vt` — the
/// container's own shift and virtual top. A break falls in a row gap (`rows`: the row tracks as
/// `(top, bottom)` in virtual coordinates; the gap is dropped, Fragmentation L3 §5.1) unless an item
/// spans it, and inside the items — those of one row side by side, so a break inside one of them cuts them all.
fn walk_grid_rows(m: &mut Model, list: &[(&LayoutBox, f32, f32)], rows: &[(f32, f32)], avoid: bool, depth: usize, vp: Size) {
    let flow: Vec<&(&LayoutBox, f32, f32)> = list.iter().filter(|e| participates(e.0)).collect();
    let top_of = |e: &(&LayoutBox, f32, f32)| e.0.rect.y + e.1;
    let bottom_of = |e: &(&LayoutBox, f32, f32)| e.0.rect.y + e.1 + e.0.rect.height;
    for k in 1..rows.len() {
        let (end, start) = (rows[k - 1].1, rows[k].0.max(rows[k - 1].1));
        if flow.iter().any(|e| top_of(e) < end - EPS && bottom_of(e) > start + EPS) {
            continue;
        }
        let before = |e: &&&(&LayoutBox, f32, f32)| top_of(e) >= start - EPS && top_of(e) < rows[k].1 - EPS;
        let after = |e: &&&(&LayoutBox, f32, f32)| bottom_of(e) > rows[k - 1].0 + EPS && bottom_of(e) <= end + EPS;
        let forced = flow.iter().filter(after).any(|e| breaks_after(e.0, 0)) || flow.iter().filter(before).any(|e| breaks_before(e.0, 0));
        let avoided = avoid
            || flow.iter().filter(after).any(|e| avoids_after(e.0))
            || flow.iter().filter(before).any(|e| avoids_before(e.0));
        m.cands.push(Cand::new(end, start, forced, tier(avoided, false)));
    }
    for e in &flow {
        let run = match item_lines(e.0) {
            ItemLines::Lines(n) => Some((0, n, 0)),
            _ => None,
        };
        walk_item(m, e.0, e.1, run, avoid, depth, vp);
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
                m.cands.push(Cand::new(pos, pos, false, tier(avoid, owid)));
            }
        }
        BoxKind::Block | BoxKind::Table | BoxKind::TableRowGroup | BoxKind::TableRow if is_fragmentable(b, depth) => {
            let avoid = avoid || b.style.break_inside == BreakValue::Avoid;
            let (n_cands, n_free) = (m.cands.len(), m.free.len());
            let row = matches!(b.kind, BoxKind::TableRow);
            let s = &b.style;
            let bottom_edge = |w: f32| -> f32 {
                let pad = s.padding_bottom.resolve_or_zero(s.font_size, w, vp);
                vb - s.border_bottom_width - pad
            };
            let mut kids: Vec<(&LayoutBox, f32, f32)> =
                b.children.iter().filter(|c| !multicol_table::is_col(c)).map(|c| (c, off, 0.0)).collect();
            // `order` / `*-reverse` put the items out of tree order: break in visual order.
            let flex = matches!(s.display, Display::Flex);
            let grid = matches!(s.display, Display::Grid);
            if flex || grid {
                kids.sort_by(|a, b| a.0.rect.y.total_cmp(&b.0.rect.y));
            }
            // The row tracks of a grid (virtual coordinates), when it recorded them.
            let rows: Option<Vec<(f32, f32)>> = b.subgrid_tracks.as_ref().filter(|_| grid).and_then(|t| t.rows.as_ref()).map(|r| {
                let top = grid_content_top(b, vp) + off;
                r.iter().map(|&(a, z)| (top + a, top + z)).collect()
            });
            let last = kids.iter().filter(|c| participates(c.0)).map(|c| c.0.rect.y + off + c.0.rect.height).reduce(f32::max);
            let last = last.map(|l| rows.iter().flatten().map(|r| r.1).fold(l, f32::max));
            match last {
                None => {
                    let lo = vt + s.border_top_width;
                    let hi = bottom_edge(b.rect.width);
                    if hi > lo + EPS {
                        m.free.push(Free::new(lo, hi, tier(avoid, false), None));
                    }
                }
                Some(last) => {
                    // The cells of a row sit side by side like the items of a flex line.
                    if let Some(rows) = rows.as_deref().filter(|r| r.len() > 1) {
                        walk_grid_rows(m, &kids, rows, avoid, depth + 1, vp);
                    } else if flex || row || grid {
                        walk_flex_lines(m, &kids, avoid, depth + 1, vp);
                    } else {
                        walk_list(m, &kids, avoid, depth + 1, vp);
                    }
                    let hi = bottom_edge(b.rect.width);
                    if hi > last + EPS {
                        m.free.push(Free::new(last, hi, tier(avoid, false), None));
                    }
                }
            }
            if matches!(b.kind, BoxKind::Table) {
                multicol_table::finish_walk(m, b, off, n_cands, n_free);
            }
        }
        _ => {}
    }
}

impl Model {
    fn free_at(&self, x: f32) -> Option<&Free> {
        let k = self.free.partition_point(|f| f.lo <= x + EPS);
        self.free[..k].iter().rev().take(4).find(|f| x <= f.hi + EPS && x >= f.lo - EPS)
    }

    /// Column windows `[start, end)` of the stack; `h_of(k)` is the height of column `k`
    /// (infinite for `column-fill: auto` without a height). With `strict` any break that
    /// violates a rule — or a column that cannot hold its content — makes the result `None`.
    pub(super) fn paginate(&self, h_of: &dyn Fn(usize) -> f32, strict: bool) -> Option<Vec<(f32, f32)>> {
        self.paginate_ex(h_of, strict).map(|w| w.into_iter().map(|p| (p.lo, p.hi)).collect())
    }

    /// [`Model::paginate`] with the table sections each window carries.
    pub(super) fn paginate_ex(&self, h_of: &dyn Fn(usize) -> f32, strict: bool) -> Option<Vec<Pg>> {
        // A repeated header/footer that takes half the column or more is not repeated.
        let sections = |lead: f32, tail: f32, h: f32| if lead + tail > h * 0.5 { (0.0, 0.0) } else { (lead, tail) };
        let mut out: Vec<Pg> = Vec::new();
        let mut lo = 0.0f32;
        let mut lead = 0.0f32;
        loop {
            let k = out.len();
            let h = h_of(k);
            // Block size of the column left for the content after the repeated header.
            let cap = h - lead;
            let a = self.cands.partition_point(|c| c.end <= lo + EPS);
            let forced_ahead = self.last_forced > lo + EPS;
            if (self.end - lo <= cap + EPS && !forced_ahead) || k + 1 >= MAX_WINDOWS {
                out.push(Pg { lo, hi: self.end.max(lo), lead, tail: 0.0 });
                return Some(out);
            }
            let reach = lo + cap + EPS;
            let b = self.cands.partition_point(|c| c.end <= reach);
            let range = &self.cands[a..b];
            // `(end, resume, tier, lead, tail)`
            let mut pick: Option<(f32, f32, u8, f32, f32)> = None;
            if let Some(f) = range.iter().find(|c| c.forced) {
                let (l, t) = sections(f.lead, f.tail, h);
                pick = Some((f.end, f.resume, 0, l, t));
            } else {
                for c in range {
                    let (l, t) = sections(c.lead, c.tail, h);
                    // The footer repeated below the content takes room of this column.
                    if c.end + t > reach {
                        continue;
                    }
                    if pick.is_none_or(|(e, _, pt, _, _)| c.tier < pt || (c.tier == pt && c.end > e)) {
                        pick = Some((c.end, c.resume, c.tier, l, t));
                    }
                }
                if h.is_finite()
                    && let Some(f) = self.free_at(lo + cap)
                {
                    let (l, t) = sections(f.lead, f.tail, h);
                    let x = lo + cap - t;
                    if x > lo + EPS
                        && let Some(f) = self.free_at(x)
                        && pick.is_none_or(|(e, _, pt, _, _)| f.tier < pt || (f.tier == pt && x >= e))
                    {
                        pick = Some((x, f.resume.unwrap_or(x), f.tier, l, t));
                    }
                }
            }
            match pick {
                Some((e, r, t, l, tl)) => {
                    if strict && t > 0 {
                        return None;
                    }
                    out.push(Pg { lo, hi: e, lead, tail: tl });
                    lo = r.max(e);
                    lead = l;
                }
                None => {
                    if strict {
                        return None;
                    }
                    // Nothing fits: the column overflows up to the next possible break.
                    let next = self.cands[a..].iter().find(|c| c.end + sections(c.lead, c.tail, h).1 > reach);
                    match next {
                        Some(c) => {
                            let (l, t) = sections(c.lead, c.tail, h);
                            out.push(Pg { lo, hi: c.end, lead, tail: t });
                            lo = c.resume.max(c.end);
                            lead = l;
                        }
                        None => {
                            out.push(Pg { lo, hi: self.end.max(lo), lead, tail: 0.0 });
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

/// A column window of the stack with the table sections repeated in it ([`Model::paginate_ex`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct Pg {
    pub(super) lo: f32,
    pub(super) hi: f32,
    /// Block size of the repeated table header at the top of the window.
    pub(super) lead: f32,
    /// Block size of the repeated table footer at the bottom of the window.
    pub(super) tail: f32,
}

/// One column window and where it lands.
pub(super) struct Window {
    pub(super) lo: f32,
    pub(super) hi: f32,
    /// Block sizes of the table header repeated at the top of the window (the stack from `lo` on
    /// starts below it) and of the footer repeated at its bottom.
    pub(super) lead: f32,
    pub(super) tail: f32,
    /// The last window also takes what starts exactly at its end.
    pub(super) last: bool,
    /// Block size of the column (`INFINITY` for `column-fill: auto` without a height): a flex
    /// container that continues in the next column fills this one up to its bottom.
    pub(super) extent: f32,
    /// Page `y` of the window's top edge.
    pub(super) col_top: f32,
    /// Shift of the item's `x` from its measured position into the column.
    pub(super) dx: f32,
}

impl Window {
    /// Page `y` where the stack position `lo` lands (below the repeated header).
    pub(super) fn top(&self) -> f32 {
        self.col_top + self.lead
    }
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
        origin: b.origin,
    }
}

/// Page-independent `y` of the content box top of a grid box (its row tracks start there).
fn grid_content_top(b: &LayoutBox, vp: Size) -> f32 {
    let s = &b.style;
    b.rect.y + s.border_top_width + s.padding_top.resolve_or_zero(s.font_size, b.rect.width, vp)
}

/// The row tracks of grid `d` that the window part `from..to` of it shows, as the painter reads them
/// from a fragment (`SubgridTracks::fragment`): clipped to the part, measured from the fragment's
/// content top (`content` — its virtual `y`), so a row gap that a break splits is not drawn.
fn grid_fragment_tracks(d: &LayoutBox, track_top: f32, content: f32, from: f32, to: f32) -> Option<Box<crate::subgrid::SubgridTracks>> {
    let t = d.subgrid_tracks.as_ref()?;
    let rows = t.rows.as_ref()?;
    let visible: Vec<(usize, (f32, f32))> = rows
        .iter()
        .enumerate()
        .filter_map(|(k, &(a, b))| {
            let (lo, hi) = ((track_top + a).max(from), (track_top + b).min(to));
            (hi > lo + EPS).then_some((k, (lo - content, hi - content)))
        })
        .collect();
    Some(Box::new(crate::subgrid::SubgridTracks {
        cols: t.cols.clone(),
        rows: Some(visible.iter().map(|v| v.1).collect()),
        fragment: true,
        row_gap_base: visible.first().map(|v| (v.0, rows.len().saturating_sub(1))),
        line_gaps: None,
    }))
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
    cut_at(d, off, w, depth, false)
}

/// [`cut`] for an absolutely positioned box lifted out by [`take_oof`]: it is sliced like a block.
pub(super) fn cut_oof(d: &LayoutBox, off: f32, w: &Window) -> Option<LayoutBox> {
    cut_at(d, off, w, 0, true)
}

fn cut_at(d: &LayoutBox, off: f32, w: &Window, depth: usize, oof: bool) -> Option<LayoutBox> {
    let vt = d.rect.y + off;
    let vb = vt + d.rect.height;
    let starts_here = vt >= w.lo - EPS && (vt < w.hi - EPS || (w.last && vt <= w.hi + EPS));
    if !starts_here && (vb <= w.lo + EPS || vt >= w.hi - EPS) {
        return None;
    }
    let is_run = matches!(d.kind, BoxKind::InlineRun { .. });
    let fragmentable = is_run || fragmentable(d, depth, oof);
    if starts_here && (vb <= w.hi + EPS || !fragmentable) {
        let mut whole = d.clone();
        super::shift_tree(&mut whole, w.dx, w.top() - w.lo + off);
        return Some(whole);
    }
    if !fragmentable {
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
        frag.rect.y = w.top() + (vt + a as f32 * lh - w.lo);
        frag.rect.height = (b - a) as f32 * lh;
        return Some(frag);
    }
    // The boxes that hold the table cut at the top of the window start at its edge and take in
    // the repeated header (the stack below `lo` starts under it); those cut at the bottom take in
    // the repeated footer.
    let lead = if top_cut { w.lead } else { 0.0 };
    let tail = if bottom_cut { w.tail } else { 0.0 };
    frag.rect.y = w.top() - lead + (from - w.lo);
    frag.rect.height = (to - from).max(0.0) + lead + tail;
    slice_edges(&mut frag, top_cut, bottom_cut);
    if matches!(d.style.display, Display::Grid) && d.subgrid_tracks.is_some() {
        let vp = Size::new(d.rect.width, d.rect.height);
        let content = if top_cut { from } else { grid_content_top(d, vp) + off };
        frag.subgrid_tracks = grid_fragment_tracks(d, grid_content_top(d, vp) + off, content, from, to);
    }
    // Flexbox L1 §12: a flex container that breaks consumes the rest of the fragmentainer.
    if bottom_cut && w.extent.is_finite() && matches!(d.style.display, Display::Flex) {
        frag.rect.height = frag.rect.height.max(w.lo + w.extent - from);
    }
    // CSS Tables L3 §fragmentation: a table box, row group, row and cell that continue in the
    // next column reach the bottom of this one (the cells of a row stretch with it); the repeated
    // footer keeps its room.
    if bottom_cut && w.extent.is_finite() && multicol_table::is_table_part(d) {
        let room = w.col_top + w.extent - if matches!(d.kind, BoxKind::Table) { 0.0 } else { w.tail };
        frag.rect.height = frag.rect.height.max(room - frag.rect.y);
    }
    let table = matches!(d.kind, BoxKind::Table);
    for c in &d.children {
        if matches!(c.kind, BoxKind::Skip) {
            if !top_cut {
                frag.children.push(c.clone());
            }
        } else if table && multicol_table::is_col(c) {
            // Column boxes follow the rows of the fragment, below.
            frag.children.push(c.clone());
        } else if let Some(piece) = cut_at(c, off, w, depth + 1, false) {
            frag.children.push(piece);
        }
    }
    if table {
        multicol_table::finish_cut(&mut frag, d, w, to, top_cut, bottom_cut);
    }
    Some(frag)
}
