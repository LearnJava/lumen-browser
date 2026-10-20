// ── CSS Overflow L5: ::scroll-marker-group / ::scroll-marker / ::scroll-button() (SCROLL-MARKERS) ──

use super::super::{BoxRole, LayoutBox, PseudoKind};

fn lay(html: &str, css: &str) -> LayoutBox {
    super::super::layout(
        &lumen_html_parser::parse(html),
        &lumen_css_parser::parse(css),
        lumen_core::geom::Size::new(800.0, 600.0),
    )
}

/// Every box of pseudo `kind`, pre-order.
fn pseudo_boxes(b: &LayoutBox, kind: PseudoKind, out: &mut Vec<(f32, f32, usize)>) {
    if b.origin.role == BoxRole::Pseudo(kind) && !matches!(b.kind, super::super::BoxKind::InlineRun { .. } | super::super::BoxKind::Skip) {
        out.push((b.rect.width, b.rect.height, b.children.len()));
    }
    for c in &b.children {
        pseudo_boxes(c, kind, out);
    }
}

fn boxes_of(root: &LayoutBox, kind: PseudoKind) -> Vec<(f32, f32, usize)> {
    let mut v = Vec::new();
    pseudo_boxes(root, kind, &mut v);
    v
}

/// WPT `css-overflow/scroll-markers/scroll-marker-006.html`: nested containers; absolute and
/// fixed targets belong to a group only through their containing block.
#[test]
fn markers_go_to_the_nearest_container_group() {
    let root = lay(
        r#"<div class="container uncontained marker">
  <div class="marker"></div>
  <div class="container marker">
    <div class="marker"></div><div class="marker"></div><div class="marker"></div>
    <div class="uncontained marker" style="position:absolute;"></div>
    <div class="uncontained marker" style="position:fixed;"></div>
    <div class="container marker contain_abs">
      <div class="marker" style="position:absolute;"></div>
      <div class="uncontained marker" style="position:fixed;"></div>
      <div class="container contain_all">
        <div class="marker"></div>
        <div class="marker" style="position:fixed;"></div>
        <div class="marker"></div><div class="marker"></div>
      </div>
      <div class="marker"></div><div class="marker"></div><div class="marker"></div>
    </div>
  </div>
  <div class="marker"></div><div class="marker"></div>
</div>
<div class="uncontained marker"></div>"#,
        ".container{scroll-marker-group:before;overflow:hidden}
         .container::scroll-marker-group{display:flex;height:25px}
         .marker::scroll-marker{display:block;width:25px;height:25px;content:'';background:green}
         .contain_abs{position:relative} .contain_all{contain:layout}
         .uncontained::scroll-marker{z-index:100;content:'ERROR';background:red}",
    );
    let groups = boxes_of(&root, PseudoKind::ScrollMarkerGroup);
    assert_eq!(groups.len(), 4, "one group per container: {groups:?}");
    for (_, h, n) in &groups {
        assert_eq!(*n, 4, "each group holds four markers: {groups:?}");
        assert_eq!(*h, 25.0);
    }
    let markers = boxes_of(&root, PseudoKind::ScrollMarker);
    assert_eq!(markers.len(), 16);
    assert!(markers.iter().all(|&(w, h, _)| w == 25.0 && h == 25.0), "{markers:?}");
}

/// WPT `scroll-buttons-001.html`: `::scroll-button()` is generated on a plain `div`, as a sibling
/// before it; two 100x50 buttons stack into the reference's 100x100 rectangle.
#[test]
fn scroll_buttons_are_preceding_siblings() {
    let root = lay(
        "<div></div>",
        "div::scroll-button(block-end){content:'';background:green;display:flex;height:50px;width:100px;border:none}
         div::scroll-button(block-start){content:'';background:green;display:flex;height:50px;width:100px;border:none}",
    );
    let start = boxes_of(&root, PseudoKind::ScrollButtonBlockStart);
    let end = boxes_of(&root, PseudoKind::ScrollButtonBlockEnd);
    assert_eq!(start, vec![(100.0, 50.0, 0)]);
    assert_eq!(end, vec![(100.0, 50.0, 0)]);
    assert!(boxes_of(&root, PseudoKind::ScrollButtonInlineStart).is_empty(), "no content, no button");
}

/// A physical alias in the selector reaches the logical box through the writing mode.
#[test]
fn scroll_button_physical_alias() {
    let root = lay(
        "<div></div>",
        "div::scroll-button(down){content:'';display:flex;height:10px;width:20px}",
    );
    assert_eq!(boxes_of(&root, PseudoKind::ScrollButtonBlockEnd), vec![(20.0, 10.0, 0)]);
    let root = lay(
        "<div style=\"writing-mode:vertical-rl\"></div>",
        "div::scroll-button(down){content:'';display:flex;height:10px;width:20px}",
    );
    assert_eq!(boxes_of(&root, PseudoKind::ScrollButtonInlineEnd), vec![(20.0, 10.0, 0)]);
}

/// The group of WPT `scroll-marker-group-003.html`: no rendered box while empty and unstyled (a
/// `Skip` box keeps its computed style for CSSOM), a laid-out box once a rule gives it a display.
#[test]
fn empty_unstyled_group_has_no_box() {
    let html = "<div id=t><li></li><li></li></div>";
    let root = lay(html, "div{scroll-marker-group:after}");
    assert!(boxes_of(&root, PseudoKind::ScrollMarkerGroup).is_empty());
    let root = lay(html, "div{scroll-marker-group:after} div::scroll-marker-group{display:flex;height:20px;width:100px}");
    assert_eq!(boxes_of(&root, PseudoKind::ScrollMarkerGroup), vec![(100.0, 20.0, 0)]);
}
