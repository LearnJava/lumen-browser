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

/// One part of the flow of a nested multicol container.
pub(super) enum NestedSeg {
    /// The items of a segment of columns: `(item, margin-top, margin-bottom)`, as measured at the
    /// origin.
    Cols(Vec<(LayoutBox, f32, f32)>),
    /// A `column-span: all` element with its margins, as the container placed it.
    Span(LayoutBox, f32, f32),
}

/// The measured segments of a nested multicol container and the geometry its columns were cut
/// with.
pub(super) struct NestedFlow {
    /// In source order: segments of columns and the spanners between them.
    pub(super) segs: Vec<NestedSeg>,
    /// `(column-height, row-gap)` of a container with rows of columns ([`cut_nested_rows`]).
    pub(super) rows: Option<(f32, f32)>,
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
    let Some(NestedSeg::Cols(measured)) = nf.segs.first() else { return None };
    let items: Vec<Item> = measured.iter().map(|(c, mt, mb)| (c, *mt, *mb)).collect();
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

/// What one fragment of a nested container collects while [`cut_nested_rows`] fills it.
#[derive(Default)]
struct FragAcc {
    children: Vec<LayoutBox>,
    /// Block extent of the content, from the top of the fragment's content box.
    used: f32,
}

/// [`cut_nested`] for a container with rows of columns (`column-height`) and/or `column-span: all`
/// elements. The fragments are filled top down with a cursor `(m, y)` — fragment `m` (one column of
/// the parent), `y` from the top of its content box: a spanner takes the full width at the cursor
/// (and moves to the next fragment when it does not fit), a segment of columns opens rows of
/// `column-height` one after another (a row after a spanner is a new one, Multicol L2 §4.4), a row
/// gap that would start a fragment is dropped, and a row cut short by the end of the fragment only
/// has the room that is left (Fragmentation L3 §5.1). The last row of a segment is balanced
/// (`column-fill: balance`, or because a spanner follows it).
pub(super) fn cut_nested_rows(
    nf: &NestedFlow,
    b: &LayoutBox,
    h: f32,
    mt: f32,
    cur_y: f32,
    col_dx: &dyn Fn(usize) -> f32,
    viewport: Size,
) -> Option<NestedCut> {
    let (col_h, row_gap) = nf.rows?;
    let clone = b.style.box_decoration_clone;
    let nn = nf.n.max(1);
    let (dt, db) = (nf.edge_top, nf.edge_bottom);
    // Block size of fragment `m`'s content box that is left for the content.
    let avail = |m: usize| {
        let top = if m == 0 { mt } else { 0.0 } + if m == 0 || clone { dt } else { 0.0 };
        (h - top - if clone { db } else { 0.0 }).max(1.0)
    };
    // Page `y` of the top of fragment `m`'s content box.
    let content_top = |m: usize| cur_y + if m == 0 { mt } else { 0.0 } + if m == 0 || clone { dt } else { 0.0 };
    let mut frags: Vec<FragAcc> = vec![FragAcc::default()];
    let (mut m, mut y) = (0usize, 0.0f32);
    for (si, seg) in nf.segs.iter().enumerate() {
        match seg {
            NestedSeg::Span(sp, smt, smb) => {
                let need = smt + sp.rect.height + smb;
                if y > 0.0 && y + need > avail(m) + multicol_flow::EPS {
                    m += 1;
                    y = 0.0;
                    frags.push(FragAcc::default());
                }
                let mut placed = sp.clone();
                shift_tree(&mut placed, col_dx(m), content_top(m) + y + smt - sp.rect.y);
                y += need;
                frags[m].children.push(placed);
                frags[m].used = frags[m].used.max(y);
            }
            NestedSeg::Cols(measured) => {
                let items: Vec<Item> = measured.iter().map(|(c, mt, mb)| (c, *mt, *mb)).collect();
                let flow = Flow::new(&items, viewport);
                let model = &flow.model;
                let followed_by_span = matches!(nf.segs.get(si + 1), Some(NestedSeg::Span(..)));
                let mut caps: Vec<f32> = Vec::new();
                let mut r = 0usize;
                loop {
                    if r > 0 {
                        y += row_gap;
                    }
                    if y > 0.0 && avail(m) - y < 1.0 {
                        m += 1;
                        y = 0.0;
                        frags.push(FragAcc::default());
                    }
                    let cap_r = col_h.min(avail(m) - y).max(1.0);
                    caps.extend(std::iter::repeat_n(cap_r, nn));
                    let h_of = |g: usize| caps.get(g).copied().unwrap_or(cap_r);
                    let mut pages = model.paginate_ex(&h_of, false)?;
                    let last_row = pages.len() <= (r + 1) * nn;
                    let mut row_h = cap_r;
                    if last_row && (nf.balance || followed_by_span) {
                        row_h = model.balanced_tail(&|g| caps[g], r * nn, (r + 1) * nn, cap_r);
                        let tail = caps.len() - nn;
                        caps[tail..].fill(row_h);
                        pages = model.paginate_ex(&|g| caps.get(g).copied().unwrap_or(row_h), false)?;
                    }
                    let total = pages.len();
                    for (j, pg) in pages[r * nn..((r + 1) * nn).min(total)].iter().enumerate() {
                        let win = Window {
                            lo: pg.lo,
                            hi: pg.hi,
                            lead: pg.lead,
                            tail: pg.tail,
                            last: r * nn + j + 1 == total,
                            extent: row_h,
                            col_top: content_top(m) + y,
                            dx: col_dx(m) + nf.content_x + j as f32 * (nf.col_w + nf.col_gap),
                        };
                        for &(item, off) in &flow.items {
                            if let Some(mut piece) = cut(item, off, &win, 0) {
                                if matches!(piece.kind, BoxKind::InlineRun { .. }) {
                                    piece.rect.width = nf.col_w;
                                }
                                frags[m].children.push(piece);
                            }
                        }
                    }
                    y += row_h;
                    frags[m].used = frags[m].used.max(y);
                    if last_row {
                        break;
                    }
                    r += 1;
                    if r > MAX_ROWS {
                        return None;
                    }
                }
            }
        }
    }
    let frags_n = frags.len();
    let mut out = Vec::with_capacity(frags_n);
    let mut extent = 0.0f32;
    for (m, acc) in frags.into_iter().enumerate() {
        let last = m + 1 == frags_n;
        let y0 = cur_y + if m == 0 { mt } else { 0.0 };
        let top_edge = if m == 0 || clone { dt } else { 0.0 };
        let bottom_edge = if last || clone { db } else { 0.0 };
        let content_h = if last { acc.used } else { avail(m) };
        let mut frag = shell(b);
        frag.rect.x = b.rect.x + col_dx(m);
        frag.rect.y = y0;
        frag.rect.height = top_edge + content_h + bottom_edge;
        slice_edges(&mut frag, m > 0 && !clone, !last && !clone);
        Arc::make_mut(&mut frag.style).multicol_fragment = true;
        frag.children = acc.children;
        extent = extent.max(y0 + frag.rect.height - cur_y);
        out.push(frag);
    }
    Some(NestedCut { frags: out, extent })
}

/// Upper bound of the rows one segment of a nested container may open.
const MAX_ROWS: usize = 500;
