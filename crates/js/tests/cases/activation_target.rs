//! Activation target (DOM Standard §2.9) through the real V8 runtime and JS
//! shim.
//!
//! Regression cover for [BUG-837](../../../../bugs/BUG-837-OPEN.md): both
//! activation call sites (`HTMLElement.prototype.click` and the element
//! wrapper's `dispatchEvent`) used to hand `_lumen_run_activation_behavior` the
//! *clicked* node, whose tag the behaviour table is keyed by. A `<span>` has no
//! row there, so the standard `<label><span>…</span></label>`,
//! `<a><img></a>` and `<button><svg>…</svg></button>` markup dispatched the
//! `click` event and then did nothing at all.
//!
//! The spec computes an activation target before dispatch — the nearest
//! inclusive ancestor on the event path that has an activation behaviour — and
//! activates *that*. The tests below assert the walk in both directions: it
//! must reach the ancestor, and it must not reach past a node that has no
//! behaviour of its own but is interactive content (HTML LS §4.10.20), nor
//! activate a disabled control.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

/// A runtime whose document lives at a URL with a path, so a fragment target is
/// visibly same-document (the anchor cases reuse the BUG-833 machinery).
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
        Ok(other) => panic!("expected string from script, got {other:?}"),
        Err(e) => panic!("eval error: {e}"),
    }
}

/// `<label><span></span><input type=checkbox></label>` — the control is the
/// label's first labelable descendant, so no `for`/`id` lookup (and therefore
/// no connection to the document) is needed to exercise the walk.
const LABEL_SPAN: &str = r#"
var _log = [];
var _label = document.createElement('label');
var _span = document.createElement('span');
var _cb = document.createElement('input');
_cb.type = 'checkbox';
_label.appendChild(_span);
_label.appendChild(_cb);
_cb.addEventListener('change', function () { _log.push('change'); });
_cb.addEventListener('click', function () { _log.push('cb-click'); });
TARGET.click();
_log.join('|') + ' checked=' + _cb.checked
"#;

#[test]
fn click_on_a_span_inside_a_label_activates_the_control() {
    let rt = make_rt();
    let out = str_eval(&rt, &LABEL_SPAN.replace("TARGET", "_span"));
    assert_eq!(
        out, "cb-click|change checked=true",
        "the activation target of a click on the <span> is the enclosing <label>"
    );
}

#[test]
fn click_on_the_label_itself_still_activates_the_control() {
    // The control case of the same measurement: this half already worked, and a
    // fix to the ancestor walk must not disturb it.
    let rt = make_rt();
    let out = str_eval(&rt, &LABEL_SPAN.replace("TARGET", "_label"));
    assert_eq!(out, "cb-click|change checked=true");
}

#[test]
fn dispatch_event_of_a_click_uses_the_activation_target_too() {
    // The second call site (BUG-837 names both): an untrusted `click` sent
    // through `dispatchEvent` runs the same activation `click()` does.
    let rt = make_rt();
    let out = str_eval(
        &rt,
        &LABEL_SPAN.replace(
            "TARGET.click()",
            "_span.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))",
        ),
    );
    assert_eq!(out, "cb-click|change checked=true");
}

/// `<a href><textarea>` vs `<a href><span>`: the anchor branch of the table is
/// reached through a plain descendant and must *not* be reached through
/// interactive content that has no activation behaviour of its own.
const ANCHOR_DESCENDANT: &str = r#"
var _a = document.createElement('a');
_a.setAttribute('href', '#sec');
var _inner = document.createElement('TAG');
_a.appendChild(_inner);
_inner.click();
location.hash
"#;

#[test]
fn click_on_a_descendant_of_an_anchor_navigates() {
    let rt = make_rt();
    let out = str_eval(&rt, &ANCHOR_DESCENDANT.replace("TAG", "span"));
    assert_eq!(out, "#sec", "a click on the <span> inside <a href> must follow the link");
    assert!(
        rt.take_navigate_request().is_none(),
        "a same-document fragment target must not ask the shell to load anything"
    );
}

#[test]
fn interactive_content_stops_the_walk() {
    // HTML LS §4.10.20 makes a <label> do nothing for clicks targeted at its
    // interactive-content descendants; the same reasoning keeps a <textarea>
    // from following the link it happens to sit inside.
    let rt = make_rt();
    let out = str_eval(&rt, &ANCHOR_DESCENDANT.replace("TAG", "textarea"));
    assert_eq!(out, "", "a click on interactive content must not activate an ancestor");
    assert!(rt.take_navigate_request().is_none());
}

/// `<form><input><button type=reset><span></span></button></form>` — the reset
/// is observable on the input's value, and the button carries the `disabled`
/// attribute in the second case.
const RESET_BUTTON: &str = r#"
var _form = document.createElement('form');
var _inp = document.createElement('input');
_inp.type = 'text';
_inp.value = 'typed';
var _btn = document.createElement('button');
_btn.type = 'reset';
DISABLE
var _span = document.createElement('span');
_btn.appendChild(_span);
_form.appendChild(_inp);
_form.appendChild(_btn);
_span.click();
'value=' + _inp.value
"#;

#[test]
fn click_on_a_span_inside_a_button_runs_the_buttons_behaviour() {
    let rt = make_rt();
    let out = str_eval(&rt, &RESET_BUTTON.replace("DISABLE", ""));
    assert_eq!(out, "value=", "the <button type=reset> must reset the form");
}

#[test]
fn a_disabled_ancestor_is_not_activated() {
    // `click()` checks `disabled` on the node it was called on, which no longer
    // covers the activation target — without the guard in
    // `_lumen_run_activation_behavior` the walk would activate a control the
    // user cannot press.
    let rt = make_rt();
    let out = str_eval(&rt, &RESET_BUTTON.replace("DISABLE", "_btn.setAttribute('disabled', '');"));
    assert_eq!(out, "value=typed", "a disabled control has no activation behaviour");
}

#[test]
fn a_click_with_no_activatable_ancestor_is_inert() {
    let rt = make_rt();
    let out = str_eval(
        &rt,
        r#"
var _log = [];
var _outer = document.createElement('div');
var _inner = document.createElement('span');
_outer.appendChild(_inner);
_inner.addEventListener('click', function () { _log.push('click'); });
_inner.click();
_log.join('|') + ' hash=' + location.hash
"#,
    );
    assert_eq!(
        out, "click hash=",
        "the event is still dispatched; there is simply nothing to activate"
    );
}

#[test]
fn a_cancelled_click_undoes_the_pre_click_flip_on_the_target() {
    // The pre-click activation belongs to the activation target as well, so a
    // handler that cancels the event must restore *its* state — reading the box
    // back through the ancestor the click never touched.
    let rt = make_rt();
    let out = str_eval(
        &rt,
        r#"
var _label = document.createElement('label');
var _span = document.createElement('span');
var _cb = document.createElement('input');
_cb.type = 'checkbox';
_label.appendChild(_span);
_label.appendChild(_cb);
_cb.addEventListener('click', function (e) { e.preventDefault(); });
_span.click();
'checked=' + _cb.checked
"#,
    );
    assert_eq!(
        out, "checked=false",
        "a cancelled click must leave the checkbox as it was"
    );
}
