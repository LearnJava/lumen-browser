//! BUG-567 — `HTMLTitleElement.prototype.text` was never defined (only
//! `document.title` had a real getter/setter). The legacy `text` IDL
//! attribute (HTML LS §4.2.2) must concatenate only DIRECT Text-node
//! children's data — not comment nodes, not text nested inside a child
//! element — in contrast to `textContent`, which walks the whole subtree.
//! On set, it must act like the `textContent` setter.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::QualName;

fn runtime_with(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.com/page.html", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn string_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

/// `make_doc()`'s `<title>` has a single plain Text child — the simple case.
#[test]
fn title_text_reads_plain_text_child() {
    let rt = runtime_with(make_doc());
    let title_text = string_of(&rt, "document.getElementsByTagName('title')[0].text");
    assert_eq!(title_text, "Test Page");
}

/// The exact scenario `title.text-01.html` exercises: a comment, a direct
/// Text node, and an element with its own nested Text child. `.text` must
/// see only the middle one; `.textContent` walks everything.
#[test]
fn title_text_ignores_comments_and_nested_element_text() {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let head = doc.create_element(QualName::html("head"));
    let title = doc.create_element(QualName::html("title"));
    let comment = doc.create_comment("COMMENT");
    let text = doc.create_text("TEXT");
    let anchor = doc.create_element(QualName::html("a"));
    let anchor_text = doc.create_text("ELEMENT");
    doc.append_child(doc.root(), html);
    doc.append_child(html, head);
    doc.append_child(head, title);
    doc.append_child(title, comment);
    doc.append_child(title, text);
    doc.append_child(title, anchor);
    doc.append_child(anchor, anchor_text);
    let rt = runtime_with(Arc::new(Mutex::new(doc)));
    let title_ref = "document.getElementsByTagName('title')[0]";
    assert_eq!(string_of(&rt, &format!("{title_ref}.text")), "TEXT");
    assert_eq!(string_of(&rt, &format!("{title_ref}.textContent")), "TEXTELEMENT");
}

/// Setting `.text` must act like the `textContent` setter — replace all
/// children with a single Text node carrying the new value verbatim (no
/// whitespace normalization, per `title.text-03.html`).
#[test]
fn title_text_setter_replaces_children_like_text_content() {
    let rt = runtime_with(make_doc());
    let title_ref = "document.getElementsByTagName('title')[0]";
    rt.eval(&format!("{title_ref}.text = '  two  spaces  '")).unwrap();
    assert_eq!(string_of(&rt, &format!("{title_ref}.text")), "  two  spaces  ");
    assert_eq!(string_of(&rt, &format!("{title_ref}.textContent")), "  two  spaces  ");
    assert_eq!(string_of(&rt, &format!("{title_ref}.firstChild.nodeValue")), "  two  spaces  ");
    assert_eq!(string_of(&rt, &format!("{title_ref}.childNodes.length + ''")), "1");
}
