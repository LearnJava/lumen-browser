//! BUG-1464 — Web-API offsets of `Selection`/`Range` must count UTF-16 code
//! units, not UTF-8 bytes.
//!
//! DOM §4.4: «offset» of a boundary point inside a `Text` node is a count of
//! UTF-16 code units, and Web IDL hands exactly that to the page
//! (`Range.setStart(text, 1)`, `Selection.setBaseAndExtent(t, 0, t, 1)`,
//! `Selection.anchorOffset`, …). The engine's internal model is different: the
//! DOM arena stores text positions as UTF-8 *byte* offsets, and the layout
//! caret/hit-test works in bytes too. The conversion between the two lived
//! nowhere — the JS boundary spoke bytes to the page and UTF-16 to itself.
//!
//! Symptom (all reproduced by the tests below): a script that selects one
//! Cyrillic letter gets an empty string back, and a script that selects a
//! whole Cyrillic word gets a byte-truncated prefix of it. `Selection`/
//! `Range` offsets reported to the page are byte counts, so
//! `getRangeAt(0).endOffset` is twice the real length for Cyrillic text.
//! The same truncation hit the shell's Ctrl+C of a script-made selection.
//!
//! Fix: convert at the JS↔DOM boundary (`crates/js/src/v8_runtime/install/
//! dom_editing.rs`) — UTF-16 in, UTF-8 bytes out, and back.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The nid of the text node inside `#main` — the style map is keyed by it.
fn text_nid(rt: &V8JsRuntime) -> u32 {
    match rt
        .eval("document.getElementById('main').firstChild.__nid__")
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid: {other:?}"),
    }
}

/// `<html><body><div id="main">text</div></body></html>` — the text node is
/// the only child of `#main`, so `firstChild` reaches it from the script.
fn make_text_doc(text: &str) -> (Arc<Mutex<Document>>, NodeId) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let div = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
        attrs.push(lumen_dom::Attribute {
            name: QualName::html("id"),
            value: "main".into(),
        });
    }
    let text_node = doc.create_text(text);
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, div);
    doc.append_child(div, text_node);
    (Arc::new(Mutex::new(doc)), text_node)
}

// ── Selection.toString() ─────────────────────────────────────────────────

#[test]
fn selection_to_string_covers_one_cyrillic_letter() {
    // "привет" — one Cyrillic letter is two UTF-8 bytes but one UTF-16 unit.
    let (arc, _) = make_text_doc("привет");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var s = window.getSelection(); \
         s.setBaseAndExtent(t, 0, t, 1); \
         s.toString() === 'п'"
    ));
}

#[test]
fn selection_to_string_covers_whole_cyrillic_word() {
    // Six letters = six UTF-16 units = twelve UTF-8 bytes; a byte-based cut
    // at 6 would return "при" instead of "привет".
    let (arc, _) = make_text_doc("привет");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var s = window.getSelection(); \
         s.setBaseAndExtent(t, 0, t, 6); \
         s.toString() === 'привет'"
    ));
}

#[test]
fn selection_to_string_covers_one_cjk_letter() {
    // U+3042 — three UTF-8 bytes, one UTF-16 unit.
    let (arc, _) = make_text_doc("あいう");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var s = window.getSelection(); \
         s.setBaseAndExtent(t, 0, t, 1); \
         s.toString() === 'あ'"
    ));
}

// ── Range.toString() ─────────────────────────────────────────────────────

#[test]
fn range_to_string_uses_utf16_offsets() {
    let (arc, _) = make_text_doc("привет");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var r = document.createRange(); \
         r.setStart(t, 0); r.setEnd(t, 3); \
         r.toString() === 'при'"
    ));
}

#[test]
fn range_delete_contents_uses_utf16_offsets() {
    // `deleteContents()` must cut three UTF-16 units ("при"), not three
    // UTF-8 bytes ("пр" plus a mangled third letter).
    let (arc, _) = make_text_doc("привет");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var r = document.createRange(); \
         r.setStart(t, 0); r.setEnd(t, 3); \
         r.deleteContents(); \
         t.data === 'вет'"
    ));
}

// ── Offsets reported back to the page ───────────────────────────────────

#[test]
fn selection_range_offsets_are_reported_in_utf16_units() {
    // The range handed out by `getRangeAt()` must speak the same units the
    // page used to build it — six, not twelve bytes.
    let (arc, _) = make_text_doc("привет");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var s = window.getSelection(); \
         s.setBaseAndExtent(t, 0, t, 6); \
         s.getRangeAt(0).endOffset === 6"
    ));
}

#[test]
fn selection_offsets_are_reported_in_utf16_units() {
    // The mouse selection installed by the shell carries UTF-8 byte offsets
    // (the engine's internal model); the page must still see UTF-16 units.
    let (arc, text) = make_text_doc("привет");
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            // Byte 2 — right after "п".
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 2 }),
            // Byte 6 — right after "при".
            focus: Some(lumen_dom::DomPosition { container: text, offset: 6 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var s = window.getSelection(); \
         s.anchorOffset === 1 && s.focusOffset === 3"
    ));
}

#[test]
fn range_offset_bounds_use_utf16_length() {
    // "😀a" is two characters but three UTF-16 units — an offset of 3 is
    // legal and must not raise IndexSizeError.
    let (arc, _) = make_text_doc("😀a");
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var r = document.createRange(); \
         r.setStart(t, 0); r.setEnd(t, 3); \
         r.toString() === '😀a'"
    ));
}

// ── text-transform over a UTF-16 selection ───────────────────────────────

#[test]
fn selection_to_string_applies_text_transform_over_utf16_selection() {
    // WPT `text-transform-full-size-kana-009`: the base text of the
    // selection was cut as UTF-8 bytes, so a one-character selection came
    // back empty and the shim's `text-transform` path never ran (it bails
    // out on an empty base). The covered piece is now cut in UTF-16 units
    // and transformed — U+3041 (small ぁ) → U+3042 (full-size あ).
    let (arc, _) = make_text_doc("ぁ");
    let rt = v8_runtime_with_dom(arc);
    let nid = text_nid(&rt);
    let mut props = std::collections::HashMap::new();
    props.insert("text-transform".to_string(), "full-size-kana".to_string());
    let mut styles = std::collections::HashMap::new();
    styles.insert(nid, props);
    rt.update_computed_styles(styles);
    assert!(bool_eval(
        &rt,
        "var t = document.getElementById('main').firstChild; \
         var s = window.getSelection(); \
         s.setBaseAndExtent(t, 0, t, 1); \
         s.toString() === 'あ'"
    ));
}
