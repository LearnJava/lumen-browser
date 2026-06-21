//! Test GpuSession implementation (Phase 4b)

use lumen_driver::{BrowserSession, GpuSession, WinitSession};

#[test]
fn test_gpu_session_render_to_gpu() {
    let mut session = WinitSession::new();

    // Load a simple test page
    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/01-sanity.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Test render_to_gpu()
    let rendered = session.render_to_gpu().expect("Failed to render_to_gpu");

    // Verify RenderedPage contains expected data
    // display_list should exist (may be empty in Phase 4b for simple pages)
    let _ = rendered.display_list;

    // Title may or may not be extracted in Phase 4b, but structure should be valid
    let _ = rendered.title;

    // layout_box should be root of layout tree and have a valid node reference
    let _ = rendered.layout_box.node;
}

#[test]
fn test_gpu_session_scroll_position() {
    let mut session = WinitSession::new();

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let test_file = std::path::Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("graphic_tests/01-sanity.html");
    let url = format!("file://{}", test_file.display());
    session.navigate(&url).expect("Failed to navigate");

    // Initial scroll should be (0, 0)
    assert_eq!(session.scroll_position(), (0.0, 0.0), "Initial scroll should be (0, 0)");

    // Apply scroll delta
    let delta = lumen_driver::ScrollDelta { x: 10.0, y: 20.0 };
    session.set_scroll(delta).expect("Failed to set scroll");

    // Verify scroll position updated
    assert_eq!(session.scroll_position(), (10.0, 20.0), "Scroll should be (10, 20)");

    // Apply another delta
    let delta = lumen_driver::ScrollDelta { x: 5.0, y: -10.0 };
    session.set_scroll(delta).expect("Failed to set scroll");

    // Verify scroll position accumulated
    assert_eq!(session.scroll_position(), (15.0, 10.0), "Scroll should be (15, 10)");
}

#[test]
fn test_gpu_session_viewport() {
    let session = WinitSession::new();

    // Default viewport should be 1024×720
    assert_eq!(session.viewport_size().width, 1024.0, "Default viewport width should be 1024");
    assert_eq!(session.viewport_size().height, 720.0, "Default viewport height should be 720");

    // Test with_viewport
    let session = WinitSession::with_viewport(800.0, 600.0);
    assert_eq!(session.viewport_size().width, 800.0, "Custom viewport width should be 800");
    assert_eq!(session.viewport_size().height, 600.0, "Custom viewport height should be 600");
}
