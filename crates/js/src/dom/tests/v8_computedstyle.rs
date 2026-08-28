//! V8 port of the fourteenth S12b-24 sub-area — `window.getComputedStyle()`
//! (`getPropertyValue`/camelCase access/pseudo-element arg/null element),
//! backed by `V8JsRuntime::update_computed_styles`. Moved here verbatim from
//! the QuickJS monolith above; only the fixture runtime and the nid lookup's
//! runtime type changed.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn make_computed_styles_map(
    nid: u32,
    props: &[(&str, &str)],
) -> std::collections::HashMap<u32, std::collections::HashMap<String, String>> {
    let mut inner = std::collections::HashMap::new();
    for (k, v) in props {
        inner.insert(k.to_string(), v.to_string());
    }
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, inner);
    outer
}

fn get_main_nid(rt: &V8JsRuntime) -> u32 {
    match rt.eval("document.getElementById('main').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid: {other:?}"),
    }
}

#[test]
fn get_computed_style_returns_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.getComputedStyle === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_computed_style_is_callable_with_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("typeof window.getComputedStyle(document.getElementById('main')) === 'object'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn get_computed_style_returns_empty_without_data() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}

#[test]
fn get_computed_style_returns_value_after_update() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("color", "rgb(255, 0, 0)")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rgb(255, 0, 0)".to_string()));
}

#[test]
fn get_computed_style_get_property_value_unknown_prop_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("color", "blue")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('font-weight')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}

#[test]
fn get_computed_style_multiple_properties() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[
        ("color", "rgb(0, 128, 0)"),
        ("font-size", "16px"),
        ("display", "block"),
    ]);
    rt.update_computed_styles(styles);
    let color = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
        .unwrap();
    let font_size = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('font-size')")
        .unwrap();
    let display = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('display')")
        .unwrap();
    assert_eq!(color, lumen_core::JsValue::String("rgb(0, 128, 0)".to_string()));
    assert_eq!(font_size, lumen_core::JsValue::String("16px".to_string()));
    assert_eq!(display, lumen_core::JsValue::String("block".to_string()));
}

#[test]
fn get_computed_style_camel_case_access() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("font-size", "14px")]);
    rt.update_computed_styles(styles);
    // camelCase property access: fontSize → font-size
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).fontSize")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("14px".to_string()));
}

#[test]
fn get_computed_style_with_pseudo_element_ignored() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("color", "red")]);
    rt.update_computed_styles(styles);
    // Pseudo-element arg is accepted but ignored (not yet supported)
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main'), '::before').getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("red".to_string()));
}

#[test]
fn get_computed_style_update_replaces_previous() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles1 = make_computed_styles_map(nid, &[("color", "red")]);
    rt.update_computed_styles(styles1);
    let styles2 = make_computed_styles_map(nid, &[("color", "blue")]);
    rt.update_computed_styles(styles2);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("blue".to_string()));
}

#[test]
fn get_computed_style_null_element_returns_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    // getElementById returns null for unknown ID; pass null explicitly
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('nonexistent')).getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}

#[test]
fn get_computed_style_background_color() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("background-color", "rgba(0, 0, 255, 0.5)")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('background-color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rgba(0, 0, 255, 0.5)".to_string()));
}

#[test]
fn get_computed_style_display_none() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("display", "none")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('display')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("none".to_string()));
}

#[test]
fn get_computed_style_opacity() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("opacity", "0.75")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('opacity')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("0.75".to_string()));
}

#[test]
fn get_computed_style_margin_shorthand_not_present() {
    // Longhand properties are stored; shorthand "margin" is not
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("margin-top", "8px"), ("margin-bottom", "8px")]);
    rt.update_computed_styles(styles);
    let margin_top = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('margin-top')")
        .unwrap();
    assert_eq!(margin_top, lumen_core::JsValue::String("8px".to_string()));
}

#[test]
fn get_computed_style_span_element() {
    let rt = v8_runtime_with_dom(make_doc());
    let span_nid = match rt
        .eval("document.querySelector('.highlight').__nid__")
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid: {other:?}"),
    };
    let styles = make_computed_styles_map(span_nid, &[("color", "rgb(128, 0, 128)")]);
    rt.update_computed_styles(styles);
    let r = rt
        .eval("window.getComputedStyle(document.querySelector('.highlight')).getPropertyValue('color')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rgb(128, 0, 128)".to_string()));
}

#[test]
fn get_computed_style_position_absolute() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let styles = make_computed_styles_map(nid, &[("position", "absolute"), ("top", "10px"), ("left", "20px")]);
    rt.update_computed_styles(styles);
    let pos = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('position')")
        .unwrap();
    let top = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('top')")
        .unwrap();
    assert_eq!(pos, lumen_core::JsValue::String("absolute".to_string()));
    assert_eq!(top, lumen_core::JsValue::String("10px".to_string()));
}

/// BUG-732: `getPropertyValue('--x')` reads the custom-property snapshot
/// (`update_custom_properties`), not the standard-property one — the two
/// are published side by side because custom properties inherit and are
/// shared, see `lumen_layout::collect_custom_properties`.
#[test]
fn get_computed_style_custom_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = get_main_nid(&rt);
    let mut inner = std::collections::HashMap::new();
    inner.insert("--gap".to_string(), "8px".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, Arc::new(inner));
    rt.update_custom_properties(outer);
    let via_get = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('--gap')")
        .unwrap();
    assert_eq!(via_get, lumen_core::JsValue::String("8px".to_string()));
    // Bracket access spells the property the same way and must agree.
    let via_index = rt
        .eval("window.getComputedStyle(document.getElementById('main'))['--gap']")
        .unwrap();
    assert_eq!(via_index, lumen_core::JsValue::String("8px".to_string()));
}

/// A variable the element neither declares nor inherits is the empty
/// string, never `undefined` — callers branch on `=== ''`.
#[test]
fn get_computed_style_unknown_custom_property_is_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("window.getComputedStyle(document.getElementById('main')).getPropertyValue('--nope')")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}
