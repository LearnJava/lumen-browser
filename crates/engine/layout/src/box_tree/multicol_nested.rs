//! CSS Multicol L1 §8.1 / Fragmentation L3 — a multicol container nested in a `column-fill: auto`
//! multicol container with a column height: it does not stay one monolithic box in the first outer
//! column, its columns continue in the next outer column.
//!
//! The inner container is laid out first as an ordinary item of the outer one
//! (`multicol_trampoline`), which keeps the flow it cut ([`NestedFlow`]) when the outer container
//! can use it. If the inner box is taller than the outer column, [`cut_nested`] paginates that flow
//! again: every fragment of the inner box occupies one outer column and has `column-count` columns
//! of its own, all as tall as the outer column leaves room for; the last fragment is balanced
//! (`column-fill: balance`) or as tall as its content. A fragment that does not end the box keeps
//! the whole column; `box-decoration-break` decides which edges (border, padding) every fragment
//! keeps.

use super::multicol_flow::{cut, shell, slice_edges, Flow, Item, Window};
use super::*;

/// The measured items of a nested multicol container (its only segment) and the geometry its
/// columns were cut with.
pub(super) struct NestedFlow {
    /// `(item, margin-top, margin-bottom)`, as measured at the origin.
    pub(super) items: Vec<(LayoutBox, f32, f32)>,
    /// `column-count` of the inner container.
    pub(super) n: usize,
    pub(super) col_w: f32,
    pub(super) col_gap: f32,
    /// `column-fill: balance`.
    pub(super) balance: bool,
    /// `x` of the inner content box, in the coordinates the items were measured in.
    pub(super) content_x: f32,
    /// Border plus padding at the top and at the bottom of the inner box.
    pub(super) edge_top: f32,
    pub(super) edge_bottom: f32,
}

/// The fragments of a nested multicol container.
pub(super) struct NestedCut {
    pub(super) frags: Vec<LayoutBox>,
    /// Block extent from the top of the outer segment to the bottom of the lowest fragment.
    pub(super) extent: f32,
}

/// Cuts nested container `b` (laid out at the origin with margin-top `mt`) across the columns of
/// its multicol parent: `h` is the parent's column height, `cur_y` the page `y` of the parent's
/// segment, `col_dx(m)` the shift of the parent's column `m`.
pub(super) fn cut_nested(
    nf: &NestedFlow,
    b: &LayoutBox,
    h: f32,
    mt: f32,
    cur_y: f32,
    col_dx: &dyn Fn(usize) -> f32,
    viewport: Size,
) -> Option<NestedCut> {
    let clone = b.style.box_decoration_clone;
    let nn = nf.n.max(1);
    let items: Vec<Item> = nf.items.iter().map(|(c, mt, mb)| (c, *mt, *mb)).collect();
    let flow = Flow::new(&items, viewport);
    let model = &flow.model;
    let (dt, db) = (nf.edge_top, nf.edge_bottom);
    // Block size of the inner columns of fragment `m`: what the outer column leaves after the
    // margin and the edges the fragment keeps (only the last fragment closes the box).
    let cap = |m: usize, last: bool| {
        let top = if m == 0 { mt } else { 0.0 } + if m == 0 || clone { dt } else { 0.0 };
        (h - top - if clone || last { db } else { 0.0 }).max(1.0)
    };
    let mut guess = 1usize;
    let mut pages = Vec::new();
    for _ in 0..4 {
        pages = model.paginate_ex(&|g| cap(g / nn, g / nn + 1 == guess), false)?;
        let wanted = pages.len().div_ceil(nn);
        if wanted == guess {
            break;
        }
        guess = wanted;
    }
    let frags_n = pages.len().div_ceil(nn);
    let last_m = frags_n - 1;
    if nf.balance {
        let tail = model.balanced_tail(&|g| cap(g / nn, false), last_m * nn, frags_n * nn, cap(last_m, true));
        pages = model.paginate_ex(&|g| if g / nn < last_m { cap(g / nn, false) } else { tail }, false)?;
    }
    let total = pages.len();
    let mut frags = Vec::with_capacity(frags_n);
    let mut extent = 0.0f32;
    for m in 0..frags_n {
        let last = m + 1 == frags_n;
        let y = cur_y + if m == 0 { mt } else { 0.0 };
        let top_edge = if m == 0 || clone { dt } else { 0.0 };
        let bottom_edge = if last || clone { db } else { 0.0 };
        let cols = &pages[m * nn..((m + 1) * nn).min(total)];
        let content_h = if last {
            cols.iter().map(|p| p.hi - p.lo + p.lead + p.tail).fold(0.0, f32::max)
        } else {
            cap(m, false)
        };
        let mut frag = shell(b);
        frag.rect.x = b.rect.x + col_dx(m);
        frag.rect.y = y;
        frag.rect.height = top_edge + content_h + bottom_edge;
        slice_edges(&mut frag, m > 0 && !clone, !last && !clone);
        for (j, pg) in cols.iter().enumerate() {
            let win = Window {
                lo: pg.lo,
                hi: pg.hi,
                lead: pg.lead,
                tail: pg.tail,
                last: m * nn + j + 1 == total,
                extent: content_h,
                col_top: y + top_edge,
                dx: col_dx(m) + nf.content_x + j as f32 * (nf.col_w + nf.col_gap),
            };
            for &(item, off) in &flow.items {
                if let Some(mut piece) = cut(item, off, &win, 0) {
                    if matches!(piece.kind, BoxKind::InlineRun { .. }) {
                        piece.rect.width = nf.col_w;
                    }
                    frag.children.push(piece);
                }
            }
        }
        extent = extent.max(y + frag.rect.height - cur_y);
        frags.push(frag);
    }
    Some(NestedCut { frags, extent })
}
