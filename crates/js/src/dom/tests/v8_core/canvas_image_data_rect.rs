//! `getImageData`/`putImageData`/`createImageData` по полосатой фикстуре
//! (BUG-454: допуск на шум RGB) — вынесено из `selectors_canvas_window.rs`
//! при упоре в потолок 2000 строк (docs/lint-policy.md §5.1). Фикстуры
//! `v8_core` видны через `use super::*;`.

use super::*;

/// BUG-454: `getImageData` is noised now (±1 per RGB channel, alpha never
/// perturbed — ADR-007 layer 4), so a test built before the fix that compares
/// exact colour bytes from a striped fixture needs tolerance at exactly the
/// positions that are RGB channels of an in-canvas pixel. `tolerant` lists
/// those positions (0-based) in the comma-separated fields of `actual`/`expected`;
/// every other position (dimensions, alpha, an out-of-canvas transparent pixel)
/// must still match exactly.
#[cfg(test)]
fn assert_csv_close(actual: &lumen_core::JsValue, expected: &str, tolerant: &[usize]) {
    let actual = match actual {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    };
    let a: Vec<i64> = actual.split(',').map(|s| s.parse().unwrap()).collect();
    let e: Vec<i64> = expected.split(',').map(|s| s.parse().unwrap()).collect();
    assert_eq!(a.len(), e.len(), "{actual} vs {expected}");
    for i in 0..a.len() {
        if tolerant.contains(&i) {
            assert!(
                (a[i] - e[i]).abs() <= 1,
                "field {i} outside ±1 noise tolerance: {actual} vs {expected}"
            );
        } else {
            assert_eq!(a[i], e[i], "field {i} mismatch: {actual} vs {expected}");
        }
    }
}

/// Builds a 6×1 canvas with three non-overlapping stripes at x = 0/2/4
/// (red/green/blue) and a transparent gap nowhere — the bug's own repro shape,
/// shrunk. Returns the runtime with `ctx` bound in the global scope.
#[cfg(test)]
fn striped_canvas_runtime() -> V8JsRuntime {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
         c.setAttribute('width', '6'); c.setAttribute('height', '2');\
         var ctx = c.getContext('2d');\
         ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 2, 2);\
         ctx.fillStyle = '#00ff00'; ctx.fillRect(2, 0, 2, 2);\
         ctx.fillStyle = '#0000ff'; ctx.fillRect(4, 0, 2, 2);\
         function rgba(x, y) {\
             var d = ctx.getImageData(x, y, 1, 1);\
             return d.width + ',' + d.height + ',' + d.data.length + ',' +\
                 d.data[0] + ',' + d.data[1] + ',' + d.data[2] + ',' + d.data[3];\
         }",
    )
    .unwrap();
    rt
}

#[test]
fn get_image_data_honours_its_rectangle() {
    // BUG-448: all four arguments were read only in the failure branch, so
    // every call answered with the whole canvas and every pixel probe read
    // (0, 0). `getImageData(0, 0, w, h)` — what the graphic tests use — was
    // the one accidentally correct call, which is why this stayed invisible.
    let rt = striped_canvas_runtime();
    for (x, expected) in [
        (1, "1,1,4,255,0,0,255"),
        (3, "1,1,4,0,255,0,255"),
        (5, "1,1,4,0,0,255,255"),
    ] {
        let got = rt.eval(&format!("rgba({x}, 0)")).unwrap();
        assert_csv_close(&got, expected, &[3, 4, 5]);
    }
    // A sub-rectangle carries exactly its own pixels, not the canvas's.
    let strip = rt
        .eval("var d = ctx.getImageData(2, 0, 3, 1); d.width + 'x' + d.height + ':' + d.data.length")
        .unwrap();
    assert_eq!(strip, lumen_core::JsValue::String("3x1:12".into()));
}

#[test]
fn get_image_data_pads_outside_the_canvas() {
    // Canvas §4.12.5.1.10: the rectangle may leave the bitmap; what is outside
    // is transparent black rather than clipped away or an error.
    let rt = striped_canvas_runtime();
    let outside = rt.eval("rgba(50, 50)").unwrap();
    assert_eq!(outside, lumen_core::JsValue::String("1,1,4,0,0,0,0".into()));
    let straddling = rt
        .eval("var d = ctx.getImageData(5, 0, 2, 1); d.data[0] + ',' + d.data[2] + ',' + d.data[7]")
        .unwrap();
    assert_csv_close(
        &straddling,
        "0,255,0",
        &[0, 1], // R/B of the in-canvas pixel; the third field is an out-of-canvas alpha (exact 0)
    );
    let before_origin = rt
        .eval("var d = ctx.getImageData(-1, 0, 2, 1); d.data[3] + ',' + d.data[4]")
        .unwrap();
    assert_csv_close(
        &before_origin,
        "0,255",
        &[1], // field 0 is an out-of-canvas alpha (exact 0), field 1 is an in-canvas R
    );
}

#[test]
fn get_image_data_argument_conversion_follows_webidl() {
    let rt = striped_canvas_runtime();
    let probe = |expr: &str| -> String {
        match rt
            .eval(&format!("(function(){{ try {{ {expr}; return 'ok'; }} catch (e) {{ return e.name; }} }})()"))
            .unwrap()
        {
            lumen_core::JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    };
    // `[EnforceRange] long`: non-finite is a TypeError, not a silent zero.
    assert_eq!(probe("ctx.getImageData(NaN, 0, 1, 1)"), "TypeError");
    assert_eq!(probe("ctx.getImageData(Infinity, 0, 1, 1)"), "TypeError");
    assert_eq!(probe("ctx.getImageData(0, 0, 1)"), "TypeError");
    // A zero-sized rectangle is an IndexSizeError; a negative one is legal and
    // gets normalized about its origin.
    assert_eq!(probe("ctx.getImageData(0, 0, 0, 1)"), "IndexSizeError");
    assert_eq!(probe("ctx.getImageData(0, 0, 1, 0)"), "IndexSizeError");
    let flipped = match rt
        .eval("var d = ctx.getImageData(2, 1, -2, -1); d.width + 'x' + d.height + ':' + d.data[0]")
        .unwrap()
    {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    };
    let (dims, r) = flipped.split_once(':').expect("dims:value shape");
    assert_eq!(dims, "2x1", "(2,1,-2,-1) is the rect at (0,0) sized 2x1, i.e. the red stripe");
    let r: i64 = r.parse().expect("numeric R channel");
    assert!((r - 255).abs() <= 1, "R channel outside ±1 noise tolerance: {r}");
    // A fractional coordinate truncates toward zero rather than rounding.
    let got = rt.eval("rgba(3.9, 0.9)").unwrap();
    assert_csv_close(&got, "1,1,4,0,255,0,255", &[3, 4, 5]);
}

#[test]
fn put_image_data_applies_the_dirty_rectangle() {
    // The 7-argument form was parsed and dropped, so a page repainting one
    // tile wrote the whole ImageData (BUG-448, "сопутствующий дефект").
    let rt = striped_canvas_runtime();
    rt.eval(
        "var src = ctx.createImageData(6, 2);\
         for (var i = 0; i < src.data.length; i += 4) {\
             src.data[i] = 1; src.data[i+1] = 2; src.data[i+2] = 3; src.data[i+3] = 255;\
         }\
         ctx.putImageData(src, 0, 0, 2, 0, 2, 1);",
    )
    .unwrap();
    assert_csv_close(&rt.eval("rgba(2, 0)").unwrap(), "1,1,4,1,2,3,255", &[3, 4, 5]);
    assert_csv_close(&rt.eval("rgba(1, 0)").unwrap(), "1,1,4,255,0,0,255", &[3, 4, 5]);
    assert_csv_close(&rt.eval("rgba(2, 1)").unwrap(), "1,1,4,0,255,0,255", &[3, 4, 5]);
}

#[test]
fn create_image_data_has_both_overloads() {
    // The copy form fell through to `w|0`/`h|0` on an ImageData object and
    // produced a 0×0 buffer (found by the pre-fix probe, not by the report).
    let rt = striped_canvas_runtime();
    let copy = rt
        .eval("var d = ctx.createImageData(ctx.getImageData(0, 0, 4, 2)); d.width + 'x' + d.height + ':' + d.data.length + ':' + d.data[0]")
        .unwrap();
    assert_eq!(copy, lumen_core::JsValue::String("4x2:32:0".into()));
    let sized = rt
        .eval("var d = ctx.createImageData(2, 3); d.width + 'x' + d.height + ':' + d.data.length")
        .unwrap();
    assert_eq!(sized, lumen_core::JsValue::String("2x3:24".into()));
    let zero = rt
        .eval("(function(){ try { ctx.createImageData(0, 0); return 'ok'; } catch (e) { return e.name; } })()")
        .unwrap();
    assert_eq!(zero, lumen_core::JsValue::String("IndexSizeError".into()));
    // Every ImageData the context hands out carries its colour space.
    assert_eq!(
        rt.eval("ctx.getImageData(0, 0, 1, 1).colorSpace + ',' + ctx.createImageData(1, 1).colorSpace")
            .unwrap(),
        lumen_core::JsValue::String("srgb,srgb".into())
    );
}
