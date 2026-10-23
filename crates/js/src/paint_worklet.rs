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
}
