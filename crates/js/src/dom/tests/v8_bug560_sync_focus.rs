//! BUG-560 — regression coverage: `element.focus()` must make `:focus`/
//! `:focus-within` observable in the SAME synchronous script turn, both
//! through `getComputedStyle()` (the same-tick style flush,
//! `crate::v8_runtime::style_flush`) and through `matches()`/
//! `querySelectorAll()` (`crate::v8_runtime::install_node_lookup`'s selector
//! natives). Pre-fix, both stayed on the pre-focus state until the shell's
//! next pump drained `_lumen_request_focus` — which a purely synchronous
//! script (testharness.js's `test()` callbacks among them) never reaches.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::v8_bug493_sync_flush::v8_runtime_with_flush`] with a
/// `:focus`/`:focus-within` rule on `#main` instead of an unconditional one.
fn v8_runtime_with_focus_rule(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "#main { color: rgb(0, 0, 0); } \
         #main:focus { color: rgb(0, 128, 0); } \
         #main:focus-within { background-color: rgb(0, 0, 128); }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

/// Pre-fix this returned the unfocused `rgb(0, 0, 0)` — the same-tick flush
/// never installed the just-requested focus target before laying out.
#[test]
fn get_computed_style_sees_same_tick_focus_call() {
    let rt = v8_runtime_with_focus_rule(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.setAttribute('tabindex', '0');
                el.focus();
                return getComputedStyle(el).color;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rgb(0, 128, 0)".to_string()));
}

/// `:focus-within` on an ancestor must see a same-tick `.focus()` on a
/// descendant, the exact shape `focus-within-009.html` (this bug's WPT
/// origin) exercises.
#[test]
fn get_computed_style_sees_same_tick_focus_within() {
    let rt = v8_runtime_with_focus_rule(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                var child = document.createElement('span');
                child.setAttribute('tabindex', '0');
                el.appendChild(child);
                child.focus();
                return getComputedStyle(el).backgroundColor;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rgb(0, 0, 128)".to_string()));
}

/// `matches()`/`querySelectorAll()` resolve dynamic pseudo-classes through an
/// entirely different native path (`_lumen_node_matches_selector`/
/// `_lumen_query_selector_all`) than `getComputedStyle` — pre-fix, that path
/// never installed the interactive-state thread-local at all, so `:focus`
/// matched nothing, ever, even well after the shell's pump had applied it.
#[test]
fn matches_sees_same_tick_focus_call() {
    let rt = v8_runtime_with_focus_rule(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.setAttribute('tabindex', '0');
                el.focus();
                return el.matches(':focus');
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Sibling of [`matches_sees_same_tick_focus_call`] for `querySelectorAll`.
#[test]
fn query_selector_all_sees_same_tick_focus_call() {
    let rt = v8_runtime_with_focus_rule(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.setAttribute('tabindex', '0');
                el.focus();
                return document.querySelectorAll(':focus').length;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

/// `.blur()` must clear the same-tick focus state symmetrically.
#[test]
fn matches_stops_seeing_focus_after_same_tick_blur() {
    let rt = v8_runtime_with_focus_rule(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.setAttribute('tabindex', '0');
                el.focus();
                el.blur();
                return el.matches(':focus');
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}
