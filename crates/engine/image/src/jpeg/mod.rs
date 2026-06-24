use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::{Image, IccProfile, PixelFormat};

/// APP2 ICC_PROFILE identifier (11 ASCII chars + null terminator).
const ICC_PROFILE_IDENTIFIER: &[u8] = b"ICC_PROFILE\0";

/// Parses multi-segment JPEG APP2 ICC profile data (JFIF-ICC, ISO 15076-1).
///
/// Format per segment: FF E2 LL LL "ICC_PROFILE\0" SEQ TOTAL data...
/// Segments are collected by SEQ order and concatenated into one raw profile blob.
pub(crate) fn parse_jpeg_icc_profile(bytes: &[u8]) -> Option<IccProfile> {
    // Skip SOI (FF D8).
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut pos = 2;

    // Map from segment sequence number (1-based) to segment data slice range.
    let mut segments: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut expected_total: Option<u8> = None;

    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            break; // Corrupt stream.
        }
        let marker = bytes[pos + 1];

        // SOI / EOI have no length field.
        if marker == 0xD8 {
            pos += 2;
            continue;
        }
        if marker == 0xD9 {
            break;
        }

        // All other markers: FF XX LL LL data[LL-2]
        if pos + 4 > bytes.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if seg_len < 2 || pos + 2 + seg_len > bytes.len() {
            break;
        }
        let data = &bytes[pos + 4..pos + 2 + seg_len]; // seg_len includes 2-byte length field

        if marker == 0xE2 && data.len() > ICC_PROFILE_IDENTIFIER.len() + 2
            && data[..ICC_PROFILE_IDENTIFIER.len()] == *ICC_PROFILE_IDENTIFIER
        {
            let seq = data[ICC_PROFILE_IDENTIFIER.len()];
            let total = data[ICC_PROFILE_IDENTIFIER.len() + 1];
            let fragment = data[ICC_PROFILE_IDENTIFIER.len() + 2..].to_vec();

            if seq >= 1 && total >= 1 && usize::from(seq) <= usize::from(total) {
                match expected_total {
                    None => { expected_total = Some(total); }
                    Some(t) if t != total => { return None; } // Inconsistent headers.
                    _ => {}
                }
                segments.push((seq, fragment));
            }
        }

        pos += 2 + seg_len;
    }

    if segments.is_empty() {
        return None;
    }

    // Sort by sequence number and concatenate.
    segments.sort_by_key(|(seq, _)| *seq);
    let total = expected_total?;
    if segments.len() != usize::from(total) {
        return None; // Missing segments.
    }

    let mut profile_data: Vec<u8> = Vec::new();
    for (_, fragment) in segments {
        profile_data.extend_from_slice(&fragment);
    }

    if profile_data.len() >= 128 {
        Some(IccProfile { data: profile_data })
    } else {
        None
    }
}

pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, JpegError> {
    // Pass 1: read headers to determine input colorspace and dimensions.
    // ZCursor wraps &[u8] cheaply so a second decode from the start is fine.
    let mut probe = JpegDecoder::new(ZCursor::new(bytes));
    probe.decode_headers().map_err(|e| JpegError(e.to_string()))?;

    let input_cs = probe.input_colorspace().unwrap_or(ColorSpace::Unknown);
    let (width, height) = probe
        .dimensions()
        .ok_or_else(|| JpegError("no dimensions after decode_headers".into()))?;

    // CMYK / YCCK with an embedded CMYK ICC profile: colour-manage through the
    // profile's A2B0 LUT (ICC-4) instead of zune's naïve CMYK→RGB. Falls through
    // to the RGB path when the profile is absent or unusable.
    if matches!(input_cs, ColorSpace::CMYK | ColorSpace::YCCK)
        && let Some(img) = try_decode_cmyk_icc(bytes, input_cs, width, height)
    {
        return Ok(img);
    }

    // Grayscale JPEGs: force Luma output so we keep 1 byte/pixel.
    // zune-jpeg's default output is RGB (even for Y-only JPEGs).
    let is_gray = matches!(input_cs, ColorSpace::Luma | ColorSpace::LumaA);
    let options = if is_gray {
        DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma)
    } else {
        DecoderOptions::default()
    };

    // Pass 2: full decode with correct output colorspace.
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().map_err(|e| JpegError(e.to_string()))?;

    let format = if is_gray { PixelFormat::Gray8 } else { PixelFormat::Rgb8 };
    let icc_profile = parse_jpeg_icc_profile(bytes);

    Ok(Image { width: width as u32, height: height as u32, format, data: pixels, icc_profile })
}

/// Colour-manages a CMYK/YCCK JPEG through its embedded CMYK ICC profile (ICC-4).
///
/// Returns `Some(Image)` of gamma-encoded sRGB `Rgb8` pixels when the JPEG
/// carries a CMYK ICC profile whose `A2B0` table can be compiled into a
/// [`CmykTransform`]; otherwise `None`, leaving the caller to fall back to
/// zune's built-in CMYK→RGB conversion.
///
/// The four device channels are reconstructed from zune's raw 4-channel output:
/// YCCK is converted to stored CMY via JFIF YCbCr→RGB first. Adobe-marked files
/// (and all YCCK) store inverted samples, so ink coverage is `1 − sample`; the
/// resulting image carries no ICC profile because it is already sRGB.
fn try_decode_cmyk_icc(
    bytes: &[u8],
    input_cs: ColorSpace,
    width: usize,
    height: usize,
) -> Option<Image> {
    let icc = parse_jpeg_icc_profile(bytes)?;
    // ICC-5: build/compile the CMYK A2B0 transform via the process-wide cache so
    // repeated CMYK JPEGs sharing a profile parse it only once.
    let transform = lumen_core::icc::cached_cmyk_transform(&icc.data)?;

    // Decode raw 4-channel samples (input == output ⇒ zune copies them verbatim).
    let options = DecoderOptions::default().jpeg_set_out_colorspace(input_cs);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let raw = decoder.decode().ok()?;
    let pixel_count = width.checked_mul(height)?;
    if raw.len() < pixel_count * 4 {
        return None;
    }

    let is_ycck = matches!(input_cs, ColorSpace::YCCK);
    // Adobe APP14 (or any YCCK) means the stored CMYK samples are inverted.
    let inverted = is_ycck || jpeg_has_adobe_marker(bytes);
    let dev = |s: u8| -> f64 {
        let n = f64::from(s) / 255.0;
        if inverted { 1.0 - n } else { n }
    };

    let mut out = Vec::with_capacity(pixel_count * 3);
    for px in raw.chunks_exact(4).take(pixel_count) {
        let (cs, ms, ys, ks) = if is_ycck {
            // YCCK: YCbCr→RGB gives the inverted CMY samples Adobe stores.
            let (r, g, b) = ycbcr_to_rgb(px[0], px[1], px[2]);
            (255 - r, 255 - g, 255 - b, px[3])
        } else {
            (px[0], px[1], px[2], px[3])
        };
        let (r, g, b) = transform.apply(dev(cs), dev(ms), dev(ys), dev(ks));
        out.push((r * 255.0).round().clamp(0.0, 255.0) as u8);
        out.push((g * 255.0).round().clamp(0.0, 255.0) as u8);
        out.push((b * 255.0).round().clamp(0.0, 255.0) as u8);
    }

    Some(Image {
        width: width as u32,
        height: height as u32,
        format: PixelFormat::Rgb8,
        data: out,
        icc_profile: None,
    })
}

/// Scans for an `APP14` "Adobe" marker, which signals that a CMYK/YCCK JPEG
/// stores its samples inverted (`255 − value` = ink coverage).
fn jpeg_has_adobe_marker(bytes: &[u8]) -> bool {
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return false;
    }
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            break;
        }
        let marker = bytes[pos + 1];
        if marker == 0xD8 {
            pos += 2;
            continue;
        }
        // Start of scan / end of image — no more headers worth parsing.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let seg_len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        if seg_len < 2 || pos + 2 + seg_len > bytes.len() {
            break;
        }
        let data = &bytes[pos + 4..pos + 2 + seg_len];
        if marker == 0xEE && data.starts_with(b"Adobe") {
            return true;
        }
        pos += 2 + seg_len;
    }
    false
}

/// JFIF YCbCr → RGB for one pixel (BT.601 full-range), each output clamped to
/// `[0, 255]`. Used to recover stored CMY samples from YCCK.
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let y = f64::from(y);
    let cb = f64::from(cb) - 128.0;
    let cr = f64::from(cr) - 128.0;
    let r = y + 1.402 * cr;
    let g = y - 0.344_136 * cb - 0.714_136 * cr;
    let b = y + 1.772 * cb;
    (
        r.round().clamp(0.0, 255.0) as u8,
        g.round().clamp(0.0, 255.0) as u8,
        b.round().clamp(0.0, 255.0) as u8,
    )
}

/// Ошибка декодирования JPEG (обёртка над zune-jpeg).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegError(pub String);

impl core::fmt::Display for JpegError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JpegError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds minimal JPEG bytes containing APP2 ICC_PROFILE segments.
    ///
    /// `segments` is a list of (sequence_number, fragment_bytes) tuples (1-based).
    fn build_jpeg_with_icc(icc_fragments: &[(u8, &[u8])]) -> Vec<u8> {
        let total = icc_fragments.len() as u8;
        let mut out = vec![0xFFu8, 0xD8]; // SOI

        for (seq, fragment) in icc_fragments {
            // segment_data = identifier(12) + seq(1) + total(1) + fragment
            let seg_data_len = ICC_PROFILE_IDENTIFIER.len() + 2 + fragment.len();
            // APP2 length field includes the 2 length bytes but NOT FF E2.
            let length_field = (2 + seg_data_len) as u16;
            out.push(0xFF);
            out.push(0xE2); // APP2 marker
            out.extend_from_slice(&length_field.to_be_bytes());
            out.extend_from_slice(ICC_PROFILE_IDENTIFIER);
            out.push(*seq);
            out.push(total);
            out.extend_from_slice(fragment);
        }

        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        out
    }

    /// Produces a minimal 128-byte ICC profile with given description bytes embedded.
    fn make_icc_data(desc: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 256];
        data[16..20].copy_from_slice(b"RGB ");
        data[36..40].copy_from_slice(b"acsp");
        if 130 + desc.len() <= data.len() {
            data[130..130 + desc.len()].copy_from_slice(desc);
        }
        data
    }

    #[test]
    fn single_segment_icc_extracted() {
        let profile_data = make_icc_data(b"Display P3");
        let jpeg = build_jpeg_with_icc(&[(1, &profile_data)]);
        let parsed = parse_jpeg_icc_profile(&jpeg);
        assert!(parsed.is_some(), "Expected ICC profile to be extracted");
        let p = parsed.unwrap();
        assert_eq!(p.data, profile_data);
    }

    #[test]
    fn two_segment_icc_reassembled() {
        let full_data = make_icc_data(b"Rec. 2020");
        let mid = full_data.len() / 2;
        let (part1, part2) = full_data.split_at(mid);
        let jpeg = build_jpeg_with_icc(&[(1, part1), (2, part2)]);
        let parsed = parse_jpeg_icc_profile(&jpeg);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().data, full_data);
    }

    #[test]
    fn segments_out_of_order_reassembled() {
        let full_data = make_icc_data(b"Display P3");
        let mid = full_data.len() / 2;
        let (part1, part2) = full_data.split_at(mid);
        // Provide segments in reverse order.
        let jpeg = build_jpeg_with_icc(&[(2, part2), (1, part1)]);
        let parsed = parse_jpeg_icc_profile(&jpeg);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().data, full_data);
    }

    #[test]
    fn no_icc_segments_returns_none() {
        let jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xD9];
        assert!(parse_jpeg_icc_profile(&jpeg).is_none());
    }

    #[test]
    fn app2_without_icc_identifier_ignored() {
        // APP2 segment with different payload (not ICC_PROFILE\0).
        let mut jpeg = vec![0xFFu8, 0xD8];
        let payload = b"XMP_PROFILE\0some data here padding to make it long enough";
        let length_field = (2 + payload.len()) as u16;
        jpeg.push(0xFF);
        jpeg.push(0xE2);
        jpeg.extend_from_slice(&length_field.to_be_bytes());
        jpeg.extend_from_slice(payload);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        assert!(parse_jpeg_icc_profile(&jpeg).is_none());
    }

    #[test]
    fn missing_segment_returns_none() {
        let full_data = make_icc_data(b"Display P3");
        let mid = full_data.len() / 2;
        let (part1, _part2) = full_data.split_at(mid);
        // Only segment 1 of 2 — segment 2 is missing.
        let _jpeg = build_jpeg_with_icc(&[(1, part1)]);
        // total=1 is encoded by build_jpeg_with_icc; passing a single fragment
        // with seq=1 total=1 is complete — adjust to actually test missing:
        // Manually build with total=2 but only provide seq=1.
        let seg_data_len = ICC_PROFILE_IDENTIFIER.len() + 2 + part1.len();
        let length_field = (2 + seg_data_len) as u16;
        let mut jpeg2 = vec![0xFFu8, 0xD8];
        jpeg2.push(0xFF);
        jpeg2.push(0xE2);
        jpeg2.extend_from_slice(&length_field.to_be_bytes());
        jpeg2.extend_from_slice(ICC_PROFILE_IDENTIFIER);
        jpeg2.push(1u8); // seq=1
        jpeg2.push(2u8); // total=2 (but seg 2 never appears)
        jpeg2.extend_from_slice(part1);
        jpeg2.extend_from_slice(&[0xFF, 0xD9]);
        assert!(parse_jpeg_icc_profile(&jpeg2).is_none());
    }

    #[test]
    fn non_jpeg_bytes_returns_none() {
        let data = vec![0x89u8, 0x50, 0x4E, 0x47]; // PNG signature
        assert!(parse_jpeg_icc_profile(&data).is_none());
    }

    #[test]
    fn detects_adobe_app14_marker() {
        // SOI + APP14 "Adobe" + EOI.
        let mut jpeg = vec![0xFFu8, 0xD8];
        let payload = b"Adobe\0\x64\x00\x00\x00\x00"; // "Adobe" + version/flags/transform
        let length_field = (2 + payload.len()) as u16;
        jpeg.push(0xFF);
        jpeg.push(0xEE); // APP14
        jpeg.extend_from_slice(&length_field.to_be_bytes());
        jpeg.extend_from_slice(payload);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        assert!(jpeg_has_adobe_marker(&jpeg));
    }

    #[test]
    fn no_adobe_marker_when_absent() {
        let jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xD9];
        assert!(!jpeg_has_adobe_marker(&jpeg));
    }

    #[test]
    fn ycbcr_to_rgb_neutral_grey() {
        // Cb = Cr = 128 (neutral) → R = G = B = Y.
        let (r, g, b) = ycbcr_to_rgb(100, 128, 128);
        assert_eq!((r, g, b), (100, 100, 100));
    }

    #[test]
    fn ycbcr_to_rgb_primaries_clamp() {
        // White and black endpoints stay in range.
        assert_eq!(ycbcr_to_rgb(255, 128, 128), (255, 255, 255));
        assert_eq!(ycbcr_to_rgb(0, 128, 128), (0, 0, 0));
    }
}
