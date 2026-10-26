//! `check-layout-th.js` emulation over vendored WPT files (css-align `justify-self` /
//! `align-self`, BUG-1526): every element carrying `data-expected-width|height` /
//! `data-offset-x|y` is compared with its laid-out border box, `offset-*` relative to the
//! padding edge of the nearest positioned ancestor (the document origin when there is none).

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};

use super::super::{LayoutBox, Position};

fn style_text(doc: &Document) -> String {
    fn walk(doc: &Document, id: NodeId, out: &mut String) {
        let node = doc.get(id);
        if let NodeData::Element { name, .. } = &node.data
            && name.local == "style"
        {
            for &child in &node.children {
                if let NodeData::Text(s) = &doc.get(child).data {
                    out.push_str(s);
                    out.push('\n');
                }
            }
            return;
        }
        for &child in &node.children {
            walk(doc, child, out);
        }
    }
    let mut out = String::new();
    walk(doc, doc.root(), &mut out);
    out
}

fn expectation(doc: &Document, b: &LayoutBox, key: &str) -> Option<f32> {
    let NodeData::Element { attrs, .. } = &doc.get(b.node).data else { return None };
    attrs.iter().find(|a| a.name.local == key).and_then(|a| a.value.trim().parse().ok())
}

fn check_box(doc: &Document, b: &LayoutBox, origin: (f32, f32), file: &str, out: &mut Vec<String>) {
    let label = || match &doc.get(b.node).data {
        NodeData::Element { name, attrs, .. } => {
            let style = attrs.iter().find(|a| a.name.local == "style").map_or("", |a| a.value.as_str());
            let class = attrs.iter().find(|a| a.name.local == "class").map_or("", |a| a.value.as_str());
            format!("{file}: <{} class=\"{class}\" style=\"{style}\">", name.local)
        }
        _ => String::new(),
    };
    for (key, got) in [
        ("data-expected-width", b.rect.width),
        ("data-expected-height", b.rect.height),
        ("data-offset-x", b.rect.x - origin.0),
        ("data-offset-y", b.rect.y - origin.1),
    ] {
        if let Some(want) = expectation(doc, b, key)
            && (want - got).abs() > 0.5
        {
            out.push(format!("{} {key}: want {want}, got {got}", label()));
        }
    }
    let child_origin = if matches!(b.style.position, Position::Static) {
        origin
    } else {
        (b.rect.x + b.style.border_left_width, b.rect.y + b.style.border_top_width)
    };
    for c in &b.children {
        check_box(doc, c, child_origin, file, out);
    }
}

/// Mismatches of one vendored file (`path` relative to `tests/wpt/`); empty when it passes.
pub(super) fn check_file(path: &str) -> Vec<String> {
    let full = format!("{}/../../../tests/wpt/{path}", env!("CARGO_MANIFEST_DIR"));
    let html = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(&style_text(&doc));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let mut out = Vec::new();
    check_box(&doc, &root, (0.0, 0.0), path, &mut out);
    out
}

/// Ad-hoc survey: `LUMEN_ALIGN_WPT=css/css-align/blocks/a.html,...` prints every mismatch.
#[test]
#[ignore = "manual survey"]
fn survey() {
    let Ok(list) = std::env::var("LUMEN_ALIGN_WPT") else { return };
    let mut bad = 0;
    for p in list.split(',').filter(|p| !p.is_empty()) {
        let v = check_file(p);
        println!("{} {p} ({} mismatches)", if v.is_empty() { "PASS" } else { "FAIL" }, v.len());
        for m in v.iter().take(12) {
            println!("    {m}");
        }
        bad += usize::from(!v.is_empty());
    }
    println!("failing files: {bad}");
}
