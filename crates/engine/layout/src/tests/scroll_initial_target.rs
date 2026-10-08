use super::*;
use crate::style::ScrollInitialTarget;

// ──────── CSS Scroll Snap L2 §4 — scroll-initial-target ────────

fn find_by_id<'a>(root: &'a LayoutBox, doc: &lumen_dom::Document, id: &str) -> &'a LayoutBox {
    fn go<'a>(b: &'a LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a LayoutBox> {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
        {
            return Some(b);
        }
        b.children.iter().find_map(|c| go(c, doc, id))
    }
    go(root, doc, id).expect("element with id")
}

fn lay_doc(html: &str, css: &str) -> (LayoutBox, lumen_dom::Document) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&format!("body{{margin:0}}{css}"));
    (layout(&doc, &sheet, Size::new(800.0, 600.0)), doc)
}

#[test]
fn scroll_initial_target_initial_is_none() {
    let root = lay("<p>x</p>", "");
    assert_eq!(first_p_style(&root).scroll_initial_target, ScrollInitialTarget::None);
}

#[test]
fn scroll_initial_target_parses_nearest_and_none() {
    let root = lay("<p>x</p>", "p { scroll-initial-target: nearest; }");
    assert_eq!(first_p_style(&root).scroll_initial_target, ScrollInitialTarget::Nearest);
    let root = lay("<p>x</p>", "p { scroll-initial-target: nearest; scroll-initial-target: none; }");
    assert_eq!(first_p_style(&root).scroll_initial_target, ScrollInitialTarget::None);
}

#[test]
fn scroll_initial_target_invalid_value_ignored() {
    let root = lay(
        "<p>x</p>",
        "p { scroll-initial-target: nearest; scroll-initial-target: 100px; }",
    );
    assert_eq!(first_p_style(&root).scroll_initial_target, ScrollInitialTarget::Nearest);
}

#[test]
fn scroll_initial_target_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { scroll-initial-target: nearest; }",
    );
    // `first_p_style` отдаёт первый блок (div) — берём его дочерний `p`.
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(div.style.scroll_initial_target, ScrollInitialTarget::Nearest);
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.scroll_initial_target, ScrollInitialTarget::None);
}

#[test]
fn initial_target_scrolls_container_to_target_start() {
    let (mut root, doc) = lay_doc(
        "<div id=s><div style='height:500px'></div><div id=t></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } \
         #t { height: 50px; scroll-initial-target: nearest; }",
    );
    let res = apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0));
    assert!(res.is_some());
    // Контейнер выровнен по началу цели: 500px до цели, предел прокрутки 450.
    assert_eq!(find_by_id(&root, &doc, "s").scroll_y, 450.0);
}

#[test]
fn initial_target_without_marker_is_noop() {
    let (mut root, doc) = lay_doc(
        "<div id=s><div style='height:500px'></div><div id=t></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } #t { height: 50px; }",
    );
    assert!(apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0)).is_none());
    assert_eq!(find_by_id(&root, &doc, "s").scroll_y, 0.0);
}

#[test]
fn initial_target_respects_snap_align_center() {
    let (mut root, doc) = lay_doc(
        "<div id=s><div style='height:300px'></div><div id=t></div><div style='height:600px'></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } \
         #t { height: 50px; scroll-initial-target: nearest; scroll-snap-align: center; }",
    );
    apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0));
    // Центр цели = 325, центр порта = 50 → смещение 275.
    assert_eq!(find_by_id(&root, &doc, "s").scroll_y, 275.0);
}

#[test]
fn initial_target_scroll_margin_expands_area() {
    let (mut root, doc) = lay_doc(
        "<div id=s><div style='height:300px'></div><div id=t></div><div style='height:600px'></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } \
         #t { height: 50px; scroll-initial-target: nearest; scroll-margin-top: 20px; }",
    );
    apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0));
    assert_eq!(find_by_id(&root, &doc, "s").scroll_y, 280.0);
}

#[test]
fn initial_target_nested_scrollers_both_scrolled() {
    let (mut root, doc) = lay_doc(
        "<div id=o><div style='height:400px'></div>\
         <div id=i><div style='height:400px'></div><div id=t></div><div style='height:400px'></div></div>\
         <div style='height:800px'></div></div>",
        "#o { overflow: scroll; width: 200px; height: 200px; } \
         #i { overflow: scroll; width: 150px; height: 150px; } \
         #t { height: 40px; scroll-initial-target: nearest; }",
    );
    apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0));
    assert_eq!(find_by_id(&root, &doc, "i").scroll_y, 400.0);
    // Внешний: внутренний уже сдвинул цель, `i` целиком выравнивается по началу
    // области цели внутри внешнего порта.
    assert!(find_by_id(&root, &doc, "o").scroll_y > 0.0);
}

#[test]
fn initial_target_page_viewport_offset_returned() {
    let (mut root, _doc) = lay_doc(
        "<div style='height:1000px'></div><div id=t style='height:50px'></div><div style='height:1000px'></div>",
        "#t { scroll-initial-target: nearest; }",
    );
    let res = apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0)).unwrap();
    assert_eq!(res.page, Some((0.0, 1000.0)));
}

#[test]
fn initial_target_inside_scroller_does_not_scroll_page() {
    let (mut root, _doc) = lay_doc(
        "<div id=s><div style='height:500px'></div><div id=t></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } \
         #t { height: 50px; scroll-initial-target: nearest; }",
    );
    let res = apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0)).unwrap();
    assert_eq!(res.page, None);
}

#[test]
fn initial_target_first_wins_per_scroller() {
    let (mut root, doc) = lay_doc(
        "<div id=s><div id=a style='height:100px'></div><div style='height:400px'></div>\
         <div id=b style='height:50px'></div><div style='height:400px'></div></div>",
        "#s { overflow: scroll; width: 100px; height: 100px; } \
         #a, #b { scroll-initial-target: nearest; }",
    );
    apply_scroll_initial_targets(&mut root, Size::new(800.0, 600.0));
    assert_eq!(find_by_id(&root, &doc, "s").scroll_y, 0.0);
}
