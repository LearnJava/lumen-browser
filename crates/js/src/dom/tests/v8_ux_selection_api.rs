//! UX-SELECTION-API — `selectionStart`/`selectionEnd`/`setSelectionRange()` read
//! and write the selection held in the document, the slot the shell's caret
//! is mirrored into.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with(doc: &Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(Arc::clone(doc), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn input_nid(rt: &V8JsRuntime) -> NodeId {
    let r = rt
        .eval(
            "(function() {
                var i = document.createElement('input');
                i.id = 'f'; i.value = 'a\u{1F600}bcd';
                document.body.appendChild(i);
                return i.__nid__;
            })()",
        )
        .unwrap();
    let lumen_core::JsValue::Number(n) = r else { panic!("nid: {r:?}") };
    NodeId::from_raw(n as u32)
}

#[test]
fn set_selection_range_lands_in_the_document() {
    let doc = make_doc();
    let rt = rt_with(&doc);
    let nid = input_nid(&rt);
    rt.eval("document.getElementById('f').setSelectionRange(1, 3, 'backward')").unwrap();
    let sel = doc.lock().unwrap().take_script_selection(nid).expect("script selection");
    assert_eq!((sel.start, sel.end, sel.dir), (1, 3, 2));
    assert!(doc.lock().unwrap().take_script_selection(nid).is_none(), "flag is consumed once");
}

#[test]
fn shell_side_caret_is_what_the_getters_report() {
    let doc = make_doc();
    let rt = rt_with(&doc);
    let nid = input_nid(&rt);
    doc.lock().unwrap().set_text_selection(nid, 2, 5, 1, false);
    let r = rt
        .eval(
            "(function() { var i = document.getElementById('f');
               return i.selectionStart + '-' + i.selectionEnd + '-' + i.selectionDirection; })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("2-5-forward".into()));
}

#[test]
fn untouched_field_reports_caret_at_the_end() {
    let doc = make_doc();
    let rt = rt_with(&doc);
    input_nid(&rt);
    let r = rt.eval("document.getElementById('f').selectionStart").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(6.0));
}
