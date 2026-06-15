/// ICC color profile parsing and detection.
/// Supports sRGB, Display P3 (DCI-P3), and Rec2020 color spaces.
/// ICC profiles are binary structures defined by the International Color Consortium.
/// This module provides legacy wrapper around lumen_core::detect_color_space_from_icc.
use lumen_core::ColorSpace;

/// Legacy wrapper for ICC profile detection (deprecated, use lumen_core::detect_color_space_from_icc).
pub fn detect_color_space_from_icc(icc_data: &[u8]) -> ColorSpace {
    lumen_core::detect_color_space_from_icc(icc_data)
}

/// Apply tone mapping for a detected color space (Phase 1 placeholder).
///
/// Currently a pass-through; Phase 1 will implement pixel-level conversion.
pub fn apply_tone_mapping(_color_space: ColorSpace, _pixel_data: &mut [u8]) {
    // Phase 1: Implement pixel-by-pixel conversion
    // Will convert from source color space to sRGB for display
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_invalid_profile() {
        let short_data = vec![0u8; 100];
        assert_eq!(detect_color_space_from_icc(&short_data), ColorSpace::Srgb);
    }

    #[test]
    fn detects_srgb_profile() {
        // Minimal ICC profile with RGB color space signature
        let mut profile = vec![0u8; 128];
        // Color space signature at offset 16: 'RGB ' (0x52474220)
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;

        assert_eq!(detect_color_space_from_icc(&profile), ColorSpace::Srgb);
    }

    #[test]
    fn detects_p3_from_description() {
        let mut profile = vec![0u8; 200];
        // Color space signature: RGB
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;

        // Add "Display P3" text somewhere in profile
        let p3_text = b"Display P3";
        if profile.len() > 150 {
            for (i, &b) in p3_text.iter().enumerate() {
                if 150 + i < profile.len() {
                    profile[150 + i] = b;
                }
            }
        }

        assert_eq!(detect_color_space_from_icc(&profile), ColorSpace::DisplayP3);
    }

    #[test]
    fn detects_rec2020_from_description() {
        let mut profile = vec![0u8; 200];
        // Color space signature: RGB
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;

        // Add "Rec2020" text
        let rec_text = b"Rec2020";
        if profile.len() > 150 {
            for (i, &b) in rec_text.iter().enumerate() {
                if 150 + i < profile.len() {
                    profile[150 + i] = b;
                }
            }
        }

        assert_eq!(detect_color_space_from_icc(&profile), ColorSpace::Rec2020);
    }
}
