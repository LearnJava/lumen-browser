//! Subtree-scoped introspection (DEVX-12): a `screenshot`/display-list dump
//! restricted to one element's subtree instead of the whole page.
//!
//! Complements DEVX-10's `explain_element` (single node, causal chain) and
//! DEVX-11's `explain_page` (whole page, aggregate counts) with a third
//! granularity — "this one component" — for callers who know which part of
//! the page they care about and want to avoid paying for/reading the rest.
//! Built entirely on already-merged mechanics (DEVX-7 provenance for the
//! display-list dump, `lumen_layout::incremental` for
//! [`crate::session::InProcessSession::relayout_scoped`]) rather than new
//! ones — see `docs/tasks/p1-introspection-track.md` §DEVX-12.
//!
//! [`display_list_scoped`] is session-agnostic (operates on an already-built
//! `&LayoutBox`+`&Document`, same split as [`crate::explain`]); the relayout
//! half of DEVX-12 lives on [`crate::session::InProcessSession`] directly
//! because it needs mutable session state.

use lumen_core::error::{Error, Result};
use lumen_core::geom::Rect;
use lumen_dom::Document;
use lumen_layout::{LayoutBox, PaintOrder, StackingTree};
use lumen_paint::ProvenanceIndex;

/// Display-list commands attributed (via DEVX-7 provenance) to `sel`'s
/// subtree: the matched element's own box plus every descendant box, in
/// original display-list order.
///
/// Returns `None` if `sel` matches nothing — mirrors
/// [`lumen_layout::find_box_by_selector`], which already excludes
/// `BoxKind::Skip` boxes (a `display:none` subtree paints nothing, so there
/// is nothing to scope a dump to).
pub fn display_list_scoped(root: &LayoutBox, doc: &Document, sel: &str) -> Option<String> {
    let target = lumen_layout::find_box_by_selector(root, doc, sel)?;

    let stacking_tree = StackingTree::build(root);
    let order = PaintOrder::from_tree(&stacking_tree);
    let (commands, provenance) = lumen_paint::build_display_list_ordered(root, &stacking_tree, &order);

    // Every box's own emission is a contiguous span (or several, for a box
    // with children — ADR-025 §1 lead/trail split), but sibling/descendant
    // spans interleave with them in the flat command list. Sorting by
    // `range.start` before concatenating restores the original relative
    // order — spans are non-overlapping by construction, so this is exact,
    // not an approximation.
    let mut ranges = Vec::new();
    collect_subtree_spans(target, &provenance, &mut ranges);
    ranges.sort_unstable();
    ranges.dedup();

    let mut scoped = Vec::with_capacity(ranges.iter().map(|(s, e)| e - s).sum());
    for (start, end) in ranges {
        scoped.extend_from_slice(&commands[start..end]);
    }
    Some(lumen_paint::serialize_display_list(&scoped))
}

fn collect_subtree_spans(b: &LayoutBox, provenance: &ProvenanceIndex, out: &mut Vec<(usize, usize)>) {
    for span in provenance.spans_for(b.origin) {
        out.push((span.range.start, span.range.end));
    }
    for child in &b.children {
        collect_subtree_spans(child, provenance, out);
    }
}

/// Crop a full-page PNG screenshot to `border_box` (DEVX-12).
///
/// `border_box` is clamped to the image bounds first — an element partially
/// outside the viewport (absolutely positioned, or below an overflow page)
/// still yields a valid, smaller crop rather than an error.
///
/// # Errors
/// `Err` if `png` doesn't decode, or the clamped rect has zero area (the
/// box is fully outside the screenshot — nothing to crop).
pub fn crop_screenshot(png: &[u8], border_box: Rect) -> Result<Vec<u8>> {
    let image = lumen_image::decode(png)
        .map_err(|e| Error::Other(format!("crop_screenshot: PNG decode: {e}")))?;
    let rgba = image.to_rgba8();
    let (iw, ih) = (image.width as i64, image.height as i64);

    let x0 = (border_box.x.floor() as i64).clamp(0, iw);
    let y0 = (border_box.y.floor() as i64).clamp(0, ih);
    let x1 = ((border_box.x + border_box.width).ceil() as i64).clamp(0, iw);
    let y1 = ((border_box.y + border_box.height).ceil() as i64).clamp(0, ih);
    let (cw, ch) = ((x1 - x0).max(0), (y1 - y0).max(0));
    if cw == 0 || ch == 0 {
        return Err(Error::Other("crop_screenshot: элемент вне области снимка".into()));
    }

    let mut cropped = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y0..y1 {
        let row_start = ((row * iw + x0) * 4) as usize;
        let row_end = row_start + (cw * 4) as usize;
        cropped.extend_from_slice(&rgba[row_start..row_end]);
    }

    let cropped_image = lumen_image::Image {
        width: cw as u32,
        height: ch as u32,
        format: lumen_image::PixelFormat::Rgba8,
        data: cropped,
        icc_profile: None,
    };
    lumen_image::encode_png_rgba8(&cropped_image)
        .map_err(|e| Error::Other(format!("crop_screenshot: PNG encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 solid-red PNG, cropped to the top-left 2x2, decodes back to a
    /// 2x2 image whose pixels are still all red.
    #[test]
    fn crop_screenshot_extracts_expected_region() {
        let image = lumen_image::Image {
            width: 4,
            height: 4,
            format: lumen_image::PixelFormat::Rgba8,
            data: [255, 0, 0, 255].repeat(16),
            icc_profile: None,
        };
        let png = lumen_image::encode_png_rgba8(&image).expect("encode");

        let cropped_png = crop_screenshot(&png, Rect::new(0.0, 0.0, 2.0, 2.0)).expect("crop");
        let cropped = lumen_image::decode(&cropped_png).expect("decode cropped");
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.to_rgba8(), [255, 0, 0, 255].repeat(4));
    }

    #[test]
    fn crop_screenshot_rejects_rect_fully_outside_image() {
        let image = lumen_image::Image {
            width: 4,
            height: 4,
            format: lumen_image::PixelFormat::Rgba8,
            data: [0, 0, 0, 255].repeat(16),
            icc_profile: None,
        };
        let png = lumen_image::encode_png_rgba8(&image).expect("encode");

        let err = crop_screenshot(&png, Rect::new(10.0, 10.0, 5.0, 5.0));
        assert!(err.is_err());
    }
}
