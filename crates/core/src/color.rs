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
    /// CIE L*a*b* — перцептивное пространство, используемое как ICC PCS (D50).
    /// Не пространство пикселей дисплея: применяется в цепочке управления
    /// цветом ([`crate::pcs`]), не для прямого вывода canvas/изображений.
    Lab,
}

impl ColorSpace {
    /// Возвращает название пространства как строку (для CSS canvas.colorSpace).
    pub fn name(&self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::DisplayP3 => "display-p3",
            ColorSpace::Rec2020 => "rec2020",
            ColorSpace::Lab => "lab",
        }
    }
}

/// Определяет основное цветовое пространство ICC-профиля.
///
/// Парсит профиль настоящим ICC-парсером ([`crate::icc::IccProfile`]) и
/// классифицирует RGB-профили по реальным колорант-примариям (`rXYZ`/`gXYZ`/
/// `bXYZ`), а не по сниффингу строки описания. Возвращает `ColorSpace::Srgb`,
/// если профиль не разбирается или это не RGB-профиль.
pub fn detect_color_space_from_icc(icc_data: &[u8]) -> ColorSpace {
    match crate::icc::IccProfile::parse(icc_data) {
        Some(profile) => profile.color_space(),
        None => ColorSpace::Srgb,
    }
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
        assert_eq!(ColorSpace::Lab.name(), "lab");
    }

    #[test]
    fn color_space_display() {
        assert_eq!(ColorSpace::Srgb.to_string(), "srgb");
        assert_eq!(ColorSpace::DisplayP3.to_string(), "display-p3");
        assert_eq!(ColorSpace::Rec2020.to_string(), "rec2020");
        assert_eq!(ColorSpace::Lab.to_string(), "lab");
    }

    #[test]
    fn color_space_clone_and_eq() {
        let space = ColorSpace::DisplayP3;
        let cloned = space;
        assert_eq!(space, cloned);
    }

    #[test]
    fn detects_invalid_profile() {
        // Too short / not an ICC profile → graceful sRGB fallback.
        let short_data = vec![0u8; 100];
        assert_eq!(detect_color_space_from_icc(&short_data), ColorSpace::Srgb);
    }

    #[test]
    fn garbage_with_rgb_sig_but_no_acsp_falls_back() {
        // A buffer carrying the 'RGB ' colour-space signature but lacking the
        // mandatory 'acsp' marker is not a valid profile — must not be sniffed.
        let mut profile = vec![0u8; 200];
        profile[16..20].copy_from_slice(&[0x52, 0x47, 0x42, 0x20]); // 'RGB '
        assert_eq!(detect_color_space_from_icc(&profile), ColorSpace::Srgb);
    }

    // Note: classification of well-formed sRGB/Display-P3/Rec.2020 profiles by
    // their colorant primaries is covered by the parser tests in `crate::icc`.
}
