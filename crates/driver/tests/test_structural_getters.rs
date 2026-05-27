//! Test structural-getters API (8A.4): layout_box_by_selector and all_layout_boxes_by_selector

use lumen_driver::{BrowserSession, InProcessSession};

#[test]
fn test_layout_box_by_selector() {
    let mut session = InProcessSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/01-sanity.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Test 1: Find box by selector (single element)
    let square_box = session
        .layout_box_by_selector(".square")
        .expect("Failed to get layout box")
        .expect("Square box not found");

    // Verify it's a 200x200 square
    assert!(
        (square_box.border_box.width - 200.0).abs() < 1.0,
        "Square width should be ~200px, got {}",
        square_box.border_box.width
    );
    assert!(
        (square_box.border_box.height - 200.0).abs() < 1.0,
        "Square height should be ~200px, got {}",
        square_box.border_box.height
    );

    // Verify position
    assert!(
        (square_box.border_box.x - 413.0).abs() < 2.0,
        "Square X position should be ~413px, got {}",
        square_box.border_box.x
    );
    assert!(
        (square_box.border_box.y - 261.0).abs() < 2.0,
        "Square Y position should be ~261px, got {}",
        square_box.border_box.y
    );

    // Verify tag name
    assert_eq!(square_box.tag_name, "div", "Tag name should be 'div'");

    // Test 2: Selector not found
    let not_found = session
        .layout_box_by_selector(".nonexistent")
        .expect("Failed to query selector");
    assert!(not_found.is_none(), "Should return None for non-existent selector");
}

#[test]
fn test_all_layout_boxes_by_selector() {
    // Create a simple HTML with multiple divs
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <style>
                body { margin: 0; padding: 0; }
                .item {
                    width: 100px;
                    height: 50px;
                    background: #ccc;
                    margin: 10px;
                }
            </style>
        </head>
        <body>
            <div class="item">Item 1</div>
            <div class="item">Item 2</div>
            <div class="item">Item 3</div>
            <div class="other">Not an item</div>
        </body>
        </html>
    "#;

    let mut session = InProcessSession::new();
    session
        .navigate_html(html)
        .expect("Failed to navigate HTML");

    // Find all items
    let items = session
        .all_layout_boxes_by_selector(".item")
        .expect("Failed to query selector");

    // Should find exactly 3 items
    assert_eq!(items.len(), 3, "Should find 3 items with .item selector");

    // Verify all items have the correct dimensions
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.border_box.width - 100.0).abs() < 1.0,
            "Item {} width should be 100px",
            i + 1
        );
        assert!(
            (item.border_box.height - 50.0).abs() < 1.0,
            "Item {} height should be 50px",
            i + 1
        );
        assert_eq!(item.tag_name, "div");
    }

    // Verify vertical positioning (each item has margin 10px)
    let y_positions: Vec<f32> = items.iter().map(|b| b.border_box.y).collect();
    // Items should be stacked vertically with increasing Y positions
    assert!(
        y_positions[0] < y_positions[1] && y_positions[1] < y_positions[2],
        "Items should be stacked vertically"
    );

    // Test empty result
    let empty = session
        .all_layout_boxes_by_selector(".nonexistent")
        .expect("Failed to query selector");
    assert!(empty.is_empty(), "Should return empty vector for non-existent selector");
}

#[test]
fn test_structural_getters_consistency() {
    // Verify that layout_box_by_selector matches the first element from all_layout_boxes_by_selector
    let mut session = InProcessSession::new();

    let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <style>
                .test { width: 100px; height: 50px; background: blue; }
            </style>
        </head>
        <body>
            <div class="test">First</div>
            <div class="test">Second</div>
        </body>
        </html>
    "#;

    session
        .navigate_html(html)
        .expect("Failed to navigate HTML");

    // Get single box
    let single = session
        .layout_box_by_selector(".test")
        .expect("Failed to get single box")
        .expect("Box not found");

    // Get all boxes
    let all = session
        .all_layout_boxes_by_selector(".test")
        .expect("Failed to get all boxes");

    // First box should match
    assert_eq!(all.len(), 2, "Should have 2 matching boxes");
    assert_eq!(single.node_id, all[0].node_id, "IDs should match");
    assert_eq!(single.border_box, all[0].border_box, "Border boxes should match");
    assert_eq!(single.margin_box, all[0].margin_box, "Margin boxes should match");
}
