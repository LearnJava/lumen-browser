//! Test 34-forms.html — form control static layout via attribute selectors.
//!
//! Inputs are sized through `input[type=…]` rules (content-box + 1px border).
//! The load-bearing checks: text inputs render 182x26, checkboxes/radios 18x18,
//! buttons 102x30, textarea 222x56, select 142x26 — proving attribute selectors
//! resolve and form controls take their declared box. Inputs are matched by tag in
//! document order; the `type=hidden` input is display:none so it has no layout box
//! and is absent → 15 visible inputs out of 16 in the markup.

use lumen_driver::{BrowserSession, InProcessSession};

fn navigate(session: &mut InProcessSession, file: &str) {
    let root = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(root)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(file);
    session
        .navigate(&format!("file://{}", path.display()))
        .expect("navigate");
}

#[test]
fn test_34_forms() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/34-forms.html");

    // 15 laid-out <input> elements in document order (the 16th, type=hidden, is
    // display:none and produces no layout box).
    let inputs = session.all_layout_boxes_by_selector("input").expect("query input");
    assert_eq!(inputs.len(), 15, "expected 15 visible input elements");

    // inputs[0..3): text/email/password → 180x24 content + 1px border = 182x26.
    for (i, b) in inputs[0..3].iter().enumerate() {
        assert!(
            (b.border_box.width - 182.0).abs() < 1.0 && (b.border_box.height - 26.0).abs() < 1.0,
            "text input[{i}] should be 182x26, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // inputs[3..8): checkbox/checkbox/radio/radio/checkbox → 16x16 + 1px border = 18x18.
    for (i, b) in inputs[3..8].iter().enumerate() {
        assert!(
            (b.border_box.width - 18.0).abs() < 1.0 && (b.border_box.height - 18.0).abs() < 1.0,
            "checkbox/radio input[{}] should be 18x18, got {}x{}",
            i + 3,
            b.border_box.width,
            b.border_box.height
        );
    }

    // Buttons: 100x28 content + 1px border = 102x30, two of them.
    let buttons = session.all_layout_boxes_by_selector("button").expect("query button");
    assert_eq!(buttons.len(), 2, "expected 2 buttons");
    for (i, b) in buttons.iter().enumerate() {
        assert!(
            (b.border_box.width - 102.0).abs() < 1.0 && (b.border_box.height - 30.0).abs() < 1.0,
            "button[{i}] should be 102x30, got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // textarea: 220x54 content + 1px border = 222x56.
    let ta = session.layout_box_by_selector("textarea").unwrap().expect("textarea");
    assert!(
        (ta.border_box.width - 222.0).abs() < 1.0 && (ta.border_box.height - 56.0).abs() < 1.0,
        "textarea should be 222x56, got {}x{}",
        ta.border_box.width,
        ta.border_box.height
    );

    // select: 140x24 content + 1px border = 142x26.
    let sel = session.layout_box_by_selector("select").unwrap().expect("select");
    assert!(
        (sel.border_box.width - 142.0).abs() < 1.0 && (sel.border_box.height - 26.0).abs() < 1.0,
        "select should be 142x26, got {}x{}",
        sel.border_box.width,
        sel.border_box.height
    );
}
