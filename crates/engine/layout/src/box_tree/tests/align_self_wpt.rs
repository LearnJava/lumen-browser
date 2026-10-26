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

thread_local! {
    static TITLE: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn expectation(doc: &Document, b: &LayoutBox, key: &str) -> Option<f32> {
    let NodeData::Element { attrs, .. } = &doc.get(b.node).data else { return None };
    attrs.iter().find(|a| a.name.local == key).and_then(|a| a.value.trim().parse().ok())
}

fn check_box(
    doc: &Document,
    b: &LayoutBox,
    origin: (f32, f32),
    file: &str,
    seen: &mut std::collections::HashSet<NodeId>,
    out: &mut Vec<String>,
) {
    let title = match &doc.get(b.node).data {
        NodeData::Element { attrs, .. } => attrs.iter().find(|a| a.name.local == "title").map(|a| a.value.to_string()),
        _ => None,
    };
    if let Some(t) = title {
        TITLE.with(|c| *c.borrow_mut() = t);
    }
    // Generated boxes (`::before`) share the node of their element: only the first box counts.
    let own = seen.insert(b.node);
    let label = || match &doc.get(b.node).data {
        NodeData::Element { name, attrs, .. } => {
            let style = attrs.iter().find(|a| a.name.local == "style").map_or("", |a| a.value.as_str());
            let class = attrs.iter().find(|a| a.name.local == "class").map_or("", |a| a.value.as_str());
            format!("{file} [{}]: <{} class=\"{class}\" style=\"{style}\">", TITLE.with(|c| c.borrow().clone()), name.local)
        }
        _ => String::new(),
    };
    for (key, got) in own.then_some([
        ("data-expected-width", b.rect.width),
        ("data-expected-height", b.rect.height),
        ("data-offset-x", b.rect.x - origin.0),
        ("data-offset-y", b.rect.y - origin.1),
    ]).into_iter().flatten() {
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
        check_box(doc, c, child_origin, file, seen, out);
    }
}

fn dump_box(doc: &Document, b: &LayoutBox, depth: usize) {
    let tag = match &doc.get(b.node).data {
        NodeData::Element { name, .. } => name.local.to_string(),
        _ => "#".to_string(),
    };
    println!("{:indent$}{tag} {:?} x={} y={} w={} h={}", "", std::mem::discriminant(&b.kind), b.rect.x, b.rect.y, b.rect.width, b.rect.height, indent = depth * 2);
    for c in &b.children {
        dump_box(doc, c, depth + 1);
    }
}

/// Mismatches of one vendored file (`path` relative to `tests/wpt/`); empty when it passes.
pub(super) fn check_file(path: &str) -> Vec<String> {
    let full = if std::path::Path::new(path).is_absolute() {
        path.to_string()
    } else {
        format!("{}/../../../tests/wpt/{path}", env!("CARGO_MANIFEST_DIR"))
    };
    let html = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
    check_html(path, &html)
}

/// Mismatches of one document; `path` names it in the messages.
pub(super) fn check_html(path: &str, html: &str) -> Vec<String> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&style_text(&doc));
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let mut out = Vec::new();
    check_box(&doc, &root, (0.0, 0.0), path, &mut Default::default(), &mut out);
    if std::env::var_os("LUMEN_ALIGN_DUMP").is_some() {
        dump_box(&doc, &root, 0);
    }
    out
}

/// Vendored css-align files that pass in full (BUG-1526). Left out, with the reason: `blocks/
/// justify-self-text-align-2` (`text-align: -webkit-right` as the source of `justify-self: normal`),
/// `abspos/*-default-overflow-*-vrl` (a vertical box's `fit-content` needs its min-content
/// height), `abspos/stretch-intrinsic-size-*` and `abspos/table-*-stretch` (stretching a replaced,
/// ratio-sized or table box), `content-distribution/default-overflow-alignment-*` (flex/grid
/// content distribution).
const PASSING: &[&str] = &[
    "css/css-align/blocks/justify-self-block-in-inline.html",
    "css/css-align/blocks/justify-self-htb-ltr-htb.html",
    "css/css-align/blocks/justify-self-htb-ltr-vlr.html",
    "css/css-align/blocks/justify-self-htb-ltr-vrl.html",
    "css/css-align/blocks/justify-self-htb-rtl-htb.html",
    "css/css-align/blocks/justify-self-htb-rtl-vlr.html",
    "css/css-align/blocks/justify-self-htb-rtl-vrl.html",
    "css/css-align/blocks/justify-self-vlr-ltr-htb.html",
    "css/css-align/blocks/justify-self-vlr-ltr-vlr.html",
    "css/css-align/blocks/justify-self-vlr-ltr-vrl.html",
    "css/css-align/blocks/justify-self-vlr-rtl-htb.html",
    "css/css-align/blocks/justify-self-vlr-rtl-vlr.html",
    "css/css-align/blocks/justify-self-vlr-rtl-vrl.html",
    "css/css-align/blocks/justify-self-vrl-ltr-htb.html",
    "css/css-align/blocks/justify-self-vrl-ltr-vlr.html",
    "css/css-align/blocks/justify-self-vrl-ltr-vrl.html",
    "css/css-align/blocks/justify-self-vrl-rtl-htb.html",
    "css/css-align/blocks/justify-self-vrl-rtl-vlr.html",
    "css/css-align/blocks/justify-self-vrl-rtl-vrl.html",
    "css/css-align/blocks/safe-justify-self-htb.html",
    "css/css-align/blocks/safe-justify-self-vlr.html",
    "css/css-align/blocks/safe-justify-self-vrl.html",
    "css/css-align/blocks/align-content-block-003.html",
    "css/css-align/blocks/align-content-block-004.html",
    "css/css-align/blocks/align-content-block-005.html",
    "css/css-align/blocks/align-content-block-012.html",
    "css/css-align/abspos/align-self-default-overflow-htb-ltr-htb.html",
    "css/css-align/abspos/align-self-default-overflow-htb-rtl-htb.html",
    "css/css-align/abspos/align-self-default-overflow-vrl-ltr-htb.html",
    "css/css-align/abspos/align-self-default-overflow-vrl-ltr-vrl.html",
    "css/css-align/abspos/align-self-default-overflow-vrl-rtl-htb.html",
    "css/css-align/abspos/align-self-default-overflow-vrl-rtl-vrl.html",
    "css/css-align/abspos/align-self-htb-ltr-htb.html",
    "css/css-align/abspos/align-self-htb-ltr-vlr.html",
    "css/css-align/abspos/align-self-htb-ltr-vrl.html",
    "css/css-align/abspos/align-self-htb-rtl-htb.html",
    "css/css-align/abspos/align-self-htb-rtl-vlr.html",
    "css/css-align/abspos/align-self-htb-rtl-vrl.html",
    "css/css-align/abspos/align-self-vlr-ltr-htb.html",
    "css/css-align/abspos/align-self-vlr-ltr-vlr.html",
    "css/css-align/abspos/align-self-vlr-ltr-vrl.html",
    "css/css-align/abspos/align-self-vlr-rtl-htb.html",
    "css/css-align/abspos/align-self-vlr-rtl-vlr.html",
    "css/css-align/abspos/align-self-vlr-rtl-vrl.html",
    "css/css-align/abspos/align-self-vrl-ltr-htb.html",
    "css/css-align/abspos/align-self-vrl-ltr-vlr.html",
    "css/css-align/abspos/align-self-vrl-ltr-vrl.html",
    "css/css-align/abspos/align-self-vrl-rtl-htb.html",
    "css/css-align/abspos/align-self-vrl-rtl-vlr.html",
    "css/css-align/abspos/align-self-vrl-rtl-vrl.html",
    "css/css-align/abspos/default-overflow-with-scroll.html",
    "css/css-align/abspos/justify-self-default-overflow-htb-ltr-htb.html",
    "css/css-align/abspos/justify-self-default-overflow-htb-ltr-vrl.html",
    "css/css-align/abspos/justify-self-default-overflow-htb-rtl-htb.html",
    "css/css-align/abspos/justify-self-default-overflow-htb-rtl-vrl.html",
    "css/css-align/abspos/justify-self-default-overflow-vrl-ltr-htb.html",
    "css/css-align/abspos/justify-self-default-overflow-vrl-rtl-htb.html",
    "css/css-align/abspos/justify-self-htb-ltr-htb.html",
    "css/css-align/abspos/justify-self-htb-ltr-vlr.html",
    "css/css-align/abspos/justify-self-htb-ltr-vrl.html",
    "css/css-align/abspos/justify-self-htb-rtl-htb.html",
    "css/css-align/abspos/justify-self-htb-rtl-vlr.html",
    "css/css-align/abspos/justify-self-htb-rtl-vrl.html",
    "css/css-align/abspos/justify-self-vlr-ltr-htb.html",
    "css/css-align/abspos/justify-self-vlr-ltr-vlr.html",
    "css/css-align/abspos/justify-self-vlr-ltr-vrl.html",
    "css/css-align/abspos/justify-self-vlr-rtl-htb.html",
    "css/css-align/abspos/justify-self-vlr-rtl-vlr.html",
    "css/css-align/abspos/justify-self-vlr-rtl-vrl.html",
    "css/css-align/abspos/justify-self-vrl-ltr-htb.html",
    "css/css-align/abspos/justify-self-vrl-ltr-vlr.html",
    "css/css-align/abspos/justify-self-vrl-ltr-vrl.html",
    "css/css-align/abspos/justify-self-vrl-rtl-htb.html",
    "css/css-align/abspos/justify-self-vrl-rtl-vlr.html",
    "css/css-align/abspos/justify-self-vrl-rtl-vrl.html",
    "css/css-align/abspos/safe-align-self-htb.html",
    "css/css-align/abspos/safe-align-self-vlr.html",
    "css/css-align/abspos/safe-align-self-vrl.html",
    "css/css-align/abspos/safe-justify-self-htb.html",
    "css/css-align/abspos/safe-justify-self-vlr.html",
    "css/css-align/abspos/safe-justify-self-vrl.html",
];

#[test]
fn vendored_css_align_self_alignment_files_pass() {
    let failures: Vec<String> = PASSING.iter().flat_map(|f| check_file(f)).take(20).collect();
    assert!(failures.is_empty(), "{}", failures.join("
"));
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
