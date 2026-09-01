//! LIB-4: rasterizes external SVG images (`<img src=*.svg>`,
//! `background-image: url(*.svg)`) via `resvg`/`usvg`.
//!
//! Replaces the previous approach of wrapping the SVG in a minimal HTML
//! document and running it through the browser's own layout/paint pipeline
//! (`crates/shell/src/svg_image.rs`, RP-5). That path reused the own
//! SVG-to-DisplayList renderer (`lumen-layout`'s `box_tree/svg.rs` and
//! `lumen-paint`'s `svg_path.rs`), which draws zero `linearGradient`,
//! `radialGradient`, `clipPath`, `mask`, `filter`, `pattern` or `marker`
//! (ADR-027's own measurement) — a logo with a gradient came out flat.
//!
//! Nested `xlink:href` references that are `data:` URLs decode through
//! usvg's default resolver (unchanged); a relative or absolute URL naming
//! another file is not fetched — this crate has no network access by
//! design, matching every other decoder here (pure bytes-to-pixels). Such an
//! SVG still renders, just without that one embedded image.

use resvg::tiny_skia;
use resvg::usvg;

use crate::{Image, PixelFormat};

/// Error rasterizing an external SVG image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvgError {
    /// `usvg` failed to parse the document (malformed XML, no root `<svg>`, …).
    Parse(String),
    /// The parsed tree's intrinsic size rounds to zero pixels in some dimension.
    ZeroSize,
}

impl core::fmt::Display for SvgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SvgError::Parse(msg) => write!(f, "не парсится SVG: {msg}"),
            SvgError::ZeroSize => write!(f, "нулевой размер SVG"),
        }
    }
}

/// Rasterizes SVG `bytes` at their intrinsic size.
///
/// Intrinsic size follows CSS Images §5.1's fallback chain: root `<svg>`'s
/// `width`/`height` or `viewBox` if either is present (resolved by `usvg`,
/// not a hand-rolled attribute parser), else the CSS default object size
/// 300×150 — `usvg`'s own no-hint default is the SVG spec's 100×100 default
/// *viewport*, a different fallback for a different question, so it is
/// overridden rather than trusted here.
///
/// # Errors
/// [`SvgError::Parse`] if `usvg` rejects the document; [`SvgError::ZeroSize`]
/// if the resulting raster would have a zero-length dimension.
pub fn decode_svg(bytes: &[u8]) -> Result<Image, SvgError> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| SvgError::Parse(e.to_string()))?;

    let size = tree.size();
    let (width, height) = if has_root_sizing_hint(bytes) {
        (size.width(), size.height())
    } else {
        (300.0, 150.0)
    };
    let width = width.round().clamp(1.0, 4096.0) as u32;
    let height = height.round().clamp(1.0, 4096.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(SvgError::ZeroSize)?;
    let transform =
        tiny_skia::Transform::from_scale(width as f32 / size.width(), height as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    Ok(Image {
        width,
        height,
        format: PixelFormat::Rgba8,
        data: unpremultiply(pixmap.data()),
        icc_profile: None,
    })
}

/// Whether the root `<svg>` tag declares `width`, `height` or `viewBox` —
/// the hints CSS Images §5.1 resolves a size from. Scans only the root tag's
/// own text (up to its first `>`, within the first 4096 bytes), so an
/// unrelated descendant's `width` never counts.
///
/// Matching a standalone attribute name mirrors `is_svg`'s ASCII-case-
/// insensitive style: the byte before the name must be whitespace, so
/// `stroke-width` never matches `width`.
fn has_root_sizing_hint(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let Some(tag_start) = head.windows(4).position(|w| w.eq_ignore_ascii_case(b"<svg")) else {
        return false;
    };
    let Some(tag_end) = head[tag_start..].iter().position(|&b| b == b'>') else {
        return false;
    };
    let tag = &head[tag_start..tag_start + tag_end];
    [&b"width"[..], b"height", b"viewbox"]
        .iter()
        .any(|name| has_standalone_attr_name(tag, name))
}

/// Whether `name` occurs in `tag` as a standalone attribute name: preceded
/// by ASCII whitespace and followed by optional whitespace then `=`.
fn has_standalone_attr_name(tag: &[u8], name: &[u8]) -> bool {
    let mut i = 1;
    while i + name.len() <= tag.len() {
        if tag[i - 1].is_ascii_whitespace() && tag[i..i + name.len()].eq_ignore_ascii_case(name) {
            let mut j = i + name.len();
            while j < tag.len() && tag[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < tag.len() && tag[j] == b'=' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Un-premultiplies a `tiny-skia` premultiplied-RGBA8 buffer into the
/// straight alpha [`Image::data`] uses for [`PixelFormat::Rgba8`] — the
/// inverse of `lumen-paint::cpu_raster`'s `image_to_premultiplied`.
fn unpremultiply(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premultiplied.len());
    for chunk in premultiplied.chunks_exact(4) {
        let a = chunk[3];
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let unmul = |c: u8| -> u8 { (u16::from(c) * 255 / u16::from(a)).min(255) as u8 };
        out.extend_from_slice(&[unmul(chunk[0]), unmul(chunk[1]), unmul(chunk[2]), a]);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn plain_shape_decodes_to_intrinsic_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8">
            <rect width="16" height="8" fill="red"/>
        </svg>"#;
        let image = decode_svg(svg).unwrap();
        assert_eq!((image.width, image.height), (16, 8));
        assert_eq!(image.format, PixelFormat::Rgba8);
        // Opaque red fill: every pixel is (255, 0, 0, 255) once unpremultiplied.
        assert_eq!(image.data[0..4], [255, 0, 0, 255]);
    }

    #[test]
    fn viewbox_without_width_height_sets_intrinsic_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 32"></svg>"#;
        let image = decode_svg(svg).unwrap();
        assert_eq!((image.width, image.height), (64, 32));
    }

    #[test]
    fn css_default_size_when_nothing_given() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        let image = decode_svg(svg).unwrap();
        assert_eq!((image.width, image.height), (300, 150));
    }

    #[test]
    fn linear_gradient_renders_distinct_colours() {
        // The own SVG-to-DisplayList renderer draws zero gradients (ADR-027);
        // this is the regression guard that the resvg path actually paints one.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="1">
            <defs>
                <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0" stop-color="black"/>
                    <stop offset="1" stop-color="white"/>
                </linearGradient>
            </defs>
            <rect width="10" height="1" fill="url(#g)"/>
        </svg>"#;
        let image = decode_svg(svg).unwrap();
        let first_px = &image.data[0..4];
        let last_px = &image.data[image.data.len() - 4..];
        assert_ne!(first_px, last_px, "gradient endpoints must differ in colour");
    }

    #[test]
    fn malformed_svg_is_a_parse_error() {
        let err = decode_svg(b"<svg><unclosed").unwrap_err();
        assert!(matches!(err, SvgError::Parse(_)));
    }
}
