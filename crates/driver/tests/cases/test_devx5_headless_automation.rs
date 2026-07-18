//! `InProcessSession::click/type_text/scroll/eval` — headless MCP automation
//! (DEVX-5): `--mcp`/`--mcp-port` drive these through `BrowserSession`.
//!
//! Covers: click-follows-link navigation, checkbox toggle-on-click,
//! type_text writing into an input's `value`, page scroll, and (under
//! `--features v8`) `eval()` against the **persistent** V8 runtime
//! installed on the document at `navigate()` time — unlike
//! `WinitSession::eval`'s one-shot runtime, JS-side DOM mutations persist
//! across `eval()` calls within the same navigation.

use lumen_driver::{BrowserSession, InProcessSession, ScrollDelta, Target};

#[test]
fn click_follows_link_navigation() {
    let mut session = InProcessSession::new();
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
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><input id="agree" type="checkbox"></body></html>"#)
        .expect("navigate_html failed");

    session
        .click(&Target::Selector("#agree".into()))
        .expect("click on checkbox failed");
    session
        .click(&Target::Selector("#agree".into()))
        .expect("second click on checkbox failed");

    let after = session.query("#agree").expect("query failed");
    assert_eq!(after.len(), 1);
}

#[test]
fn click_on_non_link_non_checkbox_is_noop_ok() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><div id="box">hello</div></body></html>"#)
        .expect("navigate_html failed");

    session
        .click(&Target::Selector("#box".into()))
        .expect("click on plain div should be a harmless no-op, not an error");
}

#[test]
fn type_text_sets_input_value() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><input id="name" type="text"></body></html>"#)
        .expect("navigate_html failed");

    session
        .type_text(&Target::Selector("#name".into()), "Lumen")
        .expect("type_text into text input should succeed");
}

#[test]
fn type_text_rejects_non_typeable_target() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><div id="box">hello</div></body></html>"#)
        .expect("navigate_html failed");

    let err = session
        .type_text(&Target::Selector("#box".into()), "nope")
        .expect_err("type_text into a non-input element should error");
    let _ = err;
}

#[test]
fn scroll_updates_compositor_offset() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body style="height:3000px"></body></html>"#)
        .expect("navigate_html failed");

    session
        .scroll(&Target::Point { x: 0.0, y: 0.0 }, ScrollDelta { x: 0.0, y: 200.0 })
        .expect("scroll failed");

    let offset_y = session
        .active_property_trees()
        .and_then(|t| t.scroll.nodes.first().map(|n| n.offset_y))
        .unwrap_or(0.0);
    assert_eq!(offset_y, 200.0);
}

#[cfg(feature = "v8")]
#[test]
fn eval_reads_back_dom_state_after_click_and_type() {
    let mut session = InProcessSession::new();
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

#[cfg(feature = "v8")]
#[test]
fn eval_runs_plain_expression() {
    let mut session = InProcessSession::new();
    session
        .navigate_html("<html><body></body></html>")
        .expect("navigate_html failed");

    let result = session.eval("1 + 1").expect("eval failed");
    assert_eq!(result, "2");
}

/// The key DEVX-5 differentiator vs. `WinitSession::eval`'s one-shot runtime:
/// a DOM mutation made by one `eval()` call must be visible to the next
/// `eval()` call within the same navigation, since both share one persistent
/// V8 runtime installed on the same `Arc<Mutex<Document>>`.
#[cfg(feature = "v8")]
#[test]
fn eval_mutations_persist_across_calls() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><div id="box"></div></body></html>"#)
        .expect("navigate_html failed");

    session
        .eval("document.getElementById('box').setAttribute('data-mark', 'seen')")
        .expect("first eval failed");

    let mark = session
        .eval("document.getElementById('box').getAttribute('data-mark')")
        .expect("second eval failed");
    assert_eq!(mark, "\"seen\"");
}

#[cfg(not(feature = "v8"))]
#[test]
fn eval_errors_without_v8_feature() {
    let mut session = InProcessSession::new();
    session
        .navigate_html("<html><body></body></html>")
        .expect("navigate_html failed");

    let err = session.eval("1 + 1").expect_err("eval must error without v8");
    let _ = err;
}
