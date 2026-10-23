//! FRAME-7: the text-entry caret bar inside a focused `<input>` rides the
//! same per-`NodeId` compositor-override map as CSS-animation offload
//! (`CompositorOverride::caret`, threaded through `fill_buckets` /
//! `emit_box_self` into `emit_form_control_indicator`). These tests mirror
//! the pattern in `anim_and_chrome.rs` / `shadows_and_transforms.rs` for the
//! existing opacity/transform/color overrides, applied to the new field.

use super::*;
use lumen_dom::NodeId;

fn find_input_node(b: &lumen_layout::LayoutBox) -> Option<NodeId> {
    if matches!(b.kind, BoxKind::FormControl { .. }) {
        return Some(b.node);
    }
    b.children.iter().find_map(find_input_node)
}

fn caret_fixture(value: &str) -> (lumen_layout::LayoutBox, NodeId) {
    let html = format!(r#"<input type="text" value="{value}" style="font-size:16px">"#);
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse("");
    let tree = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let node = find_input_node(&tree).expect("input box must exist");
    (tree, node)
}

#[test]
fn no_override_emits_no_caret_fill_rect() {
    let (tree, _node) = caret_fixture("abc");
    let dl = build_display_list(&tree);
    // Value text is drawn (DrawText exists) but no 1px-wide FillRect for a
    // caret — nothing in this crate emits a 1.0-width FillRect otherwise.
    let has_narrow_fill = dl.iter().any(|c| matches!(c,
        DisplayCommand::FillRect { rect, .. } if (rect.width - 1.0).abs() < 0.001));
    assert!(!has_narrow_fill, "no compositor override → no caret bar");
}

#[test]
fn caret_override_emits_fill_rect_at_char_position() {
    let (tree, node) = caret_fixture("abc");
    let stacking_tree = StackingTree::build(&tree);
    let order = PaintOrder::from_tree(&stacking_tree);

    let mut overrides = HashMap::new();
    overrides.insert(node, CompositorOverride { caret: Some(2), ..Default::default() });
    let frame = CompositorAnimFrame { overrides, has_active: false };
    let dl = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));

    let input_box = {
        fn find(b: &lumen_layout::LayoutBox, want: NodeId) -> Option<&lumen_layout::LayoutBox> {
            if b.node == want { return Some(b); }
            b.children.iter().find_map(|c| find(c, want))
        }
        find(&tree, node).expect("input box")
    };
    // Same geometry `emit_input_caret` computes: content_x + 2px inset,
    // advance = font_size * 0.5 per char before the cursor (2 chars: "ab").
    let bl = input_box.style.border_left_width;
    let expected_x = input_box.rect.x + bl + 2.0 + 16.0 * 0.5 * 2.0;

    let caret = dl.iter().find_map(|c| match c {
        DisplayCommand::FillRect { rect, .. } if (rect.width - 1.0).abs() < 0.001 => Some(*rect),
        _ => None,
    });
    let rect = caret.expect("override with caret=Some(_) must emit a 1px-wide FillRect");
    assert!((rect.x - expected_x).abs() < 0.01, "caret x expected {expected_x}, got {}", rect.x);
}

#[test]
fn caret_color_auto_follows_text_color() {
    let (tree, node) = caret_fixture("a");
    let stacking_tree = StackingTree::build(&tree);
    let order = PaintOrder::from_tree(&stacking_tree);
    let mut overrides = HashMap::new();
    overrides.insert(node, CompositorOverride { caret: Some(0), ..Default::default() });
    let frame = CompositorAnimFrame { overrides, has_active: false };
    let dl = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));

    // Default UA text color is black; `caret-color: auto` (never set here)
    // must resolve to the same color, not to some other default.
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let caret_color = dl.iter().find_map(|c| match c {
        DisplayCommand::FillRect { rect, color } if (rect.width - 1.0).abs() < 0.001 => Some(*color),
        _ => None,
    });
    assert_eq!(caret_color, Some(black), "caret-color:auto must follow the text color");
}

/// UX-CARET: the OS text-cursor thickness reaches the bar.
#[test]
fn caret_width_override_sets_bar_width() {
    let (tree, node) = caret_fixture("abc");
    let stacking_tree = StackingTree::build(&tree);
    let order = PaintOrder::from_tree(&stacking_tree);

    let mut overrides = HashMap::new();
    overrides.insert(
        node,
        CompositorOverride { caret: Some(1), caret_width: Some(3.5), ..Default::default() },
    );
    let frame = CompositorAnimFrame { overrides, has_active: false };
    let dl = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));

    let wide = dl
        .iter()
        .any(|c| matches!(c, DisplayCommand::FillRect { rect, .. } if (rect.width - 3.5).abs() < 0.001));
    assert!(wide, "caret_width = 3.5 must emit a 3.5-wide FillRect");
}

/// CSS UI L4 §6.2 `caret-shape`: the shape decides the caret rectangle.
fn caret_shape_fill(shape: &str) -> Rect {
    let html = format!(
        r#"<input type="text" value="abc" style="font-size:16px; caret-shape:{shape}; caret-color:lime">"#
    );
    let doc = lumen_html_parser::parse(&html);
    let tree = lumen_layout::layout(&doc, &lumen_css_parser::parse(""), Size::new(800.0, 600.0));
    let node = find_input_node(&tree).expect("input box must exist");
    let stacking_tree = StackingTree::build(&tree);
    let order = PaintOrder::from_tree(&stacking_tree);
    let mut overrides = HashMap::new();
    overrides.insert(node, CompositorOverride { caret: Some(1), caret_width: Some(2.0), ..Default::default() });
    let frame = CompositorAnimFrame { overrides, has_active: false };
    let dl = build_display_list_ordered_with_anim(&tree, &stacking_tree, &order, Some(&frame));
    // The caret is the last FillRect: it is emitted after the value text.
    dl.iter()
        .rev()
        .find_map(|c| match c {
            DisplayCommand::FillRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("caret FillRect")
}

#[test]
fn caret_shape_bar_block_underscore_rects() {
    let bar = caret_shape_fill("bar");
    let block = caret_shape_fill("block");
    let under = caret_shape_fill("underscore");
    assert_eq!(bar.width, 2.0);
    assert_eq!(block.width, 8.0, "block = one character cell (font-size * 0.5)");
    assert_eq!(block.height, bar.height);
    assert_eq!((under.width, under.height), (8.0, 2.0));
    assert!((under.y + under.height - (bar.y + bar.height)).abs() < 0.001, "underscore sits at the cell bottom");
}
