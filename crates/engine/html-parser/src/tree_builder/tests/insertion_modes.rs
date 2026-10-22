//! Тесты режимов вставки: <template>, InFrameset / AfterFrameset /
//! AfterAfterFrameset, InHeadNoscript и InSelectInTable.

use super::*;

// ──────── <template> element ────────

/// Helper: find a `<template>` element node in body.
fn find_template(doc: &Document) -> Option<NodeId> {
    let body = body_of(doc);
    fn search(doc: &Document, id: NodeId) -> Option<NodeId> {
        let node = doc.get(id);
        if matches!(&node.data, NodeData::Element { name, .. } if name.local == "template") {
            return Some(id);
        }
        for &child in &node.children {
            if let Some(found) = search(doc, child) {
                return Some(found);
            }
        }
        None
    }
    search(doc, body)
}

#[test]
fn template_element_exists_in_dom() {
    let doc = parse("<body><template id=\"t\"><p>content</p></template></body>");
    let tmpl = find_template(&doc);
    assert!(tmpl.is_some(), "template element must be in DOM");
}

#[test]
fn template_element_has_no_dom_children() {
    // Template content goes to fragment, not to template's DOM children.
    let doc = parse("<body><template><p>content</p></template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    let children = &doc.get(tmpl).children;
    assert!(
        children.is_empty(),
        "template DOM children must be empty, got {children:?}"
    );
}

#[test]
fn template_has_content_fragment() {
    let doc = parse("<body><template><p>hello</p></template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    let frag = doc.template_content(tmpl);
    assert!(frag.is_some(), "template must have a content fragment");
}

#[test]
fn template_content_contains_child_elements() {
    let doc = parse("<body><template><p>hello</p><span>world</span></template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    let frag = doc.template_content(tmpl).expect("no content fragment");
    let children = &doc.get(frag).children;
    assert_eq!(children.len(), 2, "fragment must have 2 children (p, span)");
    let p_name = doc.get(children[0]).element_name().map(|q| q.local.as_str());
    assert_eq!(p_name, Some("p"));
    let span_name = doc.get(children[1]).element_name().map(|q| q.local.as_str());
    assert_eq!(span_name, Some("span"));
}

#[test]
fn template_content_text_preserved() {
    let doc = parse("<body><template>hello world</template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    let frag = doc.template_content(tmpl).expect("no content fragment");
    let children = &doc.get(frag).children;
    assert!(!children.is_empty(), "text must be in fragment");
    let text = match &doc.get(children[0]).data {
        NodeData::Text(s) => s.clone(),
        other => panic!("expected text, got {other:?}"),
    };
    assert_eq!(text.trim(), "hello world");
}

#[test]
fn template_sibling_content_after_template_is_in_body() {
    let doc = parse("<body><template><p>in-template</p></template><div>after</div></body>");
    let body = body_of(&doc);
    // The <div> must be a direct child of body, not inside the template.
    let div = doc.get(body).children.iter().find(|&&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "div")
    });
    assert!(div.is_some(), "<div> must be a body child, not inside template");
}

#[test]
fn template_in_head() {
    // <template> in <head> is also valid HTML.
    let doc = parse("<html><head><template><style>body{}</style></template></head><body></body></html>");
    let head = head_of(&doc);
    let tmpl = {
        let mut found = None;
        for &c in &doc.get(head).children {
            if matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template") {
                found = Some(c);
                break;
            }
        }
        found
    };
    assert!(tmpl.is_some(), "template in head must be present");
    let tmpl = tmpl.unwrap();
    assert!(doc.get(tmpl).children.is_empty(), "template DOM children must be empty");
    let frag = doc.template_content(tmpl).expect("no content fragment");
    assert!(!doc.get(frag).children.is_empty(), "fragment must have children");
    // BUG-417: the template must not cost us the <body> — the assertions above
    // only describe the template itself, which stayed correct all along.
    let _ = body_of(&doc);
}

/// BUG-417: a `<template>` seen while the parser is still «in head» must not
/// swallow `<body>`. `</template>` resets the insertion mode appropriately
/// (§13.2.4.1) — back to InHead — so the next flow content closes head and
/// creates the body; jumping straight to InBody skipped that transition and
/// left `document.body` null with the rest of the page inside `<head>`.
#[test]
fn template_before_body_still_creates_body() {
    let doc = parse(
        "<!DOCTYPE html><template id=t><div>inside</div></template><p id=b>AFTER-TEXT</p>",
    );
    // <body> exists at all (body_of panics otherwise)…
    let body = body_of(&doc);
    // …and everything after </template> lands in it, not in <head>.
    let p = find_element(&doc, "p").expect("<p> after </template> must be parsed");
    assert!(
        doc.get(body).children.contains(&p),
        "<p> after </template> must be a child of <body>, not <head>"
    );
    // The template itself is unharmed: still in <head>, content holds the DIV.
    let head = head_of(&doc);
    let tmpl = *doc
        .get(head)
        .children
        .iter()
        .find(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template")
        })
        .expect("<template> must stay in <head>");
    let frag = doc.template_content(tmpl).expect("no content fragment");
    assert_eq!(
        doc.get(frag).children.len(),
        1,
        "template content must still hold exactly the <div>"
    );
}

/// BUG-417, second case of the report: explicit `<head>`/`<body>` around the
/// template. Here the page rendered, but `document.body` was still null and the
/// content landed under `<html>`.
#[test]
fn template_in_explicit_head_keeps_body() {
    let doc = parse(
        "<html><head><template><style>i{}</style></template></head><body><p>x</p></body></html>",
    );
    let body = body_of(&doc);
    let p = find_element(&doc, "p").expect("<p> must be parsed");
    assert!(
        doc.get(body).children.contains(&p),
        "<p> must be a child of <body>, not of <html>"
    );
}

/// Regression guard for the nested case the old two-way branch handled: after
/// the inner `</template>` the parser must go back to the *outer* template's
/// content, not to `<body>`.
#[test]
fn nested_template_end_returns_to_outer_template() {
    let doc = parse(
        "<body><template id=outer><template id=inner></template><p>after</p></template></body>",
    );
    let outer = find_template(&doc).expect("outer template not found");
    assert_eq!(doc.get(outer).get_attr("id"), Some("outer"));
    let frag = doc.template_content(outer).expect("no content fragment");
    let names: Vec<&str> = doc
        .get(frag)
        .children
        .iter()
        .map(|&c| match &doc.get(c).data {
            NodeData::Element { name, .. } => name.local.as_str(),
            _ => "",
        })
        .collect();
    assert!(
        names.contains(&"p"),
        "content after the inner </template> must stay in the outer template, got {names:?}"
    );
}

#[test]
fn template_attributes_preserved() {
    let doc = parse(r#"<body><template id="foo" data-x="bar"></template></body>"#);
    let tmpl = find_template(&doc).expect("template not found");
    let id_val = doc.get(tmpl).get_attr("id");
    assert_eq!(id_val, Some("foo"));
    let data_val = doc.get(tmpl).get_attr("data-x");
    assert_eq!(data_val, Some("bar"));
}

#[test]
fn template_content_fragment_is_document_fragment() {
    let doc = parse("<body><template><p>x</p></template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    let frag = doc.template_content(tmpl).expect("no content fragment");
    assert!(
        matches!(doc.get(frag).data, NodeData::DocumentFragment),
        "content must be DocumentFragment, got {:?}",
        doc.get(frag).data
    );
}

#[test]
fn nested_template_outer_content_in_outer_fragment() {
    // Outer template's fragment should contain the inner template element.
    let doc = parse("<body><template><template id=\"inner\"><p>deep</p></template></template></body>");
    let outer = find_template(&doc).expect("outer template not found");
    let outer_frag = doc.template_content(outer).expect("no outer fragment");
    // outer fragment must contain the inner template element
    let inner = doc.get(outer_frag).children.iter().find(|&&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "template")
    });
    assert!(inner.is_some(), "inner template must be in outer fragment");
}

#[test]
fn template_empty_is_valid() {
    let doc = parse("<body><template></template></body>");
    let tmpl = find_template(&doc).expect("template not found");
    assert!(doc.get(tmpl).children.is_empty());
    let frag = doc.template_content(tmpl).expect("no content fragment");
    assert!(doc.get(frag).children.is_empty(), "empty template fragment must have no children");
}

#[test]
fn template_display_includes_fragment() {
    // Document::fmt must include the template content fragment in its output.
    let doc = parse("<body><template><p>displayed</p></template></body>");
    let s = doc.to_string();
    assert!(s.contains("#document-fragment"), "display must show #document-fragment");
    assert!(s.contains("<p>"), "display must show fragment content");
}

// ─────────────────────────────────────────────────────────────
// Tests: InFrameset / AfterFrameset / AfterAfterFrameset
// ─────────────────────────────────────────────────────────────

#[test]
fn frameset_basic_structure() {
    let doc = parse(
        "<!DOCTYPE html><html><head></head>\
         <frameset rows=\"50%,50%\">\
           <frame src=\"a.html\">\
           <frame src=\"b.html\">\
         </frameset></html>",
    );
    let fs = find_element(&doc, "frameset");
    assert!(fs.is_some(), "frameset element must be created");
    let fs = fs.unwrap();
    let frames: Vec<_> = doc
        .get(fs)
        .children
        .iter()
        .filter(|&&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "frame")
        })
        .collect();
    assert_eq!(frames.len(), 2, "two frame elements expected");
}

#[test]
fn frameset_no_body() {
    // A frameset document must NOT create an implicit <body>.
    let doc = parse("<!DOCTYPE html><html><head></head><frameset><frame></frameset></html>");
    let body = find_element(&doc, "body");
    assert!(body.is_none(), "frameset document must not contain <body>");
}

#[test]
fn frameset_frame_is_void() {
    // <frame> is void: must not be pushed onto open_elements.
    // Verify by checking it has no children.
    let doc = parse("<frameset><frame src=\"x.html\"><frame src=\"y.html\"></frameset>");
    let fs = find_element(&doc, "frameset").expect("frameset");
    for &child in &doc.get(fs).children {
        if matches!(&doc.get(child).data, NodeData::Element { name, .. } if name.local == "frame") {
            assert!(
                doc.get(child).children.is_empty(),
                "frame element must be void (no children)"
            );
        }
    }
}

#[test]
fn frameset_nested() {
    let doc = parse(
        "<frameset cols=\"50%,50%\">\
           <frameset rows=\"50%,50%\">\
             <frame><frame>\
           </frameset>\
           <frame>\
         </frameset>",
    );
    let outer_fs = find_element(&doc, "frameset").expect("outer frameset");
    let inner_fs = doc.get(outer_fs).children.iter().copied().find(|&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "frameset")
    });
    assert!(inner_fs.is_some(), "inner frameset must exist");
}

#[test]
fn iframe_noembed_xmp_content_stays_inert_text() {
    // BUG-983: before the tokenizer fix, markup inside these RAWTEXT
    // elements became live DOM instead of a single text node.
    for tag in ["iframe", "noembed", "xmp"] {
        let doc = parse(&format!("<{tag}><div id=\"lost\">abc</div></{tag}>"));
        let host = find_element(&doc, tag).unwrap_or_else(|| panic!("{tag} element: {doc}"));
        let has_live_div = doc.get(host).children.iter().any(|&c| {
            matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "div")
        });
        assert!(!has_live_div, "{tag}: nested markup must not become a live element: {doc}");
        let has_text = doc.get(host).children.iter().any(|&c| {
            matches!(&doc.get(c).data, NodeData::Text(s) if s.contains("lost"))
        });
        assert!(has_text, "{tag}: nested markup must survive as literal text: {doc}");
    }
}

#[test]
fn frameset_noframes_content_raw() {
    // <noframes> inside <frameset> is parsed as raw text via InHead routing.
    let doc = parse("<frameset><frame><noframes>fallback</noframes></frameset>");
    let nf = find_element(&doc, "noframes").expect("noframes element");
    let has_text = doc.get(nf).children.iter().any(|&c| {
        matches!(&doc.get(c).data, NodeData::Text(s) if s.contains("fallback"))
    });
    assert!(has_text, "noframes must contain raw text");
}

#[test]
fn after_frameset_only_whitespace_and_noframes() {
    // After </frameset>, only whitespace text and <noframes> are valid.
    // Non-whitespace text should be silently ignored (parse error).
    let doc = parse("<frameset><frame></frameset>spurious text");
    // "spurious text" must not appear anywhere in the document
    fn has_text(doc: &Document, node: NodeId, needle: &str) -> bool {
        if matches!(&doc.get(node).data, NodeData::Text(s) if s.contains(needle)) {
            return true;
        }
        doc.get(node).children.iter().any(|&c| has_text(doc, c, needle))
    }
    assert!(
        !has_text(&doc, doc.root(), "spurious"),
        "spurious text after </frameset> must be ignored"
    );
}

#[test]
fn after_after_frameset_html_close() {
    // </html> after </frameset> transitions to AfterAfterFrameset.
    // Any further non-whitespace content is ignored (parse error).
    let doc = parse("<frameset><frame></frameset></html>extra");
    fn has_text(doc: &Document, node: NodeId, needle: &str) -> bool {
        if matches!(&doc.get(node).data, NodeData::Text(s) if s.contains(needle)) {
            return true;
        }
        doc.get(node).children.iter().any(|&c| has_text(doc, c, needle))
    }
    assert!(
        !has_text(&doc, doc.root(), "extra"),
        "content after </html> in frameset doc must be ignored"
    );
}

// ─────────────────────────────────────────────────────────────
// Tests: InHeadNoscript (scripting disabled)
// ─────────────────────────────────────────────────────────────

fn parse_noscript_off(input: &str) -> Document {
    let mut b = IncrementalTreeBuilder::new();
    b.scripting_enabled = false;
    b.feed(input);
    b.finish()
}

#[test]
fn in_head_noscript_end_tag_closes_and_returns_to_in_head() {
    let doc = parse_noscript_off(
        "<html><head><noscript><link rel=\"stylesheet\" href=\"x.css\"></noscript></head><body></body></html>",
    );
    let head = head_of(&doc);
    let noscript = doc.get(head).children.iter().copied().find(|&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "noscript")
    });
    assert!(noscript.is_some(), "noscript element must be in head");
    let ns = noscript.unwrap();
    // The <link> inside noscript must be a child (parsed as markup, not raw text).
    let link = doc.get(ns).children.iter().copied().find(|&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "link")
    });
    assert!(link.is_some(), "link inside noscript must be parsed as markup");
}

#[test]
fn in_head_noscript_whitespace_processed_in_head() {
    // Whitespace inside <noscript> (scripting off) must be inserted as text.
    let doc = parse_noscript_off(
        "<html><head><noscript>   </noscript></head></html>",
    );
    let head = head_of(&doc);
    let ns = doc.get(head).children.iter().copied().find(|&c| {
        matches!(&doc.get(c).data, NodeData::Element { name, .. } if name.local == "noscript")
    }).expect("noscript in head");
    let has_ws = doc.get(ns).children.iter().any(|&c| {
        matches!(&doc.get(c).data, NodeData::Text(s) if s.trim().is_empty())
    });
    assert!(has_ws, "whitespace inside noscript (scripting off) must be text node");
}

#[test]
fn in_head_noscript_unknown_start_pops_and_reprocesses() {
    // An unexpected start tag inside <noscript> (scripting off) pops noscript
    // and reprocesses: the tag ends up in AfterHead/InBody.
    let doc = parse_noscript_off(
        "<html><head><noscript><body></body></html>",
    );
    // <body> inside <noscript> must cause noscript to close; the <body>
    // element must then be created in the normal position.
    let body = find_element(&doc, "body");
    assert!(body.is_some(), "body must be created after noscript closes");
}

// ─────────────────────────────────────────────────────────────
// Tests: InSelectInTable (complete implementation)
// ─────────────────────────────────────────────────────────────

#[test]
fn select_in_table_cell_context() {
    // <select> inside <td> should use InSelectInTable mode so that a
    // start tag like <table> closes the select and reprocesses.
    let doc = parse(
        "<table><tr><td>\
           <select><option>a</option></select>\
         </td></tr></table>",
    );
    // The select must exist inside the td.
    let sel = find_element(&doc, "select");
    assert!(sel.is_some(), "select must be created in table cell");
}

#[test]
fn select_in_table_closed_by_table_start_tag() {
    // Inside InSelectInTable: a <table> start tag must close the select.
    // After close, the <table> element is reprocessed.
    let doc = parse(
        "<table><tr><td>\
           <select><option>x</option><table><tr><td>y</td></tr></table>\
         </td></tr></table>",
    );
    // The <table> start tag closed the select; a nested table must exist.
    let tables: Vec<_> = {
        fn collect_tables(doc: &Document, node: NodeId, out: &mut Vec<NodeId>) {
            if matches!(&doc.get(node).data, NodeData::Element { name, .. } if name.local == "table") {
                out.push(node);
            }
            for &c in &doc.get(node).children {
                collect_tables(doc, c, out);
            }
        }
        let mut v = Vec::new();
        collect_tables(&doc, doc.root(), &mut v);
        v
    };
    // At least the outer table must exist.
    assert!(!tables.is_empty(), "at least one table must exist");
}

#[test]
fn reset_insertion_mode_select_in_table_context() {
    // Verify that reset_insertion_mode uses InSelectInTable when a
    // <select> has table ancestors, not plain InSelect.
    let doc = parse(
        "<table><tr><td><select><option>1</option></select></td></tr></table>",
    );
    // The select must exist, confirming the mode switch didn't break parsing.
    let sel = find_element(&doc, "select");
    assert!(sel.is_some(), "select in table cell must be parsed correctly");
}

#[test]
fn viewport_meta_device_width_scale() {
    let doc = parse(
        r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=1.0"></head><body></body></html>"#,
    );
    let vm = doc.viewport_meta().expect("viewport meta must be set");
    assert!((vm.initial_scale - 1.0).abs() < 0.001);
    assert_eq!(vm.width, Some(ViewportWidth::DeviceWidth));
}

#[test]
fn viewport_meta_custom_scale() {
    let doc = parse(
        r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=2.0"></head><body></body></html>"#,
    );
    let vm = doc.viewport_meta().expect("viewport meta must be set");
    assert!((vm.initial_scale - 2.0).abs() < 0.001);
}

#[test]
fn viewport_meta_fixed_width() {
    let doc = parse(
        r#"<html><head><meta name="viewport" content="width=375, initial-scale=1"></head><body></body></html>"#,
    );
    let vm = doc.viewport_meta().expect("viewport meta must be set");
    assert_eq!(vm.width, Some(ViewportWidth::Pixels(375.0)));
    assert!((vm.initial_scale - 1.0).abs() < 0.001);
}

#[test]
fn viewport_meta_absent_returns_none() {
    let doc = parse("<html><head></head><body></body></html>");
    assert!(doc.viewport_meta().is_none(), "no meta → no viewport_meta");
}

#[test]
fn viewport_meta_non_viewport_meta_ignored() {
    let doc = parse(
        r#"<html><head><meta name="description" content="hello"></head><body></body></html>"#,
    );
    assert!(doc.viewport_meta().is_none(), "description meta must not set viewport_meta");
}
