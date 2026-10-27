//! BUG-1297: CSSOM свойств CSS Backgrounds в `element.style` — списки `background-*`,
//! `box-shadow`, `border-radius` проверяются по грамматике и хранятся в канонической записи
//! (`_LUMEN_BACKGROUND_PROPERTIES`, `_lumen_css_canonical_background`).

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

/// `style[prop] = val` на свежем элементе; результат чтения обратно.
fn assign(rt: &V8JsRuntime, prop: &str, val: &str) -> String {
    eval_str(
        rt,
        &format!("var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"),
    )
}

#[test]
fn invalid_background_values_are_rejected() {
    let rt = rt();
    for (prop, val) in [
        ("backgroundSize", "-1px"),
        ("backgroundSize", "1px 2px 3px"),
        ("backgroundRepeat", "repeat repeat-x"),
        ("backgroundAttachment", "auto"),
        ("backgroundImage", "red"),
        ("backgroundPosition", "left right"),
        ("backgroundPosition", "1% center 2px"),
        ("backgroundPositionX", "top"),
        ("backgroundClip", "fill-box"),
        ("backgroundOrigin", "margin-box"),
        ("boxShadow", "auto"),
        ("boxShadow", "1px"),
        ("boxShadow", "red 1px 2px blue"),
        ("boxShadow", "1px 1px -1px"),
        ("borderRadius", "auto"),
        ("borderRadius", "1px / 2px / 3px"),
        ("borderTopLeftRadius", "10px 20px 30px"),
    ] {
        assert_eq!(assign(&rt, prop, val), "", "{prop} = {val:?} должно отклоняться");
    }
}

#[test]
fn valid_background_values_are_canonical() {
    let rt = rt();
    for (prop, val, want) in [
        ("backgroundSize", "1px", "1px auto"),
        ("backgroundSize", "auto auto", "auto"),
        ("backgroundRepeat", "repeat repeat", "repeat"),
        ("backgroundClip", "text border-area", "border-area text"),
        ("backgroundAttachment", "scroll, fixed, local", "scroll, fixed, local"),
        ("backgroundPosition", "center left", "left center"),
        ("backgroundPosition", "top 17px right 18px", "right 18px top 17px"),
        ("backgroundPositionX", "right 10px", "right 10px"),
        ("boxShadow", "inset 1px 2px red", "red 1px 2px inset"),
        ("boxShadow", "-4px 4px 0 0 green", "green -4px 4px 0px 0px"),
        ("borderRadius", "1px 1px 1px 2% / 1px 2% 1px 2%", "1px 1px 1px 2% / 1px 2%"),
        ("borderRadius", "1px 1px 1px 1px / 1px 1px 2% 1px", "1px / 1px 1px 2%"),
    ] {
        assert_eq!(assign(&rt, prop, val), want, "{prop} = {val:?}");
    }
}

#[test]
fn rejected_assignment_keeps_the_previous_value() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.backgroundRepeat = 'no-repeat'; \
             d.style.backgroundRepeat = 'repeat space round'; d.style.backgroundRepeat"
        ),
        "no-repeat"
    );
}

#[test]
fn border_style_accepts_hidden_and_empty_assignment_clears_the_shorthand() {
    let rt = rt();
    assert_eq!(assign(&rt, "borderStyle", "hidden dotted dashed"), "hidden dotted dashed");
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.borderStyle = 'inset outset'; \
             d.style.borderStyle = ''; d.style.cssText"
        ),
        ""
    );
}

#[test]
fn border_image_cssom_and_supports() {
    let rt = rt();
    assert_eq!(assign(&rt, "borderImage", "fill 1 2% 3 4% / auto"), "1 2% 3 4% fill / auto");
    assert_eq!(assign(&rt, "borderImage", "none 100% / 1 / 0 stretch"), "none");
    assert_eq!(assign(&rt, "borderImageSlice", "-1"), "");
    assert_eq!(assign(&rt, "borderImageRepeat", "round stretch space"), "");
    assert_eq!(eval_str(&rt, "String(CSS.supports('border-image-slice', '-1'))"), "false");
    assert_eq!(eval_str(&rt, "String(CSS.supports('border-image-source', 'url(x)'))"), "true");
    assert_eq!(eval_str(&rt, "String(CSS.supports('border-image', 'inherit'))"), "true");
}
