//! BUG-1446 — `getComputedStyle(li, '::marker')` answered the list item's own style (`content:
//! normal`, `display: list-item`, the item's `text-transform`/`text-indent`, its 1008px width)
//! instead of the marker box's: UA `unicode-bidi: isolate`, `font-variant-numeric:
//! tabular-nums`, `text-transform: none`, `text-indent: 0`, the author `::marker` rule, `display`
//! `inline`/block-container and the marker box's own size.

#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r#"<html><head><style>
  .inside { list-style-position: inside }
  .outside { list-style-position: outside }
  .str { list-style-type: "string" }
  .marker::marker { content: "marker"; color: rgb(0, 128, 0); font-size: 20px }
  li { text-transform: lowercase; text-indent: 1px }
</style></head><body>
<ul>
  <li id="o-sym" class="outside">outside symbol</li>
  <li id="i-str" class="inside str">inside string</li>
  <li id="o-mk" class="outside marker">outside marker</li>
  <li id="i-mk" class="inside marker">inside marker</li>
</ul>
</body></html>"#;

fn cs(s: &mut InProcessSession, id: &str, prop: &str) -> String {
    s.eval(&format!(
        "getComputedStyle(document.getElementById('{id}'), '::marker').getPropertyValue('{prop}')"
    ))
    .expect("eval")
    .trim_matches('"')
    .to_owned()
}

#[test]
fn marker_computed_style_has_ua_defaults_not_the_items_inherited_values() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    for id in ["o-sym", "i-str", "o-mk", "i-mk"] {
        assert_eq!(cs(&mut s, id, "unicode-bidi"), "isolate", "{id}: unicode-bidi");
        assert_eq!(cs(&mut s, id, "font-variant-numeric"), "tabular-nums", "{id}: font-variant-numeric");
        assert_eq!(cs(&mut s, id, "text-transform"), "none", "{id}: text-transform");
        assert_eq!(cs(&mut s, id, "text-indent"), "0px", "{id}: text-indent");
    }
}

#[test]
fn marker_computed_style_reads_the_marker_rule() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(cs(&mut s, "o-mk", "content"), "\\\"marker\\\"");
    assert_eq!(cs(&mut s, "o-mk", "color"), "rgb(0, 128, 0)");
    assert_eq!(cs(&mut s, "o-mk", "font-size"), "20px");
    assert_eq!(cs(&mut s, "o-sym", "content"), "normal");
}

#[test]
fn marker_computed_display_and_size_are_the_marker_boxs() {
    let mut s = InProcessSession::new();
    s.navigate_html(PAGE).expect("navigate_html");
    assert_eq!(cs(&mut s, "i-str", "display"), "inline");
    assert_eq!(cs(&mut s, "i-mk", "display"), "inline");
    assert_eq!(cs(&mut s, "o-sym", "display"), "block");
    assert_eq!(cs(&mut s, "o-mk", "display"), "block");
    // The item is as wide as the page; the marker is a few em at most.
    let w: f32 = cs(&mut s, "o-sym", "width").trim_end_matches("px").parse().expect("width in px");
    assert!(w > 0.0 && w < 100.0, "marker width {w}");
}
