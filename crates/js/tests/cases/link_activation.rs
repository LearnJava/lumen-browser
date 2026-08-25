//! Hyperlink activation behaviour (HTML LS §4.6.1 «follow the hyperlink»)
//! through the real V8 runtime and JS shim.
//!
//! Regression cover for [BUG-833](../../../../bugs/BUG-833-FIXED.md): the
//! `A`/`AREA` branch of `_lumen_run_activation_behavior` used to call
//! `_lumen_navigate` unconditionally, so activating `<a href="#x">` asked the
//! shell for a *full* navigation — the document was reparsed from scratch, no
//! `hashchange` was dispatched, and a page clicking such a link from script
//! looped through reloads forever. HTML LS §7.4.2 decides «fragment or load»
//! from the URL alone, so link activation has to enter the same
//! `_lumen_navigate_or_fragment` path `location.href =` already used.
//!
//! Both halves are asserted: a same-document fragment target must leave the
//! navigation channel *empty* (nothing for the shell to load) while updating
//! `location` and firing `hashchange`, and a cross-document target must still
//! post the full navigation it always did.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;
use lumen_js::NavigateRequest;

/// A runtime whose document lives at a URL with a path, so a fragment-only
/// target and a path-relative one resolve to visibly different strings.
fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(
        doc,
        "https://example.com/doc",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

fn str_eval(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected string from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

/// Build a detached `<a>` with the given `href` and activate it. Activation
/// behaviour does not require the element to be connected, so no body is
/// needed — which keeps the test independent of parser/layout state.
const CLICK_ANCHOR: &str = r#"
var _log = [];
addEventListener('hashchange', function (e) {
    _log.push('hashchange ' + e.oldURL + ' -> ' + e.newURL);
});
var _a = document.createElement('a');
_a.setAttribute('href', HREF);
_a.click();
// BUG-832: `hashchange` is queued on the task source (HTML LS §7.10.6), so the
// event loop has to turn once before the log can contain it. `location`, by
// contrast, has already moved — the two halves are deliberately read together
// here so a regression that re-synchronizes the dispatch shows up as a *double*
// entry rather than passing quietly.
_lumen_tick_timers();
_log.join('|') + ' @ ' + location.href + ' hash=' + location.hash
"#;

fn click_anchor(rt: &V8JsRuntime, href: &str) -> String {
    str_eval(rt, &CLICK_ANCHOR.replace("HREF", &format!("'{href}'")))
}

#[test]
fn fragment_link_activation_stays_in_the_document() {
    let rt = make_rt();
    let out = click_anchor(&rt, "#sec");
    assert_eq!(
        out,
        "hashchange https://example.com/doc -> https://example.com/doc#sec \
         @ https://example.com/doc#sec hash=#sec",
        "a fragment-only target must fire `hashchange` and move `location`"
    );
    assert!(
        rt.take_navigate_request().is_none(),
        "a same-document fragment navigation must not ask the shell to load anything"
    );
}

#[test]
fn fragment_link_activation_from_a_url_with_a_query() {
    // The comparison is «everything before the `#`», so a target that keeps the
    // query and only adds a fragment is same-document too.
    let rt = make_rt();
    let out = click_anchor(&rt, "https://example.com/doc#other");
    assert!(
        out.starts_with("hashchange https://example.com/doc -> https://example.com/doc#other"),
        "unexpected result: {out}"
    );
    assert!(rt.take_navigate_request().is_none(), "unexpected navigation: {out}");
}

#[test]
fn cross_document_link_activation_still_navigates() {
    let rt = make_rt();
    let out = click_anchor(&rt, "/other");
    assert_eq!(
        out,
        " @ https://example.com/doc hash=",
        "a cross-document target must not touch `location` on the JS side"
    );
    match rt.take_navigate_request() {
        Some(NavigateRequest::Push(url)) => assert_eq!(url, "https://example.com/other"),
        other => panic!("expected a Push navigation, got {other:?}"),
    }
}

#[test]
fn cross_document_link_activation_with_a_different_path_and_a_fragment() {
    // A fragment on a *different* document is a full navigation — the URL, not
    // the presence of a `#`, is what decides.
    let rt = make_rt();
    click_anchor(&rt, "/elsewhere#sec");
    match rt.take_navigate_request() {
        Some(NavigateRequest::Push(url)) => {
            assert_eq!(url, "https://example.com/elsewhere#sec");
        }
        other => panic!("expected a Push navigation, got {other:?}"),
    }
}
