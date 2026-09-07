//! BUG-518 срез 6 — CSS Mixins CSSOM (`@mixin`/`@apply`/`@contents` reflected
//! through `document.styleSheets[i].cssRules`/`.cssText`). End-to-end check
//! over the real V8 shim (`_lumen_stylesheet_rule_json`/`_lumen_make_css_rule`
//! bridge), transcribing the vendored
//! `css/css-mixins/mixins/mixin-cssom.tentative.html` subtests directly.
//! Bypasses `lumen_driver::InProcessSession` (its test pipeline never calls
//! `update_stylesheet_nodes` — `document.styleSheets` is unreachable through
//! it today, a pre-existing gap in that harness, not this feature) in favor
//! of `V8JsRuntime::update_stylesheet_nodes` directly, which is exactly what
//! the real page pipeline (`crates/shell/src/page_pipeline.rs`) calls.
//!
//! The sixth subtest (`@apply` illegal at top level) needed
//! `CSSStyleSheet.insertRule`/`.deleteRule` on an *owned* sheet, added by
//! BUG-518 срез 7 — see the tests at the bottom of this file.
#![cfg(feature = "v8-backend")]

use lumen_core::{JsRuntime, JsValue};
use lumen_css_parser::StylesheetNodeEntry;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;
use std::sync::{Arc, Mutex};

fn rt_with_sheet(css: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/", None, None, None, None, None, None, None, None, false)
        .unwrap();
    let sheet = lumen_css_parser::parse(css);
    rt.update_stylesheet_nodes(vec![StylesheetNodeEntry {
        node: 0,
        sheet: Arc::new(sheet),
        disabled: false,
    }]);
    rt
}

fn eval_string(rt: &V8JsRuntime, expr: &str) -> String {
    match rt.eval(expr).unwrap() {
        JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn mixin_rule_css_text_serializes_result_with_nested_bare_amp_rule() {
    // "serialization of @mixin"
    let rt = rt_with_sheet(
        "@mixin --m1() { @result { color: green; & { --foo: bar; } } }",
    );
    assert_eq!(rt.eval("document.styleSheets[0].cssRules.length").unwrap(), JsValue::Number(1.0));
    assert_eq!(
        eval_string(&rt, "document.styleSheets[0].cssRules[0].cssText"),
        "@mixin --m1() {\n  @result {\n  color: green;\n  & { --foo: bar; }\n}\n}",
    );
}

#[test]
fn rule_css_text_serializes_bare_apply() {
    // "serialization of rule with @apply"
    let rt = rt_with_sheet("#foo { @apply --m1; }");
    assert_eq!(
        eval_string(&rt, "document.styleSheets[0].cssRules[0].cssText"),
        "#foo {\n  @apply --m1;\n}",
    );
}

#[test]
fn mixin_rule_css_text_serializes_result_with_contents_placeholder() {
    // "serialization of @mixin with @contents"
    let rt = rt_with_sheet("@mixin --m2() { @result { @contents } }");
    assert_eq!(
        eval_string(&rt, "document.styleSheets[0].cssRules[0].cssText"),
        "@mixin --m2() {\n  @result {\n  @contents;\n}\n}",
    );
}

#[test]
fn rule_css_text_serializes_apply_with_contents_argument() {
    // "serialization of rule with @apply and contents argument"
    let rt = rt_with_sheet("#foo { color: red; @apply --m2 { color: green; } }");
    assert_eq!(
        eval_string(&rt, "document.styleSheets[0].cssRules[0].cssText"),
        "#foo {\n  color: red;\n  @apply --m2 { color: green; }\n}",
    );
}

#[test]
fn mixin_rule_css_text_serializes_parameters_with_type_and_default() {
    // "serialization of @mixin with parameters"
    let rt = rt_with_sheet(
        "@mixin --m3(--arg type(<length>): 1em, --other-arg) { \
         @result { margin-left: var(--arg); } }",
    );
    assert_eq!(
        eval_string(&rt, "document.styleSheets[0].cssRules[0].cssText"),
        "@mixin --m3(--arg <length>: 1em, --other-arg) {\n  @result {\n  margin-left: var(--arg);\n}\n}",
    );
}

#[test]
fn mixin_rule_is_not_a_css_style_rule_and_reflects_its_name() {
    // Sanity check that `_lumen_make_css_rule`'s `kind === 'mixin'` branch
    // is actually reached (not silently falling through to
    // `_lumen_build_css_style_rule`, which would misreport `undefined`
    // `selectorText`/`style` instead of `CSSMixinRule`'s own shape).
    let rt = rt_with_sheet("@mixin --m1() { @result { color: green; } }");
    assert_eq!(
        rt.eval("document.styleSheets[0].cssRules[0] instanceof CSSStyleRule").unwrap(),
        JsValue::Bool(false),
    );
    assert_eq!(eval_string(&rt, "document.styleSheets[0].cssRules[0].name"), "--m1");
}

#[test]
fn cssom_rules_includes_top_level_mixin_but_not_layered_one() {
    // A `@mixin` declared inside `@layer` gets no `cssRules` entry of its
    // own (an `@layer` block itself has none either) — only the two
    // top-level ones do.
    let rt = rt_with_sheet(
        "@mixin --a() {} @layer L { @mixin --b() {} } @mixin --c() {} p {}",
    );
    assert_eq!(rt.eval("document.styleSheets[0].cssRules.length").unwrap(), JsValue::Number(3.0));
    assert_eq!(eval_string(&rt, "document.styleSheets[0].cssRules[0].name"), "--a");
    assert_eq!(eval_string(&rt, "document.styleSheets[0].cssRules[1].name"), "--c");
}

// --- BUG-518 срез 7: `CSSStyleSheet.insertRule`/`.deleteRule` on an owned
// (`document.styleSheets`) sheet — previously only a *constructed* sheet had
// these (CSSOM-5 срез 3). `Stylesheet::insert_rule`/`delete_rule` themselves
// are already covered at the parser level (`css-parser/parser/tests/
// revision.rs`); these tests are the end-to-end check that the owned-sheet
// registry's JS bridge (`_lumen_stylesheet_insert_rule`/
// `_lumen_stylesheet_delete_rule`, `crates/js/src/v8_runtime/install/
// stylesheets.rs`) actually reaches it.

#[test]
fn insert_rule_and_delete_rule_mutate_the_owned_sheet() {
    let rt = rt_with_sheet("#a { color: red; }");
    assert_eq!(rt.eval("document.styleSheets[0].insertRule('#b { color: green; }', 1)").unwrap(), JsValue::Number(1.0));
    assert_eq!(rt.eval("document.styleSheets[0].cssRules.length").unwrap(), JsValue::Number(2.0));
    assert_eq!(eval_string(&rt, "document.styleSheets[0].cssRules[1].selectorText"), "#b");

    rt.eval("document.styleSheets[0].deleteRule(0)").unwrap();
    assert_eq!(rt.eval("document.styleSheets[0].cssRules.length").unwrap(), JsValue::Number(1.0));
    assert_eq!(eval_string(&rt, "document.styleSheets[0].cssRules[0].selectorText"), "#b");
}

#[test]
fn insert_rule_at_top_level_apply_throws_syntax_error() {
    // Transcribes `mixin-cssom.tentative.html`'s "@apply is not legal at top
    // level" subtest verbatim: `@apply` is declaration-position only, so
    // parsing it as a standalone top-level rule text yields zero rules, not
    // one — `Stylesheet::insert_rule` already turns that into
    // `CssomRuleMutationError::Syntax` with no special-casing needed here.
    let rt = rt_with_sheet("");
    rt.eval(
        "globalThis.__err = null; \
         try { document.styleSheets[0].insertRule('@apply --m1();'); } \
         catch (e) { globalThis.__err = e.name; }",
    )
    .unwrap();
    assert_eq!(eval_string(&rt, "globalThis.__err"), "SyntaxError");
    assert_eq!(rt.eval("document.styleSheets[0].cssRules.length").unwrap(), JsValue::Number(0.0));
}

#[test]
fn insert_rule_rejects_index_past_the_end_with_index_size_error() {
    let rt = rt_with_sheet("#a {}");
    rt.eval(
        "globalThis.__err = null; \
         try { document.styleSheets[0].insertRule('#b {}', 5); } \
         catch (e) { globalThis.__err = e.name; }",
    )
    .unwrap();
    assert_eq!(eval_string(&rt, "globalThis.__err"), "IndexSizeError");
}

#[test]
fn delete_rule_rejects_index_at_the_length_with_index_size_error() {
    let rt = rt_with_sheet("#a {}");
    rt.eval(
        "globalThis.__err = null; \
         try { document.styleSheets[0].deleteRule(1); } \
         catch (e) { globalThis.__err = e.name; }",
    )
    .unwrap();
    assert_eq!(eval_string(&rt, "globalThis.__err"), "IndexSizeError");
}
