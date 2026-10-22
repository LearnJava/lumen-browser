//! BUG-1458 — regression coverage: the shell's hovered node, echoed through
//! `V8JsRuntime::set_hovered_nid`, must make `:hover` observable to the page
//! both through `getComputedStyle()` (the same-tick style flush,
//! `crate::v8_runtime::style_flush`) and through `matches()` (the selector
//! natives of `install_node_lookup`). Pre-fix the engine thread never set a
//! hover target at all: a `mouseover` handler read the unhovered style and
//! `matches(':hover')` was `false` while the painted frame showed the hover.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of `v8_bug560_sync_focus`'s helper with a `:hover` rule on `#main`.
fn v8_runtime_with_hover_rule(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "#main { color: rgb(0, 0, 0); } #main:hover { color: rgb(0, 128, 0); }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

fn main_nid(rt: &V8JsRuntime) -> u32 {
    match rt.eval("_lumen_get_element_by_id('main')").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid value: {other:?}"),
    }
}

fn color(rt: &V8JsRuntime) -> lumen_core::JsValue {
    rt.eval("getComputedStyle(document.getElementById('main')).color").unwrap()
}

fn green() -> lumen_core::JsValue {
    lumen_core::JsValue::String("rgb(0, 128, 0)".to_string())
}

fn black() -> lumen_core::JsValue {
    lumen_core::JsValue::String("rgb(0, 0, 0)".to_string())
}

/// The first read bakes the unhovered snapshot (full path); the hover echo must
/// then invalidate it without any DOM mutation, and the unhover must restore it.
#[test]
fn get_computed_style_follows_the_hover_echo() {
    let rt = v8_runtime_with_hover_rule(make_doc());
    let nid = main_nid(&rt);
    assert_eq!(color(&rt), black());
    rt.set_hovered_nid(Some(nid));
    assert_eq!(color(&rt), green());
    rt.set_hovered_nid(None);
    assert_eq!(color(&rt), black());
}

/// Hover set before the very first read — the full (basis-less) path.
#[test]
fn get_computed_style_sees_hover_set_before_first_flush() {
    let rt = v8_runtime_with_hover_rule(make_doc());
    let nid = main_nid(&rt);
    rt.set_hovered_nid(Some(nid));
    assert_eq!(color(&rt), green());
}

/// `matches(':hover')` resolves through a different native than the flush.
#[test]
fn matches_sees_the_hover_echo() {
    let rt = v8_runtime_with_hover_rule(make_doc());
    let nid = main_nid(&rt);
    let probe = "document.getElementById('main').matches(':hover')";
    assert_eq!(rt.eval(probe).unwrap(), lumen_core::JsValue::Bool(false));
    rt.set_hovered_nid(Some(nid));
    assert_eq!(rt.eval(probe).unwrap(), lumen_core::JsValue::Bool(true));
    rt.set_hovered_nid(None);
    assert_eq!(rt.eval(probe).unwrap(), lumen_core::JsValue::Bool(false));
}
