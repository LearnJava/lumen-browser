//! `shape-outside: url()` images handed back to the document after the first
//! layout pass (CSS Shapes L1 §6.2).
//!
//! Moved out of `page_pipeline.rs` unchanged (DEVX-22, file-size gate).

use crate::*;

/// The alpha plane of `image`, one byte per pixel.
fn image_alpha(image: &lumen_image::Image) -> lumen_dom::ImageAlpha {
    lumen_dom::ImageAlpha {
        width: image.width,
        height: image.height,
        alpha: image.to_rgba8().chunks_exact(4).map(|px| px[3]).collect(),
    }
}

/// Hands the decoded `shape-outside: url()` images of `layout` to the document
/// (CSS Shapes L1 §6.2) and lays the page out again when any of them is new: layout
/// has no pixels, a float whose image is unknown wraps as if it had no shape.
#[allow(clippy::too_many_arguments)]
pub(crate) fn report_shape_images(
    layout: LayoutBox,
    images: &[(String, Arc<lumen_image::Image>)],
    doc_arc: &Arc<Mutex<Document>>,
    sheet: &lumen_css_parser::Stylesheet,
    measurer: &lumen_paint::MultiFontMeasurer,
    viewport: Size,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
    media_print: bool,
) -> LayoutBox {
    let urls = lumen_layout::collect_shape_image_requests(&layout);
    if urls.is_empty() {
        return layout;
    }
    let Ok(mut d) = doc_arc.lock() else { return layout };
    let mut changed = false;
    for url in &urls {
        if let Some((_, image)) = images.iter().find(|(key, _)| key == url) {
            changed |= d.set_shape_image(url, image_alpha(image));
        }
    }
    if !changed {
        return layout;
    }
    crate::page_pipeline::layout_page(&d, sheet, measurer, viewport, hp, dark_mode, media_print)
}
