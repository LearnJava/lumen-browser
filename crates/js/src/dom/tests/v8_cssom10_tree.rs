//! CSSOM-10: CSSOM для at-правил и вложенных правил поверх дерева
//! `lumen_css_parser::cssom_tree` — классы правил, идентичность обёрток,
//! `insertRule`/`deleteRule` на группирующих правилах, правка `selectorText`/
//! `style`, конструируемые листы. См. `docs/tasks/CSSOM-10.md`.

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
fn at_rules_are_visible_with_their_interfaces() {
    let rt = runtime_with_sheet(
        "@layer a, b;\n@layer c { p { color: red; } }\n@supports (display: grid) { p { color: blue; } }\n\
         @scope (.a) to (.b) { div { color: green; } }\n@font-face { font-family: x; }\n\
         @keyframes k { from { top: 0px; } to { top: 5px; } }\n@property --x { syntax: '<length>'; inherits: false; initial-value: 0px; }",
    );
    let out = eval_string(
        &rt,
        "(function() {
            var rules = document.styleSheets[0].cssRules, out = [];
            for (var i = 0; i < rules.length; i++) out.push(rules[i].constructor.name);
            var scope = rules[3];
            out.push(scope.start + '|' + scope.end);
            out.push(rules[0].nameList.join('+') + '|' + rules[1].name);
            out.push(rules[2].conditionText);
            out.push(String(rules[5].cssRules.length) + '|' + rules[5].name);
            out.push(rules[6].name + '|' + rules[6].inherits + '|' + rules[6].initialValue);
            out.push(String(rules[3] instanceof CSSGroupingRule) + String(rules[3] instanceof CSSConditionRule));
            return out.join(',');
        })()",
    );
    assert_eq!(
        out,
        "CSSLayerStatementRule,CSSLayerBlockRule,CSSSupportsRule,CSSScopeRule,CSSFontFaceRule,CSSKeyframesRule,\
         CSSPropertyRule,.a|.b,a+b|c,(display: grid),2|k,--x|false|0px,truefalse"
    );
}

#[test]
fn rule_wrappers_keep_identity_and_parent_links() {
    let rt = runtime_with_sheet(".a { color: red; & .b { color: green; } }");
    let out = eval_string(
        &rt,
        "(function() {
            var ss = document.styleSheets[0], r = ss.cssRules[0];
            return [ss.cssRules[0] === r, r.cssRules[0] === r.cssRules[0], r.cssRules[0].parentRule === r,
                    r.parentRule === null, r.parentStyleSheet === ss, CSSStyleRule.__proto__ === CSSGroupingRule].join(',');
        })()",
    );
    assert_eq!(out, "true,true,true,true,true,true");
}

#[test]
fn nested_insert_delete_and_css_text_follow_chrome_format() {
    let rt = runtime_with_sheet(".a { color: red; & .b { color: green; } }");
    let out = eval_string(
        &rt,
        "(function() {
            var r = document.styleSheets[0].cssRules[0];
            r.insertRule('& .c { color: blue; }', 1);
            var t1 = r.cssText;
            r.deleteRule(0);
            var thrown = '';
            try { r.insertRule('& .x {}', 9); } catch (e) { thrown = e.name; }
            return t1 + '|' + r.cssRules.length + '|' + thrown;
        })()",
    );
    assert_eq!(out, ".a {\n  color: red;\n  & .b { color: green; }\n  & .c { color: blue; }\n}|1|IndexSizeError");
}

#[test]
fn insert_rule_accepts_every_at_rule_kind_at_top_level() {
    let rt = runtime_with_sheet("");
    let out = eval_string(
        &rt,
        "(function() {
            var ss = document.styleSheets[0];
            ss.insertRule('@layer second {}', 0);
            ss.insertRule('@media screen { p { color: red; } }', 1);
            ss.insertRule('@container (min-width: 1px) { p { color: red; } }', 2);
            ss.insertRule('@keyframes k { from { top: 0px; } }', 3);
            var names = [];
            for (var i = 0; i < ss.cssRules.length; i++) names.push(ss.cssRules[i].constructor.name);
            var bad = '';
            try { ss.insertRule('@bogus x;', 0); } catch (e) { bad = e.name; }
            return names.join(',') + '|' + bad + '|' + ss.cssRules[1].cssText;
        })()",
    );
    assert_eq!(
        out,
        "CSSLayerBlockRule,CSSMediaRule,CSSContainerRule,CSSKeyframesRule|SyntaxError|@media screen {\n  p { color: red; }\n}"
    );
}

#[test]
fn selector_text_and_style_setters_edit_the_tree() {
    let rt = runtime_with_sheet(".a { color: red; }");
    let out = eval_string(
        &rt,
        "(function() {
            var r = document.styleSheets[0].cssRules[0];
            r.selectorText = '.z';
            r.style.color = 'blue';
            r.style = 'margin: 1px';
            return r.cssText;
        })()",
    );
    assert_eq!(out, ".z { margin: 1px; }");
}

#[test]
fn removed_rule_reports_null_parents_and_keeps_its_text() {
    let rt = runtime_with_sheet("@media screen { p { color: red; } }\n.q { color: red; }");
    let out = eval_string(
        &rt,
        "(function() {
            var ss = document.styleSheets[0], m = ss.cssRules[0], p = m.cssRules[0];
            ss.deleteRule(0);
            return [m.parentStyleSheet, p.parentRule, p.cssText, ss.cssRules.length].join('|');
        })()",
    );
    assert_eq!(out, "||p { color: red; }|1");
}

#[test]
fn constructed_sheet_shares_the_tree_model() {
    let rt = runtime_with_sheet("");
    let out = eval_string(
        &rt,
        "(function() {
            var s = new CSSStyleSheet();
            s.replaceSync('@layer a { p { color: red; } }');
            s.insertRule('@layer b {}', 1);
            var g = s.cssRules[0];
            g.insertRule('div { color: blue; }', 1);
            var imp = '';
            try { s.insertRule('@import url(x.css);', 0); } catch (e) { imp = e.name; }
            return [s.cssRules.length, g.cssRules.length, g.cssRules[1].parentRule === g, s.cssRules[1].name, imp].join(',');
        })()",
    );
    assert_eq!(out, "2,2,true,b,SyntaxError");
}
