//! DEVX-10: `explain_element(selector)` — causal chain DoD.
//!
//! `docs/tasks/p1-introspection-track.md` §DEVX-10 DoD, two cases:
//! 1. An element collapsed to 0×0 inside an `overflow:hidden` ancestor must
//!    show *which* link the chain stops at (here: `size`), not just "no
//!    paint".
//! 2. An element whose text child becomes an anonymous inline-run box
//!    (`BoxRole::AnonymousInlineRun`, sharing the element's `NodeId`) must
//!    not have that child's paint attributed to the element's own chain
//!    (ADR-025 §1 — identity is `(node, role)`, never `node` alone).

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn zero_size_inside_overflow_hidden_breaks_at_size_link() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body>
                 <div id="clip" style="overflow:hidden;width:100px;height:100px">
                   <div id="collapsed" style="width:0;height:0;background:red"></div>
                 </div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let explain = session.explain_element("#collapsed").expect("explain_element failed");

    assert!(explain.in_dom, "the node exists in the DOM");
    assert!(explain.style_applied, "the cascade ran and produced a layout box");
    assert!(explain.in_layout, "the box is not display:none/skipped");
    assert_eq!(
        explain.size,
        Some((0.0, 0.0)),
        "DEVX-10 DoD: the chain must show the *exact* link it breaks at — here \
         the box exists, is in layout, but collapsed to 0×0, not merely \
         \"invisible\""
    );
}

#[test]
fn missing_selector_only_sets_in_dom_false() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><div id="real"></div></body></html>"#)
        .expect("navigate_html failed");

    let explain = session.explain_element("#does-not-exist").expect("explain_element failed");

    assert!(!explain.in_dom);
    assert!(!explain.style_applied);
    assert!(!explain.in_layout);
    assert_eq!(explain.size, None);
    assert_eq!(explain.layer, None);
}

#[test]
fn anonymous_inline_run_paint_not_attributed_to_parent_element() {
    let mut session = InProcessSession::new();
    // `#wrap` has no background/border of its own — any command attributed to
    // it that isn't correctly scoped to `BoxRole::Element` would be a bug.
    // Its text content ("hello") becomes an `AnonymousInlineRun` box that
    // shares `#wrap`'s `NodeId` but carries a different `BoxRole` (ADR-025).
    session
        .navigate_html(
            r#"<html><body style="margin:0">
                 <div id="wrap">hello</div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    // Sanity: the page did paint something (the text glyphs), so a
    // zero-commands result below is a real "not misattributed" signal, not
    // an artifact of nothing having painted at all.
    let display_list = session.display_list_for_compare().expect("display_list_for_compare failed");
    assert!(!display_list.is_empty(), "sanity: the text must have produced display-list commands");

    let explain = session.explain_element("#wrap").expect("explain_element failed");
    assert!(explain.in_layout);
    assert_eq!(
        explain.commands_emitted, 0,
        "DEVX-10 DoD: `#wrap`'s own principal box (BoxRole::Element) has no \
         visible background/border — the text's AnonymousInlineRun paint \
         (which shares #wrap's NodeId) must not be folded into #wrap's own \
         chain"
    );
}
