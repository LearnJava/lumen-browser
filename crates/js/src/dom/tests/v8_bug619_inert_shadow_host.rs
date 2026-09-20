//! BUG-619 — `inert` on a shadow host didn't propagate to non-slotted
//! shadow-tree children: `_lumen_is_focusable`'s ancestor walk used a plain
//! light-DOM `_lumen_get_parent`, which stops dead at a `ShadowRoot` (it has
//! no light-DOM parent) instead of crossing to the root's host. Slotted
//! content happened to work already (it keeps its light-DOM parent pointer
//! to the host), masking the gap.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn inert_on_shadow_host_blocks_focus_on_non_slotted_shadow_child() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             host.id = 'shadow-host'; \
             host.setAttribute('inert', ''); \
             document.body.appendChild(host); \
             var sr = host.attachShadow({ mode: 'open' }); \
             var button2 = document.createElement('button'); \
             button2.id = 'button-2'; \
             sr.appendChild(button2); \
             button2.focus(); \
             document.activeElement === button2",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(false),
    );
}

#[test]
fn inert_on_shadow_host_still_blocks_focus_on_slotted_child() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             host.id = 'shadow-host'; \
             var button1 = document.createElement('button'); \
             button1.id = 'button-1'; \
             host.appendChild(button1); \
             host.setAttribute('inert', ''); \
             document.body.appendChild(host); \
             var sr = host.attachShadow({ mode: 'open' }); \
             var slot = document.createElement('slot'); \
             sr.appendChild(slot); \
             button1.focus(); \
             document.activeElement === button1",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(false),
    );
}

#[test]
fn non_inert_shadow_host_allows_focus_on_shadow_child() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        rt.eval(
            "var host = document.createElement('div'); \
             host.id = 'shadow-host'; \
             document.body.appendChild(host); \
             var sr = host.attachShadow({ mode: 'open' }); \
             var button2 = document.createElement('button'); \
             button2.id = 'button-2'; \
             sr.appendChild(button2); \
             button2.focus(); \
             document.activeElement === button2",
        )
        .unwrap(),
        lumen_core::JsValue::Bool(true),
    );
}
