//! BUG-454: `getImageData`/`toDataURL`/`toBlob` all noised, sized and typed
//! honestly (ADR-007 layer 4). Before this fix `getImageData` was the one
//! unblanked read channel (fingerprintjs and friends read it whenever
//! `toDataURL` came back blank), and `toDataURL`/`toBlob` returned a fixed
//! 1×1 PNG regardless of the canvas's real size or the requested type.

use super::*;

fn s(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> String {
    match rt.eval(&format!("String({expr})")) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => panic!("expected string, got {other:?}"),
    }
}

fn num(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> f64 {
    match rt.eval(&format!("Number({expr})")) {
        Ok(lumen_core::JsValue::Number(n)) => n,
        other => panic!("expected number, got {other:?}"),
    }
}

fn bool_(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> bool {
    matches!(rt.eval(expr), Ok(lumen_core::JsValue::Bool(true)))
}

/// Fixture: a 60×30 canvas filled solid `#123456`, `ctx`/`c` bound in JS.
fn painted_canvas(rt: &crate::v8_runtime::V8JsRuntime) {
    rt.eval(
        "var c = document.createElement('canvas'); c.width = 60; c.height = 30;\
         var ctx = c.getContext('2d'); ctx.fillStyle = '#123456';\
         ctx.fillRect(0, 0, 60, 30);",
    )
    .unwrap();
}

#[test]
fn get_image_data_perturbs_within_one_unit_and_is_stable() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    // #123456 = 18, 52, 86 — every noised channel must land within ±1, and a
    // second read of the same rectangle must land on the exact same bytes
    // (property 1 of `CanvasNoiseGenerator`: positional, not sequential).
    let first = s(&rt, "Array.from(ctx.getImageData(0,0,1,1).data)");
    let second = s(&rt, "Array.from(ctx.getImageData(0,0,1,1).data)");
    assert_eq!(first, second, "same rect must noise identically on repeat reads");
    let bytes: Vec<i64> = first
        .split(',')
        .map(|b| b.parse().unwrap())
        .collect();
    assert_eq!(bytes.len(), 4);
    assert!((bytes[0] - 18).abs() <= 1, "R out of range: {bytes:?}");
    assert!((bytes[1] - 52).abs() <= 1, "G out of range: {bytes:?}");
    assert!((bytes[2] - 86).abs() <= 1, "B out of range: {bytes:?}");
    assert_eq!(bytes[3], 255, "alpha must never be perturbed");
}

#[test]
fn to_data_url_encodes_the_real_bitmap_at_its_own_size() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    let url = s(&rt, "c.toDataURL()");
    assert!(url.starts_with("data:image/png;base64,"), "not a PNG data URL: {url}");
    let b64 = url.trim_start_matches("data:image/png;base64,");
    let png = crate::sw_worker::base64_decode(b64)
        .unwrap_or_else(|| panic!("invalid base64 in {url}"));
    let img = lumen_image::decode(&png).expect("must decode as a real PNG");
    assert_eq!(img.width, 60, "toDataURL must encode the canvas's real width");
    assert_eq!(img.height, 30, "toDataURL must encode the canvas's real height");
}

#[test]
fn to_data_url_never_lies_about_an_unsupported_type() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    // No JPEG/WebP encoder exists — HTML LS §4.12.5.7 allows falling back to
    // PNG, but the returned string's own MIME must say so honestly.
    for requested in ["image/jpeg", "image/webp", "bogus/type"] {
        let url = s(&rt, &format!("c.toDataURL('{requested}')"));
        assert!(
            url.starts_with("data:image/png;base64,"),
            "toDataURL('{requested}') must honestly report image/png, got: {url}"
        );
    }
}

#[test]
fn to_data_url_differs_for_an_empty_vs_a_painted_canvas() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    let equal = bool_(
        &rt,
        "(function(){var e=document.createElement('canvas');e.width=60;e.height=30;\
          e.getContext('2d');return e.toDataURL()===c.toDataURL();})()",
    );
    assert!(!equal, "an empty and a painted canvas must not encode identically");
}

#[test]
fn to_blob_delivers_a_correctly_sized_png_blob_as_a_queued_task() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    rt.eval(
        "var _b454_result = null;\
         c.toBlob(function(b) { _b454_result = b; });",
    )
    .unwrap();
    assert!(
        bool_(&rt, "_b454_result === null"),
        "toBlob's callback must not fire synchronously (HTML LS §4.12.5.7 step 3)"
    );
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(bool_(&rt, "_b454_result !== null"), "toBlob's queued task never ran");
    assert!(
        bool_(&rt, "_b454_result instanceof Blob"),
        "toBlob must deliver a real Blob, not null"
    );
    assert_eq!(s(&rt, "_b454_result.type"), "image/png");
    assert!(num(&rt, "_b454_result.size") > 100.0, "blob must carry a real (non-stub) PNG");
}

#[test]
fn offscreen_and_transferred_canvases_share_the_page_canvas_noise() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    let page = s(&rt, "Array.from(ctx.getImageData(0,0,1,1).data)");

    let offscreen = s(
        &rt,
        "(function(){var o=new OffscreenCanvas(8,8);var oc=o.getContext('2d');\
          oc.fillStyle='#123456';oc.fillRect(0,0,8,8);\
          return Array.from(oc.getImageData(0,0,1,1).data);})()",
    );
    assert_eq!(
        page, offscreen,
        "OffscreenCanvas must derive the same per-session+origin seed as the page's own canvases"
    );

    let transferred = s(
        &rt,
        "(function(){var t=document.createElement('canvas');t.width=8;t.height=8;\
          var o=t.transferControlToOffscreen();var oc=o.getContext('2d');\
          oc.fillStyle='#123456';oc.fillRect(0,0,8,8);\
          return Array.from(oc.getImageData(0,0,1,1).data);})()",
    );
    assert_eq!(
        page, transferred,
        "transferControlToOffscreen's snapshot must keep the same noise seed as the source canvas"
    );
}

#[test]
fn draw_image_of_a_noised_canvas_reads_back_with_the_same_noise() {
    let rt = v8_runtime_with_dom(make_doc());
    painted_canvas(&rt);
    let direct = s(&rt, "Array.from(ctx.getImageData(0,0,1,1).data)");
    let via_draw_image = s(
        &rt,
        "(function(){var d=document.createElement('canvas');d.width=60;d.height=30;\
          var dc=d.getContext('2d');dc.drawImage(c,0,0);\
          return Array.from(dc.getImageData(0,0,1,1).data);})()",
    );
    assert_eq!(direct, via_draw_image);
}
