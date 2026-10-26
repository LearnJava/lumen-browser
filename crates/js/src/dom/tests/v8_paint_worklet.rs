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
