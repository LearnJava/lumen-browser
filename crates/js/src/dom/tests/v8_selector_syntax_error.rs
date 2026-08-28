//! BUG-391 — `querySelector(All)`/`matches()`/`closest()` must throw a
//! `SyntaxError` DOMException for an invalid or engine-unknown selector.
//!
//! The regression these guard against is the *silent* one: before the fix
//! every entry point funnelled an unparsable selector into the same empty
//! result an ordinary no-match produces, so the standard WPT
//! feature-detection idiom (`assert_throws_dom('SyntaxError', () =>
//! el.matches(':unknown-pseudo'))`) could not tell "not supported" from
//! "did not match". Hence the paired negative assertions below: a valid
//! selector that matches nothing must still *not* throw, and
//! `getElementsByTagName`/`getElementsByClassName` — which share the same
//! natives but are specified never to throw — must stay quiet.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Evaluates `expr` and reports the `name` of whatever it threw, or
/// `'no-throw'` when it completed normally.
fn throw_name(rt: &V8JsRuntime, expr: &str) -> String {
    let script = format!(
        "(function() {{ try {{ {expr}; return 'no-throw'; }} \
                 catch (e) {{ return String(e && e.name); }} }})()"
    );
    match rt.eval(&script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn matches_throws_syntax_error_on_unknown_pseudo_class() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        throw_name(&rt, "document.body.matches(':halfscreen')"),
        "SyntaxError"
    );
}

#[test]
fn matches_throws_dom_exception_not_plain_error() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "(function() { try { document.body.matches(':halfscreen'); return false; } \
                 catch (e) { return e instanceof DOMException && e.code === DOMException.SYNTAX_ERR; } })()"
    ));
}

#[test]
fn query_selector_throws_syntax_error_on_malformed_selector() {
    let rt = v8_runtime_with_dom(make_doc());
    for expr in [
        "document.querySelector('(')",
        "document.querySelector('')",
        "document.querySelector(':bogus-pseudo')",
        "document.querySelectorAll('div,')",
    ] {
        assert_eq!(throw_name(&rt, expr), "SyntaxError", "for {expr}");
    }
}

#[test]
fn scoped_query_selector_and_closest_throw_too() {
    let rt = v8_runtime_with_dom(make_doc());
    for expr in [
        "document.body.querySelector(':bogus-pseudo')",
        "document.body.querySelectorAll(':bogus-pseudo')",
        "document.body.closest(':bogus-pseudo')",
    ] {
        assert_eq!(throw_name(&rt, expr), "SyntaxError", "for {expr}");
    }
}

#[test]
fn valid_selector_with_no_match_still_returns_null() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(throw_name(&rt, "document.querySelector('.no-match')"), "no-throw");
    assert!(bool_eval(&rt, "document.querySelector('.no-match') === null"));
    assert!(bool_eval(&rt, "document.querySelectorAll('.no-match').length === 0"));
    assert!(bool_eval(&rt, "document.body.matches('.no-match') === false"));
    assert!(bool_eval(&rt, "document.body.closest('.no-match') === null"));
}

#[test]
fn ordinary_selectors_keep_working() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.querySelector('#main') !== null"));
    assert!(bool_eval(&rt, "document.querySelectorAll('.highlight').length === 1"));
    assert!(bool_eval(&rt, "document.querySelector('#main').matches('div#main')"));
    assert!(bool_eval(&rt, "document.querySelector('.highlight').closest('div') !== null"));
}

#[test]
fn get_elements_by_tag_and_class_never_throw() {
    // DOM LS specifies neither as throwing on a weird argument — they
    // reuse the same lenient natives on purpose (BUG-391 guards only the
    // selector-taking entry points).
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        throw_name(&rt, "document.getElementsByTagName(':bogus')"),
        "no-throw"
    );
    assert_eq!(
        throw_name(&rt, "document.getElementsByClassName(':bogus')"),
        "no-throw"
    );
}
