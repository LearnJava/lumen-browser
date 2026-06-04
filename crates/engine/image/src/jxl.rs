//! JPEG XL stub decoder.
//!
//! Phase 0: Detection and graceful error reporting.
//! No actual decoding is performed (requires jxl_oxide or libjxl dependency).

use core::fmt;

/// JPEG XL magic bytes (naked): FF 0A.
const JXL_NAKED_MAGIC: [u8; 2] = [0xFF, 0x0A];

/// JPEG XL ISOBMFF container signature (box type).
const JXL_ISOBMFF_BOX_TYPE: [u8; 4] = [b'j', b'x', b'l', b' '];

/// Error decoding a JPEG XL image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JxlError;

impl fmt::Display for JxlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JPEG XL decoding not supported (Phase 0)")
    }
}

impl std::error::Error for JxlError {}

/// Detects JPEG XL image format.
///
/// Returns true if the byte sequence matches:
/// - Naked format: `FF 0A` header
/// - ISOBMFF container: `ftyp` box with `jxl ` brand or compatible brands
#[must_use]
pub fn is_jxl(bytes: &[u8]) -> bool {
    if bytes.len() < 2 {
        return false;
    }

    // Check naked format (FF 0A)
    if bytes[..2] == JXL_NAKED_MAGIC {
        return true;
    }

    // Check ISOBMFF container: look for 'ftyp' box with 'jxl ' brand
    // ISOBMFF structure: 4 bytes size + 4 bytes type
    if bytes.len() < 12 {
        return false;
    }

    if &bytes[4..8] == b"ftyp" {
        // Check if major brand is 'jxl '
        if bytes.len() >= 12 && bytes[8..12] == JXL_ISOBMFF_BOX_TYPE {
            return true;
        }
        // Check compatible brands (offset 16 onwards, each 4 bytes)
        if bytes.len() >= 16 {
            let compatible_start = 16;
            for i in (compatible_start..bytes.len()).step_by(4) {
                if i + 4 <= bytes.len() && bytes[i..i + 4] == JXL_ISOBMFF_BOX_TYPE {
                    return true;
                }
            }
        }
    }

    false
}

/// Stub JPEG XL decoder (Phase 0).
///
/// Always returns `JxlError` — no actual decoding support yet.
pub fn decode_jxl(_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), JxlError> {
    Err(JxlError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_jxl_naked_format() {
        let jxl_naked = vec![0xFF, 0x0A, 0x00, 0x00];
        assert!(is_jxl(&jxl_naked));
    }

    #[test]
    fn test_is_jxl_naked_format_minimal() {
        let jxl_naked = vec![0xFF, 0x0A];
        assert!(is_jxl(&jxl_naked));
    }

    #[test]
    fn test_is_jxl_isobmff_major_brand() {
        // ftyp box with jxl major brand: size(4) + 'ftyp'(4) + 'jxl '(4) + ...
        let mut jxl_isobmff = vec![0x00, 0x00, 0x00, 0x14]; // box size = 20
        jxl_isobmff.extend_from_slice(b"ftyp");
        jxl_isobmff.extend_from_slice(b"jxl "); // major brand
        jxl_isobmff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        assert!(is_jxl(&jxl_isobmff));
    }

    #[test]
    fn test_is_jxl_isobmff_compatible_brand() {
        // ftyp box with compatible brand jxl
        let mut jxl_isobmff = vec![0x00, 0x00, 0x00, 0x18]; // box size = 24
        jxl_isobmff.extend_from_slice(b"ftyp");
        jxl_isobmff.extend_from_slice(b"mj2 "); // different major brand
        jxl_isobmff.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        jxl_isobmff.extend_from_slice(b"jxl "); // compatible brand
        assert!(is_jxl(&jxl_isobmff));
    }

    #[test]
    fn test_is_jxl_not_jxl() {
        let png_sig = vec![0x89, 0x50, 0x4E, 0x47]; // PNG signature
        assert!(!is_jxl(&png_sig));

        let empty = vec![];
        assert!(!is_jxl(&empty));

        let single_byte = vec![0xFF];
        assert!(!is_jxl(&single_byte));
    }

    #[test]
    fn test_decode_jxl_always_fails() {
        let jxl_data = vec![0xFF, 0x0A];
        assert!(decode_jxl(&jxl_data).is_err());
    }
}
