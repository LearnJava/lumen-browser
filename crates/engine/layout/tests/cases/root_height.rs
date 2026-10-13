//! BUG-1348: `html { height: 100% }` resolves against the initial containing block.

use lumen_core::geom::Size;
use lumen_css_parser::parse as parse_css;
use lumen_html_parser::parse as parse_html;
use lumen_layout::layout;

#[test]
fn html_percent_height_resolves_against_viewport() {
    let doc = parse_html(r#"<html style="height:100%"><body style="margin:0"><p>x</p>"#);
    let tree = layout(&doc, &parse_css(""), Size::new(800.0, 600.0));
    let html = &tree.children[0];
    assert!((html.rect.height - 600.0).abs() < 0.5, "html height {}", html.rect.height);
}
