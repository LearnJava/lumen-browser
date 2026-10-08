//! V8 test coverage for [`install_canvas2d_bindings_v8`] (S12b-B30; the rquickjs
//! suite this ports from was removed in the same batch): 26 of the 31 original
//! tests (the other 5 — `parse_canvas_font_size`/`measure_text_width`/
//! `present_rgba_writes_pixels_and_marks_dirty` — are pure Rust with no JS engine
//! involved, kept once in `mod tests` above, already covering both engines).
//!
//! [`V8JsRuntime::new`] spawns a dedicated OS thread per runtime, so a bare
//! rquickjs-style peek at `CANVASES`/`DIRTY` from the test's own thread would see
//! an empty, unrelated `thread_local!` instance. Dirty-buffer assertions go
//! through the already-public [`V8JsRuntime::flush_canvas_updates`]; assertions
//! with no JS-visible getter native (`line_width`, `global_alpha`, `text_align`,
//! `text_baseline`) go through the new test-only [`V8JsRuntime::run_for_test`],
//! which runs an arbitrary closure on the JS thread.

// `panic!` — штатный способ провалить тест; исключение из clippy.toml не
// достаёт до хелперов модуля (docs/lint-policy.md §10).
#![allow(clippy::panic, clippy::unwrap_used)]
use lumen_core::JsValue;
use lumen_core::ext::JsRuntime as _;

use crate::v8_runtime::V8JsRuntime;

fn with_canvas2d() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    super::install_canvas2d_bindings_v8(&rt, "https://example.test").unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
    matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
}

fn str_eval(rt: &V8JsRuntime, expr: &str) -> String {
    match rt.eval(expr).unwrap() {
        JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

/// Evaluate an expression yielding the RGBA byte array a `getImageData`
/// native returns (`JsValue::Array` of numbers).
fn bytes_eval(rt: &V8JsRuntime, expr: &str) -> Vec<u8> {
    match rt.eval(expr).unwrap() {
        JsValue::Array(items) => items
            .into_iter()
            .map(|v| match v {
                JsValue::Number(n) => n as u8,
                other => panic!("expected byte, got {other:?}"),
            })
            .collect(),
        other => panic!("expected array, got {other:?}"),
    }
}

/// BUG-454: `with_canvas2d()` now runs behind a real (armed) noise
/// generator, so any RGBA bytes a test reads through `getImageData` may
/// drift by ±1 per colour channel (alpha is never perturbed) — this
/// compares actual RGBA8 pixels (one or more, back to back) against
/// expected ones under that tolerance, for tests whose subject is
/// cropping/addressing rather than exact colour reproduction. A
/// transparent-black pixel (alpha 0, e.g. outside the canvas) is exempt
/// from noise by construction (property 3 of `CanvasNoiseGenerator`), so
/// it is compared exactly like every alpha byte.
fn assert_rgba_close(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len(), "byte count mismatch: {actual:?} vs {expected:?}");
    assert_eq!(actual.len() % 4, 0, "not whole RGBA8 pixels: {actual:?}");
    for (a_px, e_px) in actual.chunks_exact(4).zip(expected.chunks_exact(4)) {
        assert_eq!(a_px[3], e_px[3], "alpha must be exact: {actual:?} vs {expected:?}");
        for i in 0..3 {
            assert!(
                (i64::from(a_px[i]) - i64::from(e_px[i])).abs() <= 1,
                "channel {i} outside ±1 noise tolerance: {actual:?} vs {expected:?}"
            );
        }
    }
}

fn num_eval(rt: &V8JsRuntime, expr: &str) -> f64 {
    match rt.eval(expr).unwrap() {
        JsValue::Number(n) => n,
        other => panic!("expected number, got {other:?}"),
    }
}

#[test]
fn js_create_registers_context() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(7, 100, 50);").unwrap();
    let dims = rt.run_for_test(|| super::with_canvas(7, |c| (c.width(), c.height())));
    assert_eq!(dims, (100, 50));
}

#[test]
fn js_create_clamps_dimensions() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(1, 0, 99999);").unwrap();
    let dims = rt.run_for_test(|| super::with_canvas(1, |c| (c.width(), c.height())));
    assert_eq!(
        dims,
        (1, super::MAX_CANVAS_DIM),
        "zero clamped up to 1, oversized clamped to max"
    );
}

#[test]
fn js_create_is_idempotent_preserving_buffer() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(3, 10, 10);\
         _lumen_canvas2d_set_fill_style(3, '#ff0000');\
         _lumen_canvas2d_fill_rect(3, 0, 0, 10, 10);\
         _lumen_canvas2d_create(3, 10, 10);",
    )
    .unwrap();
    // Re-create must not wipe an existing buffer (entry().or_insert).
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(3, 0, 0, 1, 1)"),
        &[255, 0, 0, 255],
    );
}

#[test]
fn js_fill_rect_marks_dirty_and_paints() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(2, 4, 4);\
         _lumen_canvas2d_set_fill_style(2, 'rgb(0,255,0)');\
         _lumen_canvas2d_fill_rect(2, 0, 0, 4, 4);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1);
    let (nid, w, h, rgba) = &updates[0];
    assert_eq!(*nid, 2);
    assert_eq!((*w, *h), (4, 4));
    assert_eq!(rgba[1], 255, "green channel painted");
}

#[test]
fn js_flush_dirty_drains_once() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(5, 4, 4);\
         _lumen_canvas2d_fill_rect(5, 0, 0, 4, 4);",
    )
    .unwrap();
    assert_eq!(rt.flush_canvas_updates().len(), 1);
    assert!(rt.flush_canvas_updates().is_empty(), "second drain is empty");
}

#[test]
fn js_clear_rect_marks_dirty() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(8, 4, 4);\
         _lumen_canvas2d_set_fill_style(8, '#0000ff');\
         _lumen_canvas2d_fill_rect(8, 0, 0, 4, 4);",
    )
    .unwrap();
    let _ = rt.flush_canvas_updates();
    rt.eval("_lumen_canvas2d_clear_rect(8, 0, 0, 4, 4);").unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].3[3], 0, "alpha cleared to transparent");
}

#[test]
fn js_path_fill_marks_dirty() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(9, 20, 20);\
         _lumen_canvas2d_set_fill_style(9, '#ffffff');\
         _lumen_canvas2d_begin_path(9);\
         _lumen_canvas2d_move_to(9, 0, 0);\
         _lumen_canvas2d_line_to(9, 20, 0);\
         _lumen_canvas2d_line_to(9, 20, 20);\
         _lumen_canvas2d_close_path(9);\
         _lumen_canvas2d_fill(9);",
    )
    .unwrap();
    assert_eq!(rt.flush_canvas_updates().len(), 1);
}

#[test]
fn js_stroke_marks_dirty_without_path_ops() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(10, 8, 8);\
         _lumen_canvas2d_begin_path(10);\
         _lumen_canvas2d_move_to(10, 0, 0);\
         _lumen_canvas2d_line_to(10, 8, 8);\
         _lumen_canvas2d_stroke(10);",
    )
    .unwrap();
    assert_eq!(rt.flush_canvas_updates().len(), 1);
}

#[test]
fn js_arc_does_not_mark_dirty_until_fill() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(11, 20, 20);\
         _lumen_canvas2d_begin_path(11);\
         _lumen_canvas2d_arc(11, 10, 10, 5, 0, 6.28, false);",
    )
    .unwrap();
    assert!(rt.flush_canvas_updates().is_empty(), "path building alone is not dirty");
}

#[test]
fn js_line_width_rejects_invalid() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(12, 4, 4);\
         _lumen_canvas2d_set_line_width(12, 3.5);\
         _lumen_canvas2d_set_line_width(12, -1);\
         _lumen_canvas2d_set_line_width(12, 0);",
    )
    .unwrap();
    let line_width = rt.run_for_test(|| super::with_canvas(12, |c| c.line_width));
    assert_eq!(line_width, 3.5_f32, "invalid widths ignored");
}

#[test]
fn js_global_alpha_clamped_to_unit_range() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(13, 4, 4);\
         _lumen_canvas2d_set_global_alpha(13, 0.5);\
         _lumen_canvas2d_set_global_alpha(13, 2.0);\
         _lumen_canvas2d_set_global_alpha(13, -0.5);",
    )
    .unwrap();
    let alpha = rt.run_for_test(|| super::with_canvas(13, |c| c.global_alpha));
    assert_eq!(alpha, 0.5_f32, "out-of-range ignored");
}

#[test]
fn js_resize_clears_and_marks_dirty() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(14, 4, 4);\
         _lumen_canvas2d_resize(14, 16, 8);",
    )
    .unwrap();
    let (w, h) = rt.run_for_test(|| super::with_canvas(14, |c| (c.width(), c.height())));
    assert_eq!((w, h), (16, 8));
    assert_eq!(rt.flush_canvas_updates().len(), 1);
}

#[test]
fn js_get_image_data_returns_only_the_requested_rect() {
    // BUG-448: the native used to take a bare `nid` and answer with the
    // whole bitmap, so every rectangle read the origin. The rect is a
    // parameter now, and its size decides the payload's size.
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(15, 4, 2);").unwrap();
    assert_eq!(
        bytes_eval(&rt, "_lumen_canvas2d_get_image_data(15, 0, 0, 4, 2)").len(),
        4 * 2 * 4,
        "whole canvas is 4x2 RGBA"
    );
    assert_eq!(
        bytes_eval(&rt, "_lumen_canvas2d_get_image_data(15, 1, 1, 1, 1)").len(),
        4,
        "a one-pixel read costs one pixel"
    );
}

#[test]
fn js_get_image_data_reads_the_addressed_pixel() {
    // The bug's own repro: three non-overlapping stripes, each read at its
    // own x. Before the fix all three answered with pixel (0, 0).
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(16, 6, 1);\
         _lumen_canvas2d_set_fill_style(16, '#ff0000');\
         _lumen_canvas2d_fill_rect(16, 0, 0, 2, 1);\
         _lumen_canvas2d_set_fill_style(16, '#00ff00');\
         _lumen_canvas2d_fill_rect(16, 2, 0, 2, 1);\
         _lumen_canvas2d_set_fill_style(16, '#0000ff');\
         _lumen_canvas2d_fill_rect(16, 4, 0, 2, 1);",
    )
    .unwrap();
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 0, 0, 1, 1)"),
        &[255, 0, 0, 255],
    );
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 2, 0, 1, 1)"),
        &[0, 255, 0, 255],
    );
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(16, 4, 0, 1, 1)"),
        &[0, 0, 255, 255],
    );
}

#[test]
fn js_get_image_data_outside_the_canvas_is_transparent_black() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(17, 2, 2);\
         _lumen_canvas2d_set_fill_style(17, '#ff0000');\
         _lumen_canvas2d_fill_rect(17, 0, 0, 2, 2);",
    )
    .unwrap();
    assert_eq!(
        bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, 5, 5, 1, 1)"),
        vec![0, 0, 0, 0],
        "wholly outside"
    );
    // Straddling the right edge: first pixel is real, second is outside.
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, 1, 0, 2, 1)"),
        &[255, 0, 0, 255, 0, 0, 0, 0],
    );
    // A negative origin is legal and pads on the near side.
    assert_rgba_close(
        &bytes_eval(&rt, "_lumen_canvas2d_get_image_data(17, -1, 0, 2, 1)"),
        &[0, 0, 0, 0, 255, 0, 0, 255],
    );
}

#[test]
fn js_get_image_data_unknown_canvas_is_empty() {
    let rt = with_canvas2d();
    assert!(bytes_eval(&rt, "_lumen_canvas2d_get_image_data(999, 0, 0, 1, 1)").is_empty());
}

#[test]
fn js_ops_on_unknown_canvas_are_noops() {
    let rt = with_canvas2d();
    // No create() — every op should silently no-op, no panic.
    rt.eval(
        "_lumen_canvas2d_fill_rect(404, 0, 0, 4, 4);\
         _lumen_canvas2d_set_fill_style(404, '#fff');\
         _lumen_canvas2d_fill(404);",
    )
    .unwrap();
    // fill_rect/fill mark dirty, but flush finds no context → empty.
    assert!(rt.flush_canvas_updates().is_empty());
}

#[test]
fn js_two_canvases_isolated() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(20, 4, 4);\
         _lumen_canvas2d_create(21, 8, 8);\
         _lumen_canvas2d_set_fill_style(20, '#ff0000');\
         _lumen_canvas2d_fill_rect(20, 0, 0, 4, 4);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    // Only canvas 20 was drawn; 21 stays clean.
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 20);
}

#[test]
fn js_fill_text_marks_canvas_dirty() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(30, 200, 50);\
         _lumen_canvas2d_fill_text(30, 'Hi', 10.0, 30.0);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "fillText should mark canvas dirty");
    assert_eq!(updates[0].0, 30);
}

#[test]
fn js_fill_text_rasterizes_non_transparent_pixels() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(31, 200, 60);\
         _lumen_canvas2d_set_font(31, '20px sans-serif');\
         _lumen_canvas2d_set_fill_style(31, '#000000');\
         _lumen_canvas2d_fill_text(31, 'X', 10.0, 40.0);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert!(!updates.is_empty(), "should produce a dirty buffer");
    let any_inked = updates[0].3.chunks(4).any(|px| px[3] > 0);
    assert!(any_inked, "fillText('X') should produce non-transparent pixels");
}

#[test]
fn js_set_text_align_stored_in_canvas_state() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(32, 100, 50);\
         _lumen_canvas2d_set_text_align(32, 'center');",
    )
    .unwrap();
    let align = rt.run_for_test(|| super::with_canvas(32, |c| c.text_align.clone()));
    assert_eq!(align, "center");
}

#[test]
fn js_set_text_baseline_stored_in_canvas_state() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(33, 100, 50);\
         _lumen_canvas2d_set_text_baseline(33, 'top');",
    )
    .unwrap();
    let baseline = rt.run_for_test(|| super::with_canvas(33, |c| c.text_baseline.clone()));
    assert_eq!(baseline, "top");
}

#[test]
fn js_measure_text_via_binding_uses_font_size() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(34, 200, 50);").unwrap();
    // 10px (default)
    let w10 = num_eval(&rt, "_lumen_canvas2d_measure_text(34, 'A');");
    // 20px
    rt.eval("_lumen_canvas2d_set_font(34, '20px sans-serif');").unwrap();
    let w20 = num_eval(&rt, "_lumen_canvas2d_measure_text(34, 'A');");
    assert!(w10 > 0.0, "10px width should be positive");
    assert!(w20 > w10 * 1.5, "20px should be roughly 2× 10px: {w20} vs {w10}");
}

#[test]
fn js_stroke_text_marks_canvas_dirty() {
    let rt = with_canvas2d();
    rt.eval(
        "_lumen_canvas2d_create(35, 200, 50);\
         _lumen_canvas2d_stroke_text(35, 'T', 10.0, 30.0);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "strokeText should mark canvas dirty");
}

#[test]
fn js_transfer_control_creates_offscreen_canvas_id() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(50, 40, 30);").unwrap();
    let raw = str_eval(&rt, "_lumen_canvas_transfer_control_to_offscreen(50)");
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(parsed["__canvas_id__"].as_u64().unwrap() > 0, "should have a canvas ID");
    assert_eq!(parsed["width"].as_u64().unwrap(), 40);
    assert_eq!(parsed["height"].as_u64().unwrap(), 30);
}

#[test]
fn js_transfer_control_marks_as_transferred() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(51, 10, 10);").unwrap();
    let before = bool_eval(&rt, "_lumen_canvas_is_transferred(51)");
    assert!(!before, "not transferred yet");
    rt.eval("_lumen_canvas_transfer_control_to_offscreen(51);").unwrap();
    let after = bool_eval(&rt, "_lumen_canvas_is_transferred(51)");
    assert!(after, "should be marked as transferred");
}

#[test]
fn js_is_transferred_false_for_unknown_nid() {
    let rt = with_canvas2d();
    assert!(!bool_eval(&rt, "_lumen_canvas_is_transferred(999999)"));
}

#[test]
fn js_present_rgba_resizes_existing_canvas() {
    let rt = with_canvas2d();
    rt.eval("_lumen_canvas2d_create(78, 4, 4);").unwrap();
    let _ = rt.flush_canvas_updates();
    // A present at a different size resizes the backing buffer to match the frame.
    let frame = [1u8, 2, 3, 4, 5, 6, 7, 8];
    rt.run_for_test(move || super::present_rgba(78, 2, 1, &frame));
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1);
    let (_, w, h, pixels) = &updates[0];
    assert_eq!((*w, *h), (2, 1), "canvas resized to the presented frame");
    assert_eq!(pixels, &frame);
}
