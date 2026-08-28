//! S12b-24-trusted-types (thirtieth porting slice): both Trusted Types clusters
//! (AA-5, W3C TT L2 Phase 0) merged into one module, **19 tests** — the ROADMAP/
//! findings-log estimate of "11 tests total" undercounted, having missed this
//! first cluster entirely (same "don't trust the count" gotcha as
//! `S12b-24-css-storage-nav-misc`/`S12b-24-pointer-lock`). `V8JsRuntime::install_dom`
//! now evaluates the shared `TRUSTED_TYPES_SHIM` constant directly (plain JS, no
//! `rquickjs`-specific API — see `v8_runtime.rs`), so `trustedTypes` works
//! identically to the QuickJS path. All bodies are synchronous `rt.eval(...)`,
//! no promise/microtask timing, so the S12b-2 lesson doesn't apply. QuickJS
//! copies deleted.

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

#[test]
fn trusted_types_create_policy_invokes_rule() {
    let rt = v8_runtime_with_dom(make_doc());
    // The policy's own createHTML callback transforms the input.
    let r = rt.eval(
        "var p = trustedTypes.createPolicy('escape', {
                     createHTML: function(s) { return s.replace(/</g, '&lt;'); }
                 });
                 var h = p.createHTML('<b>x</b>');
                 p.name === 'escape' && h instanceof TrustedHTML && String(h) === '&lt;b>x&lt;/b>'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_missing_rule_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    // Policy without a createScript member: calling createScript throws TypeError.
    let r = rt.eval(
        "var p = trustedTypes.createPolicy('html-only', {
                     createHTML: function(s) { return s; }
                 });
                 var got = '';
                 try { p.createScript('x'); } catch (e) { got = e.constructor.name; }
                 got === 'TypeError'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_default_policy_guard() {
    let rt = v8_runtime_with_dom(make_doc());
    // defaultPolicy is null until "default" is registered; second registration throws.
    let r = rt.eval(
        "var before = trustedTypes.defaultPolicy === null;
                 var dp = trustedTypes.createPolicy('default', { createHTML: function(s) { return s; } });
                 var after = trustedTypes.defaultPolicy === dp;
                 var guarded = false;
                 try { trustedTypes.createPolicy('default', {}); } catch (e) { guarded = e instanceof TypeError; }
                 before && after && guarded"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_brand_checks() {
    let rt = v8_runtime_with_dom(make_doc());
    // isHTML/isScript/isScriptURL: true only for the matching brand,
    // false for plain strings and for forged prototype chains.
    let r = rt.eval(
        "var p = trustedTypes.createPolicy('p', {
                     createHTML: function(s) { return s; },
                     createScript: function(s) { return s; },
                     createScriptURL: function(s) { return s; }
                 });
                 var h = p.createHTML('a'), s = p.createScript('b'), u = p.createScriptURL('c');
                 var forged = Object.create(TrustedHTML.prototype);
                 trustedTypes.isHTML(h) && !trustedTypes.isHTML(s) && !trustedTypes.isHTML('a') &&
                     !trustedTypes.isHTML(forged) &&
                     trustedTypes.isScript(s) && !trustedTypes.isScript(h) &&
                     trustedTypes.isScriptURL(u) && !trustedTypes.isScriptURL(s)"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_empty_html_and_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "trustedTypes.isHTML(trustedTypes.emptyHTML) && String(trustedTypes.emptyHTML) === '' &&
                 trustedTypes.isScript(trustedTypes.emptyScript) && String(trustedTypes.emptyScript) === ''"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_illegal_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    // Trusted value classes and TrustedTypePolicy are not page-constructible.
    let r = rt.eval(
        "var hits = 0;
                 [TrustedHTML, TrustedScript, TrustedScriptURL, TrustedTypePolicy].forEach(function(C) {
                     try { new C('x'); } catch (e) { if (e instanceof TypeError) hits++; }
                 });
                 hits === 4"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_sink_tables() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "trustedTypes.getAttributeType('iframe', 'srcdoc') === 'TrustedHTML' &&
                 trustedTypes.getAttributeType('script', 'src') === 'TrustedScriptURL' &&
                 trustedTypes.getAttributeType('div', 'onclick') === 'TrustedScript' &&
                 trustedTypes.getAttributeType('div', 'id') === null &&
                 trustedTypes.getPropertyType('div', 'innerHTML') === 'TrustedHTML' &&
                 trustedTypes.getPropertyType('script', 'src') === 'TrustedScriptURL' &&
                 trustedTypes.getPropertyType('script', 'textContent') === 'TrustedScript' &&
                 trustedTypes.getPropertyType('div', 'className') === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_rule_receives_extra_args() {
    let rt = v8_runtime_with_dom(make_doc());
    // createHTML(input, ...args): extra arguments are forwarded to the rule.
    let r = rt.eval(
        "var p = trustedTypes.createPolicy('args', {
                     createHTML: function(s, a, b) { return s + ':' + a + ':' + b; }
                 });
                 String(p.createHTML('x', 1, 2)) === 'x:1:2'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_is_defined() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("typeof trustedTypes === 'object' && trustedTypes !== null")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn create_policy_returns_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', {}); \
                     typeof p === 'object' && p !== null && p.name === 'test'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn create_html_returns_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createHTML: s => s }); \
                     const th = p.createHTML('<div>test</div>'); \
                     th instanceof TrustedHTML && th.toString() === '<div>test</div>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn create_script_returns_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createScript: s => s }); \
                     const ts = p.createScript('var x = 1'); \
                     ts instanceof TrustedScript && ts.toString() === 'var x = 1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn create_script_url_returns_trusted_script_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createScriptURL: s => s }); \
                     const tsu = p.createScriptURL('https://example.com/script.js'); \
                     tsu instanceof TrustedScriptURL && tsu.toString() === 'https://example.com/script.js'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn default_policy_create_html_works() {
    let rt = v8_runtime_with_dom(make_doc());
    // TT L2: the default policy exists only after createPolicy('default', ...).
    let r = rt
        .eval(
            "trustedTypes.createPolicy('default', { createHTML: s => s }); \
                     const th = trustedTypes.defaultPolicy.createHTML('<p>test</p>'); \
                     th instanceof TrustedHTML && th.toString() === '<p>test</p>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn duplicate_non_default_policy_names_allowed() {
    let rt = v8_runtime_with_dom(make_doc());
    // Without a CSP trusted-types directive, duplicate non-default names
    // are allowed (TT L2 §4.3); only "default" is guarded.
    let r = rt
        .eval(
            "const a = trustedTypes.createPolicy('mypolicy', {}); \
                     const b = trustedTypes.createPolicy('mypolicy', {}); \
                     a !== b && a.name === 'mypolicy' && b.name === 'mypolicy'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn is_html_true_for_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createHTML: s => s }); \
                     const th = p.createHTML('<div></div>'); \
                     trustedTypes.isHTML(th)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn is_html_false_for_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("trustedTypes.isHTML('<div></div>')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn is_script_true_for_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createScript: s => s }); \
                     const ts = p.createScript('x=1'); \
                     trustedTypes.isScript(ts)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn is_script_url_true_for_trusted_script_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "const p = trustedTypes.createPolicy('test', { createScriptURL: s => s }); \
                     const tsu = p.createScriptURL('https://example.com/s.js'); \
                     trustedTypes.isScriptURL(tsu)",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
