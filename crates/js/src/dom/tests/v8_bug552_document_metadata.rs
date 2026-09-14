//! BUG-552 — "document.compatMode (and sibling document metadata
//! properties) missing on the live document". Filed 2026-08-04 against a
//! `dom.rs` snapshot from before BUG-358's fix (2026-08-09); by the time
//! this was picked up the live `document` already had `compatMode`,
//! `characterSet`/`charset`/`inputEncoding`, `contentType`, `URL` and
//! `documentURI` wired to real per-load state, including doctype-driven
//! quirks-mode detection (`Document::mode()`, set by `lumen-html-parser`
//! from the DOCTYPE, not a hardcoded literal). These tests pin that the
//! live document answers correctly in both quirks and no-quirks mode —
//! the exact gap the bug report described as still open — so the report
//! can close as a duplicate of BUG-358 instead of sitting stale.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::DocumentMode;

fn runtime_with(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(
        doc,
        "https://example.com/page.html",
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

fn string_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The exact symptom line from the report: the property must exist on the
/// live document, not just read as `undefined` — `in` distinguishes
/// "absent" from "present but empty".
#[test]
fn document_metadata_properties_are_present() {
    let rt = runtime_with(make_doc());
    for prop in ["compatMode", "characterSet", "charset", "inputEncoding", "contentType", "URL", "documentURI"] {
        assert!(is_true(&rt, &format!("'{prop}' in document")), "missing: {prop}");
    }
}

/// `compatMode` must reflect the tree builder's real quirks-mode flag, not
/// a hardcoded `"CSS1Compat"` — the report's specific remaining complaint.
#[test]
fn compat_mode_reflects_no_quirks() {
    let doc = make_doc();
    doc.lock().unwrap().set_mode(DocumentMode::NoQuirks);
    let rt = runtime_with(doc);
    assert_eq!(string_of(&rt, "document.compatMode"), "CSS1Compat");
}

#[test]
fn compat_mode_reflects_quirks() {
    let doc = make_doc();
    doc.lock().unwrap().set_mode(DocumentMode::Quirks);
    let rt = runtime_with(doc);
    assert_eq!(string_of(&rt, "document.compatMode"), "BackCompat");
}

/// Limited-quirks (spec-defined third mode) still reports as `"CSS1Compat"`
/// — only full quirks mode gets `"BackCompat"` (HTML LS §"quirks mode").
#[test]
fn compat_mode_limited_quirks_is_css1_compat() {
    let doc = make_doc();
    doc.lock().unwrap().set_mode(DocumentMode::LimitedQuirks);
    let rt = runtime_with(doc);
    assert_eq!(string_of(&rt, "document.compatMode"), "CSS1Compat");
}

/// `characterSet`/`charset`/`inputEncoding` are spec-defined aliases of the
/// same per-load encoding, sourced from `Document::character_set`.
#[test]
fn character_set_aliases_agree() {
    let doc = make_doc();
    doc.lock().unwrap().set_character_set("windows-1251".to_string());
    let rt = runtime_with(doc);
    for prop in ["characterSet", "charset", "inputEncoding"] {
        assert_eq!(string_of(&rt, &format!("document.{prop}")), "windows-1251");
    }
}

#[test]
fn content_type_reflects_document_field() {
    let doc = make_doc();
    doc.lock().unwrap().set_content_type("application/xhtml+xml".to_string());
    let rt = runtime_with(doc);
    assert_eq!(string_of(&rt, "document.contentType"), "application/xhtml+xml");
}

/// `URL`/`documentURI` agree with the page URL the runtime was installed
/// with, and with each other (both are the document's address, per DOM §7.3).
#[test]
fn url_and_document_uri_match_page_url() {
    let rt = runtime_with(make_doc());
    assert_eq!(string_of(&rt, "document.URL"), "https://example.com/page.html");
    assert_eq!(string_of(&rt, "document.documentURI"), "https://example.com/page.html");
}
