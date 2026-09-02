//! CSSOM-1 срез 3: `document.styleSheets`, `<style>`/`<link>.sheet`,
//! `CSSStyleSheet.cssRules`, `CSSStyleRule.selectorText`/`style.cssText`,
//! `CSSMediaRule.media.mediaText` — read-only JS bindings over
//! `V8JsRuntime::update_stylesheet_nodes`. See
//! `docs/tasks/p1-cssom-1-stylesheets.md`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_css_parser::StylesheetNodeEntry;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// [`make_doc`] plus a `<style id=s1>` in `<head>` — needed to test
/// `<style>.sheet`, not just `document.styleSheets`.
fn make_doc_with_style() -> (Arc<Mutex<Document>>, u32) {
    let doc_arc = make_doc();
    let style_nid = {
        let mut doc = doc_arc.lock().unwrap();
        let head = super::super::find_element_by_tag(&doc, "head").unwrap();
        let style = doc.create_element(QualName::html("style"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(style).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"),
                value: "s1".into(),
            });
        }
        doc.append_child(head, style);
        style.index() as u32
    };
    (doc_arc, style_nid)
}

fn one_sheet_entry(node: u32, css: &str) -> Vec<StylesheetNodeEntry> {
    vec![StylesheetNodeEntry {
        node,
        sheet: Arc::new(lumen_css_parser::parse(css)),
        disabled: false,
    }]
}

#[test]
fn style_sheets_empty_without_registry() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.styleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn style_sheets_length_after_update() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt.eval("document.styleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn style_rule_selector_text_and_style_css_text() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "p.foo { color: red; font-weight: bold; }",
    ));
    let r = rt.eval("document.styleSheets[0].cssRules[0].selectorText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("p.foo".to_string()));
    let r = rt.eval("document.styleSheets[0].cssRules[0].style.cssText").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("color: red; font-weight: bold;".to_string())
    );
}

#[test]
fn media_rule_media_text_and_nested_rule() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(
        style_nid,
        "@media screen and (min-width: 600px) { div { color: blue; } }",
    ));
    let r = rt.eval("document.styleSheets[0].cssRules.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    let r = rt.eval("document.styleSheets[0].cssRules[0].media.mediaText").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("screen and (min-width: 600px)".to_string())
    );
    let r = rt
        .eval("document.styleSheets[0].cssRules[0].cssRules[0].selectorText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("div".to_string()));
}

#[test]
fn style_element_sheet_getter() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt
        .eval("document.getElementById('s1').sheet.cssRules.length")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn style_element_sheet_null_without_registry_entry() {
    let (doc, _style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval("document.getElementById('s1').sheet").unwrap();
    assert_eq!(r, lumen_core::JsValue::Null);
}

#[test]
fn instanceof_css_style_sheet_and_css_style_rule() {
    let (doc, style_nid) = make_doc_with_style();
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(one_sheet_entry(style_nid, "p { color: red; }"));
    let r = rt.eval("document.styleSheets[0] instanceof CSSStyleSheet").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0] instanceof CSSStyleRule")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt
        .eval("document.styleSheets[0].cssRules[0] instanceof CSSRule")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
