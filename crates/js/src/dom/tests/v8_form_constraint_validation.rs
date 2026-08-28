//! Тесты `v8_form_constraint_validation`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

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

/// Helper: build a document with a <form> containing one <input>.
fn make_form_doc() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html  = doc.create_element(QualName::html("html"));
    let body  = doc.create_element(QualName::html("body"));
    let form  = doc.create_element(QualName::html("form"));
    let input = doc.create_element(QualName::html("input"));
    fn set_attr(doc: &mut Document, nid: lumen_dom::NodeId, name: &str, val: &str) {
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(nid).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html(name), value: val.into() });
        }
    }
    set_attr(&mut doc, form,  "id",   "f");
    set_attr(&mut doc, input, "id",   "inp");
    set_attr(&mut doc, input, "type", "text");
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    doc.append_child(body, form);
    doc.append_child(form, input);
    Arc::new(Mutex::new(doc))
}

#[test]
fn validity_state_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof ValidityState === 'function'"));
}

#[test]
fn input_has_validity_property() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "var inp = document.getElementById('inp'); inp.validity instanceof ValidityState"));
}

#[test]
fn validity_valid_by_default() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.valid === true"));
}

#[test]
fn validity_value_missing_required_empty() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
    assert!(bool_eval(&rt,
        "var v = document.getElementById('inp').validity; \
                 v.valueMissing === true && v.valid === false"));
}

#[test]
fn validity_value_missing_clears_when_filled() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('required', ''); inp.value = 'hello'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.valueMissing === false"));
}

#[test]
fn validity_type_mismatch_email() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'email'); inp.value = 'notanemail'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.typeMismatch === true"));
}

#[test]
fn validity_type_mismatch_email_valid() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'email'); inp.value = 'user@example.com'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.typeMismatch === false && \
                 document.getElementById('inp').validity.valid === true"));
}

#[test]
fn validity_type_mismatch_url() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'url'); inp.value = 'not-a-url'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.typeMismatch === true"));
}

#[test]
fn validity_pattern_mismatch() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('pattern', '[0-9]+'); inp.value = 'abc'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.patternMismatch === true"));
}

#[test]
fn validity_pattern_match_ok() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('pattern', '[0-9]+'); inp.value = '42'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.patternMismatch === false"));
}

#[test]
fn validity_too_long() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('maxlength', '3'); inp.value = 'hello'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.tooLong === true"));
}

#[test]
fn validity_too_short() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('minlength', '5'); inp.value = 'hi'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.tooShort === true"));
}

#[test]
fn validity_range_underflow() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('min', '10'); inp.value = '5'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.rangeUnderflow === true"));
}

#[test]
fn validity_range_overflow() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('max', '10'); inp.value = '20'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.rangeOverflow === true"));
}

#[test]
fn validity_step_mismatch() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setAttribute('type', 'number'); inp.setAttribute('step', '5'); inp.value = '7'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.stepMismatch === true"));
}

#[test]
fn set_custom_validity_sets_custom_error() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setCustomValidity('bad input')").unwrap();
    assert!(bool_eval(&rt,
        "var v = document.getElementById('inp').validity; \
                 v.customError === true && v.valid === false"));
}

#[test]
fn set_custom_validity_empty_clears_error() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("var inp = document.getElementById('inp'); inp.setCustomValidity('err'); inp.setCustomValidity('')").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validity.customError === false"));
}

#[test]
fn will_validate_input_true() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('inp').willValidate === true"));
}

#[test]
fn will_validate_hidden_false() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setAttribute('type', 'hidden')").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').willValidate === false"));
}

#[test]
fn check_validity_valid_returns_true() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('inp').checkValidity() === true"));
}

#[test]
fn check_validity_fires_invalid_event() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval(
        "var inp = document.getElementById('inp'); \
                 inp.setAttribute('required', ''); \
                 var fired = false; \
                 inp.addEventListener('invalid', function() { fired = true; });"
    ).unwrap();
    rt.eval("document.getElementById('inp').checkValidity()").unwrap();
    assert!(bool_eval(&rt, "fired === true"));
}

#[test]
fn report_validity_delegates_to_check_validity() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').reportValidity() === false"));
}

#[test]
fn form_elements_collection() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "var form = document.getElementById('f'); \
                 form.elements.length >= 1"));
}

#[test]
fn form_no_validate_attr() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('f').noValidate = true").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('f').hasAttribute('novalidate')"));
}

#[test]
fn validation_message_custom() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setCustomValidity('Must be a number')").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validationMessage === 'Must be a number'"));
}

#[test]
fn validation_message_value_missing() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').setAttribute('required', '')").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').validationMessage.length > 0"));
}

#[test]
fn input_value_get_set() {
    let rt = v8_runtime_with_dom(make_form_doc());
    rt.eval("document.getElementById('inp').value = 'hello world'").unwrap();
    assert!(bool_eval(&rt,
        "document.getElementById('inp').value === 'hello world'"));
}

// BUG-441: `el.value = …` used to live in a JS-side map only, so the
// field kept rendering (and submitting) its old text. The assignment
// must land in the document's runtime value store — what layout paints
// and `collect_dom_form_fields` collects — while the `value` content
// attribute keeps holding the *default* value.
#[test]
fn input_value_assignment_reaches_document() {
    let doc = make_form_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval("document.getElementById('inp').setAttribute('value', 'default')")
        .unwrap();
    rt.eval("document.getElementById('inp').value = 'ZZ'").unwrap();

    let nid = test_nid(&rt, "inp");
    let d = doc.lock().unwrap();
    assert_eq!(d.control_value(nid), "ZZ");
    assert_eq!(d.get(nid).get_attr("value"), Some("default"));
}

// The same store is what `form.reset()` drops, restoring the default.
#[test]
fn form_reset_drops_document_side_value() {
    let doc = make_form_doc();
    let rt = v8_runtime_with_dom(Arc::clone(&doc));
    rt.eval("document.getElementById('inp').setAttribute('value', 'default')")
        .unwrap();
    rt.eval("document.getElementById('inp').value = 'typed'").unwrap();
    rt.eval("document.getElementById('f').reset()").unwrap();

    let nid = test_nid(&rt, "inp");
    assert!(bool_eval(&rt, "document.getElementById('inp').value === 'default'"));
    let d = doc.lock().unwrap();
    assert_eq!(d.dirty_value(nid), None);
    assert_eq!(d.control_value(nid), "default");
}

/// `NodeId` of the element with `id`, read back through the shim.
fn test_nid(rt: &V8JsRuntime, id: &str) -> lumen_dom::NodeId {
    let v = rt
        .eval(&format!("document.getElementById('{id}').__nid__"))
        .unwrap();
    match v {
        lumen_core::JsValue::Number(n) => lumen_dom::NodeId::from_index(n as usize),
        other => panic!("expected a numeric __nid__, got {other:?}"),
    }
}

#[test]
fn input_type_reflected() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "document.getElementById('inp').type === 'text'"));
}

// BUG-436: the shell performs the text-insertion default action itself
// (DOM `value` attribute) and then calls `_lumen_set_field_value` so the
// shim's value shadow agrees. Without the sync a field the page had ever
// assigned through script would keep reporting the stale script value to
// the `input` listener that fires right after.
#[test]
fn set_field_value_syncs_value_shadow() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   var inp = document.getElementById('inp'); \
                   inp.value = 'stale'; \
                   _lumen_set_field_value(inp.__nid__, 'abc'); \
                   return inp.value === 'abc'; \
                 })()"));
}

// ── HTMLInputElement.showPicker() tests ────────────────────────────────────

#[test]
fn show_picker_exists_on_input() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "typeof document.getElementById('inp').showPicker === 'function'"));
}

#[test]
fn show_picker_throws_for_text_type() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   var inp = document.getElementById('inp'); \
                   try { inp.showPicker(); return false; } \
                   catch(e) { return e.name === 'NotSupportedError'; } \
                 })()"));
}

#[test]
fn show_picker_fires_click_for_color() {
    let rt = v8_runtime_with_dom(make_form_doc());
    assert!(bool_eval(&rt,
        "(function() { \
                   var inp = document.getElementById('inp'); \
                   inp.setAttribute('type', 'color'); \
                   var clicked = false; \
                   inp.addEventListener('click', function() { clicked = true; }); \
                   try { inp.showPicker(); } catch(e) {} \
                   return clicked; \
                 })()"));
}
