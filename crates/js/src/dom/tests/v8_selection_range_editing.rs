//! V8 port of the "Selection + Range + execCommand + contentEditable" test row
//! (S12b-24-selection-range-editing), the thirteenth porting slice of the `dom.rs`
//! test-monolith migration described in `docs/tasks/ph3-v8-migration.md`: the
//! Selection API (`window.getSelection()`/`document.getSelection()`, `type`,
//! `rangeCount`, `isCollapsed`, `toString()`, `removeAllRanges`, `getRangeAt`,
//! `collapseToStart`), Range (`document.createRange()`, `collapse`, `cloneRange`,
//! `selectNodeContents`, `compareBoundaryPoints`, `window.Range`), `execCommand`
//! (`bold`/`italic`/unknown/`copy`, `queryCommandEnabled`/`State`/`Value`/
//! `Supported`, `insertText`, `delete`), and `contentEditable`/`isContentEditable`
//! including the `_lumen_handle_contenteditable_key` insert/delete-forward/
//! delete-backward dispatch and its `beforeinput`-cancellation + `input`-event
//! paths.
//!
//! The 42 tests moved here verbatim from the QuickJS monolith above — bodies are
//! `rt.eval(...)` plus direct `Document`/`NodeData` inspection through the shared
//! `Arc<Mutex<Document>>`, none of which touch the engine — so the only edit was
//! which runtime the fixture builds and re-typing `bool_eval` for `V8JsRuntime`.
//! No engine divergence found.

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

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// Build a doc with a single paragraph containing text "Hello World".
fn make_selection_doc() -> (Arc<Mutex<Document>>, NodeId) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let p = doc.create_element(QualName::html("p"));
    let text = doc.create_text("Hello World");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, p);
    doc.append_child(p, text);
    let arc = Arc::new(Mutex::new(doc));
    (arc, text)
}

// ── Selection API tests ───────────────────────────────────────────────────

#[test]
fn selection_window_get_selection_is_object() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.getSelection() === 'object'"));
}

#[test]
fn selection_document_get_selection_is_object() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.getSelection() === 'object'"));
}

#[test]
fn selection_initially_none_type() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.getSelection().type === 'None'"));
}

#[test]
fn selection_range_count_initially_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.getSelection().rangeCount === 0"));
}

#[test]
fn selection_is_collapsed_when_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.getSelection().isCollapsed === true"));
}

#[test]
fn selection_to_string_empty_when_no_selection() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "window.getSelection().toString() === ''"));
}

#[test]
fn selection_remove_all_ranges_clears() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().type === 'Range'"));
    rt.eval("window.getSelection().removeAllRanges()").unwrap();
    assert!(bool_eval(&rt, "window.getSelection().type === 'None'"));
}

#[test]
fn selection_type_range_when_set() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().type === 'Range'"));
}

#[test]
fn selection_is_not_collapsed_when_range() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().isCollapsed === false"));
}

#[test]
fn selection_to_string_returns_selected_text() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().toString() === 'Hello'"));
}

#[test]
fn selection_range_count_is_one_when_set() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 6 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 11 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().rangeCount === 1"));
}

#[test]
fn selection_get_range_at_returns_range() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 6 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 11 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(&rt, "window.getSelection().getRangeAt(0).toString() === 'World'"));
}

#[test]
fn selection_collapse_to_start() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    rt.eval("window.getSelection().collapseToStart()").unwrap();
    assert!(bool_eval(&rt, "window.getSelection().type === 'Caret'"));
}

// ── Range tests ───────────────────────────────────────────────────────────

#[test]
fn range_create_range_is_object() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.createRange() === 'object'"));
}

#[test]
fn range_new_is_collapsed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.createRange().collapsed === true"));
}

#[test]
fn range_start_offset_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.createRange().startOffset === 0"));
}

#[test]
fn range_collapse_to_start() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var r = document.createRange(); r.collapse(true); r.collapsed === true"
    ));
}

#[test]
fn range_clone_range() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var r = document.createRange(); var c = r.cloneRange(); c.collapsed === true"
    ));
}

#[test]
fn range_select_node_contents() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var el = document.getElementById('main'); \
                 var r = document.createRange(); \
                 r.selectNodeContents(el); \
                 r.startOffset === 0"
    ));
}

#[test]
fn range_to_string_via_get_range_at() {
    let (arc, text) = make_selection_doc();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        "window.getSelection().getRangeAt(0).toString() === 'Hello'"
    ));
}

#[test]
fn range_compare_boundary_points_same() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(
        &rt,
        "var r = document.createRange(); r.compareBoundaryPoints(0, r) === 0"
    ));
}

#[test]
fn range_window_range_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.Range === 'function'"));
}

// ── execCommand tests ─────────────────────────────────────────────────────

#[test]
fn exec_command_bold_returns_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.execCommand('bold') === true"));
}

#[test]
fn exec_command_italic_returns_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.execCommand('italic') === true"));
}

#[test]
fn exec_command_unknown_returns_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.execCommand('unknownCommand') === false"));
}

#[test]
fn exec_command_copy_returns_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.execCommand('copy') === false"));
}

#[test]
fn exec_command_query_enabled() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.queryCommandEnabled('bold') === true"));
}

#[test]
fn exec_command_query_state() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.queryCommandState('bold') === false"));
}

#[test]
fn exec_command_query_value() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.queryCommandValue('bold') === ''"));
}

#[test]
fn exec_command_query_supported() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.queryCommandSupported('bold') === true"));
}

#[test]
fn exec_command_insert_text() {
    let (arc, text) = make_selection_doc();
    let text_idx = text.index();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 0 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    rt.eval("document.execCommand('insertText', false, 'Hi ')").unwrap();
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not text"),
    };
    assert_eq!(content, "Hi Hello World");
}

#[test]
fn exec_command_delete_removes_selection() {
    let (arc, text) = make_selection_doc();
    let text_idx = text.index();
    {
        let mut doc = arc.lock().unwrap();
        // Select "Hello "
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 6 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    rt.eval("document.execCommand('delete')").unwrap();
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not text"),
    };
    assert_eq!(content, "World");
}

// ── contentEditable / isContentEditable / contenteditable dispatch tests ────

fn make_contenteditable_doc() -> (Arc<Mutex<Document>>, NodeId, NodeId) {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    let div = doc.create_element(QualName::html("div"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
        attrs.push(Attribute {
            name: QualName::html("contenteditable"),
            value: String::new(),
        });
    }
    let text = doc.create_text("Hello");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, div);
    doc.append_child(div, text);
    let arc = Arc::new(Mutex::new(doc));
    (arc, div, text)
}

#[test]
fn contenteditable_property_true() {
    let (arc, div, _) = make_contenteditable_doc();
    let div_idx = div.index();
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        &format!("_lumen_make_element({}).contentEditable === 'true'", div_idx)
    ));
}

#[test]
fn contenteditable_is_content_editable_self() {
    let (arc, div, _) = make_contenteditable_doc();
    let div_idx = div.index();
    let rt = v8_runtime_with_dom(arc);
    assert!(bool_eval(
        &rt,
        &format!("_lumen_make_element({}).isContentEditable === true", div_idx)
    ));
}

#[test]
fn contenteditable_is_content_editable_ancestor() {
    let (arc, _, text) = make_contenteditable_doc();
    let text_idx = text.index();
    let rt = v8_runtime_with_dom(arc);
    // text node itself: _lumen_is_contenteditable checks ancestors
    assert!(bool_eval(
        &rt,
        &format!("_lumen_is_contenteditable({})", text_idx)
    ));
}

#[test]
fn contenteditable_non_editable_false() {
    let rt = v8_runtime_with_dom(make_doc());
    // body has no contenteditable
    let body_idx: u32 = if let lumen_core::JsValue::Number(n) =
        rt.eval("_lumen_u2n(_lumen_get_body())").unwrap()
    {
        n as u32
    } else {
        0
    };
    assert!(bool_eval(
        &rt,
        &format!("_lumen_make_element({}).isContentEditable === false", body_idx)
    ));
}

#[test]
fn contenteditable_set_property() {
    let rt = v8_runtime_with_dom(make_doc());
    // Create a div and set contentEditable
    rt.eval("var _ce_div = document.createElement('div'); document.body.appendChild(_ce_div); _ce_div.contentEditable = 'true';").unwrap();
    assert!(bool_eval(&rt, "_ce_div.isContentEditable === true"));
}

// BUG-344: `contenteditable="plaintext-only"` must reflect verbatim
// through the `contentEditable` attribute getter, not collapse to `inherit`.
#[test]
fn contenteditable_property_plaintext_only_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _ce_div = document.createElement('div'); document.body.appendChild(_ce_div); _ce_div.setAttribute('contenteditable', 'plaintext-only');").unwrap();
    assert!(bool_eval(&rt, "_ce_div.contentEditable === 'plaintext-only'"));
}

// BUG-344: assigning `contentEditable = 'plaintext-only'` must not be
// silently downgraded to attribute removal (`inherit`), and must make
// `isContentEditable` true.
#[test]
fn contenteditable_set_property_plaintext_only() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _ce_div = document.createElement('div'); document.body.appendChild(_ce_div); _ce_div.contentEditable = 'plaintext-only';").unwrap();
    assert!(bool_eval(&rt, "_ce_div.contentEditable === 'plaintext-only'"));
    assert!(bool_eval(&rt, "_ce_div.isContentEditable === true"));
}

// ── document.designMode (HTML LS §6.6.3, BUG-353) ──────────────────────

#[test]
fn design_mode_defaults_to_off() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.designMode === 'off'"));
}

#[test]
fn design_mode_on_makes_plain_element_editable() {
    let rt = v8_runtime_with_dom(make_doc());
    // A freshly created div carries no `contenteditable` attribute at all.
    rt.eval("var _dm_div = document.createElement('div'); document.body.appendChild(_dm_div);").unwrap();
    assert!(bool_eval(&rt, "_dm_div.isContentEditable === false"));
    rt.eval("document.designMode = 'on';").unwrap();
    assert!(bool_eval(&rt, "document.designMode === 'on'"));
    assert!(bool_eval(&rt, "_dm_div.isContentEditable === true"));
    rt.eval("document.designMode = 'off';").unwrap();
    assert!(bool_eval(&rt, "_dm_div.isContentEditable === false"));
}

#[test]
fn design_mode_setter_ignores_invalid_values() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.designMode = 'on';").unwrap();
    rt.eval("document.designMode = 'nonsense';").unwrap();
    // Spec: an unrecognized value leaves the current mode untouched.
    assert!(bool_eval(&rt, "document.designMode === 'on'"));
}

#[test]
fn contenteditable_insert_text_at_caret() {
    let (arc, div, text) = make_contenteditable_doc();
    let text_idx = text.index();
    let div_idx = div.index();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    let result = rt.eval(&format!(
        "_lumen_handle_contenteditable_key('insertText',' World',{})",
        div_idx
    )).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not a text node"),
    };
    assert_eq!(content, "Hello World");
}

#[test]
fn contenteditable_delete_backward_one_char() {
    let (arc, div, text) = make_contenteditable_doc();
    let text_idx = text.index();
    let div_idx = div.index();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    rt.eval(&format!(
        "_lumen_handle_contenteditable_key('deleteContentBackward',null,{})",
        div_idx
    )).unwrap();
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not a text node"),
    };
    assert_eq!(content, "Hell");
}

#[test]
fn contenteditable_delete_forward_one_char() {
    let (arc, div, text) = make_contenteditable_doc();
    let text_idx = text.index();
    let div_idx = div.index();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 0 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 0 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    rt.eval(&format!(
        "_lumen_handle_contenteditable_key('deleteContentForward',null,{})",
        div_idx
    )).unwrap();
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not a text node"),
    };
    assert_eq!(content, "ello");
}

#[test]
fn contenteditable_beforeinput_cancellable() {
    let (arc, div, text) = make_contenteditable_doc();
    let text_idx = text.index();
    let div_idx = div.index();
    {
        let mut doc = arc.lock().unwrap();
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text, offset: 5 }),
            focus:  Some(lumen_dom::DomPosition { container: text, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    // Attach a beforeinput handler that cancels the event
    rt.eval(&format!(
        "_lumen_make_element({}).addEventListener('beforeinput', function(e) {{ e.preventDefault(); }});",
        div_idx
    )).unwrap();
    let result = rt.eval(&format!(
        "_lumen_handle_contenteditable_key('insertText','X',{})",
        div_idx
    )).unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false), "cancelled beforeinput must return false");
    // Text must not be mutated
    let doc = arc.lock().unwrap();
    let content = match &doc.get(NodeId::from_index(text_idx)).data {
        NodeData::Text(s) => s.clone(),
        _ => panic!("not a text node"),
    };
    assert_eq!(content, "Hello", "DOM must not change when beforeinput is cancelled");
}

#[test]
fn contenteditable_input_event_fires() {
    let (arc, div, _) = make_contenteditable_doc();
    let div_idx = div.index();
    {
        let mut doc = arc.lock().unwrap();
        let text_nid = doc.get(div).children[0];
        doc.set_selection(lumen_dom::Selection {
            anchor: Some(lumen_dom::DomPosition { container: text_nid, offset: 5 }),
            focus:  Some(lumen_dom::DomPosition { container: text_nid, offset: 5 }),
        });
    }
    let rt = v8_runtime_with_dom(arc.clone());
    rt.eval("var _ce_fired = false;").unwrap();
    rt.eval(&format!(
        "_lumen_make_element({}).addEventListener('input', function() {{ _ce_fired = true; }});",
        div_idx
    )).unwrap();
    rt.eval(&format!(
        "_lumen_handle_contenteditable_key('insertText','Z',{})",
        div_idx
    )).unwrap();
    assert!(bool_eval(&rt, "_ce_fired"), "input event must fire after mutation");
}
