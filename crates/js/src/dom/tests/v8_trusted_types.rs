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
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc, "", None, None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    rt
}

#[test]
fn trusted_types_create_policy_invokes_rule() {
    let rt = v8_runtime_with_dom(make_doc());
    // The policy's own createHTML callback transforms the input.
    let r = rt
        .eval(
            "var p = trustedTypes.createPolicy('escape', {
                     createHTML: function(s) { return s.replace(/</g, '&lt;'); }
                 });
                 var h = p.createHTML('<b>x</b>');
                 p.name === 'escape' && h instanceof TrustedHTML && String(h) === '&lt;b>x&lt;/b>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_missing_rule_throws_type_error() {
    let rt = v8_runtime_with_dom(make_doc());
    // Policy without a createScript member: calling createScript throws TypeError.
    let r = rt
        .eval(
            "var p = trustedTypes.createPolicy('html-only', {
                     createHTML: function(s) { return s; }
                 });
                 var got = '';
                 try { p.createScript('x'); } catch (e) { got = e.constructor.name; }
                 got === 'TypeError'",
        )
        .unwrap();
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
    let r = rt
        .eval(
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
                     trustedTypes.isScriptURL(u) && !trustedTypes.isScriptURL(s)",
        )
        .unwrap();
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
    let r = rt
        .eval(
            "trustedTypes.getAttributeType('iframe', 'srcdoc') === 'TrustedHTML' &&
                 trustedTypes.getAttributeType('script', 'src') === 'TrustedScriptURL' &&
                 trustedTypes.getAttributeType('div', 'onclick') === 'TrustedScript' &&
                 trustedTypes.getAttributeType('div', 'id') === null &&
                 trustedTypes.getPropertyType('div', 'innerHTML') === 'TrustedHTML' &&
                 trustedTypes.getPropertyType('script', 'src') === 'TrustedScriptURL' &&
                 trustedTypes.getPropertyType('script', 'textContent') === 'TrustedScript' &&
                 trustedTypes.getPropertyType('div', 'className') === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn trusted_types_rule_receives_extra_args() {
    let rt = v8_runtime_with_dom(make_doc());
    // createHTML(input, ...args): extra arguments are forwarded to the rule.
    let r = rt
        .eval(
            "var p = trustedTypes.createPolicy('args', {
                     createHTML: function(s, a, b) { return s + ':' + a + ':' + b; }
                 });
                 String(p.createHTML('x', 1, 2)) === 'x:1:2'",
        )
        .unwrap();
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

// TRUSTEDTYPES-1 срез 1: `_lumen_tt_get_compliant_script`, the script-sink
// half of TT L2 §4.1.1 that the shell's `_lumen_tt_set_require_script` flips
// on for `require-trusted-types-for 'script'` (`crates/shell/src/scripts.rs`).

#[test]
fn tt_get_compliant_script_passthrough_without_require_flag() {
    let rt = v8_runtime_with_dom(make_doc());
    // Flag unset (no CSP directive): a plain string passes through verbatim,
    // matching pre-TRUSTEDTYPES-1 behaviour for pages that never opt in.
    let r = rt
        .eval("_lumen_tt_get_compliant_script('x=1', 'test-sink') === 'x=1'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_script_throws_without_default_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var threw = false; \
                     try { _lumen_tt_get_compliant_script('x=1', 'test-sink'); } \
                     catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_script_routes_through_default_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var seenType, seenSink; \
                     trustedTypes.createPolicy('default', { createScript: function (s, t, sink) { \
                         seenType = t; seenSink = sink; return s + ':ok'; \
                     }}); \
                     _lumen_tt_get_compliant_script('x=1', 'test-sink') === 'x=1:ok' && \
                         seenType === 'TrustedScript' && seenSink === 'test-sink'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_script_unwraps_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    // A TrustedScript value already satisfies the sink; the default policy
    // (absent here) must not be consulted.
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScript: s => s }); \
                     var ts = p.createScript('x=1'); \
                     _lumen_tt_get_compliant_script(ts, 'test-sink') === 'x=1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// TRUSTEDTYPES-1 срез 2: `_lumen_tt_get_compliant_html`, the HTML-sink half
// of the same §4.1.1 algorithm, gated by the same `_lumen_tt_set_require_script`
// flag (CSP `require-trusted-types-for 'script'` names the one sink group
// TT L2 defines, covering HTML/Script/ScriptURL sinks alike).

#[test]
fn tt_get_compliant_html_passthrough_without_require_flag() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("_lumen_tt_get_compliant_html('<p>x</p>', 'test-sink') === '<p>x</p>'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_html_null_no_flag_no_nulltoempty() {
    let rt = v8_runtime_with_dom(make_doc());
    // Without the flag, `null` is not special-cased unless the caller opts in
    // via the third `nullToEmpty` argument (insertAdjacentHTML's plain
    // DOMString parameter stringifies null as the text "null").
    let r = rt
        .eval("_lumen_tt_get_compliant_html(null, 'test-sink') === 'null'")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_html_null_to_empty_opt_in() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("_lumen_tt_get_compliant_html(null, 'test-sink', true) === ''")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_html_throws_without_default_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var threw = false; \
                     try { _lumen_tt_get_compliant_html('<p>x</p>', 'test-sink'); } \
                     catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_html_routes_through_default_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var seenType, seenSink; \
                     trustedTypes.createPolicy('default', { createHTML: function (s, t, sink) { \
                         seenType = t; seenSink = sink; return s + ':ok'; \
                     }}); \
                     _lumen_tt_get_compliant_html('<p>x</p>', 'test-sink') === '<p>x</p>:ok' && \
                         seenType === 'TrustedHTML' && seenSink === 'test-sink'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_get_compliant_html_unwraps_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    // A TrustedHTML value already satisfies the sink; the default policy
    // (absent here) must not be consulted.
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createHTML: s => s }); \
                     var th = p.createHTML('<p>x</p>'); \
                     _lumen_tt_get_compliant_html(th, 'test-sink') === '<p>x</p>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_element_innerhtml_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     var threw = false; \
                     try { d.innerHTML = '<b>x</b>'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_element_innerhtml_accepts_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createHTML: s => s }); \
                     var d = document.createElement('div'); \
                     d.innerHTML = p.createHTML('<b>x</b>'); \
                     d.innerHTML === '<b>x</b>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_element_innerhtml_null_becomes_empty_via_default_policy() {
    let rt = v8_runtime_with_dom(make_doc());
    // `innerHTML=null` carries [LegacyNullToEmptyString]: with a default
    // policy set, the compliant-string algorithm still routes '' through it
    // (not skipped), matching HTMLElement-generic.html's expectation that
    // `null` "accepts ... after default policy was created" (result '').
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var seen; \
                     trustedTypes.createPolicy('default', { createHTML: function(s) { seen = s; return s; } }); \
                     var d = document.createElement('div'); \
                     d.innerHTML = null; \
                     seen === '' && d.innerHTML === ''",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_insert_adjacent_html_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     document.body.appendChild(d); \
                     var threw = false; \
                     try { d.insertAdjacentHTML('beforeend', '<b>x</b>'); } \
                     catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_document_write_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var threw = false; \
                     try { document.write('<b>x</b>'); } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// TRUSTEDTYPES-1 срез 3: `Element.setAttribute`/`setAttributeNS` gated by
// `getAttributeType` (`on*` -> TrustedScript, `iframe[srcdoc]` -> TrustedHTML,
// `script[src]` -> TrustedScriptURL), the matching IDL property pair
// (`HTMLScriptElement.src`, `HTMLIFrameElement.srcdoc`), and
// `Range.createContextualFragment` (an HTML sink).

#[test]
fn tt_enforced_set_attribute_onclick_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     var threw = false; \
                     try { d.setAttribute('onclick', 'x=1'); } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_set_attribute_onclick_accepts_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScript: s => s }); \
                     var d = document.createElement('div'); \
                     d.setAttribute('onclick', p.createScript('x=1')); \
                     d.getAttribute('onclick') === 'x=1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_set_attribute_unrelated_attr_untouched() {
    // `id`/`class`/etc. are not in the TT §4.4 sink table -- plain string
    // assignment must keep working even under enforcement.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     d.setAttribute('id', 'foo'); \
                     d.getAttribute('id') === 'foo'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_set_attribute_ns_onclick_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     var threw = false; \
                     try { d.setAttributeNS(null, 'onclick', 'x=1'); } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_iframe_srcdoc_property_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var f = document.createElement('iframe'); \
                     var threw = false; \
                     try { f.srcdoc = '<p>x</p>'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_iframe_srcdoc_property_accepts_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createHTML: s => s }); \
                     var f = document.createElement('iframe'); \
                     f.srcdoc = p.createHTML('<p>x</p>'); \
                     f.srcdoc === '<p>x</p>'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_src_property_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var s = document.createElement('script'); \
                     var threw = false; \
                     try { s.src = 'http://example.test/x.js'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_src_property_accepts_trusted_script_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScriptURL: s => s }); \
                     var s = document.createElement('script'); \
                     s.src = p.createScriptURL('http://example.test/x.js'); \
                     s.src === 'http://example.test/x.js'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_create_contextual_fragment_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var range = document.createRange(); \
                     var threw = false; \
                     try { range.createContextualFragment('<b>x</b>'); } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_create_contextual_fragment_accepts_trusted_html() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createHTML: s => s }); \
                     var range = document.createRange(); \
                     var frag = range.createContextualFragment(p.createHTML('<b>x</b>')); \
                     frag.textContent === 'x'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// TRUSTEDTYPES-1 срез 4: `<script>` `.textContent`/`.innerText`/`.text` — the
// "internal slot" text sinks HTML LS §3.6 "prepare the script text" gates
// against `HTMLScriptElement text` (all three IDL members share one sink
// name per the TT §4.4 property-type table). A non-script element must stay
// on the old unconditional-stringify path.

#[test]
fn tt_enforced_script_textcontent_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var s = document.createElement('script'); \
                     var threw = false; \
                     try { s.textContent = 'x=1'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_textcontent_accepts_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScript: s => s }); \
                     var s = document.createElement('script'); \
                     s.textContent = p.createScript('x=1'); \
                     s.textContent === 'x=1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_div_textcontent_unaffected() {
    // Non-script elements must keep accepting plain strings even under
    // enforcement -- `HTMLScriptElement text` names one specific interface.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var d = document.createElement('div'); \
                     d.textContent = 'hello'; \
                     d.textContent === 'hello'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_innertext_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var s = document.createElement('script'); \
                     document.body.appendChild(s); \
                     var threw = false; \
                     try { s.innerText = 'x=1'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_innertext_accepts_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScript: s => s }); \
                     var s = document.createElement('script'); \
                     document.body.appendChild(s); \
                     s.innerText = p.createScript('x=1'); \
                     s.textContent === 'x=1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_text_property_throws_plain_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var s = document.createElement('script'); \
                     var threw = false; \
                     try { s.text = 'x=1'; } catch (e) { threw = e instanceof TypeError; } \
                     threw",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_script_text_property_accepts_trusted_script() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var p = trustedTypes.createPolicy('p', { createScript: s => s }); \
                     var s = document.createElement('script'); \
                     s.text = p.createScript('x=1'); \
                     s.text === 'x=1'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn tt_enforced_anchor_text_property_unaffected() {
    // `HTMLAnchorElement.text` shares the `.text` reflection definition with
    // `HTMLScriptElement.text` -- it must not be gated by the script sink.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "_lumen_tt_set_require_script(true); \
                     var a = document.createElement('a'); \
                     a.text = 'hello'; \
                     a.text === 'hello'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
