//! BUG-489 — `getComputedStyle()` on a `display: contents` element itself
//! answered empty for every property because `flatten_contents` removes the
//! element's own `LayoutBox` from the tree entirely, and neither collector
//! ever saw it. This is the end-to-end check over the real pipeline (style,
//! layout, `update_computed_styles`), reusing the exact repro shape from the
//! vendored WPT test `css/css-display/display-contents-computed-style.html`.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

fn unquote(s: &str) -> String {
    s.trim_matches('"').to_owned()
}

const PAGE: &str = r#"<html><head><style>
  html, .contents { display: contents }
  #t4 { width: auto; height: 50%; margin-left: 25%; padding-top: 10%; }
</style></head><body>
<div id="t1" class="contents"></div>
<div id="t4" class="contents"></div>
</body></html>"#;

#[test]
fn display_contents_element_reports_its_own_computed_style() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    let display = session
        .eval("getComputedStyle(document.getElementById('t1')).display")
        .expect("eval display");
    assert_eq!(unquote(&display), "contents");

    let width = session
        .eval("getComputedStyle(document.getElementById('t4')).width")
        .expect("eval width");
    assert_eq!(unquote(&width), "auto");
    let margin_left = session
        .eval("getComputedStyle(document.getElementById('t4')).marginLeft")
        .expect("eval marginLeft");
    assert_eq!(unquote(&margin_left), "25%");
}

#[test]
fn display_contents_is_blockified_for_the_root_element() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    let display = session
        .eval("getComputedStyle(document.documentElement).display")
        .expect("eval display");
    assert_eq!(unquote(&display), "block");
}
