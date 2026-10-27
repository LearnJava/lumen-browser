//! `transform-box` (CSS Transforms L1): `CSS.supports` и `getComputedStyle`.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><body><div id="t" style="width:100px;height:50px">x</div></body></html>"#;

fn ev(s: &mut InProcessSession, js: &str) -> String {
    s.eval(js).expect("eval").trim_matches('"').to_owned()
}

#[test]
fn transform_box_is_supported_and_has_a_computed_value() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(ev(&mut s, "String(CSS.supports('transform-box','fill-box'))"), "true");
    let get = "getComputedStyle(document.getElementById('t')).transformBox";
    assert_eq!(ev(&mut s, get), "view-box");
    let set = |s: &mut InProcessSession, v: &str| {
        ev(s, &format!("document.getElementById('t').style.transformBox='{v}'; {get}"))
    };
    assert_eq!(set(&mut s, "fill-box"), "fill-box");
    assert_eq!(set(&mut s, "content-box"), "content-box");
}
