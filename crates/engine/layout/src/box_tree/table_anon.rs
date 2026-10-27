//! CSS 2.1 §17.2.1 — anonymous table objects (BUG-1362, TABLE-ANON).
//!
//! The box builder produces one box per element and wraps inline content into
//! anonymous runs; it knows nothing about which `display: table-*` boxes may sit
//! where. This pass runs once per container, after its children are built, and
//! repairs the child list so that table layout only ever sees a well-formed tree:
//!
//! * **missing children** — in a `table` the children that are not a row group, row, caption or
//!   column are wrapped (consecutive ones together) in an anonymous row; in a row group the
//!   non-rows likewise; in a row the non-cells are wrapped in an anonymous cell (stage 2);
//! * **missing parents** — a `table-cell`/`row`/`row-group`/`caption`/`column` whose parent is
//!   not a table is wrapped, together with its consecutive siblings of that kind, in an
//!   anonymous table (stage 3).
//!
//! Whitespace-only text never reaches this pass (stage 1: the builder drops it). Out-of-flow
//! children (absolutely positioned, floated) are not table parts and are left where they are.
//! Flex/grid containers blockify their items, and a column group holds only columns, so neither is
//! touched.
//!
//! A well-formed tree (every real `<table>`) is returned as it came: the check is one scan.

use super::*;

/// What a box means to table layout, by its `display`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Cell,
    Row,
    Group,
    Caption,
    Column,
}

/// Anonymous inline runs clone their parent's style, `display` included, so a run inside a cell
/// reports `table-cell`: only boxes that take part in block layout have a table role.
fn part_of_box(b: &LayoutBox) -> Option<Part> {
    if matches!(
        b.kind,
        BoxKind::InlineRun { .. } | BoxKind::InlineBlockRow | BoxKind::InlineSpace | BoxKind::Marker { .. }
    ) {
        return None;
    }
    part_of(b.style.display)
}

fn part_of(display: Display) -> Option<Part> {
    Some(match display {
        Display::TableCell => Part::Cell,
        Display::TableRow => Part::Row,
        Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => Part::Group,
        Display::TableCaption => Part::Caption,
        Display::TableColumn | Display::TableColumnGroup => Part::Column,
        _ => return None,
    })
}

/// Which children a container accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Table,
    Group,
    Row,
    /// Any other block container: table parts are *not* accepted.
    Other,
}

fn ctx_of(kind: &BoxKind, style: &ComputedStyle) -> Option<Ctx> {
    match kind {
        BoxKind::Table => Some(Ctx::Table),
        BoxKind::TableRowGroup => Some(Ctx::Group),
        BoxKind::TableRow => Some(Ctx::Row),
        BoxKind::Block | BoxKind::FlowRoot
            if !matches!(
                style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
                    | Display::TableColumnGroup | Display::TableColumn
            ) =>
        {
            Some(Ctx::Other)
        }
        _ => None,
    }
}

/// Boxes the pass leaves alone: out-of-flow ones are not table parts, and a fieldset's legend
/// keeps its author `display` on the box (`getComputedStyle`) but is laid out as a block.
fn exempt(doc: &Document, b: &LayoutBox) -> bool {
    matches!(b.style.position, Position::Absolute | Position::Fixed)
        || b.style.float_side != FloatSide::None
        || (b.origin.role == BoxRole::Element && is_fieldset_legend(doc, b.node))
}

/// True when `b` belongs directly under a container of kind `ctx`.
fn accepted(ctx: Ctx, b: &LayoutBox) -> bool {
    let part = part_of_box(b);
    match ctx {
        Ctx::Table => matches!(part, Some(Part::Row | Part::Group | Part::Caption | Part::Column)),
        Ctx::Group => part == Some(Part::Row),
        Ctx::Row => part == Some(Part::Cell),
        Ctx::Other => part.is_none(),
    }
}

/// True when the DOM child with `display` — and, for text, a non-whitespace content — would not be
/// accepted by a table-internal container with display `owner`. The builder uses it to choose
/// between the plain item-container path (well-formed table) and the block-flow path, which
/// gathers text and inline content into runs for [`fix_children`] to wrap.
pub(super) fn display_rejected_by(owner: Display, child: Display) -> bool {
    let part = part_of(child);
    match owner {
        Display::Table | Display::InlineTable => {
            !matches!(part, Some(Part::Row | Part::Group | Part::Caption | Part::Column))
        }
        Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
            part != Some(Part::Row)
        }
        Display::TableRow => part != Some(Part::Cell),
        _ => false,
    }
}

/// Style of an anonymous box: inherited properties from `owner`, everything else initial.
fn anon_table_style(owner: &ComputedStyle, display: Display) -> Arc<ComputedStyle> {
    let mut s = ComputedStyle::inheriting(owner);
    s.display = display;
    Arc::new(s)
}

fn anon_box(
    node: NodeId,
    owner: &ComputedStyle,
    display: Display,
    kind: BoxKind,
    children: Vec<LayoutBox>,
) -> LayoutBox {
    let style = anon_table_style(owner, display);
    LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node,
        rect: Rect::ZERO,
        used_padding: None,
        used_line_height: style.font_size * style.line_height,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        style,
        kind,
        children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role: BoxRole::AnonymousBlock },
    }
}

/// CSS 2.1 §17.2: the first `table-header-group` child is always rendered before every other row
/// group and the first `table-footer-group` child after them, whatever their source order. Later
/// header/footer groups behave like ordinary row groups. BUG-1537.
fn hoist_header_footer(children: &mut Vec<LayoutBox>) {
    let is_group = |c: &LayoutBox| !matches!(c.kind, BoxKind::Skip) && part_of_box(c) == Some(Part::Group);
    let header = children
        .iter()
        .position(|c| is_group(c) && c.style.display == Display::TableHeaderGroup);
    let first_rows = children.iter().position(|c| is_group(c) || part_of_box(c) == Some(Part::Row));
    if let (Some(h), Some(first)) = (header, first_rows)
        && h > first
    {
        let b = children.remove(h);
        children.insert(first, b);
    }
    let footer = children
        .iter()
        .position(|c| is_group(c) && c.style.display == Display::TableFooterGroup);
    let last_rows = children.iter().rposition(|c| is_group(c) || part_of_box(c) == Some(Part::Row));
    if let (Some(f), Some(last)) = (footer, last_rows)
        && f < last
    {
        let b = children.remove(f);
        children.insert(last, b);
    }
}

/// Repairs `children` of a container (`kind`/`style`) per CSS 2.1 §17.2.1. `owner` is the
/// container's node: the anonymous boxes carry it, like every other anonymous box.
pub(super) fn fix_children(
    doc: &Document,
    owner: NodeId,
    kind: &BoxKind,
    style: &ComputedStyle,
    mut children: Vec<LayoutBox>,
) -> Vec<LayoutBox> {
    let Some(ctx) = ctx_of(kind, style) else {
        return children;
    };
    if ctx == Ctx::Table {
        hoist_header_footer(&mut children);
    }
    let broken = children
        .iter()
        .any(|c| !matches!(c.kind, BoxKind::Skip) && !exempt(doc, c) && !accepted(ctx, c));
    if !broken {
        return children;
    }
    // The wrapper that replaces a run of rejected children, and the context its children are
    // then fixed up against.
    let (wrap_display, wrap_kind): (Display, BoxKind) = match ctx {
        Ctx::Table | Ctx::Group => (Display::TableRow, BoxKind::TableRow),
        Ctx::Row => (Display::TableCell, BoxKind::Block),
        Ctx::Other => (Display::Table, BoxKind::Table),
    };
    let mut out = Vec::with_capacity(children.len());
    let mut run: Vec<LayoutBox> = Vec::new();
    let flush = |run: &mut Vec<LayoutBox>, out: &mut Vec<LayoutBox>| {
        if run.is_empty() {
            return;
        }
        let members = std::mem::take(run);
        let mut wrapper = anon_box(owner, style, wrap_display, wrap_kind.clone(), Vec::new());
        wrapper.children = fix_children(doc, owner, &wrapper.kind, &wrapper.style, members);
        out.push(wrapper);
    };
    for c in children {
        if matches!(c.kind, BoxKind::Skip) || exempt(doc, &c) {
            // Transparent: neither a table part nor content to wrap. Skips inside a run stay
            // with it so the run is not split by an invisible box.
            if run.is_empty() {
                out.push(c);
            } else {
                run.push(c);
            }
        } else if accepted(ctx, &c) {
            flush(&mut run, &mut out);
            out.push(c);
        } else {
            run.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}
