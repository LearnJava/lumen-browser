//! BUG-951 — regression coverage: `<label>.focus()` must forward focus to
//! the label's associated control (`for` attribute, else the first labelable
//! descendant) instead of being a no-op, unless the label carries its own
//! `tabindex`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// `<label for=target-input>` — resolves through `getElementById`.
#[test]
fn label_focus_forwards_via_for_attribute() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var input = document.createElement('input');
                input.id = 'target-input';
                document.body.appendChild(input);
                var label = document.createElement('label');
                label.setAttribute('for', 'target-input');
                document.body.appendChild(label);
                label.focus();
                return document.activeElement === input;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `<label><input>…</label>` — no `for`, resolves to the first labelable
/// descendant.
#[test]
fn label_focus_forwards_to_labelable_descendant() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var label = document.createElement('label');
                var input = document.createElement('input');
                label.appendChild(input);
                document.body.appendChild(label);
                label.focus();
                return document.activeElement === input;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A label with its own `tabindex` focuses itself, not its control — the
/// general tabindex branch already covers this, forwarding must not override
/// it.
#[test]
fn label_with_explicit_tabindex_focuses_itself() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var label = document.createElement('label');
                label.setAttribute('tabindex', '0');
                var input = document.createElement('input');
                label.appendChild(input);
                document.body.appendChild(label);
                label.focus();
                return document.activeElement === label;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// A label with no resolvable control and no `tabindex` stays a no-op —
/// forwarding must not crash or focus the label itself.
#[test]
fn label_with_no_control_and_no_tabindex_is_noop() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var label = document.createElement('label');
                document.body.appendChild(label);
                label.focus();
                return document.activeElement === label;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

/// `label.control` (HTML LS §4.10.4) must agree with what `focus()` forwards
/// to — same resolution, shared helper.
#[test]
fn label_control_getter_matches_focus_target() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var input = document.createElement('input');
                input.id = 'ctrl';
                document.body.appendChild(input);
                var label = document.createElement('label');
                label.setAttribute('for', 'ctrl');
                document.body.appendChild(label);
                return label.control === input;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-621 — `inert-label-focus.html` subtest 1: an `inert` label is not
/// itself focusable, but `focus()` still forwards to its (non-inert)
/// control. The inert check belongs to the focus target, not the label.
#[test]
fn inert_label_focus_still_forwards_to_control() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var input = document.createElement('input');
                input.id = 'submit621';
                input.type = 'submit';
                document.body.appendChild(input);
                var label = document.createElement('label');
                label.setAttribute('for', 'submit621');
                label.setAttribute('inert', '');
                document.body.appendChild(label);
                label.focus();
                return document.activeElement === input;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-621 — same forwarding when the inertness comes from an ancestor of
/// the label rather than the label itself.
#[test]
fn label_inside_inert_subtree_forwards_to_control_outside() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var input = document.createElement('input');
                input.id = 'outside621';
                document.body.appendChild(input);
                var wrap = document.createElement('div');
                wrap.setAttribute('inert', '');
                var label = document.createElement('label');
                label.setAttribute('for', 'outside621');
                wrap.appendChild(label);
                document.body.appendChild(wrap);
                label.focus();
                return document.activeElement === input;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-621 — `inert-label-focus.html` subtest 3: a label whose control sits
/// in an inert subtree must not move focus anywhere.
#[test]
fn label_for_control_in_inert_subtree_has_no_effect() {
    let rt = rt_with_dom();
    let r = rt
        .eval(
            "(function() {
                var text = document.createElement('input');
                document.body.appendChild(text);
                var wrap = document.createElement('div');
                wrap.setAttribute('inert', '');
                var input = document.createElement('input');
                input.id = 'inside621';
                wrap.appendChild(input);
                document.body.appendChild(wrap);
                var label = document.createElement('label');
                label.setAttribute('for', 'inside621');
                document.body.appendChild(label);
                text.focus();
                label.focus();
                return document.activeElement === text;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
