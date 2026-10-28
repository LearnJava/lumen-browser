//! PAINT-WORKLET — `Worklet`, `CSS.paintWorklet.addModule()`, `registerPaint()` and the
//! pull renderer behind `set_paint_jobs` / `flush_paint_images` (`shim/paint_worklet.js`).
//! The computed style of the painted element is replaced with a fake element, as in
//! `v8_cssanim_model`.

use super::*;
use lumen_core::PaintRequest;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_bool(rt: &V8JsRuntime, src: &str) -> bool {
    matches!(rt.eval(src).unwrap(), lumen_core::JsValue::Bool(true))
}

/// A module in a blob URL, loaded through `addModule`; the load settles in microtasks, so the
/// result is read by a second `eval`.
fn add_module(rt: &V8JsRuntime, module_src: &str) {
    let quoted = serde_json::Value::String(module_src.to_string()).to_string();
    rt.eval(&format!(
        "globalThis.__done = false; globalThis.__err = null; \
         var __u = URL.createObjectURL(new Blob([{quoted}], {{type: 'text/javascript'}})); \
         CSS.paintWorklet.addModule(__u).then(function(){{ __done = true; }}, \
                                              function(e){{ __err = String(e); }});"
    ))
    .unwrap();
    rt.eval("0").unwrap();
}

fn job(key: &str, name: &str, args: &[&str], w: f32, h: f32, dpr: f32) -> PaintRequest {
    PaintRequest {
        key: key.into(),
        node: 1,
        name: name.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        width: w,
        height: h,
        dpr,
    }
}

const FAKE_EL: &str = "var __vals = {}; _lumen_make_element = function(nid) { return { __nid__: nid, \
    computedStyleMap: function() { return { \
      get: function(p) { return __vals[p]; }, \
      __lookup__: function(p) { return p in __vals ? String(__vals[p]) : ''; }, \
      getAll: function(p) { return p in __vals ? [__vals[p]] : []; } }; } }; };";

#[test]
fn paint_worklet_is_a_worklet_and_registerpaint_stays_off_window() {
    let rt = rt();
    assert!(eval_bool(
        &rt,
        "typeof Worklet === 'function' && CSS.paintWorklet instanceof Worklet \
         && typeof CSS.paintWorklet.addModule === 'function' \
         && CSS.paintWorklet === CSS.paintWorklet \
         && typeof globalThis.registerPaint === 'undefined'"
    ));
}

#[test]
fn add_module_resolves_for_a_blob_module() {
    let rt = rt();
    add_module(&rt, "registerPaint('foo', class { paint(){} })");
    assert!(eval_bool(&rt, "__done === true"), "addModule did not resolve");
}

#[test]
fn add_module_rejects_with_abort_error_when_the_module_throws() {
    let rt = rt();
    add_module(&rt, "throw new Error('boom')");
    assert!(eval_bool(&rt, "__done === false && __err.indexOf('AbortError') >= 0"));
}

#[test]
fn register_paint_twice_with_one_name_throws() {
    let rt = rt();
    add_module(&rt, "registerPaint('dup', class { paint(){} }); registerPaint('dup', class { paint(){} });");
    assert!(eval_bool(&rt, "__done === false && __err.indexOf('already registered') >= 0"));
}

#[test]
fn register_paint_requires_a_paint_method() {
    let rt = rt();
    add_module(&rt, "registerPaint('nopaint', class { })");
    assert!(eval_bool(&rt, "__done === false && __err.indexOf('paint()') >= 0"));
}

#[test]
fn jobs_render_through_the_registered_class_and_publish_pixels() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(
        &rt,
        "registerPaint('foo', class { paint(ctx, g){ \
           ctx.fillStyle = 'rgb(0,255,0)'; ctx.fillRect(0, 0, g.width, g.height); } })",
    );
    rt.set_paint_jobs(&[job("paint:1:k", "foo", &[], 4.0, 2.0, 1.0)]);
    let images = rt.flush_paint_images();
    assert_eq!(images.len(), 1, "one paint() layer must publish one bitmap");
    let (key, w, h, rgba) = &images[0];
    assert_eq!((key.as_str(), *w, *h), ("paint:1:k", 4, 2));
    assert_eq!(&rgba[..4], &[0, 255, 0, 255], "the worklet filled the tile green");
    assert!(rt.flush_paint_images().is_empty(), "an unchanged layer is not redrawn");
}

#[test]
fn input_properties_and_arguments_reach_paint_and_changes_redraw() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(
        &rt,
        "registerPaint('col', class { static get inputProperties() { return ['--c']; } \
           paint(ctx, g, props, args){ ctx.fillStyle = String(props.get('--c')); \
             ctx.fillRect(0, 0, g.width, g.height); \
             if (String(args[0]) !== '7px') { ctx.fillStyle = 'red'; ctx.fillRect(0, 0, 1, 1); } } })",
    );
    rt.eval("__vals['--c'] = 'rgb(0,0,255)';").unwrap();
    rt.set_paint_jobs(&[job("paint:1:a", "col", &["7px"], 2.0, 2.0, 1.0)]);
    let first = rt.flush_paint_images();
    assert_eq!(&first[0].3[..4], &[0, 0, 255, 255]);
    rt.eval("__vals['--c'] = 'rgb(255,0,255)';").unwrap();
    rt.set_paint_jobs(&[job("paint:1:a", "col", &["7px"], 2.0, 2.0, 1.0)]);
    let second = rt.flush_paint_images();
    assert_eq!(second.len(), 1, "a changed inputProperties value redraws the layer");
    assert_eq!(&second[0].3[..4], &[255, 0, 255, 255]);
}

#[test]
fn dpr_scales_the_bitmap_not_the_paint_size() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(
        &rt,
        "registerPaint('sz', class { paint(ctx, g){ ctx.fillStyle = 'rgb(10,20,30)'; \
           ctx.fillRect(0, 0, g.width, g.height); \
           globalThis.__seen = g.width + 'x' + g.height; } })",
    );
    rt.set_paint_jobs(&[job("paint:1:d", "sz", &[], 3.0, 2.0, 2.0)]);
    let images = rt.flush_paint_images();
    assert_eq!((images[0].1, images[0].2), (6, 4));
    assert!(eval_bool(&rt, "__seen === '3x2'"));
    assert_eq!(&images[0].3[..4], &[10, 20, 30, 255], "the whole 2x tile is covered");
}

#[test]
fn a_throwing_paint_publishes_nothing() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(&rt, "registerPaint('bad', class { paint(){ throw new Error('x'); } })");
    rt.set_paint_jobs(&[job("paint:1:b", "bad", &[], 2.0, 2.0, 1.0)]);
    assert!(rt.flush_paint_images().is_empty());
}

#[test]
fn register_paint_twice_is_an_invalid_modification_error() {
    let rt = rt();
    add_module(
        &rt,
        "try { registerPaint('foo', class { paint(){} }); registerPaint('foo', class { paint(){} }); } \
         catch (e) { globalThis.__name = e.name; }",
    );
    assert!(eval_bool(&rt, "__name === 'InvalidModificationError'"));
}

#[test]
fn a_module_with_top_level_await_still_registers() {
    let rt = rt();
    add_module(&rt, "await 1; registerPaint('late', class { paint(){} }); globalThis.__after = true;");
    rt.eval("0").unwrap();
    assert!(eval_bool(&rt, "__done === true && __after === true"), "{:?}", rt.eval("String(__err)"));
}

#[test]
fn dynamic_import_in_a_worklet_rejects_with_a_type_error() {
    let rt = rt();
    add_module(
        &rt,
        "registerPaint('imp', class { async paint() { \
           try { await import('./x.mjs'); globalThis.__imp = 'resolved'; } \
           catch (e) { globalThis.__imp = e.constructor === TypeError ? 'TypeError' : 'other'; } } })",
    );
    rt.eval(FAKE_EL).unwrap();
    rt.set_paint_jobs(&[job("paint:1:i", "imp", &[], 2.0, 2.0, 1.0)]);
    rt.flush_paint_images();
    assert!(eval_bool(&rt, "__imp === 'TypeError'"));
}

#[test]
fn an_async_paint_publishes_after_it_settles() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(
        &rt,
        "registerPaint('slow', class { async paint(ctx, g) { await 0; \
           ctx.fillStyle = 'rgb(1,2,3)'; ctx.fillRect(0, 0, g.width, g.height); } })",
    );
    rt.set_paint_jobs(&[job("paint:1:s", "slow", &[], 2.0, 2.0, 1.0)]);
    let images = rt.flush_paint_images();
    assert_eq!(images.len(), 1);
    assert_eq!(&images[0].3[..4], &[1, 2, 3, 255], "pixels drawn after the await are published");
}

#[test]
fn a_class_whose_input_arguments_differ_between_reads_is_never_painted() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    add_module(
        &rt,
        "var __n = 0; registerPaint('flaky', class { static get inputArguments() { return ['x' + (__n++)]; } \
           paint(ctx){ ctx.fillStyle = 'red'; ctx.fillRect(0, 0, 1, 1); } })",
    );
    rt.set_paint_jobs(&[job("paint:1:f", "flaky", &[], 2.0, 2.0, 1.0)]);
    assert!(rt.flush_paint_images().is_empty());
}

#[test]
fn custom_properties_reach_the_worklet_as_typed_values() {
    let rt = rt();
    rt.eval(FAKE_EL).unwrap();
    rt.eval(
        "CSS.registerProperty({ name: '--len', syntax: '<length>+ | none', initialValue: 'none', inherits: false }); \
         CSS.registerProperty({ name: '--lp', syntax: '<length-percentage>', initialValue: '0px', inherits: false }); \
         CSS.registerProperty({ name: '--img', syntax: '<image> | none', initialValue: 'none', inherits: false }); \
         __vals['--len'] = '8px 16px'; __vals['--lp'] = 'calc(10% + 10px)'; \
         __vals['--img'] = 'url(\"http://a/\")'; __vals['--free'] = 'bar';",
    )
    .unwrap();
    add_module(
        &rt,
        "registerPaint('typed', class { static get inputProperties() { return ['--len', '--lp', '--img', '--free', '--none']; } \
           paint(ctx, g, m) { var s = v => v.constructor.name + ' ' + v; \
             globalThis.__seen = ['--len', '--lp', '--img', '--free', '--none'] \
               .map(k => k + '=' + m.getAll(k).map(s).join(',')).join(' | '); } })",
    );
    rt.set_paint_jobs(&[job("paint:1:t", "typed", &[], 2.0, 2.0, 1.0)]);
    rt.flush_paint_images();
    assert!(eval_bool(
        &rt,
        "__seen === '--len=CSSUnitValue 8px,CSSUnitValue 16px | --lp=CSSMathSum calc(10% + 10px) \
         | --img=CSSImageValue url(\"http://a/\") | --free=CSSUnparsedValue bar | --none=CSSUnparsedValue '"
    ), "{:?}", rt.eval("__seen"));
}

#[test]
fn offscreen_get_transform_tracks_the_matrix_and_ignores_non_finite_input() {
    let rt = rt();
    assert!(eval_bool(
        &rt,
        "var c = new OffscreenCanvas(4, 4).getContext('2d'); \
         c.translate(3, 4); c.scale(2, 2); var m = c.getTransform(); \
         c.setTransform({ a: Infinity, d: 5 }); c.setTransform(1, 0, 0, NaN, 0, 0); var kept = c.getTransform(); \
         c.save(); c.setTransform({ a: 3, d: 3 }); c.restore(); var restored = c.getTransform(); \
         m.a === 2 && m.d === 2 && m.e === 3 && m.f === 4 && kept.a === 2 && restored.a === 2 && restored.e === 3"
    ));
}

#[test]
fn offscreen_stroke_and_fill_take_a_path2d() {
    let rt = rt();
    assert!(eval_bool(
        &rt,
        "var c = new OffscreenCanvas(8, 8).getContext('2d'); \
         var p = new Path2D(); p.rect(0, 0, 8, 8); c.fillStyle = 'rgb(0,255,0)'; c.fill(p); \
         var px = c.getImageData(4, 4, 1, 1).data; px[1] >= 253 && px[0] <= 2 && px[3] >= 253"
    ));
}

#[test]
fn alpha_false_canvases_start_and_stay_opaque() {
    let rt = rt();
    assert!(eval_bool(
        &rt,
        "var c = new OffscreenCanvas(2, 2).getContext('2d', { alpha: false }); \
         c.clearRect(0, 0, 2, 2); var px = c.getImageData(0, 0, 1, 1).data; \
         var t = new OffscreenCanvas(2, 2).getContext('2d').getImageData(0, 0, 1, 1).data; \
         globalThis.__r = [px[3], px[0], t[3]].join(); px[3] >= 253 && px[0] <= 2 && t[3] <= 2"
    ), "{:?}", rt.eval("String(globalThis.__r)"));
}
