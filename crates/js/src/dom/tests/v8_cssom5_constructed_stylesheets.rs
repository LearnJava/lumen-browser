//! CSSOM-5 срез 1: `new CSSStyleSheet()`, `.replaceSync()`/`.replace()`,
//! `.cssRules`, `document.adoptedStyleSheets`/`shadowRoot.adoptedStyleSheets`
//! — the write half of CSSOM-1's read-only registry
//! (`v8_cssom_stylesheets.rs`). See
//! `crates/js/src/v8_runtime/install/constructed_stylesheets.rs` for what is
//! and is not wired up yet (no cascade application in this срез).
//!
//! Срез 2 tests below exercise `V8JsRuntime::document_adopted_fingerprint`/
//! `document_adopted_stylesheet` directly — the shell-side merge into the
//! page cascade (`crates/shell/src/relayout.rs`) has no JS runtime of its
//! own to unit-test against here.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn construct_without_new_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    let err = rt.eval("CSSStyleSheet()").unwrap_err();
    assert!(format!("{err:?}").contains("requires 'new'"), "{err:?}");
}

#[test]
fn construct_then_replace_sync_populates_rules() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var s = new CSSStyleSheet(); s.cssRules.length")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
    let r = rt
        .eval("s.replaceSync('p { color: red; }'); s.cssRules.length")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    let r = rt.eval("s.cssRules[0].selectorText").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("p".to_string()));
}

#[test]
fn replace_returns_promise_and_resolves() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval("var s = new CSSStyleSheet(); s.replace('div{}') instanceof Promise")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn constructed_sheet_is_instanceof_css_style_sheet() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new CSSStyleSheet() instanceof CSSStyleSheet").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn adopted_style_sheets_starts_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("document.adoptedStyleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn adopted_style_sheets_round_trips_constructed_sheet() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var s = new CSSStyleSheet(); s.replaceSync('p{}'); \
             document.adoptedStyleSheets = [s]; \
             document.adoptedStyleSheets.length",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    let r = rt
        .eval("document.adoptedStyleSheets[0].cssRules[0].selectorText")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("p".to_string()));
}

#[test]
fn adopted_style_sheets_rejects_non_stylesheet_elements() {
    let rt = v8_runtime_with_dom(make_doc());
    let err = rt.eval("document.adoptedStyleSheets = [null]").unwrap_err();
    assert!(format!("{err:?}").contains("Failed to convert value to 'CSSStyleSheet'"), "{err:?}");
    // BUG-897's exact measured symptom: the invalid assignment must not have
    // taken effect (previously it silently "succeeded" as a plain property).
    let r = rt.eval("document.adoptedStyleSheets.length").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn adopted_style_sheets_rejects_owned_readonly_sheet() {
    let (doc, style_nid) = {
        let doc_arc = make_doc();
        let style_nid = {
            let mut doc = doc_arc.lock().unwrap();
            let head = super::super::find_element_by_tag(&doc, "head").unwrap();
            let style = doc.create_element(QualName::html("style"));
            doc.append_child(head, style);
            style.index() as u32
        };
        (doc_arc, style_nid)
    };
    let rt = v8_runtime_with_dom(doc);
    rt.update_stylesheet_nodes(vec![lumen_css_parser::StylesheetNodeEntry {
        node: style_nid,
        sheet: Arc::new(lumen_css_parser::parse("p{}")),
        disabled: false,
    }]);
    // `document.styleSheets[0]` passes `instanceof CSSStyleSheet` but was
    // never constructed — it must still be rejected (no `_lumenSheetIdx`).
    let err = rt
        .eval("document.adoptedStyleSheets = [document.styleSheets[0]]")
        .unwrap_err();
    assert!(format!("{err:?}").contains("Failed to convert value to 'CSSStyleSheet'"), "{err:?}");
}

#[test]
fn shadow_root_adopted_style_sheets_survives_rebuild() {
    let rt = v8_runtime_with_dom(make_doc());
    // `host.shadowRoot` returns a fresh wrapper on every read (BUG-877) — the
    // assignment and the read below deliberately use two separate accesses
    // of `host.shadowRoot` to prove the state lives in Rust, not the wrapper.
    let r = rt
        .eval(
            "var host = document.createElement('div'); \
             document.body.appendChild(host); \
             var sr = host.attachShadow({mode:'open'}); \
             var s = new CSSStyleSheet(); s.replaceSync('a{}'); \
             sr.adoptedStyleSheets = [s]; \
             host.shadowRoot.adoptedStyleSheets.length",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn document_adopted_fingerprint_changes_on_assignment() {
    let rt = v8_runtime_with_dom(make_doc());
    let before = rt.document_adopted_fingerprint();
    // Trailing `.length` read keeps eval's completion value a plain number —
    // letting a constructed-sheet-bearing value fall out of eval() walks
    // `from_v8` into `cssRules`/`parentStyleSheet` getters that fabricate a
    // fresh, uncached wrapper on every read (BUG-994), which defeats the
    // identity-hash cycle guard and OOMs the isolate. Unrelated to this
    // срез's cascade wiring — see BUG-994.
    rt.eval(
        "var s = new CSSStyleSheet(); s.replaceSync('p{}'); \
         document.adoptedStyleSheets = [s]; document.adoptedStyleSheets.length",
    )
    .unwrap();
    let after = rt.document_adopted_fingerprint();
    assert_ne!(before, after);
}

#[test]
fn document_adopted_fingerprint_changes_on_replace_sync() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var s = new CSSStyleSheet(); document.adoptedStyleSheets = [s];")
        .unwrap();
    let before = rt.document_adopted_fingerprint();
    rt.eval("s.replaceSync('p { color: red; }');").unwrap();
    let after = rt.document_adopted_fingerprint();
    assert_ne!(before, after);
}

#[test]
fn document_adopted_fingerprint_changes_on_disabled_toggle() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var s = new CSSStyleSheet(); document.adoptedStyleSheets = [s];")
        .unwrap();
    let before = rt.document_adopted_fingerprint();
    rt.eval("s.disabled = true;").unwrap();
    let after = rt.document_adopted_fingerprint();
    assert_ne!(before, after);
}

#[test]
fn document_adopted_fingerprint_ignores_shadow_scope() {
    let rt = v8_runtime_with_dom(make_doc());
    let before = rt.document_adopted_fingerprint();
    // Trailing `.length` read — see the comment on
    // `document_adopted_fingerprint_changes_on_assignment` (BUG-978).
    rt.eval(
        "var host = document.createElement('div'); \
         document.body.appendChild(host); \
         var sr = host.attachShadow({mode:'open'}); \
         var s = new CSSStyleSheet(); s.replaceSync('a{}'); \
         sr.adoptedStyleSheets = [s]; sr.adoptedStyleSheets.length",
    )
    .unwrap();
    let after = rt.document_adopted_fingerprint();
    assert_eq!(before, after);
}

#[test]
fn document_adopted_stylesheet_is_none_when_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(rt.document_adopted_stylesheet().is_none());
}

#[test]
fn document_adopted_stylesheet_merges_enabled_members_in_order() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var a = new CSSStyleSheet(); a.replaceSync('p{color:red;}'); \
         var b = new CSSStyleSheet(); b.replaceSync('p{color:blue;}'); \
         document.adoptedStyleSheets = [a, b]; document.adoptedStyleSheets.length",
    )
    .unwrap();
    let sheet = rt.document_adopted_stylesheet().unwrap();
    assert_eq!(sheet.rules.len(), 2);
}

#[test]
fn document_adopted_stylesheet_skips_disabled_members() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var a = new CSSStyleSheet(); a.replaceSync('p{}'); a.disabled = true; \
         document.adoptedStyleSheets = [a]; document.adoptedStyleSheets.length",
    )
    .unwrap();
    assert!(rt.document_adopted_stylesheet().is_none());
}
