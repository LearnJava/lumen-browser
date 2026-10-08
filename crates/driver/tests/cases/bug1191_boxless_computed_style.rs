//! BUG-1191 — `getComputedStyle()` answered an empty declaration for an element
//! in the flat tree that owns no `LayoutBox` (empty inline, descendant of
//! `display: none` / `content-visibility: hidden`). Such elements now carry
//! their cascaded style; `innerText`/`checkVisibility` must not read that as
//! «rendered».

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><head><style>
  #none { display: none }
  #cv { content-visibility: hidden }
</style></head><body>
<div id="host">a<span id="empty"></span>b</div>
<div id="none"><p id="in-none">X</p></div>
<div id="cv"><p id="in-cv">Y</p></div>
</body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

#[test]
fn boxless_elements_report_full_computed_style() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    for id in ["empty", "in-none", "in-cv"] {
        let len = ev(&mut s, &format!("getComputedStyle(document.getElementById('{id}')).length > 0"));
        assert_eq!(len, "true", "{id}: length");
    }
    assert_eq!(ev(&mut s, "getComputedStyle(document.getElementById('empty')).display"), "inline");
    assert_eq!(ev(&mut s, "getComputedStyle(document.getElementById('in-none')).display"), "block");
}

#[test]
fn boxless_elements_are_still_not_rendered() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(ev(&mut s, "document.getElementById('in-none').checkVisibility()"), "false");
    assert_eq!(ev(&mut s, "document.getElementById('in-cv').checkVisibility()"), "false");
    assert_eq!(ev(&mut s, "document.getElementById('in-none').innerText"), "X");
    assert_eq!(ev(&mut s, "document.getElementById('host').innerText"), "ab");
}
