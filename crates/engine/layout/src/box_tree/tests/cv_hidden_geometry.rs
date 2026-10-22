//! BUG-1400 — a descendant of a `content-visibility: hidden` box still answers geometry queries
//! (CSS Containment L2): the skipped subtree is laid out as a probe, read through
//! `collect_layout_rects`, while the hidden box itself stays a childless, size-contained box.

use lumen_core::geom::Size;

const CSS: &str = ".cv { content-visibility: hidden; width: 100px; margin-left: 20px }";
const HTML: &str = r#"<div class="cv" id="cv"><div id="zz" style="height:40px;width:30px;margin-left:5px">t</div><span id="sp">text here</span></div><p id="after">x</p>"#;

fn by_id(doc: &lumen_dom::Document, id: &str) -> u32 {
    (0..doc.len())
        .map(lumen_dom::NodeId::from_index)
        .find(|&n| doc.get(n).get_attr("id") == Some(id))
        .unwrap_or_else(|| panic!("no #{id}"))
        .index() as u32
}

fn rects(css: &str, html: &str) -> (lumen_dom::Document, super::super::LayoutBox, std::collections::HashMap<u32, [f32; 4]>) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let map = crate::collect_layout_rects(&root, &doc);
    (doc, root, map)
}

#[test]
fn descendant_of_hidden_box_has_a_real_rect() {
    let (doc, _root, map) = rects(CSS, HTML);
    let zz = map.get(&by_id(&doc, "zz")).expect("hidden descendant is in the rect map");
    assert_eq!((zz[2], zz[3]), (30.0, 40.0), "{zz:?}");
    assert_eq!(zz[0], 8.0 + 20.0 + 5.0, "body margin + the hidden box's margin + its own: {zz:?}");
    let sp = map.get(&by_id(&doc, "sp")).expect("inline descendant is in the rect map");
    // Headless layout has no text measurer: the width is 0, the line box is not.
    assert_eq!((sp[0], sp[1]), (28.0, 48.0), "{sp:?}");
    assert!((sp[3] - 19.2).abs() < 0.01, "{sp:?}");
}

#[test]
fn hidden_box_itself_stays_empty_and_does_not_grow() {
    let (doc, root, map) = rects(CSS, HTML);
    let cv = map[&by_id(&doc, "cv")];
    assert_eq!((cv[0], cv[2], cv[3]), (28.0, 100.0, 0.0), "{cv:?}");
    // The paragraph after it sits where it would after an empty box: the probe adds no height.
    let (empty_doc, _, empty) = rects(CSS, r#"<div class="cv" id="cv"></div><p id="after">x</p>"#);
    assert_eq!(map[&by_id(&doc, "after")], empty[&by_id(&empty_doc, "after")]);
    fn find(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if b.style.content_visibility == crate::style::ContentVisibility::Hidden {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    let hidden = find(&root).expect("hidden box");
    assert!(hidden.children.is_empty(), "paint and hit-testing see no children");
}

#[test]
fn nested_hidden_boxes_answer_too() {
    let html = r#"<div class="cv"><div class="cv" style="margin-left:0"><div id="deep" style="height:7px">x</div></div></div>"#;
    let (doc, _root, map) = rects(CSS, html);
    let deep = map.get(&by_id(&doc, "deep")).expect("nested hidden descendant");
    assert_eq!(deep[3], 7.0, "{deep:?}");
}
