/// Цветовое пространство изображения и canvas.
/// Поддерживаемые пространства: sRGB (стандартное), Display P3 (расширенное), Rec2020 (HDR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    /// sRGB (стандартное 8-битное пространство для большинства экранов; ITU-R BT.709).
    #[default]
    Srgb,
    /// Display P3 (расширенное цветовое пространство для новых дисплеев; DCI-P3).
    DisplayP3,
    /// Rec.2020 (HDR пространство для высокодинамичного контента; ITU-R BT.2020).
    Rec2020,
}

impl ColorSpace {
    /// Возвращает название пространства как строку (для CSS canvas.colorSpace).
    pub fn name(&self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::DisplayP3 => "display-p3",
            ColorSpace::Rec2020 => "rec2020",
        }
    }
}

/// Парсит ICC профиль и определяет его основное цветовое пространство.
///
/// Возвращает `ColorSpace::Srgb` если обнаружение не удаётся или профиль поврежден.
pub fn detect_color_space_from_icc(icc_data: &[u8]) -> ColorSpace {
    if icc_data.len() < 128 {
        return ColorSpace::Srgb;
    }

    // ICC profile header: 128 bytes minimum
    // Byte offset 16-19: Color space signature (e.g., 'RGB ' or 'Lab ')
    let color_space_sig = read_be_u32(icc_data, 16);

    match color_space_sig {
        // 'RGB ' — RGB profile
        0x52474220 => {
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

fn read_be_u32(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn contains_p3_tag(icc_data: &[u8]) -> bool {
    if icc_data.len() < 132 {
        return false;
    }
    let profile_desc = String::from_utf8_lossy(icc_data);
    profile_desc.to_lowercase().contains("display p3")
        || profile_desc.to_lowercase().contains("dci-p3")
}

fn contains_rec2020_tag(icc_data: &[u8]) -> bool {
    let profile_desc = String::from_utf8_lossy(icc_data);
    profile_desc.to_lowercase().contains("rec2020")
        || profile_desc.to_lowercase().contains("rec. 2020")
}

impl core::fmt::Display for ColorSpace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_space_name() {
        assert_eq!(ColorSpace::Srgb.name(), "srgb");
        assert_eq!(ColorSpace::DisplayP3.name(), "display-p3");
        assert_eq!(ColorSpace::Rec2020.name(), "rec2020");
    }

    #[test]
    fn color_space_display() {
        assert_eq!(ColorSpace::Srgb.to_string(), "srgb");
        assert_eq!(ColorSpace::DisplayP3.to_string(), "display-p3");
        assert_eq!(ColorSpace::Rec2020.to_string(), "rec2020");
    }

    #[test]
    fn color_space_clone_and_eq() {
        let space = ColorSpace::DisplayP3;
        let cloned = space;
        assert_eq!(space, cloned);
    }

    #[test]
    fn detects_invalid_profile() {
        let short_data = vec![0u8; 100];
        assert_eq!(detect_color_space_from_icc(&short_data), ColorSpace::Srgb);
    }

    #[test]
    fn detects_srgb_profile() {
        let mut profile = vec![0u8; 128];
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;
        assert_eq!(detect_color_space_from_icc(&profile), ColorSpace::Srgb);
    }

    #[test]
    fn detects_p3_from_description() {
        let mut profile = vec![0u8; 200];
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;
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
        profile[16] = 0x52;
        profile[17] = 0x47;
        profile[18] = 0x42;
        profile[19] = 0x20;
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
