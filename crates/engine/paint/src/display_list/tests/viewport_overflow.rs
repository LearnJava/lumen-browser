//! CSS Overflow L3 §3.5: `overflow` of `<html>` (or of `<body>` when `<html>`'s is `visible`) goes
//! to the viewport; the element itself does not clip its content to its own box.

use super::text_and_images::Fixed8;
use super::*;

/// Number of clip commands open when the green (`#008000`) box is filled.
fn open_clips_at_green(html: &str, css: &str) -> usize {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = lumen_layout::layout_measured(&doc, &sheet, Size::new(800.0, 600.0), &Fixed8);
    let dl = build_display_list(&tree);
    let mut open = 0usize;
    for c in &dl {
        match c {
            DisplayCommand::PushScrollLayer { .. } | DisplayCommand::PushClipRect { .. } => open += 1,
            DisplayCommand::PopScrollLayer | DisplayCommand::PopClip => open -= 1,
            DisplayCommand::FillRect { color, .. } if *color == (Color { r: 0, g: 128, b: 0, a: 255 }) => {
                return open;
            }
            _ => {}
        }
    }
    panic!("нет зелёной заливки");
}

const BODY: &str = r#"<div class="a"></div>"#;
const G: &str = ".a{width:20px;height:20px;background:#008000}";

#[test]
fn body_overflow_hidden_does_not_clip_when_root_visible() {
    assert_eq!(open_clips_at_green(BODY, &format!("body{{overflow:hidden;height:5px}}{G}")), 0);
}

#[test]
fn root_overflow_hidden_does_not_clip() {
    assert_eq!(open_clips_at_green(BODY, &format!("html{{overflow:hidden;height:5px}}{G}")), 0);
}

/// `<html>` has a non-visible `overflow`: it keeps the viewport, `<body>`'s own value applies to `<body>`.
#[test]
fn body_overflow_hidden_clips_when_root_not_visible() {
    assert_eq!(
        open_clips_at_green(BODY, &format!("html{{overflow:auto}}body{{overflow:hidden;height:5px}}{G}")),
        2, // `<html>`'s own scroll layer + `<body>`'s clip
    );
}

#[test]
fn ordinary_overflow_hidden_still_clips() {
    assert_eq!(open_clips_at_green(r#"<div style="overflow:hidden;height:5px"><div class="a"></div></div>"#, G), 1);
}
