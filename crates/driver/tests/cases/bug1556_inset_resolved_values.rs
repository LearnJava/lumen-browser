//! BUG-1556 — `getComputedStyle` used/resolved values of `top/right/bottom/left`: the WPT
//! `css/cssom/getComputedStyle-insets-*.html` matrix (6 writing modes × 6 containing-block
//! writing modes per variant), run in-process against the vendored support script.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const HARNESS: &str = r#"
var __fails = [];
function test(fn, name) { try { fn(); } catch (e) { __fails.push(name + ' :: ' + e.message); } }
function assert_equals(a, b, m) { if (a !== b) throw new Error(m + ' got ' + a + ' expected ' + b); }
"#;

fn support_script() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests/wpt/css/cssom/support/getComputedStyle-insets.js");
    std::fs::read_to_string(path).expect("support script").replace("export ", "")
}

/// Runs one WPT variant; returns the failing subtests (`<wm> inside <wm> - <message> :: <detail>`).
fn failures(css: &str, config: &str) -> Vec<String> {
    let mut s = InProcessSession::new();
    s.navigate_html(&format!("<html><head><style>{css}</style></head><body></body></html>")).expect("navigate_html");
    // After load, like the module script of the WPT page: layout has run once and every
    // `style.cssText` write below has to be visible to the next `getComputedStyle` read.
    let js = format!(
        "(function(){{ {HARNESS}{}
runTests({config}); return JSON.stringify(__fails); }})()",
        support_script()
    );
    let json = s.eval(&js).expect("eval");
    let raw: String = serde_json::from_str(&json).unwrap_or(json);
    serde_json::from_str(&raw).expect("fail list")
}

fn check(name: &str, css: &str, config: &str) {
    let failed = failures(css, config);
    assert!(failed.is_empty(), "{name}: {} failing subtests:
{}", failed.len(), failed.join("
"));
}

#[test]
fn static_insets() {
    check("static", "", "{style: 'position: static', containingBlockElement: containerForInflow, containingBlockArea: 'content', preservesPercentages: true, preservesAuto: true, canStretchAutoSize: false}");
}

#[test]
fn nobox_insets() {
    check("nobox", "", "{style: 'display: none', containingBlockElement: null, preservesPercentages: true, preservesAuto: true, canStretchAutoSize: false}");
}

#[test]
fn relative_insets() {
    check("relative", "", "{style: 'position: relative', containingBlockElement: containerForInflow, containingBlockArea: 'content', preservesPercentages: false, preservesAuto: false, canStretchAutoSize: false}");
}

#[test]
fn sticky_insets() {
    check("sticky", "#container-for-inflow { overflow: hidden; }", "{style: 'position: sticky;', containingBlockElement: containerForInflow, containingBlockArea: 'content', preservesPercentages: false, preservesAuto: true, canStretchAutoSize: false}");
}

#[test]
fn sticky_in_abspos_container_insets() {
    check("sticky-abspos", "#container-for-abspos { height: 200px; width: 400px; overflow: hidden; }", "{style: 'position: sticky;', containingBlockElement: containerForAbspos, containingBlockArea: 'content', preservesPercentages: false, preservesAuto: true, canStretchAutoSize: false}");
}

#[test]
fn absolute_insets() {
    check("absolute", "", "{style: 'position: absolute', containingBlockElement: containerForAbspos, containingBlockArea: 'padding', preservesPercentages: false, preservesAuto: false, canStretchAutoSize: true, staticPositionY: 1 + 2 + 4 + 8, staticPositionX: 2 + 4 + 8 + 16}");
}

#[test]
fn fixed_insets() {
    check("fixed", "", "{style: 'position: fixed', containingBlockElement: containerForFixed, containingBlockArea: 'padding', preservesPercentages: false, preservesAuto: false, canStretchAutoSize: true, staticPositionY: 1 + 2 + 4 + 8 + 16 + 32 + 64, staticPositionX: 2 + 4 + 8 + 16 + 32 + 64 + 128}");
}
