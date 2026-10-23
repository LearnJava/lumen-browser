//! Types shared by the layers of the CSS Painting API (Houdini) pipeline.
//!
//! `lumen-paint` finds the `background-image: paint()` layers of a laid-out page
//! and describes each as a [`PaintRequest`]; `lumen-js` (the paint worklet) draws
//! it and hands back a [`PaintImage`]; the shell carries both across without either
//! crate knowing the other. They live here because the two ends sit in different
//! branches of the dependency graph.

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

/// A bitmap drawn by a paint worklet: `(key, width, height, rgba)`.
pub type PaintImage = (String, u32, u32, Vec<u8>);
