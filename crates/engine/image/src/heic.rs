//! HEIC/HEIF stub decoder.
//!
//! Phase 1: Detection and graceful error reporting.
//! No actual decoding is performed (requires libheif or similar dependency).

use core::fmt;

/// HEIC/HEIF brand identifiers in ISOBMFF ftyp box.
const HEIC_BRANDS: &[[u8; 4]] = &[
    *b"heic", // HEVC Image Container (high profile)
    *b"heix", // HEVC Image Container (extended range)
    *b"hevc", // HEVC image container (older brand)
    *b"mif1", // Multi-Image Format (HEIF)
];

/// Error decoding a HEIC/HEIF image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeicError;

impl fmt::Display for HeicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HEIC/HEIF decoding not supported (Phase 1)")
    }
}

impl std::error::Error for HeicError {}

/// Detects HEIC/HEIF image format.
///
/// Returns true if the byte sequence is an ISOBMFF container (`ftyp` box)
/// with major brand or compatible brand matching one of: `heic`, `heix`, `hevc`, `mif1`.
#[must_use]
pub fn is_heic(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }

    if &bytes[4..8] != b"ftyp" {
        return false;
    }

    // Check major brand (bytes 8-11)
    if is_heic_brand(&bytes[8..12]) {
        return true;
    }

    // Check compatible brands (starting at byte 16, each 4 bytes)
    if bytes.len() >= 16 {
        for i in (16..bytes.len()).step_by(4) {
            if i + 4 <= bytes.len() && is_heic_brand(&bytes[i..i + 4]) {
                return true;
            }
        }
    }

    false
}

fn is_heic_brand(brand: &[u8]) -> bool {
    HEIC_BRANDS.iter().any(|b| b == brand)
}

/// Stub HEIC/HEIF decoder (Phase 1).
///
/// Always returns `HeicError` — no actual decoding support yet.
pub fn decode_heic(_bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), HeicError> {
    Err(HeicError)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ftyp(major: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![0x00, 0x00, 0x00, 0x10]; // box size = 16
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        v
    }

    fn make_ftyp_with_compat(major: &[u8; 4], compat: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![0x00, 0x00, 0x00, 0x14]; // box size = 20
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // minor version
        v.extend_from_slice(compat);
        v
    }

    #[test]
    fn test_is_heic_major_brand_heic() {
        assert!(is_heic(&make_ftyp(b"heic")));
    }

    #[test]
    fn test_is_heic_major_brand_heix() {
        assert!(is_heic(&make_ftyp(b"heix")));
    }

    #[test]
    fn test_is_heic_major_brand_hevc() {
        assert!(is_heic(&make_ftyp(b"hevc")));
    }

    #[test]
    fn test_is_heic_major_brand_mif1() {
        assert!(is_heic(&make_ftyp(b"mif1")));
    }

    #[test]
    fn test_is_heic_compatible_brand() {
        // major brand = "misc", compatible = "heic"
        assert!(is_heic(&make_ftyp_with_compat(b"misc", b"heic")));
    }

    #[test]
    fn test_is_heic_not_heic() {
        let png_sig = vec![0x89, 0x50, 0x4E, 0x47];
        assert!(!is_heic(&png_sig));

        let empty: &[u8] = &[];
        assert!(!is_heic(empty));

        // AVIF file (different brand) — should NOT match
        let avif = make_ftyp(b"avif");
        assert!(!is_heic(&avif));
    }

    #[test]
    fn test_decode_heic_always_fails() {
        let heic_data = make_ftyp(b"heic");
        assert!(decode_heic(&heic_data).is_err());
    }
}
