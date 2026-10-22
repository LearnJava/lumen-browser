    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    use crate::v8_runtime::V8JsRuntime;

    fn with_offscreen() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        // On a page `install_dom` runs first and brings `DOMException` with it
        // (installed before this module's own bindings); here nothing does, so
        // `throw new DOMException(...)` in `getImageData` would otherwise become
        // a `ReferenceError` (same trap as `filesystem_access.rs`'s harness).
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        super::install_offscreen_canvas_bindings_v8(&rt, "https://example.test").unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn js_offscreen_canvas_constructor() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(100, 200);
                canvas.width === 100 && canvas.height === 200 &&
                typeof canvas.__canvas_id__ === 'number'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_get_context() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                let ctx2d = canvas.getContext('2d');
                ctx2d !== null && ctx2d.canvas === canvas
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_get_context_cached() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                let ctx1 = canvas.getContext('2d');
                let ctx2 = canvas.getContext('2d');
                ctx1 === ctx2  // Same instance cached
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_context_is_real_class_instance() {
        // BUG-932: the context, gradient, pattern, text-metrics and image-data
        // objects used to be duck-typed literals with `Object.prototype` —
        // `ctx.constructor.name === 'Object'` and `instanceof
        // OffscreenCanvasRenderingContext2D` threw a ReferenceError (no such
        // global existed). All five now brand-check and report correctly.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                let ctx = canvas.getContext('2d');
                let g = ctx.createLinearGradient(0, 0, 1, 1);
                let p = ctx.createPattern(new OffscreenCanvas(2, 2), 'repeat');
                let tm = ctx.measureText('x');
                let id = ctx.getImageData(0, 0, 1, 1);

                ctx.constructor.name === 'OffscreenCanvasRenderingContext2D' &&
                ctx instanceof OffscreenCanvasRenderingContext2D &&
                Object.prototype.toString.call(ctx) === '[object OffscreenCanvasRenderingContext2D]' &&

                g.constructor.name === 'CanvasGradient' &&
                g instanceof CanvasGradient &&

                p !== null && p.constructor.name === 'CanvasPattern' &&
                p instanceof CanvasPattern &&

                tm.constructor.name === 'TextMetrics' &&
                tm instanceof TextMetrics &&

                id.constructor.name === 'ImageData' &&
                id instanceof ImageData
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_bitmaprenderer_context_is_real_class_instance() {
        // BUG-617 gap 1: `getContext('bitmaprenderer')` used to return a bare
        // object literal — `instanceof ImageBitmapRenderingContext` threw
        // `ReferenceError` (no such global existed at all), same defect BUG-932
        // fixed for the 2D context/gradient/pattern/text-metrics/image-data set.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(10, 10);
                let ctx = canvas.getContext('bitmaprenderer');
                ctx !== null &&
                ctx.canvas === canvas &&
                ctx.constructor.name === 'ImageBitmapRenderingContext' &&
                ctx instanceof ImageBitmapRenderingContext &&
                Object.prototype.toString.call(ctx) === '[object ImageBitmapRenderingContext]' &&
                canvas.getContext('bitmaprenderer') === ctx  // cached, same instance
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_bitmaprenderer_transfer_from_image_bitmap() {
        // BUG-617 gap 2: `OffscreenCanvas.getContext()` used to hard-code
        // `contextType !== '2d' -> null`, so `'bitmaprenderer'` could never work
        // here even though the on-page `<canvas>` factory already supported it.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let src = new OffscreenCanvas(4, 4);
                let sctx = src.getContext('2d');
                sctx.fillStyle = 'rgb(10, 20, 30)';
                sctx.fillRect(0, 0, 4, 4);
                let bitmap = src.transferToImageBitmap();

                let dst = new OffscreenCanvas(4, 4);
                let brctx = dst.getContext('bitmaprenderer');
                brctx.transferFromImageBitmap(bitmap);

                let dctx = dst.getContext('2d');
                let pixel = dctx.getImageData(0, 0, 1, 1).data;
                pixel[0] === 10 && pixel[1] === 20 && pixel[2] === 30
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_bitmaprenderer_transfer_from_image_bitmap_null_clears() {
        // `transferFromImageBitmap(null)` clears the context's output bitmap
        // instead of throwing (HTML LS §4.12.5.1).
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let dst = new OffscreenCanvas(4, 4);
                let dctx2d = dst.getContext('2d');
                dctx2d.fillStyle = 'rgb(255, 0, 0)';
                dctx2d.fillRect(0, 0, 4, 4);

                let brctx = dst.getContext('bitmaprenderer');
                brctx.transferFromImageBitmap(null);

                let pixel = dctx2d.getImageData(0, 0, 1, 1).data;
                pixel[0] === 0 && pixel[1] === 0 && pixel[2] === 0 && pixel[3] === 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_bitmaprenderer_transfer_from_image_bitmap_invalid_throws() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let dst = new OffscreenCanvas(4, 4);
                let brctx = dst.getContext('bitmaprenderer');
                let threw = false;
                try { brctx.transferFromImageBitmap({}); }
                catch (e) { threw = e instanceof TypeError; }
                threw
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_context_illegal_constructor_throws() {
        // Direct `new OffscreenCanvasRenderingContext2D()` (bypassing
        // getContext) must fail like every other browser-minted interface,
        // not silently succeed with a useless instance.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let threw = false;
                try { new OffscreenCanvasRenderingContext2D(); }
                catch (e) { threw = e instanceof TypeError; }
                threw
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_fill_rect() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(100, 100);
                let ctx = canvas.getContext('2d');
                ctx.fillStyle = '#ff0000';
                ctx.fillRect(10, 10, 50, 50);
                // Should mark canvas as dirty
                true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_transfer_to_image_bitmap() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(10, 10);
                let bitmap = canvas.transferToImageBitmap();
                bitmap.width === 10 && bitmap.height === 10 &&
                typeof bitmap.__canvas_id__ === 'number' &&
                typeof bitmap.close === 'function'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_image_bitmap_is_real_class_instance() {
        // BUG-933: `transferToImageBitmap()` used to return a
        // `{width, height, __canvas_id__, close}` literal — `constructor.name`
        // was 'Object' and `instanceof ImageBitmap` threw a ReferenceError.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(12, 7);
                let bm = canvas.transferToImageBitmap();
                let illegal = false;
                try { new ImageBitmap(); } catch (e) { illegal = e instanceof TypeError; }
                typeof ImageBitmap === 'function' &&
                bm instanceof ImageBitmap &&
                bm.constructor === ImageBitmap &&
                bm.constructor.name === 'ImageBitmap' &&
                Object.prototype.toString.call(bm) === '[object ImageBitmap]' &&
                Object.getPrototypeOf(bm) === ImageBitmap.prototype &&
                !Object.prototype.hasOwnProperty.call(bm, 'close') &&
                !Object.prototype.hasOwnProperty.call(bm, 'width') &&
                Object.keys(bm).length === 0 &&
                bm.width === 12 && bm.height === 7 &&
                typeof bm.__canvas_id__ === 'number' && bm.__canvas_id__ > 0 &&
                illegal
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_image_bitmap_close_detaches_once() {
        // BUG-933: close() zeroes width/height (HTML LS §8.10 getter steps),
        // a second close() is a no-op, and a closed bitmap still reaches the
        // native detach check in transferFromImageBitmap (InvalidStateError,
        // not the TypeError reserved for "not an ImageBitmap at all").
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let src = new OffscreenCanvas(4, 4);
                let bm = src.transferToImageBitmap();
                bm.close();
                bm.close();
                let dst = new OffscreenCanvas(4, 4);
                let brctx = dst.getContext('bitmaprenderer');
                let name = '';
                try { brctx.transferFromImageBitmap(bm); } catch (e) { name = e.name; }
                let brand = '';
                try { ImageBitmap.prototype.close.call({}); } catch (e) { brand = e.name; }
                bm.width === 0 && bm.height === 0 &&
                name === 'InvalidStateError' && brand === 'TypeError'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_create_image_bitmap_resolves_image_bitmap() {
        // BUG-933: both createImageBitmap resolve paths (plain and cropped)
        // mint the class, not a literal.
        let rt = with_offscreen();
        rt.eval(
            r#"
                var _b933_plain = null, _b933_crop = null, _b933_err = '';
                let c = new OffscreenCanvas(8, 6);
                createImageBitmap(c).then(function(b) { _b933_plain = b; },
                                          function(e) { _b933_err += e; });
                createImageBitmap(c, 1, 1, 3, 2).then(function(b) { _b933_crop = b; },
                                                      function(e) { _b933_err += e; });
            "#,
        )
        .unwrap();
        let ok = bool_eval(
            &rt,
            r#"
                _b933_err === '' &&
                _b933_plain instanceof ImageBitmap &&
                _b933_plain.width === 8 && _b933_plain.height === 6 &&
                _b933_crop instanceof ImageBitmap &&
                _b933_crop.width === 3 && _b933_crop.height === 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_invalid_context_type() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                let canvas = new OffscreenCanvas(50, 50);
                canvas.getContext('webgl') === null
            "#,
        );
        assert!(ok);
    }

    // ── Phase 1: createImageBitmap + Worker availability tests ────────────────

    #[test]
    fn native_from_image_data_valid_2x2() {
        // 2×2 RGBA pixels, all transparent black
        let hex = "0".repeat(32); // 16 bytes = 32 hex chars
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            &format!("_lumen_offscreen_canvas_from_image_data(2, 2, '{hex}') > 0"),
        );
        assert!(ok, "expected non-zero canvas_id from valid 2x2 image data");
    }

    #[test]
    fn native_from_image_data_size_mismatch_returns_zero() {
        let rt = with_offscreen();
        // 3×3 requested but only 4 bytes provided → mismatch
        let ok = bool_eval(
            &rt,
            "_lumen_offscreen_canvas_from_image_data(3, 3, 'aabbccdd') === 0",
        );
        assert!(ok, "size mismatch should return 0");
    }

    #[test]
    fn native_from_image_data_red_pixel_stored() {
        let rt = with_offscreen();
        // 1×1 canvas with red pixel (ff0000ff); read back via the same wire
        // format ("{w},{h},{hex}") the JS shim parses, since the V8 runtime
        // owns OFFSCREEN_CANVASES on its own dedicated thread.
        let ok = bool_eval(
            &rt,
            r#"
                var id = _lumen_offscreen_canvas_from_image_data(1, 1, 'ff0000ff');
                var raw = _lumen_offscreen_canvas2d_get_image_data(id);
                id > 0 && raw === '1,1,ff0000ff'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_create_image_bitmap_is_function() {
        let rt = with_offscreen();
        assert!(
            bool_eval(&rt, "typeof createImageBitmap === 'function'"),
            "createImageBitmap should be a function"
        );
    }

    #[test]
    fn js_create_image_bitmap_from_image_data_sync_via_native() {
        // Test the native binding that createImageBitmap(ImageData) uses internally.
        // We bypass the Promise wrapper and test the hex-encode → canvas ID path.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var data = new Uint8Array([
                  100, 150, 200, 255,
                  50,  80,  120, 200,
                  10,  20,  30,  100,
                  0,   0,   0,   0
                ]);
                var hex = '';
                for (var i = 0; i < data.length; i++) {
                  var b = data[i] & 0xff;
                  hex += (b < 16 ? '0' : '') + b.toString(16);
                }
                var cid = _lumen_offscreen_canvas_from_image_data(2, 2, hex);
                cid > 0
            "#,
        );
        assert!(ok, "createImageBitmap inner binding should produce a canvas");
    }

    #[test]
    fn native_transfer_to_image_bitmap_returns_new_canvas_id() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx2d = canvas.getContext('2d');
                ctx2d.fillStyle = '#00ff00';
                ctx2d.fillRect(0, 0, 4, 4);
                var newId = _lumen_offscreen_canvas_transfer_to_image_bitmap(canvas.__canvas_id__);
                newId > 0 && newId !== canvas.__canvas_id__
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_create_image_bitmap_from_offscreen_canvas_does_not_detach_source() {
        // createImageBitmap(OffscreenCanvas) must snapshot pixels without
        // neutering the source (unlike transferToImageBitmap()). Exercises the
        // same read-snapshot-recreate native sequence the JS shim uses,
        // bypassing the Promise wrapper (same rationale as the ImageData test
        // above: no microtask pump in this bare `eval` harness).
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx2d = canvas.getContext('2d');
                ctx2d.fillStyle = '#00ff00';
                ctx2d.fillRect(0, 0, 4, 4);
                var raw = _lumen_offscreen_canvas2d_get_image_data(canvas.__canvas_id__);
                var parts = raw.split(',');
                var snapCid = _lumen_offscreen_canvas_from_image_data(parseInt(parts[0], 10), parseInt(parts[1], 10), parts[2]);
                var bitmapCreated = snapCid > 0 && snapCid !== canvas.__canvas_id__;
                var srcStillReadable = _lumen_offscreen_canvas2d_get_image_data(canvas.__canvas_id__).length > 0;
                bitmapCreated && srcStillReadable
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_canvas_available_in_fresh_context() {
        // Simulates worker thread: fresh JS runtime with OffscreenCanvas installed.
        let rt = with_offscreen();
        assert!(
            bool_eval(
                &rt,
                "typeof OffscreenCanvas === 'function' && typeof createImageBitmap === 'function'"
            ),
            "OffscreenCanvas and createImageBitmap must be available in fresh (worker) context"
        );
    }

    #[test]
    fn js_offscreen_get_image_data_reads_requested_rect() {
        // BUG-456 симптом 3 / BUG-448 twin: getImageData() used to take no
        // parameters at all and hand back the raw "{w},{h},{hex}" wire string.
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                ctx.fillStyle = '#ff0000'; ctx.fillRect(0, 0, 4, 4);
                ctx.fillStyle = '#00ff00'; ctx.fillRect(2, 2, 2, 2);
                var img = ctx.getImageData(2, 2, 2, 2);
                // BUG-454: getImageData is noised now (±1 per RGB channel, alpha exact) —
                // this reads addressing/cropping correctness, not exact colour.
                img.width === 2 && img.height === 2 && img.data.length === 16 &&
                img.colorSpace === 'srgb' &&
                Math.abs(img.data[0] - 0) <= 1 && Math.abs(img.data[1] - 255) <= 1 &&
                Math.abs(img.data[2] - 0) <= 1 && img.data[3] === 255
            "#,
        );
        assert!(ok, "requested 2x2 rect as a real ImageData, not the whole 4x4 as a wire string");
    }

    #[test]
    fn js_offscreen_get_image_data_zero_size_throws_index_size_error() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                var name = '';
                try { ctx.getImageData(0, 0, 0, 4); }
                catch (e) { name = e.name; }
                name === 'IndexSizeError'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_get_image_data_rejects_non_finite_argument() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var canvas = new OffscreenCanvas(4, 4);
                var ctx = canvas.getContext('2d');
                var threw = false;
                try { ctx.getImageData(NaN, 0, 1, 1); }
                catch (e) { threw = e instanceof TypeError; }
                threw
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 2: lineWidth/globalAlpha getters ────────────────────
    #[test]
    fn js_offscreen_line_width_and_global_alpha_round_trip() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.lineWidth = 4;
                ctx.globalAlpha = 0.5;
                ctx.lineWidth === 4 && ctx.globalAlpha === 0.5
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 1: state stack, transforms, additional path ops ────
    #[test]
    fn js_offscreen_save_restore_round_trips_tracked_attributes() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.fillStyle = '#00ff00';
                ctx.lineWidth = 3;
                ctx.save();
                ctx.fillStyle = '#ff0000';
                ctx.lineWidth = 9;
                ctx.restore();
                ctx.fillStyle === '#00ff00' && ctx.lineWidth === 3
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_context_has_transform_and_path_methods() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ['translate', 'rotate', 'scale', 'transform', 'setTransform',
                 'resetTransform', 'rect', 'bezierCurveTo', 'quadraticCurveTo',
                 'arcTo', 'ellipse', 'roundRect', 'clip', 'save', 'restore', 'reset']
                    .every(function(name) { return typeof ctx[name] === 'function'; })
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_transform_ops_do_not_throw() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(20, 20).getContext('2d');
                ctx.save();
                ctx.translate(2, 2);
                ctx.rotate(0.1);
                ctx.scale(1.5, 1.5);
                ctx.transform(1, 0, 0, 1, 1, 1);
                ctx.setTransform(1, 0, 0, 1, 0, 0);
                ctx.setTransform();
                ctx.resetTransform();
                ctx.restore();
                ctx.beginPath();
                ctx.rect(0, 0, 5, 5);
                ctx.bezierCurveTo(1, 1, 2, 2, 3, 3);
                ctx.quadraticCurveTo(1, 1, 2, 2);
                ctx.arcTo(0, 0, 5, 0, 2);
                ctx.ellipse(5, 5, 3, 2, 0, 0, Math.PI * 2);
                ctx.roundRect(0, 0, 10, 10, 2);
                ctx.clip();
                true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_reset_clears_fill_style_and_transform() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.fillStyle = '#00ff00';
                ctx.lineWidth = 7;
                ctx.translate(5, 5);
                ctx.reset();
                ctx.fillStyle === '#000000' && ctx.lineWidth === 1
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 1: state properties (composite op/line cap/line
    // join/miter limit/shadow*) — next slice after the geometry natives.
    #[test]
    fn js_offscreen_state_properties_round_trip() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation = 'multiply';
                ctx.lineCap = 'round';
                ctx.lineJoin = 'bevel';
                ctx.miterLimit = 5;
                ctx.shadowColor = '#ff0000';
                ctx.shadowBlur = 3;
                ctx.shadowOffsetX = 1;
                ctx.shadowOffsetY = 2;
                ctx.globalCompositeOperation === 'multiply' &&
                ctx.lineCap === 'round' && ctx.lineJoin === 'bevel' &&
                ctx.miterLimit === 5 && ctx.shadowColor === '#ff0000' &&
                ctx.shadowBlur === 3 && ctx.shadowOffsetX === 1 &&
                ctx.shadowOffsetY === 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_state_properties_have_spec_defaults() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation === 'source-over' &&
                ctx.lineCap === 'butt' && ctx.lineJoin === 'miter' &&
                ctx.miterLimit === 10 && ctx.shadowColor === 'rgba(0, 0, 0, 0)' &&
                ctx.shadowBlur === 0 && ctx.shadowOffsetX === 0 &&
                ctx.shadowOffsetY === 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_state_properties_survive_save_restore() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.lineCap = 'round';
                ctx.shadowBlur = 4;
                ctx.save();
                ctx.lineCap = 'square';
                ctx.shadowBlur = 9;
                ctx.restore();
                ctx.lineCap === 'round' && ctx.shadowBlur === 4
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_reset_clears_state_properties() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.globalCompositeOperation = 'xor';
                ctx.lineCap = 'square';
                ctx.miterLimit = 2;
                ctx.shadowColor = '#00ff00';
                ctx.shadowOffsetX = 5;
                ctx.reset();
                ctx.globalCompositeOperation === 'source-over' &&
                ctx.lineCap === 'butt' && ctx.miterLimit === 10 &&
                ctx.shadowColor === 'rgba(0, 0, 0, 0)' && ctx.shadowOffsetX === 0
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_miter_limit_ignores_non_positive_value() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.miterLimit = 5;
                ctx.miterLimit = -1;
                ctx.miterLimit = 0;
                ctx.miterLimit = NaN;
                ctx.miterLimit === 5
            "#,
        );
        assert!(ok);
    }

    // ── BUG-456 симптом 1 (remaining scope, 2026-08-31 slice): font/text,
    // drawImage, gradients/patterns, putImageData/createImageData, line-dash
    // and hit-testing stubs — same shape as the geometry/state-property slices
    // above, ported from the element context's twins in `canvas2d.rs`/
    // `web_api_shim_mid.js`.

    #[test]
    fn js_offscreen_font_text_properties_round_trip_and_reset() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                var defaults = ctx.font === '10px sans-serif' &&
                    ctx.textAlign === 'start' && ctx.textBaseline === 'alphabetic';
                ctx.font = '20px sans-serif';
                ctx.textAlign = 'center';
                ctx.textBaseline = 'middle';
                var roundTrip = ctx.font === '20px sans-serif' &&
                    ctx.textAlign === 'center' && ctx.textBaseline === 'middle';
                ctx.save();
                ctx.font = '5px sans-serif';
                ctx.restore();
                var restored = ctx.font === '20px sans-serif';
                ctx.reset();
                var reset = ctx.font === '10px sans-serif' &&
                    ctx.textAlign === 'start' && ctx.textBaseline === 'alphabetic';
                defaults && roundTrip && restored && reset
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_measure_text_reports_positive_width_that_grows_with_font_size() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(200, 50).getContext('2d');
                var m1 = ctx.measureText('Hello');
                ctx.font = '40px sans-serif';
                var m2 = ctx.measureText('Hello');
                m1.width > 0 && m2.width > m1.width &&
                typeof m1.actualBoundingBoxAscent === 'number' &&
                typeof m1.fontBoundingBoxDescent === 'number'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_fill_text_and_stroke_text_do_not_throw() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(100, 50).getContext('2d');
                ctx.fillStyle = '#00ff00';
                ctx.fillText('Hi', 5, 20);
                ctx.strokeText('Hi', 5, 40);
                true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_draw_image_from_another_offscreen_canvas() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var src = new OffscreenCanvas(4, 4);
                var sctx = src.getContext('2d');
                sctx.fillStyle = '#ff0000';
                sctx.fillRect(0, 0, 4, 4);
                var dst = new OffscreenCanvas(8, 8);
                var dctx = dst.getContext('2d');
                dctx.drawImage(src, 0, 0);
                var img = dctx.getImageData(0, 0, 1, 1);
                Math.abs(img.data[0] - 255) <= 1 && Math.abs(img.data[1] - 0) <= 1
            "#,
        );
        assert!(ok, "drawImage(OffscreenCanvas) should copy the source's pixels");
    }

    #[test]
    fn js_offscreen_create_image_data_and_put_image_data_round_trip() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(4, 4).getContext('2d');
                var img = ctx.createImageData(2, 2);
                var shapeOk = img.width === 2 && img.height === 2 && img.data.length === 16;
                img.data[0] = 0; img.data[1] = 255; img.data[2] = 0; img.data[3] = 255;
                img.data[4] = 0; img.data[5] = 255; img.data[6] = 0; img.data[7] = 255;
                img.data[8] = 0; img.data[9] = 255; img.data[10] = 0; img.data[11] = 255;
                img.data[12] = 0; img.data[13] = 255; img.data[14] = 0; img.data[15] = 255;
                ctx.putImageData(img, 1, 1);
                var back = ctx.getImageData(1, 1, 1, 1);
                shapeOk && Math.abs(back.data[1] - 255) <= 1 && back.data[3] === 255
            "#,
        );
        assert!(ok, "createImageData/putImageData should round-trip pixels");
    }

    #[test]
    fn js_offscreen_create_image_data_copy_form_and_zero_size_throws() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(4, 4).getContext('2d');
                var src = ctx.createImageData(3, 5);
                var copy = ctx.createImageData(src);
                var copyOk = copy.width === 3 && copy.height === 5 && copy.data[0] === 0;
                var threw = false;
                try { ctx.createImageData(0, 4); }
                catch (e) { threw = e.name === 'IndexSizeError'; }
                copyOk && threw
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_linear_gradient_paints_and_round_trips_as_fill_style() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                var g = ctx.createLinearGradient(0, 0, 10, 0);
                g.addColorStop(0, '#ff0000');
                g.addColorStop(1, '#0000ff');
                ctx.fillStyle = g;
                var roundTrip = ctx.fillStyle === g && typeof g.__gid__ === 'number';
                ctx.fillRect(0, 0, 10, 10);
                var left = ctx.getImageData(0, 0, 1, 1);
                var right = ctx.getImageData(9, 0, 1, 1);
                roundTrip && left.data[0] > right.data[0] && right.data[2] > left.data[2]
            "#,
        );
        assert!(ok, "a CanvasGradient fillStyle should paint the interpolated colours");
    }

    #[test]
    fn js_offscreen_radial_and_conic_gradients_are_functions_and_produce_objects() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                var rg = ctx.createRadialGradient(5, 5, 0, 5, 5, 5);
                var cg = ctx.createConicGradient(0, 5, 5);
                typeof rg.addColorStop === 'function' && typeof rg.__gid__ === 'number' &&
                typeof cg.addColorStop === 'function' && typeof cg.__gid__ === 'number' &&
                rg.__gid__ !== cg.__gid__
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_gradient_add_color_stop_rejects_out_of_range_offset() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                var g = ctx.createLinearGradient(0, 0, 1, 0);
                var threw = false;
                try { g.addColorStop(1.5, '#fff'); }
                catch (e) { threw = e.name === 'IndexSizeError'; }
                threw
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_pattern_from_another_canvas_paints_and_round_trips_as_fill_style() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var src = new OffscreenCanvas(2, 2);
                var sctx = src.getContext('2d');
                sctx.fillStyle = '#00ff00';
                sctx.fillRect(0, 0, 2, 2);
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                var p = ctx.createPattern(src, 'repeat');
                ctx.fillStyle = p;
                var roundTrip = ctx.fillStyle === p && typeof p.__patid__ === 'number';
                ctx.fillRect(0, 0, 10, 10);
                var pixel = ctx.getImageData(4, 4, 1, 1);
                roundTrip && Math.abs(pixel.data[1] - 255) <= 1 && Math.abs(pixel.data[0] - 0) <= 1
            "#,
        );
        assert!(ok, "a CanvasPattern fillStyle should tile the source canvas's pixels");
    }

    #[test]
    fn js_offscreen_create_pattern_returns_null_for_invalid_source() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.createPattern(null, 'repeat') === null &&
                ctx.createPattern({}, 'repeat') === null
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn js_offscreen_line_dash_and_hit_test_stubs_exist_and_do_not_throw() {
        let rt = with_offscreen();
        let ok = bool_eval(
            &rt,
            r#"
                var ctx = new OffscreenCanvas(10, 10).getContext('2d');
                ctx.setLineDash([1, 2]);
                var dash = ctx.getLineDash();
                Array.isArray(dash) && dash.length === 0 &&
                ctx.isPointInPath(1, 1) === false &&
                ctx.isPointInStroke(1, 1) === false
            "#,
        );
        assert!(ok);
    }
