use super::*;

fn with(f: impl FnOnce(&mut OsParams)) -> OsParams {
    let mut p = OsParams::DEFAULT;
    f(&mut p);
    p
}

#[test]
fn defaults_reproduce_the_old_constants() {
    let d = OsParams::DEFAULT;
    assert_eq!(d.double_click_ms, 500);
    assert_eq!(d.double_click_slop, (4.0, 4.0));
    assert_eq!(d.drag_threshold, (4.0, 4.0));
    // 3 lines per notch is 100 CSS px (Chromium), not the old 40.
    assert!((d.wheel_notch_px(800.0) - 100.0).abs() < 1e-4);
    assert_eq!(d, d.sanitized(), "the defaults must already be sanitized");
}

#[test]
fn drag_threshold_20_keeps_a_15px_move_a_click() {
    let p = with(|p| p.drag_threshold = (20.0, 20.0));
    assert!(!p.exceeds_drag(15.0, 0.0));
    assert!(!p.exceeds_drag(0.0, -15.0));
    assert!(p.exceeds_drag(20.0, 0.0));
    assert!(p.exceeds_drag(15.0, 15.0), "radial: 21.2 px is past 20");
    // And with the default 4 px the same move is a drag.
    assert!(OsParams::DEFAULT.exceeds_drag(15.0, 0.0));
    assert!(!OsParams::DEFAULT.exceeds_drag(3.0, 0.0));
    assert!(OsParams::DEFAULT.exceeds_drag(4.0, 0.0));
}

#[test]
fn six_wheel_lines_scroll_200_css_px() {
    let p = with(|p| p.wheel = WheelScroll::Lines(6));
    assert!((p.wheel_notch_px(800.0) - 200.0).abs() < 1e-4);
    let one = with(|p| p.wheel = WheelScroll::Lines(1));
    assert!((one.wheel_notch_px(800.0) - 100.0 / 3.0).abs() < 1e-4);
}

#[test]
fn page_wheel_scrolls_most_of_the_viewport() {
    let p = with(|p| p.wheel = WheelScroll::Page);
    assert!((p.wheel_notch_px(800.0) - 700.0).abs() < 1e-3);
    // A tiny viewport never yields a notch smaller than the default step.
    assert!((p.wheel_notch_px(40.0) - 100.0).abs() < 1e-4);
}

#[test]
fn double_click_follows_the_os_time_and_slop() {
    let slow = with(|p| p.double_click_ms = 1_500);
    assert!(slow.is_double_click(1_200.0, 1.0, 1.0));
    assert!(!OsParams::DEFAULT.is_double_click(1_200.0, 1.0, 1.0));
    assert!(!slow.is_double_click(1_501.0, 0.0, 0.0));
    assert!(!slow.is_double_click(-1.0, 0.0, 0.0), "time must not run backwards");

    let wide = with(|p| p.double_click_slop = (10.0, 6.0));
    assert!(wide.is_double_click(100.0, 9.0, 5.0));
    assert!(!wide.is_double_click(100.0, 11.0, 0.0));
    assert!(!wide.is_double_click(100.0, 0.0, 7.0), "axes are independent");
}

#[test]
fn sanitized_rejects_harmful_values() {
    let bad = OsParams {
        double_click_ms: 0,
        double_click_slop: (f32::NAN, -3.0),
        drag_threshold: (0.0, f32::INFINITY),
        wheel: WheelScroll::Lines(0),
        caret_width_px: f32::NAN,
        caret_blink_ms: Some(0),
        caret_timeout_ms: None,
        hover_ms: 0,
        reduce_motion: true,
    }
    .sanitized();
    assert!(bad.double_click_ms >= 100);
    assert_eq!(bad.double_click_slop, OsParams::DEFAULT.double_click_slop);
    assert_eq!(bad.drag_threshold, OsParams::DEFAULT.drag_threshold);
    assert_eq!(bad.wheel, OsParams::DEFAULT.wheel);
    assert_eq!(bad.caret_width_px, 1.0);
    assert!(bad.caret_blink_ms.is_some_and(|ms| ms >= 100));
    assert!(bad.hover_ms >= 10);
    assert!(bad.reduce_motion, "boolean settings pass through untouched");

    let huge = with(|p| {
        p.double_click_ms = u32::MAX;
        p.drag_threshold = (5_000.0, 5_000.0);
        p.wheel = WheelScroll::Lines(u32::MAX - 1);
    })
    .sanitized();
    assert_eq!(huge.double_click_ms, 5_000);
    assert_eq!(huge.drag_threshold, (64.0, 64.0));
    assert_eq!(huge.wheel, WheelScroll::Lines(100));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_reader_returns_sane_values() {
    let p = windows::read();
    assert!(p.double_click_ms >= 100);
    assert!(p.wheel_notch_px(800.0) > 0.0);
}
