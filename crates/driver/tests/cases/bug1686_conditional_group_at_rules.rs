//! BUG-1686 — `@keyframes`/`@counter-style` inside `@media`/`@supports` take effect only while
//! the enclosing condition holds, and a nested `@supports` inside `@media` reaches the cascade.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

fn animations(css: &str) -> String {
    let mut session = InProcessSession::new();
    session
        .navigate_html(&format!(
            "<html><head><style>{css}</style></head><body><div id=a style=\"margin-left:0\">x</div></body></html>"
        ))
        .expect("navigate_html");
    session.eval("String(document.getAnimations().length)").expect("eval")
}

#[test]
fn keyframes_inside_true_media_and_supports_animate() {
    let css = "#a{animation:k 1s infinite}\
        @media all{@keyframes k{from{margin-left:0}to{margin-left:50px}}}";
    assert_eq!(animations(css), "\"1\"");
    let css = "#a{animation:k 1s infinite}\
        @supports (color:red){@keyframes k{from{margin-left:0}to{margin-left:50px}}}";
    assert_eq!(animations(css), "\"1\"");
}

#[test]
fn keyframes_inside_false_media_do_not_animate() {
    let css = "#a{animation:k 1s infinite}\
        @media print{@keyframes k{from{margin-left:0}to{margin-left:50px}}}";
    assert_eq!(animations(css), "\"0\"");
}

#[test]
fn supports_nested_in_media_applies_in_the_cascade() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            "<html><head><style>\
             @media screen{@supports (color:red){#a{margin-left:33px}}}\
             @media print{@supports (color:red){#b{margin-left:44px}}}\
             </style></head><body><div id=a>x</div><div id=b>y</div></body></html>",
        )
        .expect("navigate_html");
    let v = session
        .eval(
            "getComputedStyle(document.getElementById('a')).marginLeft + ' ' + \
             getComputedStyle(document.getElementById('b')).marginLeft",
        )
        .expect("eval");
    assert_eq!(v, "\"33px 0px\"");
}
