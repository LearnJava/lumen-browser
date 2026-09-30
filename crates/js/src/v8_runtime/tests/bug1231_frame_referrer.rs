//! BUG-1231: `document.referrer` фасада документа `<iframe>` отдаёт реферер,
//! выставленный shell'ом на документ ребёнка (тот же, что ушёл в `Referer`).

use super::*;

fn referrer_of_frame(stamp: Option<&str>, accessible: bool) -> JsValue {
    let parent_doc = make_doc();
    let child_doc = make_doc();
    child_doc.lock().unwrap().set_document_referrer(stamp.map(str::to_owned));
    let parent = runtime_with_dom(parent_doc, "https://parent.example/index.html");
    parent.register_frame_document(
        1,
        child_doc,
        "https://child.example/".to_owned(),
        None,
        accessible,
        false,
        None,
    );
    parent.eval("_lumen_frame_content_window(1).document.referrer").unwrap()
}

#[test]
fn frame_document_referrer_reflects_stamped_value() {
    assert_eq!(
        referrer_of_frame(Some("https://parent.example/"), true),
        JsValue::String("https://parent.example/".into())
    );
}

#[test]
fn frame_document_referrer_is_empty_without_stamp() {
    assert_eq!(referrer_of_frame(None, true), JsValue::String(String::new()));
}
