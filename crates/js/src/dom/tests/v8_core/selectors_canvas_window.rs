//! Вторая половина модуля `v8_core`, отделённая при выносе из `dom.rs`
//! (дорожка SPLIT, батч JS-2): исходные 3 244 строки — единственный модуль
//! группы над потолком в 2 000 строк, шов проведён по границе теста.
//!
//! Здесь селекторы (`querySelector`/`matches`/`closest`), Canvas 2D вместе с
//! CSS-ресайзом, и поверхность окна: таймеры (BUG-831/BUG-847), History API и
//! отчёт о неперехваченных исключениях (BUG-591). Фикстуры и помощники
//! (`v8_runtime_with_dom`, `v8_runtime_with_url`, `test_img_bitmap`) остались
//! в `mod.rs` и видны отсюда через `use super::*;`.

use super::*;

#[test]
fn image_src_reflects_content_attribute() {
    // BUG-305: `img.src = …` reaches the underlying `src` attribute so layout
    // can see the dynamically-assigned image, and reads back the same value.
    let rt = v8_runtime_with_dom(make_doc());
    let via_attr = rt
        .eval("var i = new Image(); i.src = 'test.png'; i.getAttribute('src')")
        .unwrap();
    assert_eq!(via_attr, lumen_core::JsValue::String("test.png".into()));
    let via_prop = rt
        .eval("var i = new Image(); i.src = 'blue.png'; i.src")
        .unwrap();
    assert_eq!(via_prop, lumen_core::JsValue::String("blue.png".into()));
    // Unset `src` reflects as the empty string, per the reflect-a-URL steps.
    let unset = rt.eval("new Image().src").unwrap();
    assert_eq!(unset, lumen_core::JsValue::String("".into()));
}

#[test]
fn html_image_element_is_a_global() {
    // BUG-305: `HTMLImageElement` is exposed as a bare interface global.
    let rt = v8_runtime_with_dom(make_doc());
    let ty = rt.eval("typeof HTMLImageElement").unwrap();
    assert_eq!(ty, lumen_core::JsValue::String("function".into()));
}

#[test]
fn query_selector_compound_tag_and_id() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('div#main') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_compound_wrong_tag_returns_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span#main') === null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_compound_tag_and_class() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span.highlight') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_child_combinator() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('div > span') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_descendant_combinator() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('body span') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_id_child_class() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('#main > .highlight') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_matches_compound() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span').matches('span.highlight')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_matches_wrong_compound_returns_false() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span').matches('div.highlight')")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn element_closest_finds_ancestor() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span').closest('div') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn element_closest_id_selector() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('span').closest('#main') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn query_selector_attribute_selector() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("document.querySelector('[id=\"main\"]') !== null")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_get_context_2d_returns_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas');\
                     var ctx = c.getContext('2d');\
                     ctx !== null && typeof ctx.fillRect === 'function' \
                       && typeof ctx.beginPath === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_get_context_2d_caches_same_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let same = rt
        .eval(
            "var c = document.createElement('canvas');\
                     c.getContext('2d') === c.getContext('2d')",
        )
        .unwrap();
    assert_eq!(same, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_default_dimensions_are_300x150() {
    let rt = v8_runtime_with_dom(make_doc());
    let w = rt
        .eval("var c = document.createElement('canvas'); c.width")
        .unwrap();
    let h = rt
        .eval("var c = document.createElement('canvas'); c.height")
        .unwrap();
    assert_eq!(w, lumen_core::JsValue::Number(300.0));
    assert_eq!(h, lumen_core::JsValue::Number(150.0));
}

#[test]
fn canvas_draw_flushes_dirty_buffer() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
                 c.setAttribute('width', '4'); c.setAttribute('height', '4');\
                 var ctx = c.getContext('2d');\
                 ctx.fillStyle = '#00ff00';\
                 ctx.fillRect(0, 0, 4, 4);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "one dirty canvas after fillRect");
    let (_nid, w, h, rgba) = &updates[0];
    assert_eq!((*w, *h), (4, 4));
    assert_eq!(rgba[1], 255, "green channel painted");
}

#[test]
fn canvas_gradient_object_has_gid_and_add_color_stop() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas'); var ctx = c.getContext('2d');\
                     var g = ctx.createLinearGradient(0, 0, 1, 1);\
                     typeof g === 'object' && g.__gid__ !== undefined \
                       && typeof g.addColorStop === 'function'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_radial_and_conic_gradient_constructors_distinct() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas'); var ctx = c.getContext('2d');\
                     var r = ctx.createRadialGradient(0, 0, 0, 0, 0, 5);\
                     var k = ctx.createConicGradient(0, 5, 5);\
                     r.__gid__ !== undefined && k.__gid__ !== undefined && r.__gid__ !== k.__gid__",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_gradient_fillstyle_paints_pixels() {
    // A gradient with two identical green stops fills solid green regardless
    // of interpolation — robustly exercises the fillStyle gradient dispatch.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
                 c.setAttribute('width', '4'); c.setAttribute('height', '4');\
                 var ctx = c.getContext('2d');\
                 var g = ctx.createLinearGradient(0, 0, 4, 0);\
                 g.addColorStop(0, '#00ff00'); g.addColorStop(1, '#00ff00');\
                 ctx.fillStyle = g;\
                 ctx.fillRect(0, 0, 4, 4);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert_eq!(updates.len(), 1, "gradient fill marks the canvas dirty");
    assert_eq!(updates[0].3[1], 255, "solid-green gradient painted");
}

#[test]
fn canvas_shadow_properties_are_wired() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas'); var ctx = c.getContext('2d');\
                     ctx.shadowColor = '#ff0000'; ctx.shadowBlur = 4;\
                     ctx.shadowOffsetX = 2; ctx.shadowOffsetY = 3;\
                     ctx.shadowColor === '#ff0000' && ctx.shadowBlur === 4 \
                       && ctx.shadowOffsetX === 2 && ctx.shadowOffsetY === 3",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_create_pattern_returns_pattern_id() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var src = document.createElement('canvas');\
                     src.setAttribute('width', '4'); src.setAttribute('height', '4');\
                     var sctx = src.getContext('2d'); sctx.fillStyle = '#0000ff'; sctx.fillRect(0, 0, 4, 4);\
                     var c = document.createElement('canvas'); var ctx = c.getContext('2d');\
                     var p = ctx.createPattern(src, 'repeat');\
                     p !== null && p.__patid__ !== undefined",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_create_pattern_null_for_invalid_source() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas'); var ctx = c.getContext('2d');\
                     ctx.createPattern(null, 'repeat') === null \
                       && ctx.createPattern({}, 'repeat') === null",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_draw_image_blits_canvas_source() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var src = document.createElement('canvas');\
                 src.setAttribute('width', '4'); src.setAttribute('height', '4');\
                 var sctx = src.getContext('2d'); sctx.fillStyle = '#ff0000'; sctx.fillRect(0, 0, 4, 4);\
                 var c = document.createElement('canvas');\
                 c.setAttribute('width', '4'); c.setAttribute('height', '4');\
                 var ctx = c.getContext('2d');\
                 ctx.drawImage(src, 0, 0);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    let any_red = updates
        .iter()
        .any(|(_n, _w, _h, rgba)| rgba[0] == 255 && rgba[2] == 0);
    assert!(any_red, "drawImage blits the red source onto the destination");
}

#[test]
fn canvas_draw_image_9arg_crops_source_subrect() {
    // Source 2×2: left column red, right column blue. The 9-arg form crops the
    // right (blue) column and stretches it over the whole destination — the
    // result must contain blue and no red, proving source-crop is honoured.
    let rt = v8_runtime_with_dom(make_doc());
    let dest_nid = match rt
        .eval(
            "var src = document.createElement('canvas');\
                     src.setAttribute('width', '2'); src.setAttribute('height', '2');\
                     var sctx = src.getContext('2d');\
                     sctx.fillStyle = '#ff0000'; sctx.fillRect(0, 0, 1, 2);\
                     sctx.fillStyle = '#0000ff'; sctx.fillRect(1, 0, 1, 2);\
                     var c = document.createElement('canvas');\
                     c.setAttribute('width', '2'); c.setAttribute('height', '2');\
                     var ctx = c.getContext('2d');\
                     ctx.drawImage(src, 1, 0, 1, 2, 0, 0, 2, 2);\
                     c.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected dest nid number, got {other:?}"),
    };
    let updates = rt.flush_canvas_updates();
    // Inspect the destination canvas pixel buffer (identified by its node id —
    // the source canvas also contains red, so it must not be sampled here).
    let dest = updates
        .iter()
        .find(|(n, _, _, _)| *n == dest_nid)
        .expect("destination canvas update");
    let rgba = &dest.3;
    let mut any_blue = false;
    let mut any_red = false;
    for px in rgba.chunks_exact(4) {
        if px[3] == 0 {
            continue;
        }
        if px[2] == 255 && px[0] == 0 {
            any_blue = true;
        }
        if px[0] == 255 && px[2] == 0 {
            any_red = true;
        }
    }
    assert!(any_blue, "cropped blue column must be drawn");
    assert!(!any_red, "red column must be excluded by the source crop");
}

#[test]
fn canvas_draw_image_from_img_element_3arg() {
    // 3-arg drawImage(img, dx, dy): the img element's registered RGBA8 bitmap is
    // blitted at natural size onto the destination canvas.
    let rt = v8_runtime_with_dom(make_doc());
    // Register a 2×2 fully-red bitmap for the img element (nid is arbitrary but
    // must match the DOM node created below).
    let img_nid: u32 = match rt
        .eval(
            "var img = document.createElement('img');\
                     img.setAttribute('src', 'test.png');\
                     img.setAttribute('width', '2');\
                     img.setAttribute('height', '2');\
                     document.body.appendChild(img);\
                     img.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected img nid, got {other:?}"),
    };
    // Inject decoded bitmap: 2×2 solid red RGBA8.
    let rgba8 = vec![255u8, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
    rt.register_img_bitmaps(vec![(img_nid, test_img_bitmap(2, 2, rgba8))]);

    let dest_nid = match rt
        .eval(
            "var c = document.createElement('canvas');\
                     c.setAttribute('width', '4'); c.setAttribute('height', '4');\
                     var ctx = c.getContext('2d');\
                     ctx.drawImage(img, 0, 0);\
                     c.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected dest nid, got {other:?}"),
    };
    let updates = rt.flush_canvas_updates();
    let dest = updates.iter().find(|(n, _, _, _)| *n == dest_nid)
        .expect("destination canvas must have an update");
    // Top-left 2×2 region should be red; natural size is used (dw/dh=0 → native picks 2×2).
    let rgba = &dest.3;
    let any_red = rgba.chunks_exact(4).any(|px| px[0] == 255 && px[2] == 0 && px[3] == 255);
    assert!(any_red, "drawImage(img, dx, dy) must blit the registered red bitmap");
}

#[test]
fn canvas_draw_image_from_img_element_5arg() {
    // 5-arg drawImage(img, dx, dy, dw, dh): blits and scales the bitmap.
    let rt = v8_runtime_with_dom(make_doc());
    let img_nid: u32 = match rt
        .eval(
            "var img = document.createElement('img');\
                     img.setAttribute('src', 'blue.png');\
                     document.body.appendChild(img);\
                     img.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected img nid, got {other:?}"),
    };
    // 1×1 solid blue.
    let rgba8 = vec![0u8, 0, 255, 255];
    rt.register_img_bitmaps(vec![(img_nid, test_img_bitmap(1, 1, rgba8))]);

    let dest_nid = match rt
        .eval(
            "var c = document.createElement('canvas');\
                     c.setAttribute('width', '4'); c.setAttribute('height', '4');\
                     var ctx = c.getContext('2d');\
                     ctx.drawImage(img, 0, 0, 4, 4);\
                     c.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected dest nid, got {other:?}"),
    };
    let updates = rt.flush_canvas_updates();
    let dest = updates.iter().find(|(n, _, _, _)| *n == dest_nid)
        .expect("destination canvas must have an update");
    let any_blue = dest.3.chunks_exact(4).any(|px| px[2] == 255 && px[0] == 0 && px[3] == 255);
    assert!(any_blue, "drawImage(img, dx, dy, dw, dh) must blit the registered blue bitmap");
}

#[test]
fn canvas_draw_image_from_img_element_9arg_crop() {
    // 9-arg drawImage(img, sx, sy, sw, sh, dx, dy, dw, dh): crops source sub-rect.
    // Source: 2×1 bitmap — left pixel red, right pixel green.
    // Crop right (green) half only: sx=1, sy=0, sw=1, sh=1.
    let rt = v8_runtime_with_dom(make_doc());
    let img_nid: u32 = match rt
        .eval(
            "var img = document.createElement('img');\
                     img.setAttribute('src', 'rg.png');\
                     document.body.appendChild(img);\
                     img.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected img nid, got {other:?}"),
    };
    // 2×1 RGBA8: [R, G, B, A] × 2 pixels.
    let rgba8 = vec![255u8, 0, 0, 255, 0, 255, 0, 255]; // red | green
    rt.register_img_bitmaps(vec![(img_nid, test_img_bitmap(2, 1, rgba8))]);

    let dest_nid = match rt
        .eval(
            "var c = document.createElement('canvas');\
                     c.setAttribute('width', '2'); c.setAttribute('height', '2');\
                     var ctx = c.getContext('2d');\
                     ctx.drawImage(img, 1, 0, 1, 1, 0, 0, 2, 2);\
                     c.__nid__;",
        )
        .unwrap()
    {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected dest nid, got {other:?}"),
    };
    let updates = rt.flush_canvas_updates();
    let dest = updates.iter().find(|(n, _, _, _)| *n == dest_nid)
        .expect("destination canvas must have an update");
    let rgba = &dest.3;
    let any_green = rgba.chunks_exact(4).any(|px| px[1] == 255 && px[0] == 0 && px[3] == 255);
    let any_red   = rgba.chunks_exact(4).any(|px| px[0] == 255 && px[2] == 0 && px[3] == 255);
    assert!(any_green, "9-arg drawImage from <img> must blit the green crop");
    assert!(!any_red, "red pixels from left half must be excluded by the crop");
}

#[test]
fn canvas_draw_image_from_img_unregistered_is_noop() {
    // drawImage with an <img> that has no registered bitmap must be a silent no-op.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var img = document.createElement('img');\
                 img.setAttribute('src', 'missing.png');\
                 document.body.appendChild(img);\
                 var c = document.createElement('canvas');\
                 c.setAttribute('width', '2'); c.setAttribute('height', '2');\
                 var ctx = c.getContext('2d');\
                 ctx.drawImage(img, 0, 0);",
    )
    .unwrap();
    // No bitmap registered → canvas remains transparent, no dirty update needed.
    let updates = rt.flush_canvas_updates();
    // The canvas was never dirtied so either has no entry or all-transparent pixels.
    let all_transparent = updates.iter().all(|(_, _, _, rgba)| {
        rgba.chunks_exact(4).all(|px| px[3] == 0)
    });
    assert!(all_transparent, "drawImage with unregistered <img> must be a no-op");
}

#[test]
fn canvas_put_image_data_paints_pixels() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var c = document.createElement('canvas');\
                 c.setAttribute('width', '2'); c.setAttribute('height', '2');\
                 var ctx = c.getContext('2d');\
                 var img = ctx.createImageData(2, 2);\
                 for (var i = 0; i < img.data.length; i += 4) {\
                     img.data[i] = 0; img.data[i + 1] = 255; img.data[i + 2] = 0; img.data[i + 3] = 255;\
                 }\
                 ctx.putImageData(img, 0, 0);",
    )
    .unwrap();
    let updates = rt.flush_canvas_updates();
    assert!(
        updates.iter().any(|(_n, _w, _h, rgba)| rgba[1] == 255),
        "putImageData paints the supplied green pixels"
    );
}

#[test]
fn canvas_get_context_webgl_returns_functional_context() {
    // Tightened during the port (was `canvas_get_context_webgl_via_2d_shim_is_null`,
    // asserting `getContext('webgl') === null`). That expectation encoded an
    // install-ordering defect of the QuickJS path, not a shim boundary:
    // `lib.rs::install_dom` evaluates `webgl_canvas::WEBGL_SHIM` *before*
    // `dom::install_dom_api` defines `document`, so the shim's
    // `if (typeof document !== 'undefined')` guard skipped its
    // `document.createElement` hook and WebGL was silently dead there.
    // `V8JsRuntime::install_dom` evals WEB_API_SHIM first, so the hook lands and
    // `getContext('webgl')` hands out the real software-rasterizer context —
    // the spec-correct answer (HTML LS §4.12.4). The 2D shim's own fall-through
    // to `null` for unknown types is still covered by
    // `webgl_canvas::tests::get_context_unknown_type_returns_null`.
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt
        .eval(
            "var c = document.createElement('canvas');\
                     var gl = c.getContext('webgl');\
                     gl !== null && typeof gl.getParameter === 'function' \
                       && gl.drawingBufferWidth === 300 && gl.canvas === c \
                       && typeof gl.fillRect === 'undefined'",
        )
        .unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

/// BUG-450: этот тест фиксировал сам дефект — «`div.getContext('2d')` отдаёт
/// null» описывает элемент, У КОТОРОГО ЕСТЬ метод `getContext`. По HTML LS
/// §4.12.5 метод принадлежит `HTMLCanvasElement`, и скрипты определяют
/// поддержку канваса именно как `'getContext' in el`, поэтому у `<div>` его не
/// должно быть вовсе.
#[test]
fn non_canvas_has_no_get_context_at_all() {
    let rt = v8_runtime_with_dom(make_doc());
    let absent = rt
        .eval(
            "var d = document.createElement('div');\
             !('getContext' in d) && d.getContext === undefined",
        )
        .unwrap();
    assert_eq!(absent, lumen_core::JsValue::Bool(true));
}

// ── Canvas CSS resize tests ───────────────────────────────────────────────

#[test]
fn canvas_css_resize_scales_pixels() {
    // After a CSS-driven resize, scale_resize is called and pixels are preserved.
    let rt = v8_runtime_with_dom(make_doc());
    // Create canvas, draw a red fill, then trigger CSS resize.
    rt.eval(r#"
                var c = document.createElement('canvas');
                c.width = 4; c.height = 4;
                var ctx = c.getContext('2d');
                ctx.fillStyle = '#ff0000';
                ctx.fillRect(0, 0, 4, 4);
                window.__test_canvas_nid = c.__nid__;
            "#).unwrap();
    let nid_val = rt.eval("window.__test_canvas_nid").unwrap();
    let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
    // First delivery at 4×4 — records baseline.
    rt.update_layout_rects([(nid, [0.0, 0.0, 4.0, 4.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    // Drain dirty list so next flush only sees scale_resize changes.
    // Must go through the runtime so the drain runs on the JS thread where
    // the canvas thread-local registry lives (B-1: runtime off the UI thread).
    let _ = rt.flush_canvas_updates();
    // Change CSS dims to 8×8 — triggers scale_resize + marks dirty.
    rt.update_layout_rects([(nid, [0.0, 0.0, 8.0, 8.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    // Canvas backing buffer should now be 8×8.
    let dirty = rt.flush_canvas_updates();
    let resized = dirty.iter().any(|(id, w, h, _)| *id == nid && *w == 8 && *h == 8);
    assert!(resized, "canvas should have been scaled to 8×8");
}

#[test]
fn canvas_css_resize_fires_resize_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var c2 = document.createElement('canvas');
                c2.width = 10; c2.height = 10;
                c2.getContext('2d');
                var _css_resize_fired = false;
                c2.addEventListener('resize', function() { _css_resize_fired = true; });
                window.__test_c2_nid = c2.__nid__;
            "#).unwrap();
    let nid_val = rt.eval("window.__test_c2_nid").unwrap();
    let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
    // First delivery at 10×10 — records baseline, no event.
    rt.update_layout_rects([(nid, [0.0, 0.0, 10.0, 10.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    let fired_before = rt.eval("_css_resize_fired").unwrap();
    assert_eq!(fired_before, lumen_core::JsValue::Bool(false));
    // Change CSS dims — event should fire.
    rt.update_layout_rects([(nid, [0.0, 0.0, 20.0, 20.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    let fired = rt.eval("_css_resize_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(true));
}

#[test]
fn canvas_css_resize_no_event_when_size_unchanged() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var c3 = document.createElement('canvas');
                c3.width = 10; c3.height = 10;
                c3.getContext('2d');
                var _css_cnt = 0;
                c3.addEventListener('resize', function() { _css_cnt++; });
                window.__test_c3_nid = c3.__nid__;
            "#).unwrap();
    let nid_val = rt.eval("window.__test_c3_nid").unwrap();
    let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
    let rect = [(nid, [0.0, 0.0, 10.0, 10.0])].into_iter().collect();
    rt.update_layout_rects(rect);
    // First delivery — baseline.
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    // Second delivery — same size, no event.
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    let cnt = rt.eval("_css_cnt").unwrap();
    assert_eq!(cnt, lumen_core::JsValue::Number(0.0));
}

#[test]
fn canvas_css_resize_not_triggered_without_context() {
    // A canvas without a 2D context is not tracked by _lumen_deliver_canvas_css_resize.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var c4 = document.createElement('canvas');
                // intentionally no getContext('2d')
                var _no_ctx_fired = false;
                c4.addEventListener('resize', function() { _no_ctx_fired = true; });
                window.__test_c4_nid = c4.__nid__;
            "#).unwrap();
    let nid_val = rt.eval("window.__test_c4_nid").unwrap();
    let nid = if let lumen_core::JsValue::Number(n) = nid_val { n as u32 } else { panic!("no nid") };
    rt.update_layout_rects([(nid, [0.0, 0.0, 50.0, 50.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 100.0])].into_iter().collect());
    rt.eval("_lumen_deliver_canvas_css_resize()").unwrap();
    let fired = rt.eval("_no_ctx_fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(false));
}

#[test]
fn alert_does_not_crash() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("alert('test')").unwrap();
}

#[test]
fn window_print_emits_request() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("window.print()").unwrap();
    let reqs = rt.take_print_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].margin_top, 48.0);
    assert_eq!(reqs[0].margin_bottom, 48.0);
    assert_eq!(reqs[0].margin_left, 48.0);
    assert_eq!(reqs[0].margin_right, 48.0);
}

#[test]
fn timeout_is_deferred_until_tick() {
    let rt = v8_runtime_with_dom(make_doc());
    // Timer must NOT fire synchronously — deferred to _lumen_tick_timers().
    let result = rt
        .eval("var x = 0; setTimeout(function() { x = 1; }, 0); x")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(0.0));
}

#[test]
fn timeout_fires_after_tick() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var x = 0; setTimeout(function() { x = 1; }, 0);")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); x").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn clear_timeout_prevents_fire() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var x = 0; var id = setTimeout(function() { x = 1; }, 0); clearTimeout(id);")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); x").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(0.0));
}

#[test]
fn set_interval_fires_repeatedly() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var n = 0; setInterval(function() { n++; }, 0);")
        .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let result = rt.eval("n").unwrap();
    // Fired at least twice (exact count depends on scheduling).
    assert!(matches!(result, lumen_core::JsValue::Number(n) if n >= 2.0));
}

#[test]
fn clear_interval_stops_fire() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var n = 0; var id = setInterval(function() { n++; }, 0);")
        .unwrap();
    rt.eval("_lumen_tick_timers(); clearInterval(id);")
        .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let result = rt.eval("n").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

/// [BUG-831] A string handler used to be dropped at the door
/// (`if (typeof fn !== 'function') return 0;`), so the page could not
/// tell "never compiled" from "not due yet".
#[test]
fn bug831_string_timeout_runs_at_tick() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var x = 0; setTimeout(\"x = 1\", 0);").unwrap();
    let result = rt.eval("_lumen_tick_timers(); x").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

/// [BUG-831] HTML LS §8.6 runs the string as a **classic script**, not
/// as a function body: a `var` it declares becomes a global. This is
/// what separates indirect eval from `new Function(src)`, which would
/// swallow the declaration in its own scope.
#[test]
fn bug831_string_timeout_runs_in_global_scope() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("setTimeout(\"var bug831_global = 7;\", 0);")
        .unwrap();
    let result = rt
        .eval("_lumen_tick_timers(); typeof bug831_global === 'number' && bug831_global === 7")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// [BUG-831] The id handed back for a string handler was the literal
/// `0` the spec reserves for "no timer created", yet `clearTimeout`
/// accepted it — so cancelling one string timer silently cancelled
/// nothing. It must be a real id that cancels a real timer.
#[test]
fn bug831_string_timeout_id_is_real_and_cancellable() {
    let rt = v8_runtime_with_dom(make_doc());
    let id = rt
        .eval("var y = 0; var id831 = setTimeout(\"y = 1\", 0); id831 !== 0")
        .unwrap();
    assert_eq!(id, lumen_core::JsValue::Bool(true));
    let result = rt
        .eval("clearTimeout(id831); _lumen_tick_timers(); y")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(0.0));
}

/// [BUG-831] §8.6 compiles the string when the timer **fires**, not
/// when it is scheduled: a syntactically broken handler must schedule
/// normally and report its exception from the callback the way a
/// throwing function handler does (BUG-591), not throw at the call.
#[test]
fn bug831_string_handler_compiles_at_fire_not_at_schedule() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var armed831 = setTimeout(\"(\", 0);").unwrap();
    // Scheduling a broken string is not an error…
    assert_eq!(
        rt.eval("armed831 !== 0").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
    // …and the failure at fire time stays inside the timer loop.
    rt.eval("var ran831 = false; setTimeout(function() { ran831 = true; }, 0);")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); ran831").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// [BUG-831] An interval's string is recompiled on every firing, so it
/// repeats like a function handler instead of running once (or, as
/// before the fix, never).
#[test]
fn bug831_string_interval_repeats() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var n831 = 0; setInterval(\"n831++\", 0);").unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let result = rt.eval("n831").unwrap();
    assert!(matches!(result, lumen_core::JsValue::Number(n) if n >= 2.0));
}

/// [BUG-847] The `timeout` argument is a WebIDL `long`, so ToInt32
/// makes `Math.pow(2, 32)` an immediate timer. Before the fix it was
/// scheduled 49 days out and the page could not tell that apart from a
/// timer that had been dropped.
#[test]
fn bug847_delay_above_int32_range_is_immediate() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var n847 = 0; setTimeout(function() { n847 = 1; }, Math.pow(2, 32));")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); n847").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

/// [BUG-847] `Math.pow(2, 31)` reaches 0 by the other side of the same
/// modulo — ToInt32 makes it the most negative `long`, which §8.6
/// step 5 then raises to 0. An interval must repeat at that rate, not
/// fire once (`type-long-setinterval.any.js`).
#[test]
fn bug847_interval_delay_at_int32_boundary_repeats() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var i847 = 0; setInterval(function() { i847++; }, Math.pow(2, 31));")
        .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let result = rt.eval("i847").unwrap();
    assert!(matches!(result, lumen_core::JsValue::Number(n) if n >= 2.0));
}

/// [BUG-847] `Infinity` used to become the deadline verbatim, i.e. a
/// timer nothing can ever reach. ToInt32 of a non-finite number is 0.
#[test]
fn bug847_infinite_delay_is_immediate() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var inf847 = 0; setTimeout(function() { inf847 = 1; }, Infinity);")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); inf847").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

/// [BUG-847] The conversion starts at ToNumber, so a delay that is not
/// already a number is converted rather than dropped: the old
/// `typeof delay === 'number'` guard read `'100'` and an object with a
/// `valueOf` as 0, i.e. as "fire on the next tick".
#[test]
fn bug847_non_number_delay_is_converted_not_dropped() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "(function () {
                        _lumen_timers.length = 0;
                        var base = _lumen_now_ms();
                        setTimeout(function () {}, '100');
                        setTimeout(function () {}, { valueOf: function () { return 250; } });
                        setTimeout(function () {}, 1.9);
                        return _lumen_timers.map(function (t) {
                            return Math.floor(t.deadline - base);
                        }).join(',');
                     })()",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("100,250,1".into()));
}

/// [BUG-847] A negative delay is 0, not a deadline in the past that
/// happens to work by accident — and `undefined`/`NaN`/`null` are 0
/// through ToNumber rather than through the dropped-argument branch.
#[test]
fn bug847_negative_and_nan_delays_are_zero() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            "(function () {
                        _lumen_timers.length = 0;
                        var base = _lumen_now_ms();
                        setTimeout(function () {}, -100);
                        setTimeout(function () {}, NaN);
                        setTimeout(function () {});
                        // Zero, so the deadline is the scheduling instant — not
                        // 100 ms in the past, and not NaN (which fails every
                        // comparison and so is never due).
                        return _lumen_timers.every(function (t) {
                            var d = t.deadline - base;
                            return d >= 0 && d < 5;
                        });
                     })()",
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// [BUG-847] The handle of `clearTimeout` is a WebIDL `long` too, so a
/// stringified id must cancel the timer it names. The strict `===`
/// against the raw argument matched nothing and the callback still ran.
#[test]
fn bug847_clear_timeout_converts_its_handle() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var c847 = 0; var id847 = setTimeout(function() { c847 = 1; }, 0); clearTimeout(String(id847));")
        .unwrap();
    let result = rt.eval("_lumen_tick_timers(); c847").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(0.0));
}

#[test]
fn bug271_nested_timeout_clamped_to_4ms() {
    // HTML LS §8.6: nesting level > 5 clamps timeout < 4 ms up to 4 ms.
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("_lumen_clamp_timeout(0, 6) === 4 && _lumen_clamp_timeout(0, 5) === 0 && _lumen_clamp_timeout(10, 7) === 10")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug271_timer_callback_inherits_nesting_level() {
    // A timer scheduled from inside a timer callback records nesting+1,
    // so deep setTimeout(fn,0) chains eventually hit the 4 ms clamp.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("setTimeout(function() { setTimeout(function() {}, 0); }, 0);")
        .unwrap();
    let before = rt
        .eval("_lumen_timers.some(function(t) { return t.nesting === 2; })")
        .unwrap();
    assert_eq!(before, lumen_core::JsValue::Bool(false));
    rt.eval("_lumen_tick_timers();").unwrap();
    // The inner timer scheduled from inside the fired callback carries nesting 2.
    let after = rt
        .eval("_lumen_timers.some(function(t) { return t.nesting === 2; })")
        .unwrap();
    assert_eq!(after, lumen_core::JsValue::Bool(true));
}

#[test]
fn scheduler_post_task_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("typeof scheduler.postTask(function() { return 42; })")
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("object".into()));
}

#[test]
fn scheduler_post_task_rejects_non_function() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval("var rejected = false; scheduler.postTask(42).catch(function() { rejected = true; }); rejected")
        .unwrap();
    // Promise rejection is async; we can only verify the call didn't throw synchronously.
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn history_initial_length_is_one() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("history.length").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

#[test]
fn history_initial_state_is_null() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("history.state === null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn history_push_state_increments_length() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval("history.pushState({page: 1}, '', '/page1');").unwrap();
    rt.eval("history.pushState({page: 2}, '', '/page2');").unwrap();
    let result = rt.eval("history.length").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(3.0));
}

#[test]
fn history_state_after_push_returns_state() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval("history.pushState({x: 42}, '', '/p');").unwrap();
    let result = rt.eval("history.state.x").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(42.0));
}

#[test]
fn history_replace_state_keeps_length() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval("history.pushState({n: 1}, '', '/a');").unwrap();
    rt.eval("history.replaceState({n: 99}, '', '/a2');").unwrap();
    let len = rt.eval("history.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(2.0));
    let state = rt.eval("history.state.n").unwrap();
    assert_eq!(state, lumen_core::JsValue::Number(99.0));
}

/// BUG-829 — HTML LS §7.4.6 step 3: the `url` argument is parsed
/// relative to the document base URL, and the *resolved* URL is what
/// the document gets. A query-only argument used to land in
/// `location.href` verbatim, leaving `search` empty — so an SPA router
/// read its own query parameters off a string that was not a URL.
#[test]
fn history_push_state_resolves_url_against_document_base() {
    let rt = v8_runtime_with_url("https://example.com/shop/index.html");
    rt.eval("history.pushState({}, '', '?id=42')").unwrap();
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/shop/index.html?id=42".into())
    );
    assert_eq!(
        rt.eval("location.search").unwrap(),
        lumen_core::JsValue::String("?id=42".into())
    );
    // A path-relative argument resolves against the directory of the
    // document, a root-relative one against the origin.
    rt.eval("history.pushState({}, '', 'cart.html')").unwrap();
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/shop/cart.html".into())
    );
    rt.eval("history.pushState({}, '', '/about')").unwrap();
    assert_eq!(
        rt.eval("location.pathname").unwrap(),
        lumen_core::JsValue::String("/about".into())
    );
}

/// The fragment half of the same defect: `pushState(s, '', '#frag')`
/// used to leave `location.hash` empty while `href` became `#frag`.
#[test]
fn history_push_state_resolves_fragment_only_url() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("history.pushState({}, '', '#frag')").unwrap();
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#frag".into())
    );
    assert_eq!(
        rt.eval("location.hash").unwrap(),
        lumen_core::JsValue::String("#frag".into())
    );
}

/// `replaceState` shares the resolution path with `pushState`.
#[test]
fn history_replace_state_resolves_url_against_document_base() {
    let rt = v8_runtime_with_url("https://example.com/shop/index.html");
    rt.eval("history.replaceState({}, '', '?id=7')").unwrap();
    assert_eq!(
        rt.eval("location.search").unwrap(),
        lumen_core::JsValue::String("?id=7".into())
    );
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/shop/index.html?id=7".into())
    );
}

/// HTML LS §7.4.6 step 3.3 — a document may only be rewritten to a URL
/// of its own origin. The throw has to happen before anything is
/// stored, so a refused call must leave the session history untouched.
#[test]
fn history_push_state_cross_origin_url_throws_security_error() {
    let rt = v8_runtime_with_url("https://example.com/page");
    let r = rt
        .eval(
            "var name = '';                      try { history.pushState({}, '', 'https://evil.example/x'); }                      catch (e) { name = e.name; }                      name + '|' + history.length + '|' + location.href",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "SecurityError|1|https://example.com/page".into()
        )
    );
    assert!(rt.take_history_url_updates().is_empty());
}

/// A different port is a different origin too — the check is on the
/// whole scheme/credentials/host/port tuple, not on the host alone.
#[test]
fn history_replace_state_cross_port_url_throws_security_error() {
    let rt = v8_runtime_with_url("https://example.com/page");
    let r = rt
        .eval(
            "var name = '';                      try { history.replaceState({}, '', 'https://example.com:8443/page'); }                      catch (e) { name = e.name; } name",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("SecurityError".into()));
}

#[test]
fn history_back_fires_popstate_with_previous_state() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "var events = []; \
                 window.addEventListener('popstate', function(e) { events.push(e.state); }); \
                 history.pushState({page: 1}, '', '/p1'); \
                 history.pushState({page: 2}, '', '/p2'); \
                 history.back();",
    )
    .unwrap();
    // Traversal is shell-authoritative: history.back() moved the read-cache
    // cursor and queued a -1 delta, but the popstate is delivered by the shell.
    // Simulate the shell handing the destination entry back to JS.
    assert_eq!(rt.take_history_traversals(), vec![-1]);
    rt.eval("_lumen_deliver_popstate(_lumen_history_state_json(), _lumen_history_url())")
        .unwrap();
    let len = rt.eval("events.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(1.0));
    let page = rt.eval("events[0].page").unwrap();
    assert_eq!(page, lumen_core::JsValue::Number(1.0));
}

#[test]
fn history_forward_after_back() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "history.pushState({n: 1}, '', '/p1'); \
                 history.pushState({n: 2}, '', '/p2'); \
                 history.back();",
    )
    .unwrap();
    let state_after_back = rt.eval("history.state.n").unwrap();
    assert_eq!(state_after_back, lumen_core::JsValue::Number(1.0));

    rt.eval("history.forward();").unwrap();
    let state_after_fwd = rt.eval("history.state.n").unwrap();
    assert_eq!(state_after_fwd, lumen_core::JsValue::Number(2.0));
}

#[test]
fn history_go_beyond_bounds_does_not_fire_popstate() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var fired = false; \
                 window.addEventListener('popstate', function() { fired = true; }); \
                 history.go(-5);",
    )
    .unwrap();
    let result = rt.eval("fired").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(false));
}

#[test]
fn window_onpopstate_fires_on_back() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "var captured = null; \
                 window.onpopstate = function(e) { captured = e.state; }; \
                 history.pushState({v: 7}, '', '/p'); \
                 history.back();",
    )
    .unwrap();
    let result = rt.eval("captured === null").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true)); // initial state is null
}

#[test]
fn history_push_drops_forward_entries() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "history.pushState({n: 1}, '', '/p1'); \
                 history.pushState({n: 2}, '', '/p2'); \
                 history.back(); \
                 history.pushState({n: 3}, '', '/p3');",
    )
    .unwrap();
    // After back + push, forward entries are dropped: entries = [init, {n:1}, {n:3}]
    let len = rt.eval("history.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(3.0));
    let state = rt.eval("history.state.n").unwrap();
    assert_eq!(state, lumen_core::JsValue::Number(3.0));
}

#[test]
fn history_go_zero_reloads() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("history.go(0)").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Reload)));
}

#[test]
fn history_go_updates_location() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "history.pushState({},'','https://example.com/p1'); \
                 history.pushState({},'','https://example.com/p2'); \
                 history.go(-1);",
    )
    .unwrap();
    // go(-1) queued the traversal and moved the read-cache cursor; the shell
    // delivers the popstate that syncs `location`. Simulate that delivery.
    assert_eq!(rt.take_history_traversals(), vec![-1]);
    rt.eval("_lumen_deliver_popstate(_lumen_history_state_json(), _lumen_history_url())")
        .unwrap();
    let path = rt.eval("location.pathname").unwrap();
    assert_eq!(path, lumen_core::JsValue::String("/p1".into()));
}

#[test]
fn history_go_queues_single_step_traversal() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "history.pushState({},'','/p1'); \
                 history.pushState({},'','/p2'); \
                 history.back();",
    )
    .unwrap();
    // back() routes through history.go(-1): one shell traversal queued.
    assert_eq!(rt.take_history_traversals(), vec![-1]);
}

#[test]
fn history_go_multistep_queues_full_delta_and_moves_cache() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "history.pushState({n:1},'','/p1'); \
                 history.pushState({n:2},'','/p2'); \
                 history.pushState({n:3},'','/p3'); \
                 history.go(-2);",
    )
    .unwrap();
    // The full multi-step delta is queued once (the shell fires a single
    // destination popstate), and the read-cache cursor jumped two entries.
    assert_eq!(rt.take_history_traversals(), vec![-2]);
    assert_eq!(
        rt.eval("history.state.n").unwrap(),
        lumen_core::JsValue::Number(1.0)
    );
}

#[test]
fn history_go_zero_does_not_queue_traversal() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("history.go(0)").unwrap();
    assert!(rt.take_history_traversals().is_empty());
    assert!(matches!(
        rt.take_navigate_request(),
        Some(NavigateRequest::Reload)
    ));
}

#[test]
fn history_go_out_of_range_does_not_queue_traversal() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("history.go(7)").unwrap();
    // Out of range in the read-cache → no traversal handed to the shell.
    assert!(rt.take_history_traversals().is_empty());
}

#[test]
fn history_go_out_of_bounds_no_popstate() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var fired=false; \
                 window.addEventListener('popstate', function(){fired=true;}); \
                 history.go(7);",
    )
    .unwrap();
    let fired = rt.eval("fired").unwrap();
    assert_eq!(fired, lumen_core::JsValue::Bool(false));
}

#[test]
fn window_object_exposes_history() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt.eval("window.history === history").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn window_remove_event_listener_stops_popstate() {
    let rt = v8_runtime_with_url("https://example.com/start");
    rt.eval(
        "var count = 0; \
                 function handler(e) { count++; } \
                 window.addEventListener('popstate', handler); \
                 history.pushState({}, '', '/p'); \
                 history.back();",
    )
    .unwrap();
    // Traversal is shell-authoritative: history.back() queues a -1 delta and
    // the shell delivers the popstate. While registered, the handler fires once.
    assert_eq!(rt.take_history_traversals(), vec![-1]);
    rt.eval("_lumen_deliver_popstate(_lumen_history_state_json(), _lumen_history_url())")
        .unwrap();
    // Remove the listener, then traverse again. The shell still delivers a
    // popstate for each queued delta, but the removed handler must not fire.
    rt.eval(
        "window.removeEventListener('popstate', handler); \
                 history.forward(); \
                 history.back();",
    )
    .unwrap();
    let _ = rt.take_history_traversals();
    rt.eval("_lumen_deliver_popstate(_lumen_history_state_json(), _lumen_history_url())")
        .unwrap();
    rt.eval("_lumen_deliver_popstate(_lumen_history_state_json(), _lumen_history_url())")
        .unwrap();
    // handler fired once (before removal), then stayed silent.
    let result = rt.eval("count").unwrap();
    assert_eq!(result, lumen_core::JsValue::Number(1.0));
}

// ── BUG-591: uncaught-exception reporting (window 'error'/onerror) ────────

/// Mirrors WPT's `event-handler-processing-algorithm-error/window-runtime-error.html`
/// ("error event has the right 5 args on Window, with a runtime error").
#[test]
fn bug591_timer_exception_fires_window_error_with_five_arg_onerror() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var onerrorArgs = null; \
                 var errorEvtSeen = null; \
                 window.onerror = function() { onerrorArgs = Array.prototype.slice.call(arguments); }; \
                 window.addEventListener('error', function(e) { errorEvtSeen = e; }); \
                 setTimeout(function() { thisFunctionDoesNotExist(); }, 0);",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let args_len = rt.eval("onerrorArgs ? onerrorArgs.length : -1").unwrap();
    assert_eq!(
        args_len,
        lumen_core::JsValue::Number(5.0),
        "window.onerror must see the 5-arg OnErrorEventHandler form for a runtime error"
    );
    let is_ref_error = rt
        .eval("errorEvtSeen && errorEvtSeen.error instanceof ReferenceError")
        .unwrap();
    assert_eq!(is_ref_error, lumen_core::JsValue::Bool(true));
}

/// Mirrors WPT's `window-runtime-error.html` ("error event is weird —
/// return true cancels; many args — on Window, with a runtime error").
#[test]
fn bug591_onerror_returning_true_prevents_default() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var capturedEvt = null; \
                 window.onerror = function() { return true; }; \
                 window.addEventListener('error', function(e) { capturedEvt = e; }); \
                 setTimeout(function() { thisFunctionDoesNotExist(); }, 0);",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    // `capturedEvt` is read back in a separate eval, after dispatchEvent's
    // whole synchronous run (listeners, then onerror) has completed —
    // reading `.defaultPrevented` from inside the listener itself would
    // see it too early, since onerror runs after the explicit listeners.
    let result = rt.eval("capturedEvt.defaultPrevented").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug591_raf_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = false; \
                 window.addEventListener('error', function(e) { caught = (e.message.indexOf('boom') !== -1); }); \
                 requestAnimationFrame(function() { throw new Error('boom'); });",
    )
    .unwrap();
    rt.eval("_lumen_run_raf_callbacks(0);").unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// Mirrors WPT's `microtask-queuing/queue-microtask-exceptions.any.html`
/// ("It rethrows exceptions") — must surface as 'error', not as a promise
/// rejection (the microtask is scheduled via a fire-and-forget internal
/// `Promise.prototype.then`, so before this fix an uncaught throw here
/// went entirely unnoticed instead of firing the wrong event).
#[test]
fn bug591_queue_microtask_exception_fires_window_error_not_rejection() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var errorFired = false; var rejectionFired = false; \
                 window.addEventListener('error', function() { errorFired = true; }); \
                 window.addEventListener('unhandledrejection', function() { rejectionFired = true; }); \
                 queueMicrotask(function() { throw new Error('boo'); });",
    )
    .unwrap();
    // V8 drains the microtask queue at the end of the eval that queued it
    // (see queue_microtask_callback_runs_after_sync_tail above); the
    // reporter dispatches synchronously off that same drain.
    let result = rt.eval("errorFired && !rejectionFired").unwrap();
    assert_eq!(result, lumen_core::JsValue::Bool(true));
}

/// The DOM-insertion classic-script path (`_lumen_script_execute_classic`,
/// exercised by `dynamic_inline_script_runs_on_append` above) used to only
/// `console.error` an uncaught body exception; it must now also report it.
#[test]
fn bug591_dynamic_script_runtime_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let result = rt
        .eval(
            r#"var msg = null;
                       window.addEventListener('error', function(e) { msg = e.message; });
                       var s = document.createElement('script');
                       s.textContent = 'throw new Error("dyn-boom");';
                       document.body.appendChild(s);
                       msg"#,
        )
        .unwrap();
    assert_eq!(result, lumen_core::JsValue::String("dyn-boom".to_string()));
}

/// `V8JsRuntime::eval_and_report` (`v8_runtime.rs`) is the Rust-side
/// counterpart wired into `crates/shell/src/main.rs`'s initial classic
/// `<script>` loop; `lineno` here comes from `v8::Message` (1-based), not
/// from the `Error.stack`-parsing fallback the JS-only callers above use.
#[test]
fn bug591_eval_and_report_top_level_script_error_reaches_window() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var seen = null; window.addEventListener('error', function(e) { seen = e; });")
        .unwrap();
    let outcome = rt.eval_and_report("throw new Error('top-level-boom');");
    assert!(outcome.is_err());
    let message = rt.eval("seen && seen.message").unwrap();
    assert_eq!(
        message,
        lumen_core::JsValue::String("top-level-boom".to_string())
    );
    let lineno = rt.eval("seen && seen.lineno").unwrap();
    assert_eq!(lineno, lumen_core::JsValue::Number(1.0));
}

// ── BUG-591 (continued): exceptions from addEventListener/on<type> DOM
// event listeners now also go through "report the exception" (HTML LS
// §8.1.3.6), via `_lumen_dispatch`/`_lumen_dispatch_bubble` (native
// element/document listeners) and `EventTarget.prototype.dispatchEvent`
// (pure-JS EventTarget subclasses). Previously these ended at a bare
// `catch(e){}` with no way for the page to observe the failure.

/// `_lumen_dispatch_bubble` is the path native input (click, keydown)
/// takes; mirrors WPT's `event-handler-processing-algorithm-error/*`
/// files for a plain `addEventListener` callback (not `onerror`).
#[test]
fn bug591_add_event_listener_callback_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 document.getElementById('main').addEventListener('click', function() { throw new Error('listener-boom'); }); \
                 _lumen_dispatch_bubble(document.getElementById('main').__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(
        result,
        lumen_core::JsValue::String("listener-boom".to_string())
    );
}

/// Same as above but for the `on<type>` IDL/content-attribute form,
/// which `_lumen_dispatch_bubble` invokes in a separate branch from
/// `addEventListener` listeners.
#[test]
fn bug591_onclick_handler_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 document.getElementById('main').onclick = function() { throw new Error('onclick-boom'); }; \
                 _lumen_dispatch_bubble(document.getElementById('main').__nid__, 'click');",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(
        result,
        lumen_core::JsValue::String("onclick-boom".to_string())
    );
}

/// `document.addEventListener` listeners go through `document`'s own
/// `dispatchEvent` (not `_lumen_dispatch_bubble`'s document-level
/// branch) when the event is dispatched directly on `document`.
#[test]
fn bug591_document_dispatch_event_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 document.addEventListener('readystatechange', function() { throw new Error('doc-boom'); }); \
                 document.dispatchEvent(new Event('readystatechange'));",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("doc-boom".to_string()));
}

/// A pure-JS `EventTarget` subclass (the base many Web API shims
/// `extend`) dispatches through `EventTarget.prototype.dispatchEvent`,
/// a separate implementation from the native element/document paths
/// exercised above.
#[test]
fn bug591_event_target_subclass_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 var et = new EventTarget(); \
                 et.addEventListener('ping', function() { throw new Error('et-boom'); }); \
                 et.dispatchEvent(new Event('ping'));",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("et-boom".to_string()));
}

/// Slice 2026-08-23: `_lumen_apply_ready_state`/`_lumen_apply_visibility`
/// drive the engine's own `load`/`DOMContentLoaded`/`visibilitychange`
/// dispatch by looping listeners directly instead of going through
/// `window.dispatchEvent` (unlike a script-initiated
/// `window.dispatchEvent(new Event('load'))`, already covered above) —
/// these three loops used to swallow a listener's exception in a bare
/// `catch(e) {}`, which is what turned WPT's `css/css-shapes/spec-examples`
/// FAILs into NOTRUN + TIMEOUT (WPT-RUN-6 slice 24).
#[test]
fn bug591_window_load_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 window.addEventListener('load', function() { throw new Error('load-boom'); });",
    )
    .unwrap();
    rt.eval("_lumen_apply_ready_state('interactive'); _lumen_apply_ready_state('complete');")
        .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("load-boom".to_string()));
}

#[test]
fn bug591_window_onload_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 window.onload = function() { throw new Error('onload-boom'); };",
    )
    .unwrap();
    rt.eval("_lumen_apply_ready_state('interactive'); _lumen_apply_ready_state('complete');")
        .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("onload-boom".to_string()));
}

#[test]
fn bug591_window_domcontentloaded_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 window.addEventListener('DOMContentLoaded', function() { throw new Error('dcl-boom'); });",
    )
    .unwrap();
    rt.eval("_lumen_apply_ready_state('interactive');").unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("dcl-boom".to_string()));
}

#[test]
fn bug591_window_visibilitychange_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 window.addEventListener('visibilitychange', function() { throw new Error('vc-boom'); });",
    )
    .unwrap();
    rt.eval("_lumen_apply_visibility(true);").unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("vc-boom".to_string()));
}

/// Guards the deliberate exception to the rule above: a `window`
/// `'error'` listener that itself throws must NOT be routed back
/// through `_lumen_report_exception` (it already runs inside that
/// function's own dispatch), or a self-rethrowing handler would
/// recurse into `window.dispatchEvent` forever.
#[test]
fn bug591_window_error_listener_exception_does_not_recurse() {
    let rt = v8_runtime_with_dom(make_doc());
    let outcome = rt.eval(
        "var calls = 0; \
                 window.addEventListener('error', function() { calls++; throw new Error('error-listener-boom'); }); \
                 document.getElementById('main').addEventListener('click', function() { throw new Error('original-boom'); }); \
                 _lumen_dispatch_bubble(document.getElementById('main').__nid__, 'click'); \
                 calls",
    );
    // A recursive implementation would blow the stack (Err) or loop
    // forever (test hang); the fix runs the 'error' listener exactly
    // once, for the original click-listener exception.
    assert_eq!(outcome.unwrap(), lumen_core::JsValue::Number(1.0));
}

/// The residual WPT-RUN-6 slice 27 measured on 2026-08-23: the rIdle
/// callback ran, threw, and nothing anywhere heard about it, while the
/// neighbouring `requestAnimationFrame` path (same WPT page shape,
/// `animation-frames/callback-exception.html`) already reported.
///
/// The deadlines are forced to 0 instead of slept through — rIC
/// schedules its callback 50 ms out, and a unit test must not depend
/// on the wall clock to observe it.
#[test]
fn bug591_request_idle_callback_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 requestIdleCallback(function() { throw new Error('ric-boom'); });",
    )
    .unwrap();
    rt.eval(
        "for (var i = 0; i < _lumen_timers.length; i++) _lumen_timers[i].deadline = 0; \
                 _lumen_tick_timers();",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("ric-boom".to_string()));
}

/// `MessagePort` delivery — one of the two paths this bug's own file
/// had listed as out of scope until now. Goes through
/// `_lumen_mc_report`, not `_lumen_report_exception` directly, because
/// `MESSAGE_CHANNEL_SHIM` is also evaluated in the service-worker
/// scope, which has no reporting function at all.
#[test]
fn bug591_message_port_handler_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 var ch = new MessageChannel(); \
                 ch.port1.onmessage = function() { throw new Error('port-boom'); }; \
                 ch.port2.postMessage('ping');",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers();").unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("port-boom".to_string()));
}

/// `MediaQueryList` — the other path the bug file had listed as out of
/// scope. It carries its own hand-rolled `dispatchEvent`, so neither
/// the `EventTarget` base class nor the native element dispatch
/// (both wired 2026-08-22) reached it.
#[test]
fn bug591_media_query_list_listener_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 var mql = window.matchMedia('(min-width: 1px)'); \
                 mql.addEventListener('change', function() { throw new Error('mql-boom'); }); \
                 mql.dispatchEvent({ type: 'change', matches: true, media: mql.media });",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("mql-boom".to_string()));
}

/// The half of [BUG-840] that belongs to this bug: a
/// `PerformanceObserver` callback's exception was eaten by the shim
/// before any host-side path could see it, which is why the
/// `webaudio`/`performance-timeline` families went quiet instead of
/// failing.
#[test]
fn bug591_performance_observer_callback_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 var po = new PerformanceObserver(function() { throw new Error('po-boom'); }); \
                 po.observe({ entryTypes: ['mark'] }); \
                 performance.mark('m1');",
    )
    .unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("po-boom".to_string()));
}

/// `MutationObserver` stands for the whole observer family here
/// (Resize/Intersection deliver through the same shape, but only from
/// the relayout pipeline, which a unit test cannot drive).
#[test]
fn bug591_mutation_observer_callback_exception_fires_window_error() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var caught = null; \
                 window.addEventListener('error', function(e) { caught = e.message; }); \
                 var mo = new MutationObserver(function() { throw new Error('mo-boom'); }); \
                 mo.observe(document.getElementById('main'), { childList: true }); \
                 document.getElementById('main').appendChild(document.createElement('span'));",
    )
    .unwrap();
    rt.eval("_lumen_flush_mutation_observers();").unwrap();
    let result = rt.eval("caught").unwrap();
    assert_eq!(result, lumen_core::JsValue::String("mo-boom".to_string()));
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
    for (x, expected) in [(1, "255,0,0,255"), (3, "0,255,0,255"), (5, "0,0,255,255")] {
        let got = rt.eval(&format!("rgba({x}, 0)")).unwrap();
        assert_eq!(
            got,
            lumen_core::JsValue::String(format!("1,1,4,{expected}")),
            "pixel at x={x} must be its own stripe, at its own size"
        );
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
    assert_eq!(
        straddling,
        lumen_core::JsValue::String("0,255,0".into()),
        "in-canvas pixel keeps its blue, the one past the edge is transparent"
    );
    let before_origin = rt
        .eval("var d = ctx.getImageData(-1, 0, 2, 1); d.data[3] + ',' + d.data[4]")
        .unwrap();
    assert_eq!(before_origin, lumen_core::JsValue::String("0,255".into()));
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
    let flipped = rt
        .eval("var d = ctx.getImageData(2, 1, -2, -1); d.width + 'x' + d.height + ':' + d.data[0]")
        .unwrap();
    assert_eq!(
        flipped,
        lumen_core::JsValue::String("2x1:255".into()),
        "(2,1,-2,-1) is the rect at (0,0) sized 2x1, i.e. the red stripe"
    );
    // A fractional coordinate truncates toward zero rather than rounding.
    assert_eq!(
        rt.eval("rgba(3.9, 0.9)").unwrap(),
        lumen_core::JsValue::String("1,1,4,0,255,0,255".into())
    );
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
    assert_eq!(
        rt.eval("rgba(2, 0)").unwrap(),
        lumen_core::JsValue::String("1,1,4,1,2,3,255".into()),
        "inside the dirty rect"
    );
    assert_eq!(
        rt.eval("rgba(1, 0)").unwrap(),
        lumen_core::JsValue::String("1,1,4,255,0,0,255".into()),
        "left of the dirty rect is untouched"
    );
    assert_eq!(
        rt.eval("rgba(2, 1)").unwrap(),
        lumen_core::JsValue::String("1,1,4,0,255,0,255".into()),
        "below the dirty rect is untouched"
    );
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
