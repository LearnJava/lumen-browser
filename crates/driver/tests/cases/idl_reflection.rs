//! BUG-383 — IDL reflection of HTML content attributes, form-control
//! collections and the activation methods (`click()`, `select()`,
//! `form.reset()`).
//!
//! The live-element factory used to hand-list the handful of reflected
//! attributes earlier fixes happened to need (`value`, `name`, `type`,
//! `checked`, `src`), so `a.href`, `input.disabled`, `select.selectedIndex`,
//! `textarea.rows` and forty more read back `undefined`, `form.elements` was a
//! plain `Array` with no `namedItem`, and no element had `click()` at all.
//!
//! These assertions run on the **default (V8) engine** through
//! `InProcessSession`. That matters twice over: the in-crate `dom::tests` run on
//! QuickJS, where a missing attribute comes back as `undefined` instead of
//! `null` (BUG-442) — so a presence check written the wrong way passes there and
//! fails here — and the reflection properties live on the interface prototypes,
//! which only the real element factory wires up.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body>
<form id="f" action="/act" method="post" novalidate>
  <input id="i" type="text" value="v" name="n" maxlength="5" placeholder="p" required disabled readonly>
  <input id="c" type="checkbox" checked>
  <input id="r1" type="radio" name="g">
  <input id="r2" type="radio" name="g">
  <select id="s" name="sel"><option value="a">A</option><option value="b" selected>B</option></select>
  <textarea id="t" rows="4" cols="7">hello</textarea>
  <button id="b">Go</button>
  <label id="l" for="i">Label</label>
</form>
<a id="a" href="sub/page.html">link</a>
<div id="d">plain</div>
</body></html>"#;

/// Strip the JSON quoting `BrowserSession::eval` applies to string results.
fn unquote(s: &str) -> String {
    s.trim_matches('"').to_owned()
}

fn session() -> InProcessSession {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    s
}

/// `eval` the expression and return it as a plain string.
fn ev(s: &mut InProcessSession, expr: &str) -> String {
    unquote(&s.eval(expr).unwrap_or_else(|e| panic!("eval {expr}: {e:?}")))
}

#[test]
fn reflected_attributes_are_not_undefined() {
    let mut s = session();
    // The exact list the bug report enumerated as `undefined`.
    for (expr, expected) in [
        ("typeof document.getElementById('a').href", "string"),
        ("document.getElementById('i').disabled", "true"),
        ("document.getElementById('i').readOnly", "true"),
        ("document.getElementById('i').required", "true"),
        ("document.getElementById('i').maxLength", "5"),
        ("document.getElementById('i').placeholder", "p"),
        ("document.getElementById('i').defaultValue", "v"),
        ("document.getElementById('i').name", "n"),
        ("document.getElementById('c').defaultChecked", "true"),
        ("document.getElementById('c').indeterminate", "false"),
        ("document.getElementById('s').selectedIndex", "1"),
        ("document.getElementById('s').options.length", "2"),
        ("document.getElementById('s').length", "2"),
        ("document.getElementById('t').rows", "4"),
        ("document.getElementById('t').cols", "7"),
        ("document.getElementById('f').noValidate", "true"),
        ("document.getElementById('f').length", "7"),
        ("document.getElementById('l').htmlFor", "i"),
    ] {
        assert_eq!(ev(&mut s, expr), expected, "{expr}");
    }
}

/// Reflection is per-interface, not per-element: a `<div>` must not sprout
/// `src`/`type`/`disabled` just because some other element has them. Before the
/// fix `src`, `type` and `name` were own properties of *every* wrapper.
#[test]
fn reflection_does_not_leak_onto_unrelated_elements() {
    let mut s = session();
    for expr in [
        "typeof document.getElementById('d').src",
        "typeof document.getElementById('d').disabled",
        "typeof document.getElementById('d').maxLength",
    ] {
        assert_eq!(ev(&mut s, expr), "undefined", "{expr}");
    }
    // …while a global attribute is present on every element.
    assert_eq!(ev(&mut s, "typeof document.getElementById('d').title"), "string");
}

/// `type` has a per-interface default: `text` for `<input>`, `submit` for
/// `<button>`, fixed strings for `<select>`/`<textarea>`. The old shared getter
/// answered `text` for all four.
#[test]
fn type_defaults_are_per_interface() {
    let mut s = session();
    assert_eq!(ev(&mut s, "document.getElementById('i').type"), "text");
    assert_eq!(ev(&mut s, "document.getElementById('b').type"), "submit");
    assert_eq!(ev(&mut s, "document.getElementById('t').type"), "textarea");
    assert_eq!(ev(&mut s, "document.getElementById('s').type"), "select-one");
}

/// A `url`-kind attribute reflects as an absolute URL resolved against the
/// document base URL, not as the raw attribute text (HTML LS §2.6.1).
#[test]
fn url_attributes_reflect_absolute() {
    let mut s = session();
    let href = ev(&mut s, "document.getElementById('a').href");
    assert!(
        href.ends_with("sub/page.html") && href.len() > "sub/page.html".len(),
        "a.href should be resolved against the document base URL, got {href:?}"
    );
    // An absent URL attribute reflects as '' — never as the document URL.
    assert_eq!(ev(&mut s, "document.createElement('a').href"), "");
}

#[test]
fn form_elements_is_a_form_controls_collection() {
    let mut s = session();
    assert_eq!(
        ev(&mut s, "Array.isArray(document.getElementById('f').elements)"),
        "false",
        "form.elements was a plain Array, which silently lacks named access"
    );
    assert_eq!(
        ev(&mut s, "document.getElementById('f').elements instanceof HTMLFormControlsCollection"),
        "true"
    );
    assert_eq!(
        ev(&mut s, "Object.prototype.toString.call(document.getElementById('f').elements)"),
        "[object HTMLFormControlsCollection]"
    );
    assert_eq!(
        ev(&mut s, "document.getElementById('f').elements.namedItem('n').id"),
        "i"
    );
    assert_eq!(ev(&mut s, "document.getElementById('f').elements[0].id"), "i");
    assert_eq!(
        ev(&mut s, "Array.from(document.getElementById('f').elements).length"),
        "7"
    );
}

/// `click()` existed nowhere — not on the instance, not in the prototype chain.
/// It must dispatch a cancelable `click` and then run the activation behaviour.
#[test]
fn click_dispatches_and_runs_activation_behavior() {
    let mut s = session();
    assert_eq!(ev(&mut s, "typeof HTMLElement.prototype.click"), "function");

    let out = ev(
        &mut s,
        "(function () {
            var c = document.getElementById('c');
            var seen = [];
            c.addEventListener('click',  function () { seen.push('click:' + c.checked); });
            c.addEventListener('change', function () { seen.push('change'); });
            c.click();
            return seen.join(',') + '|' + c.checked;
        })()",
    );
    // Pre-click activation flips checkedness *before* the event, so a listener
    // observes the new state; `change` follows only if nobody cancelled.
    assert_eq!(out, "click:false,change|false");
}

/// A cancelled `click` must undo the pre-click checkbox flip.
#[test]
fn cancelled_click_restores_checkedness() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var c = document.getElementById('c');
            c.addEventListener('click', function (e) { e.preventDefault(); });
            c.click();
            return String(c.checked);
        })()",
    );
    assert_eq!(out, "true", "a prevented click must leave `checked` untouched");
}

#[test]
fn radio_click_selects_within_the_group() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var r1 = document.getElementById('r1'), r2 = document.getElementById('r2');
            r1.click();
            var afterFirst = r1.checked + ',' + r2.checked;
            r2.click();
            return afterFirst + '|' + r1.checked + ',' + r2.checked;
        })()",
    );
    assert_eq!(out, "true,false|false,true");
}

#[test]
fn select_options_and_value_agree() {
    let mut s = session();
    assert_eq!(ev(&mut s, "document.getElementById('s').value"), "b");
    assert_eq!(ev(&mut s, "document.getElementById('s').options[0].value"), "a");
    // An <option> with no `value` attribute takes its value from its text.
    assert_eq!(ev(&mut s, "document.getElementById('s').options[1].text"), "B");
    assert_eq!(ev(&mut s, "document.getElementById('s').options[1].index"), "1");

    let out = ev(
        &mut s,
        "(function () {
            var s = document.getElementById('s');
            s.selectedIndex = 0;
            var afterIndex = s.value;
            s.value = 'b';
            return afterIndex + '|' + s.selectedIndex;
        })()",
    );
    assert_eq!(out, "a|1");
}

#[test]
fn text_selection_api_works_on_text_inputs() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var i = document.getElementById('i');
            i.value = 'abcdef';
            i.select();
            var all = i.selectionStart + '-' + i.selectionEnd;
            i.setSelectionRange(1, 3);
            return all + '|' + i.selectionStart + '-' + i.selectionEnd;
        })()",
    );
    assert_eq!(out, "0-6|1-3");
    // The selection API does not apply to a checkbox (HTML LS §4.10.5.4).
    assert_eq!(ev(&mut s, "document.getElementById('c').selectionStart"), "null");
}

#[test]
fn form_reset_restores_defaults() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var f = document.getElementById('f');
            var i = document.getElementById('i');
            var c = document.getElementById('c');
            i.value = 'dirty';
            c.checked = false;
            f.reset();
            return i.value + '|' + c.checked;
        })()",
    );
    assert_eq!(out, "v|true");
}

/// BUG-444 — a **native** click must not destroy the control's default
/// checkedness.
///
/// This is the one path BUG-383's `_lumen_default_checked` workaround could
/// never cover: the shell/driver flipped the `checked` content attribute
/// straight from Rust, so it never passed the JS-side snapshot. Since
/// checkedness *was* that attribute, the first native click erased the default
/// — `defaultChecked` flipped with it and `form.reset()` had nothing to
/// restore. The scripted path above passed all along, which is exactly why
/// this needs its own test.
#[test]
fn native_click_keeps_default_checkedness() {
    let mut s = session();
    assert_eq!(ev(&mut s, "document.getElementById('c').checked"), "true");

    s.click(&lumen_driver::Target::Selector("#c".into())).expect("native click");

    // Current checkedness follows the click…
    assert_eq!(ev(&mut s, "document.getElementById('c').checked"), "false");
    // …while the default it was born with survives it.
    assert_eq!(
        ev(&mut s, "document.getElementById('c').defaultChecked"),
        "true",
        "a native click flipped `defaultChecked` — checkedness is still stored \
         in the `checked` content attribute"
    );
    // …so `form.reset()` has something to restore.
    assert_eq!(
        ev(
            &mut s,
            "(function () {
                document.getElementById('f').reset();
                return String(document.getElementById('c').checked);
            })()"
        ),
        "true",
        "form.reset() could not restore the default a native click destroyed"
    );
}

/// The runtime checkedness a native click writes must be the *same* storage a
/// script reads and writes — one control cannot have two answers depending on
/// who asks.
#[test]
fn native_and_scripted_checkedness_share_one_storage() {
    let mut s = session();
    // Script unticks, native click re-ticks.
    ev(&mut s, "document.getElementById('c').checked = false");
    s.click(&lumen_driver::Target::Selector("#c".into())).expect("native click");
    assert_eq!(ev(&mut s, "document.getElementById('c').checked"), "true");

    // `defaultChecked` is writable and independent of the current state
    // (HTML LS §4.10.5.5 — it reflects the content attribute).
    ev(&mut s, "document.getElementById('c').defaultChecked = false");
    assert_eq!(ev(&mut s, "document.getElementById('c').checked"), "true");
    assert_eq!(ev(&mut s, "document.getElementById('c').getAttribute('checked')"), "null");
}

/// `label.control` / `control.labels` / `control.form` — the association graph
/// the bug listed as entirely missing.
#[test]
fn control_associations_resolve() {
    let mut s = session();
    assert_eq!(ev(&mut s, "document.getElementById('l').control.id"), "i");
    assert_eq!(ev(&mut s, "document.getElementById('i').labels.length"), "1");
    assert_eq!(ev(&mut s, "document.getElementById('i').form.id"), "f");
}

/// `form.submit()` / `requestSubmit()` exist and `requestSubmit()` fires a
/// cancelable `submit` a page can take over (HTML LS §4.10.21.4 step 11), while
/// `submit()` is defined to skip it.
#[test]
fn form_submit_methods_exist_and_requestsubmit_fires_the_event() {
    let mut s = session();
    assert_eq!(ev(&mut s, "typeof document.getElementById('f').submit"), "function");
    assert_eq!(ev(&mut s, "typeof document.getElementById('f').reset"), "function");
    let out = ev(
        &mut s,
        "(function () {
            var f = document.getElementById('f');
            var n = 0;
            f.addEventListener('submit', function (e) { n++; e.preventDefault(); });
            f.requestSubmit();
            var afterRequest = n;
            f.submit();
            return afterRequest + '|' + n;
        })()",
    );
    assert_eq!(out, "1|1", "submit() must not fire a `submit` event");
}

/// `select.remove(index)` removes an option; `select.remove()` with no argument
/// is still `ChildNode.remove()`. The bug report singled this pair out — the
/// element wrapper's own `remove` shadows anything put on the prototype.
#[test]
fn select_remove_takes_an_index() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var sel = document.getElementById('s');
            sel.remove(0);
            var afterIndexed = sel.options.length + ',' + sel.options[0].value;
            sel.remove();
            return afterIndexed + '|' + (document.getElementById('s') === null);
        })()",
    );
    assert_eq!(out, "1,b|true");
}

/// `select.add()` inserts an option, optionally before an index or an option.
#[test]
fn select_add_inserts_options() {
    let mut s = session();
    let out = ev(
        &mut s,
        "(function () {
            var sel = document.getElementById('s');
            sel.add(new Option('C', 'c'));
            var appended = sel.options.length + ',' + sel.options[2].value;
            sel.add(new Option('Z', 'z'), 0);
            return appended + '|' + sel.options[0].value + ',' + sel.options.length;
        })()",
    );
    assert_eq!(out, "3,c|z,4");
}
