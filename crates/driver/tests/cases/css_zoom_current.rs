//! CSSOM View `Element.currentCSSZoom` (BUG-943) and the computed `zoom`: the
//! effective factor (own `zoom` times every ancestor's), 1 for unrendered content.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><head></head><body>
<div id="unzoomed"><div id="unzoomedChild"></div></div>
<div style="zoom: 2" id="outer">
  <div style="zoom: 2" id="inner">
    <div id="renderedChild"></div>
    <b id="inlineChild">x</b>
    <div style="display: none" id="nonRenderedChild"><p id="deepHidden"></p></div>
  </div>
</div>
<div style="zoom: 50%" id="pct"></div>
</body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

#[test]
fn current_css_zoom_compounds_down_the_tree() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    for (id, want) in [
        ("unzoomed", "1"),
        ("unzoomedChild", "1"),
        ("outer", "2"),
        ("inner", "4"),
        ("renderedChild", "4"),
        ("inlineChild", "4"),
        ("pct", "0.5"),
        ("nonRenderedChild", "1"),
        ("deepHidden", "1"),
    ] {
        assert_eq!(ev(&mut s, &format!("document.getElementById('{id}').currentCSSZoom")), want, "{id}");
    }
}

#[test]
fn current_css_zoom_follows_style_changes_and_detached_is_one() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    ev(&mut s, "document.getElementById('unzoomed').style.zoom = 2; 0");
    assert_eq!(ev(&mut s, "document.getElementById('unzoomed').currentCSSZoom"), "2");
    assert_eq!(ev(&mut s, "document.getElementById('unzoomedChild').currentCSSZoom"), "2");
    assert_eq!(ev(&mut s, "document.createElement('div').currentCSSZoom"), "1");
    assert_eq!(ev(&mut s, "getComputedStyle(document.getElementById('inner')).zoom"), "4");
}
