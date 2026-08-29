//! Типы значений CSS для цвета и цветовых пространств: `Color`/`ColorFloat`
//! (включая non-sRGB CSS Color L4 пространства), системные цвета
//! (`SystemColor`), разрешение `CssColor` в конкретный `Color`.
//!
//! Перенесено батчем SPLIT-ST16 из `crates/engine/layout/src/style.rs`
//! (анкер `struct Color` до конца `impl CssColor`) без правок тел.

use lumen_core::ColorSpace;

use crate::style::parse::color::{encode_srgb, system_color};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

/// CSS Color L4 §10 — цветовое пространство для wide-gamut значений.
/// Wide-gamut цвет с float-каналами [0..1 для in-gamut, за пределами — out-of-gamut].
/// Используется для `color(display-p3 …)`, `color(rec2020 …)`, `color(srgb …)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorFloat {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub space: ColorSpace,
}

impl ColorFloat {
    /// Конвертирует в sRGB u8, применяя матрицу цветового пространства и гамму.
    /// Out-of-gamut значения клипируются в [0, 255].
    pub fn to_srgb_color(self) -> Color {
        let (lr, lg, lb) = match self.space {
            // Lab is a PCS encoding, not an RGB `ColorFloat` channel space, so it
            // never reaches this RGB→sRGB path; decode as sRGB to stay panic-free.
            ColorSpace::Srgb | ColorSpace::Lab => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                (lr, lg, lb)
            }
            ColorSpace::DisplayP3 => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                p3_linear_to_srgb_linear(lr, lg, lb)
            }
            ColorSpace::Rec2020 => {
                let lr = rec2020_gamma_decode(self.r);
                let lg = rec2020_gamma_decode(self.g);
                let lb = rec2020_gamma_decode(self.b);
                rec2020_linear_to_srgb_linear(lr, lg, lb)
            }
        };
        Color {
            r: encode_srgb(lr),
            g: encode_srgb(lg),
            b: encode_srgb(lb),
            a: (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }

    /// Линейные sRGB-каналы [0..1] для прямой передачи в GPU без квантизации.
    pub fn to_linear_srgb(self) -> [f32; 4] {
        let (lr, lg, lb) = match self.space {
            // Lab is a PCS encoding, not an RGB `ColorFloat` channel space, so it
            // never reaches this RGB→sRGB path; decode as sRGB to stay panic-free.
            ColorSpace::Srgb | ColorSpace::Lab => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                (lr, lg, lb)
            }
            ColorSpace::DisplayP3 => {
                let lr = srgb_gamma_decode(self.r);
                let lg = srgb_gamma_decode(self.g);
                let lb = srgb_gamma_decode(self.b);
                p3_linear_to_srgb_linear(lr, lg, lb)
            }
            ColorSpace::Rec2020 => {
                let lr = rec2020_gamma_decode(self.r);
                let lg = rec2020_gamma_decode(self.g);
                let lb = rec2020_gamma_decode(self.b);
                rec2020_linear_to_srgb_linear(lr, lg, lb)
            }
        };
        [lr, lg, lb, self.a.clamp(0.0, 1.0)]
    }

    /// Конвертирует `ColorFloat` в линейные каналы заданного `target` цветового
    /// пространства.
    ///
    /// `target == self.space` → identity: только декодируется гамма и
    /// возвращаются линейные каналы исходного пространства.
    /// `target == Srgb` → существующий `to_linear_srgb()` (никак не регрессит).
    /// Остальные комбинации пока маппятся через linear sRGB (Step 2 baseline).
    pub fn to_display(self, target: crate::ColorSpace) -> [f32; 4] {
        if target == self.space {
            return [
                self.decode(self.r),
                self.decode(self.g),
                self.decode(self.b),
                self.a.clamp(0.0, 1.0),
            ];
        }
        if target == crate::ColorSpace::Srgb {
            return self.to_linear_srgb();
        }
        // Baseline: route through linear sRGB for all other combos.
        // Step 2 acceptance criteria only require identity-preserve and sRGB
        // regression; P3↔Rec2020 direct mapping is deferred.
        let [r, g, b, a] = self.to_linear_srgb();
        let cf = ColorFloat {
            r,
            g,
            b,
            a,
            space: crate::ColorSpace::Srgb,
        };
        cf.to_display(target)
    }

    fn decode(self, c: f32) -> f32 {
        match self.space {
            crate::ColorSpace::Srgb | crate::ColorSpace::DisplayP3 => srgb_gamma_decode(c),
            crate::ColorSpace::Rec2020 => rec2020_gamma_decode(c),
            crate::ColorSpace::Lab => c,
        }
    }
}

/// CSS Color L4 §17 — XYZ (D65) → linear sRGB (sRGB primary matrix, CIE 1931).
/// Constants match the D65→linear-sRGB block already used in `lab_to_srgb`.
fn xyz_d65_to_srgb_linear(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let lr = 3.240_625_5 * x - 1.537_208 * y - 0.498_628_6 * z;
    let lg = -0.968_930_7 * x + 1.875_756_1 * y + 0.041_517_5 * z;
    let lb = 0.055_710_1 * x - 0.204_021_1 * y + 1.056_995_9 * z;
    (lr, lg, lb)
}

/// CSS Color L4 §11 — Bradford D50 → D65 chromatic adaptation of XYZ.
/// Constants match the D50→D65 block already used in `lab_to_srgb`.
fn xyz_d50_to_d65(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let xn = 0.955_576_6 * x - 0.023_039_3 * y + 0.063_163_6 * z;
    let yn = -0.028_289_5 * x + 1.009_941_6 * y + 0.021_007_7 * z;
    let zn = 0.012_298_2 * x - 0.020_483_0 * y + 1.329_909_8 * z;
    (xn, yn, zn)
}

/// CSS Color L4 §10 — convert a non-displayable predefined `color()` space to
/// linear sRGB. `c1`/`c2`/`c3` are the raw channel values. Returns `None` for
/// an unknown space token (caller treats the whole `color()` as invalid).
///
/// Displayable spaces (`srgb`/`display-p3`/`rec2020`) are *not* handled here —
/// they are stored verbatim as `ColorFloat` to preserve linear precision for
/// GPU paint. The spaces below have no sRGB-displayable representation, so they
/// are gamut-mapped to sRGB at parse time.
pub(in crate::style) fn predefined_to_srgb_linear(space: &str, c1: f32, c2: f32, c3: f32) -> Option<(f32, f32, f32)> {
    Some(match space {
        // Linear-light sRGB primaries — channels are already linear sRGB.
        "srgb-linear" => (c1, c2, c3),
        // Adobe RGB (1998): gamma 563/256, then A98 linear → XYZ(D65) → sRGB.
        "a98-rgb" => {
            let dec = |c: f32| c.signum() * c.abs().powf(563.0 / 256.0);
            let (r, g, b) = (dec(c1), dec(c2), dec(c3));
            let x = 0.576_669 * r + 0.185_558 * g + 0.188_229 * b;
            let y = 0.297_345 * r + 0.627_364 * g + 0.075_291 * b;
            let z = 0.027_031 * r + 0.070_689 * g + 0.991_338 * b;
            xyz_d65_to_srgb_linear(x, y, z)
        }
        // ProPhoto RGB: gamma 1.8 (linear toe below 16·Et), linear → XYZ(D50)
        // → D65 → sRGB.
        "prophoto-rgb" => {
            let dec = |c: f32| {
                if c.abs() <= 16.0 / 512.0 {
                    c / 16.0
                } else {
                    c.signum() * c.abs().powf(1.8)
                }
            };
            let (r, g, b) = (dec(c1), dec(c2), dec(c3));
            let x = 0.797_761 * r + 0.135_186 * g + 0.031_349 * b;
            let y = 0.288_071 * r + 0.711_843 * g + 0.000_086 * b;
            let z = 0.825_105 * b;
            let (x65, y65, z65) = xyz_d50_to_d65(x, y, z);
            xyz_d65_to_srgb_linear(x65, y65, z65)
        }
        // CIE XYZ with a D65 white point (`xyz` is an alias for `xyz-d65`).
        "xyz" | "xyz-d65" => xyz_d65_to_srgb_linear(c1, c2, c3),
        // CIE XYZ with a D50 white point — adapt to D65 first.
        "xyz-d50" => {
            let (x65, y65, z65) = xyz_d50_to_d65(c1, c2, c3);
            xyz_d65_to_srgb_linear(x65, y65, z65)
        }
        _ => return None,
    })
}

/// Linear sRGB → gamma sRGB float in [0,1] (IEC 61966-2-1). Float twin of
/// [`encode_srgb`], used to store gamut-mapped wide-gamut colours back into a
/// `ColorFloat` with `space = Srgb`.
pub(in crate::style) fn encode_srgb_f32(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Display P3 linear → sRGB linear (ICC/CSS Color L4 §10.9 matrix).
fn p3_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.224_94 * r - 0.224_94 * g;
    let sg = -0.042_076 * r + 1.042_076 * g;
    let sb = -0.019_692 * r - 0.078_654 * g + 1.098_346 * b;
    (sr, sg, sb)
}

/// Rec2020 linear → sRGB linear (CSS Color L4 §10.9 matrix).
fn rec2020_linear_to_srgb_linear(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let sr =  1.660_491 * r - 0.587_641 * g - 0.072_85 * b;
    let sg = -0.124_551 * r + 1.132_9 * g - 0.008_35 * b;
    let sb = -0.018_151 * r - 0.100_578 * g + 1.118_73 * b;
    (sr, sg, sb)
}

/// Декодирование sRGB / Display P3 гаммы → линейный свет.
pub(in crate::style) fn srgb_gamma_decode(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Декодирование Rec2020 гаммы (BT.2020 OETF) → линейный свет.
pub(in crate::style) fn rec2020_gamma_decode(c: f32) -> f32 {
    const ALPHA: f32 = 1.099_296_8;
    const BETA: f32 = 0.018_053_97;
    if c < 4.5 * BETA {
        c / 4.5
    } else {
        ((c + (ALPHA - 1.0)) / ALPHA).powf(1.0 / 0.45)
    }
}

/// CSS Color Level 4 §6.2 — system color keywords. Stored as a `Copy` enum to
/// avoid heap allocation in `CssColor`. Resolved to a concrete RGB at cascade
/// used-value time via `system_color()`, not at parse time, so the element's
/// used color scheme (`light`/`dark`) is taken into account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemColor {
    /// `Canvas` / `Window`
    Canvas,
    /// `CanvasText` / `WindowText` / `FieldText`
    CanvasText,
    /// `Field` (input/textarea backgrounds)
    Field,
    /// `ButtonFace`
    ButtonFace,
    /// `ButtonText`
    ButtonText,
    /// `ButtonBorder` / `ThreeDFace`
    ButtonBorder,
    /// `LinkText`
    LinkText,
    /// `VisitedText`
    VisitedText,
    /// `ActiveText`
    ActiveText,
    /// `Highlight` / `SelectedItem`
    Highlight,
    /// `HighlightText` / `SelectedItemText`
    HighlightText,
    /// `GrayText` / `GreyText`
    GrayText,
    /// `Mark`
    Mark,
    /// `MarkText`
    MarkText,
    /// `AccentColor`
    AccentColor,
    /// `AccentColorText`
    AccentColorText,
    /// `ThreeDHighlight`
    ThreeDHighlight,
    /// `ThreeDShadow`
    ThreeDShadow,
    /// `ThreeDLightShadow`
    ThreeDLightShadow,
    /// `ThreeDDarkShadow`
    ThreeDDarkShadow,
    /// `Scrollbar`
    Scrollbar,
    /// `ScrollbarTrack`
    ScrollbarTrack,
    /// `ScrollbarThumb`
    ScrollbarThumb,
}

impl SystemColor {
    /// Parse a CSS system color keyword (case-insensitive). Returns `None` for
    /// non-system-color strings; aliases are normalised to their canonical variant.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "canvas" | "window" => Some(Self::Canvas),
            "canvastext" | "windowtext" | "fieldtext" => Some(Self::CanvasText),
            "field" => Some(Self::Field),
            "buttonface" => Some(Self::ButtonFace),
            "buttontext" => Some(Self::ButtonText),
            "buttonborder" | "threedface" => Some(Self::ButtonBorder),
            "linktext" => Some(Self::LinkText),
            "visitedtext" => Some(Self::VisitedText),
            "activetext" => Some(Self::ActiveText),
            "highlight" | "selecteditem" => Some(Self::Highlight),
            "highlighttext" | "selecteditemtext" => Some(Self::HighlightText),
            "graytext" | "greytext" => Some(Self::GrayText),
            "mark" => Some(Self::Mark),
            "marktext" => Some(Self::MarkText),
            "accentcolor" => Some(Self::AccentColor),
            "accentcolortext" => Some(Self::AccentColorText),
            "threedhighlight" => Some(Self::ThreeDHighlight),
            "threedshadow" => Some(Self::ThreeDShadow),
            "threedlightshadow" => Some(Self::ThreeDLightShadow),
            "threeddarkshadow" => Some(Self::ThreeDDarkShadow),
            "scrollbar" => Some(Self::Scrollbar),
            "scrollbartrack" => Some(Self::ScrollbarTrack),
            "scrollbarthumb" => Some(Self::ScrollbarThumb),
            _ => None,
        }
    }

    /// Returns the canonical lowercase CSS keyword name for this variant.
    fn css_name(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::CanvasText => "canvastext",
            Self::Field => "field",
            Self::ButtonFace => "buttonface",
            Self::ButtonText => "buttontext",
            Self::ButtonBorder => "buttonborder",
            Self::LinkText => "linktext",
            Self::VisitedText => "visitedtext",
            Self::ActiveText => "activetext",
            Self::Highlight => "highlight",
            Self::HighlightText => "highlighttext",
            Self::GrayText => "graytext",
            Self::Mark => "mark",
            Self::MarkText => "marktext",
            Self::AccentColor => "accentcolor",
            Self::AccentColorText => "accentcolortext",
            Self::ThreeDHighlight => "threedhighlight",
            Self::ThreeDShadow => "threedshadow",
            Self::ThreeDLightShadow => "threedlightshadow",
            Self::ThreeDDarkShadow => "threeddarkshadow",
            Self::Scrollbar => "scrollbar",
            Self::ScrollbarTrack => "scrollbartrack",
            Self::ScrollbarThumb => "scrollbarthumb",
        }
    }

    /// Resolve to a concrete sRGB `Color` for the given used color scheme.
    /// `dark` — result of `ColorScheme::used_dark(prefer_dark)` for this element.
    pub fn resolve_color(self, dark: bool) -> Color {
        system_color(self.css_name(), dark)
            .unwrap_or(Color { r: 0, g: 0, b: 0, a: 255 })
    }
}

/// CSS Color L4 §4.2 — типизированное цветовое значение каскада.
///
/// `Rgba` — разрешённый конкретный цвет; `CurrentColor` — keyword `currentcolor`,
/// который разрешается в вычисленное значение `color` элемента при рендеринге.
/// `Wide` — wide-gamut цвет из `color()` функции (Display P3, Rec2020, sRGB float).
/// `System` — CSS Color 4 §6.2 system color keyword; resolved to Rgba at cascade
/// used-value time by `resolve_system_colors` at the end of `compute_style`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CssColor {
    Rgba(Color),
    CurrentColor,
    Wide(ColorFloat),
    /// System color keyword (e.g. `Canvas`, `ButtonFace`). Resolved to `Rgba`
    /// at the end of `compute_style` via `resolve_system_colors_in_style`.
    System(SystemColor),
}

impl CssColor {
    /// Разрешает значение в sRGB u8 Color. `Wide` конвертируется через матрицу.
    /// `System` — fallback light-mode resolution (post-pass should have resolved).
    pub fn resolve(self, current_color: Color) -> Color {
        match self {
            CssColor::Rgba(c) => c,
            CssColor::CurrentColor => current_color,
            CssColor::Wide(f) => f.to_srgb_color(),
            CssColor::System(sc) => sc.resolve_color(false),
        }
    }

    /// Конвертирует в `Color`, минуя `current_color`. `CurrentColor` → `None`.
    /// Wide-gamut значения конвертируются через матрицу в sRGB u8.
    pub fn to_color_opt(self) -> Option<Color> {
        match self {
            CssColor::Rgba(c) => Some(c),
            CssColor::Wide(f) => Some(f.to_srgb_color()),
            CssColor::CurrentColor => None,
            CssColor::System(sc) => Some(sc.resolve_color(false)),
        }
    }

    /// Линейные sRGB-каналы для прямой передачи в GPU.
    pub fn resolve_linear(self, current_color: Color) -> [f32; 4] {
        match self {
            CssColor::Rgba(c) => [
                srgb_gamma_decode(c.r as f32 / 255.0),
                srgb_gamma_decode(c.g as f32 / 255.0),
                srgb_gamma_decode(c.b as f32 / 255.0),
                c.a as f32 / 255.0,
            ],
            CssColor::CurrentColor => {
                let c = current_color;
                [
                    srgb_gamma_decode(c.r as f32 / 255.0),
                    srgb_gamma_decode(c.g as f32 / 255.0),
                    srgb_gamma_decode(c.b as f32 / 255.0),
                    c.a as f32 / 255.0,
                ]
            }
            CssColor::Wide(f) => f.to_linear_srgb(),
            CssColor::System(sc) => {
                let c = sc.resolve_color(false);
                [
                    srgb_gamma_decode(c.r as f32 / 255.0),
                    srgb_gamma_decode(c.g as f32 / 255.0),
                    srgb_gamma_decode(c.b as f32 / 255.0),
                    c.a as f32 / 255.0,
                ]
            }
        }
    }
}

