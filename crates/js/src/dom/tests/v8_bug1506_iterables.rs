//! BUG-1506: `CSSRuleList`, `StyleSheetList` и `CSSStyleDeclaration` итерируются
//! (`[...sheet.cssRules]`, `for…of`), у объекта стиля есть `getPropertyPriority`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_css_parser::StylesheetNodeEntry;

fn runtime_with_sheet(css: &str) -> V8JsRuntime {
    let doc_arc = make_doc();
    let style_nid = {
        let mut doc = doc_arc.lock().unwrap();
        let head = super::super::find_element_by_tag(&doc, "head").unwrap();
        let style = doc.create_element(QualName::html("style"));
        doc.append_child(head, style);
        style.index() as u32
    };
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc_arc, "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt.update_stylesheet_nodes(vec![StylesheetNodeEntry {
        node: style_nid,
        sheet: Arc::new(lumen_css_parser::parse(css)),
        disabled: false,
    }]);
    rt
}

fn eval_string(rt: &V8JsRuntime, js: &str) -> String {
    match rt.eval(js).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("ожидалась строка, получено {other:?}"),
    }
}

#[test]
fn css_collections_and_style_declarations_are_iterable() {
    let rt = runtime_with_sheet("p { color: red !important; top: 1px; }\ndiv { margin: 0px; }");
    let out = eval_string(
        &rt,
        "(function() {
            var out = [];
            var sheet = document.styleSheets[0];
            function t(name, f) { try { out.push(name + '=' + f()); } catch (e) { out.push(name + '!' + e.message); } }
            t('rules', function() { return [...sheet.cssRules].length; });
            t('forof', function() { var n = 0; for (var r of sheet.cssRules) n++; return n; });
            t('sheets', function() { return [...document.styleSheets].length; });
            t('ruleStyle', function() { return [...sheet.cssRules[0].style].join('|'); });
            t('ruleStyleIter', function() { return typeof sheet.cssRules[0].style[Symbol.iterator]; });
            t('rulePrio', function() { return sheet.cssRules[0].style.getPropertyPriority('color') + '/' + sheet.cssRules[0].style.getPropertyPriority('top') + '/' + sheet.cssRules[0].style.getPropertyPriority('nope'); });
            var el = document.createElement('div');
            el.setAttribute('style', 'width: 5px; height: 6px !important');
            t('elStyle', function() { return [...el.style].join('|'); });
            t('elLen', function() { return typeof el.style.length + ':' + el.style.length; });
            t('elItem', function() { return typeof el.style.item + ':' + el.style.item(1); });
            t('elPrio', function() { return el.style.getPropertyPriority('height') + '/' + el.style.getPropertyPriority('width'); });
            t('keys', function() { return Object.keys(el.style).length; });
            return out.join('\\n');
        })()",
    );
    assert_eq!(
        out,
        "rules=2\nforof=2\nsheets=1\nruleStyle=color|top\nruleStyleIter=function\nrulePrio=important//\n\
elStyle=width|height\nelLen=number:2\nelItem=function:height\nelPrio=important/\nkeys=0"
    );
}
