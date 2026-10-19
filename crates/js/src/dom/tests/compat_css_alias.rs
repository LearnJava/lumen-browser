//! WHATWG Compat §3 (COMPAT-CSS-ALIAS): `-webkit-*` простые алиасы в `element.style` и `CSS.supports`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn prefixed_idl_attribute_and_property_name_share_the_canonical_slot() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.webkitTransform = 'rotate(5deg)'; \
             [d.style.transform, d.style.getPropertyValue('-webkit-transform'), d.style.WebkitTransform, d.style.cssText].join('|')"
        ),
        "rotate(5deg)|rotate(5deg)|rotate(5deg)|transform: rotate(5deg);"
    );
    assert_eq!(
        eval_str(
            &rt,
            "var e = document.createElement('div'); e.style.setProperty('-webkit-background-clip', 'text'); \
             e.style.backgroundClip + '|' + e.style.cssText"
        ),
        "text|background-clip: text;"
    );
}

#[test]
fn style_attribute_with_prefixed_name_reads_back_canonical() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.setAttribute('style', '-webkit-box-sizing: border-box'); \
             d.style.boxSizing"
        ),
        "border-box"
    );
}

#[test]
fn alias_attributes_are_in_style_and_css_supports_holds() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var s = document.createElement('div').style; \
             ['webkitMaskImage','WebKitBackgroundClip','webkitFlexFlow','webkitTransitionTimingFunction'].map(function(n){return n in s}).join() \
             + '|' + ('webkitBogus' in s) + '|' + CSS.supports('-webkit-mask-image', 'none') + '|' + CSS.supports('-webkit-order: 1')"
        ),
        "true,true,true,true|false|true|true"
    );
}

#[test]
fn webkit_mask_box_image_attributes_are_enumerated_but_not_supported_in_css() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var s = document.createElement('div').style; \
             ['webkitMaskBoxImage','webkitMaskBoxImageOutset','webkitMaskBoxImageRepeat','webkitMaskBoxImageSlice',\
              'webkitMaskBoxImageSource','webkitMaskBoxImageWidth'].map(function(n){return n in s}).join() \
             + '|' + ('webkitMaskBoxImageBogus' in s) + '|' + CSS.supports('-webkit-mask-box-image', 'none')"
        ),
        "true,true,true,true,true,true|false|false"
    );
}
