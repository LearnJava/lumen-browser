//! DEVX-11: `explain_page()` — page-level aggregate DoD.
//!
//! `docs/tasks/p1-introspection-track.md` §DEVX-11: invariant-firing counts
//! by category plus telemetry (box counts, anonymous boxes, overflow
//! elements, commands, clip depth, relayouts, timing).

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn telemetry_counts_boxes_anonymous_boxes_and_commands() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
                 <div id="a">hello</div>
                 <div id="b" style="overflow:auto;width:50px;height:50px">clip me</div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let explain = session.explain_page().expect("explain_page failed");

    // A text child ("hello") becomes an AnonymousInlineRun box — must be
    // counted, and the tree has more than zero boxes overall.
    assert!(explain.box_count > 0, "the tree has at least the html/body/div boxes");
    assert!(explain.anonymous_box_count > 0, "the text nodes produce anonymous inline-run boxes");
    // `#b` has overflow:auto on both axes — must be counted.
    assert!(explain.overflow_element_count >= 1, "#b's overflow:auto must be counted");
    assert!(explain.command_count > 0, "the page painted something");
}

#[test]
fn invariant_violations_are_all_zero_on_a_clean_page() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body>
                 <div style="width:200px;padding:8px;border:1px solid black">
                   <p>ordinary content</p>
                 </div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let explain = session.explain_page().expect("explain_page failed");

    assert_eq!(
        explain.invariant_violations,
        lumen_driver::InvariantViolationCounts::default(),
        "DEVX-8a/8b: an ordinary page must fire zero invariants — a nonzero \
         count here is a real engine bug, not this test being wrong"
    );
}

#[test]
fn relayout_count_starts_at_one_and_increments_after_a_mutation() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(r#"<html><body><div id="target" style="width:10px"></div></body></html>"#)
        .expect("navigate_html failed");

    let before = session.explain_page().expect("explain_page failed");
    assert_eq!(before.relayout_count, Some(1), "navigate()'s initial layout counts as pass 1");

    session
        .eval(r#"document.getElementById('target').style.width = '20px';"#)
        .expect("eval failed");

    let after = session.explain_page().expect("explain_page failed");
    assert_eq!(after.relayout_count, Some(2), "eval()'s DEVX-9 relayout must bump the counter");
}
