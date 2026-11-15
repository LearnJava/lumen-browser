//! BUG-1173 — `new URL(x)` without a `base` must parse `x` with no base at
//! all (URL Standard §6.1): a schemeless string is a failure → `TypeError`,
//! and the document URL must not be substituted. Callers that want the
//! document resolved into it pass the base explicitly.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn omitted_base_throws_instead_of_resolving_against_the_document() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
function throws(v) { try { new URL(v); return false; } catch (e) { return e instanceof TypeError; } }
throws('0') && throws('a--b') && throws('../realitive/url')
  && throws('/absolute/../path?') && throws('Secure-Payment-Confirmation')
  && throws('') && throws('not-valid')
  && new URL('https://site.example/x').href === 'https://site.example/x'
  && new URL('0', 'https://example.com/').href === 'https://example.com/0'
  && new URL('', 'https://example.com/a/b').href === 'https://example.com/a/b'
  && new URL('../x', 'https://example.com/a/b').href === 'https://example.com/x'
"#
    ));
}

#[test]
fn input_type_url_rejects_schemeless_values() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var el = document.createElement('input');
el.type = 'url';
el.value = 'foo';
var bad = el.validity.typeMismatch && !el.checkValidity();
el.value = 'https://site.example/x';
var good = !el.validity.typeMismatch && el.checkValidity();
bad && good
"#
    ));
}
