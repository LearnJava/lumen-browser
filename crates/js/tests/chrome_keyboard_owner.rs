//! UX-FOCUS-ARBITER: while the address bar owns the keyboard, a page can take
//! it back neither with `element.focus()` (a `setTimeout(focus)` loop) nor with
//! `autofocus` at DOMContentLoaded.
//!
//! This is its own test binary on purpose: `lumen_js::chrome_input` is a
//! process-wide flag, and the unit tests of the crate run in parallel in one
//! process — flipping it there would make their `focus()` calls flaky.
#![cfg(feature = "v8-backend")]
#![allow(clippy::panic, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::{Attribute, Document, NodeData, QualName};
use lumen_js::chrome_input::set_chrome_owns_keyboard;
use lumen_js::v8_runtime::V8JsRuntime;

/// `<html><body><input id=field autofocus></body></html>`.
fn runtime() -> V8JsRuntime {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let field = doc.create_element(QualName::html("input"));
    for (name, value) in [("id", "field"), ("autofocus", "")] {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(field).data {
            attrs.push(Attribute { name: QualName::html(name), value: value.into() });
        }
    }
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, field);
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        Arc::new(Mutex::new(doc)),
        "https://example.com/doc",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        None,
    )
    .unwrap();
    rt
}

#[test]
fn page_cannot_take_the_keyboard_from_the_address_bar() {
    // Chrome owns the keyboard: `focus()` and the autofocus flush do nothing.
    set_chrome_owns_keyboard(true);
    let rt = runtime();
    rt.eval("document.getElementById('field').focus()").unwrap();
    assert!(rt.take_focus_requests().is_empty(), "focus() must be ignored");
    rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
    assert!(rt.take_focus_requests().is_empty(), "autofocus must be ignored");

    // The address bar closed: both work again.
    set_chrome_owns_keyboard(false);
    rt.eval("document.getElementById('field').focus()").unwrap();
    assert_eq!(rt.take_focus_requests().len(), 1, "focus() works once the chrome lets go");
    let rt = runtime();
    rt.eval("_lumen_apply_ready_state('interactive')").unwrap();
    assert_eq!(rt.take_focus_requests().len(), 1, "autofocus works once the chrome lets go");
}
