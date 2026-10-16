//! BUG-1361: `position: absolute` whose containing block lies outside an
//! `overflow` ancestor is not clipped by it (CSS 2.1 §11.1.1) — in the ordered
//! build neither its rect clip nor its scroll layer reaches the box.

use super::text_and_images::Fixed8;
use super::*;

/// Names of the clip/scroll commands open at the moment the green (`#008000`)
/// box is filled.
fn open_clips_at_green(html: &str, css: &str) -> Vec<&'static str> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    let stacking = lumen_layout::StackingTree::build(&tree);
    let order = lumen_layout::PaintOrder::from_tree(&stacking);
    let dl = build_display_list_ordered(&tree, &stacking, &order).0;
    let mut open: Vec<&'static str> = Vec::new();
    for c in &dl {
        match c {
            DisplayCommand::PushScrollLayer { .. } => open.push("scroll"),
            DisplayCommand::PushClipRect { .. } => open.push("clip"),
            DisplayCommand::PopScrollLayer | DisplayCommand::PopClip => {
                open.pop();
            }
            DisplayCommand::FillRect { color, .. } if *color == (Color { r: 0, g: 128, b: 0, a: 255 }) => {
                return open;
            }
            _ => {}
        }
    }
    panic!("нет зелёной заливки");
}

const A: &str = ".a{position:absolute;top:0;left:0;width:20px;height:20px;background:#008000}";

#[test]
fn abspos_escapes_static_scroll_container() {
    let open = open_clips_at_green(
        r#"<div style="height:100px;overflow:scroll;margin-top:40px"><div class="a"></div></div>"#,
        A,
    );
    assert!(open.is_empty(), "{open:?}");
}

#[test]
fn abspos_escapes_static_overflow_hidden() {
    let open = open_clips_at_green(
        r#"<div style="height:100px;overflow:hidden"><div class="a"></div></div>"#,
        A,
    );
    assert!(open.is_empty(), "{open:?}");
}

/// Containing block is the overflow box itself (`position: relative`) — still clipped.
#[test]
fn abspos_in_positioned_overflow_box_is_clipped() {
    let open = open_clips_at_green(
        r#"<div style="height:100px;overflow:hidden;position:relative"><div class="a"></div></div>"#,
        A,
    );
    assert_eq!(open, vec!["clip"]);
}

/// A static overflow box between the containing block and the child does not clip it,
/// the containing block's own clip still does.
#[test]
fn abspos_keeps_clip_of_cb_drops_clip_below_it() {
    let open = open_clips_at_green(
        r#"<div style="height:100px;overflow:hidden;position:relative"><div style="height:50px;overflow:hidden"><div class="a"></div></div></div>"#,
        A,
    );
    assert_eq!(open, vec!["clip"]);
}
