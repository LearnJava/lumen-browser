//! Tests for PH1-7: Compositor thread + Property Trees.
//!
//! Verifies that:
//! - `PropertyTrees` are built and committed after each `navigate()`.
//! - `active_property_trees()` returns the correct snapshot.
//! - `scroll_page_by()` updates scroll offsets without triggering relayout.
//! - Compositor two-buffer model works: pending promotes to active.

use lumen_driver::{BrowserSession, InProcessSession};

// ── active_property_trees() ──────────────────────────────────────────────────

#[test]
fn property_trees_none_before_navigate() {
    let session = InProcessSession::new();
    assert!(
        session.active_property_trees().is_none(),
        "no trees before first navigate"
    );
}

#[test]
fn property_trees_present_after_navigate() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>hello</p>").unwrap();
    assert!(
        session.active_property_trees().is_some(),
        "trees must be present after navigate"
    );
}

#[test]
fn property_trees_have_root_nodes() {
    let mut session = InProcessSession::new();
    session.navigate_html("<div>text</div>").unwrap();
    let trees = session.active_property_trees().unwrap();
    assert!(
        !trees.transform.nodes.is_empty(),
        "TransformTree must have at least the root node"
    );
    assert!(
        !trees.scroll.nodes.is_empty(),
        "ScrollTree must have at least the root node"
    );
    assert!(
        !trees.effect.nodes.is_empty(),
        "EffectTree must have at least the root node"
    );
    assert!(
        !trees.clip.nodes.is_empty(),
        "ClipTree must have at least the root node"
    );
}

#[test]
fn property_trees_transform_node_for_css_transform() {
    let mut session = InProcessSession::new();
    session
        .navigate_html("<div style='transform: translate(10px, 20px);'>x</div>")
        .unwrap();
    let trees = session.active_property_trees().unwrap();
    // root node (id=0) + one TransformNode for the div.
    assert!(
        trees.transform.nodes.len() >= 2,
        "expected at least 2 transform nodes, got {}",
        trees.transform.nodes.len()
    );
    let div_node = &trees.transform.nodes[1];
    assert!(
        !div_node.local.is_identity(),
        "translate transform must produce a non-identity matrix"
    );
}

#[test]
fn property_trees_effect_node_for_opacity() {
    let mut session = InProcessSession::new();
    session
        .navigate_html("<div style='opacity: 0.5;'>x</div>")
        .unwrap();
    let trees = session.active_property_trees().unwrap();
    assert!(
        trees.effect.nodes.len() >= 2,
        "expected effect node for opacity < 1"
    );
    let div_effect = &trees.effect.nodes[1];
    assert!(
        (div_effect.opacity - 0.5).abs() < 1e-4,
        "effect node must carry opacity=0.5"
    );
}

#[test]
fn property_trees_clip_node_for_overflow_hidden() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            "<div style='overflow: hidden; width: 100px; height: 50px;'>x</div>",
        )
        .unwrap();
    let trees = session.active_property_trees().unwrap();
    assert!(
        trees.clip.nodes.len() >= 2,
        "overflow:hidden must create a clip node"
    );
    assert!(
        trees.clip.nodes[1].clip.is_some(),
        "clip node must have a clip rect"
    );
}

#[test]
fn property_trees_reset_on_second_navigate() {
    let mut session = InProcessSession::new();
    // Page with transform.
    session
        .navigate_html("<div style='transform: rotate(45deg);'>x</div>")
        .unwrap();
    let trees_first = session.active_property_trees().unwrap();
    let len_first = trees_first.transform.nodes.len();

    // Page without transform — TransformTree resets to root-only.
    session.navigate_html("<p>plain</p>").unwrap();
    let trees_second = session.active_property_trees().unwrap();
    let len_second = trees_second.transform.nodes.len();

    assert!(
        len_first > len_second,
        "second page has fewer transform nodes ({len_second}) than first ({len_first})"
    );
}

// ── scroll_page_by() — off-main-thread scroll ────────────────────────────────

#[test]
fn scroll_page_by_returns_false_before_navigate() {
    let mut session = InProcessSession::new();
    let moved = session.scroll_page_by(0.0, 100.0);
    assert!(!moved, "scroll_page_by must return false before navigate");
}

#[test]
fn scroll_page_by_returns_true_after_navigate() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();
    let moved = session.scroll_page_by(0.0, 50.0);
    assert!(moved, "scroll_page_by must return true after navigate");
}

#[test]
fn scroll_page_by_updates_root_scroll_offset() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();

    // Scroll down 100px.
    session.scroll_page_by(0.0, 100.0);
    let trees = session.active_property_trees().unwrap();
    let root_offset_y = trees.scroll.nodes[0].offset_y;
    assert!(
        (root_offset_y - 100.0).abs() < 1.0,
        "expected root scroll offset_y ≈ 100, got {root_offset_y}"
    );
}

#[test]
fn scroll_page_by_accumulates_across_calls() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();

    session.scroll_page_by(0.0, 40.0);
    session.scroll_page_by(0.0, 60.0);
    let trees = session.active_property_trees().unwrap();
    let offset_y = trees.scroll.nodes[0].offset_y;
    assert!(
        (offset_y - 100.0).abs() < 1.0,
        "accumulated scroll must be 100, got {offset_y}"
    );
}

#[test]
fn scroll_page_by_does_not_go_negative() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();

    // Scroll up from 0 — must clamp to 0.
    session.scroll_page_by(0.0, -50.0);
    let trees = session.active_property_trees().unwrap();
    let offset_y = trees.scroll.nodes[0].offset_y;
    assert!(
        offset_y >= 0.0,
        "scroll offset must not go negative, got {offset_y}"
    );
}

#[test]
fn scroll_page_by_horizontal() {
    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();

    session.scroll_page_by(80.0, 0.0);
    let trees = session.active_property_trees().unwrap();
    let offset_x = trees.scroll.nodes[0].offset_x;
    assert!(
        (offset_x - 80.0).abs() < 1.0,
        "horizontal scroll must be 80, got {offset_x}"
    );
}

#[test]
fn scroll_page_by_does_not_trigger_relayout() {
    // Verify that scroll_page_by is faster than a full navigate (heuristic:
    // calling it 100 times must finish in under 100ms on any reasonable machine).
    use std::time::Instant;

    let mut session = InProcessSession::new();
    session.navigate_html("<p>text</p>").unwrap();

    let t0 = Instant::now();
    for i in 0..100 {
        session.scroll_page_by(0.0, i as f32);
    }
    let elapsed = t0.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "100 scroll_page_by calls took {}ms — suggests relayout is happening",
        elapsed.as_millis()
    );
}

#[test]
fn scroll_page_by_independent_of_layout_boxes() {
    // Verify layout_snapshot() is unaffected by scroll (scroll is compositor-only).
    let mut session = InProcessSession::new();
    session
        .navigate_html("<div id='box' style='width:200px;height:100px;'>x</div>")
        .unwrap();

    let boxes_before = session.layout_snapshot().unwrap();
    session.scroll_page_by(0.0, 200.0);
    let boxes_after = session.layout_snapshot().unwrap();

    assert_eq!(
        boxes_before.len(),
        boxes_after.len(),
        "scroll must not change number of layout boxes"
    );
    // Layout box coordinates don't change — scroll is applied at compositor level.
    for (b, a) in boxes_before.iter().zip(boxes_after.iter()) {
        assert_eq!(
            b.border_box, a.border_box,
            "layout box rect must be unchanged by scroll"
        );
    }
}
