//! BUG-1401: `element.style.{contain,container-type,content-visibility}` — каноническая запись,
//! отказ от недопустимых значений, `!important` не попадает в значение longhand.

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
        &format!(
            "var d = document.createElement('div'); d.style[{prop:?}] = {val:?}; d.style[{prop:?}]"
        ),
    )
}

#[test]
fn contain_valid_values_are_canonical() {
    let rt = rt();
    for (val, want) in [
        ("none", "none"),
        ("strict", "strict"),
        ("content", "content"),
        ("layout size", "size layout"),
        ("paint style", "style paint"),
        ("layout paint style size", "size layout style paint"),
        ("layout inline-size", "inline-size layout"),
        ("LAYOUT", "layout"),
    ] {
        assert_eq!(assign(&rt, "contain", val), want, "contain = {val:?}");
    }
}

#[test]
fn contain_invalid_values_are_rejected() {
    let rt = rt();
    for val in [
        "auto", "layout layout", "strict layout", "paint strict", "none none", "none strict",
        "strict content", "size layout size", "paint content", "size inline-size", "",
    ] {
        assert_eq!(assign(&rt, "contain", val), "", "contain = {val:?} должно отклоняться");
    }
}

#[test]
fn content_visibility_rejects_garbage() {
    let rt = rt();
    for val in ["bogus", "auto hidden", "visible auto", "hidden visible"] {
        assert_eq!(assign(&rt, "contentVisibility", val), "", "content-visibility = {val:?}");
    }
    assert_eq!(assign(&rt, "contentVisibility", "hidden"), "hidden");
}

#[test]
fn container_type_canonical_order_and_validation() {
    let rt = rt();
    assert_eq!(assign(&rt, "containerType", "scroll-state size"), "size scroll-state");
    assert_eq!(assign(&rt, "containerType", "inline-size"), "inline-size");
    assert_eq!(assign(&rt, "containerType", "normal"), "normal");
    for val in ["size inline-size", "normal size", "size size", "bogus"] {
        assert_eq!(assign(&rt, "containerType", val), "", "container-type = {val:?}");
    }
}

#[test]
fn important_suffix_is_not_part_of_the_value() {
    let rt = rt();
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.cssText = 'container-type:size !important'; d.style.containerType"
        ),
        "size"
    );
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.cssText = 'width: 10px ! important'; d.style.width"
        ),
        "10px"
    );
}

/// BUG-1514: шорткат `container` — разбор в `container-name` + `container-type`, каноническая
/// запись (тип `normal` опускается), отказ от недопустимых значений.
#[test]
fn container_shorthand_roundtrip() {
    let rt = rt();
    for (val, want) in [
        ("none / normal", "none"),
        ("foo bar / inline-size", "foo bar / inline-size"),
        ("foo / normal", "foo"),
        ("foo", "foo"),
        ("none / size scroll-state", "none / size scroll-state"),
        ("foo/scroll-state size", "foo / size scroll-state"),
    ] {
        assert_eq!(assign(&rt, "container", val), want, "container = {val:?}");
    }
    for val in ["", "foo /", "/ size", "none foo", "foo / bogus", "foo and bar", "foo / size / size"] {
        assert_eq!(assign(&rt, "container", val), "", "container = {val:?} должно отклоняться");
    }
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.setAttribute('style', \
             'container-type:inline-size;container-name:foo bar'); d.style.container"
        ),
        "foo bar / inline-size"
    );
    assert_eq!(
        eval_str(
            &rt,
            "var d = document.createElement('div'); d.style.container = 'a / size'; \
             d.style.containerName + '|' + d.style.containerType + '|' + d.style.cssText"
        ),
        "a|size|container: a / size;"
    );
}
