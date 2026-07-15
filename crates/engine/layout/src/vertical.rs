//! Vertical writing-mode layout (CSS Writing Modes L3 §3).
//!
//! Implements axis-swap layout for `writing-mode: vertical-rl` and
//! `writing-mode: vertical-lr`. In these modes:
//! - The *inline axis* runs top→bottom (physical y-direction).
//! - The *block axis* runs right→left (rl) or left→right (lr) — physical x.
//! - CSS `height` → inline-size → physical height.
//! - CSS `width`  → block-size  → physical width.
//!
//! Vertical inline text flow (`lay_out_vertical_inline_run` /
//! `wrap_inline_run_vertical`, below) is implemented: text wraps top→bottom
//! by inline-size, in addition to the block-direction stacking this header
//! used to describe as the only thing done here.
//!
//! Text orientation (rotating glyphs 90°) is a paint concern
//! (`docs/tasks/ph3-writing-mode-vertical.md`), not layout: this module only
//! computes column positions. The CPU rasterizer honors `text_orientation`
//! (Срез 1); the wgpu and femtovg backends still draw every run horizontally
//! regardless of orientation (Срезы 2+, pending).
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

use crate::{InlineFrag, InlineSegment, TextMeasurer};
use crate::box_tree::{measure_text_w_varied, strip_soft_hyphens, BoxKind, LayoutBox};
use crate::style::{BoxSizing, Length, WritingMode};

#[allow(dead_code)]
pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x303F |
        0x3040..=0x309F |
        0x30A0..=0x30FF |
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF |
        0xF900..=0xFAFF |
        0xFF00..=0xFFEF
    )
}

#[allow(dead_code)]
pub(crate) fn is_vertical(mode: WritingMode) -> bool {
    !matches!(mode, WritingMode::HorizontalTb)
}

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

// BUG-264 (lumen-layout portion): `lay_out_vertical_inline_run` / `wrap_inline_run_vertical`
// are declared after this test module (P3-vertical Phase 2 layout), tripping
// `clippy::items_after_test_module` and blocking the `-p lumen-layout --all-targets`
// finish gate for every role. Suppressing here is the idiomatic non-functional unblock
// (same class as BUG-263's `too_many_arguments` allows); reordering the functions is
// deferred. The wgpu/cpu_raster pieces of BUG-264 stay OPEN (lumen-paint, P5).
#[cfg(test)]
#[allow(clippy::items_after_test_module)]
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


#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_vertical_inline_run(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    _available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    _pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    let s = b.style.clone();
    let em = s.font_size;

    let inline_size_avail = available_height.unwrap_or(viewport.height).max(0.0);
    let padding_top = s.padding_top.resolve_or_zero(em, inline_size_avail, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, inline_size_avail, viewport);
    let frame_vert = padding_top + padding_bottom + s.border_top_width + s.border_bottom_width;
    let content_inline = (inline_size_avail - frame_vert).max(0.0);

    let BoxKind::InlineRun { segments, lines, .. } = &mut b.kind else {
        return;
    };
    let Some(m) = measurer else {
        return;
    };

    let wrap_budget = if s.white_space.is_nowrap() || s.text_wrap_mode == crate::style::TextWrapMode::Nowrap {
        f32::INFINITY
    } else {
        content_inline
    };

    *lines = wrap_inline_run_vertical(
        segments,
        wrap_budget,
        em,
        viewport,
        m,
        hp,
        s.white_space,
        s.word_break,
        s.overflow_wrap,
        s.writing_mode,
        s.text_orientation,
    );

    let total_advance: f32 = lines.iter().flat_map(|l| l.iter()).map(|f| f.width).sum();
    let min_height = em * s.line_height;
    let total_vertical_extent = total_advance.max(min_height);

    b.rect.x = start_x;
    b.rect.y = start_y;
    let col_width = em * s.line_height;
    b.rect.width = col_width;
    b.rect.height = total_vertical_extent;
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wrap_inline_run_vertical(
    segments: &[InlineSegment],
    max_height: f32,
    container_font_size: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    _hp: &dyn HyphenationProvider,
    _white_space: crate::style::WhiteSpace,
    _word_break: crate::style::WordBreak,
    _overflow_wrap: crate::style::OverflowWrap,
    _writing_mode: WritingMode,
    _text_orientation: crate::style::TextOrientation,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    let mut result: Vec<Vec<InlineFrag>> = vec![Vec::new()];
    let mut current_line: &mut Vec<InlineFrag> = result.last_mut().unwrap();
    let mut current_y: f32 = 0.0;
    let mut prev_trailing_ws: bool = false;

    for seg in segments {
        if seg.forced_break {
            if !current_line.is_empty() && !current_line.last().map(|f| f.text == "\n").unwrap_or(false) {
                current_line.push(InlineFrag {
                    x: 0.0,
                    y_offset: 0.0,
                    width: 0.0,
                    text: "\n".to_string(),
                    style: seg.style.clone(),
                    padding_left: 0.0,
                    padding_right: 0.0,
                    is_element_box: false,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: seg.source_char_offset,
                });
            }
            current_y = 0.0;
            prev_trailing_ws = false;
            continue;
        }

        let seg_lead_ws = seg.text.starts_with(|c: char| c.is_whitespace());
        let seg_trail_ws = seg.text.ends_with(|c: char| c.is_whitespace());

        if _white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            prev_trailing_ws = false;
            let style = &seg.style;
            let em_s = style.font_size;
            let ls = style.letter_spacing;
            let tab_size = style.tab_size;
            let pad_l = style.padding_left.resolve_or_zero(em_s, max_height, viewport);
            let _pad_r = style.padding_right.resolve_or_zero(em_s, max_height, viewport);
            let frag_h = measure_text_w_varied(&seg.text, em_s, ls, tab_size, &style.font_family, &style.font_variation_settings, m);
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: frag_h,
                text: seg.text.clone(),
                style: style.clone(),
                padding_left: pad_l,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
            current_y += frag_h;
            continue;
        }

        if let Some(img_src) = &seg.img_src {
            let img_advance = m.char_width(' ', container_font_size) * 3.0;
            if !current_line.is_empty() && current_y + img_advance > max_height {
                result.push(Vec::new());
                current_line = result.last_mut().unwrap();
                current_y = 0.0;
            }
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: img_advance,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                img_is_lazy: seg.img_is_lazy,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
            current_y += img_advance;
            prev_trailing_ws = seg_trail_ws;
            continue;
        }

        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        if raw_words.is_empty() {
            if seg_lead_ws || seg_trail_ws {
                prev_trailing_ws = true;
            }
            continue;
        }

        let style = &seg.style;
        let em_s = style.font_size;
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let inter_word = space_w + ls + ws;
        let pad_l = style.padding_left.resolve_or_zero(em_s, max_height, viewport);
        let _pad_r = style.padding_right.resolve_or_zero(em_s, max_height, viewport);

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;
            let (display_word, _) = strip_soft_hyphens(raw_word);

            let frag_source_offset = {
                let raw_ptr = raw_word.as_ptr() as usize;
                let seg_ptr = seg.text.as_ptr() as usize;
                let word_off = if raw_ptr >= seg_ptr && raw_ptr <= seg_ptr + seg.text.len() {
                    (raw_ptr - seg_ptr) as u32
                } else {
                    0u32
                };
                seg.source_char_offset.saturating_add(word_off)
            };

            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_h = measure_text_w_varied(&display_word, em_s, ls, 0.0, &style.font_family, &style.font_variation_settings, m);
            let word_inter = if is_seg_first && !(prev_trailing_ws || seg_lead_ws) { 0.0 } else { inter_word };
            let gap = if current_line.is_empty() { 0.0 } else { word_inter };

            let needs_wrap = !current_line.is_empty()
                && current_y + gap + pre + word_h > max_height;

            if needs_wrap {
                current_line.push(InlineFrag {
                    x: current_y,
                    y_offset: 0.0,
                    width: gap + pre,
                    text: " ".repeat(0),
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: 0.0,
                    is_element_box: seg.is_element_box,
                    img_src: None,
                    img_is_lazy: false,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: frag_source_offset,
                });
                result.push(Vec::new());
                current_line = result.last_mut().unwrap();
                current_y = 0.0;
            }

            let _entry_pre = if is_seg_first { pre } else { 0.0 };
            current_line.push(InlineFrag {
                x: current_y,
                y_offset: 0.0,
                width: word_h,
                text: display_word.to_string(),
                style: style.clone(),
                padding_left: if is_seg_first { pad_l } else { 0.0 },
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                img_is_lazy: false,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: frag_source_offset,
            });
            current_y += word_h + post;
            prev_trailing_ws = seg_trail_ws;
        }
    }

    if result.is_empty() {
        result.push(Vec::new());
    }
    result
}

