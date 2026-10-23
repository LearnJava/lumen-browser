//! HTML LS §13.2.6.4.6 «after head» — a `<script>`/`<style>` between `</head>` and `<body>` is
//! processed by the «in head» rules but the insertion mode stays «after head» (BUG-1511). The
//! element's end tag used to return to «in head», where the following `<body>` popped `<html>`
//! and the whole body was built outside the tree: `document.body.isConnected` was false.

#![allow(clippy::unwrap_used)]

use lumen_dom::{Document, NodeData, NodeId};

fn find(doc: &Document, id: NodeId, local: &str) -> Option<NodeId> {
    if matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == local) {
        return Some(id);
    }
    doc.get(id).children.iter().find_map(|&c| find(doc, c, local))
}

fn parent_local(doc: &Document, id: NodeId) -> String {
    let parent = doc.get(id).parent.expect("has a parent");
    match &doc.get(parent).data {
        NodeData::Element { name, .. } => name.local.to_string(),
        _ => "#other".to_string(),
    }
}

#[test]
fn script_and_style_between_head_and_body_keep_body_under_html() {
    let doc = lumen_html_parser::parse(
        "<!DOCTYPE html><html><head><title>t</title></head><script>var a = 1;</script>\
         <style>p {}</style><body><div id=x>hi</div></body></html>",
    );
    let body = find(&doc, doc.root(), "body").expect("body");
    assert_eq!(parent_local(&doc, body), "html");
    let div = find(&doc, doc.root(), "div").expect("div");
    assert_eq!(parent_local(&doc, div), "body");
    // Both rawtext elements went to the head.
    assert_eq!(parent_local(&doc, find(&doc, doc.root(), "script").unwrap()), "head");
    assert_eq!(parent_local(&doc, find(&doc, doc.root(), "style").unwrap()), "head");
}
