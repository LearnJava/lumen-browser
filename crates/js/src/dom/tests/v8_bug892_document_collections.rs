//! BUG-892 — `document.forms`/`scripts`/`links`/`embeds`/`plugins`/`anchors`
//! were `undefined` while `document.images` worked, so `document.scripts.length`
//! threw inside the AWS WAF challenge and `document.links.length` inside
//! Webflow. HTML LS §3.1.5: each is a live `[SameObject]` HTMLCollection.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc, "", None, None, None, None, None, None, None, None, None, false,
    )
    .unwrap();
    rt
}

fn num(rt: &V8JsRuntime, code: &str) -> f64 {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Number(n) => n,
        other => panic!("{code}: expected a number, got {other:?}"),
    }
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Appends `<tag attr=value…>` under `#main` and returns nothing; the element
/// is left in the global `_last`.
fn add(rt: &V8JsRuntime, tag: &str, attrs: &[(&str, &str)]) {
    let mut code = format!("var _last = document.createElement('{tag}');");
    for (k, v) in attrs {
        code.push_str(&format!("_last.setAttribute('{k}', '{v}');"));
    }
    code.push_str("document.getElementById('main').appendChild(_last);");
    rt.eval(&code).unwrap();
}

#[test]
fn all_collections_are_html_collections_and_same_object() {
    let rt = v8_runtime_with_dom(make_doc());
    for name in [
        "images", "forms", "scripts", "links", "embeds", "plugins", "anchors", "applets",
    ] {
        assert!(
            is_true(&rt, &format!("document.{name} instanceof HTMLCollection")),
            "document.{name} is not an HTMLCollection"
        );
        assert!(
            is_true(&rt, &format!("document.{name} === document.{name}")),
            "document.{name} is not [SameObject]"
        );
        assert_eq!(
            num(&rt, &format!("document.{name}.length")),
            0.0,
            "document.{name}"
        );
    }
    assert!(is_true(&rt, "document.plugins === document.embeds"));
}

#[test]
fn forms_are_live_and_named() {
    let rt = v8_runtime_with_dom(make_doc());
    let forms = "var _forms = document.forms; _forms.length";
    assert_eq!(num(&rt, forms), 0.0);
    add(&rt, "form", &[("name", "fm1")]);
    // The collection captured before the insertion sees it (liveness).
    assert_eq!(num(&rt, "_forms.length"), 1.0);
    assert!(is_true(&rt, "document.forms.fm1 === _last"));
    assert!(is_true(&rt, "document.forms.namedItem('fm1') === _last"));
    assert!(is_true(&rt, "document.forms[0] === _last"));
}

#[test]
fn scripts_embeds_and_anchors_follow_their_elements() {
    let rt = v8_runtime_with_dom(make_doc());
    add(&rt, "script", &[]);
    add(&rt, "script", &[]);
    add(&rt, "embed", &[]);
    add(&rt, "a", &[("name", "top")]);
    add(&rt, "a", &[("href", "/x")]);
    assert_eq!(num(&rt, "document.scripts.length"), 2.0);
    assert_eq!(num(&rt, "document.embeds.length"), 1.0);
    assert_eq!(num(&rt, "document.plugins.length"), 1.0);
    // `anchors` is `a[name]` only; the `href`-only `a` is not an anchor.
    assert_eq!(num(&rt, "document.anchors.length"), 1.0);
    assert!(is_true(
        &rt,
        "document.anchors.top.getAttribute('name') === 'top'"
    ));
}

#[test]
fn links_are_a_and_area_with_href_in_tree_order() {
    let rt = v8_runtime_with_dom(make_doc());
    add(&rt, "a", &[("name", "no-href")]);
    add(&rt, "area", &[("href", "/1"), ("id", "first")]);
    add(&rt, "a", &[("href", "/2"), ("id", "second")]);
    assert_eq!(num(&rt, "document.links.length"), 2.0);
    // Tree order, not selector order (`a[href], area[href]` lists `a` first).
    assert!(is_true(&rt, "document.links[0].id === 'first'"));
    assert!(is_true(&rt, "document.links[1].id === 'second'"));
    // The Webflow loop that used to throw `reading 'length'`.
    assert_eq!(
        num(
            &rt,
            "var _n = 0; for (var a = document.links, l = 0; l < a.length; l++) _n++; _n"
        ),
        2.0
    );
}
