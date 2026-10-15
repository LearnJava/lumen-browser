//! UX-USER-ACTIVATION: `window.open`, `navigator.clipboard.writeText` and the
//! permission prompts need a live user activation once the shell turns the
//! gate on (`lumen_js::popup_policy`).
//!
//! This is its own test binary on purpose: the gate is a process-wide flag, and
//! the unit tests of the crate run in parallel in one process — turning it on
//! there would block the `window.open()` calls of unrelated tests.
#![cfg(feature = "v8-backend")]
#![allow(clippy::panic, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use lumen_core::{JsRuntime, JsValue};
use lumen_dom::{Document, QualName};
use lumen_js::popup_policy::{set_gate_enabled, set_site_resolver};
use lumen_js::v8_runtime::V8JsRuntime;

fn runtime() -> V8JsRuntime {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
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

/// A trusted `mousedown` — what the shell dispatches for a real click.
fn click(rt: &V8JsRuntime) {
    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);").unwrap();
}

fn is_null(rt: &V8JsRuntime, expr: &str) -> bool {
    rt.eval(&format!("({expr}) === null")).unwrap() == JsValue::Bool(true)
}

fn blocked_flags(rt: &V8JsRuntime) -> Vec<bool> {
    rt.take_window_open_requests().into_iter().map(|r| r.blocked).collect()
}

// One test: the gate and the resolver are process-wide, so the scenarios
// cannot run in parallel with each other.
#[test]
fn gate_needs_a_live_activation() {
    // ── gate off (the default): bare scripts keep working ─────────────────
    set_gate_enabled(false);
    let rt = runtime();
    assert!(!is_null(&rt, "window.open('https://a.test/')"));
    assert_eq!(blocked_flags(&rt), vec![false]);

    // ── gate on ───────────────────────────────────────────────────────────
    set_gate_enabled(true);
    set_site_resolver(None);

    // window.open on load: blocked, null, reported as blocked.
    let rt = runtime();
    assert!(is_null(&rt, "window.open('https://a.test/')"), "no gesture → null");
    assert_eq!(blocked_flags(&rt), vec![true]);

    // `_self` is a navigation, not a popup.
    assert!(!is_null(&rt, "window.open('https://a.test/', '_self')"));
    assert_eq!(blocked_flags(&rt), vec![false]);

    // A click opens exactly one popup; the gesture is consumed.
    click(&rt);
    assert!(!is_null(&rt, "window.open('https://a.test/')"), "click → popup");
    assert!(is_null(&rt, "window.open('https://b.test/')"), "second open of the same gesture");
    assert_eq!(blocked_flags(&rt), vec![false, true]);

    // A fresh click re-arms it.
    click(&rt);
    assert!(!is_null(&rt, "window.open('https://c.test/')"));
    assert_eq!(blocked_flags(&rt), vec![false]);

    // A site the user allowed opens popups without a gesture.
    set_site_resolver(Some(Arc::new(|origin, kind| origin == "https://example.com" && kind == "popups")));
    let rt = runtime();
    assert!(!is_null(&rt, "window.open('https://a.test/')"));
    assert!(!is_null(&rt, "window.open('https://b.test/')"));
    assert_eq!(blocked_flags(&rt), vec![false, false]);
    set_site_resolver(None);

    // ── clipboard.writeText ───────────────────────────────────────────────
    let rt = runtime();
    rt.eval("var w = 'pending'; navigator.clipboard.writeText('x').then(function() { w = 'ok'; }, function(e) { w = e.name; });")
        .unwrap();
    rt.eval("0").unwrap();
    assert_eq!(rt.eval("w").unwrap(), JsValue::String("NotAllowedError".into()), "no gesture");
    click(&rt);
    rt.eval("navigator.clipboard.writeText('x').then(function() { w = 'ok'; }, function(e) { w = e.name; });")
        .unwrap();
    rt.eval("0").unwrap();
    assert_eq!(rt.eval("w").unwrap(), JsValue::String("ok".into()), "click → write");
    // The write does not consume the activation.
    assert_eq!(rt.eval("navigator.userActivation.isActive").unwrap(), JsValue::Bool(true));

    // ── permission prompts ────────────────────────────────────────────────
    let rt = runtime();
    rt.eval("Notification.requestPermission();").unwrap();
    assert_eq!(rt.take_permission_requests(), vec!["notifications:nogesture".to_string()]);
    click(&rt);
    rt.eval("Notification.requestPermission();").unwrap();
    assert_eq!(rt.take_permission_requests(), vec!["notifications".to_string()]);

    set_gate_enabled(false);
    let rt = runtime();
    rt.eval("Notification.requestPermission();").unwrap();
    assert_eq!(rt.take_permission_requests(), vec!["notifications".to_string()], "gate off");
}
