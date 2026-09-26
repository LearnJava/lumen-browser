//! GAP-ORIGIN — the `Origin` interface (HTML LS §7.1.1, whatwg/html#11846):
//! `new Origin()`, `Origin.from()` over strings, `URL`, `Origin`, the global
//! and hyperlink elements, `opaque`, `isSameOrigin()`/`isSameSite()`.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::ext::PublicSuffixList;
use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

/// Every TLD is a public suffix and nothing else is — enough to give
/// `a.example`/`b.a.example` one registrable domain without the real table
/// (which lives in `lumen-storage`, a dependent of this crate).
struct LastLabelPsl;

impl PublicSuffixList for LastLabelPsl {
    fn public_suffix<'a>(&self, domain: &'a str) -> Option<&'a str> {
        domain.rsplit('.').next()
    }
    fn registrable_domain<'a>(&self, domain: &'a str) -> Option<&'a str> {
        let mut dots = domain.rmatch_indices('.');
        dots.next()?;
        Some(dots.next().map_or(domain, |(i, _)| &domain[i + 1..]))
    }
    fn is_public_suffix(&self, domain: &str) -> bool {
        !domain.contains('.')
    }
    fn provider_name(&self) -> &'static str {
        "last-label"
    }
}

fn make_rt() -> V8JsRuntime {
    lumen_js::set_public_suffix_list(Arc::new(LastLabelPsl));
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false)
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
fn from_string_distinguishes_tuple_opaque_and_invalid() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
function throws(v) { try { Origin.from(v); return false; } catch (e) { return e instanceof TypeError; } }
!Origin.from('https://site.example:123').opaque
  && !Origin.from('blob:https://example.com/guid').opaque
  && !Origin.from('https://[::1]/').opaque
  && Origin.from('data:text/plain,x').opaque
  && Origin.from('weird-hierarchical-protocol://host/path').opaque
  && Origin.from('blob:weird-protocol:whatever').opaque
  && throws('') && throws('not-valid') && throws(null) && throws({})
  && throws(Origin) && throws(1) && throws(window.location)
"#
    ));
}

#[test]
fn opaque_origins_are_same_origin_only_with_themselves() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var a = new Origin(), b = new Origin();
var d1 = Origin.from('data:text/plain,x'), d2 = Origin.from('data:text/plain,x');
a.opaque && a.isSameOrigin(a) && a.isSameSite(a) && !a.isSameOrigin(b) && !a.isSameSite(b)
  && !d1.isSameOrigin(d2) && Origin.from(d1).isSameOrigin(d1)
"#
    ));
}

#[test]
fn tuple_comparison_is_schemeful_and_site_uses_registrable_domain() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var a = Origin.from('https://a.example'), aa = Origin.from('https://a.a.example');
var b = Origin.from('https://b.example'), http = Origin.from('http://a.example');
a.isSameOrigin(Origin.from(new URL('https://a.example:443/p')))
  && !a.isSameOrigin(aa) && a.isSameSite(aa) && !a.isSameSite(b)
  && !a.isSameOrigin(http) && !a.isSameSite(http)
"#
    ));
}

#[test]
fn global_and_hyperlink_elements_have_origins() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
function throws(v) { try { Origin.from(v); return false; } catch (e) { return e instanceof TypeError; } }
var a = document.createElement('a');
var noHref = throws(a);
a.href = 'https://site.example/x';
var svgA = document.createElementNS('http://www.w3.org/2000/svg', 'a');
var svgNoHref = throws(svgA);
svgA.href.baseVal = 'data:text/plain,x';
var xl = document.createElementNS('http://www.w3.org/2000/svg', 'a');
xl.setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', 'https://site.example/');
Origin.from(window).isSameOrigin(Origin.from('https://example.com'))
  && Origin.from(globalThis).isSameOrigin(Origin.from(window))
  && noHref && Origin.from(a).isSameOrigin(Origin.from('https://site.example'))
  && svgNoHref && Origin.from(svgA).opaque && svgA.href === svgA.href
  && svgA.href.baseVal === 'data:text/plain,x'
  && Origin.from(xl).isSameOrigin(Origin.from('https://site.example'))
"#
    ));
}

#[test]
fn constructed_message_event_has_no_origin() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var ctorThrows = false;
try { Origin.from(new MessageEvent('message', { origin: 'https://example.com' })); }
catch (e) { ctorThrows = e instanceof TypeError; }
ctorThrows
"#
    ));
}
