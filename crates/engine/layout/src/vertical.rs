//! Vertical writing-mode layout (CSS Writing Modes L3 §3).
//!
//! Implements axis-swap layout for `writing-mode: vertical-rl` and
//! `writing-mode: vertical-lr`. In these modes:
//! - The *inline axis* runs top→bottom (physical y-direction).
//! - The *block axis* runs right→left (rl) or left→right (lr) — physical x.
//! - CSS `height` → inline-size → physical height.
//! - CSS `width`  → block-size  → physical width.
//!
//! Text orientation (rotating glyphs 90°) and vertical inline text flow
//! are Phase 2 tasks: this module only handles block-direction stacking.
//! InlineRun nodes inside vertical containers use horizontal text flow as
//! a stub; glyphs appear sideways but positions are correct.
//!
//! Algorithm sketch (vertical-rl):
//! 1. Inline-size (physical height) comes from CSS `height` or `available_height`.
//! 2. Children stack along the block axis: rightmost child has the largest x;
//!    each subsequent child's x decreases by its physical width (= block-size).
//! 3. The container's physical width is the sum of all children's physical widths
//!    plus padding+border, unless CSS `width` is set explicitly.
//!
//! For `vertical-lr` the only change is the cursor direction: leftmost child
//! has the smallest x; the cursor increments rather than decrements.

use lumen_core::ext::HyphenationProvider;
use lumen_core::geom::{Rect, Size};

use crate::TextMeasurer;
use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{BoxSizing, Length, WritingMode};

/// Lay out a Block/FlowRoot box in vertical writing mode.
///
/// Called from `lay_out()` in `box_tree.rs` when the element's
/// `style.writing_mode` is `VerticalRl`, `VerticalLr`, `SidewaysRl`, or `SidewaysLr`.
///
/// # Parameters
/// - `b`: the box to lay out (modified in place).
/// - `start_x`, `start_y`: top-left corner of the containing block's content area.
/// - `available_width`: physical width available; in vertical mode this is the
///   available *block-size* (room for children to stack horizontally).
/// - `available_height`: physical height available; in vertical mode this is the
///   available *inline-size* (room for the inline axis = lines of text).
/// - `measurer`, `viewport`, `pcb`, `hp`: forwarded to child layout.
///
/// # Axis mapping
/// - `vertical-rl` / `sideways-rl`: block direction is right→left (x decreases).
/// - `vertical-lr` / `sideways-lr`: block direction is left→right (x increases).
/// - In both cases the inline direction is top→bottom (y increases).
///
/// # Limitations (Phase 0 stub)
/// - InlineRun children fall back to horizontal text flow (sideways glyphs).
/// - Margin collapsing along the block axis is not implemented.
/// - Floats / `clear` are ignored inside vertical contexts.
/// - `min-/max-width` / `min-/max-height` are not clamped in vertical mode.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_vertical_block(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    let s = b.style.clone();
    let em = s.font_size;

    // Physical box-model offsets. In vertical mode CSS sides keep their
    // *physical* meaning (top stays top, left stays left): the cascade does
    // not re-map padding/border to logical sides. This matches the Writing
    // Modes L3 spec — only width/height swap roles.
    let cb_for_percents = available_width.max(0.0);
    let margin_left = s.margin_left.resolve_or_zero(em, cb_for_percents, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb_for_percents, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb_for_percents, viewport);

    let border_left = s.border_left_width;
    let border_right = s.border_right_width;
    let border_top = s.border_top_width;
    let border_bottom = s.border_bottom_width;

    let frame_horiz = padding_left + padding_right + border_left + border_right;
    let frame_vert = padding_top + padding_bottom + border_top + border_bottom;

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    // Inline-size (physical height) — from CSS `height`, fall back to available.
    // `height` is the inline axis in vertical mode; auto means "fill available
    // inline-size", mirroring how `width: auto` fills the available inline-size
    // in horizontal-tb.
    let inline_size_avail = available_height
        .unwrap_or(viewport.height)
        .max(0.0);
    let inline_size = resolve_axis_size(
        s.height.as_ref(),
        em,
        Some(inline_size_avail),
        viewport,
        s.box_sizing,
        frame_vert,
    )
    .unwrap_or(inline_size_avail);

    b.rect.height = inline_size.max(frame_vert);

    // Block-size (physical width) — from CSS `width`. If absent, the container
    // shrinks to fit its children: we lay out children first, then sum their
    // physical widths.
    let explicit_block_size = resolve_axis_size(
        s.width.as_ref(),
        em,
        Some(available_width.max(0.0)),
        viewport,
        s.box_sizing,
        frame_horiz,
    );

    // Inline-axis content extent (physical height for children to use).
    let content_inline = (inline_size - frame_vert).max(0.0);
    let content_y = b.rect.y + border_top + padding_top;

    // Block-axis content cursor (physical x for stacking).
    let is_rtl = matches!(
        s.writing_mode,
        WritingMode::VerticalRl | WritingMode::SidewaysRl
    );

    // If the container has explicit width, the children's available block
    // extent is bounded by that width; otherwise grow as needed.
    let content_block_avail = match explicit_block_size {
        Some(bs) => (bs - frame_horiz).max(0.0),
        None => (available_width - margin_left - frame_horiz).max(0.0),
    };

    // Starting x for the children's stacking cursor:
    //   vertical-rl: cursor starts at the right edge of the content box and
    //                moves leftwards as children are placed.
    //   vertical-lr: cursor starts at the left edge of the content box and
    //                moves rightwards.
    let content_x_left = b.rect.x + border_left + padding_left;
    let mut cursor_block_consumed: f32 = 0.0;

    for child in &mut b.children {
        if matches!(child.kind, BoxKind::Skip) {
            child.rect = Rect::new(content_x_left, content_y, 0.0, 0.0);
            continue;
        }

        // Tentative placement: lay the child out at the left content edge.
        // The child's own logic (recursive vertical lay-out, or horizontal
        // fallback for InlineRun) will write into child.rect.width / .height.
        //
        // The two "available_*" parameters retain their PHYSICAL meaning across
        // writing modes (CSS Writing Modes L3 §5: containing-block dimensions
        // are physical; only `width`/`height` semantics swap). So:
        // - available_width  = remaining physical width  = remaining block-size
        // - available_height = parent's content inline-size = physical height
        //
        // The recursive vertical layout then re-interprets these: it reads
        // `available_height` (physical) as the inline-size basis for CSS
        // `height`, and uses `available_width` for the block-axis cursor.
        //
        // Horizontal-fallback children (InlineRun, etc.) treat available_width
        // as physical width — they get the remaining block extent, which
        // produces sideways text inside the inline-axis strip. Acceptable
        // Phase 0 behaviour.
        let remaining_block = (content_block_avail - cursor_block_consumed).max(0.0);

        crate::box_tree::lay_out_for_vertical(
            child,
            content_x_left,
            content_y,
            remaining_block,
            Some(content_inline),
            measurer,
            viewport,
            pcb,
            hp,
        );

        // child.rect.width is the child's physical width = block-size consumed.
        let child_block = child.rect.width.max(0.0);

        // Reposition the child to the correct physical x along the block axis.
        let placed_x = if is_rtl {
            // vertical-rl: rightmost cursor minus consumed-so-far minus this child's width.
            let right_edge = content_x_left + content_block_avail;
            right_edge - cursor_block_consumed - child_block
        } else {
            content_x_left + cursor_block_consumed
        };

        // Shift child (and any nested geometry produced during its layout).
        let dx = placed_x - child.rect.x;
        if dx != 0.0 {
            shift_subtree_x(child, dx);
        }

        cursor_block_consumed += child_block;
    }

    // Finalise physical width: explicit CSS width wins; otherwise grow to fit
    // children plus padding+border.
    b.rect.width = if let Some(bs) = explicit_block_size {
        bs.max(frame_horiz)
    } else {
        cursor_block_consumed + frame_horiz
    };
}

/// Resolve an axis-sizing CSS length (`width` or `height` in vertical mode).
///
/// Returns the border-box size in CSS px when the length is resolvable;
/// returns `None` for `auto`, intrinsic keywords (Phase 0), or percentage
/// without a basis. Applies `box-sizing` (`content-box` adds padding+border).
fn resolve_axis_size(
    len: Option<&Length>,
    em: f32,
    basis: Option<f32>,
    viewport: Size,
    sizing: BoxSizing,
    frame: f32,
) -> Option<f32> {
    let len = len?;
    if len.is_intrinsic() {
        return None;
    }
    let raw = len.resolve(em, basis, viewport)?;
    let bb = match sizing {
        BoxSizing::ContentBox => raw + frame,
        BoxSizing::BorderBox => raw.max(frame),
    };
    Some(bb.max(0.0))
}

/// Translate every rect under `b` by `dx` along the x axis.
///
/// Required because the child's recursive layout positions descendants
/// relative to the tentative `content_x_left`; once the parent commits the
/// child's true physical x (right→left for `vertical-rl`), the whole subtree
/// must follow.
fn shift_subtree_x(b: &mut LayoutBox, dx: f32) {
    b.rect.x += dx;
    if let BoxKind::InlineRun { lines, .. } = &mut b.kind {
        for line in lines.iter_mut() {
            for frag in line.iter_mut() {
                frag.x += dx;
            }
        }
    }
    for c in &mut b.children {
        shift_subtree_x(c, dx);
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::geom::Size;

    use super::*;
    use crate::BoxKind;

    fn lay(html: &str, css: &str) -> LayoutBox {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        crate::box_tree::layout(&doc, &sheet, Size::new(800.0, 600.0))
    }

    /// Walk the tree to find the first descendant `Block` element whose style
    /// has the requested writing mode set (i.e. the test's `<div>` under test).
    fn find_vertical_block(b: &LayoutBox) -> Option<&LayoutBox> {
        if matches!(b.kind, BoxKind::Block)
            && !matches!(b.style.writing_mode, WritingMode::HorizontalTb)
        {
            return Some(b);
        }
        for c in &b.children {
            if let Some(found) = find_vertical_block(c) {
                return Some(found);
            }
        }
        None
    }

    fn first_non_skip_child(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip))
    }

    #[test]
    fn vertical_rl_container_height_is_inline_size() {
        let root = lay(
            "<div id=v><div></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 300px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.height - 200.0).abs() < 0.5,
            "expected physical height 200 (CSS height = inline-size), got {}",
            v.rect.height,
        );
        assert!(
            (v.rect.width - 300.0).abs() < 0.5,
            "expected physical width 300 (CSS width = block-size), got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_lr_container_height_is_inline_size() {
        let root = lay(
            "<div id=v><div></div></div>",
            "#v { writing-mode: vertical-lr; height: 250px; width: 120px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.height - 250.0).abs() < 0.5,
            "expected physical height 250, got {}",
            v.rect.height,
        );
        assert!(
            (v.rect.width - 120.0).abs() < 0.5,
            "expected physical width 120, got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_rl_single_child_fills_inline_extent() {
        // Single child with no explicit height should fill the parent's
        // inline-size (= parent's physical height = 200px).
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 100px; } \
             .c { width: 40px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        assert!(
            (c.rect.height - 200.0).abs() < 0.5,
            "child should fill parent's inline-size (200), got {}",
            c.rect.height,
        );
    }

    #[test]
    fn vertical_rl_children_stack_right_to_left() {
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; width: 200px; } \
             .a { width: 50px; } .b { width: 50px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children");
        // First child (.a) should be to the right of the second (.b).
        assert!(
            kids[0].rect.x > kids[1].rect.x,
            "vertical-rl: first child should be rightmost, got a.x={} b.x={}",
            kids[0].rect.x,
            kids[1].rect.x,
        );
    }

    #[test]
    fn vertical_lr_children_stack_left_to_right() {
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-lr; height: 100px; width: 200px; } \
             .a { width: 50px; } .b { width: 50px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children");
        // First child (.a) should be to the left of the second (.b).
        assert!(
            kids[0].rect.x < kids[1].rect.x,
            "vertical-lr: first child should be leftmost, got a.x={} b.x={}",
            kids[0].rect.x,
            kids[1].rect.x,
        );
    }

    #[test]
    fn vertical_rl_explicit_child_block_size() {
        let root = lay(
            "<div id=v><div class=c></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; width: 200px; } \
             .c { width: 60px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        let c = first_non_skip_child(v).expect("child missing");
        // Child inherits writing-mode from parent; its CSS width (60) is its
        // block-size = physical width.
        assert!(
            (c.rect.width - 60.0).abs() < 0.5,
            "child explicit width 60 should yield physical width 60, got {}",
            c.rect.width,
        );
    }

    #[test]
    fn vertical_rl_auto_container_width_grows() {
        // No explicit width on the container; physical width should equal the
        // sum of children's physical widths (here 40+30 = 70).
        let root = lay(
            "<div id=v><div class=a></div><div class=b></div></div>",
            "#v { writing-mode: vertical-rl; height: 100px; } \
             .a { width: 40px; } .b { width: 30px; }",
        );
        let v = find_vertical_block(&root).expect("vertical block missing");
        assert!(
            (v.rect.width - 70.0).abs() < 0.5,
            "auto-width container should shrink-to-fit children (70), got {}",
            v.rect.width,
        );
    }

    #[test]
    fn vertical_rl_nested_containers() {
        // Outer vertical-rl with two children; the second child is itself a
        // vertical-rl block. Both inner and outer should layout independently
        // without panicking, and the inner container should have a sensible
        // physical size.
        let root = lay(
            "<div id=v><div class=a></div><div id=inner><div class=ic></div></div></div>",
            "#v { writing-mode: vertical-rl; height: 200px; width: 300px; } \
             .a { width: 50px; } \
             #inner { writing-mode: vertical-rl; height: 100px; width: 80px; } \
             .ic { width: 25px; }",
        );
        let v = find_vertical_block(&root).expect("outer vertical block missing");
        // Outer must have explicit physical size (300×200).
        assert!((v.rect.width - 300.0).abs() < 0.5);
        assert!((v.rect.height - 200.0).abs() < 0.5);
        // The second child (#inner) must have its own explicit physical size (80×100).
        let kids: Vec<&LayoutBox> = v
            .children
            .iter()
            .filter(|c| !matches!(c.kind, BoxKind::Skip))
            .collect();
        assert!(kids.len() >= 2, "expected at least 2 children, got {}", kids.len());
        let inner = kids[1];
        assert!(
            (inner.rect.width - 80.0).abs() < 0.5,
            "inner #inner physical width should be 80, got {}",
            inner.rect.width,
        );
        assert!(
            (inner.rect.height - 100.0).abs() < 0.5,
            "inner #inner physical height should be 100, got {}",
            inner.rect.height,
        );
    }
}
