//! BUG-1671 — `getBoundingClientRect()` of a plain inline element that owns no
//! text fragment of its own: an empty `<span></span>` and an `<a>` whose only
//! child is a block. Before BUG-1558 both answered the all-zero rect JS gives
//! for "no geometry at all", indistinguishable from `display: none`.
//!
//! The geometry comes from the layout snapshot the embedder pushes, so the page
//! is laid out for real and the numbers below are read through the same path a
//! site's script sees. Chrome's answer for both ids is a zero-width rect one
//! line high, sitting where the element is in the line.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

const PAGE: &str = r##"<html><head><style>body { margin: 0; font-size: 20px }</style></head><body>
<p style="margin:0">aaa<span id="e"></span>bbb</p>
<p style="margin:0">aaa <a id="b" href="#"><div>blk</div></a></p>
</body></html>"##;

fn rect(session: &mut InProcessSession, js: &str) -> [f32; 4] {
    let json = session
        .eval(&format!(
            "JSON.stringify((function () {{ var r = {js}; return [r.x, r.y, r.width, r.height]; }})())"
        ))
        .expect("eval rect");
    let inner = json.trim_matches('"').replace('\\', "");
    let nums: Vec<f32> = inner
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    assert_eq!(nums.len(), 4, "unparsable rect {json}");
    [nums[0], nums[1], nums[2], nums[3]]
}

#[test]
fn empty_inline_and_block_in_inline_get_a_line_rect() {
    let mut session = InProcessSession::new();
    session.navigate_html(PAGE).expect("navigate_html");

    let e = rect(&mut session, "document.getElementById('e').getBoundingClientRect()");
    assert_eq!(e[2], 0.0, "an empty inline is zero-wide: {e:?}");
    assert!(e[3] > 0.0, "an empty inline is one line high: {e:?}");
    assert!(e[0] > 0.0, "it sits after the text before it: {e:?}");

    let b = rect(&mut session, "document.getElementById('b').getBoundingClientRect()");
    assert_eq!(b[2], 0.0, "the block-in-inline slice is zero-wide: {b:?}");
    assert!(b[3] > 0.0, "and one line high: {b:?}");
    assert!(b[0] > 0.0, "and sits after the text before it: {b:?}");

    // `getClientRects()` must answer a non-empty list, not `[]`.
    let len = session
        .eval("document.getElementById('e').getClientRects().length")
        .expect("eval client rects");
    assert_eq!(len, "1", "#e getClientRects(): {len}");
    let len = session
        .eval("document.getElementById('b').getClientRects().length")
        .expect("eval client rects");
    assert_ne!(len, "0", "#b getClientRects() is empty");
}
