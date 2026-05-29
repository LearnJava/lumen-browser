//! Test 27-direction-rtl.html — direction/RTL bar alignment via absolute position.
//!
//! Three 280x140 boxes (content-box + 2px border → 284x144) each hold a 60%-wide
//! (168px) bar. The LTR/end bars pin to the left edge; the RTL-start bar pins to
//! the right edge. Asserts the bars land at the expected box-relative x.

use lumen_driver::{BrowserSession, InProcessSession};

fn navigate(session: &mut InProcessSession, file: &str) {
    let root = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(root)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(file);
    session
        .navigate(&format!("file://{}", path.display()))
        .expect("navigate");
}

#[test]
fn test_27_direction_rtl() {
    let mut session = InProcessSession::new();
    navigate(&mut session, "graphic_tests/27-direction-rtl.html");

    let ltr = session.all_layout_boxes_by_selector(".ltr-box").expect("query .ltr-box");
    let rtl = session.all_layout_boxes_by_selector(".rtl-box").expect("query .rtl-box");
    let bars = session.all_layout_boxes_by_selector(".bar").expect("query .bar");

    assert_eq!(ltr.len(), 1, "expected 1 LTR box");
    assert_eq!(rtl.len(), 2, "expected 2 RTL boxes");
    assert_eq!(bars.len(), 3, "expected 3 bars");

    // Boxes are 280x140 content + 2px border → 284x144 border-box.
    for b in ltr.iter().chain(rtl.iter()) {
        assert!(
            (b.border_box.width - 284.0).abs() < 1.0 && (b.border_box.height - 144.0).abs() < 1.0,
            "box should be 284x144 (280+border), got {}x{}",
            b.border_box.width,
            b.border_box.height
        );
    }

    // Every bar is 60% × 280 = 168px wide, 30px tall.
    for (i, bar) in bars.iter().enumerate() {
        assert!(
            (bar.border_box.width - 168.0).abs() < 1.0 && (bar.border_box.height - 30.0).abs() < 1.0,
            "bar[{i}] should be 168x30, got {}x{}",
            bar.border_box.width,
            bar.border_box.height
        );
    }

    // Document order: bar[0]=ltr (left:0), bar[1]=rtl-start (right:0), bar[2]=rtl-end (left:0).
    let (bar_ltr, bar_rtl_start, bar_rtl_end) = (&bars[0], &bars[1], &bars[2]);

    // LTR bar pins to the inner-left edge of the LTR box (just inside the 2px border).
    assert!(
        (bar_ltr.border_box.x - (ltr[0].border_box.x + 2.0)).abs() < 1.5,
        "LTR bar should pin to left: bar.x={}, box.left+border={}",
        bar_ltr.border_box.x,
        ltr[0].border_box.x + 2.0
    );

    // RTL-start bar (right:0) pins to the inner-right edge of the first RTL box.
    let rtl0_inner_right = rtl[0].border_box.x + rtl[0].border_box.width - 2.0;
    let bar_start_right = bar_rtl_start.border_box.x + bar_rtl_start.border_box.width;
    assert!(
        (bar_start_right - rtl0_inner_right).abs() < 1.5,
        "RTL-start bar should pin to right: bar.right={bar_start_right}, box.inner_right={rtl0_inner_right}"
    );

    // RTL-end bar (left:0) pins to the inner-left edge of the second RTL box.
    assert!(
        (bar_rtl_end.border_box.x - (rtl[1].border_box.x + 2.0)).abs() < 1.5,
        "RTL-end bar should pin to left: bar.x={}, box.left+border={}",
        bar_rtl_end.border_box.x,
        rtl[1].border_box.x + 2.0
    );
}
