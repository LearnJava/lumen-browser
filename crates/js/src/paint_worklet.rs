//! CSS Painting API (Houdini) host side.
//!
//! The JS half lives in `shim/paint_worklet.js` (`Worklet`, `CSS.paintWorklet`,
//! `registerPaint`, the renderer). This file wires it to the runtime: it installs
//! the shim, hands the shim the `paint()` layers of the laid-out page
//! ([`PaintJob`] → `__lumen_paint_set_requests`) and drains the bitmaps the shim
//! drew ([`take_published`]). Everything is per JS thread (`thread_local!`), like
//! the canvas registries it sits next to.

use std::cell::RefCell;

/// One `background-image: paint()` layer the page needs a bitmap for.
///
/// Mirrors `lumen_paint::PaintRequest` (this crate sits below `lumen-paint`, so the
/// shell converts between the two).
#[derive(Debug, Clone, PartialEq)]
pub struct PaintJob {
    /// Renderer image key the bitmap is registered under.
    pub key: String,
    /// Arena index of the element whose computed style feeds `inputProperties`.
    pub node: u32,
    /// Name given to `registerPaint()`.
    pub name: String,
    /// Raw `paint()` arguments after the name.
    pub args: Vec<String>,
    /// Paint-size width in CSS px.
    pub width: f32,
    /// Paint-size height in CSS px.
    pub height: f32,
    /// Device pixel ratio the bitmap is rendered at.
    pub dpr: f32,
}

/// A bitmap drawn by a paint worklet: `(key, width, height, rgba)`.
pub type PaintImage = (String, u32, u32, Vec<u8>);

thread_local! {
    /// Bitmaps published by `_lumen_paint_publish`, drained by the host.
    static PUBLISHED: RefCell<Vec<PaintImage>> = const { RefCell::new(Vec::new()) };
}

/// Drain the bitmaps published since the last call.
pub(crate) fn take_published() -> Vec<PaintImage> {
    PUBLISHED.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

/// `_lumen_paint_publish(key, canvasId)`: move the pixels of a finished
/// `OffscreenCanvas` into the publish queue (the canvas is consumed).
#[cfg(feature = "v8-backend")]
fn publish(key: String, canvas_id: u32) {
    if let Some((w, h, rgba)) = crate::offscreen_canvas::take_offscreen_pixels(canvas_id) {
        PUBLISHED.with(|p| p.borrow_mut().push((key, w, h, rgba)));
    }
}

/// JSON array the shim's `__lumen_paint_set_requests` parses.
pub(crate) fn jobs_to_json(jobs: &[PaintJob]) -> String {
    let items: Vec<serde_json::Value> = jobs
        .iter()
        .map(|j| {
            serde_json::json!({
                "key": j.key,
                "node": j.node,
                "name": j.name,
                "args": j.args,
                "w": j.width,
                "h": j.height,
                "dpr": j.dpr,
            })
        })
        .collect();
    serde_json::Value::Array(items).to_string()
}

#[cfg(feature = "v8-backend")]
const PAINT_WORKLET_SHIM: &str = include_str!("shim/paint_worklet.js");

/// Install `Worklet`, `CSS.paintWorklet` and the paint renderer.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_paint_worklet_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn2;
    use lumen_core::ext::JsRuntime as _;
    rt.register_native("_lumen_paint_publish", into_v8_fn2(publish))?;
    rt.eval(PAINT_WORKLET_SHIM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_serialise_to_the_shape_the_shim_reads() {
        let job = PaintJob {
            key: "paint:1:ab".into(),
            node: 7,
            name: "foo".into(),
            args: vec!["1px".into()],
            width: 50.0,
            height: 20.5,
            dpr: 2.0,
        };
        let v: serde_json::Value = serde_json::from_str(&jobs_to_json(&[job])).unwrap();
        assert_eq!(v[0]["key"], "paint:1:ab");
        assert_eq!(v[0]["node"], 7);
        assert_eq!(v[0]["args"][0], "1px");
        assert_eq!(v[0]["w"], 50.0);
        assert_eq!(v[0]["h"], 20.5);
    }

    // ── JS integration ────────────────────────────────────────────────────────

    #[cfg(feature = "v8-backend")]
    fn runtime() -> crate::v8_runtime::V8JsRuntime {
        use lumen_core::ext::JsRuntime as _;
        let rt = crate::v8_runtime::V8JsRuntime::new().unwrap();
        rt.eval("if (!globalThis.CSS) { globalThis.CSS = {}; }").unwrap();
        install_paint_worklet_api_v8(&rt).unwrap();
        rt
    }

    #[cfg(feature = "v8-backend")]
    fn pump(rt: &crate::v8_runtime::V8JsRuntime) {
        use lumen_core::ext::JsRuntime as _;
        // Microtasks drain when an `eval` returns; the module load chains a few promises.
        for _ in 0..6 {
            rt.eval("0").unwrap();
        }
    }

    #[cfg(feature = "v8-backend")]
    fn eval_bool(rt: &crate::v8_runtime::V8JsRuntime, src: &str) -> bool {
        use lumen_core::ext::JsRuntime as _;
        use lumen_core::JsValue;
        matches!(rt.eval(src).unwrap(), JsValue::Bool(true))
    }

    #[test]
    #[cfg(feature = "v8-backend")]
    fn paint_worklet_is_a_worklet_with_add_module() {
        let rt = runtime();
        assert!(eval_bool(
            &rt,
            "typeof Worklet === 'function' && CSS.paintWorklet instanceof Worklet \
             && typeof CSS.paintWorklet.addModule === 'function' \
             && CSS.paintWorklet === CSS.paintWorklet \
             && typeof globalThis.registerPaint === 'undefined'"
        ));
    }

    #[test]
    #[cfg(feature = "v8-backend")]
    fn add_module_returns_a_promise_and_rejects_unknown_blob() {
        let rt = runtime();
        assert!(eval_bool(
            &rt,
            "var p = CSS.paintWorklet.addModule('blob:null/00000000-0000-4000-8000-000000000000'); \
             p instanceof Promise"
        ));
    }

    #[test]
    #[cfg(feature = "v8-backend")]
    fn add_module_runs_a_blob_module_and_registers() {
        use lumen_core::ext::JsRuntime as _;
        let rt = runtime();
        // Run the whole flow synchronously where possible: the module is evaluated in a
        // microtask, so drain with a second eval after resolving.
        rt.eval(
            "globalThis.__done = false; globalThis.__err = null; \
             var u = URL.createObjectURL(new Blob([\"registerPaint('foo', class { paint(){} })\"], \
                                                   {type: 'text/javascript'})); \
             CSS.paintWorklet.addModule(u).then(function(){ __done = true; }, function(e){ __err = String(e); });",
        )
        .unwrap();
        pump(&rt);
        assert!(eval_bool(&rt, "__done === true"), "addModule did not resolve");
    }

    #[test]
    #[cfg(feature = "v8-backend")]
    fn requests_render_through_registered_class_and_publish_pixels() {
        use lumen_core::ext::JsRuntime as _;
        let rt = runtime();
        rt.eval(
            "globalThis._lumen_make_element = function(){ return { computedStyleMap: function(){                return { get: function(){ return undefined; }, getAll: function(){ return []; } }; } }; };",
        )
        .unwrap();
        rt.eval(
            "var u = URL.createObjectURL(new Blob([\"registerPaint('foo', class { paint(ctx, g){ \
               ctx.fillStyle = 'rgb(0,255,0)'; ctx.fillRect(0, 0, g.width, g.height); } })\"], \
               {type: 'text/javascript'})); CSS.paintWorklet.addModule(u);",
        )
        .unwrap();
        pump(&rt);
        rt.set_paint_jobs(&[PaintJob {
            key: "paint:1:k".into(),
            node: 1,
            name: "foo".into(),
            args: vec![],
            width: 4.0,
            height: 2.0,
            dpr: 1.0,
        }]);
        let images = rt.flush_paint_images();
        assert_eq!(images.len(), 1, "one paint() layer must publish one bitmap");
        let (key, w, h, rgba) = &images[0];
        assert_eq!((key.as_str(), *w, *h), ("paint:1:k", 4, 2));
        assert_eq!(&rgba[..4], &[0, 255, 0, 255], "the worklet filled the tile green");
        assert!(rt.flush_paint_images().is_empty(), "an unchanged layer is not redrawn");
    }
}
