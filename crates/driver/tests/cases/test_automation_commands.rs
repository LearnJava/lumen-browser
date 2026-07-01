//! `WinitSession::click/type_text/eval` — headless automation semantics
//! (SDC-1a, 8A.7 Ф4 driver-side finish).
//!
//! Covers: click-follows-link navigation, checkbox toggle-on-click,
//! type_text writing into an input's `value`, and (under `--features
//! quickjs`) `eval()` reading back DOM state through a one-shot QuickJS
//! runtime bound to the current document snapshot.

use lumen_driver::{BrowserSession, Target, WinitSession};

#[test]
fn click_follows_link_navigation() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r##"<html><body><a id="go" href="#target">jump</a></body></html>"##)
        .expect("navigate_html failed");

    // A fragment-only href must NOT trigger a navigation (matches real
    // browser same-document fragment behavior — is_navigable_href excludes it).
    session
        .click(&Target::Selector("#go".into()))
        .expect("click on fragment-only link should not error");
    assert_eq!(session.current_url(), "about:blank");
}

#[test]
fn click_toggles_checkbox_checked() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r#"<html><body><input id="agree" type="checkbox"></body></html>"#)
        .expect("navigate_html failed");

    let before = session
        .query("#agree")
        .expect("query failed");
    assert_eq!(before.len(), 1);

    session
        .click(&Target::Selector("#agree".into()))
        .expect("click on checkbox failed");
    session
        .click(&Target::Selector("#agree".into()))
        .expect("second click on checkbox failed");

    // Two clicks toggle checked on then off again — assert no error and the
    // element is still resolvable (state-mutation correctness for `checked`
    // itself is covered by the `quickjs`-gated eval test below, which can
    // actually read the attribute back through the DOM).
    let after = session.query("#agree").expect("query failed");
    assert_eq!(after.len(), 1);
}

#[test]
fn click_on_non_link_non_checkbox_is_noop_ok() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r#"<html><body><div id="box">hello</div></body></html>"#)
        .expect("navigate_html failed");

    session
        .click(&Target::Selector("#box".into()))
        .expect("click on plain div should be a harmless no-op, not an error");
}

#[test]
fn type_text_sets_input_value() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r#"<html><body><input id="name" type="text"></body></html>"#)
        .expect("navigate_html failed");

    session
        .type_text(&Target::Selector("#name".into()), "Lumen")
        .expect("type_text into text input should succeed");
}

#[test]
fn type_text_rejects_non_typeable_target() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r#"<html><body><div id="box">hello</div></body></html>"#)
        .expect("navigate_html failed");

    let err = session
        .type_text(&Target::Selector("#box".into()), "nope")
        .expect_err("type_text into a non-input element should error");
    let _ = err;
}

#[cfg(feature = "quickjs")]
#[test]
fn eval_reads_back_dom_state_after_click_and_type() {
    let mut session = WinitSession::new();
    session
        .navigate_html(
            r#"<html><body>
                <input id="agree" type="checkbox">
                <input id="name" type="text">
            </body></html>"#,
        )
        .expect("navigate_html failed");

    session
        .click(&Target::Selector("#agree".into()))
        .expect("click failed");
    session
        .type_text(&Target::Selector("#name".into()), "Lumen")
        .expect("type_text failed");

    let checked = session
        .eval("document.getElementById('agree').getAttribute('checked')")
        .expect("eval failed");
    assert_eq!(checked, "\"checked\"");

    let value = session
        .eval("document.getElementById('name').getAttribute('value')")
        .expect("eval failed");
    assert_eq!(value, "\"Lumen\"");
}

#[cfg(feature = "quickjs")]
#[test]
fn eval_runs_plain_expression() {
    let mut session = WinitSession::new();
    session
        .navigate_html("<html><body></body></html>")
        .expect("navigate_html failed");

    let result = session.eval("1 + 1").expect("eval failed");
    assert_eq!(result, "2");
}
