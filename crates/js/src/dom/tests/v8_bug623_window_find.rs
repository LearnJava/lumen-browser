//! BUG-623 — `window.find()` (legacy text search) was missing entirely:
//! `typeof window.find === "undefined"`, so `inert-and-find*.html` and
//! `interactivity-inert-find.html` died on `is not a function`. It must search
//! the flat tree, skip inert subtrees and everything outside an open modal
//! dialog, and advance from the current selection.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "",
        None,
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

fn eval_bool(rt: &V8JsRuntime, src: &str) -> lumen_core::JsValue {
    rt.eval(src).unwrap()
}

/// `inert-and-find.html` basic case: one occurrence is found once, then the
/// search from the selected match finds nothing more.
#[test]
fn find_selects_match_and_advances_past_it() {
    let rt = rt_with_dom();
    let r = eval_bool(
        &rt,
        "(function() {
            var d = document.createElement('div');
            d.textContent = 'Find me please';
            document.body.appendChild(d);
            window.getSelection().removeAllRanges();
            var first = window.find('me');
            var sel = String(window.getSelection());
            var second = window.find('me');
            return typeof window.find === 'function' && first === true
                && sel === 'me' && second === false;
        })()",
    );
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `inert-and-find.html` "Basic use case": text under `inert` is not findable.
#[test]
fn find_skips_inert_subtree() {
    let rt = rt_with_dom();
    let r = eval_bool(
        &rt,
        "(function() {
            var d = document.createElement('div');
            d.setAttribute('inert', '');
            d.textContent = 'Do not find me please';
            document.body.appendChild(d);
            window.getSelection().removeAllRanges();
            return window.find('me');
        })()",
    );
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

/// `inert-and-find-flat-tree.html`: a modal dialog inside a shadow root —
/// both its shadow text and the light-DOM text slotted into it are findable.
#[test]
fn find_walks_flat_tree_into_modal_dialog_in_shadow_root() {
    let rt = rt_with_dom();
    let r = eval_bool(
        &rt,
        "(function() {
            var host = document.createElement('div');
            var slotted = document.createElement('div');
            slotted.textContent = 'slotted';
            host.appendChild(slotted);
            document.body.appendChild(host);
            var sr = host.attachShadow({ mode: 'open' });
            var dlg = document.createElement('dialog');
            var inner = document.createElement('div');
            inner.textContent = 'inside shadowroot';
            dlg.appendChild(inner);
            dlg.appendChild(document.createElement('slot'));
            sr.appendChild(dlg);
            dlg.showModal();
            var a = window.find('inside shadowroot');
            var b = window.find('slotted');
            return a === true && b === true;
        })()",
    );
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// An open modal dialog makes the rest of the document inert for find.
#[test]
fn find_ignores_text_outside_open_modal_dialog() {
    let rt = rt_with_dom();
    let r = eval_bool(
        &rt,
        "(function() {
            var outside = document.createElement('p');
            outside.textContent = 'outside text';
            document.body.appendChild(outside);
            var dlg = document.createElement('dialog');
            dlg.textContent = 'dialog text';
            document.body.appendChild(dlg);
            dlg.showModal();
            window.getSelection().removeAllRanges();
            var out = window.find('outside');
            var inside = window.find('dialog');
            return out === false && inside === true;
        })()",
    );
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// Case folding by default, `caseSensitive`, `backwards` and `wrapAround`.
#[test]
fn find_honours_case_backwards_and_wrap_flags() {
    let rt = rt_with_dom();
    let r = eval_bool(
        &rt,
        "(function() {
            var d = document.createElement('div');
            d.textContent = 'Alpha beta ALPHA';
            document.body.appendChild(d);
            var sel = window.getSelection();
            sel.removeAllRanges();
            var folded = window.find('alpha');
            sel.removeAllRanges();
            var strict = window.find('alpha', true);
            sel.removeAllRanges();
            var back = window.find('alpha', false, true);
            var backOff = sel.anchorOffset;
            var noWrap = window.find('beta') === false;
            var wrap = window.find('beta', false, false, true);
            return folded && !strict && back && backOff === 11 && noWrap && wrap;
        })()",
    );
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
