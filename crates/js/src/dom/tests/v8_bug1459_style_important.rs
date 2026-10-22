//! BUG-1459: приоритет `!important` инлайнового `element.style` — `cssText`, `el.style = …`,
//! `setProperty(…, 'important')`, `getPropertyPriority`.

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
fn css_text_keeps_important() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.cssText = 'color: green !important; width: 5px';
         d.style.cssText + '|' + d.getAttribute('style') + '|' + d.style.color + '|' + d.style.getPropertyPriority('color')
           + '|' + d.style.getPropertyPriority('width')",
    );
    assert_eq!(got, "color: green !important; width: 5px;|color: green !important; width: 5px;|green|important|");
}

#[test]
fn style_assignment_forwards_important() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style = 'background-color: green !important';
         d.getAttribute('style') + '|' + d.style.getPropertyPriority('background-color')",
    );
    assert_eq!(got, "background-color: green !important;|important");
}

#[test]
fn set_attribute_style_is_readable_with_priority() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div'); d.setAttribute('style', 'color: green !important');
         d.style.cssText + '|' + d.style.getPropertyPriority('color')",
    );
    assert_eq!(got, "color: green !important;|important");
}

#[test]
fn set_property_priority() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div');
         d.style.setProperty('color', 'green', 'important');
         var a = d.style.cssText + '|' + d.style.getPropertyPriority('color');
         d.style.setProperty('color', 'red');
         var b = d.style.cssText + '|' + d.style.getPropertyPriority('color');
         d.style.setProperty('color', 'blue', 'IMPORTANT');
         d.style.setProperty('width', '1px', 'bogus');
         var c = d.style.cssText;
         d.style.removeProperty('color');
         a + '#' + b + '#' + c + '#' + d.style.cssText + '#' + d.style.getPropertyPriority('color')",
    );
    assert_eq!(
        got,
        "color: green !important;|important#color: red;|#color: blue !important;#\
         #"
    );
}

#[test]
fn shorthand_priority_and_mixed_longhands() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div');
         d.style.cssText = 'margin: 1px !important';
         var a = d.style.cssText + '|' + d.style.getPropertyPriority('margin') + '|' + d.style.getPropertyPriority('margin-top');
         d.style.setProperty('margin-top', '2px');
         var b = d.style.cssText;
         d.style.setProperty('margin-top', '1px', 'important');
         a + '#' + b + '#' + d.style.cssText",
    );
    assert_eq!(
        got,
        concat!(
            "margin: 1px !important;|important|important#",
            "margin-top: 2px; margin-right: 1px !important; margin-bottom: 1px !important; margin-left: 1px !important;#",
            "margin: 1px !important;"
        )
    );
}

#[test]
fn later_normal_declaration_does_not_override_important() {
    let rt = rt();
    let got = eval_str(
        &rt,
        "var d = document.createElement('div'); d.style.cssText = 'color: green !important; color: red';
         d.style.cssText",
    );
    assert_eq!(got, "color: green !important;");
}
