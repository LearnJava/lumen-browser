//! CSS Fragmentation L3 §4 / CSS Tables L3 — the table-specific part of [`super::multicol_flow`].
//!
//! A table is cut like a block whose children are its row groups and rows (a break falls between
//! two rows, `break-*` of a row, group or cell counts — [`super::multicol_break`]), and a row like
//! a flex line: its cells sit side by side and a break inside one of them cuts them all, each
//! cell fragment being as tall as its row fragment. What differs:
//!
//! * a cell that spans rows (`rowspan`) is never cut, so no break falls inside its extent
//!   ([`finish_walk`] drops those break points; a row holding it stays whole);
//! * the header group (`display: table-header-group`) is repeated at the top of every fragment
//!   after the first, the footer group at the bottom of every fragment before the last
//!   (Fragmentation L4 §5.1); no break falls inside or right after the header, nor before the
//!   footer. The room they take is carried by the break points ([`super::multicol_flow::Cand::lead`])
//!   and by the windows ([`super::multicol_flow::Window::lead`]);
//! * column boxes (`<col>`, `<colgroup>`) span the rows of each fragment ([`finish_cut`]).

use super::multicol_flow::{participates, Model, Window, EPS};
use super::*;

/// `<col>` / `<colgroup>`: boxes that span the table's rows, not children of its block flow.
pub(super) fn is_col(b: &LayoutBox) -> bool {
    matches!(b.style.display, Display::TableColumn | Display::TableColumnGroup)
}

fn is_grid_part(b: &LayoutBox) -> bool {
    participates(b) && matches!(b.kind, BoxKind::TableRowGroup | BoxKind::TableRow)
}

/// The rows of a table, groups unfolded.
fn rows(t: &LayoutBox) -> impl Iterator<Item = &LayoutBox> {
    t.children.iter().filter(|c| is_grid_part(c)).flat_map(|c| -> Vec<&LayoutBox> {
        if matches!(c.kind, BoxKind::TableRow) {
            vec![c]
        } else {
            c.children.iter().filter(|r| matches!(r.kind, BoxKind::TableRow) && participates(r)).collect()
        }
    })
}

/// The header and footer groups of a table that repeat, with the block size they take
/// (including the spacing that separates them from the body).
pub(super) struct Sections<'a> {
    pub(super) head: Option<&'a LayoutBox>,
    pub(super) foot: Option<&'a LayoutBox>,
    pub(super) lead: f32,
    pub(super) tail: f32,
    /// Spacing between the last body row and the footer.
    pub(super) foot_gap: f32,
}

/// A header group repeats when it is the visually first part of the grid, a footer group when it
/// is the last one, and there is a body between them.
pub(super) fn sections(t: &LayoutBox) -> Sections<'_> {
    let parts: Vec<&LayoutBox> = t.children.iter().filter(|c| is_grid_part(c)).collect();
    let pick = |d: Display| parts.iter().copied().find(|c| c.style.display == d);
    let (head, foot) = (pick(Display::TableHeaderGroup), pick(Display::TableFooterGroup));
    let is_section = |c: &&LayoutBox| head.is_some_and(|h| std::ptr::eq(h, *c)) || foot.is_some_and(|f| std::ptr::eq(f, *c));
    let body: Vec<&LayoutBox> = parts.iter().filter(|c| !is_section(c)).copied().collect();
    let none = Sections { head: None, foot: None, lead: 0.0, tail: 0.0, foot_gap: 0.0 };
    if body.is_empty() {
        return none;
    }
    let body_top = body.iter().map(|c| c.rect.y).fold(f32::INFINITY, f32::min);
    let body_bottom = body.iter().map(|c| c.rect.y + c.rect.height).fold(f32::NEG_INFINITY, f32::max);
    let head = head.filter(|h| h.rect.y + h.rect.height <= body_top + EPS);
    let foot = foot.filter(|f| f.rect.y >= body_bottom - EPS);
    let lead = head.map_or(0.0, |h| h.rect.height + (body_top - h.rect.y - h.rect.height).max(0.0));
    let foot_gap = foot.map_or(0.0, |f| (f.rect.y - body_bottom).max(0.0));
    let tail = foot.map_or(0.0, |f| f.rect.height + foot_gap);
    Sections { head, foot, lead, tail, foot_gap }
}

/// Post-processes the break points the walk of table `t` added (`cands[c0..]`, `free[f0..]`):
/// drops those a table cannot break at and tells the rest what the repeated sections take.
pub(super) fn finish_walk(m: &mut Model, t: &LayoutBox, off: f32, c0: usize, f0: usize) {
    let sec = sections(t);
    let head_end = sec.head.map(|h| h.rect.y + off + h.rect.height);
    let foot_start = sec.foot.map(|f| f.rect.y + off);
    // Extents of the cells that run past their row: no break falls inside them.
    let spans: Vec<(f32, f32)> = rows(t)
        .flat_map(|r| {
            let bottom = r.rect.y + r.rect.height;
            r.children
                .iter()
                .filter(move |c| participates(c) && c.rect.y + c.rect.height > bottom + EPS)
                .map(move |c| (c.rect.y + off, c.rect.y + off + c.rect.height))
        })
        .collect();
    let in_span = |x: f32| spans.iter().any(|&(a, b)| a + EPS < x && x < b - EPS);
    let (lead, tail) = (sec.lead, sec.tail);
    let mut cands = m.cands.split_off(c0);
    cands.retain(|c| {
        !in_span(c.end)
            && head_end.is_none_or(|h| c.end > h + EPS)
            && foot_start.is_none_or(|f| c.resume < f - EPS)
    });
    for c in &mut cands {
        if c.lead == 0.0 && c.tail == 0.0 {
            (c.lead, c.tail) = (lead, tail);
        }
    }
    m.cands.extend(cands);
    let mut free = m.free.split_off(f0);
    free.retain(|f| {
        !spans.iter().any(|&(a, b)| f.lo < b - EPS && f.hi > a + EPS)
            && head_end.is_none_or(|h| f.hi > h + EPS)
            && foot_start.is_none_or(|s| f.lo < s - EPS)
    });
    for f in &mut free {
        if f.lead == 0.0 && f.tail == 0.0 {
            (f.lead, f.tail) = (lead, tail);
        }
    }
    m.free.extend(free);
}

/// Sets the extent of a column box (and of the columns inside a group) to `[top, bottom)`.
fn clip_col(c: &mut LayoutBox, top: f32, bottom: f32) {
    c.rect.y = top;
    c.rect.height = (bottom - top).max(0.0);
    for k in &mut c.children {
        clip_col(k, top, bottom);
    }
}

/// Finishes the fragment `frag` of table `d` cut for window `w` (`to` is the stack position its
/// content ends at): column boxes span the rows of the fragment, the header and footer are
/// repeated.
pub(super) fn finish_cut(frag: &mut LayoutBox, d: &LayoutBox, w: &Window, to: f32, top_cut: bool, bottom_cut: bool) {
    let extent = frag
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRowGroup | BoxKind::TableRow))
        .map(|c| (c.rect.y, c.rect.y + c.rect.height))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
    frag.children.retain_mut(|c| {
        if !is_col(c) {
            return true;
        }
        let Some((top, bottom)) = extent else { return false };
        super::shift_tree(c, w.dx, 0.0);
        clip_col(c, top, bottom);
        true
    });
    if !top_cut && !bottom_cut {
        return;
    }
    let sec = sections(d);
    if top_cut && w.lead > 0.0 && let Some(h) = sec.head {
        let mut head = h.clone();
        super::shift_tree(&mut head, w.dx, w.col_top - h.rect.y);
        let at = frag.children.iter().position(|c| is_grid_part(c)).unwrap_or(frag.children.len());
        frag.children.insert(at, head);
    }
    if bottom_cut && w.tail > 0.0 && let Some(f) = sec.foot {
        let mut foot = f.clone();
        super::shift_tree(&mut foot, w.dx, w.top() + (to - w.lo) + sec.foot_gap - f.rect.y);
        frag.children.push(foot);
    }
}
