/// ICC color profile parsing and detection.
/// Supports sRGB, Display P3 (DCI-P3), and Rec2020 color spaces.
/// ICC profiles are binary structures defined by the International Color Consortium.
/// This module extracts essential information (color space signature, render intent)
/// to determine which tone mapping to apply during image display.
use lumen_layout::style::ColorSpace;

/// Parses an ICC profile and detects its primary color space.
///
/// Returns `ColorSpace::Srgb` if detection fails or profile is malformed.
pub fn detect_color_space_from_icc(icc_data: &[u8]) -> ColorSpace {
    if icc_data.len() < 128 {
        return ColorSpace::Srgb; // Invalid profile
    }

    // ICC profile header: 128 bytes minimum
    // Byte offset 16-19: Color space signature (e.g., 'RGB ' or 'Lab ')
    // Byte offset 20-23: PCS (Profile Connection Space) signature

    // Read color space signature at offset 16 (little-endian 4-byte tag)
    let color_space_sig = read_be_u32(icc_data, 16);

    match color_space_sig {
        // 'RGB ' — RGB profile
        0x52474220 => {
            // Look for Display P3 or Rec2020 indicators
            if contains_p3_tag(icc_data) {
                ColorSpace::DisplayP3
            } else if contains_rec2020_tag(icc_data) {
                ColorSpace::Rec2020
            } else {
                ColorSpace::Srgb
            }
        }
        // 'Lab ' and others — out-of-scope for Phase 0
        _ => ColorSpace::Srgb,
    }
}

/// Reads a 4-byte big-endian value at offset.
fn read_be_u32(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

/// Checks if ICC profile contains Display P3 characteristic tags.
fn contains_p3_tag(icc_data: &[u8]) -> bool {
    // ICC profile structure: [128-byte header][tag table]
    // Tag table starts at byte 128
    if icc_data.len() < 132 {
        return false;
    }

    // Number of tags at offset 8 in tag table (offset 128+8)
    let _tag_count = read_be_u32(icc_data, 128 + 8) >> 24; // High byte of tag count

    // Search for Display P3 signatures or descriptive text tags
    // Phase 0: Look for common P3 signatures in profile
    let profile_desc = String::from_utf8_lossy(icc_data);
    profile_desc.to_lowercase().contains("display p3")
        || profile_desc.to_lowercase().contains("dci-p3")
}

/// Checks if ICC profile contains Rec2020 characteristic tags.
fn contains_rec2020_tag(icc_data: &[u8]) -> bool {
    let profile_desc = String::from_utf8_lossy(icc_data);
    profile_desc.to_lowercase().contains("rec2020")
        || profile_desc.to_lowercase().contains("rec. 2020")
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
