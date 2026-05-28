//! Test antidetect surface API — verify no automation hooks in JS bindings
//!
//! Automation detection hooks are commonly used by anti-bot systems to detect
//! automation tools like Selenium, Puppeteer, Playwright. These include:
//! - navigator.webdriver (Selenium)
//! - chrome.runtime (CDP)
//! - __playwright (Playwright)
//! - cdc_* variables (CDP client markers)
//! - window.devtools, navigator.hardwareConcurrency override, etc.
//!
//! Lumen should not expose any of these, allowing scripts to work without
//! detection warnings. This is a **negative test**: we verify absence, not presence.
//!
//! Phase 1 (code review): verify that crates/js/src/dom.rs doesn't export
//! automation-detection properties into the global JS environment.
//!
//! Phase 2 (eval-based): when task 8A.7 (persistent JS runtime) is complete,
//! add JavaScript eval tests to verify runtime behavior.

use std::path::Path;

/// Read the JS environment bindings from dom.rs
fn read_dom_js_source() -> String {
    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let dom_path = Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root")
        .join("crates/js/src/dom.rs");

    std::fs::read_to_string(&dom_path)
        .expect("Failed to read dom.rs")
}

#[test]
fn test_navigator_webdriver_not_exported() {
    let source = read_dom_js_source();

    // navigator.webdriver should NOT be in the navigator object definition
    assert!(
        !source.contains("webdriver:") && !source.contains("webdriver ="),
        "navigator.webdriver should not be exported; Selenium detection marker must be absent"
    );
}

#[test]
fn test_chrome_runtime_not_exported() {
    let source = read_dom_js_source();

    // chrome or chrome.runtime should NOT be in window object or as global variable
    let has_chrome_global = source.contains("var chrome") || source.contains("chrome: ");
    assert!(
        !has_chrome_global,
        "chrome object/chrome.runtime should not be exported; CDP detection marker must be absent"
    );
}

#[test]
fn test_playwright_marker_not_exported() {
    let source = read_dom_js_source();

    // __playwright should NOT be defined as a global or window property
    assert!(
        !source.contains("__playwright") && !source.contains("__pwExecPath"),
        "__playwright marker should not be exported; Playwright detection must be absent"
    );
}

#[test]
fn test_cdc_variables_not_exported() {
    let source = read_dom_js_source();

    // cdc_* variables (CDP client markers) should NOT be defined
    assert!(
        !source.contains("cdc_"),
        "cdc_* variables should not be exported; CDP client detection marker must be absent"
    );
}

#[test]
fn test_devtools_object_not_exported() {
    let source = read_dom_js_source();

    // window.devtools should NOT be defined
    let has_devtools = source.contains("devtools:") || source.contains("devtools = ");
    assert!(
        !has_devtools,
        "window.devtools should not be exported; devtools detection marker must be absent"
    );
}

#[test]
fn test_no_common_automation_markers() {
    let source = read_dom_js_source();

    // Check for other common automation detection markers
    let markers = vec![
        "__webdriverio",
        "__cypress",
        "__jasmineRequire",
        "nightwatch",
        "callPhantom",
        "phantom",
    ];

    for marker in markers {
        assert!(
            !source.contains(&format!("var {}", marker))
                && !source.contains(&format!("{}: ", marker))
                && !source.contains(&format!("{} =", marker)),
            "Automation marker '{}' should not be exported",
            marker
        );
    }
}

#[test]
fn test_navigator_has_clean_surface() {
    let source = read_dom_js_source();

    // Extract the navigator object definition
    let navigator_start = source.find("var navigator = {")
        .expect("navigator object not found in dom.rs");
    let navigator_end = source[navigator_start..]
        .find("};")
        .expect("navigator object closing not found") + navigator_start + 2;
    let navigator_def = &source[navigator_start..navigator_end];

    // Verify navigator only has safe properties
    let safe_props = vec!["userAgent", "language", "onLine", "serviceWorker"];
    for safe_prop in safe_props {
        assert!(
            navigator_def.contains(safe_prop),
            "navigator should have '{}' property",
            safe_prop
        );
    }

    // Verify no unsafe properties are present
    let unsafe_props = vec!["webdriver", "plugins", "hardwareConcurrency", "vendor"];
    for unsafe_prop in unsafe_props {
        assert!(
            !navigator_def.contains(&format!("{}:", unsafe_prop))
                && !navigator_def.contains(&format!("{} =", unsafe_prop)),
            "navigator should NOT have '{}' property",
            unsafe_prop
        );
    }
}

#[test]
fn test_window_has_clean_surface() {
    let source = read_dom_js_source();

    // Extract the window object definition
    let window_start = source.find("var window = {")
        .expect("window object not found in dom.rs");
    let window_end = source[window_start..]
        .find("};")
        .expect("window object closing not found") + window_start + 2;
    let window_def = &source[window_start..window_end];

    // Verify window object doesn't have dangerous properties
    let unsafe_props = vec![
        "chrome:",
        "devtools:",
        "__playwright",
        "__webdriverio",
        "cdc_",
        "callPhantom",
    ];
    for unsafe_prop in unsafe_props {
        assert!(
            !window_def.contains(unsafe_prop),
            "window object should NOT contain '{}' detection marker",
            unsafe_prop
        );
    }
}
