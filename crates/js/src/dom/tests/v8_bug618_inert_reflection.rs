//! BUG-618 — `HTMLElement.prototype.inert` was shadowed by a Phase-0 stub
//! (`inert.rs`, installed after the page shim) whose getter read a private
//! per-instance `_inert` flag instead of the `inert` content attribute. The
//! generic reflection table (HTML LS §2.6.1, BUG-383) already declares
//! `inert` as a plain `bool` reflection; these tests pin that the attribute
//! and the IDL property are one state in both directions.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc, "", None, None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Markup-level `inert` (the attribute is present before any script runs)
/// must read back as `true` — the exact symptom of the bug report.
#[test]
fn inert_attribute_in_markup_reads_true() {
    let doc = make_doc();
    {
        let mut d = doc.lock().unwrap();
        let main = d.find_by_id("main").expect("make_doc has #main");
        if let NodeData::Element { attrs, .. } = &mut d.get_mut(main).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("inert"),
                value: "".into(),
            });
        }
    }
    let rt = v8_runtime_with_dom(doc);
    assert!(bool_eval(
        &rt,
        "var el = document.getElementById('main'); \
         el.hasAttribute('inert') && el.inert === true"
    ));
}

#[test]
fn set_attribute_is_visible_through_the_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var el = document.getElementById('main'); \
         var before = el.inert; \
         el.setAttribute('inert', ''); \
         var on = el.inert; \
         el.removeAttribute('inert'); \
         before === false && on === true && el.inert === false"
    ));
}

#[test]
fn property_write_reflects_into_the_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var el = document.getElementById('main'); \
         el.inert = 1; \
         var on = el.hasAttribute('inert') && el.getAttribute('inert') === ''; \
         el.inert = 0; \
         on && !el.hasAttribute('inert') && el.inert === false"
    ));
}

/// The descriptor lives on `HTMLElement.prototype` and nothing stores state
/// on the instance (the old stub left an `_inert` expando behind).
#[test]
fn no_per_instance_shadow_state() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var el = document.getElementById('main'); \
         el.inert = true; \
         var d = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'inert'); \
         typeof d.get === 'function' && typeof d.set === 'function' \
           && !Object.prototype.hasOwnProperty.call(el, '_inert') \
           && !Object.prototype.hasOwnProperty.call(el, 'inert')"
    ));
}
