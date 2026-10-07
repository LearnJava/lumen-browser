//! BUG-1316: у живого `document` есть element-only аксессоры `ParentNode`
//! (`children`/`childElementCount`/`firstElementChild`/`lastElementChild`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

#[test]
fn live_document_parent_node_accessors() {
    let rt = rt();
    let r = rt
        .eval(
            "[typeof document.children, document.children.length, document.childElementCount,\
              document.firstElementChild === document.documentElement,\
              document.lastElementChild === document.documentElement,\
              document.children[0] === document.documentElement,\
              document.children instanceof HTMLCollection].join()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("object,1,1,true,true,true,true".into()));
}

/// Реальные страницы начинаются с `<!DOCTYPE html>`: doctype — узел с `tagName` `html`,
/// но не элемент, и в `children`/`firstElementChild` попадать не должен.
#[test]
fn doctype_is_not_an_element_child() {
    let doc = make_doc();
    {
        let mut d = doc.lock().unwrap();
        let dt = d.create_doctype("html", "", "");
        let html = d.get(d.root()).children[0];
        d.insert_before(dt, html);
    }
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    let r = rt
        .eval(
            "[document.childNodes.length, document.childNodes[0].nodeType,              document.children.length, document.childElementCount,              document.children[0] === document.documentElement,              document.firstElementChild === document.documentElement].join()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("2,10,1,1,true,true".into()));
}
