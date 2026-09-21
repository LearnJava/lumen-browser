//! GAP-USERACT (BUG-751) — HTML LS §6.4 transient activation.
//!
//! `navigator.userActivation` used to be a frozen `{isActive: true,
//! hasBeenActive: true}` literal, so every "must be handling a user gesture"
//! gate (File System Access, Window Management, Local Font Access, Screen
//! Capture) was permanently open. These tests drive the real trusted-input
//! dispatch path (`_lumen_dispatch_mouse_event`/`_lumen_dispatch_key_event`,
//! the same natives the shell calls on real OS input — see
//! `crates/shell/src/input/mod.rs`) and check `navigator.userActivation`
//! reacts, decays only via an explicit consume, and ignores a page's own
//! `dispatchEvent()`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_active(rt: &V8JsRuntime) -> bool {
    rt.eval("navigator.userActivation.isActive").unwrap() == lumen_core::JsValue::Bool(true)
}

fn has_been_active(rt: &V8JsRuntime) -> bool {
    rt.eval("navigator.userActivation.hasBeenActive").unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn no_activation_before_any_input() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!is_active(&rt));
    assert!(!has_been_active(&rt));
}

#[test]
fn trusted_mousedown_marks_activation() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);")
        .unwrap();
    assert!(is_active(&rt));
    assert!(has_been_active(&rt));
}

#[test]
fn trusted_keydown_marks_activation_but_bare_modifier_does_not() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "_lumen_dispatch_key_event(_lumen_root_nid, 'keydown', 'Shift', 'ShiftLeft', 16, 0, 0, false, false);",
    )
    .unwrap();
    assert!(!is_active(&rt), "a bare modifier key must not grant activation");

    rt.eval(
        "_lumen_dispatch_key_event(_lumen_root_nid, 'keydown', 'a', 'KeyA', 65, 0, 0, false, false);",
    )
    .unwrap();
    assert!(is_active(&rt));
}

#[test]
fn consume_clears_is_active_but_not_has_been_active() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);")
        .unwrap();
    assert!(is_active(&rt));

    rt.eval("_lumen_consume_user_activation();").unwrap();
    assert!(!is_active(&rt), "consume must clear isActive immediately");
    assert!(has_been_active(&rt), "hasBeenActive is sticky, consume must not clear it");

    // A fresh activation-triggering event re-arms isActive.
    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);")
        .unwrap();
    assert!(is_active(&rt));
}

#[test]
fn page_authored_dispatch_event_does_not_grant_activation() {
    // A script calling dispatchEvent() itself must not be able to forge
    // activation — only the shell-driven trusted-input natives may.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "document.body.dispatchEvent(new MouseEvent('mousedown', {bubbles: true}));",
    )
    .unwrap();
    assert!(!is_active(&rt));
    assert!(!has_been_active(&rt));
}

#[test]
fn requiring_user_activation_gate_reflects_real_state() {
    // Exercises the filesystem_access.rs::requireUserActivation shape end to
    // end through the real navigator.userActivation getter, without pulling
    // in the whole File System Access module.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "function requireUserActivation() { \
             if (navigator.userActivation.isActive === false) throw new Error('gate'); \
         }",
    )
    .unwrap();

    let rejected = rt.eval("(function(){ try { requireUserActivation(); return false; } catch (e) { return true; } })()").unwrap();
    assert_eq!(rejected, lumen_core::JsValue::Bool(true), "no gesture yet: gate must reject");

    rt.eval("_lumen_dispatch_mouse_event(_lumen_root_nid, 'mousedown', 0, 0, 0, 1, 0);")
        .unwrap();
    let rejected = rt.eval("(function(){ try { requireUserActivation(); return false; } catch (e) { return true; } })()").unwrap();
    assert_eq!(rejected, lumen_core::JsValue::Bool(false), "fresh gesture: gate must pass");
}
