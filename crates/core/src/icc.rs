//! Read-only parser for ICC colour profiles (ICC.1:2010 / v2 + v4).
//!
//! This is the foundation of the Lumen colour-management module (slice ICC-1).
//! It parses the 128-byte profile header, the tag table and the subset of tags
//! that the RGB matrix-shaper and CMYK LUT paths need downstream:
//!
//! * `rXYZ` / `gXYZ` / `bXYZ` — RGB colorant primaries (PCS XYZ).
//! * `rTRC` / `gTRC` / `bTRC` — per-channel tone-reproduction curves.
//! * `wtpt` — media white point.
//! * `A2B0` / `B2A0` — device↔PCS lookup tables (raw bytes, parsed by ICC-4).
//!
//! ICC-1 does **not** evaluate curves or apply transforms — it only produces a
//! structured, owned [`IccProfile`]. Curve evaluation, the PCS (Lab/XYZ) maths
//! and the matrix/LUT transforms live in later slices (ICC-2…ICC-4).
//!
//! The parser is deliberately allocation-light and never panics: every accessor
//! is bounds-checked and malformed input yields `None` rather than an error.

/// Profile/device class (header bytes 12–15).
///
/// Identifies the role of the profile in a colour-managed workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileClass {
    /// `'scnr'` — input device (scanner, camera).
    Input,
    /// `'mntr'` — display device (monitor).
    Display,
    /// `'prtr'` — output device (printer).
    Output,
    /// `'link'` — device link.
    DeviceLink,
    /// `'spac'` — colour-space conversion.
    ColorSpace,
    /// `'abst'` — abstract profile.
    Abstract,
    /// `'nmcl'` — named-colour profile.
    NamedColor,
    /// Unrecognised class signature (raw big-endian value).
    Unknown(u32),
}

impl ProfileClass {
    fn from_sig(sig: u32) -> Self {
        match sig {
            0x73636E72 => ProfileClass::Input,      // 'scnr'
            0x6D6E7472 => ProfileClass::Display,     // 'mntr'
            0x70727472 => ProfileClass::Output,      // 'prtr'
            0x6C696E6B => ProfileClass::DeviceLink,   // 'link'
            0x73706163 => ProfileClass::ColorSpace,   // 'spac'
            0x61627374 => ProfileClass::Abstract,     // 'abst'
            0x6E6D636C => ProfileClass::NamedColor,    // 'nmcl'
            other => ProfileClass::Unknown(other),
        }
    }
}

/// Colour space of profile data or of the PCS (header bytes 16–19 and 20–23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataColorSpace {
    /// `'XYZ '` — CIE XYZ (used as a PCS).
    Xyz,
    /// `'Lab '` — CIE L*a*b* (used as a PCS).
    Lab,
    /// `'RGB '` — three-channel RGB.
    Rgb,
    /// `'GRAY'` — single-channel greyscale.
    Gray,
    /// `'CMYK'` — four-channel CMYK.
    Cmyk,
    /// Any other colour-space signature (raw big-endian value).
    Unknown(u32),
}

impl DataColorSpace {
    fn from_sig(sig: u32) -> Self {
        match sig {
            0x58595A20 => DataColorSpace::Xyz,  // 'XYZ '
            0x4C616220 => DataColorSpace::Lab,  // 'Lab '
            0x52474220 => DataColorSpace::Rgb,  // 'RGB '
            0x47524159 => DataColorSpace::Gray, // 'GRAY'
            0x434D594B => DataColorSpace::Cmyk, // 'CMYK'
            other => DataColorSpace::Unknown(other),
        }
    }

    /// Number of channels for this colour space, or `None` if unknown.
    pub fn channels(&self) -> Option<usize> {
        match self {
            DataColorSpace::Xyz | DataColorSpace::Lab | DataColorSpace::Rgb => Some(3),
            DataColorSpace::Gray => Some(1),
            DataColorSpace::Cmyk => Some(4),
            DataColorSpace::Unknown(_) => None,
        }
    }
}

/// A tristimulus value in the PCS (parsed from an `XYZType` tag).
///
/// Stored as `f64` after decoding the on-disk `s15Fixed16` representation.
/// For ICC the PCS white reference is D50.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XyzNumber {
    /// CIE X tristimulus.
    pub x: f64,
    /// CIE Y tristimulus (luminance).
    pub y: f64,
    /// CIE Z tristimulus.
    pub z: f64,
}

/// A tone-reproduction curve (`curveType` `'curv'` or `parametricCurveType` `'para'`).
///
/// ICC-1 only *parses* the curve; evaluation lives in ICC-3.
#[derive(Debug, Clone, PartialEq)]
pub enum ToneCurve {
    /// Empty `curv` tag — the identity transfer function (linear, gamma 1.0).
    Identity,
    /// `curv` tag with a single entry — a pure gamma, decoded from `u8Fixed8`.
    Gamma(f64),
    /// `curv` tag with N≥2 entries — a sampled 1-D LUT of `u16` values
    /// (input and output normalised to `[0, 1]`).
    Table(Vec<u16>),
    /// `para` tag — a parametric curve. `function` is the ICC function type
    /// (0–4); `params` holds the decoded `s15Fixed16` parameters in spec order
    /// (`g`, then `a b c d e f` as required by the function type).
    Parametric {
        /// Parametric function type (0=g, 1=gab, 2=gabc, 3=gabcd, 4=gabcdef).
        function: u16,
        /// Decoded parameters in spec order.
        params: Vec<f64>,
    },
}

/// A parsed ICC profile (read-only, owned).
///
/// Produced by [`IccProfile::parse`]. Fields that were absent in the source
/// profile are `None`. Raw bytes are retained only for the multi-dimensional
/// `A2B0`/`B2A0` tags, which later slices (ICC-4) interpret.
#[derive(Debug, Clone, PartialEq)]
pub struct IccProfile {
    /// Profile version `(major, minor, bugfix)` decoded from header bytes 8–11.
    pub version: (u8, u8, u8),
    /// Profile/device class (header bytes 12–15).
    pub class: ProfileClass,
    /// Device data colour space (header bytes 16–19).
    pub data_color_space: DataColorSpace,
    /// Profile connection space (header bytes 20–23): `Xyz` or `Lab`.
    pub pcs: DataColorSpace,
    /// Red colorant (`rXYZ`), PCS XYZ.
    pub red_xyz: Option<XyzNumber>,
    /// Green colorant (`gXYZ`), PCS XYZ.
    pub green_xyz: Option<XyzNumber>,
    /// Blue colorant (`bXYZ`), PCS XYZ.
    pub blue_xyz: Option<XyzNumber>,
    /// Media white point (`wtpt`), PCS XYZ.
    pub white_point: Option<XyzNumber>,
    /// Red tone curve (`rTRC`).
    pub red_trc: Option<ToneCurve>,
    /// Green tone curve (`gTRC`).
    pub green_trc: Option<ToneCurve>,
    /// Blue tone curve (`bTRC`).
    pub blue_trc: Option<ToneCurve>,
    /// Raw bytes of the `A2B0` (device→PCS) tag, if present.
    pub a2b0: Option<Vec<u8>>,
    /// Raw bytes of the `B2A0` (PCS→device) tag, if present.
    pub b2a0: Option<Vec<u8>>,
}

impl IccProfile {
    /// Parses an ICC profile from raw bytes.
    ///
    /// Returns `None` if the buffer is too short, lacks the `'acsp'` signature,
    /// or has a structurally invalid tag table. Individual missing/garbled tags
    /// do not fail the whole parse — they simply leave their field `None`.
    pub fn parse(data: &[u8]) -> Option<IccProfile> {
        // Header is 128 bytes; tag count follows immediately at offset 128.
        if data.len() < 132 {
            return None;
        }
        // 'acsp' profile-file signature at bytes 36–39 is mandatory.
        if read_be_u32(data, 36) != 0x61637370 {
            return None;
        }

        let version = {
            let major = data[8];
            let minor = data[9] >> 4;
            let bugfix = data[9] & 0x0F;
            (major, minor, bugfix)
        };
        let class = ProfileClass::from_sig(read_be_u32(data, 12));
        let data_color_space = DataColorSpace::from_sig(read_be_u32(data, 16));
        let pcs = DataColorSpace::from_sig(read_be_u32(data, 20));

        let tag_count = read_be_u32(data, 128) as usize;
        // Each tag table entry is 12 bytes; reject absurd counts that overrun.
        let table_end = 132usize.checked_add(tag_count.checked_mul(12)?)?;
        if table_end > data.len() {
            return None;
        }

        let mut profile = IccProfile {
            version,
            class,
            data_color_space,
            pcs,
            red_xyz: None,
            green_xyz: None,
            blue_xyz: None,
            white_point: None,
            red_trc: None,
            green_trc: None,
            blue_trc: None,
            a2b0: None,
            b2a0: None,
        };

        for i in 0..tag_count {
            let entry = 132 + i * 12;
            let sig = read_be_u32(data, entry);
            let offset = read_be_u32(data, entry + 4) as usize;
            let size = read_be_u32(data, entry + 8) as usize;
            let Some(end) = offset.checked_add(size) else {
                continue;
            };
            if end > data.len() || size < 8 {
                continue;
            }
            let tag = &data[offset..end];

            match sig {
                0x7258595A => profile.red_xyz = parse_xyz(tag),    // 'rXYZ'
                0x6758595A => profile.green_xyz = parse_xyz(tag),  // 'gXYZ'
                0x6258595A => profile.blue_xyz = parse_xyz(tag),   // 'bXYZ'
                0x77747074 => profile.white_point = parse_xyz(tag), // 'wtpt'
                0x72545243 => profile.red_trc = parse_curve(tag),  // 'rTRC'
                0x67545243 => profile.green_trc = parse_curve(tag), // 'gTRC'
                0x62545243 => profile.blue_trc = parse_curve(tag),  // 'bTRC'
                0x41324230 => profile.a2b0 = Some(tag.to_vec()),    // 'A2B0'
                0x42324130 => profile.b2a0 = Some(tag.to_vec()),    // 'B2A0'
                _ => {}
            }
        }

        Some(profile)
    }

    /// Maps the profile to one of Lumen's known [`crate::ColorSpace`] variants.
    ///
    /// For RGB profiles the decision is made from the parsed colorant primaries
    /// (`rXYZ`/`gXYZ`/`bXYZ`), not by sniffing the description string: the
    /// primaries are converted to xy chromaticities and matched against the
    /// sRGB, Display-P3 and Rec.2020 reference gamuts. Non-RGB or colorant-less
    /// profiles fall back to sRGB.
    pub fn color_space(&self) -> crate::ColorSpace {
        use crate::ColorSpace;
        if self.data_color_space != DataColorSpace::Rgb {
            return ColorSpace::Srgb;
        }
        let (Some(r), Some(g), Some(b)) = (self.red_xyz, self.green_xyz, self.blue_xyz) else {
            return ColorSpace::Srgb;
        };
        let rp = xyz_to_xy(r);
        let gp = xyz_to_xy(g);
        let bp = xyz_to_xy(b);
        let (Some(rp), Some(gp), Some(bp)) = (rp, gp, bp) else {
            return ColorSpace::Srgb;
        };

        // Reference primary chromaticities (CIE xy) for each candidate gamut.
        // [red, green, blue].
        const SRGB: [(f64, f64); 3] = [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)];
        const P3: [(f64, f64); 3] = [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)];
        const REC2020: [(f64, f64); 3] = [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)];

        let measured = [rp, gp, bp];
        let dist = |reference: &[(f64, f64); 3]| -> f64 {
            measured
                .iter()
                .zip(reference.iter())
                .map(|(&(mx, my), &(rx, ry))| (mx - rx).powi(2) + (my - ry).powi(2))
                .sum()
        };

        let d_srgb = dist(&SRGB);
        let d_p3 = dist(&P3);
        let d_rec = dist(&REC2020);

        if d_srgb <= d_p3 && d_srgb <= d_rec {
            ColorSpace::Srgb
        } else if d_p3 <= d_rec {
            ColorSpace::DisplayP3
        } else {
            ColorSpace::Rec2020
        }
    }
}

/// Converts a PCS XYZ tristimulus to CIE xy chromaticity, or `None` if the
/// tristimulus sum is non-positive (degenerate).
fn xyz_to_xy(c: XyzNumber) -> Option<(f64, f64)> {
    let sum = c.x + c.y + c.z;
    if sum <= 0.0 {
        return None;
    }
    Some((c.x / sum, c.y / sum))
}

/// Parses an `XYZType` (`'XYZ '` ... three `s15Fixed16` numbers).
fn parse_xyz(tag: &[u8]) -> Option<XyzNumber> {
    // 4 type sig + 4 reserved + 3×4 numbers = 20 bytes minimum.
    if tag.len() < 20 || read_be_u32(tag, 0) != 0x58595A20 {
        return None;
    }
    Some(XyzNumber {
        x: read_s15fixed16(tag, 8),
        y: read_s15fixed16(tag, 12),
        z: read_s15fixed16(tag, 16),
    })
}

/// Parses a `curveType` (`'curv'`) or `parametricCurveType` (`'para'`) tag.
fn parse_curve(tag: &[u8]) -> Option<ToneCurve> {
    match read_be_u32(tag, 0) {
        0x63757276 => parse_curv(tag), // 'curv'
        0x70617261 => parse_para(tag), // 'para'
        _ => None,
    }
}

/// Parses a `curveType` body: a `u32` count followed by `count` `u16` entries.
fn parse_curv(tag: &[u8]) -> Option<ToneCurve> {
    if tag.len() < 12 {
        return None;
    }
    let count = read_be_u32(tag, 8) as usize;
    match count {
        0 => Some(ToneCurve::Identity),
        1 => {
            // Single entry: a u8Fixed8Number gamma value.
            let raw = read_be_u16(tag, 12)?;
            Some(ToneCurve::Gamma(f64::from(raw) / 256.0))
        }
        _ => {
            let end = 12usize.checked_add(count.checked_mul(2)?)?;
            if end > tag.len() {
                return None;
            }
            let mut table = Vec::with_capacity(count);
            for i in 0..count {
                table.push(read_be_u16(tag, 12 + i * 2)?);
            }
            Some(ToneCurve::Table(table))
        }
    }
}

/// Parses a `parametricCurveType` body: a `u16` function type, 2 reserved
/// bytes, then the `s15Fixed16` parameters required by that function type.
fn parse_para(tag: &[u8]) -> Option<ToneCurve> {
    if tag.len() < 12 {
        return None;
    }
    let function = read_be_u16(tag, 8)?;
    // Parameter counts per ICC function type 0..=4: g; gab; gabc; gabcd; gabcdef.
    let param_count = match function {
        0 => 1,
        1 => 3,
        2 => 4,
        3 => 5,
        4 => 7,
        _ => return None,
    };
    let end = 12usize.checked_add(param_count * 4)?;
    if end > tag.len() {
        return None;
    }
    let mut params = Vec::with_capacity(param_count);
    for i in 0..param_count {
        params.push(read_s15fixed16(tag, 12 + i * 4));
    }
    Some(ToneCurve::Parametric { function, params })
}

/// Reads a big-endian `u32` at `offset`, or `0` if out of bounds.
fn read_be_u32(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

/// Reads a big-endian `u16` at `offset`, or `None` if out of bounds.
fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }
    Some(u16::from_be_bytes([data[offset], data[offset + 1]]))
}

/// Reads an ICC `s15Fixed16Number` (signed 16.16 fixed point) at `offset` as
/// `f64`, or `0.0` if out of bounds.
fn read_s15fixed16(data: &[u8], offset: usize) -> f64 {
    if offset + 4 > data.len() {
        return 0.0;
    }
    let raw = i32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
    f64::from(raw) / 65536.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal but structurally valid RGB matrix-shaper profile with
    /// the given colorant primaries (each `(X, Y, Z)`) and a single shared
    /// gamma TRC. Mirrors the on-disk ICC v4 layout closely enough to exercise
    /// the real parser.
    fn build_rgb_profile(
        r: (f64, f64, f64),
        g: (f64, f64, f64),
        b: (f64, f64, f64),
    ) -> Vec<u8> {
        fn push_be_u32(v: &mut Vec<u8>, x: u32) {
            v.extend_from_slice(&x.to_be_bytes());
        }
        fn push_s15(v: &mut Vec<u8>, x: f64) {
            let raw = (x * 65536.0).round() as i32;
            v.extend_from_slice(&raw.to_be_bytes());
        }

        // Tag data blobs.
        let xyz_tag = |c: (f64, f64, f64)| {
            let mut t = Vec::new();
            push_be_u32(&mut t, 0x58595A20); // 'XYZ '
            push_be_u32(&mut t, 0); // reserved
            push_s15(&mut t, c.0);
            push_s15(&mut t, c.1);
            push_s15(&mut t, c.2);
            t
        };
        let r_blob = xyz_tag(r);
        let g_blob = xyz_tag(g);
        let b_blob = xyz_tag(b);
        let wtpt_blob = xyz_tag((0.9642, 1.0, 0.8249)); // D50

        // gamma 2.2 curv tag (single entry, u8Fixed8 → 2.2 ≈ 0x0233).
        let mut trc_blob = Vec::new();
        push_be_u32(&mut trc_blob, 0x63757276); // 'curv'
        push_be_u32(&mut trc_blob, 0); // reserved
        push_be_u32(&mut trc_blob, 1); // count
        trc_blob.extend_from_slice(&((2.2f64 * 256.0).round() as u16).to_be_bytes());

        let tags: [(u32, &[u8]); 8] = [
            (0x7258595A, &r_blob),     // rXYZ
            (0x6758595A, &g_blob),     // gXYZ
            (0x6258595A, &b_blob),     // bXYZ
            (0x77747074, &wtpt_blob),  // wtpt
            (0x72545243, &trc_blob),   // rTRC
            (0x67545243, &trc_blob),   // gTRC
            (0x62545243, &trc_blob),   // bTRC
            (0x41324230, &trc_blob),   // A2B0 (reuse blob as raw bytes)
        ];

        let mut header = vec![0u8; 128];
        header[8] = 4; // version major 4
        header[9] = 0x30; // minor 3, bugfix 0
        header[12..16].copy_from_slice(&0x6D6E7472u32.to_be_bytes()); // 'mntr'
        header[16..20].copy_from_slice(&0x52474220u32.to_be_bytes()); // 'RGB '
        header[20..24].copy_from_slice(&0x58595A20u32.to_be_bytes()); // 'XYZ '
        header[36..40].copy_from_slice(&0x61637370u32.to_be_bytes()); // 'acsp'

        // Tag table directory + data section.
        let tag_count = tags.len();
        let table_start = 132;
        let data_start = table_start + tag_count * 12;
        let mut directory = Vec::new();
        let mut blob_section = Vec::new();
        let mut cursor = data_start;
        for (sig, blob) in tags.iter() {
            push_be_u32(&mut directory, *sig);
            push_be_u32(&mut directory, cursor as u32);
            push_be_u32(&mut directory, blob.len() as u32);
            blob_section.extend_from_slice(blob);
            cursor += blob.len();
        }

        let mut out = header;
        out.extend_from_slice(&(tag_count as u32).to_be_bytes());
        out.extend_from_slice(&directory);
        out.extend_from_slice(&blob_section);
        out
    }

    #[test]
    fn rejects_too_short() {
        assert!(IccProfile::parse(&[0u8; 64]).is_none());
    }

    #[test]
    fn rejects_missing_acsp() {
        let mut data = vec![0u8; 200];
        data[128] = 0; // zero tags but missing 'acsp'
        assert!(IccProfile::parse(&data).is_none());
    }

    #[test]
    fn parses_header_fields() {
        // sRGB-ish primaries.
        let data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        let p = IccProfile::parse(&data).expect("parse");
        assert_eq!(p.version, (4, 3, 0));
        assert_eq!(p.class, ProfileClass::Display);
        assert_eq!(p.data_color_space, DataColorSpace::Rgb);
        assert_eq!(p.pcs, DataColorSpace::Xyz);
    }

    #[test]
    fn parses_colorants_and_white_point() {
        let data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        let p = IccProfile::parse(&data).expect("parse");
        let r = p.red_xyz.expect("rXYZ");
        assert!((r.x - 0.4361).abs() < 1e-3);
        assert!((r.y - 0.2225).abs() < 1e-3);
        let w = p.white_point.expect("wtpt");
        assert!((w.x - 0.9642).abs() < 1e-3);
        assert!((w.y - 1.0).abs() < 1e-3);
    }

    #[test]
    fn parses_trc_gamma() {
        let data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        let p = IccProfile::parse(&data).expect("parse");
        match p.red_trc.expect("rTRC") {
            ToneCurve::Gamma(g) => assert!((g - 2.2).abs() < 0.01),
            other => panic!("expected gamma, got {other:?}"),
        }
    }

    #[test]
    fn retains_a2b0_bytes() {
        let data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        let p = IccProfile::parse(&data).expect("parse");
        assert!(p.a2b0.is_some());
        assert!(p.b2a0.is_none());
    }

    #[test]
    fn classifies_srgb_from_primaries() {
        // sRGB colorants (Bradford-adapted to D50, as a real sRGB profile stores).
        let data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        let p = IccProfile::parse(&data).expect("parse");
        assert_eq!(p.color_space(), crate::ColorSpace::Srgb);
    }

    #[test]
    fn classifies_display_p3_from_primaries() {
        // Display-P3 colorants (D50-adapted).
        let data = build_rgb_profile(
            (0.5151, 0.2412, -0.0011),
            (0.2920, 0.6922, 0.0419),
            (0.1571, 0.0666, 0.7841),
        );
        let p = IccProfile::parse(&data).expect("parse");
        assert_eq!(p.color_space(), crate::ColorSpace::DisplayP3);
    }

    #[test]
    fn classifies_rec2020_from_primaries() {
        // Rec.2020 colorants (D50-adapted, approximate).
        let data = build_rgb_profile(
            (0.6734, 0.2790, -0.0019),
            (0.1656, 0.6757, 0.0299),
            (0.1251, 0.0453, 0.7969),
        );
        let p = IccProfile::parse(&data).expect("parse");
        assert_eq!(p.color_space(), crate::ColorSpace::Rec2020);
    }

    #[test]
    fn non_rgb_falls_back_to_srgb() {
        let mut data = build_rgb_profile(
            (0.4361, 0.2225, 0.0139),
            (0.3851, 0.7169, 0.0971),
            (0.1431, 0.0606, 0.7141),
        );
        // Patch the data colour space to 'CMYK'.
        data[16..20].copy_from_slice(&0x434D594Bu32.to_be_bytes());
        let p = IccProfile::parse(&data).expect("parse");
        assert_eq!(p.data_color_space, DataColorSpace::Cmyk);
        assert_eq!(p.color_space(), crate::ColorSpace::Srgb);
    }
}
