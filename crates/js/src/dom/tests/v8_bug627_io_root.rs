//! BUG-627 — `IntersectionObserver` honours an explicit `root` (element or
//! document) and `scrollMargin` (Intersection Observer §2.2 «root
//! intersection rectangle», §3.2.7 «compute the intersection», §3.2.10).
//! Geometry is pushed by hand the way the shell does after a relayout:
//! border-box rects, a computed-style map (border widths, `position`) and the
//! scroll-container set that marks a content clip.

use std::collections::HashMap;

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

fn nid(rt: &V8JsRuntime, id: &str) -> u32 {
    match rt
        .eval(&format!("document.getElementById('{id}').__nid__"))
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("#{id}: no nid ({other:?})"),
    }
}

/// One element's pushed geometry: border box, border width (all four sides),
/// `position`, and whether it clips its content (is a scroll container).
struct Geo<'a> {
    id: &'a str,
    rect: [f32; 4],
    border: f32,
    position: &'a str,
    clip: bool,
}

fn g<'a>(id: &'a str, rect: [f32; 4]) -> Geo<'a> {
    Geo {
        id,
        rect,
        border: 0.0,
        position: "static",
        clip: false,
    }
}

fn setup(html: &str, geos: &[Geo<'_>]) -> V8JsRuntime {
    let doc = Arc::new(Mutex::new(lumen_html_parser::parse(html)));
    let rt = v8_runtime_with_dom(doc);
    let mut rects = HashMap::new();
    let mut styles = HashMap::new();
    let mut scroll = HashMap::new();
    for geo in geos {
        let n = nid(&rt, geo.id);
        rects.insert(n, geo.rect);
        let b = format!("{}px", geo.border);
        let mut m = HashMap::new();
        for side in ["top", "right", "bottom", "left"] {
            m.insert(format!("border-{side}-width"), b.clone());
        }
        m.insert("position".to_string(), geo.position.to_string());
        styles.insert(n, m);
        if geo.clip {
            scroll.insert(n, [0.0, 0.0, geo.rect[2], geo.rect[3]]);
        }
    }
    rt.update_layout_rects(rects);
    rt.update_computed_styles(styles);
    rt.update_scroll_states(scroll);
    rt.update_viewport_size(800.0, 600.0);
    rt
}

fn observe(rt: &V8JsRuntime, options: &str) {
    rt.eval(&format!(
        "var _e = []; var io = new IntersectionObserver(function(es) {{ _e = _e.concat(es); }}, {options}); \
         io.observe(document.getElementById('target')); _lumen_deliver_intersection_observers();"
    ))
    .unwrap();
}

const SCROLLER: &str = r#"<div id="scroller"><div id="spacer"></div><div id="target"></div></div>"#;

/// WPT `scroll-margin.html`: 100×100 `overflow:hidden` scroller, target 50×50
/// right below its bottom edge. `scrollMargin: 10px` grows the clip by 10px,
/// exposing 10 of the target's 50 rows.
#[test]
fn scroll_margin_grows_an_intermediate_scroller_clip() {
    let geos = [
        Geo {
            clip: true,
            ..g("scroller", [8.0, 8.0, 100.0, 100.0])
        },
        g("spacer", [8.0, 8.0, 50.0, 100.0]),
        g("target", [8.0, 108.0, 50.0, 50.0]),
    ];
    let rt = setup(SCROLLER, &geos);
    observe(&rt, "{ scrollMargin: '10px' }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true"
    ));
    assert!(is_true(
        &rt,
        "Math.abs(_e[0].intersectionRatio - 0.2) < 1e-3"
    ));
    // rootMargin alone does not reach a clip that is not the root. WPT
    // `scroll-margin-zero.html` geometry: a 10px gap below the scroller
    // (a target flush against the clip edge would be edge-adjacent, which
    // §3.2.10 counts as intersecting).
    let gap = [
        Geo {
            clip: true,
            ..g("scroller", [8.0, 8.0, 100.0, 100.0])
        },
        g("spacer", [8.0, 8.0, 50.0, 110.0]),
        g("target", [8.0, 118.0, 50.0, 50.0]),
    ];
    let rt = setup(SCROLLER, &gap);
    observe(&rt, "{ rootMargin: '20px' }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false"
    ));
    let rt = setup(SCROLLER, &gap);
    observe(&rt, "{ scrollMargin: '0px' }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false && _e[0].intersectionRatio === 0"
    ));
}

/// WPT `scroll-margin-percent.html`: 10% of the 100px scroller is 10px.
#[test]
fn percentage_scroll_margin_resolves_against_the_scrollport() {
    let geos = [
        Geo {
            clip: true,
            ..g("scroller", [8.0, 8.0, 100.0, 100.0])
        },
        g("spacer", [8.0, 8.0, 50.0, 100.0]),
        g("target", [8.0, 108.0, 50.0, 50.0]),
    ];
    let rt = setup(SCROLLER, &geos);
    observe(&rt, "{ scrollMargin: '10%' }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && Math.abs(_e[0].intersectionRatio - 0.2) < 1e-3"
    ));
}

/// WPT `same-document-root.html` geometry: an explicit scrolling root with a
/// 3px border. rootBounds is its padding box, not the viewport, and a target
/// scrolled out of the root's scrollport does not intersect even though it is
/// inside the viewport.
#[test]
fn explicit_element_root_uses_its_padding_box() {
    let html = r#"<div id="root"><div id="filler"></div><div id="target"></div></div>"#;
    let root = Geo {
        border: 3.0,
        clip: true,
        ..g("root", [8.0, 100.0, 106.0, 206.0])
    };
    let rt = setup(
        html,
        &[
            root,
            g("filler", [11.0, 103.0, 100.0, 300.0]),
            g("target", [11.0, 403.0, 100.0, 100.0]),
        ],
    );
    observe(&rt, "{ root: document.getElementById('root') }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false"
    ));
    assert!(is_true(&rt,
        "var b = _e[0].rootBounds; b.left === 11 && b.right === 111 && b.top === 103 && b.bottom === 303"));
    assert!(is_true(
        &rt,
        "_e[0].intersectionRect.width === 0 && _e[0].intersectionRatio === 0"
    ));

    // Same root, target half inside the scrollport.
    let root = Geo {
        border: 3.0,
        clip: true,
        ..g("root", [8.0, 100.0, 106.0, 206.0])
    };
    let rt = setup(
        html,
        &[
            root,
            g("filler", [11.0, 103.0, 100.0, 150.0]),
            g("target", [11.0, 253.0, 100.0, 100.0]),
        ],
    );
    observe(
        &rt,
        "{ root: document.getElementById('root'), rootMargin: '10px 20% 40% 30px' }",
    );
    // Without the margin only the top half (253..303) is inside the padding
    // box; the 40% bottom margin (+80px) takes in the rest.
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true"
    ));
    assert!(is_true(
        &rt,
        "_e[0].intersectionRatio === 1 && _e[0].intersectionRect.bottom === 353"
    ));
    // WPT `root-margin-root-element.html`: 20%/40% resolve against the
    // padding box's width/height (100×200) → -30/+20 horizontally, -10/+80
    // vertically.
    assert!(is_true(&rt,
        "var b = _e[0].rootBounds; b.left === -19 && b.right === 131 && b.top === 93 && b.bottom === 383"));
}

/// A non-clipping explicit root (§2.2: «otherwise, the bounding box»), and
/// scrollMargin applied to a scroller between target and root while the root
/// itself clips nothing (WPT `scroll-margin-non-scrolling-root.html`).
#[test]
fn non_clipping_root_uses_its_border_box() {
    let html = r#"<div id="scroller"><div id="spacer"></div><div id="root"><div id="target"></div></div></div>"#;
    let geos = [
        Geo {
            clip: true,
            ..g("scroller", [8.0, 8.0, 100.0, 100.0])
        },
        g("spacer", [8.0, 8.0, 50.0, 110.0]),
        g("root", [8.0, 118.0, 100.0, 60.0]),
        g("target", [8.0, 118.0, 50.0, 50.0]),
    ];
    let rt = setup(html, &geos);
    observe(
        &rt,
        "{ root: document.getElementById('root'), scrollMargin: '10px' }",
    );
    // The scroller is outside the root, so it clips nothing here.
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true"
    ));
    assert!(is_true(&rt, "Math.abs(_e[0].intersectionRatio - 1) < 1e-6"));
    assert!(is_true(
        &rt,
        "_e[0].rootBounds.top === 118 && _e[0].rootBounds.height === 60"
    ));
}

/// §3.2.10 step 7 (WPT `not-in-containing-block-chain.html`,
/// `scroll-margin-not-contained.html`): a root that is not an ancestor of the
/// target on its containing-block chain never intersects, even when the
/// rects overlap.
#[test]
fn root_off_the_containing_block_chain_never_intersects() {
    let html = r#"<div id="target"></div><div id="root"></div>"#;
    let rt = setup(
        html,
        &[
            g("target", [10.0, 10.0, 100.0, 100.0]),
            g("root", [10.0, 10.0, 100.0, 100.0]),
        ],
    );
    observe(&rt, "{ root: document.getElementById('root') }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false"
    ));

    // An absolutely positioned scroller skips a static root on its way up.
    let html = r#"<div id="root"><div id="scroller"><div id="target"></div></div></div>"#;
    let rt = setup(
        html,
        &[
            g("root", [8.0, 8.0, 784.0, 0.0]),
            Geo {
                clip: true,
                position: "absolute",
                ..g("scroller", [8.0, 8.0, 100.0, 100.0])
            },
            g("target", [8.0, 8.0, 50.0, 50.0]),
        ],
    );
    observe(
        &rt,
        "{ root: document.getElementById('root'), scrollMargin: '10px' }",
    );
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false"
    ));
}

/// §3.2.10 step 6 (WPT `explicit-root-different-document.html`,
/// `cross-document-root.html`): a root in another document never intersects,
/// and the first entry still arrives.
#[test]
fn root_in_another_document_never_intersects() {
    let html = r#"<div id="target"></div>"#;
    let rt = setup(html, &[g("target", [10.0, 10.0, 100.0, 100.0])]);
    observe(
        &rt,
        "{ root: document.implementation.createHTMLDocument('') }",
    );
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === false"
    ));
    assert!(is_true(
        &rt,
        "_e[0].rootBounds.width === 0 && _e[0].intersectionRatio === 0"
    ));
    // The page's own document is the viewport, like the implicit root.
    let rt = setup(html, &[g("target", [10.0, 10.0, 100.0, 100.0])]);
    observe(&rt, "{ root: document }");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true"
    ));
    assert!(is_true(
        &rt,
        "_e[0].rootBounds.width === 800 && _e[0].rootBounds.height === 600"
    ));
}

/// §3.2.10: entries are queued on a change of threshold index or of
/// isIntersecting — including a target losing its box, which the old pass
/// skipped — and not again while neither changes.
#[test]
fn entries_follow_threshold_index_and_is_intersecting() {
    let html = r#"<div id="target"></div>"#;
    let rt = setup(html, &[g("target", [10.0, 10.0, 100.0, 100.0])]);
    observe(&rt, "{ threshold: [0, 0.5] }");
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true"
    ));
    // The box goes away (display:none): a not-intersecting entry follows.
    rt.update_layout_rects(HashMap::new());
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    assert!(is_true(
        &rt,
        "_e.length === 2 && _e[1].isIntersecting === false"
    ));
    assert!(is_true(&rt, "_e[1].boundingClientRect.width === 0"));
    // Zero-area target touching the viewport counts as fully visible.
    let t = nid(&rt, "target");
    rt.update_layout_rects([(t, [10.0, 10.0, 0.0, 0.0])].into_iter().collect());
    rt.eval("_lumen_deliver_intersection_observers();").unwrap();
    assert!(is_true(
        &rt,
        "_e.length === 3 && _e[2].isIntersecting === true && _e[2].intersectionRatio === 1"
    ));
}

/// WPT `v2/position-absolute-overflow-visible-and-not-visible.html`:
/// `overflow-y: clip; overflow-x: visible` clips only vertically, so a child
/// sticking out sideways stays fully visible.
#[test]
fn single_axis_clip_leaves_the_visible_axis_unclipped() {
    let html = r#"<div id="parent"><div id="target"></div></div>"#;
    let rt = setup(
        html,
        &[
            Geo {
                clip: true,
                ..g("parent", [0.0, 0.0, 200.0, 200.0])
            },
            g("target", [200.0, 0.0, 200.0, 200.0]),
        ],
    );
    let parent = nid(&rt, "parent");
    let target = nid(&rt, "target");
    let mut styles = HashMap::new();
    let mut p = HashMap::new();
    p.insert("overflow-x".to_string(), "visible".to_string());
    p.insert("overflow-y".to_string(), "clip".to_string());
    p.insert("position".to_string(), "relative".to_string());
    styles.insert(parent, p);
    let mut t = HashMap::new();
    t.insert("position".to_string(), "absolute".to_string());
    styles.insert(target, t);
    rt.update_computed_styles(styles);
    observe(&rt, "{}");
    assert!(is_true(
        &rt,
        "_e.length === 1 && _e[0].isIntersecting === true && _e[0].intersectionRatio === 1"
    ));
    assert!(is_true(
        &rt,
        "_e[0].intersectionRect.width === 200 && _e[0].intersectionRect.height === 200"
    ));
}
