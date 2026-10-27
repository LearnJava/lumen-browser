//! BUG-1552: `style.cssFloat` — IDL-имя свойства `float`; сворачивание `border`/`border-*`
//! при сериализации `cssText`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn eval_string(rt: &V8JsRuntime, js: &str) -> String {
    match rt.eval(js).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("ожидалась строка, получено {other:?}"),
    }
}

fn runtime() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt
}

#[test]
fn css_float_and_border_shorthand_round_trip() {
    let rt = runtime();
    let out = eval_string(
        &rt,
        "(function() {
            var out = [];
            function t(name, f) { try { out.push(name + '=' + f()); } catch (e) { out.push(name + '!' + e.message); } }
            function mk() { return document.createElement('div'); }
            function ct(css) { var e = mk(); e.style.cssText = css; return e.style.cssText; }
            t('cssFloatSet', function() { var el = mk(); el.style.cssFloat = 'left'; return el.style.cssText + '|' + el.style.float + '|' + el.style.cssFloat + '|' + el.style.getPropertyValue('float'); });
            t('cssFloatIn', function() { return 'cssFloat' in mk().style; });
            t('floatSet', function() { var e = mk(); e.style.float = 'right'; return e.style.cssText + '|' + e.style.cssFloat; });
            t('b1', function() { return ct('border: 1px; border-top: 1px'); });
            t('b2', function() { return ct('border-width: 1px; border-style: solid; border-color: red'); });
            t('b3', function() { return ct('border-top: 1px solid red; border-right: 1px solid red; border-bottom: 1px solid red; border-left: 1px solid red'); });
            t('b4', function() { return ct('border: 1px solid red; border-color: blue'); });
            t('side', function() { return ct('border-top: 2px dashed green'); });
            t('sideGet', function() { var e = mk(); e.style.borderTop = '2px dashed green'; return e.style.borderTop + '|' + e.style.borderTopWidth + '|' + e.style.border + '|'; });
            t('twoSides', function() { return ct('border-top: 1px solid red; border-left: 3px dotted blue'); });
            t('none', function() { return ct('border: none'); });
            t('zero', function() { return ct('border: 0'); });
            t('unevenWidth', function() { return ct('border: 1px solid red; border-top-width: 5px'); });
            t('importantMix', function() { return ct('border: 1px solid red !important; border-top-width: 5px'); });
            t('important', function() { return ct('border: 1px solid red !important'); });
            t('invalid', function() { var e = mk(); e.style.border = '1px solid red'; e.style.border = 'bogus 1px'; return e.style.cssText; });
            t('dupWidth', function() { var e = mk(); e.style.border = '1px 2px'; return JSON.stringify(e.style.cssText); });
            t('remove', function() { var e = mk(); e.style.border = '1px solid red'; var old = e.style.removeProperty('border'); return old + '|' + JSON.stringify(e.style.cssText) + '|' + e.style.length; });
            t('removeSide', function() { var e = mk(); e.style.border = '1px solid red'; e.style.removeProperty('border-top'); return e.style.cssText; });
            t('clear', function() { var e = mk(); e.style.border = '1px solid red'; e.style.border = ''; return JSON.stringify(e.style.cssText); });
            t('prio', function() { var e = mk(); e.style.setProperty('border', '1px solid red', 'important'); return e.style.getPropertyPriority('border') + '|' + e.style.getPropertyPriority('border-top'); });
            t('keyword', function() { return ct('border: inherit'); });
            t('rgb', function() { return ct('border: 1px solid rgb(1, 2, 3)'); });
            t('w28', function() { return ct('border-top: 1px; border-right: 1px; border-bottom: 1px; border-left: 1px; border-image: none;'); });
            t('w29', function() { return ct('border-top: 1px; border-right: 1px; border-bottom: 1px; border-left: 1px;'); });
            t('w30', function() { return ct('border-top: 1px; border-right: 2px; border-bottom: 3px; border-left: 4px;'); });
            t('w31', function() { return ct('border: 1px; border-top: 2px;'); });
            t('w32', function() { return ct('border: 1px; border-top: 1px !important;'); });
            t('w33', function() { return ct('border: 1px; border-top-color: red;'); });
            t('w34', function() { return ct('border: solid; border-style: dotted'); });
            t('w26', function() { return ct('border: 1px red;') + ct('border: red;'); });
            t('imageGet', function() { var e = mk(); e.style.border = '1px solid red'; return e.style.borderImage + '|' + e.style.borderImageSource; });
            t('imageRaw', function() { return ct('border-image: url(a.png) 30 / 10px round'); });
            t('dup', function() { return ct('color: red; margin: 1px; color: blue'); });
            return out.join('\\n');
        })()",
    );
    assert_eq!(
        out,
        "cssFloatSet=float: left;|left|left|left\n\
cssFloatIn=true\n\
floatSet=float: right;|right\n\
b1=border: 1px;\n\
b2=border-width: 1px; border-style: solid; border-color: red;\n\
b3=border-width: 1px; border-style: solid; border-color: red;\n\
b4=border: 1px solid blue;\n\
side=border-top: 2px dashed green;\n\
sideGet=2px dashed green|2px||\n\
twoSides=border-top: 1px solid red; border-left: 3px dotted blue;\n\
none=border: none;\n\
zero=border: 0px;\n\
unevenWidth=border-style: solid; border-color: red; border-width: 5px 1px 1px; border-image: none;\n\
importantMix=border: 1px solid red !important;\n\
important=border: 1px solid red !important;\n\
invalid=border: 1px solid red;\n\
dupWidth=\"\"\n\
remove=1px solid red|\"\"|0\n\
removeSide=border-right: 1px solid red; border-bottom: 1px solid red; border-left: 1px solid red; border-image: none;\n\
clear=\"\"\n\
prio=important|important\n\
keyword=border: inherit;\n\
rgb=border: 1px solid rgb(1, 2, 3);\n\
w28=border: 1px;\n\
w29=border-width: 1px; border-style: none; border-color: currentcolor;\n\
w30=border-width: 1px 2px 3px 4px; border-style: none; border-color: currentcolor;
w31=border-width: 2px 1px 1px; border-style: none; border-color: currentcolor; border-image: none;\n\
w32=border-right: 1px; border-bottom: 1px; border-left: 1px; border-image: none; border-top: 1px !important;\n\
w33=border-width: 1px; border-style: none; border-color: red currentcolor currentcolor; border-image: none;\n\
w34=border: dotted;\n\
w26=border: 1px red;border: red;\n\
imageGet=none|none\n\
imageRaw=border-image: url(a.png) 30 / 10px round;\n\
dup=margin: 1px; color: blue;"
    );
}

#[test]
fn set_property_shorthand_initial_can_be_set_and_removed() {
    // WPT css/cssom/cssom-setProperty-shorthand.html, restricted to the shorthands BUG-1552 touches.
    let rt = runtime();
    let out = eval_string(
        &rt,
        "(function() {
            var bad = [];
            var el = document.createElement('span');
            ['border-top', 'border-right', 'border-bottom', 'border-left', 'border', 'border-color',
             'border-style', 'border-width', 'border-image', 'margin', 'padding', 'overflow'].forEach(function(p) {
                ['', 'important'].forEach(function(prio) {
                    el.style.setProperty(p, 'initial', prio);
                    if (el.style.getPropertyValue(p) != 'initial') bad.push('set ' + p + ' ' + prio);
                    el.style.removeProperty(p);
                    if (el.style.getPropertyValue(p) == 'initial') bad.push('remove ' + p + ' ' + prio);
                });
            });
            return bad.join(',') + '|' + JSON.stringify(el.style.cssText);
        })()",
    );
    assert_eq!(out, "|\"\"");
}
