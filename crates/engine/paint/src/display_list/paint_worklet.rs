//! CSS Paint API (Houdini) — `background-image: paint(name, args…)`.
//!
//! The display list never runs script. A `paint()` layer is emitted as an ordinary
//! `DrawBackgroundImage` whose `src` is a key from [`paint_image_key`] and whose
//! `size` is the already-resolved tile size; the bitmap itself comes from the paint
//! worklet host (the JS runtime), which the page driver feeds with the
//! [`PaintRequest`]s [`collect_paint_requests`] returns and registers the result
//! under the same key. A key nobody registered paints nothing (same as an unfetched
//! `url()` background).

use lumen_core::geom::Rect;
use lumen_layout::{BackgroundImage, BackgroundLayer, BackgroundSize, BgSizeAxis, LayoutBox};

use super::background_origin_rect;

/// One image a paint worklet has to draw: what to call, with which arguments, at
/// which size, and under which key to hand the bitmap back.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintRequest {
    /// Renderer image-set key (`DrawBackgroundImage.src`) the bitmap is registered under.
    pub key: String,
    /// Arena index of the element whose computed style feeds `inputProperties`.
    pub node: u32,
    /// Name given to `registerPaint()`.
    pub name: String,
    /// Raw `paint()` arguments after the name (`inputArguments`).
    pub args: Vec<String>,
    /// Paint-size width in CSS px (the tile the layer draws).
    pub width: f32,
    /// Paint-size height in CSS px.
    pub height: f32,
    /// Device pixel ratio: the bitmap is rendered at `width × dpr` by `height × dpr`.
    pub dpr: f32,
}

/// CSS Images L3 §5.3 default sizing of an image with no intrinsic size or ratio
/// (a `paint()` image): an `auto` axis — and `cover`/`contain` — fills the
/// positioning area, a percentage resolves against it.
pub(super) fn paint_tile_size(size: BackgroundSize, origin: Rect) -> (f32, f32) {
    match size {
        BackgroundSize::Length(w, h) => {
            let axis = |a: BgSizeAxis, area: f32| a.resolve(area).unwrap_or(area).max(0.0);
            (axis(w, origin.width), axis(h, origin.height))
        }
        _ => (origin.width, origin.height),
    }
}

/// Key under which the bitmap of one `paint()` layer of element `node` is registered.
///
/// Folds in everything the bitmap depends on that the emitter knows — worklet
/// name, arguments, tile size — so two layers of one element never share a key
/// unless they would draw the same picture.
pub(super) fn paint_image_key(node: usize, name: &str, args: &[String], width: f32, height: f32) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    args.hash(&mut h);
    width.to_bits().hash(&mut h);
    height.to_bits().hash(&mut h);
    format!("paint:{node}:{:016x}", h.finish())
}

/// Request for `layer` of `b`, or `None` if the layer is not a non-empty `paint()` image.
fn request_for(b: &LayoutBox, layer: &BackgroundLayer, dpr: f32) -> Option<PaintRequest> {
    let BackgroundImage::Paint { name, args } = &layer.image else { return None };
    let origin = background_origin_rect(b, layer.origin);
    let (width, height) = paint_tile_size(layer.size, origin);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let node = b.node.index();
    Some(PaintRequest {
        key: paint_image_key(node, name, args, width, height),
        node: u32::try_from(node).ok()?,
        name: name.clone(),
        args: args.clone(),
        width,
        height,
        dpr,
    })
}

/// Every `background-image: paint()` layer of the laid-out tree, in document order,
/// duplicates (same key) removed. `dpr` is the device pixel ratio of the surface the
/// page is painted on.
///
/// Geometry is the one [`super::emit_background_layer`] derives from the same box,
/// so the keys match what the display list refers to; only
/// `background-attachment: fixed`/`local` layers (positioned against the viewport /
/// the scroll area) would differ, and those fall back to a key nobody registers.
#[must_use]
pub fn collect_paint_requests(root: &LayoutBox, dpr: f32) -> Vec<PaintRequest> {
    let mut out: Vec<PaintRequest> = Vec::new();
    let mut stack: Vec<&LayoutBox> = vec![root];
    while let Some(b) = stack.pop() {
        for layer in &b.style.background_layers {
            if let Some(req) = request_for(b, layer, dpr)
                && !out.iter().any(|r| r.key == req.key)
            {
                out.push(req);
            }
        }
        stack.extend(b.children.iter().rev());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_size_fills_the_positioning_area() {
        let origin = Rect::new(0.0, 0.0, 200.0, 100.0);
        assert_eq!(paint_tile_size(BackgroundSize::Auto, origin), (200.0, 100.0));
        assert_eq!(paint_tile_size(BackgroundSize::Cover, origin), (200.0, 100.0));
        assert_eq!(paint_tile_size(BackgroundSize::Contain, origin), (200.0, 100.0));
    }

    #[test]
    fn explicit_size_resolves_percent_and_auto_axes() {
        let origin = Rect::new(0.0, 0.0, 200.0, 100.0);
        let size = BackgroundSize::Length(BgSizeAxis::Percent(0.5), BgSizeAxis::Px(30.0));
        assert_eq!(paint_tile_size(size, origin), (100.0, 30.0));
        let size = BackgroundSize::Length(BgSizeAxis::Px(40.0), BgSizeAxis::Auto);
        assert_eq!(paint_tile_size(size, origin), (40.0, 100.0));
    }

    #[test]
    fn key_depends_on_name_args_and_size() {
        let a = paint_image_key(3, "foo", &[], 10.0, 10.0);
        assert_eq!(a, paint_image_key(3, "foo", &[], 10.0, 10.0));
        assert_ne!(a, paint_image_key(3, "bar", &[], 10.0, 10.0));
        assert_ne!(a, paint_image_key(3, "foo", &["1".into()], 10.0, 10.0));
        assert_ne!(a, paint_image_key(3, "foo", &[], 10.0, 11.0));
        assert!(a.starts_with("paint:3:"));
    }
}
