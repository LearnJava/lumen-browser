//! BUG-382 — the layout/computed-style snapshot must reach the JS runtime as
//! soon as the page's primary layout exists, not only via a later relayout.
//!
//! `getComputedStyle()` and `getBoundingClientRect()` do not query the layout
//! engine themselves: both read a snapshot the embedder pushes into the runtime
//! (`update_computed_styles` / `update_layout_rects`). Before the fix the
//! headless `InProcessSession` never pushed either map, so **every** property
//! came back as `""` and every rect as all-zeros; in the shell the push existed
//! only inside the relayout path, so a fresh load returned data only when some
//! unrelated relayout happened to race ahead of the first script.
//!
//! The assertions below are by the broken property — a non-empty computed value
//! and a rect matching the box the layout engine actually produced — not by
//! snapshot equality, and they run over several fresh sessions because the
//! original symptom was intermittent (~3 loads out of 4).
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

/// `#stat` is 50×20 at (8, 8): `<body>` carries an 8px margin and the div is the
/// first in-flow block child.
const PAGE: &str = r#"<html><head><style>
  body { margin: 8px }
  #stat { width: 50px; height: 20px; color: red; position: relative; z-index: 3 }
</style></head><body><div id="stat">x</div></body></html>"#;

/// Strip the JSON quoting `BrowserSession::eval` applies to string results.
fn unquote(s: &str) -> String {
    s.trim_matches('"').to_owned()
}

#[test]
fn computed_style_is_published_after_navigation() {
    // Repeat: the shell-side symptom was a race, and a single green run proves
    // nothing about a delivery path that used to work one time in four.
    for attempt in 0..4 {
        let mut session = InProcessSession::new();
        session.navigate_html(PAGE).expect("navigate_html");

        let display = unquote(
            &session
                .eval("getComputedStyle(document.getElementById('stat')).display")
                .expect("eval display"),
        );
        assert_eq!(
            display, "block",
            "attempt {attempt}: getComputedStyle(...).display is empty — the \
             computed-style snapshot never reached the JS runtime (BUG-382)"
        );

        let z_index = unquote(
            &session
                .eval("getComputedStyle(document.getElementById('stat')).zIndex")
                .expect("eval zIndex"),
        );
        assert_eq!(z_index, "3", "attempt {attempt}: z-index lost on the way to JS");

        let color = unquote(
            &session
                .eval("getComputedStyle(document.getElementById('stat')).color")
                .expect("eval color"),
        );
        assert!(
            color.contains("255") && !color.is_empty(),
            "attempt {attempt}: color is {color:?}, expected an rgb() with 255"
        );
    }
}

#[test]
fn bounding_client_rect_is_published_after_navigation() {
    for attempt in 0..4 {
        let mut session = InProcessSession::new();
        session.navigate_html(PAGE).expect("navigate_html");

        let json = session
            .eval(
                "JSON.stringify((function () { \
                   var r = document.getElementById('stat').getBoundingClientRect(); \
                   return [r.x, r.y, r.width, r.height]; \
                 })())",
            )
            .expect("eval rect");
        // `eval` JSON-encodes the string returned by JSON.stringify → unwrap once.
        let inner = unquote(&json).replace('\\', "");
        let nums: Vec<f32> = inner
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        assert_eq!(nums.len(), 4, "attempt {attempt}: unparsable rect {json}");
        assert!(
            nums[2] > 0.0 && nums[3] > 0.0,
            "attempt {attempt}: getBoundingClientRect() is a zero rect {nums:?} — \
             the layout-rect snapshot never reached the JS runtime (BUG-382)"
        );
        // The rect JS sees must be the border box the layout engine produced —
        // compare against the engine's own answer instead of hardcoding numbers,
        // so the test stays about delivery and not about layout details.
        let engine = session
            .layout_box_by_selector("#stat")
            .expect("layout_box_by_selector")
            .expect("#stat has a box");
        let b = engine.border_box;
        assert_eq!(
            (nums[0], nums[1], nums[2], nums[3]),
            (b.x, b.y, b.width, b.height),
            "attempt {attempt}: JS rect disagrees with the layout engine"
        );
        assert_eq!((b.x, b.y, b.width), (8.0, 8.0, 50.0), "layout sanity: {b:?}");
    }
}

/// BUG-1400 — geometry of a descendant of a `content-visibility: hidden` box: the skipped subtree
/// is laid out on request, so `getBoundingClientRect`, `offset*` and `getClientRects` answer, and a
/// style edit is seen by the next query.
const HIDDEN_PAGE: &str = r#"<html><head><style>
  body { margin: 0 }
  #cv { content-visibility: hidden; width: 100px; margin-left: 20px }
</style></head><body><div id="cv"><div id="zz" style="height:40px;width:30px;margin-left:5px">t</div><span id="sp">text here</span></div></body></html>"#;

#[test]
fn hidden_subtree_geometry_is_answered_and_follows_style_edits() {
    let mut session = InProcessSession::new();
    session.navigate_html(HIDDEN_PAGE).expect("navigate_html");
    let mut probe = |js: &str| unquote(&session.eval(js).expect("eval"));

    assert_eq!(
        probe("var r = document.getElementById('zz').getBoundingClientRect(); [r.x, r.width, r.height].join()"),
        "25,30,40"
    );
    assert_eq!(
        probe("var z = document.getElementById('zz'); [z.offsetWidth, z.offsetHeight].join()"),
        "30,40"
    );
    assert_eq!(probe("document.getElementById('sp').getClientRects().length"), "1");
    assert_eq!(
        probe("var c = document.getElementById('cv').getBoundingClientRect(); [c.x, c.width, c.height].join()"),
        "20,100,0",
        "the hidden box itself stays size-contained"
    );
    probe("document.getElementById('zz').style.width = '60px'; 0");
    assert_eq!(probe("document.getElementById('zz').getBoundingClientRect().width"), "60");
}
