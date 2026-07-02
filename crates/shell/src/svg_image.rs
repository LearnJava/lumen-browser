//! RP-5: растеризация внешних SVG-картинок (`<img src=*.svg>`,
//! `background-image: url(*.svg)`).
//!
//! У SVG нет магической сигнатуры и растровых пикселей — вместо отдельного
//! декодера внешняя разметка прогоняется через тот же pipeline, что и
//! инлайновый `<svg>`: обёртка в минимальный HTML-документ →
//! `parse_and_layout` → `paint_ordered` → детерминированный CPU-растеризатор.
//! Результат — обычный [`lumen_image::Image`], который дальше живёт в
//! image-cache наравне с PNG/JPEG.

use std::sync::Arc;

use lumen_core::ext::{EventSink, NullHyphenationProvider};
use lumen_core::geom::Size;

/// Extracts intrinsic width and height from an SVG string (CSS Images §5.1):
/// `width`/`height` attributes of the root `<svg>` tag (plain numbers or
/// `NNpx`), else the `viewBox` width/height, else the CSS default 300×150.
/// Each dimension is rounded and clamped to `1..=4096`.
pub(crate) fn svg_intrinsic_size(source: &str) -> (u32, u32) {
    let bytes = source.as_bytes();

    // Step 1: find "<svg" (case-insensitive).
    let Some(svg_pos) = bytes.windows(4).position(|w| w.eq_ignore_ascii_case(b"<svg")) else {
        return (300, 150);
    };

    // Step 2: extract the tag text starting from "<svg" up to its '>'.
    let tag_end = bytes[svg_pos..]
        .iter()
        .position(|&b| b == b'>')
        .map(|i| svg_pos + i + 1)
        .unwrap_or(bytes.len());
    let tag = &source[svg_pos..tag_end];

    // Step 3: width/height attributes, then viewBox, then the CSS default.
    let width = attr_value(tag, "width")
        .and_then(parse_dimension)
        .or_else(|| {
            attr_value(tag, "viewBox").and_then(|vb| {
                let normalized = vb.replace(',', " ");
                let mut parts = normalized.split_whitespace();
                parts.nth(2).and_then(|w| w.parse::<f32>().ok())
            })
        })
        .unwrap_or(300.0);

    let height = attr_value(tag, "height")
        .and_then(parse_dimension)
        .or_else(|| {
            attr_value(tag, "viewBox").and_then(|vb| {
                let normalized = vb.replace(',', " ");
                let mut parts = normalized.split_whitespace();
                parts.nth(3).and_then(|h| h.parse::<f32>().ok())
            })
        })
        .unwrap_or(150.0);

    // Step 4: round and clamp.
    (clamp_round(width), clamp_round(height))
}

/// Extract the quoted value of attribute `name` from `tag`.
///
/// Matches only standalone attribute names: the previous byte must be ASCII
/// whitespace (so `width` never matches inside `stroke-width`), followed by
/// optional spaces, `=`, optional spaces and a `"`/`'` quote.
fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let nb = name.as_bytes();
    let mut i = 1;

    while i + nb.len() <= bytes.len() {
        if !bytes[i - 1].is_ascii_whitespace()
            || !bytes[i..i + nb.len()].eq_ignore_ascii_case(nb)
        {
            i += 1;
            continue;
        }

        let mut j = i + nb.len();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            i += 1;
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() {
            i += 1;
            continue;
        }
        let quote_char = bytes[j];
        if quote_char != b'"' && quote_char != b'\'' {
            i += 1;
            continue;
        }
        j += 1;

        let k = bytes[j..].iter().position(|&b| b == quote_char)? + j;
        return Some(&tag[j..k]);
    }

    None
}

/// Parse a dimension string to f32, stripping an optional `px` suffix.
/// Percentages and other units fail the parse and yield `None`.
fn parse_dimension(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    let without_px = trimmed.strip_suffix("px").unwrap_or(trimmed);
    without_px
        .parse::<f32>()
        .ok()
        .filter(|&v| v.is_finite() && v > 0.0)
}

/// Round f32 to u32 and clamp to `1..=4096`.
fn clamp_round(v: f32) -> u32 {
    (v.round() as u32).clamp(1, 4096)
}

/// Rasterizes SVG bytes into a raster image using the existing HTML rendering
/// pipeline.
///
/// Wraps the SVG in a minimal HTML document and runs the standard headless
/// layout/paint pipeline (deterministic, JS off) at the SVG's intrinsic size.
/// Relative references inside the SVG resolve against `base` (the image URL).
/// Returns `None` on any error (already logged).
pub(crate) fn rasterize_svg(
    bytes: &[u8],
    base: &crate::ResourceBase,
    sink: &Arc<dyn EventSink>,
) -> Option<lumen_image::Image> {
    let svg_str = String::from_utf8_lossy(bytes);
    let (w, h) = svg_intrinsic_size(&svg_str);

    let html = format!(
        "<!DOCTYPE html><html><head><style>html,body{{margin:0;padding:0;overflow:hidden}}</style></head><body>{svg_str}</body></html>"
    );

    let viewport = Size::new(w as f32, h as f32);
    let parsed = match crate::parse_and_layout(
        html.as_bytes(),
        Some("text/html"),
        base,
        sink,
        viewport,
        &mut std::collections::HashSet::new(),
        None,
        None,
        None,
        &NullHyphenationProvider,
        false, // cookie_banner_dismiss
        true,  // deterministic
        false, // dark_mode
        None,  // cookie_jar
        false, // cross_origin_isolated
        None,  // sw_worker_store
        None,  // cache_backend
        lumen_core::ColorSpace::Srgb,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Не растеризуется SVG: {e}");
            return None;
        }
    };

    let dl = crate::paint_ordered(&parsed.layout);
    match lumen_paint::Renderer::render_to_image_cpu(w, h, &dl, &parsed.images, 0.0, 0.0) {
        Ok(image) => Some(image),
        Err(e) => {
            eprintln!("Не растеризуется SVG: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::svg_intrinsic_size;

    #[test]
    fn width_height_attributes_win() {
        assert_eq!(svg_intrinsic_size(r#"<svg width="120" height="80"></svg>"#), (120, 80));
        assert_eq!(svg_intrinsic_size(r#"<svg width="24px" height="24px"/>"#), (24, 24));
    }

    #[test]
    fn viewbox_fallback() {
        assert_eq!(svg_intrinsic_size(r#"<svg viewBox="0 0 64 32"></svg>"#), (64, 32));
        assert_eq!(svg_intrinsic_size(r#"<svg viewBox="0,0,10,20"/>"#), (10, 20));
    }

    #[test]
    fn percent_width_falls_through_to_viewbox() {
        assert_eq!(
            svg_intrinsic_size(r#"<svg width="100%" height="100%" viewBox="0 0 48 16"/>"#),
            (48, 16)
        );
    }

    #[test]
    fn stroke_width_is_not_width() {
        assert_eq!(
            svg_intrinsic_size(r#"<svg stroke-width="4" viewBox="0 0 30 40"/>"#),
            (30, 40)
        );
    }

    #[test]
    fn css_default_when_nothing_given() {
        assert_eq!(svg_intrinsic_size("<svg></svg>"), (300, 150));
        assert_eq!(svg_intrinsic_size("no svg here"), (300, 150));
    }

    #[test]
    fn xml_prolog_before_svg_is_ignored() {
        assert_eq!(
            svg_intrinsic_size(r#"<?xml version="1.0"?><svg width="12" height="34"/>"#),
            (12, 34)
        );
    }
}
