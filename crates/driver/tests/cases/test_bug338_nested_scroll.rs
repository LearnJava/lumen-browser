//! BUG-338: automation `scroll(target, delta)` routes to a nested
//! `overflow:auto` container instead of always moving only the page.
//!
//! Covers `WinitSession::scroll` (the concrete type this bug's diagnosis
//! named). `InProcessSession::scroll` gets the equivalent coverage as
//! module-private unit tests in `crates/driver/src/session.rs` (it can
//! assert on the private `SessionState.layout_root` directly; this file can
//! only observe `WinitSession` through the public `BrowserSession` API, so
//! it reads the scroll offset back out of the `display_list()` text dump).

use lumen_driver::{BrowserSession, ScrollDelta, Target, WinitSession};

const NESTED_SCROLL_HTML: &str = r#"<html><body style="margin:0">
    <div id="outer" style="overflow:auto;width:200px;height:100px">
        <p id="leaf" style="margin-top:400px">deep content</p>
    </div>
</body></html>"#;

#[test]
fn scroll_with_selector_target_scrolls_nested_container_not_page() {
    let mut session = WinitSession::new();
    session.navigate_html(NESTED_SCROLL_HTML).expect("navigate_html failed");

    session
        .scroll(&Target::Selector("#leaf".into()), ScrollDelta { x: 0.0, y: 50.0 })
        .expect("scroll on nested target should succeed");

    let dl = session.display_list().expect("display_list");
    assert!(
        dl.contains("PushScrollLayer") && dl.contains("scroll=(0.00,50.00)"),
        "expected #outer's scroll layer at (0, 50), got:\n{dl}"
    );

    // Page-level (compositor) scroll must stay untouched — the delta was
    // routed to the nested container instead.
    let root_offset = session.active_property_trees().and_then(|t| t.scroll.nodes.first().map(|n| n.offset_y));
    assert_eq!(root_offset, Some(0.0));
}

#[test]
fn scroll_with_target_outside_any_container_falls_back_to_page() {
    let mut session = WinitSession::new();
    session.navigate_html(NESTED_SCROLL_HTML).expect("navigate_html failed");

    session
        .scroll(&Target::Selector("body".into()), ScrollDelta { x: 0.0, y: 30.0 })
        .expect("scroll should succeed");

    let dl = session.display_list().expect("display_list");
    assert!(
        dl.contains("scroll=(0.00,0.00)"),
        "#outer must stay untouched when the target has no scrolling ancestor, got:\n{dl}"
    );
    let root_offset = session.active_property_trees().and_then(|t| t.scroll.nodes.first().map(|n| n.offset_y));
    assert_eq!(root_offset, Some(30.0), "delta should land on the page-level scroll instead");
}
