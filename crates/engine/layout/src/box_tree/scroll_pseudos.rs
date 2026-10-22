//! CSS Overflow L5 scroll pseudo-elements: `::scroll-marker-group`, `::scroll-marker` and
//! `::scroll-button()` box generation.
//!
//! Unlike `::before`/`::after`, none of them is a *child* of its originating element:
//! `::scroll-marker-group` and `::scroll-button()` are **siblings** of the originating box (the
//! group before or after it by `scroll-marker-group`, the buttons before it), and
//! `::scroll-marker` is a child of the group of its nearest *scroll-marker container* (the
//! nearest element with `scroll-marker-group` other than `none`). The pass therefore runs over
//! the finished `children` list of a parent, where every child's own subtree already exists.
//!
//! The root element has no parent to be a sibling in: its group and buttons are the first/last
//! children of its own box ([`inject_root_scroll_pseudos`]).
//!
//! Gaps: targets that are inline-level (`InlineRun` segments, boxes inside an `InlineBlockRow`)
//! and `scroll-target-group: auto` implicit targets generate no marker.

use super::*;

use crate::style::{ScrollMarkerGroupPlacement, WritingMode};

/// What the pass needs from the build context — the same values `build_box_inner` holds.
pub(super) struct ScrollPseudoCx<'a> {
    /// The element whose child list is being processed. Sibling pseudo boxes carry *its* id as
    /// `LayoutBox::node` (like anonymous wrappers) and the originating element only in
    /// `origin.node`: they precede the originating box in tree order, and every "first box of a
    /// node" lookup (computed styles, geometry, hit testing) must keep finding the element's own box.
    pub host: NodeId,
    pub doc: &'a Document,
    pub sheet: &'a Stylesheet,
    pub viewport: Size,
    pub dark_mode: bool,
    pub counters: &'a CounterMap,
    pub registry: &'a CounterStyleRegistry,
}

/// One ancestor on the path from a scroll-marker container down to the candidate target.
struct Anc {
    is_container: bool,
    /// `position` other than `static` — the containing block of absolutely positioned boxes.
    positioned: bool,
    /// The containing block of `position: fixed` boxes (transform, filter, layout/paint containment).
    fixed_cb: bool,
}

impl Anc {
    fn of(style: &ComputedStyle) -> Self {
        let fixed_cb = !style.transform.is_empty()
            || !style.filter.is_empty()
            || style.contain.0 & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0) != 0;
        Anc {
            is_container: style.scroll_marker_group.is_some(),
            positioned: style.position != Position::Static || fixed_cb,
            fixed_cb,
        }
    }
}

/// Both passes for one finished child list. Out of line on purpose: `build_box_inner` recurses once
/// per tree level, so code that only matters for a few boxes stays out of its stack frame.
#[inline(never)]
pub(super) fn inject_all_scroll_pseudos(cx: &ScrollPseudoCx, style: &ComputedStyle, children: &mut Vec<LayoutBox>) {
    inject_scroll_pseudos(cx, style, children);
    if cx.doc.document_element() == Some(cx.host) {
        inject_root_scroll_pseudos(cx, style, children);
    }
}

/// Inserts `::scroll-marker-group` and `::scroll-button()` siblings around the element boxes of
/// `children` that have them. `parent_style` is the style of the box owning `children`.
pub(super) fn inject_scroll_pseudos(
    cx: &ScrollPseudoCx,
    parent_style: &ComputedStyle,
    children: &mut Vec<LayoutBox>,
) {
    let root = cx.doc.document_element();
    let is_target = |c: &LayoutBox| is_target_box(c) && c.origin.node != root;
    let any_group = children.iter().any(|c| is_target(c) && c.style.scroll_marker_group.is_some());
    let buttons = crate::style::sheet_targets_pseudo(cx.sheet, cx.viewport, cx.dark_mode, "scroll-button");
    if !any_group && !buttons {
        return;
    }
    let blockify = matches!(
        parent_style.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    let markers_styled = crate::style::sheet_targets_pseudo(cx.sheet, cx.viewport, cx.dark_mode, "scroll-marker");
    let old = std::mem::take(children);
    let mut out: Vec<LayoutBox> = Vec::with_capacity(old.len() + 2);
    for child in old {
        if !is_target(&child) {
            out.push(child);
            continue;
        }
        let node = child.origin.node.unwrap_or(child.node);
        let mut before: Vec<LayoutBox> = Vec::new();
        let mut after: Vec<LayoutBox> = Vec::new();
        let group = child
            .style
            .scroll_marker_group
            .and_then(|g| {
                build_group(cx, node, &child.style, &child.children, markers_styled).map(|b| (g.placement, b))
            });
        if let Some((ScrollMarkerGroupPlacement::Before, g)) = group {
            before.push(g);
        } else if let Some((ScrollMarkerGroupPlacement::After, g)) = group {
            after.push(g);
        }
        if buttons {
            for kind in BUTTONS {
                if let Some(b) = build_button(cx, node, &child.style, kind) {
                    before.push(b);
                }
            }
        }
        out.extend(group_inline_levels(cx.host, parent_style, before, blockify));
        out.push(child);
        out.extend(group_inline_levels(cx.host, parent_style, after, blockify));
    }
    *children = out;
}

/// The root element's own group and buttons (CSS Overflow L5 §scroll-marker-group: «the root
/// element has no parent to be a sibling in»): the group becomes the first (`before`) or last
/// (`after`) child of the root box, the buttons the first children behind a `before` group.
/// `children` is the finished child list of the root element's box.
pub(super) fn inject_root_scroll_pseudos(cx: &ScrollPseudoCx, style: &ComputedStyle, children: &mut Vec<LayoutBox>) {
    let buttons = crate::style::sheet_targets_pseudo(cx.sheet, cx.viewport, cx.dark_mode, "scroll-button");
    if style.scroll_marker_group.is_none() && !buttons {
        return;
    }
    let markers_styled = crate::style::sheet_targets_pseudo(cx.sheet, cx.viewport, cx.dark_mode, "scroll-marker");
    let blockify = matches!(
        style.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    let mut before: Vec<LayoutBox> = Vec::new();
    let mut after: Vec<LayoutBox> = Vec::new();
    if let Some(g) = style.scroll_marker_group
        && let Some(b) = build_group(cx, cx.host, style, children, markers_styled)
    {
        if g.placement == ScrollMarkerGroupPlacement::Before {
            before.push(b);
        } else {
            after.push(b);
        }
    }
    if buttons {
        for kind in BUTTONS {
            if let Some(b) = build_button(cx, cx.host, style, kind) {
                before.push(b);
            }
        }
    }
    let before = group_inline_levels(cx.host, style, before, blockify);
    let after = group_inline_levels(cx.host, style, after, blockify);
    children.splice(0..0, before);
    children.extend(after);
}

const BUTTONS: [PseudoKind; 4] = [
    PseudoKind::ScrollButtonBlockStart,
    PseudoKind::ScrollButtonInlineStart,
    PseudoKind::ScrollButtonInlineEnd,
    PseudoKind::ScrollButtonBlockEnd,
];

/// The principal box of an element that can carry sibling pseudo-elements.
fn is_target_box(b: &LayoutBox) -> bool {
    b.origin.role == BoxRole::Element
        && b.origin.node.is_some()
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Table)
}

/// The flow-relative and the physical keyword a `::scroll-button()` box answers to for the
/// originating element's writing mode and direction (`("block-end", "down")` in horizontal-tb).
pub(crate) fn button_keywords(kind: PseudoKind, style: &ComputedStyle) -> (&'static str, &'static str) {
    let horizontal = style.writing_mode == WritingMode::HorizontalTb;
    let lr = matches!(style.writing_mode, WritingMode::VerticalLr | WritingMode::SidewaysLr);
    let rtl = style.direction == Direction::Rtl;
    match kind {
        PseudoKind::ScrollButtonBlockStart => {
            ("block-start", if horizontal { "up" } else if lr { "left" } else { "right" })
        }
        PseudoKind::ScrollButtonBlockEnd => {
            ("block-end", if horizontal { "down" } else if lr { "right" } else { "left" })
        }
        PseudoKind::ScrollButtonInlineStart => (
            "inline-start",
            match (horizontal, rtl) {
                (true, false) => "left",
                (true, true) => "right",
                (false, false) => "up",
                (false, true) => "down",
            },
        ),
        _ => (
            "inline-end",
            match (horizontal, rtl) {
                (true, false) => "right",
                (true, true) => "left",
                (false, false) => "down",
                (false, true) => "up",
            },
        ),
    }
}

/// The request string for a `::scroll-button()` box: every spelling it answers to.
fn button_request(kind: PseudoKind, style: &ComputedStyle) -> String {
    let (logical, physical) = button_keywords(kind, style);
    format!("scroll-button({logical}|{physical})")
}

fn build_button(cx: &ScrollPseudoCx, node: NodeId, style: &ComputedStyle, kind: PseudoKind) -> Option<LayoutBox> {
    let request = button_request(kind, style);
    let ps = compute_pseudo_element_style(cx.doc, node, &request, cx.sheet, style, cx.viewport, cx.dark_mode)?;
    if !matches!(ps.content, Content::Items(_)) {
        return None;
    }
    Some(pseudo_box(cx, node, ps, kind, vec![]))
}

/// The `::scroll-marker-group` of `container` with its markers. A group that is `display: none`,
/// or empty and unstyled, is a `BoxKind::Skip` box: nothing renders, but `getComputedStyle` still
/// finds its computed values (CSSOM: an ungenerated pseudo-element resolves to the computed value).
fn build_group(
    cx: &ScrollPseudoCx,
    node: NodeId,
    cstyle: &ComputedStyle,
    cchildren: &[LayoutBox],
    markers_styled: bool,
) -> Option<LayoutBox> {
    let ps = compute_pseudo_element_style(
        cx.doc, node, "scroll-marker-group", cx.sheet, cstyle, cx.viewport, cx.dark_mode,
    )?;
    let markers = if markers_styled && ps.display != Display::None {
        collect_markers(cx, node, cstyle, cchildren)
    } else {
        vec![]
    };
    let hidden = ps.display == Display::None
        || (markers.is_empty() && ps.display == Display::Block && ps.width.is_none() && ps.height.is_none());
    let blockify = matches!(
        ps.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    let items = group_inline_levels(cx.host, &ps, markers, blockify);
    let mut b = pseudo_box(cx, node, ps, PseudoKind::ScrollMarkerGroup, items);
    if hidden {
        b.kind = BoxKind::Skip;
    }
    Some(b)
}

/// `::scroll-marker` boxes (tree order) of every target whose nearest marker container is
/// `container` (CSS Overflow L5 §scroll-marker): in-flow targets belong to the nearest container
/// ancestor, absolutely positioned ones to the nearest container at their containing block's level
/// or above, fixed ones likewise relative to their (transform/filter/containment) containing
/// block — a fixed box without such an ancestor belongs to no group.
fn collect_markers(cx: &ScrollPseudoCx, node: NodeId, cstyle: &ComputedStyle, cchildren: &[LayoutBox]) -> Vec<LayoutBox> {
    let mut out = Vec::new();
    // `chain[0]` is the container itself; `chain[i]` the element ancestors below it.
    let mut chain: Vec<Anc> = vec![Anc::of(cstyle)];
    // Pre-order walk; `depth` is the number of `chain` entries valid for the box.
    let mut stack: Vec<(&LayoutBox, usize)> = cchildren.iter().rev().map(|c| (c, 1)).collect();
    while let Some((b, depth)) = stack.pop() {
        if matches!(b.origin.role, BoxRole::Pseudo(_) | BoxRole::ListMarker) {
            continue;
        }
        let is_element = b.origin.role == BoxRole::Element && b.origin.node.is_some();
        let mut child_depth = depth;
        if is_element {
            chain.truncate(depth);
            if owned_by_root(&chain, &b.style) {
                let target = b.origin.node.unwrap_or(node);
                if let Some(ps) = compute_pseudo_element_style(
                    cx.doc, target, "scroll-marker", cx.sheet, &b.style, cx.viewport, cx.dark_mode,
                ) && matches!(ps.content, Content::Items(_))
                    && ps.display != Display::None
                {
                    out.push(pseudo_box(cx, target, ps, PseudoKind::ScrollMarker, vec![]));
                }
            }
            chain.push(Anc::of(&b.style));
            child_depth = depth + 1;
        }
        for c in b.children.iter().rev() {
            stack.push((c, child_depth));
        }
    }
    out
}

/// Whether a box with `style`, below the ancestors `chain` (`chain[0]` = the container whose
/// markers are collected), belongs to `chain[0]` rather than to a nested container.
fn owned_by_root(chain: &[Anc], style: &ComputedStyle) -> bool {
    let no_container_in = |upto: usize| !chain[1..=upto.min(chain.len() - 1)].iter().any(|a| a.is_container);
    match style.position {
        Position::Absolute => match chain.iter().rposition(|a| a.positioned) {
            Some(p) => no_container_in(p),
            None => false,
        },
        Position::Fixed => match chain.iter().rposition(|a| a.fixed_cb) {
            Some(a) => no_container_in(a),
            None => false,
        },
        _ => no_container_in(chain.len() - 1),
    }
}

/// Builds a block-container pseudo box originating from `node`; `extra` are the children that go in
/// front of the generated-content run (the group passes its markers as `extra`).
fn pseudo_box(cx: &ScrollPseudoCx, node: NodeId, ps: ComputedStyle, kind: PseudoKind, extra: Vec<LayoutBox>) -> LayoutBox {
    let segs = super::inline_build::content_to_inline_segments_as(
        &ps, cx.doc, node, QuoteSlot::Before, kind, cx.viewport, cx.counters, cx.registry,
    );
    let mut children = extra;
    if !segs.is_empty() {
        children.push(anon_inline_run(cx.host, &ps, segs, BoxRole::Pseudo(kind)));
    }
    LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node: cx.host,
        rect: Rect::ZERO,
        used_padding: None, used_line_height: ps.font_size * ps.line_height,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        style: Arc::new(ps),
        kind: BoxKind::Block,
        children,
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(kind) },
    }
}

/// Wraps runs of inline-level pseudo boxes into an `InlineBlockRow` (unless the container is a
/// flex/grid one, where every box is an item); an `inline` box is laid out as `inline-block`.
fn group_inline_levels(
    host: NodeId,
    container_style: &ComputedStyle,
    boxes: Vec<LayoutBox>,
    blockify: bool,
) -> Vec<LayoutBox> {
    if blockify {
        return boxes;
    }
    let mut out = Vec::with_capacity(boxes.len());
    let mut row: Vec<LayoutBox> = Vec::new();
    for mut b in boxes {
        // CSS 2.1 §9.7: an out-of-flow box is blockified and takes no part in a line.
        if matches!(b.style.position, Position::Absolute | Position::Fixed) && b.style.float_side == FloatSide::None {
            if matches!(b.style.display, Display::Inline | Display::InlineBlock) {
                Arc::make_mut(&mut b.style).display = Display::Block;
            }
            if !row.is_empty() {
                out.push(anon_inline_block_row(host, container_style, std::mem::take(&mut row)));
            }
            out.push(b);
            continue;
        }
        if matches!(
            b.style.display,
            Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        ) {
            if b.style.display == Display::Inline {
                Arc::make_mut(&mut b.style).display = Display::InlineBlock;
            }
            row.push(b);
        } else {
            if !row.is_empty() {
                out.push(anon_inline_block_row(host, container_style, std::mem::take(&mut row)));
            }
            out.push(b);
        }
    }
    if !row.is_empty() {
        out.push(anon_inline_block_row(host, container_style, row));
    }
    out
}
