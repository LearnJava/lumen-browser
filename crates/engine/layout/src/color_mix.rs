//! CSS `color-mix()` algorithm — CSS Color L5 §10.2.
//!
//! Pure computation module: takes two sRGB colors and a mixing specification,
//! returns the mixed color in sRGB. P4 wires this to CSS `color-mix()` parsing.
//!
//! Entry point: [`mix_colors`].
//!
//! # Supported interpolation spaces
//!
//! All spaces from the CSS Color Level 4/5 spec that are commonly used in
//! `color-mix()`:
//! - `srgb` — sRGB gamma-encoded
//! - `srgb-linear` — linearized sRGB (no gamma)
//! - `hsl` — Hue/Saturation/Lightness (polar)
//! - `hwb` — Hue/Whiteness/Blackness (polar)
//! - `lab` — CIE L*a*b* (D50)
//! - `lch` — CIE L*C*h° (D50, polar)
//! - `oklab` — Oklab perceptual space
//! - `oklch` — Oklch perceptual polar space
//! - `xyz-d65` — CIE XYZ (D65 white point)
//! - `xyz-d50` — CIE XYZ (D50 white point)
//!
//! # P4 handoff
//!
//! Parse `color-mix(in <space>, <color1> [<pct>]?, <color2> [<pct>]?)` in
//! `apply_declaration()` (style.rs) and call [`mix_colors`]. Example:
//! ```ignore
//! // color-mix(in oklch, red 40%, blue)
//! let c1 = parse_color("red")?.to_f32();
//! let c2 = parse_color("blue")?.to_f32();
//! let result = mix_colors(MixColorSpace::Oklch, c1, 0.4, c2, 0.6);
//! ```
//!
//! See `STATUS-P4.md` "Needs wiring" for the full wiring checklist.

/// CSS Color L5 §10.2 — interpolation color space for `color-mix()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixColorSpace {
    /// `srgb` — gamma-encoded sRGB (default per CSS Color L5).
    Srgb,
    /// `srgb-linear` — linearized sRGB, no gamma encoding.
    SrgbLinear,
    /// `hsl` — Hue/Saturation/Lightness. Hue is a polar axis.
    Hsl,
    /// `hwb` — Hue/Whiteness/Blackness. Hue is a polar axis.
    Hwb,
    /// `lab` — CIE L*a*b* (D50 white point).
    Lab,
    /// `lch` — CIE L*C*h° (D50 white point). Hue is a polar axis.
    Lch,
    /// `oklab` — Oklab perceptual uniform space.
    Oklab,
    /// `oklch` — Oklch perceptual polar space. Hue is a polar axis.
    Oklch,
    /// `xyz-d65` — CIE XYZ with D65 white point.
    XyzD65,
    /// `xyz-d50` — CIE XYZ with D50 white point (same as `xyz` in CSS Color L4).
    XyzD50,
}

impl MixColorSpace {
    /// Parse a CSS `color-mix()` interpolation space identifier (case-insensitive).
    pub fn from_css(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "srgb" => Some(Self::Srgb),
            "srgb-linear" => Some(Self::SrgbLinear),
            "hsl" => Some(Self::Hsl),
            "hwb" => Some(Self::Hwb),
            "lab" => Some(Self::Lab),
            "lch" => Some(Self::Lch),
            "oklab" => Some(Self::Oklab),
            "oklch" => Some(Self::Oklch),
            "xyz" | "xyz-d65" => Some(Self::XyzD65),
            "xyz-d50" => Some(Self::XyzD50),
            _ => None,
        }
    }

    /// Returns `true` if this space has a hue (polar) axis.
    pub fn is_polar(self) -> bool {
        matches!(self, Self::Hsl | Self::Hwb | Self::Lch | Self::Oklch)
    }
}

/// CSS Color L5 §10.2 — mix two sRGB colors in the given interpolation space.
///
/// Both `c1` and `c2` are sRGB [r, g, b, a] with each component in `[0.0, 1.0]`.
/// `w1` and `w2` are the percentage weights for `c1` and `c2` respectively,
/// each in `[0.0, 1.0]`. They should sum to 1.0 after normalization, but the
/// function normalizes them if they don't.
///
/// Returns the mixed color in sRGB `[r, g, b, a]`.
///
/// # CSS spec reference
/// CSS Color Level 5 §10.2 "Mixing Colors: the color-mix() Function"
pub fn mix_colors(
    space: MixColorSpace,
    c1: [f32; 4],
    w1: f32,
    c2: [f32; 4],
    w2: f32,
) -> [f32; 4] {
    // Normalize weights. If both are 0, return transparent.
    let total = w1 + w2;
    if total <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let p1 = w1 / total;
    let p2 = w2 / total;

    // Alpha is always interpolated in sRGB regardless of space (CSS Color L5 §10.2).
    let a1 = c1[3];
    let a2 = c2[3];
    let alpha = p1 * a1 + p2 * a2;

    // Premultiply alpha before converting to interpolation space.
    let pre1 = premultiply(c1);
    let pre2 = premultiply(c2);

    // Convert to interpolation space, mix, convert back.
    let mixed = match space {
        MixColorSpace::Srgb => lerp4(pre1, pre2, p2),

        MixColorSpace::SrgbLinear => {
            let lin1 = srgb_to_linear_arr(pre1);
            let lin2 = srgb_to_linear_arr(pre2);
            let mixed = lerp4(lin1, lin2, p2);
            linear_to_srgb_arr(mixed)
        }

        MixColorSpace::Hsl => {
            let h1 = srgb_to_hsl(pre1);
            let h2 = srgb_to_hsl(pre2);
            let mixed = mix_polar(h1, h2, p2);
            hsl_to_srgb(mixed)
        }

        MixColorSpace::Hwb => {
            let h1 = srgb_to_hwb(pre1);
            let h2 = srgb_to_hwb(pre2);
            let mixed = mix_polar(h1, h2, p2);
            hwb_to_srgb(mixed)
        }

        MixColorSpace::Lab => {
            let l1 = srgb_to_lab(pre1);
            let l2 = srgb_to_lab(pre2);
            let mixed = lerp4(l1, l2, p2);
            lab_to_srgb(mixed)
        }

        MixColorSpace::Lch => {
            let l1 = srgb_to_lch(pre1);
            let l2 = srgb_to_lch(pre2);
            let mixed = mix_polar(l1, l2, p2);
            lch_to_srgb(mixed)
        }

        MixColorSpace::Oklab => {
            let l1 = srgb_to_oklab(pre1);
            let l2 = srgb_to_oklab(pre2);
            let mixed = lerp4(l1, l2, p2);
            oklab_to_srgb(mixed)
        }

        MixColorSpace::Oklch => {
            let l1 = srgb_to_oklch(pre1);
            let l2 = srgb_to_oklch(pre2);
            let mixed = mix_polar(l1, l2, p2);
            oklch_to_srgb(mixed)
        }

        MixColorSpace::XyzD65 => {
            let x1 = srgb_to_xyz_d65(pre1);
            let x2 = srgb_to_xyz_d65(pre2);
            let mixed = lerp4(x1, x2, p2);
            xyz_d65_to_srgb(mixed)
        }

        MixColorSpace::XyzD50 => {
            let x1 = srgb_to_xyz_d50(pre1);
            let x2 = srgb_to_xyz_d50(pre2);
            let mixed = lerp4(x1, x2, p2);
            xyz_d50_to_srgb(mixed)
        }
    };

    // Un-premultiply alpha and set the interpolated alpha.
    let result = unpremultiply(mixed, alpha);
    clamp_srgb(result)
}

// ─── Hue interpolation (shortest path per CSS Color L5 §12.4) ────────────────

/// For polar spaces: interpolate [L_or_H_idx=0, x, y, a] where component 0
/// is the hue (in degrees). Uses "shorter" hue interpolation method (CSS default).
fn mix_polar(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    let h1 = from[0];
    let h2 = to[0];

    // Shortest arc on the hue circle.
    let delta = normalize_hue(h2 - h1);
    let hue = h1 + t * delta;

    [
        hue,
        lerp(from[1], to[1], t),
        lerp(from[2], to[2], t),
        lerp(from[3], to[3], t),
    ]
}

/// Normalize hue delta to [-180, 180] for shortest-arc interpolation.
fn normalize_hue(mut delta: f32) -> f32 {
    delta %= 360.0;
    if delta > 180.0 {
        delta -= 360.0;
    } else if delta < -180.0 {
        delta += 360.0;
    }
    delta
}

// ─── Alpha pre/un-multiplication ─────────────────────────────────────────────

fn premultiply(c: [f32; 4]) -> [f32; 4] {
    let a = c[3];
    [c[0] * a, c[1] * a, c[2] * a, a]
}

fn unpremultiply(c: [f32; 4], alpha: f32) -> [f32; 4] {
    if alpha < f32::EPSILON {
        return [0.0, 0.0, 0.0, 0.0];
    }
    [c[0] / alpha, c[1] / alpha, c[2] / alpha, alpha]
}

// ─── Basic helpers ────────────────────────────────────────────────────────────

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
        lerp(a[3], b[3], t),
    ]
}

fn clamp_srgb(c: [f32; 4]) -> [f32; 4] {
    [
        c[0].clamp(0.0, 1.0),
        c[1].clamp(0.0, 1.0),
        c[2].clamp(0.0, 1.0),
        c[3].clamp(0.0, 1.0),
    ]
}

// ─── sRGB ↔ linear sRGB ──────────────────────────────────────────────────────

/// CSS Color L4 §10.1 — sRGB transfer function (gamma decode).
fn gamma_decode(v: f32) -> f32 {
    if v.abs() <= 0.04045 {
        v / 12.92
    } else {
        ((v.abs() + 0.055) / 1.055).powf(2.4).copysign(v)
    }
}

/// CSS Color L4 §10.1 — sRGB inverse transfer function (gamma encode).
fn gamma_encode(v: f32) -> f32 {
    if v.abs() <= 0.003_130_8 {
        12.92 * v
    } else {
        (1.055 * v.abs().powf(1.0 / 2.4) - 0.055).copysign(v)
    }
}

fn srgb_to_linear_arr(c: [f32; 4]) -> [f32; 4] {
    [gamma_decode(c[0]), gamma_decode(c[1]), gamma_decode(c[2]), c[3]]
}

fn linear_to_srgb_arr(c: [f32; 4]) -> [f32; 4] {
    [gamma_encode(c[0]), gamma_encode(c[1]), gamma_encode(c[2]), c[3]]
}

// ─── sRGB ↔ HSL (CSS Color L4 §3.3) ─────────────────────────────────────────

/// Returns [hue_deg, saturation (0..1), lightness (0..1), alpha].
fn srgb_to_hsl(c: [f32; 4]) -> [f32; 4] {
    let r = c[0];
    let g = c[1];
    let b = c[2];
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let l = (max + min) / 2.0;

    let s = if delta < f32::EPSILON {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs())
    };

    let h = if delta < f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    [h, s, l, c[3]]
}

/// Returns sRGB from [hue_deg, saturation, lightness, alpha].
fn hsl_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let h = c[0].rem_euclid(360.0);
    let s = c[1].clamp(0.0, 1.0);
    let l = c[2].clamp(0.0, 1.0);

    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h1 = h / 60.0;
    let x = chroma * (1.0 - (h1.rem_euclid(2.0) - 1.0).abs());

    let (r1, g1, b1) = match h1 as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };

    let m = l - chroma / 2.0;
    [r1 + m, g1 + m, b1 + m, c[3]]
}

// ─── sRGB ↔ HWB (CSS Color L4 §3.4) ─────────────────────────────────────────

/// Returns [hue_deg, whiteness (0..1), blackness (0..1), alpha].
fn srgb_to_hwb(c: [f32; 4]) -> [f32; 4] {
    let hsl = srgb_to_hsl(c);
    let w = c[0].min(c[1]).min(c[2]);
    let bk = 1.0 - c[0].max(c[1]).max(c[2]);
    [hsl[0], w, bk, c[3]]
}

/// Returns sRGB from [hue_deg, whiteness, blackness, alpha].
fn hwb_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let h = c[0];
    let mut w = c[1];
    let mut bk = c[2];
    // Normalize whiteness + blackness if they exceed 1.
    let sum = w + bk;
    if sum > 1.0 {
        w /= sum;
        bk /= sum;
    }
    let t = w + bk;
    // Convert via HSL with saturation=1, lightness derived from w+bk.
    let (r, g, b, _) = {
        let v = hsl_to_srgb([h, 1.0, 0.5, 1.0]);
        (v[0], v[1], v[2], v[3])
    };
    [
        r * (1.0 - t) + w,
        g * (1.0 - t) + w,
        b * (1.0 - t) + w,
        c[3],
    ]
}

// ─── sRGB ↔ CIE XYZ D65 ──────────────────────────────────────────────────────

/// CSS Color L4 §10.9 — sRGB linear to XYZ-D65.
/// Matrix from IEC 61966-2-1:2003.
fn linear_srgb_to_xyz_d65(lr: f32, lg: f32, lb: f32) -> (f32, f32, f32) {
    let x = 0.412_391 * lr + 0.357_584 * lg + 0.180_481 * lb;
    let y = 0.212_639 * lr + 0.715_169 * lg + 0.072_192 * lb;
    let z = 0.019_331 * lr + 0.119_195 * lg + 0.950_532 * lb;
    (x, y, z)
}

/// CSS Color L4 §10.9 — XYZ-D65 to linear sRGB.
fn xyz_d65_to_linear_srgb(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let r =  3.240_97 * x - 1.537_383 * y - 0.498_611 * z;
    let g = -0.969_244 * x + 1.875_968 * y + 0.041_555 * z;
    let b =  0.055_630 * x - 0.203_977 * y + 1.056_972 * z;
    (r, g, b)
}

fn srgb_to_xyz_d65(c: [f32; 4]) -> [f32; 4] {
    let (lr, lg, lb) = (gamma_decode(c[0]), gamma_decode(c[1]), gamma_decode(c[2]));
    let (x, y, z) = linear_srgb_to_xyz_d65(lr, lg, lb);
    [x, y, z, c[3]]
}

fn xyz_d65_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let (r, g, b) = xyz_d65_to_linear_srgb(c[0], c[1], c[2]);
    [gamma_encode(r), gamma_encode(g), gamma_encode(b), c[3]]
}

// ─── sRGB ↔ CIE XYZ D50 ──────────────────────────────────────────────────────

/// Bradford chromatic adaptation D65 → D50.
fn xyz_d65_to_d50(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let xd =  1.047_811 * x + 0.022_887 * y - 0.050_127 * z;
    let yd =  0.029_542 * x + 0.990_484 * y - 0.017_049 * z;
    let zd = -0.009_234 * x + 0.015_044 * y + 0.752_132 * z;
    (xd, yd, zd)
}

/// Bradford chromatic adaptation D50 → D65.
fn xyz_d50_to_d65(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let xd =  0.955_473 * x - 0.023_099 * y + 0.063_166 * z;
    let yd = -0.028_370 * x + 1.009_996 * y + 0.021_041 * z;
    let zd =  0.012_314 * x - 0.020_508 * y + 1.330_07 * z;
    (xd, yd, zd)
}

fn srgb_to_xyz_d50(c: [f32; 4]) -> [f32; 4] {
    let (lr, lg, lb) = (gamma_decode(c[0]), gamma_decode(c[1]), gamma_decode(c[2]));
    let (x65, y65, z65) = linear_srgb_to_xyz_d65(lr, lg, lb);
    let (x, y, z) = xyz_d65_to_d50(x65, y65, z65);
    [x, y, z, c[3]]
}

fn xyz_d50_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let (x65, y65, z65) = xyz_d50_to_d65(c[0], c[1], c[2]);
    let (r, g, b) = xyz_d65_to_linear_srgb(x65, y65, z65);
    [gamma_encode(r), gamma_encode(g), gamma_encode(b), c[3]]
}

// ─── sRGB ↔ CIE Lab D50 ──────────────────────────────────────────────────────

/// Lab f function (CSS Color L4 §10.7).
fn lab_f(t: f32) -> f32 {
    // (6/29)^3 = 0.008856
    const DELTA: f32 = 6.0 / 29.0;
    const DELTA3: f32 = DELTA * DELTA * DELTA; // ≈ 0.008856
    if t > DELTA3 {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

/// Lab f^-1 function.
fn lab_f_inv(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

// D50 white point (normalized): Xn = 0.96422, Yn = 1.0, Zn = 0.82521.
const XN_D50: f32 = 0.964_22;
const YN_D50: f32 = 1.0;
const ZN_D50: f32 = 0.825_21;

fn xyz_d50_to_lab(x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let fx = lab_f(x / XN_D50);
    let fy = lab_f(y / YN_D50);
    let fz = lab_f(z / ZN_D50);
    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);
    (l, a, b)
}

fn lab_to_xyz_d50(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    (
        XN_D50 * lab_f_inv(fx),
        YN_D50 * lab_f_inv(fy),
        ZN_D50 * lab_f_inv(fz),
    )
}

fn srgb_to_lab(c: [f32; 4]) -> [f32; 4] {
    let (lr, lg, lb) = (gamma_decode(c[0]), gamma_decode(c[1]), gamma_decode(c[2]));
    let (x65, y65, z65) = linear_srgb_to_xyz_d65(lr, lg, lb);
    let (x50, y50, z50) = xyz_d65_to_d50(x65, y65, z65);
    let (l, a, b) = xyz_d50_to_lab(x50, y50, z50);
    [l, a, b, c[3]]
}

fn lab_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let (x50, y50, z50) = lab_to_xyz_d50(c[0], c[1], c[2]);
    let (x65, y65, z65) = xyz_d50_to_d65(x50, y50, z50);
    let (r, g, b) = xyz_d65_to_linear_srgb(x65, y65, z65);
    [gamma_encode(r), gamma_encode(g), gamma_encode(b), c[3]]
}

// ─── sRGB ↔ LCH (D50) ────────────────────────────────────────────────────────

/// Returns [L, C, h_deg, alpha].
fn srgb_to_lch(c: [f32; 4]) -> [f32; 4] {
    let lab = srgb_to_lab(c);
    let l = lab[0];
    let a = lab[1];
    let b = lab[2];
    let cap_c = (a * a + b * b).sqrt();
    let h = b.atan2(a).to_degrees().rem_euclid(360.0);
    [l, cap_c, h, lab[3]]
}

/// Takes [L, C, h_deg, alpha] and returns sRGB.
fn lch_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let l = c[0];
    let cap_c = c[1];
    let h = c[2].to_radians();
    let a = cap_c * h.cos();
    let b = cap_c * h.sin();
    lab_to_srgb([l, a, b, c[3]])
}

// ─── sRGB ↔ Oklab (CSS Color L4 §10.12) ──────────────────────────────────────

/// M1 matrix: linear sRGB → LMS (for Oklab).
fn linear_srgb_to_lms(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = 0.412_221 * r + 0.536_333 * g + 0.051_446 * b;
    let m = 0.211_904 * r + 0.680_700 * g + 0.107_397 * b;
    let s = 0.088_302 * r + 0.281_719 * g + 0.629_979 * b;
    (l, m, s)
}

/// M1 inverse: LMS → linear sRGB.
fn lms_to_linear_srgb(l: f32, m: f32, s: f32) -> (f32, f32, f32) {
    let r =  4.076_742 * l - 3.307_712 * m + 0.230_970 * s;
    let g = -1.268_438 * l + 2.609_757 * m - 0.341_320 * s;
    let b = -0.004_196 * l - 0.703_419 * m + 1.707_615 * s;
    (r, g, b)
}

/// M2 matrix: LMS^(1/3) → Oklab.
fn lms_cbrt_to_oklab(l: f32, m: f32, s: f32) -> (f32, f32, f32) {
    let ok_l = 0.210_454 * l + 0.793_618 * m - 0.004_072 * s;
    let ok_a = 1.977_999 * l - 2.428_592 * m + 0.450_594 * s;
    let ok_b = 0.025_904 * l + 0.782_772 * m - 0.808_676 * s;
    (ok_l, ok_a, ok_b)
}

/// M2 inverse: Oklab → LMS^(1/3).
fn oklab_to_lms_cbrt(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let lc = l + 0.396_338 * a + 0.215_804 * b;
    let mc = l - 0.105_561 * a - 0.063_854 * b;
    let sc = l - 0.089_484 * a - 1.291_486 * b;
    (lc, mc, sc)
}

fn srgb_to_oklab(c: [f32; 4]) -> [f32; 4] {
    let lr = gamma_decode(c[0]);
    let lg = gamma_decode(c[1]);
    let lb = gamma_decode(c[2]);
    let (l, m, s) = linear_srgb_to_lms(lr, lg, lb);
    let (lc, mc, sc) = (l.cbrt(), m.cbrt(), s.cbrt());
    let (ok_l, ok_a, ok_b) = lms_cbrt_to_oklab(lc, mc, sc);
    [ok_l, ok_a, ok_b, c[3]]
}

fn oklab_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let (lc, mc, sc) = oklab_to_lms_cbrt(c[0], c[1], c[2]);
    let (l, m, s) = (lc * lc * lc, mc * mc * mc, sc * sc * sc);
    let (r, g, b) = lms_to_linear_srgb(l, m, s);
    [gamma_encode(r), gamma_encode(g), gamma_encode(b), c[3]]
}

// ─── sRGB ↔ Oklch ─────────────────────────────────────────────────────────────

/// Returns [L, C, h_deg, alpha].
fn srgb_to_oklch(c: [f32; 4]) -> [f32; 4] {
    let oklab = srgb_to_oklab(c);
    let l = oklab[0];
    let a = oklab[1];
    let b = oklab[2];
    let cap_c = (a * a + b * b).sqrt();
    let h = b.atan2(a).to_degrees().rem_euclid(360.0);
    [l, cap_c, h, oklab[3]]
}

/// Takes [L, C, h_deg, alpha] and returns sRGB.
fn oklch_to_srgb(c: [f32; 4]) -> [f32; 4] {
    let l = c[0];
    let cap_c = c[1];
    let h = c[2].to_radians();
    let a = cap_c * h.cos();
    let b = cap_c * h.sin();
    oklab_to_srgb([l, a, b, c[3]])
}

// ─── Relative color origin channels (CSS Color L5 §4.1) ──────────────────────

/// CSS Color L5 §4.1 — channel values of a relative-color origin color.
///
/// Given a straight (non-premultiplied) sRGB color `[r, g, b, a]` with each
/// component in `[0.0, 1.0]`, returns the four channel values
/// `[c0, c1, c2, alpha]` in the CSS-canonical units of `space`, matching what
/// the relative-color channel keywords resolve to:
///
/// | space   | c0          | c1        | c2        | alpha   |
/// |---------|-------------|-----------|-----------|---------|
/// | `Srgb`  | r·255       | g·255     | b·255     | a (0–1) |
/// | `Hsl`   | h (deg)     | s (0–100) | l (0–100) | a (0–1) |
/// | `Lab`   | L (0–100)   | a         | b         | a (0–1) |
/// | `Lch`   | L (0–100)   | C         | h (deg)   | a (0–1) |
/// | `Oklab` | L (0–1)     | a         | b         | a (0–1) |
/// | `Oklch` | L (0–1)     | C         | h (deg)   | a (0–1) |
///
/// Spaces other than these six are not valid base functions for relative color
/// in Lumen and return the sRGB input unchanged.
#[must_use]
pub fn relative_origin_channels(space: MixColorSpace, srgb: [f32; 4]) -> [f32; 4] {
    match space {
        MixColorSpace::Srgb => [srgb[0] * 255.0, srgb[1] * 255.0, srgb[2] * 255.0, srgb[3]],
        MixColorSpace::Hsl => {
            let h = srgb_to_hsl(srgb);
            [h[0], h[1] * 100.0, h[2] * 100.0, h[3]]
        }
        MixColorSpace::Lab => srgb_to_lab(srgb),
        MixColorSpace::Lch => srgb_to_lch(srgb),
        MixColorSpace::Oklab => srgb_to_oklab(srgb),
        MixColorSpace::Oklch => srgb_to_oklch(srgb),
        _ => srgb,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.005
    }
    fn approx_eq4(a: [f32; 4], b: [f32; 4]) -> bool {
        approx_eq(a[0], b[0]) && approx_eq(a[1], b[1]) && approx_eq(a[2], b[2]) && approx_eq(a[3], b[3])
    }

    // ── MixColorSpace::from_css ──────────────────────────────────────────────

    #[test]
    fn parse_srgb_space() {
        assert_eq!(MixColorSpace::from_css("srgb"), Some(MixColorSpace::Srgb));
        assert_eq!(MixColorSpace::from_css("SRGB"), Some(MixColorSpace::Srgb));
    }

    #[test]
    fn parse_oklch_space() {
        assert_eq!(MixColorSpace::from_css("oklch"), Some(MixColorSpace::Oklch));
    }

    #[test]
    fn parse_xyz_alias() {
        assert_eq!(MixColorSpace::from_css("xyz"), Some(MixColorSpace::XyzD65));
        assert_eq!(MixColorSpace::from_css("xyz-d65"), Some(MixColorSpace::XyzD65));
        assert_eq!(MixColorSpace::from_css("xyz-d50"), Some(MixColorSpace::XyzD50));
    }

    #[test]
    fn parse_unknown_space_is_none() {
        assert_eq!(MixColorSpace::from_css("display-p3"), None);
        assert_eq!(MixColorSpace::from_css(""), None);
    }

    #[test]
    fn polar_spaces_reported_correctly() {
        assert!(MixColorSpace::Hsl.is_polar());
        assert!(MixColorSpace::Hwb.is_polar());
        assert!(MixColorSpace::Lch.is_polar());
        assert!(MixColorSpace::Oklch.is_polar());
        assert!(!MixColorSpace::Srgb.is_polar());
        assert!(!MixColorSpace::Oklab.is_polar());
        assert!(!MixColorSpace::Lab.is_polar());
    }

    // ── mix_colors: sRGB ─────────────────────────────────────────────────────

    #[test]
    fn mix_red_blue_50_50_srgb() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Srgb, red, 0.5, blue, 0.5);
        // sRGB mix: (0.5, 0, 0.5, 1)
        assert!(approx_eq(result[0], 0.5), "r: {}", result[0]);
        assert!(approx_eq(result[1], 0.0), "g: {}", result[1]);
        assert!(approx_eq(result[2], 0.5), "b: {}", result[2]);
        assert!(approx_eq(result[3], 1.0), "a: {}", result[3]);
    }

    #[test]
    fn mix_white_black_50_srgb() {
        let white = [1.0_f32; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let result = mix_colors(MixColorSpace::Srgb, white, 0.5, black, 0.5);
        assert!(approx_eq4(result, [0.5, 0.5, 0.5, 1.0]));
    }

    #[test]
    fn mix_100pct_c1_returns_c1() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Srgb, red, 1.0, blue, 0.0);
        assert!(approx_eq4(result, red));
    }

    #[test]
    fn mix_100pct_c2_returns_c2() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Srgb, red, 0.0, blue, 1.0);
        assert!(approx_eq4(result, blue));
    }

    #[test]
    fn mix_zero_weights_returns_transparent() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Srgb, red, 0.0, blue, 0.0);
        assert!(approx_eq(result[3], 0.0));
    }

    // ── mix_colors: sRGB-linear ──────────────────────────────────────────────

    #[test]
    fn mix_red_blue_linear() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        // Linear interpolation preserves energy (brighter midpoint than sRGB gamma mix).
        let result = mix_colors(MixColorSpace::SrgbLinear, red, 0.5, blue, 0.5);
        assert!(result[0] > 0.4, "r should be from linear sRGB mix");
        assert!(approx_eq(result[1], 0.0));
        assert!(result[2] > 0.4, "b should be from linear sRGB mix");
        assert!(approx_eq(result[3], 1.0));
    }

    // ── mix_colors: oklch ────────────────────────────────────────────────────

    #[test]
    fn mix_red_blue_oklch_50_50() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Oklch, red, 0.5, blue, 0.5);
        // Oklch mix goes through the hue arc; result should be a visible color.
        assert!(result[0] >= 0.0 && result[0] <= 1.0);
        assert!(result[3] >= 0.99);
    }

    #[test]
    fn mix_same_color_oklch_returns_same() {
        let c = [0.5_f32, 0.3, 0.7, 1.0];
        let result = mix_colors(MixColorSpace::Oklch, c, 0.5, c, 0.5);
        assert!(approx_eq4(result, c));
    }

    // ── mix_colors: oklab ────────────────────────────────────────────────────

    #[test]
    fn mix_white_black_oklab() {
        let white = [1.0_f32; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let result = mix_colors(MixColorSpace::Oklab, white, 0.5, black, 0.5);
        // Oklab L=0.5 midpoint maps to sRGB ≈ 0.389 (perceptual, darker than 0.5).
        assert!(result[0] > 0.33 && result[0] < 0.46, "r: {}", result[0]);
        assert!(approx_eq(result[3], 1.0));
    }

    // ── mix_colors: lab ──────────────────────────────────────────────────────

    #[test]
    fn mix_white_black_lab() {
        let white = [1.0_f32; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let result = mix_colors(MixColorSpace::Lab, white, 0.5, black, 0.5);
        // Lab midpoint should be a medium grey.
        let mid = (result[0] + result[1] + result[2]) / 3.0;
        assert!(mid > 0.3 && mid < 0.7, "mid grey: {}", mid);
    }

    // ── mix_colors: xyz ──────────────────────────────────────────────────────

    #[test]
    fn mix_white_black_xyz_d65() {
        let white = [1.0_f32; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let result = mix_colors(MixColorSpace::XyzD65, white, 0.5, black, 0.5);
        let mid = (result[0] + result[1] + result[2]) / 3.0;
        // XYZ is linear: midpoint is 0.5 in linear light → gamma-encode → ≈ 0.735 in sRGB.
        assert!(mid > 0.6 && mid < 0.8, "xyz-d65 mid grey: {}", mid);
    }

    #[test]
    fn mix_white_black_xyz_d50() {
        let white = [1.0_f32; 4];
        let black = [0.0, 0.0, 0.0, 1.0];
        let result = mix_colors(MixColorSpace::XyzD50, white, 0.5, black, 0.5);
        let mid = (result[0] + result[1] + result[2]) / 3.0;
        // XYZ D50 same as D65: linear midpoint → gamma-encode → ≈ 0.735.
        assert!(mid > 0.6 && mid < 0.8, "xyz-d50 mid grey: {}", mid);
    }

    // ── mix_colors: hsl ──────────────────────────────────────────────────────

    #[test]
    fn mix_red_blue_hsl() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Hsl, red, 0.5, blue, 0.5);
        // HSL: hue interpolates around the circle — result is a saturated color.
        assert!(result[0] + result[1] + result[2] > 0.5, "should not be black");
        assert!(approx_eq(result[3], 1.0));
    }

    // ── mix_colors: hwb ──────────────────────────────────────────────────────

    #[test]
    fn mix_red_blue_hwb() {
        let red = [1.0_f32, 0.0, 0.0, 1.0];
        let blue = [0.0_f32, 0.0, 1.0, 1.0];
        let result = mix_colors(MixColorSpace::Hwb, red, 0.5, blue, 0.5);
        assert!(result[0] + result[1] + result[2] > 0.5);
        assert!(approx_eq(result[3], 1.0));
    }

    // ── Alpha mixing ─────────────────────────────────────────────────────────

    #[test]
    fn alpha_interpolates_linearly() {
        let c1 = [1.0_f32, 0.0, 0.0, 1.0];
        let c2 = [0.0_f32, 0.0, 1.0, 0.0];
        let result = mix_colors(MixColorSpace::Srgb, c1, 0.5, c2, 0.5);
        assert!(approx_eq(result[3], 0.5), "alpha: {}", result[3]);
    }

    // ── Round-trip conversions ────────────────────────────────────────────────

    #[test]
    fn srgb_oklab_roundtrip() {
        let c = [0.6_f32, 0.2, 0.8, 1.0];
        let lab = srgb_to_oklab(c);
        let back = oklab_to_srgb(lab);
        assert!(approx_eq4(back, c), "roundtrip: {:?} → {:?} → {:?}", c, lab, back);
    }

    #[test]
    fn srgb_lab_roundtrip() {
        let c = [0.3_f32, 0.6, 0.1, 1.0];
        let lab = srgb_to_lab(c);
        let back = lab_to_srgb(lab);
        assert!(approx_eq4(back, c), "lab roundtrip: {:?}", back);
    }

    #[test]
    fn srgb_lch_roundtrip() {
        let c = [0.8_f32, 0.1, 0.3, 1.0];
        let lch = srgb_to_lch(c);
        let back = lch_to_srgb(lch);
        assert!(approx_eq4(back, c), "lch roundtrip: {:?}", back);
    }

    #[test]
    fn srgb_oklch_roundtrip() {
        let c = [0.4_f32, 0.7, 0.2, 1.0];
        let oklch = srgb_to_oklch(c);
        let back = oklch_to_srgb(oklch);
        assert!(approx_eq4(back, c), "oklch roundtrip: {:?}", back);
    }

    #[test]
    fn srgb_hsl_roundtrip() {
        let c = [0.9_f32, 0.3, 0.5, 1.0];
        let hsl = srgb_to_hsl(c);
        let back = hsl_to_srgb(hsl);
        assert!(approx_eq4(back, c), "hsl roundtrip: {:?}", back);
    }
}
